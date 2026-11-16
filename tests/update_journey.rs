use anyhow::{Result, ensure};
use async_trait::async_trait;
use axum::{Router, extract::State, response::IntoResponse, routing::get};
use infuser::updater::{self, Artifact, Release, Service, Settings};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

struct TestService {
    install: PathBuf,
    bad: String,
    restarts: Mutex<usize>,
    busy: std::sync::atomic::AtomicBool,
    rollback_allowed: std::sync::atomic::AtomicBool,
}

const CANDIDATE: &[u8] = b"#!/bin/sh\nexit 0\n";
#[async_trait]
impl Service for TestService {
    async fn ready_to_update(&self) -> Result<bool> {
        Ok(!self.busy.load(std::sync::atomic::Ordering::Relaxed))
    }
    async fn check(&self, binary: &Path) -> Result<()> {
        ensure!(fs::read(binary)? == CANDIDATE, "wrong download");
        ensure!(
            fs::metadata(binary.parent().unwrap())?.permissions().mode() & 0o055 == 0o055,
            "release directory must be traversable by the daemon user"
        );
        ensure!(
            tokio::process::Command::new(binary)
                .status()
                .await?
                .success(),
            "downloaded candidate could not execute"
        );
        Ok(())
    }
    async fn restart(&self) -> Result<()> {
        *self.restarts.lock().unwrap() += 1;
        Ok(())
    }
    async fn check_rollback(&self, _binary: &Path) -> Result<()> {
        ensure!(
            self.rollback_allowed
                .load(std::sync::atomic::Ordering::Relaxed),
            "incompatible identity state"
        );
        Ok(())
    }
    async fn healthy(&self, commit: &str) -> Result<bool> {
        let path = fs::read_link(self.install.join("current"))?;
        Ok(path.file_name().unwrap() == commit && commit != self.bad)
    }
}

#[tokio::test]
async fn release_download_activation_failure_and_interrupted_rollback() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let initial = "a".repeat(40);
    let good = "b".repeat(40);
    let bad = "c".repeat(40);
    let install = dir.path().join("install");
    fs::create_dir_all(install.join("releases").join(&initial))?;
    symlink(
        install.join("releases").join(&initial),
        install.join("current"),
    )?;
    let state = dir.path().join("state");
    let manifest = Arc::new(Mutex::new(Release {
        commit: good.clone(),
        artifacts: BTreeMap::from([(
            "linux-amd64".into(),
            Artifact {
                file: "infuser-linux-amd64".into(),
                sha256: Sha256::digest(CANDIDATE)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            },
        )]),
    }));
    let app = Router::new()
        .route(
            "/latest/download/release.json",
            get(|State(manifest): State<Arc<Mutex<Release>>>| async move {
                axum::Json(manifest.lock().unwrap().clone())
            }),
        )
        .fallback(|| async { CANDIDATE.into_response() })
        .with_state(manifest.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let source = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Settings {
        repository: "example/infuser".into(),
        install_dir: install.clone(),
        state_dir: state.clone(),
        controller_config: dir.path().join("config"),
        service: "infuser.service".into(),
        health_url: "http://127.0.0.1/healthz".into(),
    };
    let service = TestService {
        install: install.clone(),
        bad: bad.clone(),
        restarts: Mutex::new(0),
        busy: std::sync::atomic::AtomicBool::new(false),
        rollback_allowed: std::sync::atomic::AtomicBool::new(true),
    };
    let http = reqwest::Client::new();
    // A checksum mismatch must fail before activation or service restart.
    let expected = manifest.lock().unwrap().artifacts["linux-amd64"]
        .sha256
        .clone();
    manifest
        .lock()
        .unwrap()
        .artifacts
        .get_mut("linux-amd64")
        .unwrap()
        .sha256 = "0".repeat(64);
    let error = updater::update(&config, &source, "linux-amd64", &http, &service)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("checksum mismatch"));
    assert_eq!(
        fs::read_link(install.join("current"))?.file_name().unwrap(),
        initial.as_str()
    );
    assert_eq!(*service.restarts.lock().unwrap(), 0);
    assert!(!install.join("releases").join(&good).exists());
    manifest
        .lock()
        .unwrap()
        .artifacts
        .get_mut("linux-amd64")
        .unwrap()
        .sha256 = expected;
    service
        .busy
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let obsolete = install.join("releases").join("d".repeat(40));
    fs::create_dir(&obsolete)?;
    let unrelated = install.join("releases/manual-notes");
    fs::create_dir(&unrelated)?;
    let deferred = updater::update(&config, &source, "linux-amd64", &http, &service).await?;
    assert!(deferred.contains("waiting for games"));
    assert_eq!(*service.restarts.lock().unwrap(), 0);
    assert!(config.state_dir.join("deferred-release").exists());
    assert!(!obsolete.exists());
    assert!(unrelated.exists());
    assert!(install.join("releases").join(&initial).exists());
    service
        .busy
        .store(false, std::sync::atomic::Ordering::Relaxed);
    updater::update(&config, &source, "linux-amd64", &http, &service).await?;
    assert_eq!(
        fs::read_link(install.join("current"))?.file_name().unwrap(),
        good.as_str()
    );
    assert_eq!(*service.restarts.lock().unwrap(), 1);
    updater::update(&config, &source, "linux-amd64", &http, &service).await?;
    assert_eq!(*service.restarts.lock().unwrap(), 1);
    manifest.lock().unwrap().commit = bad.clone();
    assert!(
        updater::update(&config, &source, "linux-amd64", &http, &service)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_link(install.join("current"))?.file_name().unwrap(),
        good.as_str()
    );
    assert_eq!(fs::read_to_string(state.join("failed-release"))?, bad);
    let before = *service.restarts.lock().unwrap();
    updater::update(&config, &source, "linux-amd64", &http, &service).await?;
    assert_eq!(*service.restarts.lock().unwrap(), before);

    // Simulate a power loss immediately after switching the current symlink.
    fs::remove_file(install.join("current"))?;
    symlink(install.join("releases").join(&bad), install.join("current"))?;
    fs::write(
        state.join("pending.json"),
        serde_json::to_vec(&serde_json::json!({"commit":bad,"previous":good}))?,
    )?;
    updater::update(&config, &source, "linux-amd64", &http, &service).await?;
    assert_eq!(
        fs::read_link(install.join("current"))?.file_name().unwrap(),
        good.as_str()
    );
    assert!(!state.join("pending.json").exists());
    // A migrated state must never fall back to an incompatible previous binary.
    service
        .rollback_allowed
        .store(false, std::sync::atomic::Ordering::Relaxed);
    fs::remove_file(state.join("failed-release"))?;
    let before = *service.restarts.lock().unwrap();
    let error = updater::update(&config, &source, "linux-amd64", &http, &service)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("rollback blocked"));
    assert_eq!(
        *service.restarts.lock().unwrap(),
        before + 1,
        "no incompatible restart"
    );
    assert_eq!(
        fs::read_link(install.join("current"))?.file_name().unwrap(),
        bad.as_str()
    );
    assert!(state.join("pending.json").exists());
    let before = *service.restarts.lock().unwrap();
    let error = updater::update(&config, &source, "linux-amd64", &http, &service)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("genesis boundary"));
    assert_eq!(*service.restarts.lock().unwrap(), before);
    assert!(state.join("pending.json").exists());
    server.abort();
    Ok(())
}
