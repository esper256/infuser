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
                Some("member=adult") => "adult@example.com",
                Some("member=kid") => "kid@example.com",
                Some("member=outsider") => "outsider@example.com",
                _ => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        [("X-Auth-Request-Email", ""), ("Set-Cookie", "")],
                    );
                }
            };
            (
                StatusCode::ACCEPTED,
                [
                    ("X-Auth-Request-Email", email),
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
    platform::member(&cfg, "adult@example.com", "Grown-up", true, true)?;
    platform::member(&cfg, "kid@example.com", "Kid", false, true)?;
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
    let adult = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("cookie", "member=adult")
            .header("origin", format!("https://{HOST}"))
    };
    let kid = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("cookie", "member=kid")
            .header("origin", format!("https://{HOST}"))
    };
    assert_eq!(
        http.get(format!("{url}/api/me"))
            .header("host", HOST)
            .header("X-Auth-Request-Email", "adult@example.com")
            .header("X-Infuser-Verified-Email", "adult@example.com")
            .send()
            .await?
            .status(),
        401
    );
    let refreshed = adult(reqwest::Method::GET, "/api/me").send().await?;
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
        kid(reqwest::Method::GET, "/api/admin/members")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        kid(reqwest::Method::GET, "/api/admin/peer-health")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        adult(reqwest::Method::GET, "/api/admin/peer-health")
            .send()
            .await?
            .status(),
        200
    );
    let owned: Value = kid(reqwest::Method::POST, "/api/my-games")
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
        "the child should receive only a one-time setup code"
    );
    assert!(
        owned["tag_catalog"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "puzzle")
    );
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
    let renewed: Value = kid(
        reqwest::Method::POST,
        &format!("/api/my-games/{app_id}/setup"),
    )
    .send()
    .await?
    .json()
    .await?;
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
    let connection: Value = adult(reqwest::Method::POST, "/api/admin/credentials")
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
    let list: Value = kid(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert!(
        list["apps"].as_array().unwrap().is_empty(),
        "preview leaked to kid"
    );
    assert_eq!(
        kid(reqwest::Method::POST, "/api/apps/moon/launch")
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
    let list: Value = kid(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(list["apps"][0]["title"], "Moon");
    assert_eq!(list["apps"][0]["author"]["name"], "Grown-up");
    assert_eq!(
        list["apps"][0]["author"]["id"],
        platform::hash("adult@example.com")
    );
    assert!(list["apps"][0]["author"].get("email").is_none());
    assert_eq!(list["apps"][0]["tags"], json!(["puzzle", "3d"]));
    assert_eq!(
        kid(reqwest::Method::PUT, "/api/apps/moon/tags")
            .json(&json!({"tags":["racing"]}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        kid(reqwest::Method::POST, "/api/my-games/moon/setup")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        adult(reqwest::Method::PUT, "/api/apps/moon/tags")
            .json(&json!({"tags":["Chill","Puzzle"]}))
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        kid(reqwest::Method::POST, "/api/apps/moon/launch")
            .header("origin", "https://evil.example")
            .json(&json!({"mode":"solo"}))
            .send()
            .await?
            .status(),
        403
    );
    let mut sessions = vec![];
    for guest in [false, true, true] {
        let launch: Value = kid(reqwest::Method::POST, "/api/apps/moon/launch")
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
    assert_ne!(sessions[0].0, sessions[1].0);
    assert_ne!(sessions[1].0, sessions[2].0);
    let game = |n: usize, method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", &sessions[n].0)
            .bearer_auth(&sessions[n].1)
            .header("origin", format!("https://{}", sessions[n].0))
    };
    db.edit(|d| {
        d.presence.remove("kid@example.com");
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
        !db.read()?.presence.contains_key("kid@example.com"),
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
        db.read()?.presence.contains_key("kid@example.com"),
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
        kid(reqwest::Method::PUT, "/api/apps/moon/reaction")
            .json(&json!({"emoji":"🚀","text":"I loved the stars!"}))
            .send()
            .await?
            .status(),
        200
    );
    let list: Value = adult(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(
        list["apps"][0]["reactions"][0]["text"],
        "I loved the stars!"
    );
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
    let updated: Value = kid(reqwest::Method::POST, "/api/apps/moon/launch")
        .json(&json!({"mode":"solo"}))
        .send()
        .await?
        .json()
        .await?;
    let updated_url = reqwest::Url::parse(updated["url"].as_str().unwrap())?;
    let tags: Value = kid(reqwest::Method::GET, "/api/library")
        .send()
        .await?
        .json()
        .await?;
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
    platform::member(&cfg, "kid@example.com", "Kid", false, false)?;
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
        kid(reqwest::Method::GET, "/api/library")
            .send()
            .await?
            .status(),
        403
    );
    adult(
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
        db.save_read(&platform::hash("kid@example.com"), "moon", "progress")?
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
            if h.get("cookie").is_some_and(|c| c == "member=adult") {
                (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "adult@example.com")],
                )
            } else {
                (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")])
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
    platform::member(&cfg, "adult@example.com", "Adult", true, true)?;
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
        .header("cookie", "member=adult")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(me["game_ports_admitted"], true);
    assert_eq!(received.await?, "192.0.2.10\n");
    drop(helper);
    // The rate-limit cache avoids a subprocess/helper operation on every asset/request.
    let me: Value = request()
        .header("cookie", "member=adult")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(me["game_ports_admitted"], true);
    Ok(())
}
