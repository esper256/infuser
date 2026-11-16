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

#[tokio::test]
#[ignore = "requires CADDY_BIN pointing to Caddy 2.10+"]
async fn production_template_preserves_auth_headers_paths_and_websocket_frames() -> Result<()> {
    use axum::{extract::ws::{WebSocketUpgrade,Message},response::IntoResponse};
    use futures_util::{SinkExt,StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let dir=tempfile::tempdir()?;
    let backend_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let backend_address=backend_listener.local_addr()?;
    fn clean(headers:&HeaderMap) {
        for name in ["cookie","authorization","x-infuser-port","x-infuser-upstream"] {
            assert!(!headers.contains_key(name),"backend received {name}");
        }
    }
    let backend=Router::new().route("/socket",get(|headers:HeaderMap, ws:WebSocketUpgrade| async move {
        clean(&headers);
        ws.on_upgrade(|mut socket|async move {
            while let Some(Ok(message))=socket.next().await {
                if matches!(message,Message::Text(_)|Message::Binary(_)) && socket.send(message).await.is_err() {break;}
            }
        })
    })).fallback(|headers:HeaderMap, uri:Uri|async move {
        clean(&headers);([("set-cookie","backend=must-not-escape")],uri.to_string())
    });
    let _backend=Task(tokio::spawn(async move {axum::serve(backend_listener,backend).await.unwrap();}));
    let auth_listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let auth_address=auth_listener.local_addr()?;
    let api=Router::new().route("/_infuser/proxy-auth",get(move |headers:HeaderMap|async move {
        assert!(!headers.contains_key("x-infuser-upstream"));
        assert!(!headers.contains_key("x-infuser-port"));
        if headers.get("cookie").is_none_or(|v|v!="game=authorized") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        ([ ("x-infuser-upstream",backend_address.to_string()),("x-infuser-port","12345".into()) ],"").into_response()
    })).fallback(|headers:HeaderMap,uri:Uri|async move {
        assert_eq!(headers["x-infuser-client-ip"],"127.0.0.1");
        uri.to_string()
    });
    let _api=Task(tokio::spawn(async move {axum::serve(auth_listener,api).await.unwrap();}));
    let port_reservation=std::net::TcpListener::bind("127.0.0.1:0")?;
    let port=port_reservation.local_addr()?.port();
    // Exercise the shipped template, changing only TLS/listen/upstream settings.
    let tls="    tls {\n        dns porkbun {\n            api_key {env.PORKBUN_API_KEY}\n            api_secret_key {env.PORKBUN_API_SECRET_KEY}\n        }\n    }\n";
    let source=include_str!("../deploy/Caddyfile.example");
    ensure!(source.contains(tls),"adjust the fixture's TLS substitution after template changes");
    let text=format!("{{\n admin off\n auto_https off\n}}\n{}",source.replace(tls,"")
        .replace("\nexample.com {",&format!("\nhttp://example.com:{port} {{"))
        .replace("\n*.example.com {",&format!("\nhttp://*.example.com:{port} {{"))
        .replace("127.0.0.1:7777",&auth_address.to_string()));
    let path=dir.path().join("Caddyfile");std::fs::write(&path,text)?;drop(port_reservation);
    let log=std::fs::File::create(dir.path().join("caddy.log"))?;
    let mut caddy=tokio::process::Command::new(std::env::var("CADDY_BIN")?)
        .args(["run","--adapter","caddyfile","--config"]).arg(path)
        .env("XDG_DATA_HOME",dir.path()).env("XDG_CONFIG_HOME",dir.path())
        .stdout(Stdio::null()).stderr(log).kill_on_drop(true).spawn()?;
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,port)).await.is_ok(){break;}
            ensure!(caddy.try_wait()?.is_none(),"Caddy failed: {}",std::fs::read_to_string(dir.path().join("caddy.log"))?);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_,anyhow::Error>(())
    }).await??;
    let http=reqwest::Client::new();let base=format!("http://127.0.0.1:{port}");
    let request=|host:&str,path:&str|http.get(format!("{base}{path}")).header("host",host)
        .header("x-infuser-upstream","127.0.0.1:1").header("x-infuser-port","1").header("x-infuser-client-ip","203.0.113.1");
    for host in ["example.com","pond.example.com"] {
        assert_eq!(request(host,"/healthz").send().await?.status(),404);
        assert_eq!(request(host,"/v1/apps").send().await?.status(),404);
    }
    assert_eq!(request("pond.example.com","/api/me").send().await?.status(),404);
    assert_eq!(request("example.com","/api/me").send().await?.text().await?,"/api/me");
    assert_eq!(request("pond.example.com","/game.wasm").send().await?.text().await?,"/game.wasm");
    assert_eq!(request("pond.example.com","/_infuser/server/play").send().await?.status(),401);
    let response=request("pond.example.com","/_infuser/server/match/join?room=one")
        .header("cookie","game=authorized").header("authorization","Bearer should-be-stripped").send().await?;
    assert!(!response.headers().contains_key("set-cookie"));
    assert_eq!(response.text().await?,"/match/join?room=one");
    let mut handshake=format!("ws://127.0.0.1:{port}/_infuser/server/socket").into_client_request()?;
    handshake.headers_mut().insert("host","pond.example.com".parse()?);
    assert!(tokio_tungstenite::connect_async(handshake.clone()).await.is_err());
    handshake.headers_mut().insert("cookie","game=authorized".parse()?);
    handshake.headers_mut().insert("authorization","Bearer should-be-stripped".parse()?);
    handshake.headers_mut().insert("x-infuser-upstream","127.0.0.1:1".parse()?);
    let (mut socket,_)=tokio_tungstenite::connect_async(handshake).await?;
    for frame in [tokio_tungstenite::tungstenite::Message::Text("unchanged".into()),tokio_tungstenite::tungstenite::Message::Binary(vec![0,255,1].into())] {
        socket.send(frame.clone()).await?;
        assert_eq!(tokio::time::timeout(Duration::from_secs(2),socket.next()).await?.unwrap()?,frame);
    }
    socket.close(None).await?;caddy.kill().await?;
    Ok(())
}
