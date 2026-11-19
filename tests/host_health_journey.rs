use anyhow::{Result, ensure};
use axum::{Json, Router, http::HeaderMap, routing::get};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[tokio::test]
async fn stalled_controller_recovery_is_bounded_and_backup_pause_is_respected() -> Result<()> {
    let root = tempfile::tempdir()?;
    let state = root.path().join("state");
    fs::create_dir(&state)?;
    let work = root.path().join("backup");
    fs::create_dir(&work)?;
    let bin = root.path().join("bin");
    fs::create_dir(&bin)?;
    let marker = root.path().join("restarts");
    // Fake only host boundaries; run the actual CLI, HTTP checks, persistence and exporter.
    fs::write(bin.join("docker"), "#!/bin/sh\nexit 0\n")?;
    fs::write(
        bin.join("systemctl"),
        "#!/bin/sh\ncase \"$1\" in\nshow) printf 'ActiveState=active\\nMemoryMax=536870912\\nTasksMax=128\\nNRestarts=0\\n';;\nis-active) exit 0;;\n--no-block) printf 'restart\\n' >> \"$TEST_RESTARTS\";;\nstart) printf 'start\\n' >> \"$TEST_RESTARTS\";;\n*) exit 1;;\nesac\n",
    )?;
    for name in ["docker", "systemctl"] {
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755))?;
    }
    let ready = Arc::new(AtomicBool::new(true));
    let api_ready = ready.clone();
    let app=Router::new().route("/v1/operations",get(move |h:HeaderMap|{
        let ready=api_ready.load(Ordering::Relaxed);
        async move {
            assert_eq!(h["authorization"],"Bearer fixture-secret");
            Json(json!({"database_ok":true,"lifecycle_age_seconds":if ready {0}else{120},"build":"fixture"}))
        }
    })).route("/auth",get(||async {axum::http::StatusCode::ACCEPTED}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let token = root.path().join("token");
    fs::write(&token, "fixture-secret")?;
    let config = root.path().join("controller.toml");
    fs::write(
        &config,
        format!(
            "[controller]\nstate_dir={state:?}\ntoken_file={token:?}\napi_port={port}\nauth_url=\"http://127.0.0.1:{port}/auth\""
        ),
    )?;
    let metrics = root.path().join("metrics");
    let updater = root.path().join("updater");
    let rollback = updater.join("rollback");
    fs::create_dir_all(&rollback)?;
    fs::write(
        rollback.join("last-failure.json"),
        b"{\"at\":1,\"release\":\"ab\"}",
    )?;
    let operations = root.path().join("operations.toml");
    fs::write(
        &operations,
        format!(
            "controller_config={config:?}\nbackup_work_dir={work:?}\nmetrics_dir={metrics:?}\nupdater_state_dir={updater:?}\nservices=[\"infuser.service\"]\nrecover_unresponsive_daemon=true"
        ),
    )?;
    let invoke = || {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_infuser"));
        command
            .args(["doctor", operations.to_str().unwrap(), "--quiet"])
            .env("PATH", &bin)
            .env("TEST_RESTARTS", &marker);
        command
    };
    let run = || async {
        let output = invoke().output().await?;
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        anyhow::Ok(())
    };
    run().await?;
    let text = fs::read_to_string(metrics.join("infuser.prom"))?;
    assert!(text.contains("infuser_check_success{check=\"controller\"} 1"));
    assert!(text.contains("infuser_check_success{check=\"rollback-snapshot\"} 0"));
    assert!(!text.contains("fixture-secret"));
    let release = "ab".repeat(20);
    let record = format!(
        r#"{{"version":1,"release":"{release}","created":1,"verified":true,"databases":[],"files":[]}}"#
    );
    fs::create_dir_all(rollback.join(&release))?;
    fs::write(rollback.join(&release).join("snapshot.json"), &record)?;
    fs::write(rollback.join("latest.json"), &record)?;
    fs::remove_file(rollback.join("last-failure.json"))?;
    run().await?;
    let text = fs::read_to_string(metrics.join("infuser.prom"))?;
    assert!(text.contains("infuser_check_success{check=\"rollback-snapshot\"} 1"));
    ready.store(false, Ordering::Relaxed);
    run().await?;
    run().await?;
    assert!(
        !marker.exists(),
        "one or two failed probes must not restart the daemon"
    );
    run().await?;
    assert_eq!(fs::read_to_string(&marker)?, "restart\n");
    run().await?;
    assert_eq!(
        fs::read_to_string(&marker)?,
        "restart\n",
        "cooldown prevents a restart storm"
    );
    fs::write(work.join("service-paused"), "paused")?;
    run().await?;
    let report: infuser::host_health::Report =
        serde_json::from_slice(&fs::read(state.join("host-health.json"))?)?;
    assert!(report.maintenance);
    assert_eq!(report.failures["controller"], 0);
    assert_eq!(fs::read_to_string(&marker)?, "restart\n");
    // The exit hook must resume even when a backup source disappeared.
    fs::remove_file(&config)?;
    let backup_config = root.path().join("backup.toml");
    fs::write(
        &backup_config,
        format!(
            "controller_config={config:?}\nconfig_dir={state:?}\nwork_dir={work:?}\nrepository_file={token:?}\npassword_file={token:?}\nextra_paths=[{:?}]",
            root.path().join("missing")
        ),
    )?;
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_infuser"))
        .args(["backup-resume", backup_config.to_str().unwrap()])
        .env("PATH", &bin)
        .env("TEST_RESTARTS", &marker)
        .output()
        .await?;
    ensure!(
        output.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!work.join("service-paused").exists());
    assert_eq!(fs::read_to_string(&marker)?, "restart\nstart\n");
    server.abort();
    Ok(())
}
