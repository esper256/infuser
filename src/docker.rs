use crate::{config::Settings, store::Instance};
use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;

const OWNER: &str = "org.infuser.controller";
const GAME: &str = "org.infuser.game";
const ATTEMPT: &str = "org.infuser.attempt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Missing,
    Starting,
    Ready,
    Failed,
}

#[async_trait]
pub trait Engine: Send + Sync {
    async fn preflight(&self) -> Result<()>;
    async fn inventory(&self) -> Result<Vec<(String, String)>>;
    async fn ensure(&self, id: &str, instance: &Instance, owner: &str) -> Result<Health>;
    async fn health(&self, instance: &Instance, owner: &str, id: &str) -> Result<Health>;
    async fn remove(&self, id: &str, instance: &Instance, owner: &str) -> Result<()>;
}

pub struct Docker {
    pub settings: Settings,
}

impl Docker {
    async fn run(&self, args: &[String], seconds: u64) -> Result<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(seconds),
            Command::new("docker")
                .arg("--host")
                .arg(format!("unix://{}", self.settings.docker_socket.display()))
                .args(args)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("Docker command timed out; its outcome may be uncertain")??;
        ensure!(
            output.status.success(),
            "Docker command failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(500)
                .collect::<String>()
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    async fn cmd(&self, args: &[&str]) -> Result<String> {
        self.run(&args.iter().map(|s| (*s).into()).collect::<Vec<_>>(), 15)
            .await
    }

    async fn object(&self, kind: &str, name: &str) -> Result<Option<Value>> {
        // List success with no exact match is absence. Other errors are never treated as absence.
        let filter = if kind == "container" {
            format!("name=^/{name}$")
        } else {
            format!("name=^{name}$")
        };
        let list = if kind == "container" {
            self.cmd(&[
                kind,
                "ls",
                "--all",
                "--filter",
                &filter,
                "--format",
                "{{.Names}}",
            ])
            .await?
        } else {
            self.cmd(&[kind, "ls", "--filter", &filter, "--format", "{{.Name}}"])
                .await?
        };
        if !list.lines().any(|line| line == name) {
            return Ok(None);
        }
        let value: Vec<Value> = serde_json::from_str(&self.cmd(&[kind, "inspect", name]).await?)?;
        Ok(Some(
            value.into_iter().next().context("empty Docker inspect")?,
        ))
    }

    fn owned(
        value: &Value,
        kind: &str,
        owner: &str,
        id: &str,
        attempt: Option<&str>,
    ) -> Result<()> {
        let labels = if kind == "container" {
            &value["Config"]["Labels"]
        } else {
            &value["Labels"]
        };
        ensure!(
            labels[OWNER] == owner
                && labels[GAME] == id
                && attempt.is_none_or(|a| labels[ATTEMPT] == a),
            "{kind} ownership mismatch; refusing to modify it"
        );
        Ok(())
    }

    async fn info(&self) -> Result<Value> {
        Ok(serde_json::from_str(
            &self.cmd(&["info", "--format", "{{json .}}"]).await?,
        )?)
    }

    async fn prepare(&self, id: &str, instance: &Instance, owner: &str) -> Result<()> {
        let info = self.info().await?;
        let root = info["DockerRootDir"]
            .as_str()
            .context("Docker root directory missing")?;
        let disk =
            rustix::fs::statvfs(root).context("cannot measure free space on Docker filesystem")?;
        ensure!(
            disk.f_bavail as u128 * disk.f_frsize as u128
                >= self.settings.disk_reserve_mb as u128 * 1024 * 1024,
            "Docker filesystem is below the free-space reserve"
        );
        let images: Vec<Value> = serde_json::from_str(
            &self
                .cmd(&["image", "inspect", &instance.spec.image])
                .await?,
        )?;
        let image = images
            .first()
            .context("missing game image; stage it before launching")?;
        let architecture = match info["Architecture"].as_str().unwrap_or("") {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        };
        ensure!(
            image["Os"] == "linux" && image["Architecture"] == architecture,
            "game image must match the Docker host architecture ({architecture})"
        );
        let test = image["Config"]["Healthcheck"]["Test"][0]
            .as_str()
            .unwrap_or("");
        ensure!(
            test == "CMD" || test == "CMD-SHELL",
            "game image must define a readiness HEALTHCHECK"
        );
        ensure!(
            image["Config"]["Volumes"]
                .as_object()
                .is_none_or(|v| v.is_empty()),
            "image VOLUME declarations are unsupported; use persistent=true"
        );
        if let Some(network) = self.object("network", &instance.name).await? {
            Self::owned(&network, "network", owner, id, Some(&instance.attempt))?;
        } else {
            self.cmd(&[
                "network",
                "create",
                "--driver",
                "bridge",
                "--opt",
                "com.docker.network.bridge.enable_icc=false",
                "--label",
                &format!("{OWNER}={owner}"),
                "--label",
                &format!("{GAME}={id}"),
                "--label",
                &format!("{ATTEMPT}={}", instance.attempt),
                &instance.name,
            ])
            .await?;
        }
        if instance.spec.persistent {
            let name = format!("infuser-{owner}-{id}-data");
            if let Some(volume) = self.object("volume", &name).await? {
                Self::owned(&volume, "volume", owner, id, None)?;
            } else {
                self.cmd(&[
                    "volume",
                    "create",
                    "--label",
                    &format!("{OWNER}={owner}"),
                    "--label",
                    &format!("{GAME}={id}"),
                    &name,
                ])
                .await?;
            }
        }
        Ok(())
    }

    pub fn create_args(&self, id: &str, i: &Instance, owner: &str) -> Vec<String> {
        let g = &i.spec;
        let mut args: Vec<String> = [
            "container",
            "create",
            "--name",
            &i.name,
            "--network",
            &i.name,
            "--read-only",
            "--init",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges:true",
            "--pids-limit",
            "128",
            "--restart",
            "no",
            "--user",
            &g.user,
            "--memory",
            &format!("{}m", g.memory_mb),
            "--memory-swap",
            &format!("{}m", g.memory_mb),
            "--cpus",
            &format!("{}.{:03}", g.cpu_millis / 1000, g.cpu_millis % 1000),
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,noexec,size=32m,mode=1777",
            "--log-driver",
            "local",
            "--log-opt",
            "max-size=5m",
            "--log-opt",
            "max-file=2",
            "--label",
            "org.infuser.managed=true",
            "--label",
            &format!("{OWNER}={owner}"),
            "--label",
            &format!("{GAME}={id}"),
            "--label",
            &format!("{ATTEMPT}={}", i.attempt),
            "--env",
            &format!("INFUSER_GAME_ID={id}"),
            "--env",
            &format!("INFUSER_PORT={}", g.container_port),
            "--env",
            "INFUSER_DATA_DIR=/data",
        ]
        .iter()
        .map(|s| (*s).into())
        .collect();
        let bind = if g.web {
            "127.0.0.1".into()
        } else {
            self.settings.publish_ip.to_string()
        };
        for protocol in &g.protocols {
            args.extend([
                "--publish".into(),
                format!("{bind}:{}:{}/{protocol}", i.port, g.container_port),
            ]);
        }
        if g.persistent {
            args.extend([
                "--mount".into(),
                format!("type=volume,source=infuser-{owner}-{id}-data,target=/data"),
            ]);
        }
        args.push(g.image.clone());
        args
    }

    fn status(value: Option<Value>) -> Health {
        let Some(v) = value else {
            return Health::Missing;
        };
        if !v["State"]["Running"].as_bool().unwrap_or(false) {
            return Health::Failed;
        }
        match v["State"]["Health"]["Status"].as_str() {
            Some("healthy") => Health::Ready,
            Some("starting") => Health::Starting,
            _ => Health::Failed,
        }
    }
}

#[async_trait]
impl Engine for Docker {
    async fn preflight(&self) -> Result<()> {
        let v = self.info().await?;
        ensure!(v["OSType"] == "linux", "Linux Docker is required");
        for key in ["MemoryLimit", "SwapLimit", "CpuCfsQuota", "PidsLimit"] {
            ensure!(v[key] == true, "Docker cannot enforce {key}");
        }
        ensure!(
            v["MemTotal"].as_u64().unwrap_or(0) / 1024 / 1024
                >= self.settings.memory_budget_mb + 1024,
            "leave at least 1 GiB of host RAM outside the game budget"
        );
        ensure!(
            v["NCPU"].as_u64().unwrap_or(0) * 1000 >= self.settings.cpu_budget_millis,
            "CPU budget exceeds host capacity"
        );
        Ok(())
    }

    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        let output = self
            .cmd(&[
                "container",
                "ls",
                "--all",
                "--filter",
                "label=org.infuser.managed=true",
                "--format",
                "{{.Names}} {{.Label \"org.infuser.controller\"}}",
            ])
            .await?;
        output
            .lines()
            .map(|s| {
                s.split_once(' ')
                    .map(|(n, o)| (n.into(), o.into()))
                    .context("invalid Docker inventory")
            })
            .collect()
    }

    async fn ensure(&self, id: &str, i: &Instance, owner: &str) -> Result<Health> {
        let mut object = self.object("container", &i.name).await?;
        if let Some(v) = &object {
            Self::owned(v, "container", owner, id, Some(&i.attempt))?;
        } else {
            self.prepare(id, i, owner).await?;
            self.run(&self.create_args(id, i, owner), 20).await?;
            object = self.object("container", &i.name).await?;
        }
        if object
            .as_ref()
            .is_some_and(|v| v["State"]["Status"] == "created")
        {
            self.cmd(&["container", "start", &i.name]).await?;
        }
        self.health(i, owner, id).await
    }

    async fn health(&self, i: &Instance, owner: &str, id: &str) -> Result<Health> {
        let v = self.object("container", &i.name).await?;
        if let Some(v) = &v {
            Self::owned(v, "container", owner, id, Some(&i.attempt))?;
        }
        Ok(Self::status(v))
    }

    async fn remove(&self, id: &str, i: &Instance, owner: &str) -> Result<()> {
        if let Some(v) = self.object("container", &i.name).await? {
            Self::owned(&v, "container", owner, id, Some(&i.attempt))?;
            if v["State"]["Running"] == true {
                self.run(
                    &[
                        "container".into(),
                        "stop".into(),
                        "--time".into(),
                        i.spec.stop_seconds.to_string(),
                        i.name.clone(),
                    ],
                    i.spec.stop_seconds + 15,
                )
                .await?;
            }
            self.cmd(&["container", "rm", "--volumes", &i.name]).await?;
        }
        if let Some(v) = self.object("network", &i.name).await? {
            Self::owned(&v, "network", owner, id, Some(&i.attempt))?;
            self.cmd(&["network", "rm", &i.name]).await?;
        }
        if self.object("container", &i.name).await?.is_some() {
            bail!("container still present after removal");
        }
        Ok(())
    }
}
