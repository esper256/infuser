//! Operator mistakes must not leave empty files that block the next attempt.
use anyhow::{Result, ensure};
use infuser::{config::Config, platform};
use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn failed_operator_commands_can_retry_and_never_overwrite_credentials() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("infuser.toml");
    std::fs::write(
        &config,
        format!(
            "[controller]\nstate_dir={:?}",
            dir.path().join("state").display().to_string()
        ),
    )?;
    let cfg = Config::load(&config)?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    let output = dir.path().join("art.credentials.json");
    let pass = |group| {
        Command::new(env!("CARGO_BIN_EXE_infuser"))
            .arg("metadata-pass")
            .arg(&config)
            .arg(&output)
            .args(["--group", group])
            .output()
    };
    assert!(!pass("missing-group")?.status.success());
    assert!(
        !output.exists(),
        "failed command left an unusable credential file"
    );
    let result = pass("home")?;
    ensure!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = std::fs::read(&output)?;
    let value: serde_json::Value = serde_json::from_slice(&original)?;
    assert_eq!(value["scope"], "group_metadata");
    assert_eq!(output.metadata()?.permissions().mode() & 0o777, 0o600);
    assert!(!pass("home")?.status.success());
    assert_eq!(std::fs::read(&output)?, original);
    assert_eq!(
        platform::Database::open(&cfg.controller.state_dir)?
            .read()?
            .credentials
            .len(),
        1
    );
    let preview = dir.path().join("preview.json");
    let result = Command::new(env!("CARGO_BIN_EXE_infuser"))
        .arg("preview-game")
        .arg(&config)
        .args(["missing-game", "admin@example.com"])
        .arg(&preview)
        .output()?;
    assert!(!result.status.success());
    assert!(!preview.exists());
    Ok(())
}
