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
            axum::serve(listener, router).await.unwrap()
        })),
    ))
}

#[tokio::test]
async fn profile_selection_is_private_persistent_and_shared_with_the_library() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let auth = Router::new().route(
        "/oauth2/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=one") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "one@example.com"),
                        ("X-Auth-Request-User", "one@example.com"),
                    ],
                ),
                Some("member=two") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "two@example.com"),
                        ("X-Auth-Request-User", "two@example.com"),
                    ],
                ),
                Some("member=gob") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "gob@example.com"),
                        ("X-Auth-Request-User", "gob@example.com"),
                    ],
                ),
                Some("member=keeper") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "keeper@example.com"),
                        ("X-Auth-Request-User", "keeper@example.com"),
                    ],
                ),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                ),
            }
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nprivate_groups=true\nauth_url={:?}",
        dir.path().display().to_string(),
        format!("{auth_url}/oauth2/auth")
    ))?;
    platform::member(&cfg, "one@example.com", "One", false, true)?;
    platform::member(&cfg, "two@example.com", "Two", true, true)?;
    platform::member(&cfg, "offline@example.com", "Offline", false, true)?;
    platform::member(&cfg, "disabled@example.com", "Disabled", false, false)?;
    platform::member(&cfg, "outside@example.com", "Outside", false, true)?;
    platform::Database::open(dir.path())?.edit(|data| {
        data.groups.names.insert("away".into(), "Away".into());
        data.groups
            .members
            .insert("outside@example.com".into(), "away".into());
        data.groups.operators.insert("two@example.com".into());
        for email in [
            "offline@example.com",
            "disabled@example.com",
            "outside@example.com",
        ] {
            data.members.get_mut(email).unwrap().avatar = Some("circuit-fox".into());
        }
        Ok(())
    })?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let (url, _server) = serve(portal::router(portal::Platform::open(controller)?)).await?;
    let http = reqwest::Client::new();
    let request = |method, path: &str, cookie: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", cookie)
            .header("origin", "https://games.example.com")
    };
    // Usage includes offline accounts, but never disabled accounts or another group.
    assert_eq!(
        request(Method::GET, "/api/profile", "")
            .send()
            .await?
            .status(),
        401
    );
    let usage: Value = request(Method::GET, "/api/profile", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        usage,
        json!({"users":[{"name":"Offline","avatar":"circuit-fox"}]})
    );
    assert_eq!(
        request(Method::GET, "/api/profile", "member=one")
            .header("x-infuser-inspect-group", "away")
            .send()
            .await?
            .status(),
        403
    );
    let inspected: Value = request(Method::GET, "/api/profile", "member=two")
        .header("x-infuser-inspect-group", "away")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        inspected,
        json!({"users":[{"name":"Outside","avatar":"circuit-fox"}]})
    );
    // Theme bootstrap and both stylesheets must be available before authentication.
    for (path, mime, marker) in [
        ("/assets/themes.js", "text/javascript", "InfuserThemes"),
        ("/assets/themes/nightglass.css", "text/css", "--page"),
        (
            "/assets/themes/cartridge-club.css",
            "text/css",
            "data-theme",
        ),
    ] {
        let asset = http
            .get(format!("{url}{path}"))
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(asset.headers()["content-type"], mime);
        assert!(asset.text().await?.contains(marker));
    }
    let font = http
        .get(format!(
            "{url}/assets/themes/cartridge-club/lilita-one-latin.woff2"
        ))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(font.headers()["content-type"], "font/woff2");
    assert_eq!(
        font.headers()["content-security-policy"],
        "default-src 'none'"
    );
    assert!(!font.bytes().await?.is_empty());
    let notice = http
        .get(format!("{url}/assets/themes/cartridge-club/NOTICE.md"))
        .send()
        .await?
        .error_for_status()?;
    assert!(notice.text().await?.contains("Lilita One"));
    assert_eq!(
        http.get(format!("{url}/assets/themes/cartridge-club/no-such.woff2"))
            .send()
            .await?
            .status(),
        404
    );
    let funny = http
        .get(format!("{url}/assets/themes/funny-pages.css"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(funny.headers()["content-type"], "text/css");
    assert!(funny.text().await?.contains("funny-pages"));
    let dots = http
        .get(format!("{url}/assets/themes/funny-pages/halftone.svg"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(dots.headers()["content-type"], "image/svg+xml");
    assert_eq!(
        dots.headers()["content-security-policy"],
        "default-src 'none'"
    );
    let comic = http
        .get(format!(
            "{url}/assets/themes/funny-pages/bangers-latin.woff2"
        ))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(comic.headers()["content-type"], "font/woff2");
    assert_eq!(
        comic.headers()["content-security-policy"],
        "default-src 'none'"
    );
    assert!(!comic.bytes().await?.is_empty());
    let before: Value = request(Method::GET, "/api/me", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(before["avatar"].is_null());
    assert_eq!(before["avatar_prompt_seen"], false);
    // The reminder is authenticated, origin checked and unavailable in inspection.
    assert_eq!(
        request(Method::POST, "/api/profile/picture-prompt", "")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        request(Method::POST, "/api/profile/picture-prompt", "member=one")
            .header("origin", "https://elsewhere.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::POST, "/api/profile/picture-prompt", "member=two")
            .header("x-infuser-inspect-group", "away")
            .send()
            .await?
            .status(),
        403
    );
    // Two tabs race: exactly one gets the reminder, without changing initials.
    let (a, b) = tokio::join!(
        request(Method::POST, "/api/profile/picture-prompt", "member=one").send(),
        request(Method::POST, "/api/profile/picture-prompt", "member=one").send()
    );
    let a: Value = a?.error_for_status()?.json().await?;
    let b: Value = b?.error_for_status()?.json().await?;
    assert_ne!(a["show"], b["show"]);
    let saved = platform::Database::open(dir.path())?.read()?;
    assert!(saved.members["one@example.com"].avatar_prompt_seen);
    assert!(saved.members["one@example.com"].avatar.is_none());
    assert!(!saved.members["two@example.com"].avatar_prompt_seen);
    let again: Value = request(Method::POST, "/api/profile/picture-prompt", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(again, json!({"show":false}));
    let catalog = before["avatars"].as_array().unwrap();
    assert_eq!(catalog.len(), 85);
    let portraits = before["portraits"].as_array().unwrap();
    assert_eq!(
        portraits,
        &vec![json!({"id":"origami-unicorn","name":"Origami Unicorn"})]
    );
    assert!(
        catalog
            .iter()
            .all(|avatar| avatar["id"] != "origami-unicorn")
    );
    for (id, name) in [
        ("tinker-goblin", "Tinker Goblin"),
        ("clockwork-owl", "Clockwork Owl"),
        ("circuit-crane", "Circuit Crane"),
        ("signal-kettle", "Signal Kettle"),
    ] {
        assert!(
            catalog
                .iter()
                .any(|avatar| avatar["id"] == id && avatar["name"] == name)
        );
    }
    let last_avatar = catalog.last().unwrap()["id"].as_str().unwrap();
    for avatar in catalog {
        let id = avatar["id"].as_str().unwrap();
        for size in [128, 256] {
            let image = http
                .get(format!("{url}/assets/avatars/{id}/{size}.webp"))
                .send()
                .await?
                .error_for_status()?;
            assert_eq!(image.headers()["content-type"], "image/webp");
            let bytes = image.bytes().await?;
            assert_eq!(&bytes[..4], b"RIFF");
            assert_eq!(&bytes[8..12], b"WEBP");
        }
        let saved: Value = request(Method::PUT, "/api/profile", "member=one")
            .json(&json!({"avatar":id}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(saved["avatar"], id);
    }
    for path in [
        "/assets/avatars/unknown/128.webp",
        "/assets/avatars/circuit-fox/512.webp",
    ] {
        assert_eq!(http.get(format!("{url}{path}")).send().await?.status(), 404);
    }
    assert_eq!(
        request(Method::PUT, "/api/profile", "")
            .json(&json!({"avatar":"circuit-fox"}))
            .send()
            .await?
            .status(),
        401
    );
    for origin in [None, Some("https://elsewhere.example")] {
        let mut r = http
            .put(format!("{url}/api/profile"))
            .header("host", "games.example.com")
            .header("cookie", "member=one");
        if let Some(origin) = origin {
            r = r.header("origin", origin);
        }
        assert_eq!(
            r.json(&json!({"avatar":"circuit-fox"}))
                .send()
                .await?
                .status(),
            403
        );
    }
    for (body, status) in [
        (json!({"avatar":"https://elsewhere.example/pic.png"}), 400),
        (json!({"avatar":"origami-unicorn"}), 400),
        (json!({"avatar":"../outside"}), 400),
        (
            json!({"avatar":"circuit-fox","email":"two@example.com"}),
            422,
        ),
        (json!({"avatar":null}), 422),
        (json!({}), 422),
    ] {
        assert_eq!(
            request(Method::PUT, "/api/profile", "member=one")
                .json(&body)
                .send()
                .await?
                .status()
                .as_u16(),
            status
        );
    }
    assert_eq!(
        request(Method::PUT, "/api/profile", "member=one")
            .json(&json!({"avatar":"x".repeat(2000)}))
            .send()
            .await?
            .status(),
        413
    );
    let saved: Value = request(Method::GET, "/api/me", "member=one")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(saved["avatar"], last_avatar);
    let usage: Value = request(Method::GET, "/api/profile", "member=two")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(
        usage["users"]
            .as_array()
            .unwrap()
            .iter()
            .any(|user| user == &json!({"name":"One","avatar":last_avatar}))
    );
    let other: Value = request(Method::GET, "/api/me", "member=two")
        .send()
        .await?
        .json()
        .await?;
    assert!(other["avatar"].is_null());
    let library: Value = request(Method::GET, "/api/library", "member=one")
        .send()
        .await?
        .json()
        .await?;
    let presence = library["friends"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == saved["player_id"])
        .unwrap();
    assert_eq!(presence["avatar"], last_avatar);
    assert!(presence.get("email").is_none());
    // A fresh DB connection sees the choice; invitation edits preserve it.
    assert_eq!(
        platform::Database::open(dir.path())?.read()?.members["one@example.com"]
            .avatar
            .as_deref(),
        Some(last_avatar)
    );
    // Handles are admin-controlled, including attempts to rename oneself.
    for target in ["one@example.com", "two@example.com"] {
        assert_eq!(
            request(Method::PUT, "/api/admin/members/handle", "member=one")
                .json(&json!({"email":target,"name":"Not allowed"}))
                .send()
                .await?
                .status(),
            403
        );
        assert_eq!(
            request(Method::POST, "/api/admin/members", "member=one")
                .json(&json!({"email":target,"name":"Not allowed"}))
                .send()
                .await?
                .status(),
            403
        );
    }
    assert_eq!(
        request(Method::PUT, "/api/profile", "member=one")
            .json(&json!({"avatar":"circuit-fox","name":"Not allowed"}))
            .send()
            .await?
            .status(),
        422
    );
    assert_eq!(
        http.put(format!("{url}/api/admin/members/handle"))
            .header("host", "games.example.com")
            .header("cookie", "member=two")
            .json(&json!({"email":"one@example.com","name":"No origin"}))
            .send()
            .await?
            .status(),
        403
    );
    for name in [" ".into(), "x".repeat(41), "Bad\nHandle".into()] {
        assert_eq!(
            request(Method::PUT, "/api/admin/members/handle", "member=two")
                .json(&json!({"email":"one@example.com","name":name}))
                .send()
                .await?
                .status(),
            400
        );
    }
    assert_eq!(
        request(Method::PUT, "/api/admin/members/handle", "member=two")
            .json(&json!({"email":"absent@example.com","name":"Missing"}))
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        request(Method::PUT, "/api/admin/members/handle", "member=two")
            .json(&json!({"email":"one@example.com","name":"No promotion","admin":true}))
            .send()
            .await?
            .status(),
        422
    );
    let renamed: Value = request(Method::PUT, "/api/admin/members/handle", "member=two")
        .json(&json!({"email":" ONE@example.com ","name":"  OrbitFox  "}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(renamed["player_id"], saved["player_id"]);
    assert_eq!(renamed["name"], "OrbitFox");
    let after = platform::Database::open(dir.path())?.read()?;
    let member = &after.members["one@example.com"];
    assert_eq!(member.name, "OrbitFox");
    assert_eq!(member.avatar.as_deref(), Some(last_avatar));
    assert!(!member.admin && member.enabled);
    let identity: Value = request(Method::GET, "/api/me", "member=one")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(identity["name"], "OrbitFox");
    assert_eq!(identity["player_id"], saved["player_id"]);
    let changed = request(Method::POST, "/api/admin/members", "member=two")
        .json(&json!({"email":"one@example.com","name":"Renamed","admin":false,"enabled":false}))
        .send()
        .await?;
    assert_eq!(changed.status(), 200);
    assert_eq!(
        request(Method::PUT, "/api/profile", "member=one")
            .json(&json!({"avatar":"circuit-fox"}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        platform::Database::open(dir.path())?.read()?.members["one@example.com"]
            .avatar
            .as_deref(),
        Some(last_avatar)
    );
    platform::member(&cfg, "one@example.com", "Renamed", false, true)?;
    let reset: Value = request(Method::PUT, "/api/profile", "member=one")
        .json(&json!({"avatar":""}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(reset["avatar"].is_null());
    let usage: Value = request(Method::GET, "/api/profile", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        usage,
        json!({"users":[{"name":"Offline","avatar":"circuit-fox"}]})
    );
    assert!(
        platform::Database::open(dir.path())?.read()?.members["one@example.com"]
            .avatar
            .is_none()
    );
    let legacy: platform::Member =
        serde_json::from_value(json!({"name":"Old member","admin":false,"enabled":true}))?;
    assert!(legacy.avatar.is_none());
    assert!(!legacy.avatar_prompt_seen);
    // Keeping initials and admin re-invites never reset the reminder.
    platform::member(&cfg, "one@example.com", "One", false, true)?;
    let db = platform::Database::open(dir.path())?;
    assert!(db.read()?.members["one@example.com"].avatar_prompt_seen);
    db.edit(|data| data.invite_to_group("home", "one@example.com", "One", false, true))?;
    assert!(db.read()?.members["one@example.com"].avatar_prompt_seen);
    let after: Value = request(Method::GET, "/api/me", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(after["avatar_prompt_seen"], true);
    let again: Value = request(Method::POST, "/api/profile/picture-prompt", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(again["show"], false);
    // The hidden picture stays out of the picker, members cannot select it, and
    // an operator can assign it to the member named Gob. It still renders.
    platform::member(&cfg, "gob@example.com", "Gob", false, true)?;
    platform::member(&cfg, "keeper@example.com", "Keeper", true, true)?;
    for cookie in ["member=one", "member=keeper"] {
        assert_eq!(
            request(Method::PUT, "/api/operator/members/Gob/avatar", cookie)
                .json(&json!({"avatar":"origami-unicorn"}))
                .send()
                .await?
                .status(),
            403
        );
    }
    let assigned: Value = request(
        Method::PUT,
        "/api/operator/members/Gob/avatar",
        "member=two",
    )
    .json(&json!({"avatar":"origami-unicorn"}))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(assigned["name"], "Gob");
    assert_eq!(assigned["avatar"], "origami-unicorn");
    let by_email: Value = request(
        Method::PUT,
        "/api/operator/members/gob@example.com/avatar",
        "member=two",
    )
    .json(&json!({"avatar":"origami-unicorn"}))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(by_email["avatar"], "origami-unicorn");
    assert_eq!(
        request(Method::PUT, "/api/profile", "member=gob")
            .json(&json!({"avatar":"origami-unicorn"}))
            .send()
            .await?
            .status(),
        400
    );
    let gob: Value = request(Method::GET, "/api/me", "member=gob")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(gob["avatar"], "origami-unicorn");
    assert!(
        gob["avatars"]
            .as_array()
            .unwrap()
            .iter()
            .all(|avatar| avatar["id"] != "origami-unicorn")
    );
    assert!(
        gob["portraits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|avatar| avatar["id"] == "origami-unicorn")
    );
    for size in [128, 256] {
        let image = http
            .get(format!("{url}/assets/avatars/origami-unicorn/{size}.webp"))
            .send()
            .await?
            .error_for_status()?;
        assert_eq!(image.headers()["content-type"], "image/webp");
        let bytes = image.bytes().await?;
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }
    let present: Value = request(Method::GET, "/api/library", "member=gob")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let shown = present["friends"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| person["name"] == "Gob")
        .unwrap();
    assert_eq!(shown["avatar"], "origami-unicorn");
    let seen: Value = request(Method::GET, "/api/library", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let seen_gob = seen["friends"]
        .as_array()
        .unwrap()
        .iter()
        .find(|person| person["name"] == "Gob")
        .unwrap();
    assert_eq!(seen_gob["avatar"], "origami-unicorn");
    let usage: Value = request(Method::GET, "/api/profile", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(
        usage["users"]
            .as_array()
            .unwrap()
            .iter()
            .any(|user| user == &json!({"name":"Gob","avatar":"origami-unicorn"}))
    );
    assert_eq!(
        platform::Database::open(dir.path())?.read()?.members["gob@example.com"]
            .avatar
            .as_deref(),
        Some("origami-unicorn")
    );
    Ok(())
}
