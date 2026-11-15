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
async fn feedback_requires_session_origin_membership_and_setup() -> Result<()> {
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
    let report = json!({"kind":"feature","title":"Idea","description":"A useful change","public_consent":true});
    assert_eq!(
        request(Method::GET, "/api/feedback", "member=one")
            .send()
            .await?
            .json::<Value>()
            .await?,
        json!({"available":false})
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&report)
            .send()
            .await?
            .status(),
        503
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "unknown")
            .json(&report)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .header("origin", "https://other.example")
            .json(&report)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .header("x-infuser-inspect-group", "other")
            .json(&report)
            .send()
            .await?
            .status(),
        403
    );
    let mut invalid = report.clone();
    invalid["public_consent"] = json!(false);
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&invalid)
            .send()
            .await?
            .status(),
        422
    );
    invalid = report.clone();
    invalid["extra"] = json!("secret");
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&invalid)
            .send()
            .await?
            .status(),
        422
    );
    invalid = report.clone();
    invalid["description"] = json!("x".repeat(9000));
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&invalid)
            .send()
            .await?
            .status(),
        413
    );
    platform::member(&cfg, "one@example.com", "One", false, false)?;
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&report)
            .send()
            .await?
            .status(),
        403
    );
    assert!(!dir.path().join("feedback-attempts.json").exists());
    Ok(())
}
