use crate::{
    config::{Config, Game, slug},
    controller::{Controller, Fault, now},
    manifest::{Manifest, Mode, SoloRuntime},
    platform::{self, App, Build, Database, Launch, Reaction, hash},
    store::secret,
};
use anyhow::Result;
macro_rules! ensure {
    ($condition:expr, $error:expr $(,)?) => {
        if !$condition {
            return Err($error.into());
        }
    };
}
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;
mod authors;
mod avatars;
mod capture;
mod card_art;
mod chat;
mod client;
mod conditional;
mod connections;
mod dedicated;
mod feedback;
mod games;
mod groups;
mod library;
mod member_link;
mod metadata;
mod native;
mod peerjs;
mod platform_updates;
mod presence;
mod profiles;
mod publication;
mod pwa;
mod reaction_art;
mod runtime;
mod screenshots;
mod sdk_v1;
mod server_builds;
mod server_diagnostics;
mod themes;
mod update_reactions;
mod urls;
mod visits;
mod voice;

pub fn fault(status: u16, message: &str) -> anyhow::Error {
    Fault {
        status,
        message: message.into(),
    }
    .into()
}
pub struct Error(anyhow::Error);
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.into())
    }
}
impl From<rustix::io::Errno> for Error {
    fn from(e: rustix::io::Errno) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if let Some(f) = self.0.downcast_ref::<crate::cloud::Fault>() {
            return (
                StatusCode::from_u16(f.status).unwrap(),
                [("Cache-Control", "no-store")],
                Json(json!({"error":{"code":f.code,"message":f.message}})),
            )
                .into_response();
        }
        if let Some(f) = self.0.downcast_ref::<crate::dedicated::Fault>() {
            return (
                StatusCode::from_u16(f.status).unwrap(),
                [("Cache-Control", "no-store")],
                Json(json!({"error":{"code":f.code,"message":f.message}})),
            )
                .into_response();
        }
        let (status, message) = match self.0.downcast_ref::<Fault>() {
            Some(f) => (f.status, f.message.clone()),
            None => {
                tracing::error!(error=%self.0,"platform request failed");
                (500, "Something went wrong. Please try again.".into())
            }
        };
        (
            StatusCode::from_u16(status).unwrap(),
            [("Cache-Control", "no-store")],
            Json(json!({"error":message})),
        )
            .into_response()
    }
}
type Answer = std::result::Result<Response, Error>;
fn reply(v: Value) -> Response {
    ([("Cache-Control", "no-store")], Json(v)).into_response()
}
#[derive(Clone)]
pub struct Platform {
    pub controller: Arc<Controller>,
    pub db: Arc<Database>,
    http: reqwest::Client,
    grants: Arc<Mutex<BTreeMap<IpAddr, Arc<tokio::sync::Mutex<u64>>>>>,
    publishing: Arc<Mutex<()>>,
    publication_slots: Arc<tokio::sync::Semaphore>,
    app_mutations: Arc<Mutex<BTreeMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>>,
    peers: Arc<crate::peers::Rooms>,
    relay: Option<Arc<crate::peers::Relay>>,
    feedback_lock: Arc<Mutex<()>>,
    peerjs_sockets: Arc<Mutex<std::collections::BTreeSet<String>>>,
    push: Arc<crate::notifications::Push>,
    push_dispatch: Arc<tokio::sync::Mutex<()>>,
    packager: Arc<dyn crate::server_package::worker::Engine>,
    connection_signer: Arc<crate::connections::Signer>,
    connections: Arc<Mutex<crate::connections::Attempts>>,
    connection_epoch: String,
    dedicated: Arc<Mutex<crate::dedicated::Directory>>,
    voice: Option<Arc<crate::voice::Service>>,
    native_manifests: Arc<Mutex<BTreeMap<(String, u64), infuser_native_protocol::Manifest>>>,
}
impl Platform {
    pub fn peer_rooms(&self) -> Arc<crate::peers::Rooms> {
        self.peers.clone()
    }
    pub fn open(controller: Arc<Controller>) -> Result<Self> {
        let staging = controller
            .config
            .controller
            .state_dir
            .join("upload-staging");
        // Controller ownership is exclusive. No previous uploader can still own
        // these disposable files after process recovery.
        if staging.exists() {
            std::fs::remove_dir_all(staging)?;
        }
        let db = Arc::new(Database::open(&controller.config.controller.state_dir)?);
        let initial = db.catalog()?;
        anyhow::ensure!(
            initial.visits.is_empty() || controller.config.controller.visits,
            "keep controller.visits enabled after creating visits"
        );
        anyhow::ensure!(
            initial.groups.names.len() <= 1 || controller.config.controller.private_groups,
            "enable controller.private_groups before serving multiple groups"
        );
        let relay = controller
            .config
            .controller
            .turn
            .as_ref()
            .map(crate::peers::Relay::open)
            .transpose()?
            .map(Arc::new);
        // After a restart, old browser hosts must lose their short lease before
        // this coordinator can create another session. The marker contains no secrets.
        let marker = controller
            .config
            .controller
            .state_dir
            .join("peer-coordinator.started");
        let resume_after = if relay.is_some() {
            let recovering = marker.exists();
            std::fs::File::create(&marker)?.sync_all()?;
            if recovering { now() + 30 } else { 0 }
        } else {
            0
        };
        let platform = Self {
            voice: controller
                .config
                .controller
                .voice
                .as_ref()
                .map(crate::voice::Service::open)
                .transpose()?
                .map(Arc::new),
            connection_signer: Arc::new(crate::connections::Signer::open(
                &controller.config.controller.state_dir,
            )?),
            connections: Default::default(),
            connection_epoch: secret()[..16].into(),
            dedicated: Default::default(),
            packager: Arc::new(crate::server_package::worker::DockerWorker {
                settings: controller.config.controller.clone(),
            }),
            feedback_lock: Default::default(),
            push: Arc::new(crate::notifications::Push::open(
                &controller.config.controller.state_dir,
            )?),
            push_dispatch: Default::default(),
            peerjs_sockets: Default::default(),
            controller,
            db,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(4))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            grants: Arc::new(Mutex::new(BTreeMap::new())),
            publishing: Arc::new(Mutex::new(())),
            publication_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            app_mutations: Default::default(),
            peers: Arc::new(crate::peers::Rooms::recovering(resume_after)),
            relay,
            native_manifests: Default::default(),
        };
        platform.reconcile_publications()?;
        platform.controller.deployment.observe_boot()?;
        Ok(platform)
    }
    fn config(&self) -> &Config {
        &self.controller.config
    }
    fn root(&self) -> PathBuf {
        self.config().controller.state_dir.join("content")
    }
    fn domain(&self) -> &str {
        &self.config().controller.domain
    }
    fn main_host(&self, h: &HeaderMap) -> Result<()> {
        ensure!(host(h) == self.domain(), fault(404, "unknown host"));
        Ok(())
    }
    fn origin(&self, h: &HeaderMap, expected: &str) -> Result<()> {
        ensure!(
            h.get_all("origin").iter().count() == 1,
            fault(403, "one request origin is required")
        );
        ensure!(
            h.get("origin").and_then(|v| v.to_str().ok()) == Some(&format!("https://{expected}")),
            fault(403, "request origin does not match")
        );
        Ok(())
    }
    async fn member(&self, h: &HeaderMap, write: bool) -> Result<String> {
        self.main_host(h)?;
        if write {
            ensure!(
                !h.contains_key(groups::INSPECT),
                fault(403, "Inspection is read-only. Return home first.")
            );
            self.origin(h, self.domain())?;
        }
        // This header is always removed and re-created by session_auth below.
        let email = h
            .get("x-infuser-verified-profile")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| fault(401, "Sign in to choose your Infuser profile."))?
            .to_owned();
        ensure!(
            self.db.catalog()?.profile_enabled(&email),
            fault(403, "Ask your admin for an Infuser invitation.")
        );
        self.group_view(h, &email)?;
        Ok(email)
    }
    async fn admin(&self, h: &HeaderMap, write: bool) -> Result<String> {
        let email = self.member(h, write).await?;
        ensure!(
            !h.contains_key(groups::VISIT),
            fault(403, "Return home for administration.")
        );
        let data = self.db.catalog()?;
        ensure!(
            data.members[&email].admin
                || (!write && h.contains_key(groups::INSPECT) && data.operator(&email)),
            fault(403, "An admin account is required.")
        );
        Ok(email)
    }
    fn publisher(&self, h: &HeaderMap, app: Option<&str>, role: &str) -> Result<String> {
        self.main_host(h)?;
        let token = bearer(h).ok_or_else(|| fault(401, "publisher bearer token required"))?;
        let digest = hash(token);
        let data = self.db.catalog()?;
        let credential = data
            .credentials
            .values()
            .find(|c| c.hash == digest && c.role == role && app.is_none_or(|id| id == c.app))
            .ok_or_else(|| fault(401, "invalid or revoked app credential"))?;
        ensure!(
            credential
                .owner
                .as_ref()
                .is_none_or(|email| data.profile_enabled(email)),
            fault(403, "publisher membership disabled")
        );
        if let Some(owner) = &credential.owner {
            ensure!(
                data.groups.allows(owner, &credential.app),
                fault(403, "publisher group no longer matches")
            );
        }
        Ok(credential.app.clone())
    }
    async fn admit(&self, h: &HeaderMap) -> bool {
        let Some(socket) = &self.config().controller.firewall_socket else {
            return false;
        };
        let Some(ip) = h
            .get("x-infuser-client-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<IpAddr>().ok())
        else {
            return false;
        };
        let grant = {
            let mut grants = self.grants.lock().unwrap();
            grants.retain(|_, g| {
                Arc::strong_count(g) > 1 || g.try_lock().map(|t| *t + 1800 > now()).unwrap_or(true)
            });
            grants.entry(ip).or_default().clone()
        };
        let mut refreshed = grant.lock().await;
        if *refreshed + 60 > now() {
            return true;
        }
        match crate::firewall::grant(socket, ip).await {
            Ok(()) => {
                *refreshed = now();
                true
            }
            Err(e) => {
                tracing::warn!(error=%e,"game port admission unavailable");
                false
            }
        }
    }
    fn game(&self, h: &HeaderMap) -> Result<(String, Launch)> {
        let token =
            game_token(h).ok_or_else(|| fault(401, "Launch this game from the clubhouse."))?;
        let digest = hash(token);
        self.db.find_launch(&digest, host(h), now())
    }
    fn refresh(&self, id: &str) -> Result<Value> {
        let _guard = self.publishing.lock().unwrap();
        self.refresh_locked(id)
    }
    fn refresh_locked(&self, id: &str) -> Result<Value> {
        let data = self.db.read()?;
        let launch = data
            .launches
            .get(id)
            .filter(|l| l.expires > now())
            .ok_or_else(|| fault(410, "game session expired"))?;
        ensure!(
            data.profile_enabled(&launch.email) && data.launch_allowed(launch, now()),
            fault(401, "Game authorization ended.")
        );
        let mut status = json!({"state":"ready","endpoint":null});
        let on_demand = data.builds[&build_key(&launch.app, &launch.build)]
            .manifest
            .server_start
            .is_some();
        if on_demand {
            status["server"] = self.runtime_view(launch, true)?;
        } else if let (Some(lease), Some(token)) = (&launch.lease_id, &launch.lease_token) {
            status = self
                .controller
                .lease(launch.runtime_id(), lease, token, "renew", now())?;
        }
        if !on_demand
            && launch.lease_id.is_some()
            && status["state"] == "ready"
            && self
                .controller
                .snapshot()
                .games
                .get(launch.runtime_id())
                .is_some_and(|i| i.spec.web)
        {
            status["endpoint"] = json!({"url":format!("https://{}/_infuser/server",launch.host)});
        }
        self.db.renew_launch(id, now())?;
        if let Some(visit) = launch.visit.as_ref().and_then(|id| data.visits.get(id)) {
            status["visit"] = json!({"expires_at":visit.expires,"authorization_seconds":30});
        }
        Ok(status)
    }
}
fn host(h: &HeaderMap) -> &str {
    h.get("host").and_then(|v| v.to_str().ok()).unwrap_or("")
}
fn bearer(h: &HeaderMap) -> Option<&str> {
    h.get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
fn game_token(h: &HeaderMap) -> Option<&str> {
    bearer(h).or_else(|| {
        h.get("cookie")?
            .to_str()
            .ok()?
            .split(';')
            .find_map(|c| c.trim().strip_prefix("__Host-infuser_game="))
    })
}
fn build_key(app: &str, build: &str) -> String {
    format!("{app}/{build}")
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-. @()+".contains(&b))
        && !path.starts_with("_infuser/")
}

pub fn router(platform: Platform) -> Router {
    let publishing = Router::new()
        .route("/api/metadata/apps", get(metadata::list_metadata))
        .route(
            "/api/metadata/apps/{app}",
            get(metadata::get_metadata)
                .patch(metadata::patch_metadata)
                .layer(DefaultBodyLimit::max(96 * 1024)),
        )
        .route(
            "/api/metadata/apps/{app}/card-art",
            put(metadata::put_art)
                .delete(metadata::delete_art)
                .layer(DefaultBodyLimit::max(crate::card_art::MAX_BYTES)),
        )
        .route(
            "/api/publisher/claim",
            post(claim_setup).layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/api/publisher/capabilities",
            get(publication::capabilities),
        )
        .route("/api/publisher/tags", get(publisher_tags))
        .route("/api/publisher/apps/{app}/tags", put(publisher_set_tags))
        .route(
            "/api/publisher/apps/{app}/metadata",
            get(metadata::get_metadata)
                .patch(metadata::patch_metadata)
                .layer(DefaultBodyLimit::max(96 * 1024)),
        )
        .route(
            "/api/publisher/apps/{app}/card-art",
            put(metadata::put_art)
                .delete(metadata::delete_art)
                .layer(DefaultBodyLimit::max(crate::card_art::MAX_BYTES)),
        )
        .route(
            "/api/publisher/apps/{app}/slug",
            put(urls::publisher_rename),
        )
        .route("/api/publisher/apps/{app}", delete(publication::unpublish))
        .route(
            "/api/publisher/apps/{app}/server-diagnostics",
            get(server_diagnostics::get),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}",
            put(publication::create_build)
                .get(publication::build_status)
                .delete(publication::remove_build),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/files/{*path}",
            put(publication::upload),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/publish",
            post(publication::publish),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/server/files/{*path}",
            put(server_builds::upload).layer(DefaultBodyLimit::max(
                crate::server_package::SOURCE_FILE_BYTES,
            )),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/server-build",
            get(server_builds::status).post(server_builds::start),
        )
        .layer(axum::middleware::from_fn_with_state(
            platform.clone(),
            capture::record,
        ))
        .layer(DefaultBodyLimit::max(96 * 1024))
        .layer(axum::middleware::from_fn_with_state(
            platform.clone(),
            publication::admit,
        ));
    Router::new()
        .merge(publishing)
        .merge(native::routes())
        .merge(groups::routes())
        .merge(profiles::routes())
        .merge(voice::routes())
        .merge(screenshots::routes())
        .merge(visits::routes())
        .route("/assets/visits.js",get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/visits.js")) }))
        .merge(feedback::routes())
        .merge(pwa::routes())
        .route("/assets/groups.js",get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/groups.js")) }))
        .nest("/_infuser/api/v1", sdk_v1::routes())
        .route("/_infuser/sdk/v1.js", get(sdk_v1::core))
        .route("/_infuser/sdk/v1/peers.js", get(sdk_v1::peers))
        .route("/_infuser/sdk/v1/peerjs.js", get(sdk_v1::peerjs))
        .route("/_infuser/peerjs/peerjs", get(peerjs::socket))
        .route("/_infuser/sdk/v1/cloud.js", get(sdk_v1::cloud))
        .route("/_infuser/sdk/v1/sessions.js", get(sdk_v1::sessions))
        .route("/", get(page))
        .route("/games/{slug}", get(page))
        .route("/clubhouse", get(page))
        .route("/library", get(page))
        .route("/member-link/{token}", get(member_link::preview).post(member_link::redeem))
        .route("/sign-out", get(member_link::sign_out))
        .route("/api/operator/members/{member}/sign-in-link", post(member_link::mint))
        .route("/api/operator/members/{member}/sign-in", delete(member_link::revoke))
        .route(
            "/assets/clubhouse.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(&h, "text/javascript", include_bytes!("../web/clubhouse.js"))
            }),
        )
        .route(
            "/assets/clubhouse.css",
            get(|h: HeaderMap| async move {
                conditional::static_asset(&h, "text/css", include_bytes!("../web/clubhouse.css"))
            }),
        )
        .route("/assets/themes.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/themes.js")) }))
        .route("/assets/themes/{*path}", get(themes::asset))
        .route("/assets/card-generating.webp", get(|| async {
            ([(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, "public, max-age=3600")],
             include_bytes!("../assets/card-fallback/generating.webp").as_slice())
        }))
        .route(
            "/assets/google-signin.png",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/png")],
                    include_bytes!("../assets/google/sign-in-with-google-pill.png").as_slice(),
                )
            }),
        )
        .route("/api/me", get(me))
        .route("/assets/avatars/{id}/{size}", get(avatars::image))
        .route("/assets/reactions/{file}", get(reaction_art::image))
        .route("/api/profile/picture-prompt", post(avatars::picture_prompt))
        .route("/api/profile", get(avatars::usage).put(avatars::update).layer(DefaultBodyLimit::max(1024)))
        .route("/api/presence/input", post(presence::library_input))
        .route("/_infuser/input", post(presence::game_input))
        .route("/_infuser/player-pages/{page}/close", post(presence::close_page))
        .route("/assets/activity-input.js",get(sdk_v1::activity_input))
        .route("/_infuser/activity-input.js",get(sdk_v1::activity_input))
        .route("/api/library", get(library::list))
        .route("/api/client-state", get(client::state))
        .route("/assets/client-updates.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/client-updates.js")) }))
        .route("/assets/server-updates.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/server-updates.js")) }))
        .route("/api/chat", get(chat::list).post(chat::send).layer(DefaultBodyLimit::max(4096)))
        .route("/api/chat/read", post(chat::advance).layer(DefaultBodyLimit::max(256)))
        .route("/api/chat/{id}", delete(chat::remove))
        .route("/assets/chat-emoji.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/chat-emoji.js")) }))
        .route("/assets/vendor/emoji-picker/picker.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/vendor/emoji-picker/picker.js")) }))
        .route("/assets/vendor/emoji-picker/database.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/vendor/emoji-picker/database.js")) }))
        .route("/assets/vendor/emoji-picker/picker.css", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/css", include_bytes!("../web/vendor/emoji-picker/picker.css")) }))
        .route("/assets/vendor/emoji-picker/data.json", get(|h: HeaderMap| async move { conditional::static_asset(&h, "application/json", include_bytes!("../web/vendor/emoji-picker/data.json")) }))
        .route("/assets/chat-reads.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/chat-reads.js")) }))
        .route("/assets/chat.js", get(|h: HeaderMap| async move { conditional::static_asset(&h, "text/javascript", include_bytes!("../web/chat.js")) }))
        .route("/api/games/{slug}", get(urls::resolve))
        .route("/api/apps/{app}/slug", put(urls::owner_rename))
        .route("/api/apps/{app}/tags",put(set_tags))
        .route("/api/my-games", post(new_game))
        .route("/api/my-games/{app}/setup",post(renew_setup))
        .route("/api/apps/{app}/launch", post(launch))
        .route("/api/apps/{app}/card-art/{build}", get(card_art::image))
        .route(
            "/api/apps/{app}/card-art/{build}/{digest}",
            get(card_art::addressed),
        )
        .route("/api/launches/{id}", get(launch_status))
        .route("/api/apps/{app}/updates/{publication}/reaction", put(update_reactions::set))
        .route("/api/platform-updates/{batch}/reaction", put(platform_updates::set))
        .route("/api/apps/{app}/reaction", put(react).delete(unreact))
        .route("/api/admin/apps/{app}/reactions/{player}", delete(remove_game_reaction))
        .route("/api/admin/game-authors", get(authors::inventory))
        .route("/api/admin/apps/{app}/authors", put(authors::set).layer(DefaultBodyLimit::max(16384)))
        .route("/api/admin/members", get(members).post(invite))
        .route("/api/admin/members/handle", put(rename_member).layer(DefaultBodyLimit::max(1024)))
        .route("/api/admin/apps/{app}/visibility", put(set_visibility).layer(DefaultBodyLimit::max(1024)))
        .route("/api/admin/games", get(games::list))
        .route(
            "/api/admin/apps/{app}/deletion",
            get(games::preview).post(games::delete_game).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/admin/apps/{app}/title",
            put(games::rename).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/api/admin/peer-health", get(peer_health_admin))
        .route(
            "/api/admin/credentials",
            get(credentials).post(create_credential),
        )
        .route("/api/admin/credentials/{id}", delete(revoke))
        .route("/api/server/verify", post(verify_ticket))
        .route("/api/server/v1/register", post(dedicated::register).layer(DefaultBodyLimit::max(1024)))
        .route("/api/server/v1/sessions", put(dedicated::report).layer(DefaultBodyLimit::max(131072)))
        .route("/api/server/v1/connections/{connection}/admission", post(dedicated::admission).layer(DefaultBodyLimit::max(8192)))
        .route("/api/server/v1/control", get(dedicated::control))
        .route("/_infuser/session", post(exchange))
        .route("/_infuser/launch", get(|| async { ([("Cache-Control","no-store"),("Referrer-Policy","no-referrer"),("Content-Security-Policy","default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'")],Html(include_str!("../web/game-launch.html"))) }))
        .route("/_infuser/context", get(context))
        .route("/_infuser/presence/v1", get(presence::snapshot))
        .route("/_infuser/launch-intent/v1/consume", post(consume_intent))
        .route("/_infuser/runtime/v1", get(runtime::status).post(runtime::acquire))
        .route("/_infuser/runtime/v1/{session}", delete(runtime::release))
        .route("/_infuser/peers", post(peer_join))
        .route("/_infuser/peers/{peer}", get(peer_poll).delete(peer_leave))
        .route("/_infuser/peers/{peer}/signals", post(peer_signal))
        .route("/_infuser/peers/{peer}/joinability", put(presence::set_joinable).layer(DefaultBodyLimit::max(1024)))
        .route("/_infuser/peers/{peer}/ice", post(peer_ice))
        .route("/_infuser/peers/{peer}/health", post(peer_health).layer(DefaultBodyLimit::max(4096)))
        .route("/_infuser/peers.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript")],include_str!("../sdk/peers.js")) }))
        .route("/_infuser/play", get(player))
        .route("/_infuser/player.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript")],include_str!("../web/game-player.js")) }))
        .route("/_infuser/player.css", get(|| async { ([(header::CONTENT_TYPE,"text/css")],include_str!("../web/game-player.css")) }))
        .route("/_infuser/proxy-auth", get(proxy_auth))
        .route("/_infuser/heartbeat", post(heartbeat).delete(leave))
        .route("/_infuser/saves/{slot}", get(save_get).put(save_put))
        .route("/_infuser/cloud/v1/groups/{group}", get(cloud_load).post(cloud_commit))
        .route("/_infuser/cloud/v1/groups/{group}/revisions/{revision}", get(cloud_revision))
        .route("/_infuser/cloud/v1/groups/{group}/history", get(cloud_history))
        .route("/_infuser/cloud-v1.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript")],include_str!("../sdk/cloud-v1.js")) }))
        .route(
            "/_infuser/sdk.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../sdk/infuser.js"),
                )
            }),
        )
        .fallback(game_file)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(axum::middleware::from_fn_with_state(platform.clone(),session_auth))
        .layer(axum::middleware::from_fn_with_state(
            platform.controller.maintenance.clone(),
            crate::maintenance::admission,
        ))
        .with_state(platform)
}
async fn session_auth(
    State(p): State<Platform>,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    request.headers_mut().remove("x-infuser-verified-email");
    request.headers_mut().remove("x-infuser-verified-profile");
    request.headers_mut().remove(profiles::ACCOUNT);
    let path = request.uri().path();
    let player_api = path.starts_with("/api/")
        && !path.starts_with("/api/publisher/")
        && !path.starts_with("/api/metadata/")
        && !path.starts_with("/api/server/");
    if !player_api || native::public(path) || host(request.headers()) != p.domain() {
        return next.run(request).await;
    }
    let cookie = request.headers().get("cookie").cloned();
    let selected_profile = profiles::requested(
        request.headers(),
        request.uri(),
        request.method() == axum::http::Method::GET,
    );
    let choosing = path == "/api/profiles";
    // A member-link cookie is its own sign-in. An invalid one must not fall
    // through to Google, or a shared browser could become someone else.
    let member_token = cookie
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .and_then(platform::member_link::cookie_value)
        .map(str::to_owned);
    let verified: Result<_> = async {
        if let Some(token) = member_token {
            let account = p.db.member_session(&token)?;
            let profile = if choosing {
                None
            } else {
                let key =
                    p.db.catalog()?
                        .select_profile(&account, selected_profile?.as_deref())?;
                Some(
                    key.parse::<axum::http::HeaderValue>()
                        .map_err(|_| fault(500, "Invalid stored profile handle."))?,
                )
            };
            return Ok((account, profile, Vec::<axum::http::HeaderValue>::new()));
        }
        let cookie = cookie.ok_or_else(|| fault(401, "Please sign in with Google."))?;
        let response = p
            .http
            .get(&p.config().controller.auth_url)
            .header("cookie", cookie)
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            fault(401, "Please sign in with Google.")
        );
        let email = response
            .headers()
            .get("x-auth-request-email")
            .cloned()
            .ok_or_else(|| fault(401, "Google session did not supply an email."))?;
        let subject = response
            .headers()
            .get("x-auth-request-user")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                fault(
                    401,
                    "Sign in again: Google session did not supply a stable identity.",
                )
            })?;
        let account = p.db.google_account(
            subject,
            email
                .to_str()
                .map_err(|_| fault(401, "Invalid provider email."))?,
        )?;
        let profile = if choosing {
            None
        } else {
            let key =
                p.db.catalog()?
                    .select_profile(&account, selected_profile?.as_deref())?;
            Some(
                key.parse::<axum::http::HeaderValue>()
                    .map_err(|_| fault(500, "Invalid stored profile handle."))?,
            )
        };
        let cookies: Vec<_> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .cloned()
            .collect();
        Ok((account, profile, cookies))
    }
    .await;
    match verified {
        Err(error) => {
            let selection = error
                .downcast_ref::<Fault>()
                .is_some_and(|f| f.status == 409);
            let mut response = Error(error).into_response();
            if selection {
                response
                    .headers_mut()
                    .insert("x-infuser-profile-required", "1".parse().unwrap());
            }
            response
        }
        Ok((account, profile, cookies)) => {
            request.headers_mut().insert(
                profiles::ACCOUNT,
                account.parse().expect("generated account ID"),
            );
            if let Some(profile) = profile {
                request
                    .headers_mut()
                    .insert("x-infuser-verified-profile", profile);
            }
            let mut response = next.run(request).await;
            for cookie in cookies {
                response.headers_mut().append("set-cookie", cookie);
            }
            response
        }
    }
}
async fn page(State(p): State<Platform>, request: Request) -> Answer {
    if host(request.headers()) != p.domain() {
        return game_file(State(p), request).await;
    }
    let html = include_str!("../web/index.html").replace(
        "</head>",
        &format!(
            "<meta name=\"infuser-client-version\" content=\"{}\"></head>",
            client::version()
        ),
    );
    Ok(([("Cache-Control","no-store"),("Content-Security-Policy","default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")],Html(html)).into_response())
}
async fn me(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    let admitted = if inspecting { false } else { p.admit(&h).await };
    let data = p.db.catalog()?;
    let member = &data.members[&email];
    let profile_count = data
        .account_profiles(&data.identities.profiles[&email].account_id)?
        .len();
    Ok(reply(
        json!({"name":member.name,"admin":member.admin&&!inspecting&&!h.contains_key(groups::VISIT),"operator":data.operator(&email),"inspecting":inspecting,"group":{"id":group,"name":data.groups.names.get(&group)},"home_group":data.groups.member(&email),"player_id":data.player_id(&email),"profile_count":profile_count,"avatar":member.avatar,"avatar_prompt_seen":member.avatar_prompt_seen,"avatars":avatars::catalog(),"game_ports_admitted":admitted}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchRequest {
    #[serde(default)]
    guest: bool,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    intent: Option<crate::launch_intent::Intent>,
}
async fn launch(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<LaunchRequest>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let (group, _) = p.group_view(&h, &email)?;
    p.admit(&h).await;
    if !p.db.catalog()?.visits.is_empty() {
        p.reconcile_publications()?;
    }
    let _publishing = p.publishing.lock().unwrap();
    let data = p.db.read()?;
    p.visible_app(&h, &email, &app)?;
    let borrowed = data.groups.app(&app) != group;
    let visit = if borrowed || data.groups.member(&email) != group {
        Some(
            data.active_visit(&group, now())
                .filter(|(id, v)| {
                    data.visit_access(id, &email, now()).is_some()
                        && (!borrowed || v.games.contains(&app))
                })
                .ok_or_else(|| fault(403, "Visit access ended."))?
                .0
                .clone(),
        )
    } else {
        None
    };
    let deployment = if borrowed {
        Some(
            data.deployments
                .iter()
                .find(|(_, d)| d.app == app && d.group == group)
                .ok_or_else(|| fault(503, "Visit game is preparing. Try again."))?
                .0
                .clone(),
        )
    } else {
        None
    };
    let runtime_id = deployment.as_deref().unwrap_or(&app);
    let selected = data
        .deployed(runtime_id)
        .ok_or_else(|| fault(404, "unknown game"))?;
    ensure!(
        data.apps.get(&app).is_some_and(|a| a.listed)
            || (!borrowed && data.groups.member(&email) == group && data.members[&email].admin),
        fault(404, "unknown game")
    );
    let b = &data.builds[&build_key(&app, &selected.build)];
    if visit.is_some()
        && let Some(reason) = b.visit_unavailable()
    {
        return Err(fault(422, reason).into());
    }
    if let Some(sdk) = &b.manifest.sdk {
        sdk.admit(&crate::sdk_contract::with_peerjs(
            p.relay.is_some(),
            p.config().controller.peerjs_port.is_some(),
        ))
        .map_err(|e| fault(422, &e.to_string()))?;
    }
    if let Some(intent) = &body.intent {
        intent.validate(&b.manifest)?;
        ensure!(
            body.mode.is_none(),
            fault(400, "use a launch intent or a legacy mode, not both")
        );
    }
    let mode = serde_json::to_value(&b.manifest.play_modes).unwrap();
    if let Some(requested) = &body.mode {
        ensure!(
            mode.as_array().unwrap().contains(&json!(requested)),
            fault(400, "choose a supported play mode")
        );
    }
    ensure!(
        data.launches
            .values()
            .filter(|l| l.email == email && l.expires > now())
            .count()
            < 32,
        fault(429, "Close an old game before opening more.")
    );
    // New builds opt into title-screen startup. Preserve launch-time backend
    // acquisition for existing uploads, including their explicit mode requests.
    let managed = b
        .manifest
        .multiplayer
        .as_ref()
        .is_some_and(|m| matches!(m.mode, Mode::Dedicated | Mode::PersistentWorld));
    let needs_server = b.manifest.server_start.is_none()
        && ((body.mode.as_deref() == Some("multiplayer") && managed)
            || (body.mode.is_none() && managed)
            || matches!(b.manifest.solo_runtime, Some(SoloRuntime::SharedServer)));
    let id = secret()[..32].to_string();
    let token = secret();
    let lease = if needs_server {
        Some(p.controller.acquire(runtime_id, &id, now())?)
    } else {
        None
    };
    let host_id = if body.guest || body.intent.is_some() || visit.is_some() {
        id.clone()
    } else {
        // A new build must not replace cookies/assets in an already-open old tab.
        // Personal cloud saves keep the same player/app key across these origins.
        hash(&format!("{email}/{app}/{}", selected.build))[..32].into()
    };
    let gamehost = crate::game_urls::host(&data, &app, &host_id, p.domain());
    let l = Launch {
        deployment,
        visit,
        intent: body.intent,
        app,
        build: selected.build.clone(),
        email,
        guest: body.guest,
        host: gamehost.clone(),
        token_hash: hash(&token),
        exchanged: false,
        expires: now() + 300,
        grant_expires: now() + 120,
        seen: now(),
        lease_id: lease
            .as_ref()
            .and_then(|l| l["lease_id"].as_str())
            .map(str::to_owned),
        lease_token: lease
            .as_ref()
            .and_then(|l| l["lease_token"].as_str())
            .map(str::to_owned),
    };
    p.db.insert_launch(&id, &l)?;
    Ok(reply(
        json!({"launch_id":id,"url":format!("https://{gamehost}/_infuser/launch#infuser={token}"),"state":lease.as_ref().map(|l|&l["state"]).unwrap_or(&json!("ready"))}),
    ))
}
async fn launch_status(State(p): State<Platform>, Path(id): Path<String>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    ensure!(
        !h.contains_key(groups::INSPECT),
        fault(403, "Inspection cannot renew launches.")
    );
    let data = p.db.read()?;
    let l = data
        .launches
        .get(&id)
        .ok_or_else(|| fault(404, "unknown launch"))?;
    ensure!(
        l.email == email && l.expires > now(),
        fault(403, "not your active launch")
    );
    ensure!(
        data.launch_allowed(l, now()),
        fault(403, "Game authorization ended.")
    );
    // Startup may take longer than the initial grant window. Only the authenticated
    // launcher can extend the unused, single-use exchange token.
    p.db.extend_launch_grant(&id, now() + 120)?;
    Ok(reply(p.refresh(&id)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct React {
    emoji: String,
    text: String,
    #[serde(default)]
    screenshot: Option<String>,
}
fn valid_emoji(emoji: &str) -> bool {
    [
        "⭐",
        "👍",
        "🚀",
        "❤️",
        "👏",
        "💡",
        "😅",
        "⚡",
        "🧠",
        "🎯",
        "🧭",
        "🧲",
        "🐦‍⬛",
        "🍵",
        "💎",
        "🧩",
        "🎢",
        "🪄",
        "🎨",
        "🎵",
        "😄",
        "🤝",
    ]
    .contains(&emoji)
}
async fn react(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<React>,
) -> Answer {
    let email = p.member(&h, true).await?;
    p.visible_app(&h, &email, &app)?;
    let (group, _) = p.group_view(&h, &email)?;
    let app =
        p.db.catalog()?
            .deployment_for(&app, &group)
            .ok_or_else(|| fault(404, "Game not found."))?
            .to_owned();
    ensure!(
        valid_emoji(&body.emoji) && body.text.chars().count() <= 140,
        fault(
            400,
            "Choose a reaction and keep your note to 140 characters."
        )
    );
    p.db.edit_transaction(|d, tx| {
        ensure!(d.deployed(&app).is_some(), fault(404, "unknown game"));
        let previous = d
            .reactions
            .get(&app)
            .and_then(|game| game.get(&email))
            .and_then(|r| r.screenshot.as_ref());
        let changed = previous != body.screenshot.as_ref();
        if changed && let Some(id) = &body.screenshot {
            platform::screenshots::owned(tx, &email, id)?;
        }
        d.reactions.entry(app).or_default().insert(
            email.clone(),
            Reaction {
                emoji: body.emoji,
                text: body.text.trim().into(),
                updated: now(),
                screenshot: body.screenshot,
            },
        );
        if changed {
            platform::screenshots::prune(tx, d, &email)?;
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn unreact(State(p): State<Platform>, Path(app): Path<String>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    p.visible_app(&h, &email, &app)?;
    let (group, _) = p.group_view(&h, &email)?;
    p.db.edit_transaction(|d, tx| {
        let runtime = d
            .deployment_for(&app, &group)
            .ok_or_else(|| fault(404, "Game not found."))?
            .to_owned();
        if let Some(game) = d.reactions.get_mut(&runtime) {
            game.remove(&email);
        }
        platform::screenshots::prune(tx, d, &email)
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn remove_game_reaction(
    State(p): State<Platform>,
    Path((app, player)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    let email = p.admin(&h, true).await?;
    p.visible_app(&h, &email, &app)?;
    p.db.edit_transaction(|d, tx| {
        // Moderate the home group's deployment, including games brought by visitors.
        // Never touch the source group's reactions for a borrowed game.
        let runtime = d
            .deployment_for(&app, d.groups.member(&email))
            .filter(|id| d.deployed(id).is_some())
            .ok_or_else(|| fault(404, "Game not found."))?
            .to_owned();
        if let Some(reactions) = d.reactions.get_mut(&runtime)
            && let Some(author) = reactions
                .keys()
                .find(|author| {
                    d.identities
                        .profiles
                        .get(*author)
                        .is_some_and(|p| p.id == player)
                })
                .cloned()
        {
            reactions.remove(&author);
            platform::screenshots::prune(tx, d, &author)?;
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn members(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let data = p.db.catalog()?;
    let operator = data.operator(&email);
    let mut members = BTreeMap::new();
    for (key, member) in data
        .members
        .iter()
        .filter(|(key, _)| data.groups.member(key) == group)
    {
        let mut value = serde_json::to_value(member).expect("member serialization");
        value["contact_email"] = data.contact_email(key).into();
        if operator
            && let Some(account) = data
                .identities
                .profiles
                .get(key)
                .map(|profile| profile.account_id.clone())
        {
            value["sign_in"] = p.db.account_sign_in(&data, &account)?;
        }
        members.insert(key.clone(), value);
    }
    Ok(reply(json!({"members": members})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Visibility {
    listed: bool,
}
async fn set_visibility(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<Visibility>,
) -> Answer {
    let email = p.admin(&h, true).await?;
    p.db.edit(|d| {
        d.require_app_group(&email, &app)?;
        let game = d
            .apps
            .get_mut(&app)
            .ok_or_else(|| fault(404, "Game not found."))?;
        game.listed = body.listed;
        if body.listed {
            d.admin_hidden_apps.remove(&app);
        } else {
            d.admin_hidden_apps.insert(app.clone());
        }
        Ok(())
    })?;
    Ok(reply(json!({"app_id":app,"listed":body.listed})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameMember {
    email: String,
    name: String,
}
async fn rename_member(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<RenameMember>,
) -> Answer {
    let actor = p.admin(&h, true).await?;
    let email = body.email.trim().to_lowercase();
    let name = body.name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 40 && !name.chars().any(char::is_control),
        fault(
            400,
            "Use a handle of 1–40 characters without control characters."
        )
    );
    let player = p.db.edit(|d| {
        let email = d
            .group_member_key(&email, d.groups.member(&actor))?
            .ok_or_else(|| fault(404, "Member not found."))?;
        ensure!(
            d.groups.member(&actor) == d.groups.member(&email),
            fault(404, "Member not found.")
        );
        let member = d
            .members
            .get_mut(&email)
            .ok_or_else(|| fault(404, "Member not found."))?;
        member.name = name.into();
        Ok(d.player_id(&email))
    })?;
    Ok(reply(json!({"player_id":player,"name":name})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invite {
    email: String,
    name: String,
    #[serde(default)]
    admin: bool,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}
async fn invite(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Invite>) -> Answer {
    let email = p.admin(&h, true).await?;
    p.db.edit(|d| {
        let group = d.groups.member(&email).to_owned();
        let target = d.group_member_key(&body.email, &group)?;
        ensure!(
            target.as_deref() != Some(&email) || (body.enabled && body.admin),
            fault(400, "You cannot disable or demote your own admin account.")
        );
        ensure!(
            target.as_deref().is_none_or(|target| !d.operator(target)) || body.enabled,
            fault(403, "Only the operator can disable an operator account.")
        );
        d.invite_to_group(&group, &body.email, &body.name, body.admin, body.enabled)
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn credentials(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let data = p.db.catalog()?;
    let (_, inspecting) = p.group_view(&h, &email)?;
    ensure!(
        !inspecting,
        fault(403, "Inspection does not expose credentials.")
    );
    Ok(reply(
        json!({"credentials":data.credentials.iter().filter(|(_,c)| data.can_manage_credential(&email, c)).map(|(id,c)|json!({"id":id,"app":c.app,"role":c.role,"label":c.label})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCredential {
    app: String,
    label: String,
}
async fn create_credential(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<NewCredential>,
) -> Answer {
    let owner = p.admin(&h, true).await?;
    ensure!(
        slug(&body.app) && body.label.chars().count() <= 80,
        fault(400, "Use a lowercase game ID and a short label.")
    );
    let (id, token) = p.db.edit(|d| {
        let group = d.groups.member(&owner).to_owned();
        d.reserve_app(&body.app, &group)?;
        let (id, token) = platform::credential(d, &body.app, "publish", &body.label)?;
        d.credentials.get_mut(&id).unwrap().owner = Some(owner.clone());
        d.owners.entry(body.app.clone()).or_insert(owner);
        Ok((id, token))
    })?;
    Ok(reply(
        json!({"id":id,"app_id":body.app,"api_url":format!("https://{}",p.domain()),"token":token}),
    ))
}
async fn revoke(State(p): State<Platform>, Path(id): Path<String>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, true).await?;
    p.db.edit(|d| {
        if let Some(c) = d.credentials.get(&id) {
            ensure!(
                d.can_manage_credential(&email, c),
                fault(404, "Connection not found.")
            );
        }
        d.credentials.remove(&id);
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Exchange {
    token: String,
}
async fn exchange(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Exchange>) -> Answer {
    p.origin(&h, host(&h))?;
    let digest = hash(&body.token);
    p.db.exchange_launch(&digest, host(&h), now())?;
    let cookie = format!(
        "__Host-infuser_game={}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400",
        body.token
    );
    Ok((
        [("Set-Cookie", cookie), ("Cache-Control", "no-store".into())],
        Json(json!({"ok":true})),
    )
        .into_response())
}
async fn context(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (id, l) = p.game(&h)?;
    let d = p.db.catalog()?;
    let b = &d.builds[&build_key(&l.app, &l.build)];
    let player = if l.guest {
        format!("guest-{id}")
    } else {
        d.player_id(&l.email)
    };
    Ok(reply(
        json!({"app_id":l.app,"deployment_id":l.runtime_id(),"visit":l.visit.as_ref().and_then(|id|d.visits.get(id)).map(|v|json!({"expires_at":v.expires,"authorization_seconds":30})),"build_id":l.build,"library_url":format!("https://{}/library",p.domain()),"guest":l.guest,"player_id":player,"name":if l.guest {"Guest"}else{&d.members[&l.email].name},"player_ticket":game_token(&h),"manifest":b.manifest,"capabilities":{"sessions":crate::dedicated::capabilities(),"sdk":crate::sdk_contract::with_peerjs(p.relay.is_some(), p.config().controller.peerjs_port.is_some()),"runtime_sessions":runtime::capabilities(),"presence":presence::capabilities(),"launch_intents":crate::launch_intent::capabilities(),"cloud_checkpoints":crate::cloud::capabilities(p.config().controller.cloud_save_budget_mb)},"runtime":p.refresh(&id)?}),
    ))
}
async fn consume_intent(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    let (id, _) = p.game(&h)?;
    let intent = p.db.take_launch_intent(&id, now())?;
    Ok(reply(json!({"intent":intent})))
}
async fn player(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (id, launch) = p.game(&h)?;
    let data = p.db.catalog()?;
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    let page = presence::open_page(&p.db, &id, &launch.host)?;
    let html = include_str!("../web/game-player.html").replace("PLAYER_PAGE_ID", &page);
    let mut response = (
        [
            ("Cache-Control", "no-store"),
            ("Referrer-Policy", "no-referrer"),
            ("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; media-src blob:; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'"),
        ],
        Html(html),
    ).into_response();
    // Threaded web exports require isolation on the enclosing document as well.
    if build.cross_origin_isolation {
        response.headers_mut().insert(
            "cross-origin-opener-policy",
            header::HeaderValue::from_static("same-origin"),
        );
        response.headers_mut().insert(
            "cross-origin-embedder-policy",
            header::HeaderValue::from_static("require-corp"),
        );
    }
    Ok(response)
}
async fn heartbeat(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (id, _) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.admit(&h).await;
    Ok(reply(p.refresh(&id)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerJoin {
    #[serde(default)]
    create: bool,
    connection_id: String,
    #[serde(default = "yes")]
    relay_only: bool,
    #[serde(default)]
    expected_epoch: Option<String>,
    #[serde(default)]
    expected_player: Option<String>,
    #[serde(default)]
    joinable: Option<bool>,
    #[serde(default)]
    adapter: Option<crate::manifest::PeerAdapter>,
}
async fn peer_join(State(p): State<Platform>, h: HeaderMap, Json(body): Json<PeerJoin>) -> Answer {
    p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.admit(&h).await;
    let _guard = p.publishing.lock().unwrap();
    let (id, launch) = p.game(&h)?;
    let data = p.db.read()?;
    ensure!(
        data.deployed(launch.runtime_id())
            .is_some_and(|a| a.build == launch.build),
        fault(409, "a newer build is available; reopen the game")
    );
    p.peers.retain_launches(
        launch.runtime_id(),
        &presence::live_launches(&data, launch.runtime_id(), now()),
        now(),
    );
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    let multiplayer = build
        .manifest
        .multiplayer
        .as_ref()
        .ok_or_else(|| fault(422, "this game has no multiplayer mode"))?;
    ensure!(
        multiplayer.mode == Mode::PeerHosted
            && multiplayer.transport == crate::manifest::Transport::Webrtc,
        fault(422, "this game does not use WebRTC peer hosting")
    );
    let relay = p
        .relay
        .as_ref()
        .ok_or_else(|| fault(503, "TURN is not configured"))?;
    ensure!(
        body.adapter == multiplayer.adapter,
        fault(422, "use the published game's networking adapter")
    );
    ensure!(
        body.adapter.is_none() || p.config().controller.peerjs_port.is_some(),
        fault(503, "PeerJS is not configured")
    );
    let who = crate::peers::Identity {
        launch: id.clone(),
        player: if launch.guest {
            format!("guest-{id}")
        } else {
            data.player_id(&launch.email)
        },
        name: if launch.guest {
            "Guest".into()
        } else {
            data.members[&launch.email].name.clone()
        },
        guest: launch.guest,
    };
    let mut result = p.peers.join(
        who,
        crate::peers::Join {
            app: launch.runtime_id(),
            build: &launch.build,
            connection: &body.connection_id,
            max_players: multiplayer.max_players,
            create: body.create,
            expected_epoch: body.expected_epoch.as_deref(),
            expected_player: body.expected_player.as_deref(),
            joinable: body.joinable,
        },
        now(),
    )?;
    result["ice_servers"] = json!([relay.credentials(result["self_id"].as_str().unwrap(), now())]);
    if body.adapter.is_some() {
        result["peerjs"] = json!({"path":"/_infuser/peerjs/", "id":format!("i-{}-{}",result["epoch"].as_str().unwrap(),result["self_id"].as_str().unwrap()),"key":"peerjs"});
    }
    result["ice_transport_policy"] = json!(if body.relay_only
        || matches!(
            multiplayer.direct_connect,
            Some(crate::manifest::DirectConnect::Disabled)
        ) {
        "relay"
    } else {
        "all"
    });
    Ok(reply(result))
}
#[derive(Deserialize, Default)]
struct PeerCursor {
    #[serde(default)]
    after: u64,
}
async fn peer_poll(
    State(p): State<Platform>,
    Path(peer): Path<String>,
    Query(cursor): Query<PeerCursor>,
    h: HeaderMap,
) -> Answer {
    let (id, launch) = p.game(&h)?;
    if h.contains_key("origin") {
        p.origin(&h, host(&h))?;
    }
    ensure!(
        h.get("sec-fetch-site").is_none_or(|v| v == "same-origin"),
        fault(403, "poll from the game origin")
    );
    Ok(reply(p.peers.poll(
        launch.runtime_id(),
        &peer,
        &id,
        cursor.after,
        now(),
    )?))
}
async fn peer_signal(
    State(p): State<Platform>,
    Path(peer): Path<String>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers
        .signal(launch.runtime_id(), &peer, &id, &body, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_leave(State(p): State<Platform>, Path(peer): Path<String>, h: HeaderMap) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers.leave(launch.runtime_id(), &peer, &id, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_ice(State(p): State<Platform>, Path(peer): Path<String>, h: HeaderMap) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers.poll(launch.runtime_id(), &peer, &id, 0, now())?;
    let relay = p
        .relay
        .as_ref()
        .ok_or_else(|| fault(503, "TURN is not configured"))?;
    Ok(reply(
        json!({"ice_servers":[relay.credentials(&peer, now())]}),
    ))
}
async fn peer_health(
    State(p): State<Platform>,
    Path(peer): Path<String>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers
        .health(launch.runtime_id(), &peer, &id, body, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_health_admin(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let data = p.db.catalog()?;
    let mut report = p.peers.diagnostics(now());
    if let Some(sessions) = report["sessions"].as_array_mut() {
        sessions.retain(|s| {
            s["app_id"]
                .as_str()
                .is_some_and(|app| data.deployment_group(app) == group)
        });
    }
    Ok(reply(report))
}
async fn proxy_auth(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (_, l) = p.game(&h)?;
    if h.contains_key("origin") {
        p.origin(&h, host(&h))?;
    }
    let state = p.controller.snapshot();
    let i = state
        .games
        .get(l.runtime_id())
        .ok_or_else(|| fault(404, "no server for this game"))?;
    ensure!(
        i.spec.web && i.phase == crate::store::Phase::Ready,
        fault(503, "game server is not ready")
    );
    ensure!(
        l.lease_id.is_some(),
        fault(403, "this is a solo client-only session")
    );
    p.controller.lease(
        l.runtime_id(),
        l.lease_id.as_ref().unwrap(),
        l.lease_token.as_ref().unwrap(),
        "get",
        now(),
    )?;
    let upstream = p.controller.web_address(l.runtime_id(), &i.attempt)?;
    Ok((
        [
            ("X-Infuser-Port", i.port.to_string()),
            ("X-Infuser-Upstream", upstream.to_string()),
            ("Cache-Control", "no-store".into()),
        ],
        "",
    )
        .into_response())
}
async fn leave(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    let _guard = p.publishing.lock().unwrap();
    let (id, l) = p.game(&h)?;
    p.peers.leave_launch(l.runtime_id(), &id);
    if let (Some(lease), Some(token)) = (&l.lease_id, &l.lease_token) {
        let _ = p
            .controller
            .lease(l.runtime_id(), lease, token, "delete", now());
    }
    p.db.expire_launch(&id, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn save_get(State(p): State<Platform>, Path(slot): Path<String>, h: HeaderMap) -> Answer {
    let (id, l) = p.game(&h)?;
    ensure!(slug(&slot), fault(400, "invalid save slot"));
    let owner = if l.guest {
        format!("guest-{id}")
    } else {
        p.db.catalog()?.player_id(&l.email)
    };
    let (version, data) = p.db.save_read(&owner, l.runtime_id(), &slot)?;
    Ok(reply(json!({"version":version,"data":data})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    version: u64,
    data: Value,
}
async fn save_put(
    State(p): State<Platform>,
    Path(slot): Path<String>,
    h: HeaderMap,
    Json(body): Json<Save>,
) -> Answer {
    let (id, l) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    ensure!(slug(&slot), fault(400, "invalid save slot"));
    let owner = if l.guest {
        format!("guest-{id}")
    } else {
        p.db.catalog()?.player_id(&l.email)
    };
    let version =
        p.db.save_write(&owner, l.runtime_id(), &slot, body.version, &body.data)?;
    Ok(reply(json!({"version":version})))
}
#[derive(Deserialize)]
struct Ticket {
    ticket: String,
}
async fn verify_ticket(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Ticket>,
) -> Answer {
    let app = p.publisher(&h, None, "server")?;
    let data = p.db.read()?;
    let digest = hash(&body.ticket);
    let (id, l) = data
        .launches
        .iter()
        .find(|(_, l)| {
            l.runtime_id() == app && l.token_hash == digest && l.exchanged && l.expires > now()
        })
        .ok_or_else(|| fault(401, "invalid game ticket"))?;
    ensure!(
        data.launch_allowed(l, now()),
        fault(403, "Game authorization ended.")
    );
    ensure!(
        data.profile_enabled(&l.email),
        fault(403, "membership disabled")
    );
    ensure!(
        l.lease_id.is_some(),
        fault(403, "ticket is for client-only solo play")
    );
    p.controller.lease(
        &app,
        l.lease_id.as_ref().unwrap(),
        l.lease_token.as_ref().unwrap(),
        "get",
        now(),
    )?;
    Ok(reply(
        json!({"app_id":app,"player_id":if l.guest{format!("guest-{id}")}else{data.player_id(&l.email)},"name":if l.guest{"Guest"}else{&data.members[&l.email].name},"guest":l.guest,"expires_at":l.expires}),
    ))
}
async fn game_file(State(p): State<Platform>, mut request: Request) -> Answer {
    ensure!(
        !request.uri().path().starts_with("/_infuser/"),
        fault(404, "unsupported Infuser route")
    );
    if host(request.headers()) == p.domain() {
        return Err(fault(404, "not found").into());
    }
    let valid_host = host(request.headers())
        .strip_suffix(&format!(".{}", p.domain()))
        .is_some_and(|label| {
            let opaque = label
                .split_once("--g-")
                .filter(|(slug, _)| crate::game_urls::valid(slug))
                .map(|(_, id)| id)
                .or_else(|| label.strip_prefix("g-"));
            opaque.is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        });
    ensure!(valid_host, fault(404, "unknown game host"));
    let h = request.headers();
    let session = p.game(h);
    if session.is_err() {
        if request.uri().path() == "/" && request.method() == axum::http::Method::GET {
            return Ok(([("Cache-Control","no-store"),("Referrer-Policy","no-referrer"),("Content-Security-Policy","default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'")],Html(include_str!("../web/game-launch.html"))).into_response());
        }
        return Err(session.err().unwrap().into());
    }
    let (_, l) = session.unwrap();
    let data = p.db.catalog()?;
    let b = &data.builds[&build_key(&l.app, &l.build)];
    let path = request.uri().path().trim_start_matches('/');
    let mut selected = if path.is_empty() {
        "index.html".to_string()
    } else {
        decode_path(path)?
    };
    ensure!(valid_path(&selected), fault(400, "invalid path"));
    if !b.files.contains_key(&selected)
        && b.spa
        && !selected.contains('.')
        && h.get("accept")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/html"))
    {
        selected = "index.html".into();
    }
    ensure!(
        b.files.contains_key(&selected),
        fault(404, "asset not found")
    );
    let encoded = encoded_web_asset(&selected);
    let file = p.root().join(&l.app).join(&l.build).join(&selected);
    // Never forward a platform or game credential into a filesystem service.
    request.headers_mut().remove("cookie");
    request.headers_mut().remove("authorization");
    // A Range addresses one representation. A slice of a Brotli or gzip file
    // is not a byte range of the original, and a browser cannot decode it.
    // Conditional range requests therefore stay on the original bytes.
    let negotiable = encoded.is_none();
    let ranged = request.headers().contains_key(header::RANGE);
    // Unity loaders request compressed filenames explicitly. Do not negotiate
    // another compressed sibling for bytes which already have an encoding.
    let service = ServeFile::new(file);
    let service = if negotiable && !ranged {
        service.precompressed_br().precompressed_gzip()
    } else {
        service
    };
    let response = service.oneshot(request).await.unwrap();
    let mut response = response.map(Body::new);
    if (response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED)
        && let Some((encoding, content_type)) = encoded
    {
        response.headers_mut().insert(
            "content-encoding",
            header::HeaderValue::from_static(encoding),
        );
        response.headers_mut().insert(
            "content-type",
            header::HeaderValue::from_static(content_type),
        );
    }
    if negotiable
        && (response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED)
    {
        // 304 responses from the file service omit Vary. The full response
        // varies on Accept-Encoding, including when this one is uncompressed.
        response.headers_mut().insert(
            header::VARY,
            header::HeaderValue::from_static("accept-encoding"),
        );
    }
    response.headers_mut().insert(
        "cache-control",
        header::HeaderValue::from_static("private, no-cache"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        header::HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        header::HeaderValue::from_static("nosniff"),
    );
    if b.cross_origin_isolation {
        response.headers_mut().insert(
            "cross-origin-opener-policy",
            header::HeaderValue::from_static("same-origin"),
        );
        response.headers_mut().insert(
            "cross-origin-embedder-policy",
            header::HeaderValue::from_static("require-corp"),
        );
    }
    Ok(response)
}
// .unityweb is deliberately excluded: Unity's fallback loader decompresses it.
// Only runtime assets get automatic encoding, not downloadable .gz archives.
fn encoded_web_asset(path: &str) -> Option<(&'static str, &'static str)> {
    let (plain, encoding) = path
        .strip_suffix(".br")
        .map(|p| (p, "br"))
        .or_else(|| path.strip_suffix(".gz").map(|p| (p, "gzip")))?;
    let content_type = if plain.ends_with(".wasm") {
        "application/wasm"
    } else if plain.ends_with(".js") {
        "application/javascript"
    } else if plain.ends_with(".data") || plain.ends_with(".bundle") {
        "application/octet-stream"
    } else {
        return None;
    };
    Some((encoding, content_type))
}
fn decode_path(path: &str) -> Result<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure!(i + 2 < bytes.len(), fault(400, "invalid URL"));
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| fault(400, "invalid URL"))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| fault(400, "invalid URL"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGame {
    title: Option<String>,
}
async fn new_game(State(p): State<Platform>, h: HeaderMap, Json(body): Json<NewGame>) -> Answer {
    let email = p.member(&h, true).await?;
    ensure!(
        body.title
            .as_ref()
            .is_none_or(|title| !title.trim().is_empty() && title.chars().count() <= 80),
        fault(400, "Give your game a short name.")
    );
    let title = body.title.as_deref().map(str::trim);
    let words: String = title
        .unwrap_or("")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let prefix = words
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let app = if title.is_none() {
        format!("game-{}", &secret()[..12])
    } else {
        format!(
            "game-{}-{}",
            prefix
                .chars()
                .take(28)
                .collect::<String>()
                .trim_end_matches('-'),
            &secret()[..6]
        )
    };
    let expires = now() + SETUP_CODE_SECONDS;
    let token = secret();
    p.db.edit(|d| {
        ensure!(
            d.owners.values().filter(|owner| **owner == email).count() < 256
                || d.members[&email].admin,
            fault(429, "Game limit reached; ask an admin.")
        );
        let group = d.groups.member(&email).to_owned();
        d.reserve_app(&app, &group)?;
        d.setup_claims.retain(|_, claim| claim.expires > now());
        d.setup_claims.insert(
            hash(&token),
            platform::SetupClaim {
                app: app.clone(),
                owner: email.clone(),
                label: title.unwrap_or("Existing game").into(),
                expires,
            },
        );
        d.owners.insert(app.clone(), email.clone());
        for visit in d
            .visits
            .values_mut()
            .filter(|v| v.active(now()) && v.participant(&email))
        {
            visit.games.insert(app.clone());
        }
        if let Some(title) = title {
            crate::game_urls::assign(d, &app, title);
        }
        Ok(())
    })?;
    let code = format!("infuser_setup_{token}");
    let d = p.db.catalog()?;
    Ok(reply(
        json!({"app_id":app,"api_url":format!("https://{}",p.domain()),"setup_code":code,"expires_at":expires,"supported_engines":crate::publisher::supported_engines(),"tag_catalog":crate::tags::catalog_in_group(&d,false,d.groups.member(&email))}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimRequest {
    token: String,
    app_id: Option<String>,
}
async fn claim_setup(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<ClaimRequest>,
) -> Answer {
    p.main_host(&h)?;
    ensure!(body.token.len() == 64, fault(401, "invalid setup code"));
    let (id, token, app) = p.db.edit(|d| {
        let key = hash(&body.token);
        let claim = d
            .setup_claims
            .get(&key)
            .ok_or_else(|| fault(401, "setup code is invalid or has already been used"))?
            .clone();
        ensure!(
            claim.expires > now(),
            fault(410, "setup code expired; copy a new publishing prompt")
        );
        ensure!(
            body.app_id.as_ref().is_none_or(|app| *app == claim.app),
            fault(400, "setup code belongs to a different game")
        );
        ensure!(
            d.profile_enabled(&claim.owner),
            fault(403, "publisher membership disabled")
        );
        d.require_app_group(&claim.owner, &claim.app)?;
        let (id, token) = platform::credential(d, &claim.app, "publish", &claim.label)?;
        d.credentials.get_mut(&id).unwrap().owner = Some(claim.owner);
        d.setup_claims.remove(&key);
        Ok((id, token, claim.app))
    })?;
    Ok(reply(
        json!({"id":id,"app_id":app,"api_url":format!("https://{}",p.domain()),"token":token}),
    ))
}
const SETUP_CODE_SECONDS: u64 = 3 * 60 * 60;

async fn renew_setup(State(p): State<Platform>, Path(app): Path<String>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    let token = secret();
    let expires = now() + SETUP_CODE_SECONDS;
    p.db.edit(|d| {
        d.require_app_group(&email, &app)?;
        let owner = d
            .owners
            .get(&app)
            .ok_or_else(|| fault(404, "unknown game"))?
            .clone();
        ensure!(
            owner == email || d.members[&email].admin,
            fault(403, "not your game")
        );
        ensure!(
            d.profile_enabled(&owner),
            fault(403, "publisher membership disabled")
        );
        d.setup_claims
            .retain(|_, claim| claim.app != app && claim.expires > now());
        d.setup_claims.insert(
            hash(&token),
            platform::SetupClaim {
                app: app.clone(),
                owner,
                label: app.clone(),
                expires,
            },
        );
        Ok(())
    })?;
    let code = format!("infuser_setup_{token}");
    let d = p.db.catalog()?;
    Ok(reply(
        json!({"app_id":app,"api_url":format!("https://{}",p.domain()),"setup_code":code,"expires_at":expires,"supported_engines":crate::publisher::supported_engines(),"tag_catalog":crate::tags::catalog_in_group(&d,false,d.groups.member(&email))}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagsRequest {
    tags: Vec<String>,
}
async fn set_tags(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<TagsRequest>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let tags = crate::tags::normalize(&body.tags)?;
    p.db.edit(|d| {
        d.require_app_group(&email, &app)?;
        ensure!(d.apps.contains_key(&app), fault(404, "unknown game"));
        ensure!(
            d.members[&email].admin || d.owners.get(&app) == Some(&email),
            fault(403, "only the author or an admin can edit these tags")
        );
        d.tags.insert(app, tags.clone());
        Ok(())
    })?;
    Ok(reply(json!({"tags":tags})))
}
async fn publisher_tags(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let app = p.publisher(&h, None, "publish")?;
    let d = p.db.catalog()?;
    Ok(reply(
        json!({"tags":crate::tags::catalog_in_group(&d,false,d.groups.app(&app))}),
    ))
}
async fn publisher_set_tags(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<TagsRequest>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let tags = crate::tags::normalize(&body.tags)?;
    p.db.edit(|d| {
        ensure!(d.apps.contains_key(&app), fault(404, "unknown game"));
        d.tags.insert(app, tags.clone());
        Ok(())
    })?;
    Ok(reply(json!({"tags":tags})))
}

impl Platform {
    fn cloud_group(
        &self,
        h: &HeaderMap,
        group: &str,
    ) -> Result<(String, Launch, crate::cloud::Group)> {
        let (id, launch) = self.game(h)?;
        let data = self.db.catalog()?;
        let declaration = data
            .builds
            .get(&build_key(&launch.app, &launch.build))
            .and_then(|b| b.manifest.cloud_saves.as_ref())
            .and_then(|d| d.groups.iter().find(|g| g.id == group))
            .cloned()
            .ok_or_else(|| {
                crate::cloud::fault(
                    422,
                    "undeclared_group",
                    "this build does not declare that cloud save group",
                )
            })?;
        let owner = if launch.guest {
            format!("guest-{id}")
        } else {
            data.player_id(&launch.email)
        };
        Ok((owner, launch, declaration))
    }
}
async fn cloud_load(State(p): State<Platform>, Path(group): Path<String>, h: HeaderMap) -> Answer {
    let (owner, launch, _) = p.cloud_group(&h, &group)?;
    Ok(reply(p.db.cloud_load(
        crate::cloud::Scope {
            owner: &owner,
            app: launch.runtime_id(),
            group: &group,
        },
        None,
    )?))
}
async fn cloud_revision(
    State(p): State<Platform>,
    Path((group, revision)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    let (owner, launch, _) = p.cloud_group(&h, &group)?;
    Ok(reply(p.db.cloud_load(
        crate::cloud::Scope {
            owner: &owner,
            app: launch.runtime_id(),
            group: &group,
        },
        Some(&revision),
    )?))
}
async fn cloud_history(
    State(p): State<Platform>,
    Path(group): Path<String>,
    h: HeaderMap,
) -> Answer {
    let (owner, launch, _) = p.cloud_group(&h, &group)?;
    Ok(reply(p.db.cloud_history(crate::cloud::Scope {
        owner: &owner,
        app: launch.runtime_id(),
        group: &group,
    })?))
}
async fn cloud_commit(
    State(p): State<Platform>,
    Path(group): Path<String>,
    h: HeaderMap,
    body: std::result::Result<Json<crate::cloud::Commit>, axum::extract::rejection::JsonRejection>,
) -> Answer {
    let (owner, launch, declaration) = p.cloud_group(&h, &group)?;
    p.origin(&h, host(&h))?;
    let Json(body) = body.map_err(|e| {
        crate::cloud::fault(
            e.status().as_u16(),
            "invalid_checkpoint",
            "provide a complete JSON checkpoint with an explicit expected_revision and commit_id",
        )
    })?;
    let result = p.db.cloud_commit(
        crate::cloud::Scope {
            owner: &owner,
            app: launch.runtime_id(),
            group: &group,
        },
        &declaration,
        &body,
        crate::cloud::WriteOptions {
            build: &launch.build,
            budget_bytes: p.config().controller.cloud_save_budget_mb * 1024 * 1024,
            disk_reserve_bytes: p.config().controller.disk_reserve_mb * 1024 * 1024,
            time: now(),
        },
    )?;
    let mut response = reply(serde_json::to_value(&result).map_err(anyhow::Error::from)?);
    *response.status_mut() = StatusCode::from_u16(result.status()).unwrap();
    Ok(response)
}
