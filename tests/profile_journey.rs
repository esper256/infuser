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
                    [("X-Auth-Request-Email", "one@example.com")],
                ),
                Some("member=two") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "two@example.com")],
                ),
                _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
            }
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}",
        dir.path().display().to_string(),
        format!("{auth_url}/oauth2/auth")
    ))?;
    platform::member(&cfg, "one@example.com", "One", false, true)?;
    platform::member(&cfg, "two@example.com", "Two", true, true)?;
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
    let before: Value = request(Method::GET, "/api/me", "member=one")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(before["avatar"].is_null());
    let catalog = before["avatars"].as_array().unwrap();
    assert_eq!(catalog.len(), 15);
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
    assert!(
        platform::Database::open(dir.path())?.read()?.members["one@example.com"]
            .avatar
            .is_none()
    );
    let legacy: platform::Member =
        serde_json::from_value(json!({"name":"Old member","admin":false,"enabled":true}))?;
    assert!(legacy.avatar.is_none());
    Ok(())
}
