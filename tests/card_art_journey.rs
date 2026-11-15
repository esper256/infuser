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
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=adult") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "adult@example.com")],
                ),
                Some("member=kid") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "kid@example.com")],
                ),
                _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    platform::member(&cfg, "adult@example.com", "Adult", true, true)?;
    platform::member(&cfg, "kid@example.com", "Kid", false, true)?;
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
    damaged.truncate(damaged.len() / 2);
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
    let library = json_ok(member("/api/library", "adult")).await?;
    let image_url = library["apps"][0]["card_art_url"].as_str().unwrap();
    assert_eq!(member(image_url, "none").send().await?.status(), 401);
    assert_eq!(member(image_url, "kid").send().await?.status(), 404);
    assert_eq!(
        json_ok(member("/api/library", "kid")).await?["apps"],
        json!([])
    );
    // Adult visibility changes do not publish, rebuild, or change game data.
    let visibility = |app: &str, who: &str| {
        http.put(format!("{url}/api/admin/apps/{app}/visibility"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    };
    assert_eq!(
        visibility("pond", "kid")
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
            .header("cookie", "member=adult")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        visibility("missing", "adult")
            .json(&json!({"listed":true}))
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        visibility("pond", "adult")
            .json(&json!({"listed":true,"build":"other"}))
            .send()
            .await?
            .status(),
        422
    );
    let before_visibility = p.db.read()?;
    json_ok(visibility("pond", "adult").json(&json!({"listed":true}))).await?;
    assert_eq!(
        json_ok(member("/api/library", "kid")).await?["apps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(member(image_url, "kid").send().await?.status(), 200);
    json_ok(visibility("pond", "adult").json(&json!({"listed":false}))).await?;
    assert_eq!(
        json_ok(member("/api/library", "kid")).await?["apps"],
        json!([])
    );
    assert_eq!(member(image_url, "kid").send().await?.status(), 404);
    assert_eq!(
        http.post(format!("{url}/api/apps/pond/launch"))
            .header("host", "games.example.com")
            .header("cookie", "member=kid")
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
    let image = member(image_url, "adult")
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(image.headers()["content-type"], "image/webp");
    assert_eq!(image.headers()["cache-control"], "private, no-cache");
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
    assert_eq!(member(image_url, "kid").send().await?.status(), 200);
    json_ok(
        publisher(
            Method::POST,
            "/api/publisher/apps/pond/builds/without-art/publish",
        )
        .json(&json!({"listed":true})),
    )
    .await?;
    assert!(json_ok(member("/api/library", "kid")).await?["apps"][0]["card_art_url"].is_null());
    assert_eq!(
        member(image_url, "adult").send().await?.status(),
        404,
        "art cannot bypass the active-build pointer"
    );

    // Presentation edits must not republish the game or alter live sessions/saves.
    assert_eq!(caps["game_metadata"]["version"], 1);
    let metadata_path = "/api/publisher/apps/pond/metadata";
    let art_path = "/api/publisher/apps/pond/card-art";
    let before = p.db.read()?;
    p.db.save_write(
        "kid@example.com",
        "pond",
        "progress",
        0,
        &json!({"level":7}),
    )?;
    json_ok(
        http.post(format!("{url}/api/apps/pond/launch"))
            .header("host", "games.example.com")
            .header("cookie", "member=kid")
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
    assert_eq!(
        json_ok(member("/api/games/pond", "kid")).await?["title"],
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
    let library = json_ok(member("/api/library", "kid")).await?;
    assert_eq!(library["apps"][0]["card_art_url"], override_url);
    assert_eq!(library["apps"][0]["title"], "Pond After Dark");
    assert_eq!(member(&override_url, "none").send().await?.status(), 401);
    let image = member(&override_url, "kid")
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
        p.db.save_read("kid@example.com", "pond", "progress")?,
        (1, json!({"level":7}))
    );
    // Legacy catalog writes and reopening the database retain the independent metadata.
    p.db.edit(|d| {
        d.members.get_mut("kid@example.com").unwrap().name = "New handle".into();
        Ok(())
    })?;
    let reopened = platform::Database::open(temp.path())?;
    assert_eq!(
        reopened.read()?.metadata["pond"].title.as_deref(),
        Some("Pond After Dark")
    );
    assert_eq!(reopened.metadata_art("pond", &revision)?.unwrap(), bytes);
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
    assert_eq!(member(&override_url, "kid").send().await?.status(), 404);
    assert!(reopened.metadata_art("pond", &revision)?.is_none());
    assert_eq!(
        member(replacement_url, "kid")
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
    assert_eq!(member(replacement_url, "kid").send().await?.status(), 404);
    assert_eq!(member(replacement_url, "adult").send().await?.status(), 200);
    let cleared = json_ok(publisher(Method::DELETE, art_path)).await?;
    assert_eq!(cleared["title"], "Pond After Dark");
    assert_eq!(cleared["card_art_url"], "/api/apps/pond/card-art/with-art");
    assert_eq!(member(replacement_url, "adult").send().await?.status(), 404);
    let cleared =
        json_ok(publisher(Method::PATCH, metadata_path).json(&json!({"title":null}))).await?;
    assert_eq!(cleared["title"], "Pond");
    assert!(cleared["title_override"].is_null());
    // Disabling the credential's owner and revoking the token stop metadata writes too.
    p.db.edit(|d| {
        for c in d.credentials.values_mut().filter(|c| c.app == "pond") {
            c.owner = Some("kid@example.com".into());
        }
        d.members.get_mut("kid@example.com").unwrap().enabled = false;
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
    Ok(())
}
