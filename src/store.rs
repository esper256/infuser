use crate::config::Game;
use anyhow::{Context, Result, ensure};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub fn secret() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Sleeping,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lease {
    pub session: String,
    pub token: String,
    pub expires: u64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Instance {
    pub spec: Game,
    pub port: u16,
    pub phase: Phase,
    pub attempt: String,
    pub name: String,
    pub started: u64,
    pub idle_since: Option<u64>,
    pub retry_at: u64,
    pub error: Option<String>,
    pub leases: BTreeMap<String, Lease>,
}

impl Instance {
    pub fn reserved(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Ready | Phase::Stopping)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct State {
    pub version: u32,
    pub owner: String,
    pub games: BTreeMap<String, Instance>,
}

pub struct Store {
    dir: PathBuf,
    _lock: File,
}

impl Store {
    pub fn open(dir: &Path) -> Result<(Self, State)> {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(dir.join("controller.lock"))?;
        lock.try_lock()
            .context("another controller is using this state directory")?;
        let path = dir.join("state.json");
        let state = if path.exists() {
            serde_json::from_slice::<State>(&fs::read(path)?)?
        } else {
            State {
                version: 1,
                owner: secret()[..16].into(),
                games: BTreeMap::new(),
            }
        };
        ensure!(state.version == 1, "unsupported state version");
        Ok((
            Self {
                dir: dir.into(),
                _lock: lock,
            },
            state,
        ))
    }

    pub fn save(&self, state: &State) -> Result<()> {
        // A committed reservation always reaches disk before any Docker mutation.
        let path = self.dir.join("state.next");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&serde_json::to_vec(state)?)?;
        file.sync_all()?;
        fs::rename(path, self.dir.join("state.json"))?;
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}
