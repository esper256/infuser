//! Pull CI-approved binaries. Keep development and the live deployment independent.
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
    async fn check(&self, binary: &Path) -> Result<()>;
    async fn restart(&self) -> Result<()>;
    async fn healthy(&self, commit: &str) -> Result<bool>;
}

struct Systemd {
    config: Settings,
    http: reqwest::Client,
}
#[async_trait]
impl Service for Systemd {
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
        Ok(())
    }
    async fn restart(&self) -> Result<()> {
        let status = tokio::time::timeout(
            Duration::from_secs(45),
            tokio::process::Command::new("systemctl")
                .arg("restart")
                .arg(&self.config.service)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await??;
        ensure!(status.success(), "systemd restart failed");
        Ok(())
    }
    async fn healthy(&self, commit: &str) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline {
            if let Ok(response) = self.http.get(&self.config.health_url).send().await
                && response.status().is_success()
                && let Ok(value) = response.json::<serde_json::Value>().await
                && value["status"] == "ok"
                && value["build"] == commit
            {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(false)
    }
}

#[derive(Serialize, Deserialize)]
struct Pending {
    commit: String,
    previous: String,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("next");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(path.parent().context("missing parent directory")?)?.sync_all()?;
    Ok(())
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

pub async fn run(config: Settings) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("infuser-updater")
        .build()?;
    let service = Systemd {
        config: config.clone(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?,
    };
    let source = format!("https://github.com/{}/releases", config.repository);
    update(&config, &source, platform()?, &http, &service).await
}

/// `source` and `service` are injectable so the entire update/rollback journey can run locally.
pub async fn update(
    config: &Settings,
    source: &str,
    platform: &str,
    http: &reqwest::Client,
    service: &dyn Service,
) -> Result<String> {
    fs::create_dir_all(&config.state_dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(config.state_dir.join("update.lock"))?;
    if lock.try_lock().is_err() {
        return Ok("Another update is already running".into());
    }
    fs::create_dir_all(config.install_dir.join("releases"))?;
    let pending_file = config.state_dir.join("pending.json");
    let failed_file = config.state_dir.join("failed-release");
    let current = config.install_dir.join("current");
    if pending_file.exists() {
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
        if active == candidate && service.healthy(&pending.commit).await? {
            fs::remove_file(&pending_file)?;
            return Ok(format!(
                "Recovered successful activation of {}",
                pending.commit
            ));
        }
        atomic_link(&old, &current)?;
        service.restart().await?;
        ensure!(
            service.healthy(&build_id(&old)?).await?,
            "interrupted update rollback is not healthy"
        );
        write_atomic(&failed_file, pending.commit.as_bytes())?;
        fs::remove_file(&pending_file)?;
        return Ok(format!(
            "Recovered interrupted update {}; restored previous release",
            pending.commit
        ));
    }
    let manifest = download(
        http,
        &format!("{source}/latest/download/release.json"),
        16384,
    )
    .await?;
    let release: Release = serde_json::from_slice(&manifest)?;
    ensure!(hex(&release.commit, 40), "invalid release commit");
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
    if previous_commit == release.commit {
        return Ok(format!("Already running {}", release.commit));
    }
    if fs::read_to_string(&failed_file)
        .ok()
        .is_some_and(|s| s.trim() == release.commit)
    {
        return Ok(format!(
            "Release {} previously failed; waiting for a newer build",
            release.commit
        ));
    }
    let bytes = download(
        http,
        &format!(
            "{source}/download/build-{}/{}",
            release.commit, artifact.file
        ),
        64 * 1024 * 1024,
    )
    .await?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == artifact.sha256,
        "release checksum mismatch"
    );
    let candidate_dir = config.install_dir.join("releases").join(&release.commit);
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
    if let Err(error) = service.check(&binary).await {
        write_atomic(&failed_file, release.commit.as_bytes())?;
        return Err(
            error.context("candidate rejected before activation; current version unchanged")
        );
    }
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
        ensure!(
            service.healthy(&release.commit).await?,
            "candidate failed its version-aware health check"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = activated {
        atomic_link(&previous_dir, &current)?;
        service
            .restart()
            .await
            .context("candidate failed and the old service could not restart")?;
        ensure!(
            service.healthy(&build_id(&previous_dir)?).await?,
            "candidate failed and rollback health verification also failed"
        );
        write_atomic(&failed_file, release.commit.as_bytes())?;
        fs::remove_file(&pending_file)?;
        bail!(
            "release {} failed and was rolled back: {error}",
            release.commit
        );
    }
    if failed_file.exists() {
        fs::remove_file(failed_file)?;
    }
    fs::remove_file(&pending_file)?;
    // Retain current and previous. Stale binaries are reproducible release downloads.
    for entry in fs::read_dir(&releases_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir()
            && hex(&name, 40)
            && name != release.commit
            && name != previous_commit
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(format!("Activated {}", release.commit))
}
