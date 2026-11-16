//! Offline snapshots and verified restic replacement. Credentials never enter reports.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub controller_config: PathBuf,
    pub config_dir: PathBuf,
    pub work_dir: PathBuf,
    pub repository_file: PathBuf,
    pub password_file: PathBuf,
    pub rclone_config: Option<PathBuf>,
    #[serde(default = "restic_binary")]
    pub restic: PathBuf,
    #[serde(default = "service")]
    pub service: String,
    #[serde(default)]
    pub extra_paths: Vec<PathBuf>,
}
fn restic_binary() -> PathBuf {
    "restic".into()
}
fn service() -> String {
    "infuser.service".into()
}
impl Settings {
    /// Recovery must not depend on the source directory that just failed staging.
    pub fn load_for_resume(path: &Path) -> Result<Self> {
        let settings: Self = toml::from_str(&fs::read_to_string(path)?)?;
        ensure!(
            settings.service.ends_with(".service")
                && settings
                    .service
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "invalid service name"
        );
        for path in [
            &settings.controller_config,
            &settings.config_dir,
            &settings.work_dir,
            &settings.repository_file,
            &settings.password_file,
        ]
        .into_iter()
        .chain(settings.extra_paths.iter())
        .chain(settings.rclone_config.iter())
        {
            ensure!(path.is_absolute(), "backup paths must be absolute");
        }
        Ok(settings)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let settings = Self::load_for_resume(path)?;
        let config = crate::config::Config::load(&settings.controller_config)?;
        fs::create_dir_all(&settings.work_dir)?;
        fs::set_permissions(&settings.work_dir, fs::Permissions::from_mode(0o700))?;
        let work = settings.work_dir.canonicalize()?;
        for path in [&config.controller.state_dir, &settings.config_dir]
            .into_iter()
            .chain(settings.extra_paths.iter())
        {
            let source = path.canonicalize()?;
            ensure!(source.is_dir(), "backup source must be a directory");
            ensure!(
                !work.starts_with(&source) && !source.starts_with(&work),
                "backup workspace must be separate from backed-up paths"
            );
        }
        Ok(settings)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.restic);
        command
            .args(["--repository-file"])
            .arg(&self.repository_file)
            .arg("--password-file")
            .arg(&self.password_file)
            .env("RESTIC_CACHE_DIR", self.work_dir.join("cache"))
            .stdin(Stdio::null());
        if let Some(path) = &self.rclone_config {
            command.env("RCLONE_CONFIG", path);
        }
        command
    }
    fn restic_output(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.command().args(args).output()?;
        // Do not print arbitrary backend stderr: it can contain credential URLs.
        ensure!(
            output.status.success(),
            "restic {} failed (exit {:?}); previous healthy backup retained",
            args[0],
            output.status.code()
        );
        Ok(output.stdout)
    }
    pub fn initialize(&self) -> Result<()> {
        self.restic_output(&["init"])?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Entry {
    bytes: u64,
    sha256: String,
    mode: u32,
    uid: u32,
    gid: u32,
    link: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub owner: String,
    pub build: String,
    pub created: u64,
    pub volumes: BTreeMap<String, Volume>,
    pub images: Vec<String>,
    pub files: BTreeMap<String, Entry>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Volume {
    pub path: String,
    pub game: String,
}
fn inventory(root: &Path, directory: &Path, result: &mut BTreeMap<String, Entry>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = path.symlink_metadata()?;
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF8 backup path")?
            .to_owned();
        if relative == "snapshot.json" {
            continue;
        }
        let directory = metadata.is_dir();
        if directory {
            inventory(root, &path, result)?;
        }
        let link = if metadata.file_type().is_symlink() {
            Some(fs::read_link(&path)?)
        } else {
            None
        };
        ensure!(
            metadata.is_file() || directory || link.is_some(),
            "unsupported special file in backup"
        );
        let mut hash = Sha256::new();
        if let Some(target) = &link {
            hash.update(target.as_os_str().as_encoded_bytes());
        } else if !directory {
            let mut input = fs::File::open(&path)?;
            let mut buffer = [0; 65536];
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
        }
        result.insert(
            relative,
            Entry {
                bytes: if directory { 0 } else { metadata.len() },
                sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
                mode: metadata.mode(),
                uid: metadata.uid(),
                gid: metadata.gid(),
                link,
            },
        );
    }
    Ok(())
}
pub fn seal(
    root: &Path,
    owner: &str,
    volumes: BTreeMap<String, Volume>,
    images: Vec<String>,
) -> Result<()> {
    // Consolidate platform databases in the staging copy, never the live source.
    for name in ["platform.sqlite", "publish-captures/requests.sqlite"] {
        let path = root.join("state").join(name);
        if path.exists() {
            rusqlite::Connection::open(path)?
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
        }
    }
    let mut files = BTreeMap::new();
    inventory(root, root, &mut files)?;
    let manifest = Manifest {
        version: 1,
        owner: owner.into(),
        build: crate::BUILD_SHA.into(),
        created: crate::controller::now(),
        volumes,
        images,
        files,
    };
    crate::private_file::replace(&root.join("snapshot.json"), &serde_json::to_vec(&manifest)?)
}
pub fn verify(root: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(root.join("snapshot.json"))?)?;
    ensure!(manifest.version == 1, "unsupported snapshot version");
    let mut files = BTreeMap::new();
    inventory(root, root, &mut files)?;
    ensure!(
        files == manifest.files,
        "restored files, permissions or ownership differ from snapshot"
    );
    let state = root.join("state");
    for name in ["platform.sqlite", "publish-captures/requests.sqlite"] {
        let path = state.join(name);
        if path.exists() {
            let c = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            ensure!(
                integrity == "ok",
                "restored database failed integrity check"
            );
        }
    }
    let controller: crate::store::State =
        serde_json::from_slice(&fs::read(state.join("state.json"))?)?;
    ensure!(
        controller.owner == manifest.owner
            && controller.games.values().all(|g| !g.reserved())
            && controller.build_reservation.is_none(),
        "snapshot is not a settled controller state"
    );
    Ok(manifest)
}
fn id(value: &str) -> bool {
    (8..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Restic snapshots are only retired after a complete restore passes verification.
pub fn publish(settings: &Settings, staged: &Path) -> Result<String> {
    let manifest = verify(staged)?;
    let host = format!("infuser-{}", manifest.owner);
    let raw = settings.restic_output(&[
        "backup",
        "--json",
        "--host",
        &host,
        "--tag",
        &host,
        "--",
        staged.to_str().context("invalid staging path")?,
    ])?;
    let snapshot = String::from_utf8(raw)?
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| {
            v.get("snapshot_id")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .context("backup returned no snapshot ID")?;
    ensure!(id(&snapshot), "invalid snapshot ID");
    let restored = tempfile::Builder::new()
        .prefix("verify-")
        .tempdir_in(&settings.work_dir)?;
    settings.restic_output(&[
        "restore",
        &format!("{snapshot}:{}", staged.display()),
        "--target",
        restored.path().to_str().unwrap(),
        "--verify",
    ])?;
    verify(restored.path())?;
    let verified_at = crate::controller::now();
    crate::private_file::replace(
        &settings.work_dir.join("last-healthy.json"),
        &serde_json::to_vec(
            &serde_json::json!({"snapshot":snapshot,"verified_at":verified_at,"owner":manifest.owner,"build":manifest.build}),
        )?,
    )?;
    // Explicit IDs avoid accidental cross-host/path/tag grouping during retention.
    let snapshots: Vec<serde_json::Value> = serde_json::from_slice(&settings.restic_output(&[
        "snapshots",
        "--json",
        "--host",
        &host,
        "--tag",
        &host,
    ])?)?;
    let mut args = vec!["forget".to_owned()];
    for entry in snapshots {
        if let Some(old) = entry["id"].as_str() {
            ensure!(id(old), "invalid retained snapshot ID");
            if !old.starts_with(&snapshot) {
                args.push(old.into());
            }
        }
    }
    if args.len() > 1 {
        args.push("--prune".into());
        settings.restic_output(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    Ok(snapshot)
}

fn copy(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    // Preserve Unix owners/modes and symlinks. Never dereference links from games.
    let status = Command::new("cp")
        .args(["-a", "--reflink=auto", "--"])
        .arg(source.join("."))
        .arg(target)
        .status()?;
    ensure!(status.success(), "snapshot copy failed");
    Ok(())
}
fn bytes(path: &Path) -> Result<u64> {
    let m = path.symlink_metadata()?;
    if !m.is_dir() {
        return Ok(m.len());
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        total = total.saturating_add(bytes(&entry?.path())?);
    }
    Ok(total)
}
pub fn stage(config: &crate::config::Config, settings: &Settings, target: &Path) -> Result<()> {
    // Take the real controller lock after systemd stop; a rogue restart cannot race copying.
    let (_lock, state) = crate::store::Store::open(&config.controller.state_dir)?;
    ensure!(
        state.games.values().all(|g| !g.reserved()) && state.build_reservation.is_none(),
        "game servers must stop normally before backup"
    );
    let docker = |args: &[&str]| -> Result<Vec<u8>> {
        let output = Command::new("docker")
            .arg("--host")
            .arg(format!(
                "unix://{}",
                config.controller.docker_socket.display()
            ))
            .args(args)
            .output()?;
        ensure!(output.status.success(), "Docker backup inventory failed");
        Ok(output.stdout)
    };
    let owner_filter = format!("label={}={}", crate::docker::OWNER, state.owner);
    ensure!(
        docker(&["ps", "-q", "--filter", &owner_filter])?.is_empty(),
        "owned containers are still running; backup deferred"
    );
    let mut volumes = BTreeMap::new();
    let mut sources = Vec::new();
    let volume_names =
        String::from_utf8(docker(&["volume", "ls", "-q", "--filter", &owner_filter])?)?;
    for name in volume_names.lines() {
        let value: Vec<serde_json::Value> =
            serde_json::from_slice(&docker(&["volume", "inspect", name])?)?;
        let source = Path::new(
            value[0]["Mountpoint"]
                .as_str()
                .context("volume has no local mountpoint")?,
        );
        ensure!(
            source.is_absolute()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "invalid volume path"
        );
        let relative = format!("volumes/{name}");
        sources.push((source.to_owned(), target.join(&relative)));
        ensure!(
            value[0]["Driver"] == "local"
                && value[0]["Options"].as_object().is_none_or(|v| v.is_empty()),
            "backup supports managed local Docker volumes only"
        );
        let game = value[0]["Labels"][crate::docker::GAME]
            .as_str()
            .context("volume missing game identity")?;
        volumes.insert(
            name.into(),
            Volume {
                path: relative,
                game: game.into(),
            },
        );
    }
    let c = rusqlite::Connection::open_with_flags(
        config.controller.state_dir.join("platform.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let raw: String = c.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let data: crate::platform::Data = serde_json::from_str(&raw)?;
    drop(c);
    let images: std::collections::BTreeSet<_> = state
        .games
        .values()
        .map(|g| g.spec.image.clone())
        .chain(
            data.builds
                .values()
                .filter_map(|b| b.server.as_ref().map(|s| s.image.clone())),
        )
        .collect();
    sources.push((config.controller.state_dir.clone(), target.join("state")));
    sources.push((settings.config_dir.clone(), target.join("config")));
    for (n, path) in settings.extra_paths.iter().enumerate() {
        sources.push((path.clone(), target.join(format!("extra/{n}"))));
    }
    let mut required = 0u64;
    for (source, _) in &sources {
        required = required.saturating_add(bytes(source)?);
    }
    for image in &images {
        required = required.saturating_add(
            String::from_utf8(docker(&[
                "image",
                "inspect",
                "--format",
                "{{.Size}}",
                image,
            ])?)?
            .trim()
            .parse::<u64>()?,
        );
    }
    let disk = rustix::fs::statvfs(target)?;
    ensure!(
        disk.f_bavail.saturating_mul(disk.f_frsize)
            >= required
                .saturating_mul(2)
                .saturating_add((config.controller.disk_reserve_mb + 256) * 1024 * 1024),
        "not enough disk headroom for staging and a complete restore verification"
    );
    for (source, destination) in sources {
        copy(&source, &destination)?;
    }
    if !images.is_empty() {
        let file = fs::File::create(target.join("images.tar"))?;
        let status = Command::new("docker")
            .arg("--host")
            .arg(format!(
                "unix://{}",
                config.controller.docker_socket.display()
            ))
            .args(["image", "save"])
            .args(&images)
            .stdout(file)
            .status()?;
        ensure!(
            status.success(),
            "required game images could not be archived"
        );
    }
    seal(target, &state.owner, volumes, images.into_iter().collect())?;
    verify(target)?;
    Ok(())
}

fn systemctl(settings: &Settings, action: &str) -> Result<()> {
    ensure!(
        Command::new("systemctl")
            .args([action, &settings.service])
            .status()?
            .success(),
        "service {action} failed"
    );
    Ok(())
}
/// Also used by systemd ExecStopPost after a killed worker or staging failure.
pub fn resume(settings: &Settings) -> Result<()> {
    let marker = settings.work_dir.join("service-paused");
    if marker.exists() {
        systemctl(settings, "start")?;
        fs::remove_file(marker)?;
    }
    Ok(())
}
pub async fn run(settings: Settings) -> Result<String> {
    let result = run_inner(&settings).await;
    if result.is_err() {
        crate::private_file::replace(
            &settings.work_dir.join("last-failure.json"),
            &serde_json::to_vec(
                &serde_json::json!({"at":crate::controller::now(),"stage":"backup"}),
            )?,
        )?;
    }
    result
}
async fn run_inner(settings: &Settings) -> Result<String> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(settings.work_dir.join("backup.lock"))?;
    if lock.try_lock().is_err() {
        return Ok("Backup deferred: another backup is running.".into());
    }
    resume(settings)?;
    // Our exclusive lock proves no earlier worker owns these temporary trees.
    for entry in fs::read_dir(&settings.work_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir()
            && (name.starts_with("snapshot-") || name.starts_with("verify-"))
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    let config = crate::config::Config::load(&settings.controller_config)?;
    let client = crate::maintenance::Client::new(&config)?;
    // Validate remote access before causing even a short maintenance window.
    settings.restic_output(&["cat", "config"])?;
    let Some(lease) = client.acquire("backup", false).await? else {
        return Ok("Backup deferred: games or publishing are active.".into());
    };
    let stage_dir = tempfile::Builder::new()
        .prefix("snapshot-")
        .tempdir_in(&settings.work_dir)?;
    let stage_result = (|| -> Result<()> {
        crate::private_file::replace(
            &settings.work_dir.join("service-paused"),
            b"resume after backup staging\n",
        )?;
        systemctl(settings, "stop")?;
        stage(&config, settings, stage_dir.path())
    })();
    let resume_result = resume(settings);
    let _ = client.release(&lease).await;
    resume_result?;
    stage_result?;
    let result = publish(settings, stage_dir.path());
    match result {
        Ok(snapshot) => {
            let _ = fs::remove_file(settings.work_dir.join("last-failure.json"));
            Ok(format!(
                "Backup {snapshot} restored and verified; only the latest healthy snapshot retained."
            ))
        }
        Err(error) => {
            crate::private_file::replace(
                &settings.work_dir.join("last-failure.json"),
                &serde_json::to_vec(
                    &serde_json::json!({"at":crate::controller::now(),"stage":"remote_backup_or_verification"}),
                )?,
            )?;
            Err(error)
        }
    }
}
