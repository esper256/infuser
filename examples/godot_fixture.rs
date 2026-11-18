//! Opt-in local browser fixture, never a production authentication mode.
//! cargo run --example godot_fixture -- examples/godot-web/export
use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build},
    portal,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let export = std::env::args()
        .nth(1)
        .context("export directory required")?;
    let dir = tempfile::tempdir()?;
    let auth = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let auth_url = format!("http://{}/auth", auth.local_addr()?);
    tokio::spawn(async move {
        axum::serve(
            auth,
            Router::new().route(
                "/auth",
                get(|h: HeaderMap| async move {
                    if h.get("cookie").is_some_and(|v| v == "member=admin") {
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
            ),
        )
        .await
        .unwrap();
    });
    let mut cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={auth_url:?}\ndisk_reserve_mb=64",
        dir.path().display().to_string()
    ))?;
    if let Some(index) = args.iter().position(|a| a == "--domain") {
        cfg.controller.domain = args.get(index + 1).context("domain missing")?.clone();
    }
    platform::member(&cfg, "admin@example.com", "Test Player", true, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let project = args
        .iter()
        .position(|a| a == "--project")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("examples/godot-web");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(
        Path::new(project).join("infuser.game.json"),
    )?)?;
    manifest["schema_version"] = json!(1);
    manifest["app_id"] = json!("lantern-lake");
    manifest
        .as_object_mut()
        .unwrap()
        .remove("cross_origin_isolation");
    // This fixture tests real cloud/guest journeys without pretending to provide TURN.
    manifest["sdk"]["required_capabilities"] = json!(["identity.v1", "cloud_checkpoints.v1"]);
    let dest = dir.path().join("content/lantern-lake/test");
    let mut files = BTreeMap::new();
    copy(Path::new(&export), Path::new(&export), &dest, &mut files)?;
    if let Some(index) = args.iter().position(|a| a == "--native-linux") {
        let native = infuser::native::pack(
            &[(
                "linux-x86_64".into(),
                args.get(index + 1)
                    .context("native directory missing")?
                    .into(),
            )],
            true,
        )?;
        copy(native.path(), native.path(), &dest, &mut files)?;
    }
    p.db.edit(|d| {
        d.builds.insert(
            "lantern-lake/test".into(),
            Build {
                manifest: serde_json::from_value(manifest)?,
                server: None,
                spa: false,
                cross_origin_isolation: std::env::args().any(|a| a == "--threaded"),
                ready: true,
                created: now(),
                files: files.into(),
            },
        );
        d.apps.insert(
            "lantern-lake".into(),
            App {
                publication: None,
                build: "test".into(),
                updated: now(),
                listed: true,
            },
        );
        Ok(())
    })?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:17877").await?;
    println!("Godot save fixture ready on 127.0.0.1:17877 (temporary data; no real accounts)");
    axum::serve(listener, portal::router(p)).await?;
    Ok(())
}

fn copy(root: &Path, path: &Path, dest: &Path, files: &mut BTreeMap<String, u64>) -> Result<()> {
    for e in std::fs::read_dir(path)? {
        let e = e?;
        let path = e.path();
        if e.file_type()?.is_dir() {
            copy(root, &path, dest, files)?;
        } else if e.file_type()?.is_file() {
            let relative = path.strip_prefix(root)?;
            let to = dest.join(relative);
            std::fs::create_dir_all(to.parent().unwrap())?;
            let size = std::fs::copy(&path, to)?;
            files.insert(relative.to_string_lossy().replace('\\', "/"), size);
        }
    }
    Ok(())
}
