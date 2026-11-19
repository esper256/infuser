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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_at: Option<u64>,
    pub phase: Phase,
    #[serde(default)]
    pub maintenance_blocked: bool,
}
/// A host step named by `release.json`. Omitted from the manifest when empty.
/// An installed updater from before this field existed rejects a manifest that
/// includes it, so publish `host` only after that updater is installed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostStep {
    Reboot,
    Ansible,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostPolicy {
    /// Scheduled reboot or routine Ansible. Waits until play is idle.
    Strict,
    /// This step is required for a release to take effect. Same caps as activation.
    Release,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct HostWait {
    pub step: HostStep,
    pub policy: HostPolicy,
    pub since: u64,
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
    #[serde(default)]
    pub release_built_at: Option<u64>,
    pub checked_at: u64,
    pub available_since: Option<u64>,
    pub grant: Option<Grant>,
    pub last_request: Option<String>,
    /// Host steps the release named by `host_release` still needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_required: Vec<HostStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_release: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_wait: Option<HostWait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_id: Option<String>,
    /// When this release's host steps were first recorded. The release-required
    /// reboot or Ansible wait is measured from here, including after the binary
    /// is already current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_appeared: Option<u64>,
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
            // Keep timestamp metadata outside the strict Report stored on disk so
            // rollback binaries can still deserialize deployment.json.
            let same_release =
                r.report.as_ref().and_then(|old| old.release.as_ref()) == report.release.as_ref();
            r.release_built_at = report.built_at.take().or(if same_release {
                r.release_built_at
            } else {
                None
            });
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
    /// Replace the host steps required by `release`. An empty list means this
    /// release needs no reboot or Ansible to take effect.
    pub fn note_host(&self, release: &str, steps: &[HostStep]) -> Result<()> {
        ensure!(valid_id(release, 40), "invalid release ID");
        let steps = canonical_steps(steps);
        self.edit(|record| {
            if record.host_release.as_deref() != Some(release) {
                record.host_wait = None;
                record.host_appeared = Some(now());
            }
            record.host_release = Some(release.to_owned());
            record.host_required = steps.clone();
            if steps.is_empty() {
                record.host_appeared = None;
            } else {
                record.host_appeared.get_or_insert(now());
            }
            Ok(())
        })
    }
    pub fn host_required(&self, step: HostStep) -> bool {
        let record = self.read();
        record.host_required.contains(&step)
            && record.host_release.as_deref().is_some_and(|release| {
                release == crate::BUILD_SHA
                    || record
                        .report
                        .as_ref()
                        .and_then(|report| report.release.as_deref())
                        == Some(release)
            })
    }
    pub fn note_host_wait(&self, step: HostStep, policy: HostPolicy) -> Result<()> {
        self.edit(|record| {
            if record
                .host_wait
                .as_ref()
                .is_none_or(|wait| wait.step != step || wait.policy != policy)
            {
                record.host_wait = Some(HostWait {
                    step,
                    policy,
                    since: now(),
                });
            }
            Ok(())
        })
    }
    pub fn clear_host_wait(&self, step: HostStep) -> Result<()> {
        self.edit(|record| {
            if record
                .host_wait
                .as_ref()
                .is_some_and(|wait| wait.step == step)
            {
                record.host_wait = None;
            }
            Ok(())
        })
    }
    pub fn complete_host_step(&self, step: HostStep) -> Result<()> {
        self.edit(|record| {
            record.host_required.retain(|required| *required != step);
            if record
                .host_wait
                .as_ref()
                .is_some_and(|wait| wait.step == step)
            {
                record.host_wait = None;
            }
            Ok(())
        })
    }
    /// A new boot means a required reboot has happened. The first observation
    /// only records the boot, so opening the controller does not clear a step.
    pub fn observe_boot(&self) -> Result<()> {
        let Ok(boot) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") else {
            return Ok(());
        };
        let boot = boot.trim();
        if boot.is_empty() {
            return Ok(());
        }
        self.edit(|record| {
            if record.boot_id.as_deref() != Some(boot) {
                if record.boot_id.is_some() {
                    record
                        .host_required
                        .retain(|step| *step != HostStep::Reboot);
                    if record
                        .host_wait
                        .as_ref()
                        .is_some_and(|wait| wait.step == HostStep::Reboot)
                    {
                        record.host_wait = None;
                    }
                }
                record.boot_id = Some(boot.to_owned());
            }
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
fn canonical_steps(steps: &[HostStep]) -> Vec<HostStep> {
    let mut out = Vec::new();
    for step in [HostStep::Reboot, HostStep::Ansible] {
        if steps.contains(&step) {
            out.push(step);
        }
    }
    out
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
    fn build_age_follows_release_identity_and_keeps_rollback_state_readable() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let tracker = Tracker::open(dir.path())?;
        let report = |release, built_at| Report {
            release,
            built_at,
            phase: Phase::Waiting,
            maintenance_blocked: false,
        };
        tracker.report(report(Some("b".repeat(40)), Some(123)))?;
        assert_eq!(
            Tracker::open(dir.path())?.read().release_built_at,
            Some(123)
        );
        let disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("deployment.json"))?)?;
        assert!(
            disk["report"].get("built_at").is_none(),
            "strict legacy Report must remain readable"
        );
        tracker.report(report(None, None))?;
        assert_eq!(tracker.read().release_built_at, Some(123));
        tracker.report(report(Some("b".repeat(40)), None))?;
        assert_eq!(tracker.read().release_built_at, Some(123));
        tracker.report(report(Some("c".repeat(40)), None))?;
        assert_eq!(
            tracker.read().release_built_at,
            None,
            "never reuse a previous release's age"
        );
        Ok(())
    }
    #[test]
    fn permissions_expire_persist_and_do_not_follow_a_changed_running_build() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let tracker = Tracker::open(dir.path())?;
        tracker.report(Report {
            release: Some("b".repeat(40)),
            built_at: None,
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
            built_at: None,
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
            built_at: None,
            phase: Phase::Current,
            maintenance_blocked: false,
        })?;
        let record = Tracker::open(dir.path())?.read();
        assert!(record.grant.is_none());
        assert!(record.available_since.is_none());
        Ok(())
    }
}
