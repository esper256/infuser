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
async fn card_art_follows_publication_visibility_and_immutable_builds() -> Result<()> {
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
    let (url, _server) = serve(portal::router(p)).await?;
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
    Ok(())
}
