//! Durable operator authorization, independent of root-owned updater files.
use crate::{controller::now, private_file};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Checking,
    Downloading,
    Waiting,
    Activating,
    Current,
    Failed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub release: Option<String>,
    pub phase: Phase,
    #[serde(default)]
    pub maintenance_blocked: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub actor: String,
    pub current_build: String,
    pub expires: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub report: Option<Report>,
    pub checked_at: u64,
    pub available_since: Option<u64>,
    pub grant: Option<Grant>,
    pub last_request: Option<String>,
}
pub struct Tracker {
    path: PathBuf,
    record: Mutex<Record>,
}
impl Tracker {
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("deployment.json");
        let record = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Record::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            record: Mutex::new(record),
        })
    }
    pub fn read(&self) -> Record {
        self.record.lock().unwrap().clone()
    }
    pub fn edit<T>(&self, f: impl FnOnce(&mut Record) -> Result<T>) -> Result<T> {
        let mut saved = self.record.lock().unwrap();
        let mut next = saved.clone();
        let result = f(&mut next)?;
        private_file::replace(&self.path, &serde_json::to_vec(&next)?)?;
        *saved = next;
        Ok(result)
    }
    pub fn report(&self, mut report: Report) -> Result<()> {
        ensure!(
            report.release.as_ref().is_none_or(|s| valid_id(s, 40)),
            "invalid release ID"
        );
        self.edit(|r| {
            // Activation may have succeeded just before the updater could send
            // its final report. Do not carry that old wait into a later release.
            if r.report.as_ref().and_then(|v| v.release.as_deref()) == Some(crate::BUILD_SHA) {
                r.available_since = None;
            }
            if report.release.is_none() {
                report.release = r.report.as_ref().and_then(|old| old.release.clone());
            }
            r.checked_at = now();
            if report.phase == Phase::Current {
                r.available_since = None;
                r.grant = None;
            } else if report
                .release
                .as_deref()
                .is_some_and(|id| id != crate::BUILD_SHA)
            {
                r.available_since.get_or_insert(now());
            }
            r.report = Some(report);
            Ok(())
        })
    }
    pub fn authorize(&self, id: &str, current_build: &str, actor: &str) -> Result<()> {
        ensure!(
            valid_id(id, 32),
            crate::portal::fault(400, "Invalid request ID.")
        );
        ensure!(
            current_build == crate::BUILD_SHA,
            crate::portal::fault(409, "Infuser has already changed version. Reload first.")
        );
        self.edit(|r| {
            if r.last_request.as_deref() == Some(id) {
                return Ok(());
            }
            ensure!(
                r.available() && now().saturating_sub(r.checked_at) < 180,
                crate::portal::fault(409, "Wait for the updater to check GitHub again.")
            );
            ensure!(
                !r.report.as_ref().is_some_and(|v| v.phase == Phase::Failed),
                crate::portal::fault(
                    409,
                    "This update failed validation. Wait for a newer release."
                )
            );
            r.last_request = Some(id.into());
            r.grant = Some(Grant {
                id: id.into(),
                actor: actor.into(),
                current_build: current_build.into(),
                expires: now() + 600,
            });
            Ok(())
        })
    }
}
impl Record {
    pub fn available(&self) -> bool {
        self.report.as_ref().is_some_and(|r| {
            r.phase != Phase::Current
                && r.release
                    .as_deref()
                    .is_some_and(|id| id != crate::BUILD_SHA)
        })
    }
    pub fn authorized(&self) -> bool {
        self.grant
            .as_ref()
            .is_some_and(|g| g.expires > now() && g.current_build == crate::BUILD_SHA)
    }
}
fn valid_id(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permissions_expire_persist_and_do_not_follow_a_changed_running_build() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let tracker = Tracker::open(dir.path())?;
        tracker.report(Report {
            release: Some("b".repeat(40)),
            phase: Phase::Waiting,
            maintenance_blocked: false,
        })?;
        tracker.authorize(&"1".repeat(32), crate::BUILD_SHA, "operator")?;
        assert!(Tracker::open(dir.path())?.read().authorized());
        tracker.edit(|r| {
            r.grant.as_mut().unwrap().expires = now();
            Ok(())
        })?;
        assert!(!tracker.read().authorized());
        tracker.authorize(&"1".repeat(32), crate::BUILD_SHA, "operator")?;
        assert!(
            !tracker.read().authorized(),
            "replay must not renew an expired grant"
        );
        tracker.authorize(&"2".repeat(32), crate::BUILD_SHA, "operator")?;
        tracker.edit(|r| {
            r.grant.as_mut().unwrap().current_build = "old-build".into();
            Ok(())
        })?;
        assert!(!tracker.read().authorized());
        assert!(
            tracker
                .authorize(&"3".repeat(32), "old-build", "operator")
                .is_err()
        );
        tracker.report(Report {
            release: Some("c".repeat(40)),
            phase: Phase::Failed,
            maintenance_blocked: false,
        })?;
        assert!(
            tracker
                .authorize(&"3".repeat(32), crate::BUILD_SHA, "operator")
                .is_err()
        );
        tracker.report(Report {
            release: Some("c".repeat(40)),
            phase: Phase::Current,
            maintenance_blocked: false,
        })?;
        let record = Tracker::open(dir.path())?.read();
        assert!(record.grant.is_none());
        assert!(record.available_since.is_none());
        Ok(())
    }
}
