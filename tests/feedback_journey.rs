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
async fn private_queue_enforces_adult_group_boundaries_consent_and_retention() -> Result<()> {
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
                Some("member=three") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "three@example.com")],
                ),
                _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
            }
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nprivate_groups=true\nstate_dir={:?}\nauth_url={:?}",
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
    let p = portal::Platform::open(controller)?;
    p.db.edit(|d| {
        d.groups.names.insert("other".into(), "Other".into());
        d.invite_to_group("other", "three@example.com", "Three", true, true)
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let request = |method, path: &str, cookie: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", cookie)
            .header("origin", "https://games.example.com")
    };
    let report = json!({"kind":"feature","title":"Idea","description":"Private details for adult review","diagnostics":null});
    let intake = json!({"request_id":"request-0000000001","report":report});
    assert_eq!(
        request(Method::GET, "/api/feedback", "member=one")
            .send()
            .await?
            .json::<Value>()
            .await?,
        json!({"adult":false})
    );
    let queued = request(Method::POST, "/api/feedback", "member=one")
        .json(&intake)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let id = queued["id"].as_str().unwrap();
    let review_path = format!("/api/feedback/review/{id}");
    let send_path = format!("{review_path}/send");
    let approval = json!({"report":report,"public_consent":true});
    for _ in 0..2 {
        assert_eq!(
            request(Method::POST, "/api/feedback", "member=one")
                .json(&intake)
                .send()
                .await?
                .json::<Value>()
                .await?,
            queued
        );
    }
    let mut retry = intake.clone();
    retry["request_id"] = json!("request-0000000002");
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&retry)
            .send()
            .await?
            .json::<Value>()
            .await?,
        queued
    );
    retry["request_id"] = intake["request_id"].clone();
    retry["report"]["title"] = json!("Different");
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&retry)
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        request(Method::GET, "/api/feedback/review", "member=one")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::POST, &send_path, "member=one")
            .json(&approval)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::DELETE, &review_path, "member=one")
            .send()
            .await?
            .status(),
        403
    );
    let other = request(Method::GET, "/api/feedback/review", "member=three")
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(other["entries"], json!([]));
    assert_eq!(
        request(Method::POST, &send_path, "member=three")
            .json(&approval)
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        request(Method::DELETE, &review_path, "member=three")
            .send()
            .await?
            .status(),
        404
    );
    let own = request(Method::GET, "/api/feedback/review", "member=two")
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(own["entries"].as_array().unwrap().len(), 1);
    assert_eq!(own["entries"][0]["report"], report);
    assert_eq!(own["available"], false);
    assert!(!own.to_string().contains("one@example.com"));
    assert_eq!(
        request(Method::POST, &send_path, "member=two")
            .json(&approval)
            .send()
            .await?
            .status(),
        503
    );
    assert_eq!(
        request(Method::POST, &send_path, "member=two")
            .header("origin", "https://evil.example")
            .json(&approval)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::DELETE, &review_path, "member=two")
            .header("origin", "https://evil.example")
            .send()
            .await?
            .status(),
        403
    );
    let mut denied = approval.clone();
    denied["public_consent"] = json!(false);
    assert_eq!(
        request(Method::POST, &send_path, "member=two")
            .json(&denied)
            .send()
            .await?
            .status(),
        422
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "unknown")
            .json(&intake)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .header("origin", "https://evil.example")
            .json(&intake)
            .send()
            .await?
            .status(),
        403
    );
    for method in [Method::GET, Method::POST] {
        let path = if method == Method::GET {
            "/api/feedback/review"
        } else {
            &send_path
        };
        assert_eq!(
            request(method, path, "member=two")
                .header("x-infuser-inspect-group", "other")
                .json(&approval)
                .send()
                .await?
                .status(),
            403
        );
    }
    let mut invalid = intake.clone();
    invalid["public_consent"] = json!(true);
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&invalid)
            .send()
            .await?
            .status(),
        422
    );
    invalid = intake.clone();
    invalid["report"]["description"] = json!("x".repeat(17000));
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&invalid)
            .send()
            .await?
            .status(),
        413
    );
    for n in 2..=4 {
        let mut body = intake.clone();
        body["request_id"] = json!(format!("request-000000000{n}"));
        body["report"]["title"] = json!(format!("Idea {n}"));
        assert_eq!(
            request(Method::POST, "/api/feedback", "member=one")
                .json(&body)
                .send()
                .await?
                .status()
                .as_u16(),
            if n == 4 { 429 } else { 200 }
        );
    }
    assert_eq!(
        request(Method::DELETE, &review_path, "member=two")
            .send()
            .await?
            .status(),
        200
    );
    let path = dir.path().join("feedback-queue.json");
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(saved["entries"][0]["report"], Value::Null);
    // Tombstones prevent replay from recreating dismissed content.
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&intake)
            .send()
            .await?
            .json::<Value>()
            .await?,
        queued
    );
    for entry in saved["entries"].as_array_mut().unwrap() {
        entry["created"] = json!(1);
    }
    std::fs::write(&path, serde_json::to_vec(&saved)?)?;
    p.cleanup_feedback()?;
    let saved: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(saved["entries"], json!([]));
    platform::member(&cfg, "one@example.com", "One", false, false)?;
    assert_eq!(
        request(Method::POST, "/api/feedback", "member=one")
            .json(&intake)
            .send()
            .await?
            .status(),
        403
    );
    platform::member(&cfg, "two@example.com", "Two", true, false)?;
    assert_eq!(
        request(Method::GET, "/api/feedback/review", "member=two")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        request(Method::POST, &send_path, "member=two")
            .json(&approval)
            .send()
            .await?
            .status(),
        403
    );
    assert!(!dir.path().join("feedback-attempts.json").exists());
    Ok(())
}
