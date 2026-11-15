//! Run the shipped publisher against the real API over verified local TLS.
//! The same journey runs on Linux and both macOS architectures.
use anyhow::{Result, ensure};
use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Output, Stdio},
    sync::Arc,
};
use tokio::{io::AsyncWriteExt, net::TcpListener, process::Command};
use tokio_rustls::{TlsAcceptor, rustls};

struct TlsListener(TcpListener, TlsAcceptor);
impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (socket, addr) = self.0.accept().await.unwrap();
            // A rejected/untrusted certificate must not stop subsequent clients.
            if let Ok(socket) = self.1.accept(socket).await {
                return (socket, addr);
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.0.local_addr()
    }
}
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn cli(cwd: &Path, args: &[&str], input: Option<&str>) -> Result<Output> {
    let binary = std::env::var_os("INFUSER_PUBLISHER_BIN")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_infuser-publisher").into());
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    if let Some(input) = input {
        stdin.write_all(input.as_bytes()).await?;
    }
    drop(stdin);
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await??,
    )
}
fn succeeded(output: Output) -> Result<String> {
    ensure!(
        output.status.success(),
        "publisher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

#[tokio::test]
async fn connect_publish_update_existing_web_project() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        root.join("server").display().to_string()
    ))?;
    platform::member(&cfg, "kid@example.com", "Player", false, true)?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller)?;
    let db = p.db.clone();
    let token = infuser::store::secret();
    db.edit(|d| {
        d.owners
            .insert("existing-game".into(), "kid@example.com".into());
        d.setup_claims.insert(
            platform::hash(&token),
            platform::SetupClaim {
                app: "existing-game".into(),
                owner: "kid@example.com".into(),
                label: "Existing game".into(),
                expires: infuser::controller::now() + 3600,
            },
        );
        Ok(())
    })?;
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    std::fs::write(root.join("test-ca.pem"), cert.cert.pem())?;
    let tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
        )?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("https://localhost:{}", listener.local_addr()?.port());
    let public_url = url.clone();
    // Model the HTTPS ingress, with an ephemeral port instead of privileged 443.
    // Only its advertised origin changes; claims/uploads/publication use real handlers.
    let router = portal::router(p).layer(axum::middleware::from_fn(
        move |mut req: Request, next: Next| {
            let public_url = public_url.clone();
            async move {
                let claim = req.uri().path() == "/api/publisher/claim";
                req.headers_mut()
                    .insert("host", "games.example.com".parse().unwrap());
                let response = next.run(req).await;
                if claim && response.status().is_success() {
                    let (mut parts, body) = response.into_parts();
                    let bytes = axum::body::to_bytes(body, 16384).await.unwrap();
                    let mut connection: Value = serde_json::from_slice(&bytes).unwrap();
                    connection["api_url"] = json!(public_url);
                    parts.headers.remove("content-length");
                    Response::from_parts(
                        parts,
                        Body::from(serde_json::to_vec(&connection).unwrap()),
                    )
                } else {
                    response
                }
            }
        },
    ));
    let _server = Running(tokio::spawn(async move {
        axum::serve(
            TlsListener(listener, TlsAcceptor::from(Arc::new(tls))),
            router,
        )
        .await
        .unwrap();
    }));
    let code = format!("infuser_setup_{token}\n");
    let connection = ".local/existing game.credentials.json";
    let connect = [
        "connect",
        "--server",
        &url,
        "--app",
        "existing-game",
        "--output",
        connection,
    ];
    // TLS is never disabled just to make an upload work.
    assert!(!cli(root, &connect, Some(&code)).await?.status.success());
    assert!(!root.join(connection).exists());
    assert_eq!(db.read()?.setup_claims.len(), 1);
    let connect: Vec<_> = connect
        .into_iter()
        .chain(["--ca-cert", "test-ca.pem"])
        .collect();
    let output = succeeded(cli(root, &connect, Some(&code)).await?)?;
    assert!(!output.contains(&token));
    assert_eq!(
        std::fs::metadata(root.join(connection))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let original = std::fs::read(root.join(connection))?;
    assert!(!cli(root, &connect, Some(&code)).await?.status.success());
    assert_eq!(std::fs::read(root.join(connection))?, original);
    let mut replay = connect.clone();
    replay[6] = ".local/replayed.credentials.json";
    assert!(!cli(root, &replay, Some(&code)).await?.status.success());
    assert!(!root.join(replay[6]).exists());
    assert!(db.read()?.setup_claims.is_empty());

    let project = root.join("Already made game");
    let export = project.join("web export");
    std::fs::create_dir_all(export.join("assets"))?;
    std::fs::write(
        project.join("package.json"),
        r#"{"devDependencies":{"vite":"*","three":"*"}}"#,
    )?;
    std::fs::write(
        project.join("infuser.game.json"),
        serde_json::to_vec(&json!({
            "title":"Already Made", "play_modes":["solo"],
            "sdk":{"major":1,"api_major":1,"required_capabilities":["identity.v1"]}
        }))?,
    )?;
    std::fs::write(
        export.join("index.html"),
        "<html><head></head><body>Game</body></html>",
    )?;
    std::fs::write(export.join("assets/level one@2.js"), b"first version")?;
    let publish = [
        "publish",
        connection,
        "Already made game",
        "--web-dir",
        "web export",
        "--ca-cert",
        "test-ca.pem",
    ];
    assert!(succeeded(cli(root, &publish, None).await?)?.contains("Ready in the Library"));
    let first = db.read()?.apps["existing-game"].build.clone();
    let content = cfg.controller.state_dir.join("content/existing-game");
    assert!(
        std::fs::read_to_string(content.join(&first).join("index.html"))?
            .contains("/_infuser/sdk/v1.js")
    );
    assert_eq!(
        std::fs::read(content.join(&first).join("assets/level one@2.js"))?,
        b"first version"
    );
    assert!(!std::fs::read_to_string(export.join("index.html"))?.contains("/_infuser"));
    std::fs::write(export.join("assets/level one@2.js"), b"second version")?;
    succeeded(cli(root, &publish, None).await?)?;
    let data = db.read()?;
    let second = data.apps["existing-game"].build.clone();
    assert_ne!(first, second);
    assert_eq!(data.apps.len(), 1);
    assert_eq!(data.credentials.len(), 1);
    assert_eq!(data.builds.len(), 2);
    assert_eq!(
        data.builds[&format!("existing-game/{second}")]
            .manifest
            .title,
        "Already Made"
    );
    assert_eq!(
        std::fs::read(content.join(&second).join("assets/level one@2.js"))?,
        b"second version"
    );
    assert_eq!(
        std::fs::read(content.join(&first).join("assets/level one@2.js"))?,
        b"first version"
    );
    // A mistakenly exported credential cannot replace the good release.
    std::fs::write(export.join("oops.credentials.json"), b"private")?;
    assert!(!cli(root, &publish, None).await?.status.success());
    assert_eq!(db.read()?.apps["existing-game"].build, second);
    assert_eq!(std::fs::read(root.join(connection))?, original);
    Ok(())
}
