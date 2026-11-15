//! The installed executable stays put; only verified, versioned workers change.
//! Release trust is this project's HTTPS GitHub release, never a game/server URL.
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RELEASES: &str = "https://github.com/esper256/infuser/releases";
const BUILD: &str = match option_env!("INFUSER_BUILD_SHA") {
    Some(value) => value,
    None => "development",
};
const MAX_BINARY: u64 = 128 * 1024 * 1024;
#[cfg(target_os = "windows")]
const PLATFORM: &str = "windows-x86_64";
#[cfg(not(target_os = "windows"))]
const PLATFORM: &str = "linux-x86_64";
#[derive(Clone, Deserialize, Serialize)]
struct Version {
    commit: String,
    sha256: String,
}
#[derive(Default, Deserialize, Serialize)]
struct State {
    active: Option<Version>,
    #[serde(default)]
    previous: Option<Version>,
    rejected: Option<String>,
    checked: u64,
    #[serde(default)]
    launcher: String,
}
#[derive(Deserialize)]
struct Release {
    protocol: u32,
    commit: String,
    artifacts: std::collections::BTreeMap<String, Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    file: String,
    sha256: String,
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn filename() -> String {
    format!(
        "infuser-play-{PLATFORM}{}",
        if cfg!(windows) { ".exe" } else { "" }
    )
}
fn binary(dir: &Path, version: &Version) -> Result<PathBuf> {
    ensure!(
        hex(&version.commit, 40) && hex(&version.sha256, 64),
        "invalid update identity"
    );
    Ok(dir.join(format!(
        "{}{}",
        version.commit,
        if cfg!(windows) { ".exe" } else { "" }
    )))
}
fn read_state(dir: &Path) -> State {
    fs::read(dir.join("state.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
fn save_state(dir: &Path, state: &State) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut file, state)?;
    file.as_file().sync_all()?;
    file.persist(dir.join("state.json"))?;
    Ok(())
}
fn checked_binary(dir: &Path, version: &Version) -> Result<PathBuf> {
    let path = binary(dir, version)?;
    ensure!(
        fs::metadata(&path)?.len() <= MAX_BINARY,
        "update is too large"
    );
    ensure!(
        crate::install::hash(&fs::read(&path)?) == version.sha256,
        "update checksum mismatch"
    );
    Ok(path)
}
fn install(dir: &Path, version: &Version, reader: impl Read) -> Result<PathBuf> {
    let path = binary(dir, version)?;
    if checked_binary(dir, version).is_ok() {
        return Ok(path);
    }
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    let size = std::io::copy(&mut reader.take(MAX_BINARY + 1), &mut file)?;
    ensure!(size > 0 && size <= MAX_BINARY, "invalid update size");
    file.flush()?;
    ensure!(
        crate::install::hash(&fs::read(file.path())?) == version.sha256,
        "update checksum mismatch"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    file.as_file().sync_all()?;
    file.persist(&path)?;
    Ok(path)
}
fn wait(child: &mut Child, ready: impl Fn() -> bool, timeout: Duration) -> Result<bool> {
    let until = Instant::now() + timeout;
    loop {
        if ready() {
            return Ok(true);
        }
        if child.try_wait()?.is_some() {
            return Ok(false);
        }
        if Instant::now() >= until {
            child.kill().ok();
            child.wait().ok();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
fn handoff(path: &Path, dir: &Path, args: &[String]) -> Result<Option<Child>> {
    // A live GUI must acknowledge readiness before this launcher closes. If the
    // worker cannot start (DLL/runtime/display problems), the installed UI stays.
    let mut command = Command::new(path);
    command.arg("--infuser-managed").args(args);
    handoff_command(&mut command, dir, Duration::from_secs(15))
}
fn handoff_command(command: &mut Command, dir: &Path, timeout: Duration) -> Result<Option<Child>> {
    let ready = tempfile::NamedTempFile::new_in(dir)?;
    let mut child = command.env("INFUSER_PLAY_READY", ready.path()).spawn()?;
    if wait(
        &mut child,
        || fs::metadata(ready.path()).is_ok_and(|m| m.len() > 0),
        timeout,
    )? {
        Ok(Some(child))
    } else {
        Ok(None)
    }
}

pub fn acknowledge() {
    if let Some(path) = std::env::var_os("INFUSER_PLAY_READY") {
        let _ = fs::write(path, b"ready");
    }
}
fn fetch(dir: &Path, state: &mut State) -> Result<Option<Version>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if state.checked <= now && now - state.checked < 3600 {
        return Ok(None);
    }
    // Offline launches try again in an hour; they still use a working copy.
    state.checked = now;
    save_state(dir, state)?;
    let client = Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(12))
        .user_agent("Infuser-Play-Updater/1")
        .build()?;
    let response = client
        .get(format!("{RELEASES}/latest/download/play-release.json"))
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16384, "update manifest too large");
    let release: Release = serde_json::from_slice(&bytes)?;
    ensure!(
        release.protocol == 1 && hex(&release.commit, 40),
        "unsupported update manifest"
    );
    if release.commit == BUILD
        || state
            .active
            .as_ref()
            .is_some_and(|v| v.commit == release.commit && checked_binary(dir, v).is_ok())
        || state.rejected.as_ref() == Some(&release.commit)
    {
        return Ok(None);
    }
    let artifact = release
        .artifacts
        .get(PLATFORM)
        .context("no update for this platform")?;
    ensure!(
        artifact.file == filename() && hex(&artifact.sha256, 64),
        "invalid update artifact"
    );
    let version = Version {
        commit: release.commit,
        sha256: artifact.sha256.clone(),
    };
    let response = client
        .get(format!(
            "{RELEASES}/download/build-{}/{}",
            version.commit, artifact.file
        ))
        .timeout(Duration::from_secs(45))
        .send()?
        .error_for_status()?;
    install(dir, &version, response)?;
    Ok(Some(version))
}
/// Best effort: update failure always leaves the installed companion usable.
/// Called on a background thread while the existing window shows progress.
pub fn run(root: &Path, args: &[String]) -> Option<Child> {
    if !cfg!(all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "windows")
    )) || !hex(BUILD, 40)
    {
        return None;
    }
    run_inner(root, args).unwrap_or(None)
}
fn run_inner(root: &Path, args: &[String]) -> Result<Option<Child>> {
    let dir = root.join("play-updates");
    fs::create_dir_all(&dir)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("update.lock"))?;
    // Another launcher owns activation. Keep this launch independent.
    if lock.try_lock().is_err() {
        return Ok(cached(&dir, &read_state(&dir), args));
    }
    let mut state = read_state(&dir);
    if state.launcher != BUILD {
        state = State {
            launcher: BUILD.into(),
            ..Default::default()
        };
    }
    if let Some(version) = fetch(&dir, &mut state).unwrap_or(None) {
        if let Some(child) = checked_binary(&dir, &version)
            .ok()
            .and_then(|path| handoff(&path, &dir, args).unwrap_or(None))
        {
            state.previous = state.active.take();
            state.active = Some(version);
            state.rejected = None;
            save_state(&dir, &state).ok();
            prune(&dir, &state);
            return Ok(Some(child));
        }
        state.rejected = Some(version.commit);
        save_state(&dir, &state).ok();
    }
    Ok(cached(&dir, &state, args))
}
fn cached(dir: &Path, state: &State, args: &[String]) -> Option<Child> {
    if state.launcher != BUILD {
        return None;
    }
    for version in [&state.active, &state.previous].into_iter().flatten() {
        if version.commit != BUILD
            && let Some(child) = checked_binary(dir, version)
                .ok()
                .and_then(|path| handoff(&path, dir, args).unwrap_or(None))
        {
            return Some(child);
        }
    }
    None
}

fn prune(dir: &Path, state: &State) {
    // Only unreferenced version binaries; failed removal on Windows is harmless.
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let commit = name.strip_suffix(".exe").unwrap_or(&name);
        if hex(commit, 40)
            && !state.active.as_ref().is_some_and(|v| v.commit == commit)
            && !state.previous.as_ref().is_some_and(|v| v.commit == commit)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_is_atomic_and_never_replaces_a_good_worker_with_bad_bytes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let version = Version {
            commit: "a".repeat(40),
            sha256: crate::install::hash(b"working executable"),
        };
        assert!(install(dir.path(), &version, &b"truncated"[..]).is_err());
        assert!(!binary(dir.path(), &version)?.exists());
        let path = install(dir.path(), &version, &b"working executable"[..])?;
        assert_eq!(fs::read(&path)?, b"working executable");
        assert_eq!(install(dir.path(), &version, &b"bad retry"[..])?, path);
        let state = State {
            active: Some(version),
            rejected: None,
            checked: 123,
            ..Default::default()
        };
        save_state(dir.path(), &state)?;
        assert_eq!(read_state(dir.path()).checked, 123);
        fs::write(&path, b"corrupt")?;
        assert!(checked_binary(dir.path(), state.active.as_ref().unwrap()).is_err());
        assert!(
            binary(
                dir.path(),
                &Version {
                    commit: "../escape".into(),
                    sha256: "b".repeat(64)
                }
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn worker_handoff_requires_readiness_and_handles_exit_or_timeout() -> Result<()> {
        let dir = tempfile::tempdir()?;
        // The test executable acts as a child worker on both Windows and Linux.
        let exe = std::env::current_exe()?;
        let mut good = Command::new(&exe);
        good.args(["update::tests::worker_fixture", "--exact", "--ignored"]);
        handoff_command(&mut good, dir.path(), Duration::from_secs(5))?
            .expect("ready worker")
            .wait()?;
        let mut bad = Command::new(&exe);
        bad.args(["--invalid-test-argument"])
            .stderr(std::process::Stdio::null());
        assert!(handoff_command(&mut bad, dir.path(), Duration::from_secs(5))?.is_none());
        let mut stuck = Command::new(&exe);
        stuck
            .args(["update::tests::worker_fixture", "--exact", "--ignored"])
            .env("INFUSER_TEST_STUCK", "1");
        assert!(handoff_command(&mut stuck, dir.path(), Duration::from_millis(100))?.is_none());
        Ok(())
    }
    #[test]
    #[ignore]
    fn worker_fixture() {
        if std::env::var_os("INFUSER_TEST_STUCK").is_some() {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        acknowledge();
        std::thread::sleep(Duration::from_millis(100));
    }
}
