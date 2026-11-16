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
use tower::{ServiceExt, limit::ConcurrencyLimitLayer};
use tower_http::services::ServeFile;
mod avatars;
mod capture;
mod card_art;
mod chat;
mod client;
mod connections;
mod dedicated;
mod feedback;
mod groups;
mod library;
mod metadata;
mod native;
mod peerjs;
mod presence;
mod pwa;
mod runtime;
mod sdk_v1;
mod server_builds;
mod server_diagnostics;
mod urls;

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
            return (StatusCode::from_u16(f.status).unwrap(), [("Cache-Control", "no-store")],
                Json(json!({"error":{"code":f.code,"message":f.message}}))).into_response();
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
    grants: Arc<Mutex<BTreeMap<IpAddr, u64>>>,
    publishing: Arc<Mutex<()>>,
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
}
impl Platform {
    pub fn peer_rooms(&self) -> Arc<crate::peers::Rooms> {
        self.peers.clone()
    }
    pub fn open(controller: Arc<Controller>) -> Result<Self> {
        let db = Arc::new(Database::open(&controller.config.controller.state_dir)?);
        anyhow::ensure!(
            db.read()?.groups.names.len() <= 1 || controller.config.controller.private_groups,
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
        Ok(Self {
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
            peers: Arc::new(crate::peers::Rooms::recovering(resume_after)),
            relay,
        })
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
            .get("x-infuser-verified-email")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| fault(401, "Google session did not supply an email."))?
            .to_lowercase();
        ensure!(
            self.db
                .read()?
                .members
                .get(&email)
                .is_some_and(|m| m.enabled),
            fault(403, "Ask your admin for an Infuser invitation.")
        );
        self.group_view(h, &email)?;
        Ok(email)
    }
    async fn admin(&self, h: &HeaderMap, write: bool) -> Result<String> {
        let email = self.member(h, write).await?;
        ensure!(
            self.db.read()?.members[&email].admin
                || (!write
                    && h.contains_key(groups::INSPECT)
                    && self.db.read()?.groups.operator(&email)),
            fault(403, "An admin account is required.")
        );
        Ok(email)
    }
    fn publisher(&self, h: &HeaderMap, app: Option<&str>, role: &str) -> Result<String> {
        self.main_host(h)?;
        let token = bearer(h).ok_or_else(|| fault(401, "publisher bearer token required"))?;
        let digest = hash(token);
        let data = self.db.read()?;
        let credential = data
            .credentials
            .values()
            .find(|c| c.hash == digest && c.role == role && app.is_none_or(|id| id == c.app))
            .ok_or_else(|| fault(401, "invalid or revoked app credential"))?;
        ensure!(
            credential
                .owner
                .as_ref()
                .is_none_or(|email| data.members.get(email).is_some_and(|m| m.enabled)),
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
        if self
            .grants
            .lock()
            .unwrap()
            .get(&ip)
            .is_some_and(|t| *t + 60 > now())
        {
            return true;
        }
        match crate::firewall::grant(socket, ip).await {
            Ok(()) => {
                let mut g = self.grants.lock().unwrap();
                g.retain(|_, t| *t + 1800 > now());
                g.insert(ip, now());
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
        let data = self.db.read()?;
        let (id, l) = data
            .launches
            .iter()
            .find(|(_, l)| {
                l.token_hash == digest && l.host == host(h) && l.exchanged && l.expires > now()
            })
            .ok_or_else(|| fault(401, "Game session expired. Launch it again."))?;
        ensure!(
            data.members.get(&l.email).is_some_and(|m| m.enabled),
            fault(403, "Membership is disabled.")
        );
        data.require_app_group(&l.email, &l.app)?;
        Ok((id.clone(), l.clone()))
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
        ensure!(data.members.get(&launch.email).is_some_and(|m|m.enabled)
            && data.groups.allows(&launch.email,&launch.app),fault(401,"Game authorization ended."));
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
                .lease(&launch.app, lease, token, "renew", now())?;
        }
        if !on_demand
            && launch.lease_id.is_some()
            && status["state"] == "ready"
            && self
                .controller
                .snapshot()
                .games
                .get(&launch.app)
                .is_some_and(|i| i.spec.web)
        {
            status["endpoint"] = json!({"url":format!("https://{}/_infuser/server",launch.host)});
        }
        self.db.edit(|d| {
            let l = d
                .launches
                .get_mut(id)
                .ok_or_else(|| fault(410, "game session expired"))?;
            l.seen = now();
            l.expires = now() + 300;
            if !l.guest {
                d.presence.insert(l.email.clone(), now());
            }
            Ok(())
        })?;
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
        .route("/api/publisher/capabilities", get(capabilities))
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
        .route("/api/publisher/apps/{app}", delete(unpublish))
        .route("/api/publisher/apps/{app}/server-diagnostics", get(server_diagnostics::get))
        .route(
            "/api/publisher/apps/{app}/builds/{build}",
            put(create_build).get(build_status).delete(remove_build),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/files/{*path}",
            put(upload),
        )
        .route(
            "/api/publisher/apps/{app}/builds/{build}/publish",
            post(publish),
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
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(ConcurrencyLimitLayer::new(1));
    Router::new()
        .merge(publishing)
        .merge(native::routes())
        .merge(groups::routes())
        .merge(feedback::routes())
        .merge(pwa::routes())
        .route("/assets/groups.js",get(|| async { ([("content-type","text/javascript"),("cache-control","no-cache")],include_str!("../web/groups.js")) }))
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
        .route(
            "/assets/clubhouse.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript"),(header::CACHE_CONTROL,"no-cache")],
                    include_str!("../web/clubhouse.js"),
                )
            }),
        )
        .route(
            "/assets/clubhouse.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css"),(header::CACHE_CONTROL,"no-cache")],
                    include_str!("../web/clubhouse.css"),
                )
            }),
        )
        .route("/assets/themes.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript"),(header::CACHE_CONTROL,"no-cache")], include_str!("../web/themes.js")) }))
        .route("/assets/themes/nightglass.css", get(|| async { ([(header::CONTENT_TYPE, "text/css"),(header::CACHE_CONTROL,"no-cache")], include_str!("../web/themes/nightglass.css")) }))
        .route("/assets/themes/cartridge-club.css", get(|| async { ([(header::CONTENT_TYPE, "text/css"),(header::CACHE_CONTROL,"no-cache")], include_str!("../web/themes/cartridge-club.css")) }))
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
        .route("/api/profile", put(avatars::update).layer(DefaultBodyLimit::max(1024)))
        .route("/api/library", get(library::list))
        .route("/api/client-state", get(client::state))
        .route("/assets/client-updates.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript"),(header::CACHE_CONTROL,"no-cache")], include_str!("../web/client-updates.js")) }))
        .route("/api/chat", get(chat::list).post(chat::send).layer(DefaultBodyLimit::max(4096)))
        .route("/api/chat/{id}", delete(chat::remove))
        .route("/assets/chat.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript"),(header::CACHE_CONTROL,"no-cache")], include_str!("../web/chat.js")) }))
        .route("/api/games/{slug}", get(urls::resolve))
        .route("/api/apps/{app}/slug", put(urls::owner_rename))
        .route("/api/apps/{app}/tags",put(set_tags))
        .route("/api/my-games", post(new_game))
        .route("/api/my-games/{app}/setup",post(renew_setup))
        .route("/api/apps/{app}/launch", post(launch))
        .route("/api/apps/{app}/card-art/{build}", get(card_art::image))
        .route("/api/launches/{id}", get(launch_status))
        .route("/api/apps/{app}/reaction", put(react).delete(unreact))
        .route("/api/admin/members", get(members).post(invite))
        .route("/api/admin/members/handle", put(rename_member).layer(DefaultBodyLimit::max(1024)))
        .route("/api/admin/apps/{app}/visibility", put(set_visibility).layer(DefaultBodyLimit::max(1024)))
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
        .with_state(platform)
}
async fn session_auth(
    State(p): State<Platform>,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    request.headers_mut().remove("x-infuser-verified-email");
    let path = request.uri().path();
    let player_api = path.starts_with("/api/")
        && !path.starts_with("/api/publisher/")
        && !path.starts_with("/api/metadata/")
        && !path.starts_with("/api/server/");
    if !player_api || native::public(path) || host(request.headers()) != p.domain() {
        return next.run(request).await;
    }
    let cookie = request.headers().get("cookie").cloned();
    let verified: Result<_> = async {
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
        let cookies: Vec<_> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .cloned()
            .collect();
        Ok((email, cookies))
    }
    .await;
    match verified {
        Err(error) => Error(error).into_response(),
        Ok((email, cookies)) => {
            request
                .headers_mut()
                .insert("x-infuser-verified-email", email);
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
    let data = p.db.read()?;
    let member = &data.members[&email];
    Ok(reply(
        json!({"name":member.name,"admin":member.admin&&!inspecting,"operator":data.groups.operator(&email),"inspecting":inspecting,"group":{"id":group,"name":data.groups.names.get(&group)},"home_group":data.groups.member(&email),"player_id":hash(&email),"avatar":member.avatar,"avatars":avatars::catalog(),"game_ports_admitted":admitted}),
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
    p.admit(&h).await;
    let _publishing = p.publishing.lock().unwrap();
    let data = p.db.read()?;
    data.require_app_group(&email, &app)?;
    let selected = data
        .apps
        .get(&app)
        .ok_or_else(|| fault(404, "unknown game"))?;
    ensure!(
        selected.listed || data.members[&email].admin,
        fault(404, "unknown game")
    );
    let b = &data.builds[&build_key(&app, &selected.build)];
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
        Some(p.controller.acquire(&app, &id, now())?)
    } else {
        None
    };
    let host_id = if body.guest || body.intent.is_some() {
        id.clone()
    } else {
        // A new build must not replace cookies/assets in an already-open old tab.
        // Personal cloud saves keep the same player/app key across these origins.
        hash(&format!("{email}/{app}/{}", selected.build))[..32].into()
    };
    let gamehost = crate::game_urls::host(&data, &app, &host_id, p.domain());
    let l = Launch {
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
    p.db.edit(|d| {
        d.launches.insert(id.clone(), l);
        Ok(())
    })?;
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
    data.require_app_group(&email, &l.app)?;
    // Startup may take longer than the initial grant window. Only the authenticated
    // launcher can extend the unused, single-use exchange token.
    p.db.edit(|d| {
        if let Some(l) = d.launches.get_mut(&id)
            && !l.exchanged
        {
            l.grant_expires = now() + 120;
        }
        Ok(())
    })?;
    Ok(reply(p.refresh(&id)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct React {
    emoji: String,
    text: String,
}
async fn react(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<React>,
) -> Answer {
    let email = p.member(&h, true).await?;
    p.visible_app(&h, &email, &app)?;
    ensure!(
        ["⭐", "👍", "🚀", "❤️", "👏", "💡"].contains(&body.emoji.as_str())
            && body.text.chars().count() <= 140,
        fault(
            400,
            "Choose a reaction and keep your note to 140 characters."
        )
    );
    p.db.edit(|d| {
        ensure!(d.apps.contains_key(&app), fault(404, "unknown game"));
        d.reactions.entry(app).or_default().insert(
            email,
            Reaction {
                emoji: body.emoji,
                text: body.text.trim().into(),
                updated: now(),
            },
        );
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn unreact(State(p): State<Platform>, Path(app): Path<String>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    p.visible_app(&h, &email, &app)?;
    p.db.edit(|d| {
        if let Some(r) = d.reactions.get_mut(&app) {
            r.remove(&email);
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn members(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let data = p.db.read()?;
    Ok(reply(
        json!({"members":data.members.iter().filter(|(email,_)|data.groups.member(email)==group).collect::<BTreeMap<_,_>>()}),
    ))
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
    p.db.edit(|d| {
        ensure!(
            d.groups.member(&actor) == d.groups.member(&email),
            fault(404, "Member not found.")
        );
        let member = d
            .members
            .get_mut(&email)
            .ok_or_else(|| fault(404, "Member not found."))?;
        member.name = name.into();
        Ok(())
    })?;
    Ok(reply(json!({"player_id":hash(&email),"name":name})))
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
    ensure!(
        body.email.trim().to_lowercase() != email || (body.enabled && body.admin),
        fault(400, "You cannot disable or demote your own admin account.")
    );
    p.db.edit(|d| {
        let group = d.groups.member(&email).to_owned();
        ensure!(
            !d.groups.operator(&body.email.trim().to_lowercase()) || body.enabled,
            fault(403, "Only the operator can disable an operator account.")
        );
        d.invite_to_group(&group, &body.email, &body.name, body.admin, body.enabled)
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn credentials(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let data = p.db.read()?;
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
async fn capabilities(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let app = p.publisher(&h, None, "publish")?;
    Ok(reply(
        json!({"sessions":crate::dedicated::capabilities(),"server_diagnostics":crate::docker::diagnostics_capabilities(),"server_packaging":server_builds::capabilities(&p),"game_metadata":{"version":1,"title_max_chars":80,"description_max_chars":2000,"art_prompt_max_chars":4000,"card_art_override":true},"native_delivery":{"version":1,"status":"preview","engine":"godot","platforms":infuser_native_protocol::PLATFORMS},"card_art":crate::card_art::capabilities(),"supported_engines":crate::publisher::supported_engines(),"sdk":crate::sdk_contract::with_peerjs(p.relay.is_some(), p.config().controller.peerjs_port.is_some()),"schema_version":1,"app_id":app,"architecture":std::env::consts::ARCH,"client_types":["web"],"play_modes":["solo","multiplayer"],"multiplayer":{"peer_hosted":p.relay.is_some(),"relayed":false,"dedicated":true,"persistent_world":true},"session_policy":"one_per_app","runtime_sessions":runtime::capabilities(),"presence":presence::capabilities(),"launch_intents":crate::launch_intent::capabilities(),"max_file_bytes":67108864,"max_build_bytes":268435456,"cloud_saves":true,"cloud_checkpoints":crate::cloud::capabilities(p.config().controller.cloud_save_budget_mb),"guest_mode":true,"sdk_path":"/_infuser/sdk.js"}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildRequest {
    manifest: Manifest,
    server: Option<Game>,
    server_recipe: Option<crate::server_package::Recipe>,
    #[serde(default)]
    spa: bool,
    #[serde(default)]
    cross_origin_isolation: bool,
}
async fn create_build(
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
                files: BTreeMap::new(),
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
async fn build_status(
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
async fn upload(
    State(p): State<Platform>,
    Path((app, build, path)): Path<(String, String, String)>,
    h: HeaderMap,
    body: Bytes,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(
        slug(&app) && slug(&build) && valid_path(&path),
        fault(400, "invalid artifact path")
    );
    let _guard = p.publishing.lock().unwrap();
    let data = p.db.read()?;
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
    if path == crate::card_art::PATH {
        crate::card_art::validate(&body).map_err(|e| fault(422, &format!("{e:#}")))?;
    }
    let prior = b.files.get(&path).copied().unwrap_or(0);
    let size = body.len() as u64;
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
        disk.f_bavail * disk.f_frsize >= p.config().controller.disk_reserve_mb * 1024 * 1024 + size,
        fault(507, "host disk reserve would be exhausted")
    );
    let dir = p.root().join(&app).join(&build);
    let target = dir.join(&path);
    std::fs::create_dir_all(target.parent().unwrap())?;
    let staged = dir.join(format!(".upload-{}", secret()));
    std::fs::write(&staged, &body)?;
    std::fs::File::open(&staged)?.sync_all()?;
    std::fs::rename(staged, target)?;
    p.db.edit(|d| {
        d.builds
            .get_mut(&key)
            .unwrap()
            .files
            .insert(path.clone(), size);
        Ok(())
    })?;
    Ok(reply(json!({"path":path,"bytes":size})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Promotion {
    #[serde(default = "yes")]
    listed: bool,
}
async fn publish(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
    body: Option<Json<Promotion>>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let _guard = p.publishing.lock().unwrap();
    let state = p.promote(&app, &build, body.as_ref().is_none_or(|b| b.listed))?;
    let slug = p.db.read()?.game_urls[&app].slug.clone();
    let mut response = reply(
        json!({"app_id":app,"build_id":build,"branch":"default","state":state,"slug":slug,"url":format!("https://{}/games/{slug}",p.domain())}),
    );
    if state == "queued" {
        *response.status_mut() = StatusCode::ACCEPTED;
    }
    Ok(response)
}
impl Platform {
    // Callers hold the publication lock, also used by player launches. Never switch
    // the client pointer while its shared server is still on the previous build.
    fn promote(&self, app: &str, build: &str, listed: bool) -> Result<&'static str> {
        let key = build_key(app, build);
        self.db.edit(|d| {
            let b = d.builds.get(&key).ok_or_else(|| fault(404,"unknown build"))?.clone();
            anyhow::ensure!(d.server_builds.get(&key).is_none_or(|j| j.state == crate::server_package::Phase::Ready && b.server.is_some()),fault(409,"server packaging has not passed readiness checks"));
            anyhow::ensure!(b.files.contains_key("index.html"), fault(422,"web build needs index.html"));
            let index = std::fs::read_to_string(self.root().join(&key).join("index.html"))?;
            if let Some(sdk) = &b.manifest.sdk { sdk.admit(&crate::sdk_contract::with_peerjs(self.relay.is_some(), self.config().controller.peerjs_port.is_some())).map_err(|e|fault(422,&e.to_string()))?; }
            let script = crate::sdk_contract::script(&b.manifest);
            anyhow::ensure!(index.contains(script), fault(422,&format!("include <script type=module src=\"{script}\"></script> so the game maintains its session")));
            if let Some(size) = b.files.get(crate::card_art::PATH) {
                anyhow::ensure!(*size <= crate::card_art::MAX_BYTES as u64, fault(422, "card hero exceeds 250 KiB"));
                let bytes = std::fs::read(self.root().join(&key).join(crate::card_art::PATH))?;
                crate::card_art::validate(&bytes).map_err(|e| fault(422, &format!("{e:#}")))?;
            }
            crate::native::validate(&self.root().join(&key), &b.files).map_err(|e|fault(422,&format!("{e:#}")))?;
            crate::game_urls::assign(d, app, &b.manifest.title);
            for credential in d.credentials.values_mut().filter(|c|c.app==app && c.role=="publish" && c.label=="Existing game") { credential.label=b.manifest.title.clone(); }
            let changes_server = b.server.is_some() || d.apps.get(app).is_some_and(|a| d.builds[&build_key(app,&a.build)].server.is_some());
            let running = self.controller.snapshot().games.get(app).is_some_and(|i|i.reserved());
            if ((changes_server && running) || self.peers.active(app, now())) && !d.apps.get(app).is_some_and(|a|a.build==build) {
                d.builds.get_mut(&key).unwrap().ready = true;
                d.pending_promotions.insert(app.into(),platform::PendingPromotion { build:build.into(),listed,queued_at:now() });
                return Ok("queued");
            }
            if let Some(server) = b.server {
                let keys = self.config().controller.state_dir.join("server-keys");
                std::fs::create_dir_all(&keys)?;
                let path = keys.join(app);
                let token = if path.exists() { std::fs::read_to_string(&path)? } else {
                    use std::os::unix::fs::OpenOptionsExt;
                    use std::io::Write;
                    let token = secret();
                    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
                    f.write_all(token.as_bytes())?; f.sync_all()?; token
                };
                d.credentials.insert(format!("server-{app}"),platform::Credential {app:app.into(),role:"server".into(),label:"container admission".into(),hash:hash(&token),owner:None});
                self.controller.register(app,server)?;
            }
            d.builds.get_mut(&key).unwrap().ready = true;
            d.tags.entry(app.into()).or_insert(b.manifest.tags);
            d.apps.insert(app.into(),App {build:build.into(),updated:now(),listed});
            d.pending_promotions.remove(app);
            Ok("published")
        })
    }
    pub fn activate_pending(&self) -> Result<()> {
        let _guard = self.publishing.lock().unwrap();
        let pending = self.db.read()?.pending_promotions;
        for (app, promotion) in pending {
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
            match self.promote(&app, &promotion.build, promotion.listed) {
                Ok("published") => {
                    tracing::info!(app, build = promotion.build, "Activated queued game update")
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(app,error=%error,"Queued game update could not activate")
                }
            }
        }
        Ok(())
    }
}
async fn unpublish(State(p): State<Platform>, Path(app): Path<String>, h: HeaderMap) -> Answer {
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
            l.expires = 0;
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn remove_build(
    State(p): State<Platform>,
    Path((app, build)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(slug(&app) && slug(&build), fault(400, "invalid ID"));
    let _guard = p.publishing.lock().unwrap();
    p.db.edit(|d| {
        ensure!(
            !d.apps.get(&app).is_some_and(|a| a.build == build)
                && !d
                    .pending_promotions
                    .get(&app)
                    .is_some_and(|p| p.build == build)
                && !d
                    .launches
                    .values()
                    .any(|l| l.app == app && l.build == build && l.expires > now()),
            fault(409, "build is published, queued or in use")
        );
        ensure!(
            d.server_builds
                .get(&build_key(&app, &build))
                .is_none_or(|j| !j.active()),
            fault(409, "server packaging is still queued or running")
        );
        d.builds.remove(&build_key(&app, &build));
        Ok(())
    })?;
    let path = p.root().join(&app).join(&build);
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Exchange {
    token: String,
}
async fn exchange(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Exchange>) -> Answer {
    p.origin(&h, host(&h))?;
    let digest = hash(&body.token);
    p.db.edit(|d| {
        let policy = d.groups.clone();
        let l = d
            .launches
            .values_mut()
            .find(|l| {
                l.token_hash == digest
                    && l.host == host(&h)
                    && !l.exchanged
                    && l.grant_expires > now()
            })
            .ok_or_else(|| fault(401, "launch link expired or already used"))?;
        ensure!(
            d.members.get(&l.email).is_some_and(|m| m.enabled),
            fault(403, "membership disabled")
        );
        ensure!(
            policy.allows(&l.email, &l.app),
            fault(403, "Game membership changed.")
        );
        l.exchanged = true;
        l.seen = now();
        l.expires = now() + 300;
        Ok(())
    })?;
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
    let d = p.db.read()?;
    let b = &d.builds[&build_key(&l.app, &l.build)];
    let player = if l.guest {
        format!("guest-{id}")
    } else {
        hash(&l.email)
    };
    Ok(reply(
        json!({"app_id":l.app,"build_id":l.build,"library_url":format!("https://{}/library",p.domain()),"guest":l.guest,"player_id":player,"name":if l.guest {"Guest"}else{&d.members[&l.email].name},"player_ticket":game_token(&h),"manifest":b.manifest,"capabilities":{"sessions":crate::dedicated::capabilities(),"sdk":crate::sdk_contract::with_peerjs(p.relay.is_some(), p.config().controller.peerjs_port.is_some()),"runtime_sessions":runtime::capabilities(),"presence":presence::capabilities(),"launch_intents":crate::launch_intent::capabilities(),"cloud_checkpoints":crate::cloud::capabilities(p.config().controller.cloud_save_budget_mb)},"runtime":p.refresh(&id)?}),
    ))
}
async fn consume_intent(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    let (id, _) = p.game(&h)?;
    let intent = p.db.edit(|d| {
        let launch = d
            .launches
            .get_mut(&id)
            .filter(|l| l.expires > now())
            .ok_or_else(|| fault(410, "game session expired"))?;
        Ok(launch.intent.take())
    })?;
    Ok(reply(json!({"intent":intent})))
}
async fn player(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (_, launch) = p.game(&h)?;
    let data = p.db.read()?;
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    let mut response = (
        [
            ("Cache-Control", "no-store"),
            ("Referrer-Policy", "no-referrer"),
            ("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'"),
        ],
        Html(include_str!("../web/game-player.html")),
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
        data.apps
            .get(&launch.app)
            .is_some_and(|a| a.build == launch.build),
        fault(409, "a newer build is available; reopen the game")
    );
    p.peers.retain_launches(
        &launch.app,
        &presence::live_launches(&data, &launch.app, now()),
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
            hash(&launch.email)
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
            app: &launch.app,
            build: &launch.build,
            connection: &body.connection_id,
            max_players: multiplayer.max_players,
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
        &launch.app,
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
    p.peers.signal(&launch.app, &peer, &id, &body, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_leave(State(p): State<Platform>, Path(peer): Path<String>, h: HeaderMap) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers.leave(&launch.app, &peer, &id, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_ice(State(p): State<Platform>, Path(peer): Path<String>, h: HeaderMap) -> Answer {
    let (id, launch) = p.game(&h)?;
    p.origin(&h, host(&h))?;
    p.peers.poll(&launch.app, &peer, &id, 0, now())?;
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
    p.peers.health(&launch.app, &peer, &id, body, now())?;
    Ok(reply(json!({"ok":true})))
}
async fn peer_health_admin(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let data = p.db.read()?;
    let mut report = p.peers.diagnostics(now());
    if let Some(sessions) = report["sessions"].as_array_mut() {
        sessions.retain(|s| {
            s["app_id"]
                .as_str()
                .is_some_and(|app| data.groups.app(app) == group)
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
        .get(&l.app)
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
        &l.app,
        l.lease_id.as_ref().unwrap(),
        l.lease_token.as_ref().unwrap(),
        "get",
        now(),
    )?;
    let upstream = p.controller.engine.web_address(&l.app, i, &state.owner).await?;
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
    p.peers.leave_launch(&l.app, &id);
    if let (Some(lease), Some(token)) = (&l.lease_id, &l.lease_token) {
        let _ = p.controller.lease(&l.app, lease, token, "delete", now());
    }
    p.db.edit(|d| {
        if let Some(l) = d.launches.get_mut(&id) {
            l.expires = now();
            l.seen = 0;
        }
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn save_get(State(p): State<Platform>, Path(slot): Path<String>, h: HeaderMap) -> Answer {
    let (id, l) = p.game(&h)?;
    ensure!(slug(&slot), fault(400, "invalid save slot"));
    let owner = if l.guest {
        format!("guest-{id}")
    } else {
        hash(&l.email)
    };
    let (version, data) = p.db.save_read(&owner, &l.app, &slot)?;
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
        hash(&l.email)
    };
    let version =
        p.db.save_write(&owner, &l.app, &slot, body.version, &body.data)?;
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
        .find(|(_, l)| l.app == app && l.token_hash == digest && l.exchanged && l.expires > now())
        .ok_or_else(|| fault(401, "invalid game ticket"))?;
    data.require_app_group(&l.email, &l.app)?;
    ensure!(
        data.members.get(&l.email).is_some_and(|m| m.enabled),
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
        json!({"app_id":app,"player_id":if l.guest{format!("guest-{id}")}else{hash(&l.email)},"name":if l.guest{"Guest"}else{&data.members[&l.email].name},"guest":l.guest,"expires_at":l.expires}),
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
    let data = p.db.read()?;
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
    let file = p.root().join(&l.app).join(&l.build).join(selected);
    // Never forward a platform or game credential into a filesystem service.
    request.headers_mut().remove("cookie");
    request.headers_mut().remove("authorization");
    let response = ServeFile::new(file)
        .precompressed_br()
        .precompressed_gzip()
        .oneshot(request)
        .await
        .unwrap();
    let mut response = response.map(Body::new);
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
        if let Some(title) = title {
            crate::game_urls::assign(d, &app, title);
        }
        Ok(())
    })?;
    let code = format!("infuser_setup_{token}");
    Ok(reply(
        json!({"app_id":app,"api_url":format!("https://{}",p.domain()),"setup_code":code,"expires_at":expires,"supported_engines":crate::publisher::supported_engines(),"tag_catalog":crate::tags::catalog_in_group(&p.db.read()?,false,p.db.read()?.groups.member(&email))}),
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
            d.members.get(&claim.owner).is_some_and(|m| m.enabled),
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
            d.members.get(&owner).is_some_and(|m| m.enabled),
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
    Ok(reply(
        json!({"app_id":app,"api_url":format!("https://{}",p.domain()),"setup_code":code,"expires_at":expires,"supported_engines":crate::publisher::supported_engines(),"tag_catalog":crate::tags::catalog_in_group(&p.db.read()?,false,p.db.read()?.groups.member(&email))}),
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
    let d = p.db.read()?;
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
        let data = self.db.read()?;
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
            hash(&launch.email)
        };
        Ok((owner, launch, declaration))
    }
}
async fn cloud_load(State(p): State<Platform>, Path(group): Path<String>, h: HeaderMap) -> Answer {
    let (owner, launch, _) = p.cloud_group(&h, &group)?;
    Ok(reply(p.db.cloud_load(
        crate::cloud::Scope {
            owner: &owner,
            app: &launch.app,
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
            app: &launch.app,
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
        app: &launch.app,
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
            app: &launch.app,
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
