use super::*;
use crate::publish_limits as limits;
use crate::server_package::{
    self as package, Phase,
    worker::{Engine, Progress},
};

pub(super) fn capabilities(p: &Platform) -> Value {
    match &p.config().controller.node_packaging {
        None => json!({"version":1,"enabled":false}),
        Some(settings) => {
            json!({"version":1,"enabled":true,"recipes":[{"runtime":"node","node_majors":settings.images.keys().filter_map(|v| v.parse::<u16>().ok()).collect::<Vec<_>>(),"lockfile_versions":[2,3],"install_scripts":false,"entry_extensions":["js","mjs","cjs"],"port":8080}],"max_source_bytes":package::SOURCE_BYTES,"max_file_bytes":package::SOURCE_FILE_BYTES,"max_files":limits::FILE_COUNT})
        }
    }
}
pub(super) fn content_bytes(data: &platform::Data) -> u64 {
    data.builds
        .values()
        .flat_map(|b| b.files.values())
        .sum::<u64>()
        + data
            .server_builds
            .values()
            .flat_map(|j| j.files.values())
            .sum::<u64>()
        + data
            .metadata
            .values()
            .map(|m| m.card_art_bytes)
            .sum::<u64>()
}
impl Platform {
    fn server_source(&self, key: &str) -> PathBuf {
        self.config()
            .controller
            .state_dir
            .join("server-sources")
            .join(key)
    }
    /// Allows an alternate constrained worker, including the fake worker used by
    /// the fast publication journey. Production uses the Docker recipe worker.
    pub fn with_packager(mut self, engine: Arc<dyn Engine>) -> Self {
        self.packager = engine;
        self
    }

    pub async fn recover_packaging(&self) -> Result<()> {
        let state = self.controller.snapshot();
        let data = self.db.read()?;
        let mut interrupted: Vec<String> = data
            .server_builds
            .iter()
            .filter(|(_, j)| matches!(j.state, Phase::Packaging | Phase::Checking))
            .map(|(k, _)| k.clone())
            .collect();
        if let Some(r) = &state.build_reservation
            && !interrupted.contains(&r.job)
        {
            interrupted.push(r.job.clone());
        }
        for key in interrupted {
            let preserve = data
                .server_builds
                .get(&key)
                .is_some_and(|j| j.state == Phase::Ready);
            self.packager.cleanup(&state.owner, &key, !preserve).await?;
            self.db.edit(|d| {
                if let Some(job) = d.server_builds.get_mut(&key) && matches!(job.state,Phase::Packaging|Phase::Checking) {
                        job.state = Phase::Failed;
                        job.error = Some("packaging_interrupted".into());
                        job.log("The controller restarted during packaging. Retry this sealed build or upload a new build.");
                }
                Ok(())
            })?;
            if state
                .build_reservation
                .as_ref()
                .is_some_and(|r| r.job == key)
            {
                self.controller.release_build(&key)?;
            }
        }
        // Staging is private and disposable. A killed daemon cannot run tempfile
        // destructors; remove leftovers only after its owned workers are gone.
        let staging = self.config().controller.state_dir.join("packaging-work");
        if staging.exists() {
            std::fs::remove_dir_all(staging)?;
        }
        Ok(())
    }
    /// Process at most one durable job. Do not run concurrently with itself.
    pub async fn packaging_step(&self) -> Result<()> {
        let Ok(_work) = self.controller.maintenance.enter() else {
            return Ok(());
        };
        if self.controller.snapshot().build_reservation.is_some() {
            self.recover_packaging().await?;
        }
        let owner = self.controller.snapshot().owner;
        let data = self.db.read()?;
        for (key, job) in &data.server_builds {
            if !data.builds.contains_key(key) && !job.active() {
                self.packager.cleanup(&owner, key, true).await?;
                let source = self.server_source(key);
                if source.exists() {
                    std::fs::remove_dir_all(source)?;
                }
                self.db.edit(|d| {
                    d.server_builds.remove(key);
                    Ok(())
                })?;
                return Ok(());
            }
        }
        let Some(settings) = self.config().controller.node_packaging.as_ref() else {
            return Ok(());
        };
        let Some((key, job)) = data
            .server_builds
            .iter()
            .find(|(_, j)| j.state == Phase::Queued)
            .map(|(k, j)| (k.clone(), j.clone()))
        else {
            return Ok(());
        };
        if !self.controller.reserve_build(
            &key,
            package::BUILD_MEMORY_MB,
            package::BUILD_CPU_MILLIS,
        )? {
            return Ok(());
        }
        self.db.edit(|d| {
            let job = d
                .server_builds
                .get_mut(&key)
                .ok_or_else(|| fault(404, "server build disappeared"))?;
            job.state = Phase::Packaging;
            job.attempt += 1;
            job.started = now();
            job.error = None;
            job.log("Packaging started.");
            Ok(())
        })?;
        tracing::info!(job = %key, attempt = job.attempt + 1, "Server packaging started");
        let db = self.db.clone();
        let progress_key = key.clone();
        let progress: Progress = Arc::new(move |state, line| {
            db.edit(|d| {
                let job = d
                    .server_builds
                    .get_mut(&progress_key)
                    .ok_or_else(|| fault(404, "server build disappeared"))?;
                job.state = state;
                job.log(line);
                Ok(())
            })
        });
        let mut result = self
            .packager
            .build(
                &owner,
                &key,
                &job.recipe,
                &self.server_source(&key),
                progress,
            )
            .await;
        if let Ok(artifact) = &result {
            let used = self
                .db
                .read()?
                .server_builds
                .iter()
                .filter(|(k, _)| *k != &key)
                .filter_map(|(_, j)| j.artifact.as_ref())
                .map(|a| a.image_bytes)
                .sum::<u64>();
            if used.saturating_add(artifact.image_bytes) > settings.image_budget_mb * 1024 * 1024 {
                result = Err(fault(
                    507,
                    "server image budget is full; remove an unused build before retrying",
                ));
            }
        }
        // Even an engine failure must prove cleanup before releasing capacity.
        self.packager.cleanup(&owner, &key, result.is_err()).await?;
        self.db.edit(|d| {
            let job = d.server_builds.get_mut(&key).ok_or_else(|| fault(404,"server build disappeared"))?;
            match result {
                Ok(artifact) => {
                    tracing::info!(job = %key, image = %artifact.image, "Server packaging ready");
                    d.builds.get_mut(&key).ok_or_else(|| fault(404,"build disappeared"))?.server = Some(job.recipe.game(artifact.image.clone()));
                    job.artifact = Some(artifact); job.state = Phase::Ready; job.error = None;
                    job.log("Server image passed readiness checks. The paired build is ready to publish.");
                }
                Err(error) => {
                    // Detailed output remains in the app-scoped job log.
                    tracing::warn!(job = %key, "Server packaging failed; inspect publisher job logs");
                    job.state = Phase::Failed; job.artifact = None; job.error = Some("packaging_failed".into());
                    job.log(&format!("{error:#}"));
                }
            }
            Ok(())
        })?;
        self.controller.release_build(&key)?;
        Ok(())
    }
}

pub(super) async fn status(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let data = p.db.catalog()?;
    let job = data
        .server_builds
        .get(&build_key(&app, &build))
        .ok_or_else(|| fault(404, "build has no managed server recipe"))?;
    Ok(reply(json!(job)))
}
pub(super) async fn upload(
    State(p): State<Platform>,
    Path((app, build, path)): Path<(String, String, String)>,
    h: HeaderMap,
    axum::Extension(staged): axum::Extension<publication::Staged>,
    axum::Extension(mutation): axum::Extension<Arc<publication::Mutation>>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(
        slug(&app) && slug(&build) && package::source_path(&path),
        fault(
            400,
            "invalid server source path; hidden files, node_modules and credentials are excluded"
        )
    );
    ensure!(
        staged.bytes <= package::SOURCE_FILE_BYTES as u64,
        fault(
            413,
            &format!(
                "server source file exceeds {} MiB",
                package::SOURCE_FILE_BYTES as u64 / limits::MIB
            )
        )
    );
    tokio::task::spawn_blocking(move || -> Answer {
        let _mutation = mutation;
        let key = build_key(&app, &build);
        let target = p.server_source(&key).join(&path);
        std::fs::create_dir_all(target.parent().unwrap())?;
        let _guard = p.publishing.lock().unwrap();
        let key = build_key(&app, &build);
        let data = p.db.catalog()?;
        let job = data
            .server_builds
            .get(&key)
            .ok_or_else(|| fault(404, "create a build with server_recipe first"))?;
        ensure!(
            job.state == Phase::Uploading && data.builds.get(&key).is_some_and(|b| !b.ready),
            fault(
                409,
                "server build inputs are sealed; upload a new build to change them"
            )
        );
        let prior = job.files.get(&path).copied().unwrap_or(0);
        let size = staged.bytes;
        ensure!(
            job.files.len() < limits::FILE_COUNT || job.files.contains_key(&path),
            fault(413, "too many server source files")
        );
        ensure!(
            job.files.values().sum::<u64>() - prior + size <= package::SOURCE_BYTES,
            fault(
                413,
                &format!(
                    "server source exceeds {} MiB",
                    package::SOURCE_BYTES / limits::MIB
                )
            )
        );
        ensure!(
            content_bytes(&data) - prior + size
                <= p.config().controller.content_budget_mb * 1024 * 1024,
            fault(507, "installation content budget is full")
        );
        let disk = rustix::fs::statvfs(&p.config().controller.state_dir)?;
        ensure!(
            disk.f_bavail.saturating_mul(disk.f_frsize)
                >= p.config().controller.disk_reserve_mb * 1024 * 1024 + size * 2,
            fault(507, "host disk reserve would be exhausted")
        );
        std::fs::rename(staged.file.path(), &target)?;
        std::fs::File::open(target.parent().unwrap())?.sync_all()?;
        p.db.record_server_build_file(&key, path.clone(), size)?;
        Ok(reply(json!({"path":path,"bytes":size})))
    })
    .await
    .map_err(|e| fault(500, &format!("upload worker failed: {e}")))?
}
pub(super) async fn start(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    axum::Extension(mutation): axum::Extension<Arc<publication::Mutation>>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    tokio::task::spawn_blocking(move || -> Answer {
        let _mutation = mutation;
        let key = build_key(&app, &build);
        let data = p.db.catalog()?;
        let job = data
            .server_builds
            .get(&key)
            .ok_or_else(|| fault(404, "build has no managed server recipe"))?;
        ensure!(
            data.builds.contains_key(&key),
            fault(404, "build was removed")
        );
        if matches!(job.state, Phase::Uploading | Phase::Failed) {
            job.recipe
                .validate(p.config())
                .map_err(|e| fault(422, &e.to_string()))?;
            let source = p.server_source(&key);
            ensure!(
                job.files.contains_key(&job.recipe.entry)
                    && job.files.contains_key("package.json")
                    && job.files.contains_key("package-lock.json"),
                fault(
                    422,
                    "upload the entry file, package.json and package-lock.json before finalizing"
                )
            );
            package::dependencies(
                &std::fs::read(source.join("package.json"))?,
                &std::fs::read(source.join("package-lock.json"))?,
            )
            .map_err(|e| fault(422, &e.to_string()))?;
            ensure!(
                data.builds[&key].files.contains_key("index.html"),
                fault(
                    422,
                    "upload the client export before finalizing its paired server"
                )
            );
            let _guard = p.publishing.lock().unwrap();
            p.db.edit(|d| {
                let job = d.server_builds.get_mut(&key).unwrap();
                job.state = Phase::Queued;
                job.error = None;
                job.logs.clear();
                job.log("Queued; packaging starts when host capacity is available.");
                Ok(())
            })?;
        }
        let updated = p.db.catalog()?;
        let job = &updated.server_builds[&key];
        let mut response = reply(json!(job));
        if job.state != Phase::Ready {
            *response.status_mut() = StatusCode::ACCEPTED;
        }
        Ok(response)
    })
    .await
    .map_err(|e| fault(500, &format!("packaging preparation failed: {e}")))?
}
