use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use axum::{Router, extract::State, response::IntoResponse, routing::get};
use infuser::{
    rollback,
    updater::{self, Artifact, Release, Service, Settings},
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const CANDIDATE: &[u8] = b"#!/bin/sh\nexit 0\n";

struct Host {
    install: PathBuf,
    platform: PathBuf,
    snapshots: PathBuf,
    bad: String,
    events: Mutex<Vec<String>>,
    fail_snapshot: bool,
    disk_full: bool,
}

#[async_trait]
impl Service for Host {
    async fn lock_activation(&self) -> Result<bool> {
        Ok(true)
    }
    async fn preserve_state(&self, release: &str) -> Result<()> {
        let current = fs::read_link(self.install.join("current"))?;
        ensure!(
            current.file_name().and_then(|name| name.to_str()) == Some(release),
            "snapshot must be taken before current moves"
        );
        if self.disk_full {
            return Err(rollback::DiskLimit.into());
        }
        if self.fail_snapshot {
            bail!("snapshot failed");
        }
        self.events
            .lock()
            .unwrap()
            .push(format!("preserve:{release}"));
        rollback::capture(&self.platform, &self.snapshots, release)?;
        Ok(())
    }
    async fn stop(&self) -> Result<bool> {
        self.events.lock().unwrap().push("stop".into());
        Ok(true)
    }
    async fn restore_state(&self, release: &str) -> Result<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("restore:{release}"));
        rollback::restore(&self.snapshots.join(release), &self.platform)?;
        Ok(())
    }
    async fn check(&self, binary: &Path) -> Result<()> {
        ensure!(fs::read(binary)? == CANDIDATE, "wrong download");
        Ok(())
    }
    async fn restart(&self) -> Result<()> {
        let name = fs::read_link(self.install.join("current"))?
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        self.events.lock().unwrap().push(format!("restart:{name}"));
        let connection = Connection::open(self.platform.join("platform.sqlite"))?;
        connection.execute("INSERT INTO marker VALUES(?1)", [name])?;
        Ok(())
    }
    async fn healthy(&self, commit: &str) -> Result<bool> {
        let path = fs::read_link(self.install.join("current"))?;
        Ok(path.file_name().unwrap() == commit && commit != self.bad)
    }
}

struct Layout {
    _root: tempfile::TempDir,
    install: PathBuf,
    platform: PathBuf,
    updater: PathBuf,
    config: Settings,
    source: String,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Layout {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn layout(manifest: Arc<Mutex<Release>>, initial: &str) -> Result<Layout> {
    let root = tempfile::tempdir()?;
    let platform = root.path().join("platform");
    fs::create_dir(&platform)?;
    Connection::open(platform.join("platform.sqlite"))?
        .execute_batch("CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('seed');")?;
    let install = root.path().join("install");
    fs::create_dir_all(install.join("releases").join(initial))?;
    symlink(
        install.join("releases").join(initial),
        install.join("current"),
    )?;
    let controller = root.path().join("controller.toml");
    fs::write(
        &controller,
        format!("[controller]\nstate_dir={platform:?}\n[future]\nversion=2\n"),
    )?;
    let updater = root.path().join("updater");
    let app = Router::new()
        .route(
            "/latest/download/release.json",
            get(|State(manifest): State<Arc<Mutex<Release>>>| async move {
                axum::Json(manifest.lock().unwrap().clone())
            }),
        )
        .fallback(|| async { CANDIDATE.into_response() })
        .with_state(manifest);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let source = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Settings {
        repository: "example/infuser".into(),
        install_dir: install.clone(),
        state_dir: updater.clone(),
        controller_config: controller,
        service: "infuser.service".into(),
        health_url: "http://127.0.0.1/healthz".into(),
    };
    Ok(Layout {
        _root: root,
        install,
        platform,
        updater,
        config,
        source,
        server,
    })
}

fn release(commit: &str) -> Release {
    Release {
        commit: commit.into(),
        host: Vec::new(),
        artifacts: BTreeMap::from([(
            "linux-amd64".into(),
            Artifact {
                file: "infuser-linux-amd64".into(),
                sha256: Sha256::digest(CANDIDATE)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            },
        )]),
    }
}

fn count(db: &Path, value: &str) -> Result<i64> {
    let connection = Connection::open(db)?;
    Ok(
        connection.query_row("SELECT COUNT(*) FROM marker WHERE v=?1", [value], |row| {
            row.get(0)
        })?,
    )
}

#[tokio::test]
async fn failed_activation_restores_the_snapshot_taken_before_the_switch() -> Result<()> {
    let initial = "a".repeat(40);
    let good = "b".repeat(40);
    let bad = "c".repeat(40);
    let manifest = Arc::new(Mutex::new(release(&good)));
    let layout = layout(manifest.clone(), &initial).await?;
    let host = Host {
        install: layout.install.clone(),
        platform: layout.platform.clone(),
        snapshots: layout.updater.join("rollback"),
        bad: bad.clone(),
        events: Mutex::new(Vec::new()),
        fail_snapshot: false,
        disk_full: false,
    };
    let http = reqwest::Client::new();
    let first =
        updater::update(&layout.config, &layout.source, "linux-amd64", &http, &host).await?;
    assert!(
        first.contains("Activated"),
        "first update did not activate: {first}"
    );
    manifest.lock().unwrap().commit = bad.clone();
    let error = updater::update(&layout.config, &layout.source, "linux-amd64", &http, &host)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("rolled back"));
    let events = host.events.lock().unwrap().join(" ");
    let preserve_good = events.find(&format!("preserve:{good}")).unwrap();
    let restart_bad = events.find(&format!("restart:{bad}")).unwrap();
    let stop = events.find("stop").unwrap();
    let restore_good = events.find(&format!("restore:{good}")).unwrap();
    let restart_good = events.rfind(&format!("restart:{good}")).unwrap();
    assert!(preserve_good < restart_bad && restart_bad < stop && stop < restore_good);
    assert!(restore_good < restart_good);
    assert_eq!(
        fs::read_link(layout.install.join("current"))?
            .file_name()
            .unwrap(),
        good.as_str()
    );
    let db = layout.platform.join("platform.sqlite");
    assert_eq!(
        count(&db, &bad)?,
        0,
        "failed release writes must not remain"
    );
    assert_eq!(count(&db, &good)?, 2);
    assert!(!layout.updater.join("rollback").join(&initial).exists());
    assert!(layout.updater.join("rollback").join(&good).is_dir());
    assert_eq!(
        fs::read_to_string(layout.updater.join("failed-release"))?,
        bad
    );
    Ok(())
}

#[tokio::test]
async fn snapshot_failure_leaves_the_running_release_in_place() -> Result<()> {
    let initial = "a".repeat(40);
    let good = "b".repeat(40);
    let manifest = Arc::new(Mutex::new(release(&good)));
    let layout = layout(manifest, &initial).await?;
    let host = Host {
        install: layout.install.clone(),
        platform: layout.platform.clone(),
        snapshots: layout.updater.join("rollback"),
        bad: String::new(),
        events: Mutex::new(Vec::new()),
        fail_snapshot: true,
        disk_full: false,
    };
    let error = updater::update(
        &layout.config,
        &layout.source,
        "linux-amd64",
        &reqwest::Client::new(),
        &host,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("current version unchanged"));
    assert_eq!(
        fs::read_link(layout.install.join("current"))?
            .file_name()
            .unwrap(),
        initial.as_str()
    );
    assert!(host.events.lock().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn tight_disk_skips_activation_without_moving_current() -> Result<()> {
    let initial = "a".repeat(40);
    let good = "b".repeat(40);
    let manifest = Arc::new(Mutex::new(release(&good)));
    let layout = layout(manifest, &initial).await?;
    let host = Host {
        install: layout.install.clone(),
        platform: layout.platform.clone(),
        snapshots: layout.updater.join("rollback"),
        bad: String::new(),
        events: Mutex::new(Vec::new()),
        fail_snapshot: false,
        disk_full: true,
    };
    let message = updater::update(
        &layout.config,
        &layout.source,
        "linux-amd64",
        &reqwest::Client::new(),
        &host,
    )
    .await?;
    assert!(message.contains("Not enough free disk"));
    assert_eq!(
        fs::read_link(layout.install.join("current"))?
            .file_name()
            .unwrap(),
        initial.as_str()
    );
    assert!(!layout.install.join("previous").exists());
    Ok(())
}

struct Operator {
    stops: Mutex<usize>,
}

#[async_trait]
impl Service for Operator {
    async fn lock_activation(&self) -> Result<bool> {
        Ok(true)
    }
    async fn stop(&self) -> Result<bool> {
        *self.stops.lock().unwrap() += 1;
        Ok(true)
    }
    async fn check(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    async fn restart(&self) -> Result<()> {
        Ok(())
    }
    async fn healthy(&self, commit: &str) -> Result<bool> {
        Ok(!commit.is_empty())
    }
}

#[tokio::test]
async fn operator_rollback_restores_the_previous_release_and_database() -> Result<()> {
    let initial = "a".repeat(40);
    let good = "b".repeat(40);
    let manifest = Arc::new(Mutex::new(release(&good)));
    let layout = layout(manifest, &initial).await?;
    let host = Host {
        install: layout.install.clone(),
        platform: layout.platform.clone(),
        snapshots: layout.updater.join("rollback"),
        bad: String::new(),
        events: Mutex::new(Vec::new()),
        fail_snapshot: false,
        disk_full: false,
    };
    updater::update(
        &layout.config,
        &layout.source,
        "linux-amd64",
        &reqwest::Client::new(),
        &host,
    )
    .await?;
    let db = layout.platform.join("platform.sqlite");
    assert_eq!(count(&db, &good)?, 1);
    let snapshot = layout.updater.join("rollback").join(&initial);
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_infuser"))
        .args(["rollback-verify", snapshot.to_str().unwrap()])
        .output()
        .await?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)?.contains(&initial));
    let operator = Operator {
        stops: Mutex::new(0),
    };
    let message = updater::rollback(&layout.config, &operator).await?;
    assert!(message.contains(&initial));
    assert_eq!(*operator.stops.lock().unwrap(), 1);
    assert_eq!(
        fs::read_link(layout.install.join("current"))?
            .file_name()
            .unwrap(),
        initial.as_str()
    );
    assert_eq!(count(&db, "seed")?, 1);
    assert_eq!(count(&db, &good)?, 0);
    assert_eq!(
        fs::read_to_string(layout.updater.join("failed-release"))?,
        good
    );
    let error = updater::rollback(&layout.config, &operator)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already the previous release"));
    assert_eq!(*operator.stops.lock().unwrap(), 1);
    Ok(())
}
