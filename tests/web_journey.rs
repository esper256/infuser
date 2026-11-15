use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    http::{HeaderMap, StatusCode, Uri},
    routing::get,
};
use infuser::{caddy, config::Config, controller::Controller, docker::Docker};
use std::{process::Stdio, sync::Arc, time::Duration};

struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
#[ignore = "requires CADDY_BIN pointing to a Caddy 2.10+ executable"]
async fn authenticated_static_game_navigation_assets_and_backend() -> Result<()> {
    let binary = std::env::var("CADDY_BIN").context("set CADDY_BIN")?;
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("index.html"), "<h1>Moon game</h1>")?;
    std::fs::write(dir.path().join("game.wasm"), b"\0asm\x01\0\0\0")?;
    let auth_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let auth_port = auth_listener.local_addr()?.port();
    let auth = Router::new().route(
        "/oauth2/auth",
        get(|headers: HeaderMap| async move {
            if headers.get("cookie").is_some_and(|v| v == "member=yes") {
                StatusCode::ACCEPTED
            } else {
                StatusCode::UNAUTHORIZED
            }
        }),
    );
    let _auth = Task(tokio::spawn(async move {
        axum::serve(auth_listener, auth).await.unwrap();
    }));
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let backend_port = backend_listener.local_addr()?.port();
    let backend = Router::new().fallback(|uri: Uri, headers: HeaderMap| async move {
        assert!(
            headers.get("cookie").is_none(),
            "site cookie must not reach game server"
        );
        uri.to_string()
    });
    let _backend = Task(tokio::spawn(async move {
        axum::serve(backend_listener, backend).await.unwrap();
    }));
    let config: Config = toml::from_str(&format!(
        r#"
[controller]
state_dir = "{}/state"
port_start = {backend_port}
port_end = {backend_port}
[games.moon]
image = "sha256:{}"
container_port = 7000
web = true
[sites.moon]
root = "{}"
spa = true
cross_origin_isolation = true
"#,
        dir.path().display(),
        "a".repeat(64),
        dir.path().display()
    ))?;
    let c = Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = reservation.local_addr()?.port();
    let mut text = String::from("{\n admin off\n auto_https off\n}\n");
    text += &caddy::render(&config, &c.snapshot(), false)?;
    for host in [
        "moon.games.example.com",
        "moon-server.games.example.com",
        "games.example.com",
    ] {
        text = text.replace(
            &format!("\n{host} {{"),
            &format!("\nhttp://{host}:{port} {{"),
        );
    }
    text = text.replace("127.0.0.1:4180", &format!("127.0.0.1:{auth_port}"));
    let path = dir.path().join("Caddyfile");
    std::fs::write(&path, text)?;
    drop(reservation);
    let mut child = tokio::process::Command::new(binary)
        .args(["run", "--adapter", "caddyfile", "--config"])
        .arg(path)
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let http = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .is_ok()
        {
            break;
        }
        ensure!(
            child.try_wait()?.is_none(),
            "Caddy exited before becoming ready"
        );
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Caddy did not start"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let base = format!("http://127.0.0.1:{port}");
    let request = |path: &str| {
        http.get(format!("{base}{path}"))
            .header("Host", "moon.games.example.com")
    };
    ensure!(
        request("/").send().await?.status() == 401,
        "game accessible without authentication"
    );
    let page = request("/levels/one")
        .header("Cookie", "member=yes")
        .header("Accept", "text/html")
        .send()
        .await?;
    ensure!(page.status() == 200, "SPA navigation failed");
    ensure!(
        page.headers()["Cross-Origin-Embedder-Policy"] == "require-corp",
        "missing threaded export header"
    );
    ensure!(page.text().await?.contains("Moon game"), "wrong page");
    let wasm = request("/game.wasm")
        .header("Cookie", "member=yes")
        .send()
        .await?;
    ensure!(
        wasm.status() == 200 && wasm.headers()["Content-Type"] == "application/wasm",
        "WASM serving failed"
    );
    ensure!(
        request("/missing.wasm")
            .header("Cookie", "member=yes")
            .header("Accept", "text/html")
            .send()
            .await?
            .status()
            == 404,
        "SPA fallback swallowed a missing asset"
    );
    let response = http
        .get(format!("{base}/match/join?room=one"))
        .header("Host", "moon-server.games.example.com")
        .header("Cookie", "member=yes")
        .send()
        .await?;
    ensure!(
        response.text().await? == "/match/join?room=one",
        "backend path was rewritten"
    );
    child.kill().await?;
    Ok(())
}
