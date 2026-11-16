//! Local developer harness using production auth proofs, admission and SDK routes.
//! Stage examples/dedicated-node first. Never expose this synthetic-login harness.
use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Engine, Health},
    platform::{self, App, Build},
    portal,
    store::Instance,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

struct Runtime {
    node: String,
    project: PathBuf,
    state: PathBuf,
    api: String,
    port: u16,
    child: Mutex<Option<(String, Child)>>,
}
#[async_trait]
impl Engine for Runtime {
    async fn preflight(&self) -> Result<()> {
        Ok(())
    }
    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }
    async fn ensure(&self, id: &str, instance: &Instance, owner: &str) -> Result<Health> {
        let mut child = self.child.lock().await;
        if child.is_none() {
            let process = Command::new(&self.node)
                .arg(self.project.join("server/server.js"))
                .env("PORT", self.port.to_string())
                .env("INFUSER_GAME_ID", id)
                .env("INFUSER_INSTANCE_ID", &instance.attempt)
                .env("INFUSER_API_URL", &self.api)
                .env("INFUSER_CONNECTION_ISSUER", "https://games.example.com")
                .env("INFUSER_DEV", "1")
                .env(
                    "INFUSER_CONNECTION_PUBLIC_KEY",
                    std::fs::read_to_string(
                        self.state.join(infuser::connections::PUBLIC_KEY_FILE),
                    )?,
                )
                .env(
                    "INFUSER_INSTANCE_TOKEN",
                    infuser::connections::instance_token(
                        &self.state,
                        owner,
                        id,
                        &instance.attempt,
                    )?,
                )
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()?;
            *child = Some((instance.attempt.clone(), process));
        }
        drop(child);
        self.health(instance, owner, id).await
    }
    async fn health(&self, instance: &Instance, _: &str, _: &str) -> Result<Health> {
        let mut child = self.child.lock().await;
        let Some((id, process)) = child.as_mut() else {
            return Ok(Health::Missing);
        };
        if id != &instance.attempt || process.try_wait()?.is_some() {
            return Ok(Health::Failed);
        }
        drop(child);
        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{}/healthz", self.port))
            .timeout(std::time::Duration::from_millis(300))
            .send()
            .await;
        Ok(if response.is_ok_and(|r| r.status().is_success()) {
            Health::Ready
        } else {
            Health::Starting
        })
    }
    async fn remove(&self, _: &str, _: &Instance, _: &str) -> Result<()> {
        if let Some((_, mut child)) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
        Ok(())
    }
    async fn web_address(&self, _: &str, _: &Instance, _: &str) -> Result<std::net::SocketAddr> {
        Ok(([127, 0, 0, 1], self.port).into())
    }
}
fn copy(root: &Path, path: &Path, dest: &Path, files: &mut BTreeMap<String, u64>) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            copy(root, &path, dest, files)?;
        } else if entry.file_type()?.is_file() {
            let relative = path.strip_prefix(root)?;
            let target = dest.join(relative);
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::copy(&path, &target)?;
            files.insert(relative.to_string_lossy().into(), path.metadata()?.len());
        }
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let project = std::fs::canonicalize(args.get(1).context("staged project directory required")?)?;
    let output = args.get(2).context("private ready-file path required")?;
    let directory = tempfile::tempdir()?;
    let certificate = rcgen::generate_simple_self_signed(vec![
        "games.example.com".into(),
        "*.games.example.com".into(),
        "localhost".into(),
    ])?;
    let cert = directory.path().join("cert.pem");
    let key = directory.path().join("key.pem");
    infuser::private_file::replace(&cert, certificate.cert.pem().as_bytes())?;
    infuser::private_file::replace(&key, certificate.signing_key.serialize_pem().as_bytes())?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api = format!("http://{}", listener.local_addr()?);
    let backend = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = backend.local_addr()?.port();
    drop(backend);
    let auth = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let auth_url = format!("http://{}/auth", auth.local_addr()?);
    tokio::spawn(async move {
        axum::serve(
            auth,
            Router::new().route(
                "/auth",
                get(|h: HeaderMap| async move {
                    let member = h
                        .get("cookie")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.split(';').find_map(|p| p.trim().strip_prefix("member=")));
                    match member {
                        Some("river") => (
                            StatusCode::ACCEPTED,
                            [("X-Auth-Request-Email", "river@example.com")],
                        ),
                        Some("sky") => (
                            StatusCode::ACCEPTED,
                            [("X-Auth-Request-Email", "sky@example.com")],
                        ),
                        Some("adult") => (
                            StatusCode::ACCEPTED,
                            [("X-Auth-Request-Email", "adult@example.com")],
                        ),
                        _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={auth_url:?}\ndisk_reserve_mb=64\nlease_seconds=30",
        directory.path().to_string_lossy()
    ))?;
    for (email, name, admin) in [
        ("river@example.com", "River", false),
        ("sky@example.com", "Sky", false),
        ("adult@example.com", "Test adult", true),
    ] {
        platform::member(&cfg, email, name, admin, true)?;
    }
    let engine = Arc::new(Runtime {
        node: std::env::var("INFUSER_TEST_NODE").unwrap_or_else(|_| "node".into()),
        project: project.clone(),
        state: directory.path().into(),
        api: api.clone(),
        port,
        child: Mutex::new(None),
    });
    let controller = Arc::new(Controller::open(cfg, engine.clone())?);
    let p = portal::Platform::open(controller.clone())?;
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(project.join("infuser.game.json"))?)?;
    let server = serde_json::from_value(
        json!({"image":format!("sha256:{}","a".repeat(64)),"container_port":port,"web":true,"memory_mb":128,"cpu_millis":250,"idle_seconds":1}),
    )?;
    manifest["schema_version"] = json!(1);
    manifest["app_id"] = json!("dedicated-lab");
    for key in ["web_dir", "server"] {
        manifest.as_object_mut().unwrap().remove(key);
    }
    let destination = directory.path().join("content/dedicated-lab/one");
    let mut files = BTreeMap::new();
    copy(
        &project.join("public"),
        &project.join("public"),
        &destination,
        &mut files,
    )?;
    let html = std::fs::read_to_string(destination.join("index.html"))?
        + "\n<script type=module src=\"/_infuser/sdk/v1.js\"></script>";
    std::fs::write(destination.join("index.html"), &html)?;
    files.insert("index.html".into(), html.len() as u64);
    controller.register("dedicated-lab", server)?;
    let token = p.db.edit(|d| {
        let build = Build {
            manifest: serde_json::from_value(manifest)?,
            server: Some(controller.snapshot().games["dedicated-lab"].spec.clone()),
            spa: false,
            cross_origin_isolation: false,
            ready: true,
            created: now(),
            files,
        };
        build.manifest.validate()?;
        d.builds.insert("dedicated-lab/one".into(), build);
        d.apps.insert(
            "dedicated-lab".into(),
            App {
                build: "one".into(),
                listed: true,
                updated: now(),
            },
        );
        d.owners
            .insert("dedicated-lab".into(), "river@example.com".into());
        infuser::game_urls::assign(d, "dedicated-lab", "Dedicated Server Lab");
        Ok(platform::credential(d, "dedicated-lab", "publish", "fixture")?.1)
    })?;
    let publisher = token;
    infuser::private_file::replace(Path::new(output),serde_json::to_vec(&json!({"api":api,"app_id":"dedicated-lab","token":publisher,"state_dir":directory.path(),"backend_port":port,"cert":cert,"key":key}))?.as_slice())?;
    let tick_p = p.clone();
    let tick_controller = controller.clone();
    let ticker = tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
        loop {
            tick.tick().await;
            let _ = tick_controller.reconcile(now()).await;
            let _ = tick_p.activate_pending();
        }
    });
    println!("Dedicated development API: {api} (synthetic accounts; temporary data)");
    // The synthetic backend calls loopback directly; browsers retain their game
    // host through the HTTPS test proxy. Node fetch intentionally owns Host.
    let router = portal::router(p).layer(axum::middleware::from_fn(
        |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request
                .headers()
                .get("host")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("127.0.0.1:"))
            {
                request
                    .headers_mut()
                    .insert("host", "games.example.com".parse().unwrap());
            }
            next.run(request).await
        },
    ));
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    ticker.abort();
    if let Some((_, mut child)) = engine.child.lock().await.take() {
        let _ = child.kill().await;
    }
    Ok(())
}
