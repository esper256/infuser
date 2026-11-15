use super::*;
use crate::native::{Device, Pair, Play};
use infuser_native_protocol::{MANIFEST, Manifest};

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route(
            "/desktop",
            get(|| async { Html(include_str!("../../web/desktop.html")) }),
        )
        .route(
            "/assets/desktop.js",
            get(|| async {
                (
                    [("content-type", "text/javascript")],
                    include_str!("../../web/desktop.js"),
                )
            }),
        )
        .route("/api/native/v1/device/start", post(start))
        .route("/api/native/v1/device/token", post(token))
        .route("/api/native/v1/device/approve", post(approve))
        .route("/api/native/v1/device/revoke", post(self_revoke))
        .route("/api/native/v1/devices", get(devices))
        .route("/api/native/v1/devices/{id}", delete(revoke))
        .route("/api/native/v1/play/{app}", post(play))
        .route("/api/native/v1/claim", post(claim))
        .layer(DefaultBodyLimit::max(4096))
}
pub(super) fn public(path: &str) -> bool {
    matches!(
        path,
        "/api/native/v1/device/start"
            | "/api/native/v1/device/token"
            | "/api/native/v1/device/revoke"
            | "/api/native/v1/claim"
    )
}
fn external(p: &Platform, h: &HeaderMap) -> Result<()> {
    p.main_host(h)?;
    if h.contains_key("origin") {
        p.origin(h, p.domain())?;
    }
    Ok(())
}
pub(super) fn bundle(p: &Platform, app: &str, build: &str, b: &Build) -> Result<Option<Manifest>> {
    if !b.files.contains_key(MANIFEST) {
        return Ok(None);
    }
    crate::native::read(&p.root().join(app).join(build))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    challenge: String,
}
async fn start(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Start>) -> Answer {
    external(&p, &h)?;
    ensure!(
        infuser_native_protocol::digest(&body.challenge),
        fault(400, "invalid device proof challenge")
    );
    let id = secret();
    p.db.native_edit(|_, n| {
        ensure!(
            n.pairs.len() < 128,
            fault(429, "too many pending device connections")
        );
        n.pairs.insert(
            id.clone(),
            Pair {
                challenge: body.challenge,
                expires: now() + 300,
                email: None,
            },
        );
        Ok(())
    })?;
    Ok(reply(
        json!({"request_id":id,"verification_uri":format!("https://{}/desktop?pair={id}",p.domain()),"expires_in":300}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    request_id: String,
}
async fn approve(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Approval>) -> Answer {
    let email = p.member(&h, true).await?;
    p.db.native_edit(|_, n| {
        let pair = n
            .pairs
            .get_mut(&body.request_id)
            .ok_or_else(|| fault(410, "device connection expired; reopen Infuser Play"))?;
        ensure!(
            pair.email.is_none(),
            fault(409, "device connection already approved")
        );
        pair.email = Some(email);
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Token {
    request_id: String,
    verifier: String,
}
async fn token(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Token>) -> Answer {
    external(&p, &h)?;
    ensure!(
        infuser_native_protocol::digest(&body.verifier),
        fault(400, "invalid device proof")
    );
    let result = p.db.native_edit(|d, n| {
        let pair = n.pairs.get(&body.request_id).ok_or_else(|| fault(410, "device connection expired"))?;
        ensure!(pair.challenge == hash(&body.verifier), fault(403, "device proof does not match"));
        let Some(email) = pair.email.clone() else { return Ok(json!({"status":"pending"})); };
        ensure!(d.members.get(&email).is_some_and(|m| m.enabled), fault(403, "membership disabled"));
        ensure!(n.devices.values().filter(|x| x.email == email).count() < 16, fault(429, "remove an old computer before connecting another"));
        let key = secret();
        n.devices.insert(hash(&key), Device { email: email.clone(), created: now() });
        n.pairs.remove(&body.request_id);
        Ok(json!({"status":"connected","token":key,"player_id":hash(&email),"name":d.members[&email].name}))
    })?;
    Ok(reply(result))
}
async fn devices(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    Ok(reply(p.db.native_edit(|_, n| Ok(json!({"devices":n.devices.iter().filter(|(_,x)|x.email==email).map(|(id,x)|json!({"id":id,"created":x.created})).collect::<Vec<_>>()})))?))
}
async fn self_revoke(State(p): State<Platform>, h: HeaderMap) -> Answer {
    external(&p, &h)?;
    let key = hash(bearer(&h).ok_or_else(|| fault(401, "computer credential required"))?);
    p.db.native_edit(|_, n| {
        n.devices.remove(&key);
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn revoke(State(p): State<Platform>, Path(id): Path<String>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    p.db.native_edit(|_, n| {
        ensure!(
            n.devices.get(&id).is_some_and(|x| x.email == email),
            fault(404, "computer not found")
        );
        n.devices.remove(&id);
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayRequest {
    platform: String,
    #[serde(default)]
    guest: bool,
}
async fn play(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<PlayRequest>,
) -> Answer {
    let email = p.member(&h, true).await?;
    p.admit(&h).await;
    let id = secret();
    p.db.native_edit(|d, n| {
        d.require_app_group(&email, &app)?;
        let active = d
            .apps
            .get(&app)
            .ok_or_else(|| fault(404, "game not found"))?;
        ensure!(
            active.listed || d.members[&email].admin,
            fault(404, "game not found")
        );
        let b = &d.builds[&build_key(&app, &active.build)];
        ensure!(
            b.manifest.multiplayer.is_none()
                && matches!(
                    b.manifest.solo_runtime,
                    Some(crate::manifest::SoloRuntime::ClientOnly)
                ),
            fault(
                422,
                "desktop preview currently supports client-only single-player games"
            )
        );
        let manifest = bundle(&p, &app, &active.build, b)?
            .ok_or_else(|| fault(422, "this game has no desktop build"))?;
        ensure!(
            manifest.platforms.contains_key(&body.platform),
            fault(422, "this game does not support your computer yet")
        );
        ensure!(
            n.requests.values().filter(|r| r.email == email).count() < 16,
            fault(429, "too many pending game launches")
        );
        n.requests.insert(
            hash(&id),
            Play {
                email,
                app,
                build: active.build.clone(),
                platform: body.platform,
                guest: body.guest,
                expires: now() + 600,
            },
        );
        Ok(())
    })?;
    Ok(reply(
        json!({"url":format!("infuser://play?server=https%3A%2F%2F{}&request={id}",p.domain()),"expires_in":600}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    request_id: String,
    platform: String,
}
async fn claim(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Claim>) -> Answer {
    external(&p, &h)?;
    let key = hash(bearer(&h).ok_or_else(|| fault(401, "connect this computer first"))?);
    let result = p.db.native_edit(|d, n| {
        let device = n.devices.get(&key).ok_or_else(|| fault(401, "connect this computer again"))?;
        ensure!(d.members.get(&device.email).is_some_and(|m| m.enabled), fault(403, "membership disabled"));
        let request = n.requests.get(&hash(&body.request_id)).ok_or_else(|| fault(410, "press Play again in the Library"))?.clone();
        ensure!(request.email == device.email, fault(403, "this computer is connected to a different player; switch account in Infuser"));
        ensure!(request.platform == body.platform, fault(409, "open this game on the computer that requested it"));
        d.require_app_group(&device.email,&request.app)?;
        let active = d.apps.get(&request.app).ok_or_else(|| fault(410, "game was removed"))?;
        ensure!(active.listed || d.members[&device.email].admin, fault(404, "game not found"));
        ensure!(active.build == request.build, fault(409, "game was updated; press Play again"));
        let b = &d.builds[&build_key(&request.app, &request.build)];
        let manifest = bundle(&p, &request.app, &request.build, b)?.ok_or_else(|| fault(422, "desktop build unavailable"))?;
        let artifact = manifest.platforms.get(&body.platform).ok_or_else(|| fault(422, "desktop platform unavailable"))?;
        ensure!(d.launches.values().filter(|l| l.email == device.email && l.expires > now()).count() < 32, fault(429, "close an old game first"));
        let launch_id = secret()[..32].to_string();
        let game_token = secret();
        let game_host = crate::game_urls::host(d, &request.app, &launch_id, p.domain());
        let player = if request.guest { format!("guest-{launch_id}") } else { hash(&device.email) };
        let response = json!({"version":1,"app_id":request.app,"build_id":request.build,"title":b.manifest.title,"player_id":player,"guest":request.guest,"launch_id":launch_id,"game_origin":format!("https://{game_host}"),"token":game_token,"artifact":artifact});
        d.launches.insert(launch_id, Launch { intent: None, app: request.app, build: request.build, email: device.email.clone(), guest: request.guest, host: game_host, token_hash: hash(&game_token), exchanged: true, expires: now()+300, grant_expires: 0, seen: now(), lease_id: None, lease_token: None });
        n.requests.remove(&hash(&body.request_id));
        Ok(response)
    })?;
    Ok(reply(result))
}
