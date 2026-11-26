//! Bounded publisher work, with artifact IO outside the gameplay coordination lock.
use super::*;
use axum::{
    extract::{FromRequestParts, MatchedPath},
    middleware::Next,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{OwnedMutexGuard, OwnedSemaphorePermit};

pub(super) struct Mutation {
    _maintenance: Option<Arc<crate::maintenance::Work>>,
    _slot: OwnedSemaphorePermit,
    _app: OwnedMutexGuard<()>,
}
#[derive(Clone)]
pub(super) struct Staged {
    pub file: Arc<tempfile::NamedTempFile>,
    pub bytes: u64,
    pub sha256: String,
}
impl Platform {
    pub(super) fn app_lock(&self, app: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.app_mutations.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(app).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(app.to_owned(), Arc::downgrade(&lock));
        lock
    }
}
// This middleware wraps capture and body extraction, sharing one installation-wide
// admission semaphore across every publisher route. Busy requests never buffer bodies.
pub(super) async fn admit(State(p): State<Platform>, request: Request, next: Next) -> Response {
    if !matches!(
        request.method().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    ) {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let app = Path::<BTreeMap<String, String>>::from_request_parts(&mut parts, &p)
        .await
        .ok()
        .and_then(|Path(mut values)| values.remove("app"));
    let Some(app) = app else {
        return next.run(Request::from_parts(parts, body)).await;
    };
    let route = parts
        .extensions
        .get::<MatchedPath>()
        .map(|p| p.as_str())
        .unwrap_or("")
        .to_owned();
    let authorization = if route.starts_with("/api/metadata/")
        || route.ends_with("/metadata")
        || route.ends_with("/card-art")
    {
        metadata::authorize(&p, &parts.headers, &app)
    } else {
        p.publisher(&parts.headers, Some(&app), "publish")
            .map(|_| ())
    };
    if let Err(error) = authorization {
        return Error(error).into_response();
    }
    let Ok(slot) = p.publication_slots.clone().try_acquire_owned() else {
        let mut response = Error(fault(
            503,
            "Publishing is busy; retry this request shortly.",
        ))
        .into_response();
        response
            .headers_mut()
            .insert("retry-after", "1".parse().unwrap());
        return response;
    };
    let lock = p.app_lock(&app);
    let guard =
        match tokio::time::timeout(std::time::Duration::from_secs(5), lock.lock_owned()).await {
            Ok(lock) => Arc::new(Mutation {
                _maintenance: parts
                    .extensions
                    .get::<Arc<crate::maintenance::Work>>()
                    .cloned(),
                _slot: slot,
                _app: lock,
            }),
            Err(_) => {
                let mut response = Error(fault(
                    503,
                    "Another update for this game is still running; retry shortly.",
                ))
                .into_response();
                response
                    .headers_mut()
                    .insert("retry-after", "1".parse().unwrap());
                return response;
            }
        };
    parts.extensions.insert(guard.clone());
    if route.ends_with("/files/{*path}") {
        let limit = if route.contains("/server/files/") {
            crate::server_package::SOURCE_FILE_BYTES as u64
        } else {
            64 * 1024 * 1024
        };
        match stage(&p, body, limit).await {
            Ok(staged) => {
                parts.extensions.insert(staged);
                next.run(Request::from_parts(parts, Body::empty())).await
            }
            Err(error) => Error(error).into_response(),
        }
    } else {
        next.run(Request::from_parts(parts, body)).await
    }
}
async fn stage(p: &Platform, body: Body, limit: u64) -> Result<Staged> {
    let root = p.config().controller.state_dir.join("upload-staging");
    tokio::fs::create_dir_all(&root).await?;
    let disk = rustix::fs::statvfs(&root)?;
    // Two admitted uploads may each consume 64 MiB. Reserve both before writing.
    anyhow::ensure!(
        disk.f_bavail.saturating_mul(disk.f_frsize)
            >= p.config().controller.disk_reserve_mb * 1024 * 1024 + 128 * 1024 * 1024,
        fault(507, "host disk reserve would be exhausted")
    );
    let file = Arc::new(tempfile::NamedTempFile::new_in(root)?);
    let mut output = tokio::fs::File::from_std(file.reopen()?);
    let mut input = body.into_data_stream();
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    while let Some(chunk) = tokio::time::timeout_at(deadline, input.next())
        .await
        .map_err(|_| fault(408, "Upload timed out; retry the file."))?
    {
        let chunk = chunk.map_err(|_| fault(400, "Upload was interrupted; retry the file."))?;
        bytes = bytes.saturating_add(chunk.len() as u64);
        anyhow::ensure!(bytes <= limit, fault(413, "file exceeds the upload limit"));
        digest.update(&chunk);
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    output.sync_all().await?;
    Ok(Staged {
        file,
        bytes,
        sha256: digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}

pub(super) async fn capabilities(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let app = p.publisher(&h, None, "publish")?;
    Ok(reply(
        json!({"sessions":crate::dedicated::capabilities(),"diagnostic_events":{"version":1,"retention_hours":48},"server_diagnostics":crate::docker::diagnostics_capabilities(),"server_packaging":server_builds::capabilities(&p),"game_metadata":{"input_methods":metadata::INPUT_METHODS,"title_locked":true,"duplicate_titles":{"version":1,"confirm":"allow_duplicate_title","normalization":"trim-lowercase"},"orientations":["landscape","portrait"],"resolutions":["hd","native"],"version":1,"title_max_chars":80,"description_max_chars":2000,"art_prompt_max_chars":4000,"author_model_max_chars":120,"effort_level_max_chars":40,"estimated_prompts_max":1_000_000,"card_art_override":true},"native_delivery":{"version":1,"status":"preview","engine":"godot","platforms":infuser_native_protocol::PLATFORMS},"card_art":crate::card_art::capabilities(),"supported_engines":crate::publisher::supported_engines(),"sdk":crate::sdk_contract::with_peerjs(p.relay.is_some(), p.config().controller.peerjs_port.is_some()),"schema_version":1,"app_id":app,"architecture":std::env::consts::ARCH,"client_types":["web"],"play_modes":["solo","multiplayer"],"multiplayer":{"peer_hosted":p.relay.is_some(),"relayed":false,"dedicated":true,"persistent_world":true},"session_policy":"one_per_app","runtime_sessions":runtime::capabilities(),"presence":presence::capabilities(),"launch_intents":crate::launch_intent::capabilities(),"max_file_bytes":67108864,"max_build_bytes":268435456,"cloud_saves":true,"cloud_checkpoints":crate::cloud::capabilities(p.config().controller.cloud_save_budget_mb),"guest_mode":true,"sdk_path":"/_infuser/sdk.js"}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuildRequest {
    manifest: Manifest,
    server: Option<Game>,
    server_recipe: Option<crate::server_package::Recipe>,
    #[serde(default)]
    spa: bool,
    #[serde(default)]
    cross_origin_isolation: bool,
}
pub(super) async fn create_build(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    Json(body): Json<BuildRequest>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(
        slug(&app) && slug(&build) && body.manifest.app_id == app,
        fault(400, "invalid or mismatched app/build ID")
    );
    body.manifest
        .validate()
        .map_err(|e| fault(400, &e.to_string()))?;
    ensure!(
        body.server.is_none() || body.server_recipe.is_none(),
        fault(400, "provide server or server_recipe, not both")
    );
    if let Some(recipe) = &body.server_recipe {
        recipe
            .validate(p.config())
            .map_err(|e| fault(422, &e.to_string()))?;
        ensure!(
            body.manifest
                .multiplayer
                .as_ref()
                .is_none_or(|m| m.mode == Mode::Dedicated
                    && m.transport == crate::manifest::Transport::Websocket),
            fault(
                422,
                "the Node recipe currently supports ephemeral dedicated WebSocket servers"
            )
        );
    }
    if let Some(sdk) = &body.manifest.sdk {
        sdk.admit(&crate::sdk_contract::with_peerjs(
            p.relay.is_some(),
            p.config().controller.peerjs_port.is_some(),
        ))
        .map_err(|e| fault(422, &e.to_string()))?;
    }
    if let Some(m) = &body.manifest.multiplayer {
        ensure!(
            m.adapter.is_none() || p.config().controller.peerjs_port.is_some(),
            fault(422, "PeerJS is not configured on this Infuser")
        );
        ensure!(
            matches!(m.mode, Mode::Dedicated | Mode::PersistentWorld)
                || (m.mode == Mode::PeerHosted
                    && m.transport == crate::manifest::Transport::Webrtc
                    && m.max_players <= 8
                    && p.relay.is_some()),
            fault(
                422,
                "runtime unavailable: peer hosting needs configured WebRTC/TURN and at most 8 players; direct-only networking is unsupported"
            )
        );
    }
    let needs_server = body
        .manifest
        .multiplayer
        .as_ref()
        .is_some_and(|m| matches!(m.mode, Mode::Dedicated | Mode::PersistentWorld))
        || matches!(body.manifest.solo_runtime, Some(SoloRuntime::SharedServer));
    ensure!(
        needs_server == (body.server.is_some() || body.server_recipe.is_some()),
        fault(
            400,
            "server configuration must match the declared play modes"
        )
    );
    if let Some(server) = &body.server {
        let mut cfg = p.config().clone();
        cfg.games.clear();
        cfg.games.insert(app.clone(), server.clone());
        cfg.validate().map_err(|e| fault(400, &e.to_string()))?;
        if let Some(m) = &body.manifest.multiplayer {
            use crate::manifest::Transport;
            let compatible = match m.transport {
                Transport::Websocket => server.web,
                Transport::Tcp => !server.web && server.protocols.iter().any(|p| p == "tcp"),
                Transport::Udp => !server.web && server.protocols.iter().any(|p| p == "udp"),
                Transport::Webrtc => false,
            };
            ensure!(
                compatible,
                fault(
                    422,
                    "server protocol must match the declared multiplayer transport"
                )
            );
        }
    }
    let _guard = p.publishing.lock().unwrap();
    p.db.edit(|d| {
        let key = build_key(&app, &build);
        ensure!(
            !d.builds.contains_key(&key) && !d.server_builds.contains_key(&key),
            fault(
                409,
                "build already exists; inspect it or choose a new immutable build ID"
            )
        );
        ensure!(
            d.builds
                .keys()
                .filter(|k| k.starts_with(&format!("{app}/")))
                .count()
                < 12,
            fault(
                413,
                "remove an unused build before uploading another (12 per app)"
            )
        );
        d.builds.insert(
            key.clone(),
            Build {
                manifest: body.manifest,
                server: body.server,
                spa: body.spa,
                cross_origin_isolation: body.cross_origin_isolation,
                ready: false,
                created: now(),
                files: Default::default(),
            },
        );
        if let Some(recipe) = body.server_recipe {
            d.server_builds
                .insert(key, crate::server_package::Job::new(recipe));
        }
        Ok(())
    })?;
    Ok(reply(
        json!({"app_id":app,"build_id":build,"state":"uploading"}),
    ))
}
pub(super) async fn build_status(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let d = p.db.read()?;
    let b = d
        .builds
        .get(&build_key(&app, &build))
        .ok_or_else(|| fault(404, "unknown build"))?;
    let mut result = json!(b);
    if let Some(job) = d.server_builds.get(&build_key(&app, &build)) {
        result["server_build"] = json!({"state":job.state,"error":job.error});
    }
    result["publication_state"] = json!(if d
        .pending_promotions
        .get(&app)
        .is_some_and(|p| p.build == build)
    {
        "queued"
    } else if d.apps.get(&app).is_some_and(|a| a.build == build) {
        "published"
    } else if b.ready {
        "retained"
    } else {
        "uploading"
    });
    Ok(reply(result))
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
        slug(&app) && slug(&build) && valid_path(&path),
        fault(400, "invalid artifact path")
    );
    tokio::task::spawn_blocking(move || -> Answer {
        let _mutation = mutation;
        if path == crate::card_art::PATH {
            crate::card_art::validate(&std::fs::read(staged.file.path())?)
                .map_err(|e| fault(422, &format!("{e:#}")))?;
        }
        let dir = p.root().join(&app).join(&build);
        let target = dir.join(&path);
        std::fs::create_dir_all(target.parent().unwrap())?;
        let _guard = p.publishing.lock().unwrap();
        let data = p.db.catalog()?;
        let key = build_key(&app, &build);
        let b = data
            .builds
            .get(&key)
            .ok_or_else(|| fault(404, "create the build first"))?;
        ensure!(!b.ready, fault(409, "published builds are immutable"));
        ensure!(
            data.server_builds
                .get(&key)
                .is_none_or(|j| j.state == crate::server_package::Phase::Uploading),
            fault(
                409,
                "paired build inputs are sealed; upload a new build to change them"
            )
        );

        let prior = b.files.get(&path).copied().unwrap_or(0);
        let size = staged.bytes;
        ensure!(
            b.files.len() < 4096 || b.files.contains_key(&path),
            fault(413, "too many files")
        );
        ensure!(
            b.files.values().sum::<u64>() - prior + size <= 256 * 1024 * 1024,
            fault(413, "build exceeds 256 MiB")
        );
        ensure!(
            server_builds::content_bytes(&data) - prior + size
                <= p.config().controller.content_budget_mb * 1024 * 1024,
            fault(507, "installation content budget is full")
        );
        let disk = rustix::fs::statvfs(&p.config().controller.state_dir)?;
        ensure!(
            disk.f_bavail * disk.f_frsize
                >= p.config().controller.disk_reserve_mb * 1024 * 1024 + size,
            fault(507, "host disk reserve would be exhausted")
        );
        std::fs::rename(staged.file.path(), &target)?;
        std::fs::File::open(target.parent().unwrap())?.sync_all()?;
        p.db.record_build_file(&key, path.clone(), size)?;
        Ok(reply(json!({"path":path,"bytes":size})))
    })
    .await
    .map_err(|e| fault(500, &format!("upload worker failed: {e}")))?
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Promotion {
    #[serde(default = "yes")]
    listed: bool,
    /// First publication only. An update never consults this flag.
    #[serde(default)]
    allow_duplicate_title: bool,
}
pub(super) async fn publish(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    axum::Extension(mutation): axum::Extension<Arc<publication::Mutation>>,
    body: Option<Json<Promotion>>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    tokio::task::spawn_blocking(move || -> Answer {
    let _mutation = mutation;
    p.validate_promotion(&app, &build)?;
    // Files cannot change while this request holds the per-app lock. Compress
    // before the publication lock so player launches are not stalled.
    p.precompress_build(&app, &build)?;
    let _guard = p.publishing.lock().unwrap();
    let allow_duplicate = body.as_ref().is_some_and(|b| b.allow_duplicate_title);
    let listed = body.as_ref().is_none_or(|b| b.listed);
    let duplicates = {
        let data = p.db.catalog()?;
        let manifest_title = data
            .builds
            .get(&build_key(&app, &build))
            .ok_or_else(|| fault(404, "unknown build"))?
            .manifest
            .title
            .clone();
        if games::new_publication(&data, &app) {
            games::duplicate_matches(&data, &app, &manifest_title)
        } else {
            Vec::new()
        }
    };
    if !duplicates.is_empty() && !allow_duplicate {
        return Ok(games::duplicate_conflict(&duplicates));
    }
    let state = p.promote(&app, &build, listed)?;
    let slug = p.db.catalog()?.game_urls[&app].slug.clone();
    let mut published = json!({"app_id":app,"build_id":build,"branch":"default","state":state,"slug":slug,"url":format!("https://{}/games/{slug}",p.domain())});
    if !duplicates.is_empty() {
        published["warnings"] = json!([{"code":"duplicate_title","matches":duplicates}]);
    }
    let mut response = reply(published);
    if state == "queued" {
        *response.status_mut() = StatusCode::ACCEPTED;
    }
    Ok(response)
    }).await.map_err(|e| fault(500, &format!("publication worker failed: {e}")))?
}
impl Platform {
    // The caller holds the per-app mutation lock. Files cannot change while the
    // expensive validation runs, but players can still renew and forward frames.
    fn validate_promotion(&self, app: &str, build: &str) -> Result<()> {
        let key = build_key(app, build);
        let data = self.db.catalog()?;
        let b = data
            .builds
            .get(&key)
            .ok_or_else(|| fault(404, "unknown build"))?;
        anyhow::ensure!(
            b.files.contains_key("index.html"),
            fault(422, "web build needs index.html")
        );
        let index = std::fs::read_to_string(self.root().join(&key).join("index.html"))?;
        if let Some(sdk) = &b.manifest.sdk {
            sdk.admit(&crate::sdk_contract::with_peerjs(
                self.relay.is_some(),
                self.config().controller.peerjs_port.is_some(),
            ))
            .map_err(|e| fault(422, &e.to_string()))?;
        }
        let script = crate::sdk_contract::script(&b.manifest);
        anyhow::ensure!(
            index.contains(script),
            fault(
                422,
                &format!(
                    "include <script type=module src=\"{script}\"></script> so the game maintains its session"
                )
            )
        );
        if let Some(size) = b.files.get(crate::card_art::PATH) {
            anyhow::ensure!(
                *size <= crate::card_art::MAX_BYTES as u64,
                fault(422, "card hero exceeds 250 KiB")
            );
            let bytes = std::fs::read(self.root().join(&key).join(crate::card_art::PATH))?;
            crate::card_art::validate(&bytes).map_err(|e| fault(422, &format!("{e:#}")))?;
        }
        crate::native::validate(&self.root().join(&key), &b.files)
            .map_err(|e| fault(422, &format!("{e:#}")))?;
        Ok(())
    }
    fn precompress_build(&self, app: &str, build: &str) -> Result<()> {
        let key = build_key(app, build);
        let files = self
            .db
            .catalog()?
            .builds
            .get(&key)
            .ok_or_else(|| fault(404, "unknown build"))?
            .files
            .clone();
        let reserve = self
            .config()
            .controller
            .disk_reserve_mb
            .saturating_mul(1024 * 1024);
        crate::precompress::ensure(
            &self.root().join(&key),
            &files,
            reserve,
            &crate::precompress::stamp_path(&self.config().controller.state_dir, &key),
            true,
        )?;
        Ok(())
    }
    /// Fills in siblings for builds that were published before compression
    /// existed. A repeat call keeps the copies it already wrote.
    pub fn precompress_ready(&self) -> Result<()> {
        self.precompress_ready_with(|_| {})
    }
    /// Same pass, with a hook before each file that still needs work.
    pub fn precompress_ready_with(&self, mut before_file: impl FnMut(&str)) -> Result<()> {
        let ready: Vec<_> = self
            .db
            .catalog()?
            .builds
            .iter()
            .filter(|(_, build)| build.ready)
            .map(|(key, build)| (key.clone(), build.files.clone()))
            .collect();
        let reserve = self
            .config()
            .controller
            .disk_reserve_mb
            .saturating_mul(1024 * 1024);
        for (key, files) in ready {
            let dir = self.root().join(&key);
            if !dir.is_dir() {
                continue;
            }
            if let Err(error) = crate::precompress::ensure_during(
                &dir,
                &files,
                reserve,
                &crate::precompress::stamp_path(&self.config().controller.state_dir, &key),
                false,
                &mut before_file,
            ) {
                tracing::warn!(%error, build = %key, "left an existing build uncompressed");
            }
        }
        Ok(())
    }
    /// One file at a time, on a thread the async runtime does not own.
    /// Listening and `/healthz` do not wait for it.
    pub fn spawn_precompress_backfill(&self) {
        let platform = self.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("precompress".into())
            .spawn(move || {
                // Linux applies this nice value to the calling thread only.
                if let Err(error) = rustix::process::nice(19) {
                    tracing::warn!(%error, "precompression kept normal scheduling priority");
                }
                if let Err(error) = platform.precompress_ready() {
                    tracing::warn!(%error, "precompression backfill stopped");
                }
            })
        {
            tracing::warn!(%error, "precompression backfill did not start");
        }
    }
    fn publication_event(&self, app: &str, build: &str, state: &str, reason: &str) {
        let mut previous = self.publication_events.lock().unwrap();
        if state == "published" {
            previous.remove(app);
        } else {
            let value = (build.to_owned(), state.to_owned(), reason.to_owned());
            if previous.get(app) == Some(&value) {
                return;
            }
            previous.insert(app.into(), value);
        }
        let mut data = json!({"state":state});
        if state == "queued" {
            data["queued_reason"] = reason.into();
        } else if state == "failed" {
            data["reason"] = reason.into();
        }
        self.controller
            .timeline
            .record(crate::timeline::Event::server(
                "game.publication",
                crate::timeline::ContextFields {
                    app: app.into(),
                    build: build.into(),
                    ..Default::default()
                },
                data,
                true,
            ));
    }
    // Callers hold the publication lock, also used by player launches. Never switch
    // the client pointer while its shared server is still on the previous build.
    fn promote(&self, app: &str, build: &str, listed: bool) -> Result<&'static str> {
        let key = build_key(app, build);
        let (state, reason) = self.db.edit(|d| {
            let b = d
                .builds
                .get(&key)
                .ok_or_else(|| fault(404, "unknown build"))?
                .clone();
            anyhow::ensure!(
                d.server_builds.get(&key).is_none_or(|j| j.state
                    == crate::server_package::Phase::Ready
                    && b.server.is_some()),
                fault(409, "server packaging has not passed readiness checks")
            );
            crate::game_urls::assign(d, app, &b.manifest.title);
            // Placeholder connection labels follow the Library name, including an admin lock.
            let display = metadata::title(d, app, &b.manifest.title).to_owned();
            for credential in d
                .credentials
                .values_mut()
                .filter(|c| c.app == app && c.role == "publish" && c.label == "Existing game")
            {
                credential.label = display.clone();
            }
            let changes_server = b.server.is_some()
                || d.apps
                    .get(app)
                    .is_some_and(|a| d.builds[&build_key(app, &a.build)].server.is_some());
            let running = self
                .controller
                .snapshot()
                .games
                .get(app)
                .is_some_and(|i| i.reserved());
            if ((changes_server && running) || self.peers.active(app, now()))
                && !d.apps.get(app).is_some_and(|a| a.build == build)
            {
                d.builds.get_mut(&key).unwrap().ready = true;
                d.pending_promotions.insert(
                    app.into(),
                    platform::PendingPromotion {
                        build: build.into(),
                        listed,
                        queued_at: now(),
                    },
                );
                return Ok((
                    "queued",
                    if changes_server && running {
                        "server_running"
                    } else {
                        "peers_active"
                    },
                ));
            }
            if let Some(server) = b.server {
                let keys = self.config().controller.state_dir.join("server-keys");
                std::fs::create_dir_all(&keys)?;
                let path = keys.join(app);
                let token = if path.exists() {
                    std::fs::read_to_string(&path)?
                } else {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let token = secret();
                    let mut f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?;
                    f.write_all(token.as_bytes())?;
                    f.sync_all()?;
                    token
                };
                d.credentials.insert(
                    format!("server-{app}"),
                    platform::Credential {
                        app: app.into(),
                        role: "server".into(),
                        label: "container admission".into(),
                        hash: hash(&token),
                        owner: None,
                    },
                );
                self.controller.register(app, server)?;
            }
            d.builds.get_mut(&key).unwrap().ready = true;
            d.tags.entry(app.into()).or_insert(b.manifest.tags);
            d.apps.insert(
                app.into(),
                App {
                    publication: Some(secret()),
                    build: build.into(),
                    updated: now(),
                    listed: listed && !d.admin_hidden_apps.contains(app),
                },
            );
            d.pending_promotions.remove(app);
            Ok(("published", ""))
        })?;
        self.publication_event(app, build, state, reason);
        Ok(state)
    }
    /// The catalog is publication intent; controller state is recoverable runtime
    /// accounting. Repair either side of an interrupted promotion or retirement.
    pub fn reconcile_publications(&self) -> Result<()> {
        self.expire_visits()?;
        self.sync_deployments()?;
        let _guard = self.publishing.lock().unwrap();
        let data = self.db.read()?;
        let mut desired: BTreeMap<_, _> = data
            .apps
            .iter()
            .filter_map(|(id, app)| {
                data.builds
                    .get(&build_key(id, &app.build))
                    .and_then(|b| b.server.clone())
                    .map(|s| (id.clone(), s))
            })
            .collect();
        for (id, dep) in &data.deployments {
            if let Some(server) = data
                .builds
                .get(&build_key(&dep.app, &dep.active.build))
                .and_then(|b| b.server.clone())
            {
                let directory = self.config().controller.state_dir.join("server-sources");
                std::fs::create_dir_all(&directory)?;
                let path = directory.join(id);
                if std::fs::read_to_string(&path).ok().as_deref() != Some(&dep.app) {
                    crate::private_file::replace(&path, dep.app.as_bytes())?;
                }
                desired.insert(id.clone(), server);
            }
        }
        let state = self.controller.snapshot();
        for id in state.published_games.keys() {
            if !desired.contains_key(id) {
                self.controller.stop(id)?;
                self.controller.unregister(id)?;
            }
        }
        for (id, server) in desired {
            if state
                .games
                .get(&id)
                .is_some_and(|i| i.spec != server && i.reserved())
            {
                self.controller.stop(&id)?;
            } else if state.published_games.get(&id) != Some(&server) {
                self.controller.register(&id, server)?;
            }
        }
        Ok(())
    }
    pub fn activate_pending(&self) -> Result<()> {
        let pending = self.db.read()?.pending_promotions;
        for (app, promotion) in pending {
            if self.db.read()?.deployments.contains_key(&app) {
                continue;
            }
            let Ok(_app) = self.app_lock(&app).try_lock_owned() else {
                continue;
            };
            if self
                .controller
                .snapshot()
                .games
                .get(&app)
                .is_some_and(|i| i.reserved())
            {
                continue;
            }
            // Persisted requests survive restarts; failed activation remains queued.
            if let Err(error) = self.validate_promotion(&app, &promotion.build) {
                self.publication_event(&app, &promotion.build, "failed", "validation");
                tracing::warn!(app, %error, "Queued game update failed validation");
                continue;
            }
            let _guard = self.publishing.lock().unwrap();
            // Unpublish can cancel a pending update between snapshots.
            if self
                .db
                .read()?
                .pending_promotions
                .get(&app)
                .is_none_or(|p| p.build != promotion.build)
            {
                continue;
            }
            match self.promote(&app, &promotion.build, promotion.listed) {
                Ok("published") => {
                    tracing::info!(app, build = promotion.build, "Activated queued game update")
                }
                Ok(_) => {}
                Err(error) => {
                    self.publication_event(&app, &promotion.build, "failed", "activation");
                    tracing::warn!(app,error=%error,"Queued game update could not activate")
                }
            }
        }
        Ok(())
    }
}
pub(super) async fn unpublish(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let _guard = p.publishing.lock().unwrap();
    p.peers.end(&app);
    if p.controller.snapshot().games.contains_key(&app) {
        p.controller.stop(&app)?;
    }
    p.db.edit(|d| {
        d.apps.remove(&app);
        d.pending_promotions.remove(&app);
        for l in d.launches.values_mut().filter(|l| l.app == app) {
            l.end(now(), platform::LaunchEnd::Unpublished);
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
pub(super) async fn remove_build(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    axum::Extension(mutation): axum::Extension<Arc<publication::Mutation>>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(slug(&app) && slug(&build), fault(400, "invalid ID"));
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _mutation = mutation;
        {
            let _guard = p.publishing.lock().unwrap();
            p.db.edit(|d| {
                ensure!(
                    !d.apps.get(&app).is_some_and(|a| a.build == build)
                        && !d
                            .deployments
                            .values()
                            .any(|dep| dep.app == app && dep.active.build == build)
                        && !d
                            .pending_promotions
                            .get(&app)
                            .is_some_and(|p| p.build == build)
                        && !d
                            .launches
                            .values()
                            .any(|l| l.app == app && l.build == build && l.live(now())),
                    fault(409, "build is published, queued or in use")
                );
                ensure!(
                    d.server_builds
                        .get(&build_key(&app, &build))
                        .is_none_or(|j| !j.active()),
                    fault(409, "server packaging is still queued or running")
                );
                for l in d
                    .launches
                    .values_mut()
                    .filter(|l| l.app == app && l.build == build)
                {
                    l.end(now(), platform::LaunchEnd::BuildRemoved);
                }
                d.builds.remove(&build_key(&app, &build));
                Ok(())
            })?;
            let key = build_key(&app, &build);
            p.native_manifests
                .lock()
                .unwrap()
                .retain(|(k, _), _| k != &key);
        }
        let path = p.root().join(&app).join(&build);
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
        crate::precompress::remove_stamp(
            &p.config().controller.state_dir,
            &build_key(&app, &build),
        )?;
        Ok(())
    })
    .await
    .map_err(anyhow::Error::from)??;
    Ok(reply(json!({"ok":true})))
}
