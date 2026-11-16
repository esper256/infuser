//! Disposable runs remain separate from immutable cache files and personal saves.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
#[derive(Serialize, Deserialize)]
struct Process {
    pid: u32,
    identity: String,
}
#[derive(Serialize, Deserialize)]
struct Record {
    artifact: String,
    launching: bool,
    processes: Vec<Process>,
}
const RECORD: &str = ".infuser-run.json";
fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().context("missing run parent")?)?;
    serde_json::to_writer(&mut staged, value)?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn read(path: &Path) -> Result<Record> {
    Ok(serde_json::from_slice(&fs::read(path.join(RECORD))?)?)
}
fn process(pid: u32) -> Result<Process> {
    Ok(Process {
        pid,
        identity: identity(pid)?.context("process exited before registration")?,
    })
}
#[cfg(target_os = "linux")]
fn identity(pid: u32) -> Result<Option<String>> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let rest = stat
        .rsplit_once(')')
        .context("unexpected process record")?
        .1;
    let start = rest
        .split_whitespace()
        .nth(19)
        .context("missing process start time")?;
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(Some(format!("{}:{start}", boot.trim())))
}
#[cfg(windows)]
fn identity(pid: u32) -> Result<Option<String>> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // Handle lifetime is local; no game-provided pointer or handle is used.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return if GetLastError() == ERROR_INVALID_PARAMETER {
                Ok(None)
            } else {
                Err(std::io::Error::last_os_error().into())
            };
        }
        let mut created: FILETIME = std::mem::zeroed();
        let mut exited: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let success = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if success == 0 {
            return Err(error.into());
        }
        if exited.dwLowDateTime != 0 || exited.dwHighDateTime != 0 {
            return Ok(None);
        }
        Ok(Some(format!(
            "{}:{}",
            created.dwHighDateTime, created.dwLowDateTime
        )))
    }
}
#[cfg(not(any(target_os = "linux", windows)))]
fn identity(_: u32) -> Result<Option<String>> {
    anyhow::bail!("process identity unavailable")
}
fn active(record: &Record) -> bool {
    record.launching
        || record.processes.iter().any(|p| match identity(p.pid) {
            Ok(Some(current)) => current == p.identity,
            Ok(None) => false,
            Err(_) => true,
        })
}
pub struct Run {
    directory: Option<tempfile::TempDir>,
}
impl Run {
    pub fn new(directory: tempfile::TempDir, artifact: &str) -> Result<Self> {
        write(
            &directory.path().join(RECORD),
            &Record {
                artifact: artifact.into(),
                launching: false,
                processes: vec![process(std::process::id())?],
            },
        )?;
        Ok(Self {
            directory: Some(directory),
        })
    }
    pub fn path(&self) -> &Path {
        self.directory.as_ref().unwrap().path()
    }
    pub fn spawning(&self) -> Result<()> {
        let mut record = read(self.path())?;
        record.launching = true;
        write(&self.path().join(RECORD), &record)
    }
    pub fn started(&self, pid: u32) -> Result<()> {
        let mut record = read(self.path())?;
        record.processes.push(process(pid)?);
        record.launching = false;
        write(&self.path().join(RECORD), &record)
    }
    // The caller observed process exit (or spawn failure); normal cleanup is safe.
    pub fn finished(&self) -> Result<()> {
        write(
            &self.path().join(RECORD),
            &Record {
                artifact: String::new(),
                launching: false,
                processes: vec![],
            },
        )
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        // If the companion exits unexpectedly while the game still runs, keep
        // its executable and envelope. A later launch can reclaim them safely.
        let live = read(self.path())
            .map(|r| {
                r.launching
                    || r.processes
                        .iter()
                        .filter(|p| p.pid != std::process::id())
                        .any(|p| {
                            identity(p.pid)
                                .map(|v| v.is_some_and(|v| v == p.identity))
                                .unwrap_or(true)
                        })
            })
            .unwrap_or(true);
        if live {
            let _ = self.directory.take().unwrap().keep();
        }
    }
}
pub fn prune(root: &Path, current: &str) -> Result<()> {
    let runs = root.join("runs");
    let mut retained = std::collections::BTreeSet::new();
    if runs.is_dir() {
        for entry in fs::read_dir(&runs)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir()
                || !entry.file_name().to_string_lossy().starts_with("game-")
            {
                continue;
            }
            // Unknown older runs are left intact: their parent may have exited while
            // the game remains alive. New runs always carry process identities.
            if let Ok(record) = read(&entry.path()) {
                if active(&record) {
                    retained.insert(record.artifact);
                } else {
                    fs::remove_dir_all(entry.path())?;
                }
            }
        }
    }
    let history_path = root.join("cache-history.json");
    let history: Vec<String> = fs::read(&history_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let previous = history.iter().find(|id| id.as_str() != current).cloned();
    let next: Vec<_> = std::iter::once(current.to_owned())
        .chain(previous)
        .collect();
    retained.extend(next.iter().cloned());
    let versions = root.join("versions");
    if versions.is_dir() {
        for entry in fs::read_dir(&versions)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_dir()
                && ((infuser_native_protocol::digest(&id) && !retained.contains(&id))
                    || id.starts_with(".incoming-"))
            {
                fs::remove_dir_all(entry.path())?;
            }
        }
    }
    write(&history_path, &next)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "subprocess fixture for a live native game"]
    fn held_process() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    #[test]
    fn prune_retains_current_previous_and_orphaned_live_game_but_not_dead_runs() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let versions = root.join("versions");
        let runs = root.join("runs");
        fs::create_dir_all(&runs)?;
        fs::create_dir_all(&versions)?;
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let c = "c".repeat(64);
        for id in [&a, &b] {
            fs::create_dir(versions.join(id))?;
        }
        write(&root.join("cache-history.json"), &vec![a.clone()])?;
        let live_path = tempfile::Builder::new()
            .prefix("game-")
            .tempdir_in(&runs)?
            .keep();
        let mut game = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "run_storage::tests::held_process", "--ignored"])
            .stdout(std::process::Stdio::null())
            .spawn()?;
        let record = Record {
            artifact: a.clone(),
            launching: false,
            processes: vec![process(game.id())?],
        };
        write(&live_path.join(RECORD), &record)?;
        prune(root, &b)?;
        fs::create_dir(versions.join(&c))?;
        prune(root, &c)?;
        assert!(
            versions.join(&a).exists(),
            "live orphan's cache was removed"
        );
        assert!(live_path.exists());
        game.kill()?;
        game.wait()?;
        prune(root, &c)?;
        assert!(!live_path.exists());
        assert!(!versions.join(&a).exists());
        assert!(versions.join(&b).exists() && versions.join(&c).exists());
        let uncertain = tempfile::Builder::new()
            .prefix("game-")
            .tempdir_in(&runs)?
            .keep();
        write(
            &uncertain.join(RECORD),
            &Record {
                artifact: a,
                launching: true,
                processes: vec![],
            },
        )?;
        prune(root, &c)?;
        assert!(uncertain.exists(), "uncertain spawn must be retained");
        Ok(())
    }
}
