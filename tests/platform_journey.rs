use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use serde_json::{Value, json};
use std::sync::Arc;
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
            axum::serve(l, router).await.unwrap()
        })),
    ))
}
const HOST: &str = "games.example.com";
#[tokio::test]
async fn publish_play_save_guest_react_update_and_revoke() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let auth = Router::new().route(
        "/oauth2/auth",
        get(|h: HeaderMap| async move {
            let email = match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=admin") => "admin@example.com",
                Some("member=user") => "user@example.com",
                Some("member=outsider") => "outsider@example.com",
                _ => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        [
                            ("X-Auth-Request-Email", ""),
                            ("X-Auth-Request-User", ""),
                            ("Set-Cookie", ""),
                        ],
                    );
                }
            };
            (
                StatusCode::ACCEPTED,
                [
                    ("X-Auth-Request-Email", email),
                    ("X-Auth-Request-User", email),
                    (
                        "Set-Cookie",
                        "__Host-infuser_session=refreshed; Secure; HttpOnly; Path=/",
                    ),
                ],
            )
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64",
        dir.path().display().to_string(),
        format!("{auth_url}/oauth2/auth")
    ))?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    platform::member(&cfg, "user@example.com", "User", false, true)?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller)?;
    let db = p.db.clone();
    let (url, _server) = serve(portal::router(p)).await?;
    let http = reqwest::Client::new();
    let admin = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("cookie", "member=admin")
            .header("origin", format!("https://{HOST}"))
    };
    let user = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("cookie", "member=user")
            .header("origin", format!("https://{HOST}"))
    };
    assert_eq!(
        http.get(format!("{url}/api/me"))
            .header("host", HOST)
            .header("X-Auth-Request-Email", "admin@example.com")
            .header("X-Auth-Request-User", "admin@example.com")
            .header("X-Infuser-Verified-Profile", "admin@example.com")
            .header("X-Infuser-Verified-Email", "admin@example.com")
            .send()
            .await?
            .status(),
        401
    );
    let refreshed = admin(reqwest::Method::GET, "/api/me").send().await?;
    assert_eq!(refreshed.status(), 200);
    assert!(
        refreshed.headers()["set-cookie"]
            .to_str()?
            .contains("__Host-infuser_session=refreshed"),
        "Google session refresh cookie was lost"
    );
    assert_eq!(
        http.get(format!("{url}/api/me"))
            .header("host", HOST)
            .header("cookie", "member=outsider")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        user(reqwest::Method::GET, "/api/admin/members")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        user(reqwest::Method::GET, "/api/admin/peer-health")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        admin(reqwest::Method::GET, "/api/admin/peer-health")
            .send()
            .await?
            .status(),
        200
    );
    let owned: Value = user(reqwest::Method::POST, "/api/my-games")
        .json(&json!({"title":"My star adventure"}))
        .send()
        .await?
        .json()
        .await?;
    assert!(
        owned["app_id"]
            .as_str()
            .unwrap()
            .starts_with("game-my-star-adventure-")
    );
    assert!(
        owned["token"].is_null(),
        "the user should receive only a one-time setup code"
    );
    assert!(
        owned["tag_catalog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "puzzle")
    );
    let lifetime = owned["expires_at"].as_u64().unwrap() - infuser::controller::now();
    assert!((10790..=10800).contains(&lifetime));
    let app_id = owned["app_id"].as_str().unwrap().to_string();
    let setup_token = owned["setup_code"]
        .as_str()
        .unwrap()
        .strip_prefix("infuser_setup_")
        .unwrap()
        .to_string();
    let claim = || {
        http.post(format!("{url}/api/publisher/claim"))
            .header("host", HOST)
            .json(&json!({"token":setup_token,"app_id":app_id}))
    };
    let owned: Value = claim().send().await?.json().await?;
    assert_eq!(
        claim().send().await?.status(),
        401,
        "setup code was reusable"
    );
    let renewed: Value = user(
        reqwest::Method::POST,
        &format!("/api/my-games/{app_id}/setup"),
    )
    .send()
    .await?
    .json()
    .await?;
    let lifetime = renewed["expires_at"].as_u64().unwrap() - infuser::controller::now();
    assert!((10790..=10800).contains(&lifetime));
    let expired = renewed["setup_code"]
        .as_str()
        .unwrap()
        .strip_prefix("infuser_setup_")
        .unwrap();
    db.edit(|d| {
        d.setup_claims
            .get_mut(&platform::hash(expired))
            .unwrap()
            .expires = infuser::controller::now() - 1;
        Ok(())
    })?;
    assert_eq!(
        http.post(format!("{url}/api/publisher/claim"))
            .header("host", HOST)
            .json(&json!({"token":expired}))
            .send()
            .await?
            .status(),
        410
    );
    assert_eq!(
        http.get(format!("{url}/api/publisher/capabilities"))
            .header("host", HOST)
            .bearer_auth(owned["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        200
    );
    let connection: Value = admin(reqwest::Method::POST, "/api/admin/credentials")
        .json(&json!({"app":"moon","label":"Journey publisher"}))
        .send()
        .await?
        .json()
        .await?;
    let token = connection["token"].as_str().unwrap();
    let publisher = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .bearer_auth(token)
    };
    let build = json!({"manifest":{"schema_version":1,"app_id":"moon","title":"Moon","tags":["puzzle","3d"],"play_modes":["solo"],"solo_runtime":"client_only","multiplayer":null},"server":null,"spa":true,"cross_origin_isolation":true});
    assert_eq!(
        publisher(
            reqwest::Method::PUT,
            "/api/publisher/apps/another/builds/b-one"
        )
        .json(&build)
        .send()
        .await?
        .status(),
        401
    );
    assert_eq!(
        publisher(
            reqwest::Method::PUT,
            "/api/publisher/apps/moon/builds/b-one"
        )
        .json(&build)
        .send()
        .await?
        .status(),
        200
    );
    let index = "<!doctype html><title>Moon v1</title><script type=module src=\"/_infuser/sdk.js\"></script>";
    for (path, bytes) in [
        ("index.html", index.as_bytes()),
        ("game.wasm", b"\0asm\x01\0\0\0".as_slice()),
    ] {
        assert_eq!(
            publisher(
                reqwest::Method::PUT,
                &format!("/api/publisher/apps/moon/builds/b-one/files/{path}")
            )
            .body(bytes.to_vec())
            .send()
            .await?
            .status(),
            200
        );
    }
    assert_eq!(
        publisher(
            reqwest::Method::POST,
            "/api/publisher/apps/moon/builds/b-one/publish"
        )
        .json(&json!({"listed":false}))
        .send()
        .await?
        .status(),
        200
    );
    let list: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert!(
        list["apps"].as_array().unwrap().is_empty(),
        "preview leaked to user"
    );
    assert_eq!(
        user(reqwest::Method::POST, "/api/apps/moon/launch")
            .json(&json!({"mode":"solo"}))
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        publisher(
            reqwest::Method::POST,
            "/api/publisher/apps/moon/builds/b-one/publish"
        )
        .send()
        .await?
        .status(),
        200
    );
    assert_eq!(
        publisher(
            reqwest::Method::PUT,
            "/api/publisher/apps/moon/builds/b-one/files/index.html"
        )
        .body("overwrite")
        .send()
        .await?
        .status(),
        409
    );
    let list: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(list["apps"][0]["title"], "Moon");
    assert_eq!(list["apps"][0]["author"]["name"], "Admin");
    assert!(list["apps"][0]["author"]["avatar"].is_null());
    admin(reqwest::Method::PUT, "/api/profile")
        .json(&json!({"avatar":"circuit-fox"}))
        .send()
        .await?
        .error_for_status()?;
    let updated: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(updated["apps"][0]["author"]["avatar"], "circuit-fox");
    assert_eq!(
        list["apps"][0]["author"]["id"],
        platform::hash("admin@example.com")
    );
    assert!(list["apps"][0]["author"].get("email").is_none());
    assert_eq!(list["apps"][0]["tags"], json!(["puzzle", "3d"]));
    assert_eq!(
        user(reqwest::Method::PUT, "/api/apps/moon/tags")
            .json(&json!({"tags":["racing"]}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        user(reqwest::Method::POST, "/api/my-games/moon/setup")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        admin(reqwest::Method::PUT, "/api/apps/moon/tags")
            .json(&json!({"tags":["Chill","Puzzle"]}))
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        user(reqwest::Method::POST, "/api/apps/moon/launch")
            .header("origin", "https://evil.example")
            .json(&json!({"mode":"solo"}))
            .send()
            .await?
            .status(),
        403
    );
    let mut sessions = vec![];
    for guest in [false, true, true] {
        let launch: Value = user(reqwest::Method::POST, "/api/apps/moon/launch")
            .json(&json!({"guest":guest}))
            .send()
            .await?
            .json()
            .await?;
        let link = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
        let game_host = link.host_str().unwrap().to_string();
        let ticket = link
            .fragment()
            .unwrap()
            .strip_prefix("infuser=")
            .unwrap()
            .to_string();
        let exchange = || {
            http.post(format!("{url}/_infuser/session"))
                .header("host", &game_host)
                .header("origin", format!("https://{game_host}"))
                .json(&json!({"token":ticket}))
        };
        let r = exchange().send().await?;
        assert_eq!(r.status(), 200);
        assert!(
            r.headers()["set-cookie"]
                .to_str()?
                .contains("Secure; HttpOnly; SameSite=Strict")
        );
        assert_eq!(
            exchange().send().await?.status(),
            401,
            "launch token reused"
        );
        sessions.push((game_host, ticket));
    }
    user(reqwest::Method::PUT, "/api/profile")
        .json(&json!({"avatar":"moss-knight"}))
        .send()
        .await?
        .error_for_status()?;
    let playing: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let players = playing["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["id"] == "moon")
        .unwrap()["players"]
        .as_array()
        .unwrap();
    assert_eq!(players.len(), 3);
    assert!(players.iter().all(|player| {
        if player["guest"] == true {
            player["player_id"].as_str().unwrap().starts_with("guest-")
                && player["player_id"] != player["id"]
        } else {
            player["player_id"] == player["id"]
        }
    }));
    let player_ids: std::collections::BTreeSet<_> = players
        .iter()
        .map(|player| player["player_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        player_ids.len(),
        3,
        "each guest has a distinct join identity"
    );
    assert_eq!(
        players
            .iter()
            .filter(|player| player["guest"] == true)
            .count(),
        2
    );
    assert!(
        players
            .iter()
            .all(|player| { player["avatar"] == "moss-knight" && player.get("email").is_none() })
    );
    assert_ne!(sessions[0].0, sessions[1].0);
    assert_ne!(sessions[1].0, sessions[2].0);
    let game = |n: usize, method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", &sessions[n].0)
            .bearer_auth(&sessions[n].1)
            .header("origin", format!("https://{}", sessions[n].0))
    };
    let browser = std::env::var("INFUSER_DIAGNOSTICS_BROWSER_NODE").ok();
    if let Some(node) = &browser {
        let outcome = tokio::process::Command::new(node)
            .arg("tests/diagnostics_browser.mjs")
            .args([&url, &sessions[0].0, &sessions[0].1])
            .status()
            .await?;
        anyhow::ensure!(outcome.success(), "diagnostics browser journey failed");
    }
    // Follow a real launch across a browser retry, scoped publisher read and operator CLI.
    let ctx: Value = game(0, reqwest::Method::GET, "/_infuser/context")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let at = infuser::timeline::ms();
    let mut diagnostic = json!({"scope":ctx["diagnostic_launch"],"events":[
        {"page":"deadbeef0001","seq":1,"at":at,"src":"game","kind":"js.error","data":{"class":"TypeError","message":"fixture failed at https://example.com/?token=private-fixture","file":"fixture.js","fingerprint":"deadbea7","line":17}},
        {"page":"deadbeef0001","seq":2,"at":at,"src":"player","kind":"op","data":{"name":"game.load","outcome":"fail","reason":"error:CompileError","stage":"engine","room":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}},
        {"page":"deadbeef0001","seq":3,"at":at,"src":"game","kind":"mp.connected","data":{}}
    ]});
    // A replayed older batch must never reduce the folded occurrence count.
    for count in [1, 5, 3] {
        diagnostic["events"][0]["data"]["count"] = count.into();
        game(0, reqwest::Method::POST, "/_infuser/telemetry")
            .json(&diagnostic)
            .send()
            .await?
            .error_for_status()?;
    }
    assert_eq!(
        game(1, reqwest::Method::POST, "/_infuser/telemetry")
            .json(&diagnostic)
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        publisher(
            reqwest::Method::GET,
            "/api/publisher/apps/other/diagnostic-events"
        )
        .send()
        .await?
        .status(),
        401
    );
    let batch: Value = publisher(
        reqwest::Method::GET,
        "/api/publisher/apps/moon/diagnostic-events",
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    let events = batch["events"].as_array().unwrap();
    let matching: Vec<_> = events
        .iter()
        .filter(|e| {
            e["kind"] == "js.error"
                && e["data"]["line"] == 17
                && e["data"]["fingerprint"] == "deadbea7"
        })
        .collect();
    assert_eq!(matching.len(), 1);
    let e = matching[0];
    assert_eq!(e["data"]["count"], 5);
    assert!(
        events
            .iter()
            .any(|e| e["kind"] == "op" && e["data"]["stage"] == "engine")
    );
    assert!(!events.iter().any(|e| e["kind"] == "mp.connected"));
    assert!(e.get("profile").is_none() && e["data"].get("message").is_none());
    assert!(
        events
            .iter()
            .any(|l| l["kind"] == "launch.create" && l["launch"] == e["launch"])
    );
    let config_path = dir.path().join("local.toml");
    std::fs::write(
        &config_path,
        format!(
            "[controller]\nstate_dir={:?}",
            dir.path().display().to_string()
        ),
    )?;
    let logs = std::process::Command::new(env!("CARGO_BIN_EXE_infuser"))
        .arg("logs")
        .arg(&config_path)
        .args(["--profile", "User", "--game", "moon", "--jsonl"])
        .output()?;
    anyhow::ensure!(
        logs.status.success(),
        "{}",
        String::from_utf8_lossy(&logs.stderr)
    );
    let lines = String::from_utf8(logs.stdout)?;
    if browser.is_some() {
        assert!(!lines.contains("Browser fixture crash") && lines.contains("page.freeze"));
    }
    assert!(!lines.contains("fixture failed") && !lines.contains("private-fixture"));
    let query = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_infuser"))
            .arg("logs")
            .arg(&config_path)
            .args(args)
            .output()
    };
    for (flag, expected) in [("--game", "unknown game"), ("--profile", "unknown profile")] {
        let bad = query(&[flag, "does-not-exist"])?;
        assert!(!bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains(expected));
    }
    let incident = query(&[
        "incident",
        "--profile",
        "User",
        "--at",
        &at.to_string(),
        "--window",
        "1m",
        "--json",
    ])?;
    let rows: Vec<Value> = serde_json::from_slice(&incident.stdout)?;
    assert!(
        rows.iter()
            .any(|e| e["kind"] == "operation.failure" && e["data"]["stage"] == "engine")
    );
    assert!(
        !String::from_utf8_lossy(&incident.stdout).contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    let empty = query(&["--profile", "User", "--kind", "no.such.event"])?;
    assert!(
        empty.status.success()
            && String::from_utf8_lossy(&empty.stdout).contains("no events for User")
    );
    let filtered = query(&[
        "--game", "moon", "--kind", "js.*", "--source", "client", "--level", "error", "--json",
    ])?;
    let events: Vec<Value> = serde_json::from_slice(&filtered.stdout)?;
    assert!(
        events
            .iter()
            .any(|e| e["data"]["fingerprint"] == "deadbea7")
    );
    db.edit(|d| {
        d.presence.remove("user@example.com");
        Ok(())
    })?;
    assert_eq!(
        game(1, reqwest::Method::POST, "/_infuser/heartbeat")
            .send()
            .await?
            .status(),
        200
    );
    assert!(
        !db.read()?.presence.contains_key("user@example.com"),
        "a guest is not the account owner's presence"
    );
    assert_eq!(
        game(0, reqwest::Method::POST, "/_infuser/heartbeat")
            .send()
            .await?
            .status(),
        200
    );
    assert!(
        db.read()?.presence.contains_key("user@example.com"),
        "playing with the Library closed should still count as online"
    );
    let page = game(0, reqwest::Method::GET, "/").send().await?;
    assert_eq!(page.status(), 200);
    assert_eq!(
        page.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert!(page.text().await?.contains("Moon v1"));
    let player = game(0, reqwest::Method::GET, "/_infuser/play")
        .send()
        .await?;
    assert_eq!(player.status(), 200);
    assert_eq!(
        player.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert!(
        player.headers()["content-security-policy"]
            .to_str()?
            .contains("frame-src 'self'")
    );
    assert!(player.text().await?.contains("<iframe"));
    assert_eq!(
        http.get(format!("{url}/_infuser/play"))
            .header("host", &sessions[0].0)
            .send()
            .await?
            .status(),
        401
    );
    let wasm = game(0, reqwest::Method::GET, "/game.wasm").send().await?;
    assert_eq!(wasm.headers()["content-type"], "application/wasm");
    let range = game(0, reqwest::Method::GET, "/game.wasm")
        .header("range", "bytes=0-3")
        .send()
        .await?;
    assert_eq!(range.status(), 206);
    assert_eq!(range.bytes().await?.len(), 4);
    assert_eq!(
        game(0, reqwest::Method::GET, "/missing.wasm")
            .header("accept", "text/html")
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        game(0, reqwest::Method::GET, "/level/two")
            .header("accept", "text/html")
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        http.get(format!("{url}/_infuser/context"))
            .bearer_auth(&sessions[0].1)
            .header("host", &sessions[1].0)
            .send()
            .await?
            .status(),
        401
    );
    let s: Value = game(0, reqwest::Method::PUT, "/_infuser/saves/progress")
        .json(&json!({"version":0,"data":{"stars":7}}))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(s["version"], 1);
    assert_eq!(
        game(0, reqwest::Method::PUT, "/_infuser/saves/progress")
            .json(&json!({"version":0,"data":{"stars":0}}))
            .send()
            .await?
            .status(),
        409
    );
    let guest_save: Value = game(1, reqwest::Method::GET, "/_infuser/saves/progress")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(guest_save["version"], 0);
    assert!(guest_save["data"].is_null());
    game(1, reqwest::Method::PUT, "/_infuser/saves/progress")
        .json(&json!({"version":0,"data":{"stars":99}}))
        .send()
        .await?;
    let personal: Value = game(0, reqwest::Method::GET, "/_infuser/saves/progress")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(personal["data"]["stars"], 7);
    assert_eq!(
        user(reqwest::Method::PUT, "/api/apps/moon/reaction")
            .json(&json!({"emoji":"🚀","text":"I loved the stars!"}))
            .send()
            .await?
            .status(),
        200
    );
    let list: Value = admin(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        list["apps"][0]["reactions"][0]["text"],
        "I loved the stars!"
    );
    let vote_path = format!(
        "/api/apps/moon/updates/{}/reaction",
        list["apps"][0]["update_id"].as_str().unwrap()
    );
    for emoji in ["⭐", "👍", "👍"] {
        assert_eq!(
            user(reqwest::Method::PUT, &vote_path)
                .json(&json!({"emoji":emoji}))
                .send()
                .await?
                .status(),
            200
        );
    }
    assert_eq!(
        admin(reqwest::Method::PUT, &vote_path)
            .json(&json!({"emoji":"👍"}))
            .send()
            .await?
            .status(),
        200
    );
    for bad in [json!({"emoji":"no"}), json!({"emoji":"⭐","text":"a note"})] {
        assert!(
            user(reqwest::Method::PUT, &vote_path)
                .json(&bad)
                .send()
                .await?
                .status()
                .is_client_error()
        );
    }
    let voted: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        voted["apps"][0]["update_reactions"],
        json!({"counts":{"👍":2},"mine":"👍"})
    );
    assert_eq!(
        voted["apps"][0]["reactions"][0]["text"],
        "I loved the stars!"
    );
    assert_eq!(voted["apps"][0]["reactions"][0]["emoji"], "🚀");
    // A new static build does not swap assets underneath existing players.
    publisher(
        reqwest::Method::PUT,
        "/api/publisher/apps/moon/builds/b-two",
    )
    .json(&build)
    .send()
    .await?;
    publisher(
        reqwest::Method::PUT,
        "/api/publisher/apps/moon/builds/b-two/files/index.html",
    )
    .body(index.replace("v1", "v2"))
    .send()
    .await?;
    assert_eq!(
        publisher(
            reqwest::Method::POST,
            "/api/publisher/apps/moon/builds/b-two/publish"
        )
        .send()
        .await?
        .status(),
        200
    );
    assert!(
        game(0, reqwest::Method::GET, "/")
            .send()
            .await?
            .text()
            .await?
            .contains("Moon v1")
    );
    let updated: Value = user(reqwest::Method::POST, "/api/apps/moon/launch")
        .json(&json!({"mode":"solo"}))
        .send()
        .await?
        .json()
        .await?;
    let updated_url = reqwest::Url::parse(updated["url"].as_str().unwrap())?;
    let tags: Value = user(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        user(reqwest::Method::PUT, &vote_path)
            .json(&json!({"emoji":"❤️"}))
            .send()
            .await?
            .status(),
        409
    );
    assert_ne!(tags["apps"][0]["update_id"], list["apps"][0]["update_id"]);
    assert_eq!(
        tags["apps"][0]["update_reactions"],
        json!({"counts":{},"mine":null})
    );
    assert_eq!(
        tags["apps"][0]["tags"],
        json!(["chill", "puzzle"]),
        "publishing erased author-edited tags"
    );
    assert_ne!(
        updated_url.host_str().unwrap(),
        sessions[0].0,
        "new build would overwrite the old build's browser cookie"
    );
    assert_eq!(
        publisher(
            reqwest::Method::DELETE,
            "/api/publisher/apps/moon/builds/b-one"
        )
        .send()
        .await?
        .status(),
        409
    );
    // Revocation also applies to already-open game hosts and cloud saves.
    platform::member(&cfg, "user@example.com", "User", false, false)?;
    assert_eq!(
        game(0, reqwest::Method::GET, "/_infuser/saves/progress")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.get(format!("{url}/api/publisher/capabilities"))
            .header("host", HOST)
            .bearer_auth(owned["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        user(reqwest::Method::GET, "/api/library")
            .send()
            .await?
            .status(),
        403
    );
    admin(
        reqwest::Method::DELETE,
        &format!(
            "/api/admin/credentials/{}",
            connection["id"].as_str().unwrap()
        ),
    )
    .send()
    .await?;
    assert_eq!(
        publisher(reqwest::Method::GET, "/api/publisher/capabilities")
            .send()
            .await?
            .status(),
        401
    );
    // A fresh database handle sees durable saves; no in-memory-only success.
    drop(db);
    let db = platform::Database::open(dir.path())?;
    assert_eq!(
        db.save_read(&platform::hash("user@example.com"), "moon", "progress")?
            .1["stars"],
        7
    );
    Ok(())
}

#[tokio::test]
async fn only_authenticated_members_reach_the_ip_admission_helper() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("admit.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let (sent, received) = tokio::sync::oneshot::channel();
    let helper = Running(tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut ip = String::new();
        stream.read_to_string(&mut ip).await.unwrap();
        stream.write_all(b"ok\n").await.unwrap();
        sent.send(ip).unwrap();
    }));
    let auth = Router::new().route(
        "/oauth2/auth",
        get(|h: HeaderMap| async move {
            if h.get("cookie").is_some_and(|c| c == "member=admin") {
                (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "admin@example.com"),
                        ("X-Auth-Request-User", "admin@example.com"),
                    ],
                )
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                )
            }
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nfirewall_socket={:?}",
        dir.path().display().to_string(),
        format!("{auth_url}/oauth2/auth"),
        socket.display().to_string()
    ))?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    let c = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let (url, _server) = serve(portal::router(portal::Platform::open(c)?)).await?;
    let http = reqwest::Client::new();
    let request = || {
        http.get(format!("{url}/api/me"))
            .header("host", HOST)
            .header("x-infuser-client-ip", "192.0.2.10")
    };
    assert_eq!(request().send().await?.status(), 401);
    let me: Value = request()
        .header("cookie", "member=admin")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(me["game_ports_admitted"], true);
    assert_eq!(received.await?, "192.0.2.10\n");
    drop(helper);
    // The rate-limit cache avoids a subprocess/helper operation on every asset/request.
    let me: Value = request()
        .header("cookie", "member=admin")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(me["game_ports_admitted"], true);
    Ok(())
}
