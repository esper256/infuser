//! Loopback-only browser acceptance fixture, never linked into the daemon.
use anyhow::Result;
use axum::{
    Router, body::Body, extract::Request, middleware::Next, response::Response, routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build, Launch},
    portal,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let secret = dir.path().join("turn.secret");
    std::fs::write(&secret, "fixture-only-turn-secret-0123456789")?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\npeerjs_port=7780\n[controller.turn]\nsecret_file={:?}\nurls=['turn:127.0.0.1:18700?transport=udp']\nrelay_port_start=18800\nrelay_port_end=18899",
        dir.path().display().to_string(),
        secret.display().to_string()
    ))?;
    platform::member(&cfg, "fixture@example.com", "Fixture", true, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("peerjs-fixture/infuser.game.json"))?;
    manifest["app_id"] = json!("peerjs-fixture");
    manifest["schema_version"] = json!(1);
    p.db.edit(|d| {
        d.builds.insert(
            "peerjs-fixture/test".into(),
            Build {
                manifest: serde_json::from_value(manifest)?,
                server: None,
                spa: false,
                cross_origin_isolation: false,
                ready: true,
                created: now(),
                files: Default::default(),
            },
        );
        d.apps.insert(
            "peerjs-fixture".into(),
            App {
                publication: None,
                build: "test".into(),
                listed: false,
                updated: now(),
            },
        );
        for id in ["a", "b", "c"] {
            d.launches.insert(
                id.into(),
                Launch {
                    deployment: None,
                    visit: None,
                    intent: None,
                    app: "peerjs-fixture".into(),
                    build: "test".into(),
                    email: "fixture@example.com".into(),
                    guest: true,
                    host: format!("{id}.games.example.com"),
                    token_hash: platform::hash(id),
                    exchanged: true,
                    expires: now() + 600,
                    ended: false,
                    library_session: String::new(),
                    grant_expires: now() + 600,
                    seen: now(),
                    lease_id: None,
                    lease_token: None,
                },
            );
        }
        Ok(())
    })?;
    let page = include_str!("peerjs-fixture/index.html");
    let hosted = page.replace(
        "</head>",
        "<script type=\"module\" src=\"/_infuser/sdk/v1.js\"></script></head>",
    );
    let router = Router::new()
        .route(
            "/test",
            get(move || {
                let text = hosted.clone();
                async move { axum::response::Html(text) }
            }),
        )
        .route(
            "/standalone",
            get(move || async move { axum::response::Html(page) }),
        )
        .route(
            "/game.js",
            get(|| async {
                (
                    [("content-type", "text/javascript")],
                    include_str!("peerjs-fixture/game.js"),
                )
            }),
        )
        .route(
            "/infuser-platform.js",
            get(|| async {
                (
                    [("content-type", "text/javascript")],
                    include_str!("../sdk/optional.js"),
                )
            }),
        )
        .route(
            "/peerjs.min.js",
            get(|| async {
                let b = tokio::fs::read("services/peerjs/node_modules/peerjs/dist/peerjs.min.js")
                    .await
                    .unwrap();
                Response::builder()
                    .header("content-type", "text/javascript")
                    .body(Body::from(b))
                    .unwrap()
            }),
        )
        .merge(portal::router(p))
        .layer(axum::middleware::from_fn(
            |mut req: Request, next: Next| async move {
                let cookie = req
                    .headers()
                    .get("cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let id = if cookie.contains("fixture_player=b") {
                    "b"
                } else if cookie.contains("fixture_player=c") {
                    "c"
                } else {
                    "a"
                };
                req.headers_mut()
                    .insert("host", format!("{id}.games.example.com").parse().unwrap());
                req.headers_mut()
                    .insert("authorization", format!("Bearer {id}").parse().unwrap());
                if req
                    .headers()
                    .get("origin")
                    .is_some_and(|v| v == "http://127.0.0.1:17878")
                {
                    req.headers_mut().insert(
                        "origin",
                        format!("https://{id}.games.example.com").parse().unwrap(),
                    );
                }
                next.run(req).await
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:17878").await?;
    println!("PeerJS browser fixture ready on 127.0.0.1:17878");
    axum::serve(listener, router).await?;
    Ok(())
}
