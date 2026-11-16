//! A bounded, offline installation worker and image assembly without build scripts.
use super::*;
use crate::config::Settings as ControllerSettings;
use sha2::{Digest, Sha256, Sha512};
use std::{fs, os::unix::fs::MetadataExt, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub type Progress = Arc<dyn Fn(Phase, &str) -> Result<()> + Send + Sync>;
const OWNER: &str = "org.infuser.package.owner";
const JOB: &str = "org.infuser.package.job";
const MAX_CONTEXT: u64 = 640 * 1024 * 1024;

#[async_trait::async_trait]
pub trait Engine: Send + Sync {
    async fn build(
        &self,
        owner: &str,
        key: &str,
        recipe: &Recipe,
        source: &Path,
        progress: Progress,
    ) -> Result<Artifact>;
    async fn cleanup(&self, owner: &str, key: &str, images: bool) -> Result<()>;
}
pub struct DockerWorker {
    pub settings: ControllerSettings,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
async fn bounded(mut reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        result.extend_from_slice(&buffer[..read.min(limit.saturating_sub(result.len()))]);
    }
    Ok(result)
}
impl DockerWorker {
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new("docker");
        c.args([
            "--host",
            &format!("unix://{}", self.settings.docker_socket.display()),
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        c
    }
    async fn run_input(&self, args: &[&str], seconds: u64, input: Option<&Path>) -> Result<String> {
        let mut command = self.command(args);
        if let Some(input) = input {
            command.stdin(Stdio::from(fs::File::open(input)?));
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (out, err, status) = tokio::time::timeout(Duration::from_secs(seconds), async {
            tokio::join!(
                bounded(stdout, LOG_BYTES),
                bounded(stderr, LOG_BYTES),
                child.wait()
            )
        })
        .await
        .context("packaging command timed out")?;
        let out = out?;
        let err = err?;
        ensure!(
            status?.success(),
            "packaging command failed: {}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
        Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        ))
    }
    async fn run(&self, args: &[&str], seconds: u64) -> Result<String> {
        self.run_input(args, seconds, None).await
    }
    async fn inspect(&self, kind: &str, id: &str) -> Result<Value> {
        let text = self.run(&[kind, "inspect", id], 15).await?;
        let values: Vec<Value> = serde_json::from_str(&text)?;
        values
            .into_iter()
            .next()
            .context("empty packaging inspection")
    }
    async fn archive(&self, container: &str, path: &Path) -> Result<()> {
        let mut child = self
            .command(&[
                "exec",
                container,
                "tar",
                "-C",
                "/work/context/app",
                "-cf",
                "-",
                ".",
            ])
            .spawn()?;
        let mut stdout = child.stdout.take().unwrap().take(MAX_CONTEXT + 1);
        let stderr = child.stderr.take().unwrap();
        let mut file = tokio::fs::File::create(path).await?;
        let (copy, err) = tokio::time::timeout(Duration::from_secs(60), async {
            tokio::join!(
                tokio::io::copy(&mut stdout, &mut file),
                bounded(stderr, 4096)
            )
        })
        .await
        .context("packaging export timed out")?;
        let bytes = copy?;
        ensure!(
            bytes <= MAX_CONTEXT,
            "installed server exceeds export limit"
        );
        file.flush().await?;
        ensure!(
            tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await??
                .success(),
            "packaging export failed: {}",
            String::from_utf8_lossy(&err?)
        );
        Ok(())
    }
    async fn build_inner(
        &self,
        owner: &str,
        key: &str,
        recipe: &Recipe,
        source: &Path,
        progress: &Progress,
    ) -> Result<Artifact> {
        let configuration = self
            .settings
            .node_packaging
            .as_ref()
            .context("managed Node packaging is disabled")?;
        let base = configuration
            .images
            .get(&recipe.node_major.to_string())
            .context("Node runtime unavailable")?
            .clone();
        let image = self
            .inspect("image", &base)
            .await
            .context("configured Node runtime image is not staged on this host")?;
        let architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        };
        ensure!(
            image["Os"] == "linux" && image["Architecture"] == architecture,
            "Node image does not match this host architecture"
        );
        let info: Value =
            serde_json::from_str(&self.run(&["info", "--format", "{{json .}}"], 15).await?)?;
        let disk = rustix::fs::statvfs(
            info["DockerRootDir"]
                .as_str()
                .context("Docker filesystem missing")?,
        )?;
        ensure!(
            disk.f_bavail.saturating_mul(disk.f_frsize)
                >= (self.settings.disk_reserve_mb + 2048) * 1024 * 1024,
            "packaging needs 2 GiB of working space above the host disk reserve"
        );
        let lock = fs::read(source.join("package-lock.json"))
            .context("server source needs package-lock.json")?;
        let packages = dependencies(&fs::read(source.join("package.json"))?, &lock)?;
        let work_root = self.settings.state_dir.join("packaging-work");
        fs::create_dir_all(&work_root)?;
        let staging = tempfile::tempdir_in(work_root)?;
        let input = staging.path().join("input");
        fs::create_dir_all(input.join("deps"))?;
        fs::create_dir(input.join("source"))?;
        copy_source(source, &input.join("source"))?;
        ensure!(
            input.join("source").join(&recipe.entry).is_file(),
            "server entry file was not uploaded"
        );
        fs::write(input.join("install.mjs"), include_bytes!("install.mjs"))?;
        let health = format!(
            "fetch({}).then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))",
            serde_json::to_string(&format!("http://127.0.0.1:8080{}", recipe.health_path))?
        );
        progress(
            Phase::Packaging,
            "Fetching integrity-checked registry tarballs.",
        )?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;
        let mut total = 0u64;
        for (i, dependency) in packages.iter().enumerate() {
            let mut response = http.get(&dependency.url).send().await?.error_for_status()?;
            ensure!(
                !response.status().is_redirection(),
                "registry redirects are unsupported"
            );
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                total += chunk.len() as u64;
                ensure!(
                    bytes.len() + chunk.len() <= 32 * 1024 * 1024 && total <= 128 * 1024 * 1024,
                    "dependency downloads exceed recipe limits"
                );
                bytes.extend_from_slice(&chunk);
            }
            ensure!(
                Sha512::digest(&bytes).as_slice() == dependency.integrity.as_slice(),
                "dependency integrity check failed"
            );
            fs::write(input.join("deps").join(format!("{i:04}.tgz")), bytes)?;
        }
        let name = format!("infuser-package-{owner}-{}", &digest(key.as_bytes())[..16]);
        let owner_label = format!("{OWNER}={owner}");
        let job_label = format!("{JOB}={key}");
        let meta = fs::metadata(&input)?;
        ensure!(
            meta.uid() > 0 && meta.gid() > 0,
            "run the packaging worker as the non-root controller service user"
        );
        let user = format!("{}:{}", meta.uid(), meta.gid());
        let mount = format!(
            "type=bind,source={},target=/input,readonly",
            input.display()
        );
        ensure!(
            !input.to_string_lossy().contains(','),
            "packaging state path cannot contain a comma"
        );
        progress(
            Phase::Packaging,
            "Installing dependencies offline; lifecycle scripts disabled.",
        )?;
        self.run(
            &[
                "run",
                "--detach",
                "--name",
                &name,
                "--network",
                "none",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                "--pids-limit",
                "64",
                "--user",
                &user,
                "--memory",
                "768m",
                "--memory-swap",
                "768m",
                "--cpus",
                "0.5",
                "--tmpfs",
                "/work:rw,nosuid,nodev,noexec,size=512m,nr_inodes=65536,mode=1777",
                "--label",
                &owner_label,
                "--label",
                &job_label,
                "--log-driver",
                "local",
                "--log-opt",
                "max-size=1m",
                "--log-opt",
                "max-file=1",
                "--mount",
                &mount,
                &base,
                "node",
                "/input/install.mjs",
                &recipe.node_major.to_string(),
            ],
            30,
        )
        .await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(250);
        loop {
            let result = self.run(&["exec",&name,"node","-e","const f=require('fs'); console.log(f.existsSync('/work/ready')?'ready':f.existsSync('/work/failed')?'failed':'working')"],10).await?;
            if result.trim() == "ready" {
                break;
            }
            if result.trim() == "failed" {
                let log = self.run(&["logs", "--tail", "150", &name], 10).await?;
                progress(Phase::Packaging, &log)?;
                anyhow::bail!("offline dependency installation failed; inspect server-build logs");
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "offline dependency installation timed out"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let context = staging.path().join("context.tar");
        self.archive(&name, &context).await?;
        self.run(&["rm", "--force", &name], 15).await?;
        let tag = format!("infuser-package-{owner}:{}", &digest(key.as_bytes())[..20]);
        progress(
            Phase::Packaging,
            "Assembling the server image from the checked recipe.",
        )?;
        // The assembly container is never started. Only a fixed recipe and its
        // bounded /app archive enter the image; no Dockerfile or RUN is accepted.
        let assembly = format!("{name}-assemble");
        self.run(
            &[
                "create",
                "--name",
                &assembly,
                "--network",
                "none",
                "--user",
                "1000:1000",
                "--workdir",
                "/app",
                "--entrypoint",
                "node",
                "--env",
                "PORT=8080",
                "--env",
                "INFUSER_PORT=8080",
                "--env",
                "NODE_ENV=production",
                "--health-cmd",
                &format!("node -e '{health}'"),
                "--health-interval",
                "1s",
                "--health-timeout",
                "3s",
                "--health-start-period",
                &format!("{}s", recipe.startup_seconds),
                "--health-start-interval",
                "1s",
                "--health-retries",
                "3",
                "--label",
                &owner_label,
                "--label",
                &job_label,
                &base,
                &format!("/app/{}", recipe.entry),
            ],
            30,
        )
        .await?;
        self.run_input(
            &["cp", "-", &format!("{assembly}:/app")],
            60,
            Some(&context),
        )
        .await?;
        self.run(&["commit", "--pause=false", &assembly, &tag], 120)
            .await?;
        let built = self.inspect("image", &tag).await?;
        let id = built["Id"]
            .as_str()
            .context("packaged image missing ID")?
            .to_owned();
        ensure!(pinned_image(&id), "invalid packaged image ID");
        progress(
            Phase::Checking,
            "Checking readiness in an isolated disposable server.",
        )?;
        let smoke = format!("{name}-check");
        let memory = format!("{}m", recipe.memory_mb.min(BUILD_MEMORY_MB));
        self.run(
            &[
                "run",
                "--detach",
                "--name",
                &smoke,
                "--network",
                "none",
                "--read-only",
                "--init",
                "--user",
                "1000:1000",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                "--pids-limit",
                "128",
                "--memory",
                &memory,
                "--memory-swap",
                &memory,
                "--cpus",
                "0.5",
                "--tmpfs",
                "/tmp:rw,nosuid,nodev,noexec,size=32m,mode=1777",
                "--label",
                &owner_label,
                "--label",
                &job_label,
                "--log-driver",
                "local",
                "--log-opt",
                "max-size=1m",
                "--log-opt",
                "max-file=1",
                &id,
            ],
            30,
        )
        .await?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(recipe.startup_seconds.min(90));
        loop {
            let state = self.inspect("container", &smoke).await?;
            if state["State"]["Health"]["Status"] == "healthy" {
                break;
            }
            if state["State"]["Running"] != true || tokio::time::Instant::now() >= deadline {
                let log = self.run(&["logs", "--tail", "150", &smoke], 10).await?;
                progress(Phase::Checking, &log)?;
                anyhow::bail!(
                    "server did not become ready; listen on PORT and provide the declared health endpoint"
                );
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(Artifact {
            image: id,
            image_bytes: built["Size"].as_u64().context("image size missing")?,
            base_image: base,
            lock_sha256: digest(&lock),
            recipe_version: RECIPE_VERSION,
        })
    }
}
fn copy_source(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "server source cannot contain symlinks");
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&dest)?;
            copy_source(&entry.path(), &dest)?;
        } else {
            ensure!(kind.is_file(), "server source must contain regular files");
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
#[async_trait::async_trait]
impl Engine for DockerWorker {
    async fn build(
        &self,
        owner: &str,
        key: &str,
        recipe: &Recipe,
        source: &Path,
        progress: Progress,
    ) -> Result<Artifact> {
        let result = tokio::time::timeout(
            Duration::from_secs(600),
            self.build_inner(owner, key, recipe, source, &progress),
        )
        .await;
        let result = result
            .context("server packaging deadline exceeded")
            .and_then(|r| r);
        self.cleanup(owner, key, result.is_err())
            .await
            .context("packaging cleanup failed; reservation retained")?;
        result
    }
    async fn cleanup(&self, owner: &str, key: &str, images: bool) -> Result<()> {
        let owner_filter = format!("label={OWNER}={owner}");
        let job_filter = format!("label={JOB}={key}");
        for kind in if images { &['c', 'i'][..] } else { &['c'][..] } {
            let ids = if *kind == 'c' {
                self.run(
                    &[
                        "container",
                        "ls",
                        "--all",
                        "--quiet",
                        "--filter",
                        &owner_filter,
                        "--filter",
                        &job_filter,
                    ],
                    15,
                )
                .await?
            } else {
                self.run(
                    &[
                        "image",
                        "ls",
                        "--quiet",
                        "--filter",
                        &owner_filter,
                        "--filter",
                        &job_filter,
                    ],
                    15,
                )
                .await?
            };
            for id in ids.lines().collect::<std::collections::BTreeSet<_>>() {
                ensure!(
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()),
                    "unexpected packaging object ID"
                );
                let object_kind = if *kind == 'c' { "container" } else { "image" };
                let object = self.inspect(object_kind, id).await?;
                ensure!(
                    object["Config"]["Labels"][OWNER] == owner
                        && object["Config"]["Labels"][JOB] == key,
                    "packaging object ownership mismatch"
                );
                self.run(&[object_kind, "rm", "--force", id], 30).await?;
            }
        }
        Ok(())
    }
}
