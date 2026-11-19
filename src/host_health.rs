//! Independent host checks; the daemon cannot diagnose its own total outage.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub controller_config: PathBuf,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub containers: Vec<String>,
    pub backup_work_dir: Option<PathBuf>,
    pub updater_state_dir: Option<PathBuf>,
    /// Separate, sanitized textfile directory readable by node_exporter.
    pub metrics_dir: Option<PathBuf>,
    #[serde(default)]
    pub recover_unresponsive_daemon: bool,
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = toml::from_str(&fs::read_to_string(path)?)?;
        ensure!(
            value.controller_config.is_absolute(),
            "controller config path must be absolute"
        );
        for path in [
            &value.backup_work_dir,
            &value.updater_state_dir,
            &value.metrics_dir,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(path.is_absolute(), "operations paths must be absolute");
        }
        ensure!(
            value.services.len() <= 16 && value.containers.len() <= 16,
            "too many host checks"
        );
        for name in value.services.iter().chain(&value.containers) {
            ensure!(
                !name.is_empty()
                    && name.len() <= 128
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.@-".contains(&b)),
                "invalid service/container name"
            );
        }
        Ok(value)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub ok: bool,
    pub detail: String,
    pub action: String,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub at: u64,
    pub build: String,
    pub free_bytes: u64,
    pub checks: Vec<Check>,
    pub storage: BTreeMap<String, u64>,
    pub storage_at: u64,
    pub failures: BTreeMap<String, u32>,
    pub last_restart: u64,
    #[serde(default)]
    pub restarts: BTreeMap<String, u64>,
    #[serde(default)]
    pub backup_verified_at: u64,
    #[serde(default)]
    pub backup_configured: bool,
    #[serde(default)]
    pub maintenance: bool,
}
impl Report {
    pub fn observe(&mut self, checks: Vec<Check>, time: u64) {
        for check in &checks {
            let count = self.failures.entry(check.id.clone()).or_default();
            *count = if check.ok { 0 } else { count.saturating_add(1) };
        }
        self.failures
            .retain(|id, _| checks.iter().any(|c| &c.id == id));
        self.checks = checks;
        self.at = time;
        self.version = 1;
    }
    /// Low-cardinality host metrics; no accounts, game names, URLs or credentials.
    /// Grafana owns alert state, deduplication and notification delivery.
    pub fn prometheus(&self) -> String {
        let mut text = String::new();
        for (name, value) in [
            ("infuser_collector_timestamp_seconds", self.at),
            ("infuser_disk_available_bytes", self.free_bytes),
            ("infuser_storage_timestamp_seconds", self.storage_at),
            (
                "infuser_backup_verified_timestamp_seconds",
                self.backup_verified_at,
            ),
            ("infuser_maintenance", u64::from(self.maintenance)),
            (
                "infuser_backup_configured",
                u64::from(self.backup_configured),
            ),
            ("infuser_recovery_timestamp_seconds", self.last_restart),
        ] {
            text.push_str(&format!("# TYPE {name} gauge\n{name} {value}\n"));
        }
        text.push_str("# TYPE infuser_check_success gauge\n");
        for check in &self.checks {
            text.push_str(&format!(
                "infuser_check_success{{check=\"{}\"}} {}\n",
                label(&check.id),
                u8::from(check.ok)
            ));
        }
        text.push_str("# TYPE infuser_storage_bytes gauge\n");
        for (category, bytes) in &self.storage {
            text.push_str(&format!(
                "infuser_storage_bytes{{category=\"{}\"}} {bytes}\n",
                label(category)
            ));
        }
        text.push_str("# TYPE infuser_service_restarts gauge\n");
        for (service, count) in &self.restarts {
            text.push_str(&format!(
                "infuser_service_restarts{{service=\"{}\"}} {count}\n",
                label(service)
            ));
        }
        text
    }
}
fn label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}
async fn command(program: &str, args: &[&str]) -> Result<String> {
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(result.status.success(), "host check failed");
    Ok(String::from_utf8(result.stdout)?)
}
fn check(id: &str, ok: bool, detail: impl Into<String>, action: &str) -> Check {
    Check {
        id: id.into(),
        ok,
        detail: detail.into(),
        action: action.into(),
    }
}
pub fn tree_bytes(path: &Path) -> Result<u64> {
    let metadata = path.symlink_metadata()?;
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        bytes = bytes.saturating_add(tree_bytes(&entry?.path())?);
    }
    Ok(bytes)
}
pub async fn collect(settings: &Settings) -> Result<Report> {
    let config = crate::config::Config::load(&settings.controller_config)?;
    let root = &config.controller.state_dir;
    let report_file = root.join("host-health.json");
    let mut report: Report = fs::read(&report_file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let time = crate::controller::now();
    let mut checks = Vec::new();
    let disk = rustix::fs::statvfs(root)?;
    report.free_bytes = disk.f_bavail.saturating_mul(disk.f_frsize);
    checks.push(check("disk",report.free_bytes>=config.controller.disk_reserve_mb*1024*1024,
        format!("{} MiB free",report.free_bytes/1024/1024),"Review storage usage; reclaim disposable files or add capacity. Saves and worlds are never automatically deleted."));
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let token = fs::read_to_string(&config.controller.token_file)?;
    let api = http
        .get(format!(
            "http://127.0.0.1:{}/v1/operations",
            config.controller.api_port
        ))
        .bearer_auth(token.trim())
        .send()
        .await;
    let api = match api {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
        _ => None,
    };
    let api_ok = api.as_ref().is_some_and(|v| {
        v["database_ok"] == true
            && v["lifecycle_age_seconds"]
                .as_u64()
                .is_some_and(|age| age < 30)
    });
    if let Some(value) = &api {
        report.build = value["build"].as_str().unwrap_or("unknown").into();
    }
    let paused = settings
        .backup_work_dir
        .as_ref()
        .is_some_and(|p| p.join("service-paused").exists());
    report.maintenance = paused;
    checks.push(check(
        "controller",
        api_ok || paused,
        if paused {
            "Backup staging in progress"
        } else if api_ok {
            "API, database and lifecycle are responsive"
        } else {
            "Controller readiness check failed"
        },
        "Inspect infuser.service and its recent journal.",
    ));
    let docker_host = format!("unix://{}", config.controller.docker_socket.display());
    let docker_ok = command(
        "docker",
        &[
            "--host",
            &docker_host,
            "info",
            "--format",
            "{{.ServerVersion}}",
        ],
    )
    .await
    .is_ok();
    checks.push(check(
        "docker",
        docker_ok,
        "Docker Engine availability",
        "Inspect docker.service; controller recovery waits for owned resources.",
    ));
    let auth_ok = http
        .get(&config.controller.auth_url)
        .send()
        .await
        .is_ok_and(|r| matches!(r.status().as_u16(), 200 | 202 | 401 | 403));
    checks.push(check(
        "login",
        auth_ok,
        "Local login service availability",
        "Inspect the login container and its configuration.",
    ));
    // Public HTTPS and certificate checks belong to blackbox_exporter. Keep
    // recovery independent of external DNS/network failures.
    for service in &settings.services {
        let output = command(
            "systemctl",
            &[
                "show",
                service,
                "--property=ActiveState",
                "--property=MemoryMax",
                "--property=TasksMax",
                "--property=NRestarts",
            ],
        )
        .await;
        let ok = (paused && service == "infuser.service")
            || output
                .as_ref()
                .is_ok_and(|s| s.lines().any(|l| l == "ActiveState=active"));
        checks.push(check(
            &format!("service:{service}"),
            ok,
            format!("{service} {}", if ok { "active" } else { "unavailable" }),
            "Inspect the service journal and installed unit.",
        ));
        if service == "infuser.service"
            && let Ok(output) = output
        {
            let properties: BTreeMap<_, _> =
                output.lines().filter_map(|l| l.split_once('=')).collect();
            let adequate = properties
                .get("MemoryMax")
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|v| v >= 512 * 1024 * 1024)
                && properties
                    .get("TasksMax")
                    .and_then(|v| v.parse::<u64>().ok())
                    .is_some_and(|v| v >= 128);
            checks.push(check("service-limits",adequate,"Installed daemon limits match the supported minimum","Install the current service template and reload systemd; binary updates do not install units."));
            let restarts = properties
                .get("NRestarts")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let previous = report
                .restarts
                .insert(service.clone(), restarts)
                .unwrap_or(restarts);
            checks.push(check(
                "restart-loop",
                restarts.saturating_sub(previous) == 0 || restarts < 3,
                "Automatic daemon restart activity",
                "Inspect repeated failures before retrying or changing resource limits.",
            ));
        }
    }
    for container in &settings.containers {
        let output = command(
            "docker",
            &[
                "--host",
                &docker_host,
                "inspect",
                "--format",
                "{{.State.Running}} {{if .State.Health}}{{.State.Health.Status}}{{end}}",
                container,
            ],
        )
        .await;
        let ok = output
            .as_ref()
            .is_ok_and(|s| matches!(s.trim(), "true" | "true healthy"));
        checks.push(check(
            &format!("container:{container}"),
            ok,
            format!(
                "{container} {}",
                if ok {
                    "running"
                } else {
                    "unavailable or unhealthy"
                }
            ),
            "Inspect this shared service's logs and restart policy.",
        ));
    }
    report.backup_configured = settings.backup_work_dir.is_some();
    if let Some(work) = &settings.backup_work_dir {
        let healthy: Option<Value> = fs::read(work.join("last-healthy.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        report.backup_verified_at = healthy
            .as_ref()
            .and_then(|v| v["verified_at"].as_u64())
            .unwrap_or(0);
        let age = healthy
            .as_ref()
            .and_then(|v| v["verified_at"].as_u64())
            .map(|t| time.saturating_sub(t));
        checks.push(check("backup",age.is_some_and(|age|age<36*3600),age.map_or("No verified backup recorded".into(),|s|format!("Last verified backup {} hours ago",s/3600)),"Inspect infuser-backup.service, destination access and free space. A busy server defers backups."));
        checks.push(check("backup-attempt",!work.join("last-failure.json").exists(),"Latest backup attempt","Inspect infuser-backup.service; the previous healthy remote snapshot is retained after failure."));
    }
    if let Some(work) = &settings.updater_state_dir {
        checks.push(check("update",!work.join("failed-release").exists(),"Automatic deployment rollback status","Inspect infuser-update.service; a failed release remains held until a newer build or explicit retry."));
        checks.push(check("update-wait",!work.join("deferred-release").exists() || fs::metadata(work.join("deferred-release")).ok().and_then(|m|m.modified().ok()).is_some_and(|t|t.elapsed().unwrap_or_default()<Duration::from_secs(24*3600)),"Update still waiting to install","Open Admin tools to see what it is waiting on. Authorizing one update skips that wait."));
        let rollback = work.join("rollback");
        if rollback.join("last-failure.json").exists() || rollback.join("latest.json").exists() {
            let ok = crate::rollback::record_ok(&rollback);
            checks.push(check(
                "rollback-snapshot",
                ok,
                if ok {
                    "Pre-update database snapshot is present"
                } else {
                    "Pre-update database snapshot failed or is incomplete"
                },
                "Free disk space if the filesystem is tight, then inspect infuser-update.service. The running release was left in place.",
            ));
        }
    }
    if time.saturating_sub(report.storage_at) > 3600 {
        for category in [
            "content",
            "server-sources",
            "publish-captures",
            "runtime-diagnostics",
            "upload-staging",
        ] {
            let path = root.join(category);
            report.storage.insert(
                category.into(),
                if path.exists() { tree_bytes(&path)? } else { 0 },
            );
        }
        for category in ["platform.sqlite", "platform.sqlite-wal"] {
            report.storage.insert(
                category.into(),
                fs::metadata(root.join(category))
                    .map(|m| m.len())
                    .unwrap_or(0),
            );
        }
        report.storage_at = time;
    }
    report.observe(checks, time);
    if settings.recover_unresponsive_daemon
        && !paused
        && docker_ok
        && report.failures.get("controller").copied().unwrap_or(0) >= 3
        && time.saturating_sub(report.last_restart) >= 300
        && command("systemctl", &["is-active", "infuser.service"])
            .await
            .is_ok()
    {
        report.last_restart = time;
        // Persist the cooldown before acting, including if the recovery command fails.
        write_report(&report_file, &report)?;
        let _ = command("systemctl", &["--no-block", "restart", "infuser.service"]).await;
        tracing::warn!("Restart requested after three failed controller readiness checks");
    }
    write_report(&report_file, &report)?;
    if let Some(directory) = &settings.metrics_dir {
        fs::create_dir_all(directory)?;
        let path = directory.join("infuser.prom");
        crate::private_file::replace(&path, report.prometheus().as_bytes())?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    }
    Ok(report)
}
fn write_report(path: &Path, report: &Report) -> Result<()> {
    crate::private_file::replace(path, &serde_json::to_vec(report)?)?;
    // Contains sanitized status only. State directory still restricts access.
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}
