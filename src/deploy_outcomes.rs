//! Activation results stored beside the platform databases. A snapshot restore
//! replaces only the database files and `state.json`, so this log survives the
//! rollback it describes.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

const FILE_NAME: &str = "deploy-outcomes.json";
const LOCK_NAME: &str = "deploy-outcomes.lock";
const LIMIT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Activated,
    RolledBack,
    Kept,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub release: String,
    pub at: u64,
    pub result: Kind,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub release: String,
    pub at: u64,
    pub reason: String,
}

#[derive(Serialize, Deserialize)]
struct Log {
    version: u32,
    #[serde(default)]
    dismissed: Vec<String>,
    #[serde(default)]
    outcomes: Vec<Outcome>,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            version: 1,
            dismissed: Vec::new(),
            outcomes: Vec::new(),
        }
    }
}

pub fn record(state_dir: &Path, outcome: Outcome) -> Result<()> {
    ensure!(
        release_id(&outcome.release),
        "invalid deploy outcome release"
    );
    ensure!(
        outcome.reason.len() <= 300 && !outcome.reason.chars().any(char::is_control),
        "deploy outcome reason must be short plain text"
    );
    with_log(state_dir, |log| {
        if log.outcomes.first().is_some_and(|item| {
            item.release == outcome.release
                && item.result == outcome.result
                && item.reason == outcome.reason
        }) {
            return Ok(());
        }
        log.outcomes.insert(0, outcome);
        log.outcomes.truncate(LIMIT);
        log.dismissed
            .retain(|release| log.outcomes.iter().any(|item| &item.release == release));
        Ok(())
    })
}

pub fn dismiss(state_dir: &Path, release: &str) -> Result<()> {
    ensure!(release_id(release), "invalid deploy outcome release");
    with_log(state_dir, |log| {
        ensure!(
            log.outcomes
                .iter()
                .any(|item| item.release == release && item.result == Kind::RolledBack),
            "no rollback notice for this release"
        );
        if !log.dismissed.iter().any(|item| item == release) {
            log.dismissed.push(release.to_owned());
        }
        Ok(())
    })
}

pub fn view(state_dir: &Path) -> (Option<Notice>, Vec<Outcome>) {
    let log = match load(state_dir) {
        Ok(log) => log,
        Err(error) => {
            if state_dir.join(FILE_NAME).exists() {
                tracing::warn!(%error, "deploy outcomes could not be read");
            }
            return (None, Vec::new());
        }
    };
    let notice = log
        .outcomes
        .iter()
        .find(|item| {
            item.result == Kind::RolledBack && !log.dismissed.iter().any(|id| id == &item.release)
        })
        .map(|item| Notice {
            release: item.release.clone(),
            at: item.at,
            reason: item.reason.clone(),
        });
    (notice, log.outcomes)
}

fn release_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn load(state_dir: &Path) -> Result<Log> {
    let path = state_dir.join(FILE_NAME);
    if !path.is_file() {
        return Ok(Log::default());
    }
    let log: Log = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(log.version == 1, "unsupported deploy outcome log");
    Ok(log)
}

fn with_log(state_dir: &Path, edit: impl FnOnce(&mut Log) -> Result<()>) -> Result<()> {
    fs::create_dir_all(state_dir)?;
    let lock_path = state_dir.join(LOCK_NAME);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o640)
        .open(&lock_path)?;
    share(state_dir, &lock_path)?;
    lock.lock()?;
    let mut log = load(state_dir)?;
    edit(&mut log)?;
    let path = state_dir.join(FILE_NAME);
    crate::private_file::replace(&path, &serde_json::to_vec(&log)?)?;
    share(state_dir, &path)?;
    Ok(())
}

fn share(state_dir: &Path, path: &Path) -> Result<()> {
    let owner = state_dir.symlink_metadata()?;
    std::os::unix::fs::chown(path, Some(owner.uid()), Some(owner.gid()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o640))?;
    Ok(())
}
