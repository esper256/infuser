use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    notifications::{Push, Subscription},
    platform, portal,
};
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
fn sub(device: &str) -> Subscription {
    Subscription {
        endpoint: format!("https://fcm.googleapis.com/fcm/send/{device}"),
    }
}
#[tokio::test]
async fn install_and_chat_notification_lifecycle() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let who = h.get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
            match who.strip_prefix("member=") {
                Some(who @ ("admin" | "one" | "two" | "away")) => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", format!("{who}@example.com")),
                        ("X-Auth-Request-User", format!("{who}@example.com")),
                    ],
                ),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    [
                        ("X-Auth-Request-Email", String::new()),
                        ("X-Auth-Request-User", String::new()),
                    ],
                ),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nprivate_groups=true",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    for who in ["admin", "one", "two", "away"] {
        platform::member(
            &cfg,
            &format!("{who}@example.com"),
            who,
            who == "admin",
            true,
        )?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    p.db.edit(|d| {
        d.groups.names.insert("away".into(), "Other family".into());
        d.groups
            .members
            .insert("away@example.com".into(), "away".into());
        d.groups.operators.insert("admin@example.com".into());
        Ok(())
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    };
    // Installation is public, never protected by a login redirect or served on game origins.
    let manifest: Value = req(Method::GET, "/manifest.webmanifest", "")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(manifest["name"], "Infuser");
    assert_eq!(manifest["display"], "standalone");
    assert_eq!(manifest["start_url"], "/library");
    for path in [
        "/sw.js",
        "/assets/pwa.js",
        "/assets/app-192.png",
        "/assets/app-512.png",
        "/assets/apple-touch-icon.png",
    ] {
        let r = req(Method::GET, path, "").send().await?;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["cache-control"], "no-cache");
        assert_eq!(
            http.get(format!("{url}{path}"))
                .header("host", "a.games.example.com")
                .send()
                .await?
                .status(),
            404
        );
    }
    assert_eq!(
        req(Method::GET, "/api/notifications", "")
            .send()
            .await?
            .status(),
        401
    );
    let config: Value = req(Method::GET, "/api/notifications", "one")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        B64.decode(config["public_key"].as_str().unwrap())?.len(),
        65
    );
    for who in ["admin", "one", "two", "away"] {
        assert_eq!(
            req(Method::PUT, "/api/notifications", who)
                .json(&sub(who))
                .send()
                .await?
                .status(),
            200
        );
    }
    assert_eq!(
        req(Method::PUT, "/api/notifications", "one")
            .json(&sub("two"))
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        req(Method::PUT, "/api/notifications", "one")
            .header("origin", "https://other.example.com")
            .json(&sub("test"))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        req(Method::PUT, "/api/notifications", "admin")
            .header("x-infuser-inspect-group", "away")
            .json(&sub("inspect"))
            .send()
            .await?
            .status(),
        403
    );
    for endpoint in [
        "http://fcm.googleapis.com/send",
        "https://localhost/push",
        "https://169.254.169.254/push",
        "https://fcm.googleapis.com.evil.test/push",
        "https://fcm.googleapis.com:444/push",
        "https://user@web.push.apple.com/push",
    ] {
        assert_eq!(
            req(Method::PUT, "/api/notifications", "one")
                .json(&json!({"endpoint":endpoint}))
                .send()
                .await?
                .status(),
            422
        );
    }
    let send = |id: &str| {
        req(Method::POST, "/api/chat", "one").json(&json!({"client_id":id,"text":"Come play!"}))
    };
    let id = send("notification-0001")
        .send()
        .await?
        .json::<Value>()
        .await?["id"]
        .as_u64()
        .unwrap();
    let targets = p.db.push_targets()?;
    assert_eq!(targets.len(), 2);
    assert!(
        targets
            .iter()
            .all(|(endpoint, _, _)| endpoint.ends_with("two") || endpoint.ends_with("admin"))
    );
    for (endpoint, id, _) in targets {
        p.db.push_result(&endpoint, id, Some(201))?;
    }
    send("notification-0001").send().await?.error_for_status()?;
    assert!(
        p.db.push_targets()?.is_empty(),
        "retrying a message must not re-notify"
    );
    send("notification-0002").send().await?.error_for_status()?;
    let latest = send("notification-0003")
        .send()
        .await?
        .json::<Value>()
        .await?["id"]
        .as_u64()
        .unwrap();
    assert!(latest > id);
    assert!(p.db.push_targets()?.iter().all(|(_, id, _)| *id == latest));
    // Durable outbox and signing identity survive server restart.
    let reopened = platform::Database::open(temp.path())?;
    assert_eq!(reopened.push_targets()?.len(), 2);
    assert_eq!(Push::open(temp.path())?.public_key, config["public_key"]);
    // Removing a message or revoking membership cancels pending delivery.
    p.db.chat_remove("one@example.com", latest)?;
    assert!(p.db.push_targets()?.is_empty());
    send("notification-0004").send().await?.error_for_status()?;
    p.db.edit(|d| {
        d.members.get_mut("two@example.com").unwrap().enabled = false;
        Ok(())
    })?;
    let targets = p.db.push_targets()?;
    assert_eq!(targets.len(), 1);
    assert!(targets[0].0.ends_with("admin"));
    p.db.push_result(&targets[0].0, targets[0].1, Some(410))?;
    assert!(p.db.push_targets()?.is_empty());
    p.db.edit(|d| {
        d.members.get_mut("two@example.com").unwrap().enabled = true;
        Ok(())
    })?;
    send("notification-0005").send().await?.error_for_status()?;
    let targets = p.db.push_targets()?;
    assert_eq!(targets.len(), 1);
    p.db.push_result(&targets[0].0, targets[0].1, Some(503))?;
    assert!(
        p.db.push_targets()?.is_empty(),
        "retry waits before sending again"
    );
    p.db.connection
        .lock()
        .unwrap()
        .execute("UPDATE push_outbox SET due=0", [])?;
    assert_eq!(p.db.push_targets()?.len(), 1);
    // Other members cannot unregister a device; its owner can.
    req(Method::DELETE, "/api/notifications", "one")
        .json(&sub("two"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(p.db.push_targets()?.len(), 1);
    req(Method::DELETE, "/api/notifications", "two")
        .json(&sub("two"))
        .send()
        .await?
        .error_for_status()?;
    assert!(p.db.push_targets()?.is_empty());
    // Group reassignment also fences an already queued notification.
    p.db.push_subscribe("two@example.com", &sub("two"))?;
    send("notification-0006").send().await?.error_for_status()?;
    p.db.edit(|d| {
        d.groups
            .members
            .insert("two@example.com".into(), "away".into());
        Ok(())
    })?;
    assert!(p.db.push_targets()?.is_empty());
    // Verify the standard VAPID token using its public key, not implementation internals.
    let push = Push::open(temp.path())?;
    let request = push.request(&sub("fixture"), "https://games.example.com")?;
    assert_eq!(request.headers()["content-length"], "0");
    assert!(request.body().is_none());
    assert_eq!(request.headers()["ttl"], "60");
    let authorization = request.headers()["authorization"].to_str()?;
    let (token, public) = authorization
        .strip_prefix("vapid t=")
        .unwrap()
        .split_once(", k=")
        .unwrap();
    let (input, sig) = token.rsplit_once('.').unwrap();
    let public = B64.decode(public)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &public)
        .verify(input.as_bytes(), &B64.decode(sig)?)
        .map_err(|_| anyhow::anyhow!("invalid VAPID signature"))?;
    let (header, claims) = input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&B64.decode(header)?)?;
    let claims: Value = serde_json::from_slice(&B64.decode(claims)?)?;
    assert_eq!(header["alg"], "ES256");
    assert_eq!(claims["aud"], "https://fcm.googleapis.com");
    assert_eq!(claims["sub"], "https://games.example.com");
    assert!(claims["exp"].as_u64().unwrap() > now());
    // No manual test devices or messages are ever created on production.
    if let Ok(node) = std::env::var("INFUSER_PWA_BROWSER_NODE") {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let cert_path = temp.path().join("cert.pem");
        let key_path = temp.path().join("key.pem");
        std::fs::write(&cert_path, cert.cert.pem())?;
        std::fs::write(&key_path, cert.signing_key.serialize_pem())?;
        let status = tokio::process::Command::new(node)
            .args([
                "tests/pwa_journey.mjs",
                &url,
                cert_path.to_str().unwrap(),
                key_path.to_str().unwrap(),
            ])
            .status()
            .await?;
        anyhow::ensure!(status.success(), "PWA browser journey failed");
    }
    Ok(())
}
