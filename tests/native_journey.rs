use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", l.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(l, router).await.unwrap();
        })),
    ))
}
async fn ok(r: reqwest::RequestBuilder) -> Result<Value> {
    let r = r.send().await?;
    let status = r.status();
    let body: Value = r.json().await?;
    anyhow::ensure!(status.is_success(), "{status}: {body}");
    Ok(body)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[tokio::test]
async fn native_pairing_delivery_and_guest_identity() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=admin") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "admin@example.com")],
                ),
                Some("member=user") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "user@example.com")],
                ),
                _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64",
        dir.path().display().to_string(),
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
    let publisher =
        p.db.edit(|d| Ok(platform::credential(d, "native-pond", "publish", "Pond")?.1))?;
    let (url, _server) = serve(portal::router(p)).await?;
    let http = Client::new();
    let req = |method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
    };
    let member = |method, path: &str, who: &str| {
        req(method, path)
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    };
    let base = "/api/publisher/apps/native-pond/builds/one";
    ok(req(Method::PUT,base).bearer_auth(&publisher).json(&json!({"manifest":{"schema_version":1,"app_id":"native-pond","title":"Native Pond","play_modes":["solo"],"solo_runtime":"client_only"}}))).await?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("game.exe", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"MZfixture")?;
    let bytes = zip.finish()?.into_inner();
    let native = json!({"version":1,"engine":"godot","native_only":true,"platforms":{"windows-x86_64":{"archive":"infuser-native/windows-x86_64.zip","sha256":hash(&bytes),"bytes":bytes.len(),"entrypoint":"game.exe","files":{"game.exe":{"sha256":hash(b"MZfixture"),"bytes":9}}}}});
    for (path, body) in [
        (
            "index.html",
            b"<script type=module src='/_infuser/sdk.js'></script>".to_vec(),
        ),
        ("infuser-native/v1.json", serde_json::to_vec(&native)?),
        ("infuser-native/windows-x86_64.zip", bytes.clone()),
    ] {
        ok(req(Method::PUT, &format!("{base}/files/{path}"))
            .bearer_auth(&publisher)
            .body(body))
        .await?;
    }
    ok(req(Method::POST, &format!("{base}/publish"))
        .bearer_auth(&publisher)
        .json(&json!({"listed":false})))
    .await?;
    let library = ok(member(Method::GET, "/api/library", "admin")).await?;
    assert_eq!(library["apps"][0]["native_only"], true);
    assert_eq!(
        library["apps"][0]["native_platforms"],
        json!(["windows-x86_64"])
    );
    let play_body = json!({"platform":"windows-x86_64","guest":true});
    assert_eq!(
        member(Method::POST, "/api/native/v1/play/native-pond", "user")
            .json(&play_body)
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        req(Method::POST, "/api/native/v1/device/start")
            .header("Origin", "https://evil.example")
            .json(&json!({"challenge":"a".repeat(64)}))
            .send()
            .await?
            .status(),
        403
    );
    let verifier = "a".repeat(64);
    let pair = ok(req(Method::POST, "/api/native/v1/device/start")
        .json(&json!({"challenge":hash(verifier.as_bytes())})))
    .await?;
    let proof = json!({"request_id":pair["request_id"],"verifier":verifier});
    assert_eq!(
        ok(req(Method::POST, "/api/native/v1/device/token").json(&proof)).await?["status"],
        "pending"
    );
    let wrong = json!({"request_id":pair["request_id"],"verifier":"b".repeat(64)});
    assert_eq!(
        req(Method::POST, "/api/native/v1/device/token")
            .json(&wrong)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        req(Method::POST, "/api/native/v1/device/approve")
            .json(&json!({"request_id":pair["request_id"]}))
            .send()
            .await?
            .status(),
        401
    );
    ok(
        member(Method::POST, "/api/native/v1/device/approve", "admin")
            .json(&json!({"request_id":pair["request_id"]})),
    )
    .await?;
    let device = ok(req(Method::POST, "/api/native/v1/device/token").json(&proof)).await?;
    let key = device["token"].as_str().unwrap();
    assert_eq!(
        req(Method::POST, "/api/native/v1/device/token")
            .json(&proof)
            .send()
            .await?
            .status(),
        410
    );
    let play =
        ok(member(Method::POST, "/api/native/v1/play/native-pond", "admin").json(&play_body))
            .await?;
    let link = reqwest::Url::parse(play["url"].as_str().unwrap())?;
    let nonce = link
        .query_pairs()
        .find(|(k, _)| k == "request")
        .unwrap()
        .1
        .to_string();
    let claim = json!({"request_id":nonce,"platform":"windows-x86_64"});
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .bearer_auth(key)
            .json(&json!({"request_id":nonce,"platform":"linux-x86_64"}))
            .send()
            .await?
            .status(),
        409
    );
    let second_verifier = "d".repeat(64);
    let second_pair = ok(req(Method::POST, "/api/native/v1/device/start")
        .json(&json!({"challenge":hash(second_verifier.as_bytes())})))
    .await?;
    assert_eq!(
        member(Method::POST, "/api/native/v1/device/approve", "user")
            .header("origin", "https://elsewhere.example")
            .json(&json!({"request_id":second_pair["request_id"]}))
            .send()
            .await?
            .status(),
        403
    );
    ok(member(Method::POST, "/api/native/v1/device/approve", "user")
        .json(&json!({"request_id":second_pair["request_id"]})))
    .await?;
    let second_device = ok(req(Method::POST, "/api/native/v1/device/token")
        .json(&json!({"request_id":second_pair["request_id"],"verifier":second_verifier})))
    .await?;
    let second_key = second_device["token"].as_str().unwrap();
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .bearer_auth(second_key)
            .json(&claim)
            .send()
            .await?
            .status(),
        403
    );
    ok(req(Method::POST, "/api/native/v1/device/revoke").bearer_auth(second_key)).await?;
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .bearer_auth(second_key)
            .json(&claim)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .json(&claim)
            .send()
            .await?
            .status(),
        401
    );
    let launch = ok(req(Method::POST, "/api/native/v1/claim")
        .bearer_auth(key)
        .json(&claim))
    .await?;
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .bearer_auth(key)
            .json(&claim)
            .send()
            .await?
            .status(),
        410
    );
    assert_eq!(launch["guest"], true);
    assert!(launch["player_id"].as_str().unwrap().starts_with("guest-"));
    let game_host = launch["game_origin"]
        .as_str()
        .unwrap()
        .strip_prefix("https://")
        .unwrap();
    let game = |path: &str| {
        http.get(format!("{url}{path}"))
            .header("host", game_host)
            .bearer_auth(launch["token"].as_str().unwrap())
    };
    let context = ok(game("/_infuser/api/v1/context")).await?;
    assert_eq!(context["guest"], true);
    assert_eq!(context["player_id"], launch["player_id"]);
    assert_eq!(
        game("/infuser-native/windows-x86_64.zip")
            .send()
            .await?
            .bytes()
            .await?
            .as_ref(),
        bytes.as_slice()
    );
    let devices = ok(member(Method::GET, "/api/native/v1/devices", "admin")).await?;
    let path = format!(
        "/api/native/v1/devices/{}",
        devices["devices"][0]["id"].as_str().unwrap()
    );
    assert_eq!(
        member(Method::DELETE, &path, "user").send().await?.status(),
        404
    );
    ok(member(Method::DELETE, &path, "admin")).await?;
    assert_eq!(
        req(Method::POST, "/api/native/v1/claim")
            .bearer_auth(key)
            .json(&claim)
            .send()
            .await?
            .status(),
        401
    );
    Ok(())
}
