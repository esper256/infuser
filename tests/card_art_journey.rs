use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        })),
    ))
}
fn art(size: u32, alpha: bool) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let color = if alpha {
        image_webp::ColorType::Rgba8
    } else {
        image_webp::ColorType::Rgb8
    };
    image_webp::WebPEncoder::new(&mut bytes).encode(
        &vec![0; size as usize * size as usize * if alpha { 4 } else { 3 }],
        size,
        size,
        color,
    )?;
    // A valid unknown RIFF chunk makes this larger than the JSON request limit.
    // Cover uploads must retain their independent 250 KiB limit with capture on.
    bytes.extend_from_slice(b"JUNK");
    bytes.extend_from_slice(&(100u32 * 1024).to_le_bytes());
    bytes.resize(bytes.len() + 100 * 1024, 0);
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(bytes)
}
async fn json_ok(request: reqwest::RequestBuilder) -> Result<Value> {
    let response = request.send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
    anyhow::ensure!(status.is_success(), "{status}: {body}");
    Ok(body)
}

#[tokio::test]
async fn card_art_preserves_builds_and_supports_independent_presentation_updates() -> Result<()> {
    let temp = tempfile::tempdir()?;
    // Start with the shipped pre-description schema to exercise an actual upgrade.
    rusqlite::Connection::open(temp.path().join("platform.sqlite"))?.execute_batch(
        "CREATE TABLE game_metadata (app TEXT PRIMARY KEY, title TEXT,
         card_art_revision TEXT, card_art BLOB, updated INTEGER NOT NULL DEFAULT 0);",
    )?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=admin") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "admin@example.com"),
                        ("X-Auth-Request-User", "admin@example.com"),
                    ],
                ),
                Some("member=user") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "user@example.com"),
                        ("X-Auth-Request-User", "user@example.com"),
                    ],
                ),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                ),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64\npublish_capture_mb=16",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    platform::member(&cfg, "user@example.com", "User", false, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let token =
        p.db.edit(|d| Ok(platform::credential(d, "pond", "publish", "Pond")?.1))?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let publisher = |method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .bearer_auth(&token)
    };
    let member = |path: &str, who: &str| {
        http.get(format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
    };
    let caps = json_ok(publisher(Method::GET, "/api/publisher/capabilities")).await?;
    assert_eq!(caps["card_art"]["path"], infuser::card_art::PATH);
    let manifest = json!({"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"});
    for build in ["with-art", "without-art"] {
        let base = format!("/api/publisher/apps/pond/builds/{build}");
        json_ok(publisher(Method::PUT, &base).json(&json!({"manifest":manifest}))).await?;
        json_ok(
            publisher(Method::PUT, &format!("{base}/files/index.html"))
                .body("<script type=module src='/_infuser/sdk.js'></script>"),
        )
        .await?;
    }
    let upload = "/api/publisher/apps/pond/builds/with-art/files/infuser-art/card-hero.webp";
    for invalid in [
        b"<html>not an image</html>".to_vec(),
        art(512, false)?,
        art(1024, true)?,
        vec![0; 256 * 1024],
    ] {
        assert_eq!(
            publisher(Method::PUT, upload)
                .body(invalid)
                .send()
                .await?
                .status(),
            422
        );
    }
    let bytes = art(1024, false)?;
    let mut damaged = bytes.clone();
    damaged.truncate(24); // Cut image data, not just the trailing test chunk.
    assert_eq!(
        publisher(Method::PUT, upload)
            .body(damaged)
            .send()
            .await?
            .status(),
        422
    );
    json_ok(publisher(Method::PUT, upload).body(bytes.clone())).await?;
    json_ok(
        publisher(
            Method::POST,
            "/api/publisher/apps/pond/builds/with-art/publish",
        )
        .json(&json!({"listed":false})),
    )
    .await?;
    let library = json_ok(member("/api/library", "admin")).await?;
    let image_url = library["apps"][0]["card_art_url"].as_str().unwrap();
    assert_eq!(member(image_url, "none").send().await?.status(), 401);
    assert_eq!(member(image_url, "user").send().await?.status(), 404);
    assert_eq!(
        json_ok(member("/api/library", "user")).await?["apps"],
        json!([])
    );
    // Admin visibility changes do not publish, rebuild, or change game data.
    let visibility = |app: &str, who: &str| {
        http.put(format!("{url}/api/admin/apps/{app}/visibility"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    };
    assert_eq!(
        visibility("pond", "user")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        visibility("pond", "none")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.put(format!("{url}/api/admin/apps/pond/visibility"))
            .header("host", "games.example.com")
            .header("cookie", "member=admin")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        visibility("missing", "admin")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        visibility("pond", "admin")
            .json(&json!({"listed":true,"build":"other"}))
            .send()
            .await?
            .status(),
        422
    );
    let before_visibility = p.db.read()?;
    json_ok(visibility("pond", "admin").json(&json!({"listed":true}))).await?;
    assert_eq!(
        json_ok(member("/api/library", "user")).await?["apps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(member(image_url, "user").send().await?.status(), 200);
    json_ok(visibility("pond", "admin").json(&json!({"listed":false}))).await?;
    assert_eq!(
        json_ok(member("/api/library", "user")).await?["apps"],
        json!([])
    );
    assert_eq!(member(image_url, "user").send().await?.status(), 404);
    assert_eq!(
        http.post(format!("{url}/api/apps/pond/launch"))
            .header("host", "games.example.com")
            .header("cookie", "member=user")
            .header("origin", "https://games.example.com")
            .json(&json!({"guest":false}))
            .send()
            .await?
            .status(),
        404
    );
    let after_visibility = p.db.read()?;
    assert_eq!(
        serde_json::to_value(&before_visibility.apps)?,
        serde_json::to_value(&after_visibility.apps)?
    );
    assert_eq!(
        serde_json::to_value(&before_visibility.builds)?,
        serde_json::to_value(&after_visibility.builds)?
    );
    assert_eq!(
        serde_json::to_value(&before_visibility.launches)?,
        serde_json::to_value(&after_visibility.launches)?
    );
    let image = member(image_url, "admin")
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(image.headers()["content-type"], "image/webp");
    assert_eq!(image.headers()["cache-control"], "private, no-cache");
    let etag = image.headers()["etag"].clone();
    assert_eq!(
        member(image_url, "admin")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        304
    );
    assert_eq!(
        member(image_url, "none")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        401
    );

    assert_eq!(image.bytes().await?.as_ref(), bytes);
    assert_eq!(
        publisher(Method::PUT, upload)
            .body(bytes)
            .send()
            .await?
            .status(),
        409
    );
    json_ok(
        publisher(
            Method::POST,
            "/api/publisher/apps/pond/builds/with-art/publish",
        )
        .json(&json!({"listed":true})),
    )
    .await?;
    // Publishing cannot undo an admin hide, and metadata updates still work.
    assert_eq!(member(image_url, "user").send().await?.status(), 404);
    let hidden_metadata = json_ok(publisher(Method::PATCH, "/api/publisher/apps/pond/metadata")
        .json(&json!({"description":"Updated while hidden"}))).await?;
    assert_eq!(hidden_metadata["listed"], false);
    assert_eq!(hidden_metadata["description"], "Updated while hidden");
    json_ok(publisher(Method::PATCH, "/api/publisher/apps/pond/metadata")
        .json(&json!({"description":null}))).await?;
    assert!(platform::Database::open(temp.path())?.read()?.admin_hidden_apps.contains("pond"));
    // A previously queued publication also respects the current admin hide.
    p.db.edit(|d| {
        d.pending_promotions.insert("pond".into(), platform::PendingPromotion {
            build: "without-art".into(), listed: true, queued_at: 1,
        });
        Ok(())
    })?;
    p.activate_pending()?;
    assert_eq!(p.db.read()?.apps["pond"].build, "without-art");
    assert!(!p.db.read()?.apps["pond"].listed);
    assert_eq!(json_ok(member("/api/library", "user")).await?["apps"], json!([]));
    let hidden_library = json_ok(member("/api/library", "admin")).await?;
    assert_eq!(hidden_library["apps"][0]["preview"], true);
    assert_eq!(hidden_library["apps"][0]["can_manage_visibility"], true);
    json_ok(publisher(Method::POST, "/api/publisher/apps/pond/builds/with-art/publish")
        .json(&json!({"listed":true}))).await?;
    assert!(!p.db.read()?.apps["pond"].listed);
    json_ok(visibility("pond", "admin").json(&json!({"listed":true}))).await?;
    assert!(!p.db.read()?.admin_hidden_apps.contains("pond"));
    assert_eq!(json_ok(member("/api/library", "user")).await?["apps"][0]["can_manage_visibility"], false);
    assert_eq!(member(image_url, "user").send().await?.status(), 200);
    json_ok(
        publisher(
            Method::POST,
            "/api/publisher/apps/pond/builds/without-art/publish",
        )
        .json(&json!({"listed":true})),
    )
    .await?;
    assert!(json_ok(member("/api/library", "user")).await?["apps"][0]["card_art_url"].is_null());
    assert_eq!(
        member(image_url, "admin").send().await?.status(),
        404,
        "art cannot bypass the active-build pointer"
    );

    // Presentation edits must not republish the game or alter live sessions/saves.
    assert_eq!(caps["game_metadata"]["version"], 1);
    assert_eq!(caps["game_metadata"]["description_max_chars"], 2000);
    assert_eq!(caps["game_metadata"]["art_prompt_max_chars"], 4000);
    let metadata_path = "/api/publisher/apps/pond/metadata";
    let art_path = "/api/publisher/apps/pond/card-art";
    let before = p.db.read()?;
    p.db.save_write(
        "user@example.com",
        "pond",
        "progress",
        0,
        &json!({"level":7}),
    )?;
    json_ok(
        http.post(format!("{url}/api/apps/pond/launch"))
            .header("host", "games.example.com")
            .header("cookie", "member=user")
            .header("origin", "https://games.example.com")
            .json(&json!({"guest":false})),
    )
    .await?;
    let launches = serde_json::to_value(p.db.read()?.launches)?;
    let other_token =
        p.db.edit(|d| Ok(platform::credential(d, "elsewhere", "publish", "Other")?.1))?;
    for path in [metadata_path, art_path] {
        let method = if path == metadata_path {
            Method::PATCH
        } else {
            Method::PUT
        };
        for token in ["", other_token.as_str()] {
            let request = http
                .request(method.clone(), format!("{url}{path}"))
                .header("host", "games.example.com")
                .bearer_auth(token);
            let request = if path == metadata_path {
                request.json(&json!({"title":"No"}))
            } else {
                request.body(art(1024, false)?)
            };
            assert_eq!(request.send().await?.status(), 401);
        }
    }
    assert_eq!(
        http.get(format!("{url}{metadata_path}"))
            .header("host", "games.example.com")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        json_ok(publisher(Method::GET, metadata_path)).await?["build"],
        "without-art"
    );
    for title in [" ".to_string(), "x".repeat(81), "Bad\nTitle".to_string()] {
        assert_eq!(
            publisher(Method::PATCH, metadata_path)
                .json(&json!({"title":title}))
                .send()
                .await?
                .status(),
            422
        );
    }
    assert_eq!(
        publisher(Method::PATCH, metadata_path)
            .json(&json!({}))
            .send()
            .await?
            .status(),
        400
    );
    assert_eq!(
        publisher(Method::PATCH, metadata_path)
            .json(&json!({"title":"No","listed":false}))
            .send()
            .await?
            .status(),
        422
    );
    for invalid in [b"not an image".to_vec(), art(512, false)?, art(1024, true)?] {
        assert_eq!(
            publisher(Method::PUT, art_path)
                .body(invalid)
                .send()
                .await?
                .status(),
            422
        );
    }
    assert_eq!(
        publisher(Method::PUT, art_path)
            .body(vec![0; infuser::card_art::MAX_BYTES + 1])
            .send()
            .await?
            .status(),
        413
    );
    let updated = json_ok(
        publisher(Method::PATCH, metadata_path).json(&json!({"title":"  Pond After Dark  "})),
    )
    .await?;
    assert_eq!(updated["title"], "Pond After Dark");
    assert!(updated["description"].is_null());
    let described = json_ok(publisher(Method::PATCH, metadata_path).json(&json!({
        "description":"  Explore a moonlit pond and collect fireflies.  "
    })))
    .await?;
    assert_eq!(described["title"], "Pond After Dark");
    assert_eq!(
        described["description"],
        "Explore a moonlit pond and collect fireflies."
    );
    for description in [" ".to_string(), "x".repeat(2001), "bad\ntext".to_string()] {
        assert_eq!(
            publisher(Method::PATCH, metadata_path)
                .json(&json!({"title":"Must not change", "description":description}))
                .send()
                .await?
                .status(),
            422
        );
    }
    let unchanged = json_ok(publisher(Method::GET, metadata_path)).await?;
    assert_eq!(unchanged["title"], described["title"]);
    assert_eq!(unchanged["description"], described["description"]);
    assert_eq!(
        json_ok(member("/api/library", "user")).await?["apps"][0]["description"],
        described["description"]
    );
    let cleared_description =
        json_ok(publisher(Method::PATCH, metadata_path).json(&json!({"description":null}))).await?;
    assert!(cleared_description["description"].is_null());
    assert_eq!(cleared_description["title"], described["title"]);
    json_ok(
        publisher(Method::PATCH, metadata_path)
            .json(&json!({"description":described["description"]})),
    )
    .await?;
    assert_eq!(
        json_ok(member("/api/games/pond", "user")).await?["title"],
        "Pond After Dark"
    );
    // Exercise the actual committed Firefly cover, not just a synthetic WebP.
    let bytes =
        include_bytes!("../docs/design-explorations/card-art-guide/firefly-drift.webp").to_vec();
    let updated = json_ok(
        publisher(Method::PUT, art_path)
            .header("content-type", "image/webp")
            .body(bytes.clone()),
    )
    .await?;
    assert_eq!(updated["title"], "Pond After Dark");
    let override_url = updated["card_art_url"].as_str().unwrap().to_owned();
    let revision = updated["card_art_override"].as_str().unwrap().to_owned();
    let library = json_ok(member("/api/library", "user")).await?;
    assert_eq!(library["apps"][0]["card_art_url"], override_url);
    assert_eq!(library["apps"][0]["title"], "Pond After Dark");
    assert_eq!(member(&override_url, "none").send().await?.status(), 401);
    let image = member(&override_url, "user")
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(image.headers()["cache-control"], "private, no-cache");
    assert_eq!(image.bytes().await?.as_ref(), bytes);
    let after = p.db.read()?;
    assert_eq!(
        serde_json::to_value(&after.apps)?,
        serde_json::to_value(&before.apps)?
    );
    assert_eq!(
        serde_json::to_value(&after.builds)?,
        serde_json::to_value(&before.builds)?
    );
    assert_eq!(
        serde_json::to_value(&after.pending_promotions)?,
        serde_json::to_value(&before.pending_promotions)?
    );
    assert_eq!(serde_json::to_value(&after.launches)?, launches);
    assert_eq!(
        p.db.save_read("user@example.com", "pond", "progress")?,
        (1, json!({"level":7}))
    );
    // Legacy catalog writes and reopening the database retain the independent metadata.
    p.db.edit(|d| {
        d.members.get_mut("user@example.com").unwrap().name = "New handle".into();
        Ok(())
    })?;
    let reopened = platform::Database::open(temp.path())?;
    assert_eq!(
        reopened.read()?.metadata["pond"].title.as_deref(),
        Some("Pond After Dark")
    );
    assert_eq!(reopened.metadata_art("pond", &revision)?.unwrap(), bytes);
    assert_eq!(
        reopened.read()?.metadata["pond"].description.as_deref(),
        Some("Explore a moonlit pond and collect fireflies.")
    );
    let mut replacement = Vec::new();
    image_webp::WebPEncoder::new(&mut replacement).encode(
        &vec![17; 1024 * 1024 * 3],
        1024,
        1024,
        image_webp::ColorType::Rgb8,
    )?;
    let updated = json_ok(publisher(Method::PUT, art_path).body(replacement.clone())).await?;
    let replacement_url = updated["card_art_url"].as_str().unwrap();
    assert_ne!(replacement_url, override_url);
    assert_eq!(member(&override_url, "user").send().await?.status(), 404);
    assert!(reopened.metadata_art("pond", &revision)?.is_none());
    assert_eq!(
        member(replacement_url, "user")
            .send()
            .await?
            .bytes()
            .await?
            .as_ref(),
        replacement
    );
    // Invalid replacements cannot poison the active image.
    assert_eq!(
        publisher(Method::PUT, art_path)
            .body("bad")
            .send()
            .await?
            .status(),
        422
    );
    assert_eq!(
        json_ok(publisher(Method::GET, metadata_path)).await?["card_art_url"],
        replacement_url
    );
    // A normal subsequent release retains explicit presentation overrides.
    json_ok(
        publisher(
            Method::POST,
            "/api/publisher/apps/pond/builds/with-art/publish",
        )
        .json(&json!({"listed":false})),
    )
    .await?;
    assert_eq!(
        json_ok(publisher(Method::GET, metadata_path)).await?["card_art_url"],
        replacement_url
    );
    assert_eq!(member(replacement_url, "user").send().await?.status(), 404);
    assert_eq!(member(replacement_url, "admin").send().await?.status(), 200);
    let cleared = json_ok(publisher(Method::DELETE, art_path)).await?;
    assert_eq!(cleared["title"], "Pond After Dark");
    assert_eq!(cleared["card_art_url"], "/api/apps/pond/card-art/with-art");
    assert_eq!(member(replacement_url, "admin").send().await?.status(), 404);
    let cleared =
        json_ok(publisher(Method::PATCH, metadata_path).json(&json!({"title":null}))).await?;
    assert_eq!(cleared["title"], "Pond");
    assert!(cleared["title_override"].is_null());
    // Disabling the credential's owner and revoking the token stop metadata writes too.
    p.db.edit(|d| {
        for c in d.credentials.values_mut().filter(|c| c.app == "pond") {
            c.owner = Some("user@example.com".into());
        }
        d.members.get_mut("user@example.com").unwrap().enabled = false;
        Ok(())
    })?;
    assert_eq!(
        publisher(Method::PUT, art_path)
            .body(bytes.clone())
            .send()
            .await?
            .status(),
        403
    );
    p.db.edit(|d| {
        d.credentials.retain(|_, c| c.app != "pond");
        Ok(())
    })?;
    assert_eq!(
        publisher(Method::DELETE, art_path).send().await?.status(),
        401
    );

    // A separate service credential can fulfill art requests across games, but
    // cannot publish builds, impersonate players, read saves, or administer users.
    let (service_id, service_token) = p.db.edit(|d| {
        let app = d.apps["pond"].clone();
        let mut build = d.builds[&format!("pond/{}", app.build)].clone();
        build.manifest.app_id = "second".into();
        Arc::make_mut(&mut build.files).remove(infuser::card_art::PATH);
        d.builds.insert(format!("second/{}", app.build), build);
        d.apps.insert("second".into(), app);
        platform::credential(d, "all-games", "metadata", "Infuser art agent")
    })?;
    let game_token =
        p.db.edit(|d| Ok(platform::credential(d, "pond", "publish", "Pond")?.1))?;
    let request = |token: &str, method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .bearer_auth(token)
    };
    for token in ["", game_token.as_str(), other_token.as_str()] {
        assert_eq!(
            request(token, Method::GET, "/api/metadata/apps")
                .send()
                .await?
                .status(),
            401
        );
    }
    assert_eq!(
        request(&game_token, Method::PATCH, "/api/metadata/apps/second")
            .json(&json!({"art_prompt":"Not my game"}))
            .send()
            .await?
            .status(),
        401
    );
    for invalid in [" ".to_string(), "x".repeat(4001), "bad\u{0}prompt".into()] {
        assert_eq!(
            request(&game_token, Method::PATCH, metadata_path)
                .json(&json!({"art_prompt":invalid}))
                .send()
                .await?
                .status(),
            422
        );
    }
    let brief = "Scrappy low-poly arena.\nA giant rubber duck crashes through it; no text.";
    let queued = json_ok(
        request(&game_token, Method::PATCH, metadata_path).json(&json!({"art_prompt":brief})),
    )
    .await?;
    assert_eq!(queued["art_prompt"], brief);
    assert_eq!(queued["art_prompt_pending"], true);
    let revision = queued["art_prompt_revision"].as_str().unwrap();
    assert_eq!(
        platform::Database::open(temp.path())?.read()?.metadata["pond"]
            .art_prompt
            .as_deref(),
        Some(brief)
    );
    let all = json_ok(request(&service_token, Method::GET, "/api/metadata/apps")).await?;
    assert_eq!(all["apps"].as_array().unwrap().len(), 2);
    assert_eq!(all["apps"][0]["art_prompt"], brief);
    let library = json_ok(member("/api/library", "admin")).await?;
    for app in library["apps"].as_array().unwrap() {
        assert!(app.get("art_prompt").is_none());
        assert!(app.get("art_prompt_revision").is_none());
        assert!(app.get("art_prompt_pending").is_none());
        assert_eq!(app["card_art_pending"], false);
    }
    json_ok(
        request(&service_token, Method::PATCH, "/api/metadata/apps/second")
            .json(&json!({"description":"Second game", "art_prompt":"Another brief"})),
    )
    .await?;
    let library = json_ok(member("/api/library", "admin")).await?;
    let pending = library["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "second")
        .unwrap();
    assert_eq!(pending["card_art_pending"], true);
    assert!(pending["card_art_url"].is_null());
    assert!(pending.get("art_prompt").is_none());
    json_ok(
        request(&service_token, Method::PATCH, "/api/metadata/apps/second")
            .json(&json!({"art_prompt":null})),
    )
    .await?;
    let library = json_ok(member("/api/library", "admin")).await?;
    assert_eq!(
        library["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "second")
            .unwrap()["card_art_pending"],
        false
    );
    json_ok(
        request(&service_token, Method::PATCH, "/api/metadata/apps/second")
            .json(&json!({"art_prompt":"Another brief"})),
    )
    .await?;
    let placeholder = http
        .get(format!("{url}/assets/card-generating.webp"))
        .header("host", "games.example.com")
        .send()
        .await?;
    assert_eq!(placeholder.status(), 200);
    assert_eq!(placeholder.headers()["content-type"], "image/webp");
    assert_eq!(
        placeholder.bytes().await?.as_ref(),
        include_bytes!("../assets/card-fallback/generating.webp")
    );
    // A newer request wins over an image generated from an obsolete prompt.
    json_ok(
        request(&game_token, Method::PATCH, metadata_path)
            .json(&json!({"art_prompt":"Changed brief"})),
    )
    .await?;
    assert_eq!(
        request(
            &service_token,
            Method::PUT,
            "/api/metadata/apps/pond/card-art"
        )
        .header("X-Infuser-Art-Prompt-Revision", revision)
        .body(bytes.clone())
        .send()
        .await?
        .status(),
        409
    );
    let latest = json_ok(request(
        &service_token,
        Method::GET,
        "/api/metadata/apps/pond",
    ))
    .await?;
    assert_eq!(latest["card_art_url"], cleared["card_art_url"]);
    let revision = latest["art_prompt_revision"].as_str().unwrap();
    let completed = json_ok(
        request(
            &service_token,
            Method::PUT,
            "/api/metadata/apps/pond/card-art",
        )
        .header("X-Infuser-Art-Prompt-Revision", revision)
        .body(bytes.clone()),
    )
    .await?;
    assert_eq!(completed["art_prompt"], "Changed brief");
    assert_eq!(completed["art_prompt_revision"], revision);
    assert_eq!(completed["art_prompt_pending"], false);
    let retained = platform::Database::open(temp.path())?.read()?.metadata["pond"].clone();
    assert_eq!(retained.art_prompt.as_deref(), Some("Changed brief"));
    assert_eq!(
        retained.art_prompt_completed_revision.as_deref(),
        Some(revision)
    );
    assert_eq!(completed["build"], latest["build"]);
    assert_eq!(completed["description"], latest["description"]);
    assert_eq!(
        request(
            &service_token,
            Method::PUT,
            "/api/metadata/apps/pond/card-art"
        )
        .header("X-Infuser-Art-Prompt-Revision", revision)
        .body(bytes.clone())
        .send()
        .await?
        .status(),
        409
    );
    let second = json_ok(request(
        &service_token,
        Method::GET,
        "/api/metadata/apps/second",
    ))
    .await?;
    json_ok(
        request(
            &service_token,
            Method::PUT,
            "/api/metadata/apps/second/card-art",
        )
        .header(
            "X-Infuser-Art-Prompt-Revision",
            second["art_prompt_revision"].as_str().unwrap(),
        )
        .body(bytes.clone()),
    )
    .await?;
    let library = json_ok(member("/api/library", "admin")).await?;
    let complete = library["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "second")
        .unwrap();
    assert_eq!(complete["card_art_pending"], false);
    assert!(complete["card_art_url"].is_string());
    // Removing a finished cover must not imply that its old prompt is queued again.
    json_ok(request(
        &service_token,
        Method::DELETE,
        "/api/metadata/apps/second/card-art",
    ))
    .await?;
    let library = json_ok(member("/api/library", "admin")).await?;
    let complete = library["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "second")
        .unwrap();
    assert_eq!(complete["card_art_pending"], false);
    assert!(complete["card_art_url"].is_null());
    for (method, path) in [
        (Method::GET, "/api/publisher/apps/pond/builds/with-art"),
        (Method::PUT, "/api/publisher/apps/pond/builds/forbidden"),
        (
            Method::POST,
            "/api/publisher/apps/pond/builds/with-art/publish",
        ),
        (Method::DELETE, "/api/publisher/apps/pond"),
        (Method::GET, "/api/admin/members"),
        (Method::GET, "/_infuser/api/v1/saves/progress"),
    ] {
        let body = if method == Method::PUT {
            json!({"manifest":manifest,"server":null})
        } else {
            json!({})
        };
        let status = request(&service_token, method, path)
            .json(&body)
            .send()
            .await?
            .status();
        assert!(
            status == 401 || status == 403 || status == 404,
            "{path}: {status}"
        );
    }
    p.db.edit(|d| {
        d.groups.operators.insert("admin@example.com".into());
        Ok(())
    })?;
    let credentials = json_ok(member("/api/admin/credentials", "admin")).await?;
    assert!(
        credentials["credentials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == service_id && c["role"] == "metadata")
    );
    json_ok(
        http.delete(format!("{url}/api/admin/credentials/{service_id}"))
            .header("host", "games.example.com")
            .header("cookie", "member=admin")
            .header("origin", "https://games.example.com"),
    )
    .await?;
    assert_eq!(
        request(&service_token, Method::GET, "/api/metadata/apps")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        request(&service_token, Method::PATCH, "/api/metadata/apps/second")
            .json(&json!({"title":"Revoked"}))
            .send()
            .await?
            .status(),
        401
    );
    Ok(())
}
