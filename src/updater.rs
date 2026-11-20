//! Pull CI-approved binaries. Keep development and the live deployment independent.
use crate::deployment::{Phase, Report};
use crate::private_file::replace as write_atomic;
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub repository: String,
    pub install_dir: PathBuf,
    pub state_dir: PathBuf,
    pub controller_config: PathBuf,
    #[serde(default = "service")]
    pub service: String,
    #[serde(default = "health")]
    pub health_url: String,
}
fn service() -> String {
    "infuser.service".into()
}
fn health() -> String {
    "http://127.0.0.1:7777/healthz".into()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub commit: String,
    pub artifacts: BTreeMap<String, Artifact>,
    /// Reboot or Ansible this release cannot take effect without. Omitted when
    /// empty so older manifests stay valid. An updater that does not know this
    /// field rejects a manifest that includes it; install this updater before
    /// publishing a release that sets it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<crate::deployment::HostStep>,
    // What's new is not stored here. This struct rejects unknown fields, and the
    // updater running on a host is not replaced by the release it installs.
    // `changelog.json` is a separate release asset; the daemon reads the copy
    // compiled into the binary.
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub file: String,
    pub sha256: String,
}

fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn platform() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("linux-arm64"),
        "x86_64" => Ok("linux-amd64"),
        _ => bail!("no release artifact for this architecture"),
    }
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = toml::from_str(&fs::read_to_string(path)?)?;
        let parts: Vec<_> = config.repository.split('/').collect();
        ensure!(
            parts.len() == 2
                && parts.iter().all(|p| !p.is_empty()
                    && *p != "."
                    && *p != ".."
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "repository must be owner/repo"
        );
        ensure!(
            config.install_dir.is_absolute()
                && config.state_dir.is_absolute()
                && config.controller_config.is_absolute(),
            "updater paths must be absolute"
        );
        ensure!(
            config.service.ends_with(".service")
                && config
                    .service
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "invalid systemd service name"
        );
        let url = reqwest::Url::parse(&config.health_url)?;
        ensure!(
            url.scheme() == "http"
                && matches!(
                    url.host_str(),
                    Some("127.0.0.1") | Some("localhost") | Some("[::1]")
                )
                && url.username().is_empty()
                && url.password().is_none(),
            "health_url must use local HTTP"
        );
        Ok(config)
    }
}

#[async_trait]
pub trait Service: Send + Sync {
    async fn lock_activation(&self) -> Result<bool> {
        Ok(true)
    }
    async fn report(&self, _report: Report) {}
    fn set_host_plan(&self, _steps: &[crate::deployment::HostStep]) {}
    async fn ready_for_release(&self, _release: &str) -> Result<Readiness> {
        Ok(if self.ready_to_update().await? {
            Readiness::Ready
        } else {
            Readiness::Busy("waiting before activation".into())
        })
    }
    async fn ready_to_update(&self) -> Result<bool> {
        Ok(true)
    }
    async fn check(&self, binary: &Path) -> Result<()>;
    async fn check_rollback(&self, _binary: &Path) -> Result<()> {
        Ok(())
    }
    /// Consistent snapshot of the release still running, taken before its symlink moves.
    async fn preserve_state(&self, _release: &str) -> Result<()> {
        Ok(())
    }
    /// Stop the controller so restore cannot race a live WAL. `false` means nothing was stopped.
    async fn stop(&self) -> Result<bool> {
        Ok(false)
    }
    /// Restore the snapshot for `release`. A missing snapshot is success; the caller then
    /// uses the existing binary-only rollback checks.
    async fn restore_state(&self, _release: &str) -> Result<()> {
        Ok(())
    }
    async fn restart(&self) -> Result<()>;
    async fn healthy(&self, commit: &str) -> Result<bool>;
    /// Write the marker the next process reads before it accepts user writes.
    async fn arm_probation(&self, _release: &str) -> Result<()> {
        Ok(())
    }
    async fn disarm_probation(&self) -> Result<()> {
        Ok(())
    }
    /// The candidate marker is still in place, so that process must still be
    /// refusing user writes. Missing or unreadable means do not restore.
    async fn probation_held(&self, _release: &str) -> Result<bool> {
        Ok(true)
    }
    /// `None` unless this host has a real probation marker. The default stays
    /// empty so a test double that does not arm a trial is not resumed.
    async fn armed_release(&self) -> Result<Option<String>> {
        Ok(None)
    }
    async fn trial(&self, commit: &str) -> Result<Trial> {
        Ok(if self.healthy(commit).await? {
            Trial::OnProbation
        } else {
            Trial::Unreachable
        })
    }
    async fn promote(&self, _release: &str) -> Result<()> {
        Ok(())
    }
    /// Restart the embedded relay only when its code changed. The default
    /// leaves test doubles and coturn installations untouched.
    async fn sync_turn(&self, _previous: &Path, _current: &Path) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trial {
    /// This build is up and still refusing user writes.
    OnProbation,
    /// This build is up and already accepting user writes.
    ServingWrites,
    /// The build did not answer the version-aware health check.
    Unreachable,
}

#[derive(Debug)]
struct ServingWrites;

impl std::fmt::Display for ServingWrites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "candidate accepted writes before promotion; database snapshot was not restored",
        )
    }
}

impl std::error::Error for ServingWrites {}

fn classify_health(value: &serde_json::Value, commit: &str) -> Option<Trial> {
    if value["status"] == "ok" && value["build"] == commit {
        Some(if value["probation"] == true {
            Trial::OnProbation
        } else {
            Trial::ServingWrites
        })
    } else {
        None
    }
}
pub enum Readiness {
    Ready,
    Busy(String),
    Maintenance,
}

struct Systemd {
    config: Settings,
    http: reqwest::Client,
    force: bool,
    activation_lock: Mutex<Option<File>>,
    /// None until this run has read a release manifest.
    host_steps: Mutex<Option<Vec<crate::deployment::HostStep>>>,
}
#[async_trait]
impl Service for Systemd {
    fn set_host_plan(&self, steps: &[crate::deployment::HostStep]) {
        *self.host_steps.lock().unwrap() = Some(steps.to_vec());
    }
    async fn report(&self, report: Report) {
        let host = self.host_steps.lock().unwrap().clone();
        let result = async {
            crate::maintenance::Client::from_config_file(&self.config.controller_config)?
                .report_update(&report, host.as_deref())
                .await
        }
        .await;
        if let Err(error) = result {
            tracing::debug!(%error,"deployment status unavailable (older or restarting controller)");
        }
    }
    async fn lock_activation(&self) -> Result<bool> {
        if self.activation_lock.lock().unwrap().is_some() {
            return Ok(true);
        }
        let path = std::env::var_os("INFUSER_HOST_LOCK")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/run/lock/infuser-host-maintenance.lock".into());
        let Some(lock) = acquire_host_lock(&path)? else {
            return Ok(false);
        };
        *self.activation_lock.lock().unwrap() = Some(lock);
        Ok(true)
    }
    async fn ready_for_release(&self, release: &str) -> Result<Readiness> {
        if !self.lock_activation().await? {
            return Ok(Readiness::Maintenance);
        }
        Ok(
            match crate::maintenance::Client::from_config_file(&self.config.controller_config)?
                .admit_update(release, self.force)
                .await?
            {
                crate::maintenance::UpdateGate::Open => Readiness::Ready,
                crate::maintenance::UpdateGate::Wait(reason) => Readiness::Busy(reason),
            },
        )
    }
    async fn check(&self, binary: &Path) -> Result<()> {
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(binary)
                .arg("check")
                .arg(&self.config.controller_config)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        ensure!(
            output.status.success(),
            "candidate does not accept the installed configuration: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        );
        let compatibility = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(binary)
                .arg("state-check")
                .arg(&self.config.controller_config)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        ensure!(
            compatibility.status.success(),
            "Release cannot read the current identity state. Pre-genesis versions are not supported; use a compatible release or restore a complete pre-migration backup."
        );
        Ok(())
    }
    async fn check_rollback(&self, binary: &Path) -> Result<()> {
        self.check(binary).await
    }
    async fn preserve_state(&self, release: &str) -> Result<()> {
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        crate::rollback::capture(&state_dir, &self.config.state_dir.join("rollback"), release)?;
        Ok(())
    }
    async fn stop(&self) -> Result<bool> {
        systemctl(&self.config, "stop").await?;
        Ok(true)
    }
    async fn restore_state(&self, release: &str) -> Result<()> {
        let snapshot = self.config.state_dir.join("rollback").join(release);
        if !snapshot.join("snapshot.json").is_file() {
            return Ok(());
        }
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        crate::rollback::restore(&snapshot, &state_dir)
    }
    async fn restart(&self) -> Result<()> {
        systemctl(&self.config, "restart").await
    }
    async fn healthy(&self, commit: &str) -> Result<bool> {
        Ok(self.poll_health(commit).await?.is_some())
    }
    async fn arm_probation(&self, release: &str) -> Result<()> {
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        crate::probation::arm(&state_dir, release)
    }
    async fn disarm_probation(&self) -> Result<()> {
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        crate::probation::disarm(&state_dir)
    }
    async fn probation_held(&self, release: &str) -> Result<bool> {
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        Ok(crate::probation::holds_release(&state_dir, release))
    }
    async fn armed_release(&self) -> Result<Option<String>> {
        let state_dir = platform_state_dir(&self.config.controller_config)?;
        Ok(crate::probation::armed(&state_dir))
    }
    async fn trial(&self, commit: &str) -> Result<Trial> {
        Ok(self
            .poll_health(commit)
            .await?
            .unwrap_or(Trial::Unreachable))
    }
    async fn promote(&self, release: &str) -> Result<()> {
        crate::maintenance::Client::from_config_file(&self.config.controller_config)?
            .promote(release)
            .await
    }
    async fn sync_turn(&self, previous: &Path, current: &Path) -> Result<bool> {
        if !turn_embedded(&self.config.controller_config)? {
            return Ok(false);
        }
        let next = turn_fingerprint(current).await;
        let state = platform_state_dir(&self.config.controller_config)?;
        let running = std::fs::read_to_string(state.join("turn.fingerprint"))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty());
        let _ = previous;
        if !crate::turn::restart_needed(running.as_deref(), next.as_deref(), true) {
            return Ok(false);
        }
        let listed = tokio::time::timeout(
            Duration::from_secs(8),
            tokio::process::Command::new("systemctl")
                .args(["show", "infuser-turn.service", "--property=LoadState"])
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        if !String::from_utf8_lossy(&listed.stdout).contains("LoadState=loaded") {
            tracing::info!("embedded relay unit is not installed");
            return Ok(false);
        }
        let status = tokio::time::timeout(
            Duration::from_secs(45),
            tokio::process::Command::new("systemctl")
                .args(["restart", "infuser-turn.service"])
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await??;
        ensure!(status.success(), "could not restart infuser-turn.service");
        Ok(true)
    }
}

impl Systemd {
    /// The same 30 second version check the updater already used. The first
    /// matching answer is enough; this does not add another wait.
    async fn poll_health(&self, commit: &str) -> Result<Option<Trial>> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline {
            if let Ok(response) = self.http.get(&self.config.health_url).send().await
                && response.status().is_success()
                && let Ok(value) = response.json::<serde_json::Value>().await
                && let Some(trial) = classify_health(&value, commit)
            {
                return Ok(Some(trial));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(None)
    }
}

fn platform_state_dir(path: &Path) -> Result<PathBuf> {
    // Same tolerance as maintenance admission: new daemon keys must not block the helper.
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        controller: Controller,
    }
    #[derive(Deserialize)]
    #[serde(default)]
    struct Controller {
        state_dir: PathBuf,
    }
    impl Default for Controller {
        fn default() -> Self {
            Self {
                state_dir: crate::config::Settings::default().state_dir,
            }
        }
    }
    let file: File = toml::from_str(&fs::read_to_string(path)?)?;
    ensure!(
        file.controller.state_dir.is_absolute(),
        "platform state_dir must be absolute"
    );
    Ok(file.controller.state_dir)
}

async fn turn_note(config: &Settings, service: &dyn Service) -> String {
    let previous = config.install_dir.join("previous").join("infuser");
    let current = config.install_dir.join("current").join("infuser");
    match service.sync_turn(&previous, &current).await {
        Ok(true) => " The relay restarted because its code changed.".into(),
        Ok(false) => String::new(),
        Err(error) => format!(" The relay did not restart: {error}."),
    }
}

fn turn_embedded(path: &Path) -> Result<bool> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        controller: Controller,
    }
    #[derive(Default, Deserialize)]
    struct Controller {
        #[serde(default)]
        turn: Option<Turn>,
    }
    #[derive(Default, Deserialize)]
    struct Turn {
        #[serde(default)]
        engine: String,
    }
    let file: File = toml::from_str(&fs::read_to_string(path)?)?;
    Ok(file
        .controller
        .turn
        .is_some_and(|turn| turn.engine == "embedded"))
}

async fn turn_fingerprint(binary: &Path) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(binary)
            .arg("turn-fingerprint")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (text.len() == 16 && text.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| text.to_owned())
}

async fn systemctl(config: &Settings, action: &str) -> Result<()> {
    let status = tokio::time::timeout(
        Duration::from_secs(45),
        tokio::process::Command::new("systemctl")
            .arg(action)
            .arg(&config.service)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    ensure!(status.success(), "systemd {action} failed");
    Ok(())
}

/// Stop the controller, restore the outgoing snapshot, and return to that release.
async fn restore_previous(
    service: &dyn Service,
    previous_dir: &Path,
    previous_commit: &str,
    blocked: &str,
) -> Result<()> {
    let blocked = blocked.to_owned();
    let stopped = service.stop().await.context(blocked.clone())?;
    if let Err(error) = service.restore_state(previous_commit).await {
        if stopped {
            let _ = service.restart().await;
        }
        return Err(error.context(blocked));
    }
    if let Err(error) = service.check_rollback(&previous_dir.join("infuser")).await {
        if stopped {
            let _ = service.restart().await;
        }
        return Err(error.context(blocked));
    }
    Ok(())
}

fn note_outcome(
    config: &Settings,
    release: &str,
    result: crate::deploy_outcomes::Kind,
    reason: &str,
) {
    let Ok(state_dir) = platform_state_dir(&config.controller_config) else {
        tracing::warn!(
            "deploy outcome was not recorded; controller state directory is unavailable"
        );
        return;
    };
    if let Err(error) = crate::deploy_outcomes::record(
        &state_dir,
        crate::deploy_outcomes::Outcome {
            release: release.to_owned(),
            at: crate::controller::now(),
            result,
            reason: reason.to_owned(),
        },
    ) {
        tracing::warn!(%error, "deploy outcome was not recorded");
    }
}

fn finish_activation(
    config: &Settings,
    release: &str,
    failed_file: &Path,
    pending_file: &Path,
) -> Result<()> {
    if failed_file.exists() {
        fs::remove_file(failed_file)?;
    }
    fs::remove_file(pending_file)?;
    note_outcome(config, release, crate::deploy_outcomes::Kind::Activated, "");
    Ok(())
}

/// Restore the previous release only while the candidate is still refusing
/// user writes. A missing marker means promotion may already have accepted
/// some, so the snapshot stays unused.
async fn roll_back(
    config: &Settings,
    service: &dyn Service,
    previous_dir: &Path,
    previous_commit: &str,
    candidate: &str,
    blocked: &str,
) -> Result<()> {
    if !service.probation_held(candidate).await? {
        note_outcome(
            config,
            candidate,
            crate::deploy_outcomes::Kind::Kept,
            "candidate accepted writes before promotion; database snapshot was not restored",
        );
        let _ = fs::remove_file(config.state_dir.join("pending.json"));
        bail!(
            "release {candidate} was left in place because its trial was no longer holding writes; database snapshot was not restored"
        );
    }
    restore_previous(service, previous_dir, previous_commit, blocked).await?;
    atomic_link(previous_dir, &config.install_dir.join("current"))?;
    if let Err(error) = service.disarm_probation().await {
        tracing::warn!(%error, "probation marker was not cleared");
    }
    service
        .restart()
        .await
        .context("candidate failed and the old service could not restart")?;
    ensure!(
        service.healthy(&build_id(previous_dir)?).await?,
        "candidate failed and rollback health verification also failed"
    );
    note_outcome(
        config,
        candidate,
        crate::deploy_outcomes::Kind::RolledBack,
        crate::probation::HEALTH_FAILURE,
    );
    write_atomic(
        &config.state_dir.join("failed-release"),
        candidate.as_bytes(),
    )?;
    match fs::remove_file(config.state_dir.join("pending.json")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// `pending.json` is the normal record of an armed trial. If it was lost, the
/// marker alone still means this boot is holding writes. The next `update` run
/// finishes that decision: health check, then promote or roll back.
async fn resume_held_current(config: &Settings, service: &dyn Service) -> Result<Option<String>> {
    let Some(armed) = service.armed_release().await? else {
        return Ok(None);
    };
    let current = linked_release(&config.install_dir, "current")?;
    let name = release_label(&current)?;
    if armed != name {
        return Ok(None);
    }
    match service.trial(&name).await? {
        Trial::OnProbation => {
            if service.promote(&name).await.is_err() {
                return Ok(Some(format!(
                    "Release {name} passed its health check and is holding writes until promotion finishes"
                )));
            }
            clear_probation(service).await;
            note_outcome(config, &name, crate::deploy_outcomes::Kind::Activated, "");
            let note = turn_note(config, service).await;
            Ok(Some(format!(
                "Recovered successful activation of {name}{note}"
            )))
        }
        Trial::ServingWrites => {
            clear_probation(service).await;
            note_outcome(config, &name, crate::deploy_outcomes::Kind::Activated, "");
            let note = turn_note(config, service).await;
            Ok(Some(format!(
                "Recovered successful activation of {name}{note}"
            )))
        }
        Trial::Unreachable => {
            let previous = linked_release(&config.install_dir, "previous")?;
            let previous_commit = release_label(&previous)?;
            roll_back(
                config,
                service,
                &previous,
                &previous_commit,
                &name,
                "Interrupted update cannot roll back across the identity genesis boundary; current selection and pending recovery record retained",
            )
            .await?;
            Ok(Some(format!(
                "Recovered interrupted update {name}; restored previous release"
            )))
        }
    }
}

async fn clear_probation(service: &dyn Service) {
    if let Err(error) = service.disarm_probation().await {
        tracing::warn!(%error, "probation marker was not cleared");
    }
}

fn linked_release(install: &Path, name: &str) -> Result<PathBuf> {
    let releases = install
        .join("releases")
        .canonicalize()
        .context("release directory missing")?;
    let path = fs::read_link(install.join(name))
        .with_context(|| format!("missing {name} release link"))?;
    let path = if path.is_absolute() {
        path
    } else {
        install.join(path)
    }
    .canonicalize()?;
    ensure!(
        path.parent() == Some(releases.as_path()),
        "{name} must point to a release directory"
    );
    Ok(path)
}

fn release_by_name(install: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        crate::rollback::valid_release(name),
        "invalid release directory"
    );
    let releases = install.join("releases").canonicalize()?;
    let path = releases.join(name).canonicalize()?;
    ensure!(
        path.parent() == Some(releases.as_path()),
        "release path escapes the release directory"
    );
    Ok(path)
}

fn release_label(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid release directory")?;
    ensure!(
        crate::rollback::valid_release(name),
        "release directory must be a commit or bootstrap"
    );
    Ok(name.to_owned())
}

/// Exclusive `update.lock`.
///
/// `flock` belongs to the open file description. A concurrent `spawn` copies
/// that description into the child until `exec`, so closing the owner's
/// descriptor can leave the lock held after `update` returns. `unlock`
/// releases every copy, including one a sibling thread has just inherited.
struct UpdateLock {
    file: File,
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn open_update_lock(config: &Settings) -> Result<File> {
    fs::create_dir_all(&config.state_dir)?;
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(config.state_dir.join("update.lock"))?)
}

fn hold_update_lock(config: &Settings) -> Result<UpdateLock> {
    let file = open_update_lock(config)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => {
            anyhow::anyhow!("another update is already running")
        }
        std::fs::TryLockError::Error(error) => error.into(),
    })?;
    Ok(UpdateLock { file })
}

fn acquire_host_lock(path: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

#[cfg(test)]
mod host_lock_tests {
    use super::*;
    #[tokio::test]
    async fn newer_daemon_settings_do_not_block_status_or_idle_admission() -> Result<()> {
        use axum::{Router, http::StatusCode, routing::post};
        use std::sync::Arc;

        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let router = Router::new()
            .route(
                "/v1/deployment",
                post(move |headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer test-control-token");
                    seen.lock().unwrap().push("report");
                    StatusCode::OK
                }),
            )
            .route(
                "/v1/deployment/admit",
                post(|headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer test-control-token");
                    StatusCode::CONFLICT // The controller still decides when it is safe.
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        let dir = tempfile::tempdir()?;
        let token = dir.path().join("control.secret");
        fs::write(&token, "test-control-token\n")?;
        let controller_config = dir.path().join("controller.toml");
        fs::write(
            &controller_config,
            format!(
                "[controller]\napi_port={port}\ntoken_file={token:?}\n[controller.future_feature]\nenabled=true\n[future_section]\nversion=2\n"
            ),
        )?;
        // Keep full daemon validation strict: this tolerance belongs to the helper.
        assert!(crate::config::Config::load(&controller_config).is_err());
        let service = Systemd {
            config: Settings {
                repository: "example/infuser".into(),
                install_dir: dir.path().join("install"),
                state_dir: dir.path().join("state"),
                controller_config: controller_config.clone(),
                service: "infuser.service".into(),
                health_url: health(),
            },
            http: reqwest::Client::new(),
            force: false,
            activation_lock: Mutex::new(acquire_host_lock(&dir.path().join("host.lock"))?),
            host_steps: Mutex::new(None),
        };
        service
            .report(Report {
                release: Some("a".repeat(40)),
                built_at: None,
                phase: Phase::Waiting,
                maintenance_blocked: false,
            })
            .await;
        assert_eq!(*calls.lock().unwrap(), vec!["report"]);
        assert!(matches!(
            service.ready_for_release(&"a".repeat(40)).await?,
            Readiness::Busy(_)
        ));
        for invalid in [
            "[controller]\napi_port='wrong'",
            "[controller]\napi_port=0",
            "[controller]\ntoken_file='relative.secret'",
        ] {
            fs::write(&controller_config, invalid)?;
            assert!(service.ready_for_release(&"a".repeat(40)).await.is_err());
        }
        server.abort();
        Ok(())
    }
    #[tokio::test]
    async fn build_metadata_is_optional_and_bound_to_verified_candidate() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let binary = dir.path().join("candidate");
        for (body, expected) in [
            ("echo '{\"commit\":\"abc\",\"built_at\":123}'", Some(123)),
            ("echo '{\"commit\":\"other\",\"built_at\":123}'", None),
            ("echo '{\"commit\":\"abc\",\"built_at\":0}'", None),
            ("echo old-version; exit 1", None),
        ] {
            fs::write(&binary, format!("#!/bin/sh\n{body}\n"))?;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
            assert_eq!(candidate_build_timestamp(&binary, "abc").await, expected);
        }
        Ok(())
    }
    #[test]
    fn activation_uses_the_same_lock_as_shell_maintenance_jobs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("maintenance.lock");
        let held = acquire_host_lock(&path)?.unwrap();
        assert!(acquire_host_lock(&path)?.is_none());
        assert!(
            !std::process::Command::new("flock")
                .arg("-n")
                .arg(&path)
                .arg("true")
                .status()?
                .success()
        );
        drop(held);
        assert!(
            std::process::Command::new("flock")
                .arg("-n")
                .arg(&path)
                .arg("true")
                .status()?
                .success()
        );
        Ok(())
    }

    fn lock_settings(dir: &std::path::Path) -> Settings {
        Settings {
            repository: "example/infuser".into(),
            install_dir: dir.join("install"),
            state_dir: dir.join("state"),
            controller_config: dir.join("controller.toml"),
            service: "infuser.service".into(),
            health_url: health(),
        }
    }

    #[test]
    fn dropping_the_update_lock_releases_it_while_a_duplicate_is_still_open() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = lock_settings(dir.path());
        let held = hold_update_lock(&config)?;
        // fork and dup share one open-file description. This duplicate is the
        // descriptor a child keeps until exec.
        let inherited = held.file.try_clone()?;
        assert!(
            hold_update_lock(&config).is_err(),
            "the owner must still exclude a second update"
        );
        drop(held);
        let again = hold_update_lock(&config)?;
        drop(again);
        drop(inherited);
        Ok(())
    }

    #[tokio::test]
    async fn snapshot_tolerates_new_controller_settings_and_restores_wal_state() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path().join("platform");
        fs::create_dir(&state)?;
        let db = state.join("platform.sqlite");
        let connection = rusqlite::Connection::open(&db)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('kept');",
        )?;
        let release = "d".repeat(40);
        let install = dir.path().join("install");
        fs::create_dir_all(install.join("releases").join(&release))?;
        std::os::unix::fs::symlink(
            install.join("releases").join(&release),
            install.join("current"),
        )?;
        let controller_config = dir.path().join("controller.toml");
        fs::write(
            &controller_config,
            format!(
                "[controller]\nstate_dir={state:?}\n[controller.future_feature]\nenabled=true\n[future_section]\nversion=2\n"
            ),
        )?;
        assert!(crate::config::Config::load(&controller_config).is_err());
        let config = Settings {
            repository: "example/infuser".into(),
            install_dir: install.clone(),
            state_dir: dir.path().join("updater"),
            controller_config,
            service: "infuser.service".into(),
            health_url: health(),
        };
        struct Allow;
        #[async_trait::async_trait]
        impl Service for Allow {
            async fn lock_activation(&self) -> Result<bool> {
                Ok(true)
            }
            async fn check(&self, _: &std::path::Path) -> Result<()> {
                Ok(())
            }
            async fn restart(&self) -> Result<()> {
                Ok(())
            }
            async fn healthy(&self, _: &str) -> Result<bool> {
                Ok(true)
            }
        }
        assert!(snapshot_current(&config, &Allow).await?.contains(&release));
        connection.execute("INSERT INTO marker VALUES('gone')", [])?;
        drop(connection);
        let service = Systemd {
            config: config.clone(),
            http: reqwest::Client::new(),
            force: false,
            activation_lock: std::sync::Mutex::new(None),
            host_steps: Mutex::new(None),
        };
        service.restore_state(&release).await?;
        let connection = rusqlite::Connection::open(&db)?;
        let kept: i64 =
            connection.query_row("SELECT COUNT(*) FROM marker WHERE v='kept'", [], |row| {
                row.get(0)
            })?;
        let gone: i64 =
            connection.query_row("SELECT COUNT(*) FROM marker WHERE v='gone'", [], |row| {
                row.get(0)
            })?;
        assert_eq!((kept, gone), (1, 0));
        fs::write(
            &config.controller_config,
            "[controller]\nstate_dir='relative'\n",
        )?;
        assert!(service.preserve_state(&release).await.is_err());
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Pending {
    commit: String,
    previous: String,
}

fn build_id(directory: &Path) -> Result<String> {
    let name = directory
        .file_name()
        .and_then(|n| n.to_str())
        .context("invalid release directory")?;
    if name != "bootstrap" {
        ensure!(hex(name, 40), "invalid release directory name");
        return Ok(name.into());
    }
    let value =
        fs::read_to_string(directory.join("build-id")).context("bootstrap build ID is missing")?;
    let value = value.trim();
    ensure!(
        value == "development" || hex(value, 40),
        "invalid bootstrap build ID"
    );
    Ok(value.into())
}

fn atomic_link(target: &Path, link: &Path) -> Result<()> {
    let temporary = link.with_extension("next");
    if temporary.symlink_metadata().is_ok() {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    fs::rename(&temporary, link)?;
    File::open(link.parent().context("link parent missing")?)?.sync_all()?;
    Ok(())
}

async fn download(http: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>> {
    let mut response = http.get(url).send().await?.error_for_status()?;
    ensure!(
        response.content_length().is_none_or(|n| n <= max as u64),
        "release download exceeds size limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= max,
            "release download exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn manifest_url(source: &str) -> String {
    // Separate cache keys even when staging and the final lookup take <1 second.
    format!(
        "{source}/latest/download/release.json?infuser_check={}-{}",
        crate::controller::now(),
        crate::store::secret()
    )
}

pub async fn run(config: Settings) -> Result<String> {
    run_force(config, false).await
}
pub async fn run_force(config: Settings, force: bool) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("infuser-updater")
        .build()?;
    let service = Systemd {
        activation_lock: Mutex::new(None),
        force,
        config: config.clone(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?,
        host_steps: Mutex::new(None),
    };
    let source = format!("https://github.com/{}/releases", config.repository);
    let result = update(&config, &source, platform()?, &http, &service).await;
    if result.is_err() {
        service
            .report(Report {
                release: None,
                built_at: None,
                phase: Phase::Failed,
                maintenance_blocked: false,
            })
            .await;
    }
    result
}

fn operator_service(config: &Settings) -> Result<Systemd> {
    Ok(Systemd {
        activation_lock: Mutex::new(None),
        force: false,
        config: config.clone(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?,
        host_steps: Mutex::new(None),
    })
}

pub async fn run_snapshot(config: Settings) -> Result<String> {
    let service = operator_service(&config)?;
    snapshot_current(&config, &service).await
}

pub async fn run_rollback(config: Settings) -> Result<String> {
    let service = operator_service(&config)?;
    rollback(&config, &service).await
}

/// Capture the running release without stopping it. Safe to run before the next update.
pub async fn snapshot_current(config: &Settings, service: &dyn Service) -> Result<String> {
    let _lock = hold_update_lock(config)?;
    if !service.lock_activation().await? {
        bail!("host maintenance lock is busy");
    }
    let name = release_label(&linked_release(&config.install_dir, "current")?)?;
    let state_dir = platform_state_dir(&config.controller_config)?;
    let snapshot = crate::rollback::capture(&state_dir, &config.state_dir.join("rollback"), &name)?;
    Ok(format!(
        "Snapshotted {name} ({} databases, {} files)",
        snapshot.databases.len(),
        snapshot.files.len()
    ))
}

/// One-command return to the previous release and its pre-switch database snapshot.
pub async fn rollback(config: &Settings, service: &dyn Service) -> Result<String> {
    let _lock = hold_update_lock(config)?;
    if !service.lock_activation().await? {
        bail!("host maintenance lock is busy");
    }
    let pending_file = config.state_dir.join("pending.json");
    let (target_name, abandon) = if pending_file.is_file() {
        let pending: Pending = serde_json::from_slice(&fs::read(&pending_file)?)?;
        ensure!(
            hex(&pending.commit, 40)
                && (pending.previous == "bootstrap" || hex(&pending.previous, 40)),
            "invalid pending update record"
        );
        let releases = config.install_dir.join("releases").canonicalize()?;
        let active = fs::read_link(config.install_dir.join("current"))?.canonicalize()?;
        ensure!(
            active == releases.join(&pending.commit) || active == releases.join(&pending.previous),
            "current changed outside the interrupted update; inspect pending.json"
        );
        (pending.previous, pending.commit)
    } else {
        let current_name = release_label(&linked_release(&config.install_dir, "current")?)?;
        let previous_name = release_label(&linked_release(&config.install_dir, "previous")?)?;
        ensure!(
            current_name != previous_name,
            "current is already the previous release"
        );
        (previous_name, current_name)
    };
    let target = release_by_name(&config.install_dir, &target_name)?;
    let snapshot_dir = config.state_dir.join("rollback").join(&target_name);
    ensure!(
        snapshot_dir.join("snapshot.json").is_file(),
        "no pre-update snapshot for {target_name}; refusing to switch binaries"
    );
    let manifest = crate::rollback::verify(&snapshot_dir)?;
    ensure!(
        manifest.release == target_name,
        "snapshot does not match the previous release"
    );
    let state_dir = platform_state_dir(&config.controller_config)?;
    let stopped = service.stop().await?;
    if let Err(error) = crate::rollback::restore(&snapshot_dir, &state_dir) {
        if stopped {
            let _ = service.restart().await;
        }
        return Err(error.context("database snapshot was not restored; release unchanged"));
    }
    atomic_link(&target, &config.install_dir.join("current"))?;
    if hex(&abandon, 40) {
        write_atomic(&config.state_dir.join("failed-release"), abandon.as_bytes())?;
    }
    if pending_file.exists() {
        fs::remove_file(&pending_file)?;
    }
    if let Err(error) = service.disarm_probation().await {
        tracing::warn!(%error, "probation marker was not cleared");
    }
    service.restart().await?;
    ensure!(
        service.healthy(&build_id(&target)?).await?,
        "restored {target_name} but its health check failed; inspect infuser.service"
    );
    if hex(&abandon, 40) {
        note_outcome(
            config,
            &abandon,
            crate::deploy_outcomes::Kind::RolledBack,
            crate::probation::OPERATOR_RESTORE,
        );
    }
    Ok(format!(
        "Restored {target_name}; {abandon} will not be activated again"
    ))
}

/// `source` and `service` are injectable so the entire update/rollback journey can run locally.
pub async fn update(
    config: &Settings,
    source: &str,
    platform: &str,
    http: &reqwest::Client,
    service: &dyn Service,
) -> Result<String> {
    let file = open_update_lock(config)?;
    if file.try_lock().is_err() {
        return Ok("Another update is already running".into());
    }
    let _lock = UpdateLock { file };
    fs::create_dir_all(config.install_dir.join("releases"))?;
    let pending_file = config.state_dir.join("pending.json");
    let failed_file = config.state_dir.join("failed-release");
    let current = config.install_dir.join("current");
    if pending_file.exists() {
        if !service.lock_activation().await? {
            return Ok("Interrupted update recovery is waiting for host maintenance".into());
        }
        let pending: Pending = serde_json::from_slice(&fs::read(&pending_file)?)?;
        ensure!(
            hex(&pending.commit, 40)
                && (pending.previous == "bootstrap" || hex(&pending.previous, 40)),
            "invalid pending update record"
        );
        let releases = config.install_dir.join("releases").canonicalize()?;
        let candidate = releases.join(&pending.commit);
        let old = releases.join(&pending.previous);
        let active = fs::read_link(&current)?.canonicalize()?;
        ensure!(
            active == candidate || active == old,
            "current changed outside the interrupted update; inspect pending.json"
        );
        if active == candidate {
            match service.trial(&pending.commit).await? {
                Trial::OnProbation => {
                    if service.promote(&pending.commit).await.is_err() {
                        return Ok(format!(
                            "Release {} passed its health check and is holding writes until promotion finishes",
                            pending.commit
                        ));
                    }
                    clear_probation(service).await;
                    finish_activation(config, &pending.commit, &failed_file, &pending_file)?;
                    let note = turn_note(config, service).await;
                    return Ok(format!(
                        "Recovered successful activation of {}{note}",
                        pending.commit
                    ));
                }
                Trial::ServingWrites => {
                    clear_probation(service).await;
                    finish_activation(config, &pending.commit, &failed_file, &pending_file)?;
                    let note = turn_note(config, service).await;
                    return Ok(format!(
                        "Recovered successful activation of {}{note}",
                        pending.commit
                    ));
                }
                Trial::Unreachable => {}
            }
        }
        roll_back(
            config,
            service,
            &old,
            &pending.previous,
            &pending.commit,
            "Interrupted update cannot roll back across the identity genesis boundary; current selection and pending recovery record retained",
        )
        .await?;
        return Ok(format!(
            "Recovered interrupted update {}; restored previous release",
            pending.commit
        ));
    }
    if let Some(message) = resume_held_current(config, service).await? {
        return Ok(message);
    }
    service
        .report(Report {
            release: None,
            built_at: None,
            phase: Phase::Checking,
            maintenance_blocked: false,
        })
        .await;
    let manifest = download(
        http,
        // GitHub's mutable latest redirect can stay cached after a release is
        // published. Refresh only this lookup; binaries still use immutable tags
        // and must match the manifest's SHA-256 before activation.
        &manifest_url(source),
        16384,
    )
    .await?;
    let release: Release = serde_json::from_slice(&manifest)?;
    ensure!(hex(&release.commit, 40), "invalid release commit");
    service.set_host_plan(&release.host);
    let artifact = release
        .artifacts
        .get(platform)
        .context("release does not contain this platform")?;
    ensure!(
        artifact.file == format!("infuser-{platform}") && hex(&artifact.sha256, 64),
        "invalid release artifact"
    );
    // Bootstrap must preserve a known-good binary. Never overwrite an untracked installation.
    let previous = fs::read_link(&current)
        .context("bootstrap /opt/infuser/current before enabling automatic updates")?;
    let previous_dir = previous.canonicalize()?;
    let releases_dir = config.install_dir.join("releases").canonicalize()?;
    ensure!(
        previous_dir.parent() == Some(releases_dir.as_path()),
        "current must point to a release directory"
    );
    let previous_commit = previous_dir
        .file_name()
        .and_then(|n| n.to_str())
        .context("invalid current release")?
        .to_string();
    // Run even while a newer release waits for idle players. Otherwise one
    // staged binary per release could accumulate indefinitely on a busy host.
    prune_releases(config, &release.commit)?;
    if previous_commit == release.commit {
        service
            .report(Report {
                release: Some(release.commit.clone()),
                built_at: None,
                phase: Phase::Current,
                maintenance_blocked: false,
            })
            .await;
        return Ok(format!("Already running {}", release.commit));
    }
    if fs::read_to_string(&failed_file)
        .ok()
        .is_some_and(|s| s.trim() == release.commit)
    {
        service
            .report(Report {
                release: Some(release.commit.clone()),
                built_at: None,
                phase: Phase::Failed,
                maintenance_blocked: false,
            })
            .await;
        return Ok(format!(
            "Release {} previously failed; waiting for a newer build",
            release.commit
        ));
    }
    service
        .report(Report {
            release: Some(release.commit.clone()),
            built_at: None,
            phase: Phase::Downloading,
            maintenance_blocked: false,
        })
        .await;
    let candidate_dir = config.install_dir.join("releases").join(&release.commit);
    let cached = fs::read(candidate_dir.join("infuser"))
        .ok()
        .filter(|bytes| {
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                == artifact.sha256
        });
    let bytes = if let Some(bytes) = cached {
        bytes
    } else {
        download(
            http,
            &format!(
                "{source}/download/build-{}/{}",
                release.commit, artifact.file
            ),
            64 * 1024 * 1024,
        )
        .await?
    };
    ensure!(
        Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            == artifact.sha256,
        "release checksum mismatch"
    );
    fs::create_dir_all(&candidate_dir)?;
    // The updater runs with umask 0077, but the daemon runs as another user.
    // Release binaries are public artifacts; credentials live outside this tree.
    fs::set_permissions(&candidate_dir, fs::Permissions::from_mode(0o755))?;
    let binary = candidate_dir.join("infuser");
    let staging = candidate_dir.join("infuser.part");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o755)
        .open(&staging)?;
    file.write_all(&bytes)?;
    file.set_permissions(fs::Permissions::from_mode(0o755))?;
    file.sync_all()?;
    // Linux rejects exec while any writable descriptor still refers to this inode.
    drop(file);
    fs::rename(staging, &binary)?;
    File::open(&candidate_dir)?.sync_all()?;
    let built_at = candidate_build_timestamp(&binary, &release.commit).await;
    if !service.lock_activation().await? {
        service
            .report(Report {
                release: Some(release.commit.clone()),
                built_at,
                phase: Phase::Waiting,
                maintenance_blocked: true,
            })
            .await;
        return Ok(
            "Release downloaded; waiting for host maintenance before validation and activation"
                .into(),
        );
    }
    if let Err(error) = service.check(&binary).await {
        write_atomic(&failed_file, release.commit.as_bytes())?;
        return Err(
            error.context("candidate rejected before activation; current version unchanged")
        );
    }
    // Resolve the mutable latest pointer again after staging. If it advanced,
    // leave the one-time permission untouched and fetch that release next tick.
    let latest: Release =
        serde_json::from_slice(&download(http, &manifest_url(source), 16384).await?)?;
    ensure!(hex(&latest.commit, 40), "invalid latest release commit");
    if latest.commit != release.commit {
        service
            .report(Report {
                release: Some(latest.commit),
                built_at: None,
                phase: Phase::Downloading,
                maintenance_blocked: false,
            })
            .await;
        return Ok("A newer release appeared during staging; fetching it on the next check".into());
    }
    ensure!(
        latest
            .artifacts
            .get(platform)
            .is_some_and(|a| a.file == artifact.file && a.sha256 == artifact.sha256),
        "release artifact changed after verification"
    );
    let readiness = service.ready_for_release(&release.commit).await?;
    if !matches!(readiness, Readiness::Ready) {
        let reason = match &readiness {
            Readiness::Maintenance => "waiting for another job on this computer",
            Readiness::Busy(reason) => reason.as_str(),
            Readiness::Ready => unreachable!(),
        };
        service
            .report(Report {
                release: Some(release.commit.clone()),
                built_at,
                phase: Phase::Waiting,
                maintenance_blocked: matches!(readiness, Readiness::Maintenance),
            })
            .await;
        let deferred = config.state_dir.join("deferred-release");
        if fs::read_to_string(&deferred).ok().as_deref() != Some(release.commit.as_str()) {
            write_atomic(&deferred, release.commit.as_bytes())?;
        }
        return Ok(format!("Staged {}; {reason}", release.commit));
    }
    service
        .report(Report {
            release: Some(release.commit.clone()),
            built_at,
            phase: Phase::Activating,
            maintenance_blocked: false,
        })
        .await;
    let stopped = service
        .stop()
        .await
        .context("could not stop the current release before its snapshot")?;
    if let Err(error) = service.preserve_state(&previous_commit).await {
        if stopped {
            let _ = service.restart().await;
        }
        if error.is::<crate::rollback::DiskLimit>() {
            return Ok(
                "Not enough free disk for a pre-update snapshot; current release left unchanged"
                    .into(),
            );
        }
        return Err(error.context("pre-update database snapshot failed; current version unchanged"));
    }
    if let Err(error) = service.arm_probation(&release.commit).await {
        if stopped {
            let _ = service.restart().await;
        }
        return Err(
            error.context("could not hold user writes for the trial; current version unchanged")
        );
    }
    let _ = fs::remove_file(config.state_dir.join("deferred-release"));
    atomic_link(&previous_dir, &config.install_dir.join("previous"))?;
    write_atomic(
        &pending_file,
        &serde_json::to_vec(&Pending {
            commit: release.commit.clone(),
            previous: previous_commit.clone(),
        })?,
    )?;
    atomic_link(&candidate_dir, &current)?;
    let activated = async {
        service.restart().await?;
        match service.trial(&release.commit).await? {
            Trial::OnProbation => {
                if let Err(error) = service.promote(&release.commit).await {
                    tracing::error!(%error, "health check passed; promotion remains pending");
                    return Ok(false);
                }
                Ok(true)
            }
            Trial::ServingWrites => Err(ServingWrites.into()),
            Trial::Unreachable => bail!("{}", crate::probation::HEALTH_FAILURE),
        }
    }
    .await;
    match activated {
        Ok(false) => {
            return Ok(format!(
                "Release {} passed its health check and is holding writes until promotion finishes",
                release.commit
            ));
        }
        Err(error) if error.is::<ServingWrites>() => {
            clear_probation(service).await;
            note_outcome(
                config,
                &release.commit,
                crate::deploy_outcomes::Kind::Kept,
                &error.to_string(),
            );
            let _ = fs::remove_file(&pending_file);
            bail!("release {} was left in place: {error}", release.commit);
        }
        Err(error) => {
            roll_back(
                config,
                service,
                &previous_dir,
                &previous_commit,
                &release.commit,
                "Activation failed and the previous release cannot read current state; rollback blocked, current selection and pending recovery record retained",
            )
            .await?;
            bail!(
                "release {} failed and was rolled back: {error}",
                release.commit
            );
        }
        Ok(true) => clear_probation(service).await,
    }
    finish_activation(config, &release.commit, &failed_file, &pending_file)?;
    prune_releases(config, &release.commit)?;
    service
        .report(Report {
            release: Some(release.commit.clone()),
            built_at,
            phase: Phase::Current,
            maintenance_blocked: false,
        })
        .await;
    let note = turn_note(config, service).await;
    Ok(format!("Activated {}{note}", release.commit))
}

#[cfg(test)]
mod trial_tests {
    use super::*;
    use std::{path::PathBuf, sync::Mutex, time::Duration};

    #[test]
    fn health_json_distinguishes_a_held_candidate_from_one_accepting_writes() {
        let commit = "a".repeat(40);
        assert_eq!(
            classify_health(
                &serde_json::json!({"status":"ok","build":commit,"probation":true}),
                &commit
            ),
            Some(Trial::OnProbation)
        );
        assert_eq!(
            classify_health(&serde_json::json!({"status":"ok","build":commit}), &commit),
            Some(Trial::ServingWrites)
        );
        assert_eq!(
            classify_health(
                &serde_json::json!({"status":"ok","build":"b".repeat(40),"probation":true}),
                &commit
            ),
            None
        );
    }

    #[tokio::test]
    async fn trial_returns_on_the_first_matching_health_response() -> Result<()> {
        let commit = "b".repeat(40);
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = hits.clone();
        let answer = commit.clone();
        let app = axum::Router::new().route(
            "/healthz",
            axum::routing::get(move || {
                let seen = seen.clone();
                let answer = answer.clone();
                async move {
                    seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::Json(serde_json::json!({"status":"ok","build":answer,"probation":true}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = Systemd {
            config: Settings {
                repository: "example/infuser".into(),
                install_dir: PathBuf::from("/opt/infuser"),
                state_dir: PathBuf::from("/var/lib/infuser-updater"),
                controller_config: PathBuf::from("/etc/infuser/infuser.toml"),
                service: "infuser.service".into(),
                health_url: format!("http://{address}/healthz"),
            },
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()?,
            force: false,
            activation_lock: Mutex::new(None),
            host_steps: Mutex::new(None),
        };
        assert_eq!(service.trial(&commit).await?, Trial::OnProbation);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        server.abort();
        Ok(())
    }
}

// Metadata is optional for old binaries and must never prevent an update.
async fn candidate_build_timestamp(binary: &Path, commit: &str) -> Option<u64> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(binary)
            .arg("build-info")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let timestamp = info["built_at"].as_u64()?;
    (info["commit"] == commit && timestamp > 0 && timestamp <= crate::controller::now())
        .then_some(timestamp)
}

fn prune_releases(config: &Settings, candidate: &str) -> Result<()> {
    // Interrupted activations must be resolved before deleting any candidates.
    ensure!(
        !config.state_dir.join("pending.json").exists(),
        "resolve pending deployment before pruning releases"
    );
    let releases = config.install_dir.join("releases").canonicalize()?;
    let mut keep = std::collections::BTreeSet::from([candidate.to_owned()]);
    for name in ["current", "previous"] {
        let link = config.install_dir.join(name);
        match fs::read_link(link) {
            Ok(path) => {
                let path = if path.is_absolute() {
                    path
                } else {
                    config.install_dir.join(path)
                }
                .canonicalize()?;
                ensure!(
                    path.parent() == Some(releases.as_path()),
                    "release link points outside release directory"
                );
                keep.insert(
                    path.file_name()
                        .context("release directory name missing")?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && name == "previous" => {}
            Err(error) => return Err(error.into()),
        }
    }
    for entry in fs::read_dir(&releases)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && hex(&name, 40) && !keep.contains(&name) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}
