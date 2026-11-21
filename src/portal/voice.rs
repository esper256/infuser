//! Only this gateway is public. LiveKit's HTTP API must listen on loopback.
use super::*;
use crate::voice::{AudioSettingsPatch, LEASE_SECONDS, Lease, ROOM_PREFIX, Service};
use axum::extract::{
    OriginalUri,
    ws::{Message, WebSocketUpgrade},
};
use axum::routing::patch;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as Wire;

const PLAYER_VOICE_HEADER: &str = "x-infuser-player-voice";

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route(
            "/api/voice",
            get(status)
                .post(join)
                .delete(leave)
                .layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/voice/heartbeat",
            post(heartbeat).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/voice/ack",
            post(ack).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/voice/settings",
            patch(update_settings).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/api/voice/control", get(control))
        .route(
            "/_infuser/voice",
            get(game_status)
                .patch(game_update)
                .delete(game_leave)
                .layer(DefaultBodyLimit::max(4096)),
        )
        .route("/voice/rtc", get(signal))
        .route("/voice/rtc/validate", get(validate))
        .route(
            "/assets/voice.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(
                    &h,
                    "text/javascript",
                    include_bytes!("../../web/voice.js"),
                )
            }),
        )
        .route(
            "/assets/voice-audio.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(
                    &h,
                    "text/javascript",
                    include_bytes!("../../web/voice-audio.js"),
                )
            }),
        )
        .route(
            "/_infuser/voice-audio.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "no-cache"),
                    ],
                    include_str!("../../web/voice-audio.js"),
                )
            }),
        )
        .route(
            "/_infuser/player-voice.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "no-cache"),
                    ],
                    include_str!("../../web/game-voice.js"),
                )
            }),
        )
        .route(
            "/assets/vendor/livekit-2.24.1.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "public, max-age=31536000, immutable"),
                    ],
                    include_str!("../../web/vendor/livekit/livekit-client.esm.mjs"),
                )
            }),
        )
}
fn service(p: &Platform) -> Result<&Arc<Service>> {
    p.voice
        .as_ref()
        .ok_or_else(|| fault(503, "Voice chat is not configured."))
}
fn allowed(p: &Platform, l: &Lease, time: u64) -> bool {
    l.expires > time
        && p.db
            .voice_channel_allowed(&l.owner, &l.channel, l.visit.as_deref(), time)
            .unwrap_or(false)
}
fn authorized(p: &Platform, token: &str) -> Result<Lease> {
    let v = service(p)?;
    ensure!(v.healthy(now()), fault(503, "Voice chat is reconnecting."));
    let (id, room) = v
        .claims(token, now())
        .map_err(|_| fault(401, "Voice connection expired."))?;
    let l = v
        .state
        .lock()
        .unwrap()
        .leases
        .get(&id)
        .cloned()
        .ok_or_else(|| fault(403, "Voice chat is off."))?;
    ensure!(
        l.room == room && allowed(p, &l, now()),
        fault(403, "Voice access ended.")
    );
    Ok(l)
}
impl Platform {
    pub(super) fn voice_health(&self) -> Value {
        let Some(v) = &self.voice else {
            return json!({"configured":false});
        };
        let available = v.healthy(now());
        let state = v.state.lock().unwrap();
        json!({"configured":true,"available":available,"checked_at":state.checked_at,
            "leases":state.leases.len(),"connected":state.leases.values().filter(|l|l.connected && available).count()})
    }
    pub fn voice_active(&self) -> bool {
        self.voice.as_ref().is_some_and(|v| {
            v.state
                .lock()
                .unwrap()
                .leases
                .values()
                .any(|l| allowed(self, l, now()))
        })
    }
    pub fn voice_names(&self) -> Vec<String> {
        let Some(v) = &self.voice else {
            return Vec::new();
        };
        let owners: Vec<String> = {
            let state = v.state.lock().unwrap();
            state
                .leases
                .values()
                .filter(|l| allowed(self, l, now()))
                .map(|l| l.owner.clone())
                .collect()
        };
        let Ok(data) = self.db.read() else {
            return vec!["Someone".into()];
        };
        let mut names: Vec<String> = owners
            .into_iter()
            .map(|owner| {
                data.members
                    .get(&owner)
                    .map(|member| member.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Someone".into())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }
    pub async fn wait_for_voice_control_waiter(&self, owner: &str) {
        let Some(v) = &self.voice else { return };
        loop {
            let (count, notify) = v.control_waiters(owner);
            if count > 0 {
                return;
            }
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if v.control_waiters(owner).0 > 0 {
                return;
            }
            notified.await;
        }
    }
    /// Also cleans rooms left by a previous controller. No token/room survives a
    /// controller restart until reconciliation has removed orphan participants.
    pub async fn reconcile_voice(&self) -> Result<()> {
        let Some(v) = &self.voice else { return Ok(()) };
        let _gate = v.gate.lock().await;
        let result = self.reconcile_voice_locked(v).await;
        if result.is_err() {
            v.state.lock().unwrap().checked_at = 0;
        }
        result
    }
    async fn reconcile_voice_locked(&self, v: &Service) -> Result<()> {
        let time = now();
        v.state
            .lock()
            .unwrap()
            .leases
            .retain(|_, l| allowed(self, l, time));
        let rooms = v.call(&self.http, "ListRooms", None, json!({})).await?;
        let rooms = rooms["rooms"].as_array().cloned().unwrap_or_default();
        let mut connected = std::collections::BTreeSet::new();
        for room in rooms {
            let Some(room) = room["name"].as_str().filter(|s| s.starts_with(ROOM_PREFIX)) else {
                continue;
            };
            let leases: Vec<_> = v
                .state
                .lock()
                .unwrap()
                .leases
                .values()
                .filter(|l| l.room == room)
                .cloned()
                .collect();
            if leases.is_empty() {
                v.call(&self.http, "DeleteRoom", Some(room), json!({"room":room}))
                    .await?;
                continue;
            }
            let participants = v
                .call(
                    &self.http,
                    "ListParticipants",
                    Some(room),
                    json!({"room":room}),
                )
                .await?;
            for participant in participants["participants"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(id) = participant["identity"].as_str() else {
                    continue;
                };
                if !leases.iter().any(|l| l.id == id && allowed(self, l, now())) {
                    v.call(
                        &self.http,
                        "RemoveParticipant",
                        Some(room),
                        json!({"room":room,"identity":id}),
                    )
                    .await?;
                } else if participant["state"] == "ACTIVE" || participant["state"] == 2 {
                    connected.insert(id.to_owned());
                }
            }
        }
        let mut state = v.state.lock().unwrap();
        for l in state.leases.values_mut() {
            l.connected = connected.contains(&l.id);
        }
        state.checked_at = now();
        Ok(())
    }
}
async fn context(
    p: &Platform,
    h: &HeaderMap,
    write: bool,
) -> Result<(String, String, Option<String>)> {
    let owner = p.member(h, write).await?;
    let (group, inspecting) = p.group_view(h, &owner)?;
    ensure!(
        !inspecting,
        fault(403, "Voice chat is unavailable during inspection.")
    );
    let channel = p.db.default_chat_channel(&group)?;
    let visit = h
        .get(groups::VISIT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    ensure!(
        p.db.voice_channel_allowed(&owner, &channel, visit.as_deref(), now())?,
        fault(403, "Voice access ended.")
    );
    Ok((owner, channel, visit))
}
struct WaiterGuard {
    service: Arc<Service>,
    owner: String,
}
impl WaiterGuard {
    fn enter(service: Arc<Service>, owner: String) -> Self {
        service.enter_control_wait(&owner);
        Self { service, owner }
    }
}
impl Drop for WaiterGuard {
    fn drop(&mut self) {
        self.service.leave_control_wait(&self.owner);
    }
}

fn require_player_voice_token(v: &Service, h: &HeaderMap, launch_id: &str) -> Result<()> {
    let token = h
        .get(PLAYER_VOICE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    ensure!(
        v.verify_player_token(launch_id, token),
        fault(403, "Voice controls require the player header.")
    );
    Ok(())
}

fn self_summary(lease: Option<&Lease>, healthy: bool) -> Value {
    match lease {
        Some(l) => json!({
            "id": l.id,
            "channel": l.channel,
            "connected": l.connected && healthy,
            "mic_muted": l.mic_muted,
            "deafened": l.deafened,
            "applied_revision": l.applied_revision,
            "applied_mic_muted": l.applied_mic_muted,
            "applied_deafened": l.applied_deafened
        }),
        None => Value::Null,
    }
}

fn apply_lease_audio(lease: &mut Lease, patch: &AudioSettingsPatch) -> bool {
    let mut changed = false;
    if let Some(deafened) = patch.deafened
        && lease.deafened != deafened
    {
        lease.deafened = deafened;
        changed = true;
    }
    if let Some(muted) = patch.mic_muted {
        if lease.mic_muted != muted {
            lease.mic_muted = muted;
            changed = true;
        }
        // Unmuting the microphone while deafened also undeafens, matching Discord UX.
        if !muted && lease.deafened {
            lease.deafened = false;
            changed = true;
        }
    }
    changed
}

async fn status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (owner, channel, _) = context(&p, &h, false).await?;
    let Some(v) = &p.voice else {
        return Ok(reply(json!({"configured":false,"participants":[]})));
    };
    let data = p.db.catalog()?;
    let (persisted_rev, settings) = p.db.voice_settings_load(&owner)?;
    let (rev, _) = v.revision_for(&owner, persisted_rev);
    let healthy = v.healthy(now());
    let state = v.state.lock().unwrap();
    let people: Vec<_> = state
        .leases
        .values()
        .filter(|l| l.channel == channel && l.connected && healthy && allowed(&p, l, now()))
        .map(|l| {
            json!({
                "id": data.player_id(&l.owner),
                "participant_id": l.id,
                "name": data.members[&l.owner].name,
                "avatar": data.members[&l.owner].avatar
            })
        })
        .collect();
    let own = state
        .leases
        .values()
        .find(|l| l.owner == owner && allowed(&p, l, now()));
    Ok(reply(json!({
        "configured": true,
        "available": healthy,
        "channel": channel,
        "epoch": v.epoch,
        "participants": people,
        "self": self_summary(own, healthy),
        "settings": settings,
        "revision": rev
    })))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestId {
    id: String,
}
async fn join(State(p): State<Platform>, h: HeaderMap, Json(body): Json<RequestId>) -> Answer {
    let (owner, channel, visit) = context(&p, &h, true).await?;
    ensure!(
        (16..=64).contains(&body.id.len())
            && body
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        fault(400, "Invalid voice request.")
    );
    let v = service(&p)?;
    let _gate = v.gate.lock().await;
    ensure!(
        v.healthy(now()),
        fault(503, "Voice chat is temporarily unavailable.")
    );
    let existing = v
        .state
        .lock()
        .unwrap()
        .leases
        .values()
        .find(|l| l.owner == owner)
        .cloned();
    if let Some(existing) = existing {
        ensure!(
            existing.request_id == body.id
                && existing.channel == channel
                && existing.visit == visit
                && allowed(&p, &existing, now()),
            fault(
                409,
                "Voice chat is already on in another tab. Turn it off there or here first."
            )
        );
        return Ok(reply(
            json!({"id":existing.id,"epoch":v.epoch,"url":format!("wss://{}/voice",p.domain()),"token":v.participant_token(&existing,now())}),
        ));
    }
    ensure!(
        !v.state.lock().unwrap().leases.contains_key(&body.id),
        fault(409, "Voice request already in use.")
    );
    let (persisted_rev, _) = p.db.voice_settings_load(&owner)?;
    let initial_rev = v.bump_owner(&owner, persisted_rev);
    let lease = Lease {
        id: secret()[..32].into(),
        request_id: body.id,
        owner: owner.clone(),
        room: format!("{ROOM_PREFIX}{}-{channel}", v.epoch),
        channel,
        visit,
        expires: now() + LEASE_SECONDS,
        connected: false,
        mic_muted: false,
        deafened: false,
        applied_revision: initial_rev,
        applied_mic_muted: false,
        applied_deafened: false,
    };
    let token = v.participant_token(&lease, now());
    let id = lease.id.clone();
    v.state.lock().unwrap().leases.insert(id.clone(), lease);
    p.admit(&h).await;
    Ok(reply(
        json!({"id":id,"epoch":v.epoch,"revision":initial_rev,"url":format!("wss://{}/voice",p.domain()),"token":token}),
    ))
}
async fn heartbeat(State(p): State<Platform>, h: HeaderMap, Json(body): Json<RequestId>) -> Answer {
    let (owner, channel, visit) = context(&p, &h, true).await?;
    let v = service(&p)?;
    let _gate = v.gate.lock().await;
    ensure!(v.healthy(now()), fault(503, "Voice chat is reconnecting."));
    {
        let mut state = v.state.lock().unwrap();
        let l = state
            .leases
            .get_mut(&body.id)
            .ok_or_else(|| fault(403, "Voice chat is off."))?;
        ensure!(
            l.owner == owner && l.channel == channel && l.visit == visit && allowed(&p, l, now()),
            fault(403, "Voice access ended.")
        );
        l.expires = now() + LEASE_SECONDS;
    }
    p.admit(&h).await;
    p.db.see_member(&owner, now())?;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckBody {
    id: String,
    revision: u64,
    mic_muted: bool,
    deafened: bool,
}
async fn ack(State(p): State<Platform>, h: HeaderMap, Json(body): Json<AckBody>) -> Answer {
    let (owner, _, _) = context(&p, &h, true).await?;
    let v = service(&p)?;
    let mut state = v.state.lock().unwrap();
    let l = state
        .leases
        .get_mut(&body.id)
        .ok_or_else(|| fault(403, "Voice chat is off."))?;
    ensure!(
        l.owner == owner && allowed(&p, l, now()),
        fault(403, "Voice access ended.")
    );
    if body.revision >= l.applied_revision {
        l.applied_revision = body.revision;
        l.applied_mic_muted = body.mic_muted;
        l.applied_deafened = body.deafened;
    }
    Ok(reply(json!({"ok":true})))
}
async fn remove_owner_lease(p: &Platform, v: &Service, owner: &str, filter_id: Option<&str>) {
    let _gate = v.gate.lock().await;
    let removed = {
        let mut state = v.state.lock().unwrap();
        let key = state
            .leases
            .values()
            .find(|l| {
                l.owner == owner && filter_id.is_none_or(|id| l.id == id || l.request_id == id)
            })
            .map(|l| l.id.clone());
        key.and_then(|id| state.leases.remove(&id))
    };
    if let Some(l) = removed {
        v.bump_owner(owner, 0);
        // Reconnect is already denied. A failed control call is retried by reconciliation.
        if v.call(
            &p.http,
            "RemoveParticipant",
            Some(&l.room),
            json!({"room":l.room,"identity":l.id}),
        )
        .await
        .is_err()
        {
            v.state.lock().unwrap().checked_at = 0;
            tracing::warn!("voice removal pending; admission paused until reconciliation");
        }
    }
}
async fn leave(State(p): State<Platform>, h: HeaderMap, Json(body): Json<RequestId>) -> Answer {
    // Leaving must work after a visit ends. Check identity+origin, not stale visit context.
    let mut home = h.clone();
    home.remove(groups::VISIT);
    let owner = p.member(&home, true).await?;
    let v = service(&p)?;
    remove_owner_lease(&p, v, &owner, Some(&body.id)).await;
    Ok(reply(json!({"ok":true})))
}
async fn update_settings(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<AudioSettingsPatch>,
) -> Answer {
    let (owner, _, _) = context(&p, &h, true).await?;
    let v = service(&p)?;
    let (persisted_rev, settings, durable_changed) =
        p.db.voice_settings_patch(&owner, &body, true)
            .map_err(|e| fault(422, &e.to_string()))?;
    let healthy = v.healthy(now());
    let (lease_changed, own_summary) = {
        let mut state = v.state.lock().unwrap();
        let own = state
            .leases
            .values_mut()
            .find(|l| l.owner == owner && allowed(&p, l, now()));
        if let Some(l) = own {
            let changed = apply_lease_audio(l, &body);
            (changed, self_summary(Some(l), healthy))
        } else {
            (false, Value::Null)
        }
    };
    let rev = if durable_changed || lease_changed {
        v.bump_owner(&owner, persisted_rev)
    } else {
        v.revision_for(&owner, persisted_rev).0
    };
    Ok(reply(json!({
        "epoch": v.epoch,
        "revision": rev,
        "settings": settings,
        "self": own_summary
    })))
}
#[derive(Deserialize)]
struct ControlQuery {
    #[serde(default)]
    since: u64,
    #[serde(default)]
    epoch: Option<String>,
}
async fn control(State(p): State<Platform>, h: HeaderMap, Query(q): Query<ControlQuery>) -> Answer {
    let (owner, _, _) = context(&p, &h, false).await?;
    let v = service(&p)?;
    let epoch_matches = q.epoch.as_deref() == Some(v.epoch.as_str());
    let (persisted_rev, _) = p.db.voice_settings_load(&owner)?;
    let (rev, notify) = v.revision_for(&owner, persisted_rev);
    if epoch_matches && rev <= q.since {
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let _guard = WaiterGuard::enter(v.clone(), owner.clone());
        let (recheck_rev, _) = v.revision_for(&owner, persisted_rev);
        if recheck_rev <= q.since {
            let _ = tokio::time::timeout(Duration::from_secs(25), notified).await;
        }
    }
    let (persisted_rev, settings) = p.db.voice_settings_load(&owner)?;
    let (rev, _) = v.revision_for(&owner, persisted_rev);
    if epoch_matches && rev <= q.since {
        return Ok((StatusCode::NO_CONTENT, [("Cache-Control", "no-store")]).into_response());
    }
    let healthy = v.healthy(now());
    let own_summary = {
        let state = v.state.lock().unwrap();
        let own = state
            .leases
            .values()
            .find(|l| l.owner == owner && allowed(&p, l, now()));
        self_summary(own, healthy)
    };
    Ok(reply(json!({
        "epoch": v.epoch,
        "revision": rev,
        "settings": settings,
        "self": own_summary
    })))
}
async fn game_status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    if h.contains_key("origin") {
        p.origin(&h, host(&h))?;
    }
    ensure!(
        h.get("sec-fetch-site").is_none_or(|v| v == "same-origin"),
        fault(403, "read voice state from the game origin")
    );
    let (launch_id, launch) = p.game(&h)?;
    if launch.guest {
        return Ok(reply(json!({"configured":false,"active":false})));
    }
    let Some(v) = &p.voice else {
        return Ok(reply(json!({"configured":false,"active":false})));
    };
    require_player_voice_token(v, &h, &launch_id)?;
    let healthy = v.healthy(now());
    let data = p.db.catalog()?;
    let (own, people) = {
        let state = v.state.lock().unwrap();
        let own = state
            .leases
            .values()
            .find(|l| l.owner == launch.email && allowed(&p, l, now()))
            .cloned();
        let people = match &own {
            Some(own) => state
                .leases
                .values()
                .filter(|l| {
                    l.channel == own.channel && l.connected && healthy && allowed(&p, l, now())
                })
                .map(|l| {
                    json!({
                        "id": data.player_id(&l.owner),
                        "name": data.members[&l.owner].name,
                        "avatar": data.members[&l.owner].avatar
                    })
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        (own, people)
    };
    let Some(own) = own else {
        return Ok(reply(
            json!({"configured":true,"available":healthy,"active":false,"epoch":v.epoch}),
        ));
    };
    let (persisted_rev, settings) = p.db.voice_settings_load(&launch.email)?;
    let (rev, _) = v.revision_for(&launch.email, persisted_rev);
    Ok(reply(json!({
        "configured": true,
        "available": healthy,
        "active": true,
        "epoch": v.epoch,
        "connected": own.connected && healthy,
        "self_id": data.player_id(&launch.email),
        "mic_muted": own.mic_muted,
        "deafened": own.deafened,
        "applied_revision": own.applied_revision,
        "applied_mic_muted": own.applied_mic_muted,
        "applied_deafened": own.applied_deafened,
        "revision": rev,
        "input_gain": settings.input_gain,
        "output_volume": settings.output_volume,
        "peer_volumes": settings.peer_volumes,
        "participants": people
    })))
}
async fn game_update(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<AudioSettingsPatch>,
) -> Answer {
    p.origin(&h, host(&h))?;
    let (launch_id, launch) = p.game(&h)?;
    ensure!(
        !launch.guest,
        fault(403, "Voice controls are unavailable in guest mode.")
    );
    let v = service(&p)?;
    require_player_voice_token(v, &h, &launch_id)?;
    // Game-origin routes can adjust mix/mute/deafen only while an active voice session exists.
    {
        let state = v.state.lock().unwrap();
        ensure!(
            state
                .leases
                .values()
                .any(|l| l.owner == launch.email && allowed(&p, l, now())),
            fault(409, "Voice chat is off.")
        );
    }
    let (persisted_rev, settings, durable_changed) =
        p.db.voice_settings_patch(&launch.email, &body, false)
            .map_err(|e| fault(422, &e.to_string()))?;
    let (lease_changed, mic_muted, deafened, applied_revision, applied_mic_muted, applied_deafened) = {
        let mut state = v.state.lock().unwrap();
        let l = state
            .leases
            .values_mut()
            .find(|l| l.owner == launch.email && allowed(&p, l, now()))
            .ok_or_else(|| fault(409, "Voice chat is off."))?;
        let changed = apply_lease_audio(l, &body);
        (
            changed,
            l.mic_muted,
            l.deafened,
            l.applied_revision,
            l.applied_mic_muted,
            l.applied_deafened,
        )
    };
    let rev = if durable_changed || lease_changed {
        v.bump_owner(&launch.email, persisted_rev)
    } else {
        v.revision_for(&launch.email, persisted_rev).0
    };
    Ok(reply(json!({
        "epoch": v.epoch,
        "revision": rev,
        "applied_revision": applied_revision,
        "applied_mic_muted": applied_mic_muted,
        "applied_deafened": applied_deafened,
        "input_gain": settings.input_gain,
        "output_volume": settings.output_volume,
        "peer_volumes": settings.peer_volumes,
        "mic_muted": mic_muted,
        "deafened": deafened
    })))
}
async fn game_leave(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    let (launch_id, launch) = p.game(&h)?;
    ensure!(
        !launch.guest,
        fault(403, "Voice controls are unavailable in guest mode.")
    );
    let v = service(&p)?;
    require_player_voice_token(v, &h, &launch_id)?;
    remove_owner_lease(&p, v, &launch.email, None).await;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
struct SignalQuery {
    access_token: String,
}
fn upstream(
    p: &Platform,
    h: &HeaderMap,
    uri: &axum::http::Uri,
    q: &SignalQuery,
) -> Result<(Lease, String)> {
    p.main_host(h)?;
    // SDK's HTTP validation fetch may omit Origin. WebSocket admission below requires it.
    if h.contains_key("origin") {
        p.origin(h, p.domain())?;
    }
    let lease = authorized(p, &q.access_token)?;
    ensure!(
        uri.query().is_none_or(|s| s.len() <= 12288),
        fault(400, "Invalid voice connection.")
    );
    let mut url = reqwest::Url::parse(&format!("http://127.0.0.1:{}/rtc", service(p)?.port))?;
    for (key, value) in reqwest::Url::parse(&format!("http://localhost{uri}"))?.query_pairs() {
        // Client-selected room/identity/permissions and arbitrary upstream paths are forbidden.
        ensure!(
            matches!(
                key.as_ref(),
                "access_token"
                    | "auto_subscribe"
                    | "adaptive_stream"
                    | "sdk"
                    | "version"
                    | "protocol"
                    | "client_protocol"
                    | "reconnect"
                    | "sid"
                    | "os"
                    | "os_version"
                    | "device_model"
                    | "browser"
                    | "browser_version"
                    | "reconnect_reason"
                    | "network"
                    | "subscriber_allow_pause"
                    | "disable_ice_lite"
            ),
            fault(400, "Unsupported voice connection option.")
        );
        ensure!(
            url.query_pairs().all(|(k, _)| k != key),
            fault(400, "Duplicate voice connection option.")
        );
        url.query_pairs_mut().append_pair(&key, &value);
    }
    Ok((lease, url.into()))
}
async fn validate(
    State(p): State<Platform>,
    h: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(q): Query<SignalQuery>,
) -> Answer {
    upstream(&p, &h, &uri, &q)?;
    Ok(([("cache-control", "no-store")], "ok").into_response())
}
async fn signal(
    State(p): State<Platform>,
    h: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(q): Query<SignalQuery>,
    ws: WebSocketUpgrade,
) -> Answer {
    p.origin(&h, p.domain())?;
    let (lease, url) = upstream(&p, &h, &uri, &q)?;
    let mut url = reqwest::Url::parse(&url).map_err(anyhow::Error::from)?;
    url.set_scheme("ws").unwrap();
    let upstream = tokio::time::timeout(
        Duration::from_secs(4),
        tokio_tungstenite::connect_async(url.as_str()),
    )
    .await
    .map_err(|_| fault(503, "Voice server unavailable."))?
    .map_err(|_| fault(503, "Voice server unavailable."))?
    .0;
    // Recheck after awaiting the sidecar: a concurrent leave may have revoked admission.
    authorized(&p, &q.access_token)?;
    Ok(ws.max_message_size(65536).max_frame_size(65536).on_upgrade(move |mut client|async move {
        let mut upstream=upstream;
        let mut tick=tokio::time::interval(Duration::from_secs(2));
        let mut window=now();let mut messages=0;
        loop {
            let sent=tokio::select! {
                _=tick.tick()=>{
                    if !service(&p).is_ok_and(|v|v.healthy(now()) && v.state.lock().unwrap().leases.get(&lease.id).is_some_and(|l|allowed(&p,l,now()))){break;}
                    continue;
                }
                frame=client.next()=>{
                    if now()!=window {window=now();messages=0;}messages+=1;if messages>120 {break;}
                    let wire=match frame {Some(Ok(Message::Binary(b)))=>Wire::Binary(b),Some(Ok(Message::Text(t)))=>Wire::Text(t.to_string().into()),Some(Ok(Message::Ping(b)))=>{let _=client.send(Message::Pong(b)).await;continue},Some(Ok(Message::Pong(_)))=>continue,_=>break};
                    tokio::time::timeout(Duration::from_secs(3),upstream.send(wire)).await.is_ok_and(|r|r.is_ok())
                }
                frame=upstream.next()=>{
                    let message=match frame {Some(Ok(Wire::Binary(b))) if b.len()<=65536=>Message::Binary(b),Some(Ok(Wire::Text(t))) if t.len()<=65536=>Message::Text(t.to_string().into()),Some(Ok(Wire::Ping(b)))=>{let _=upstream.send(Wire::Pong(b)).await;continue},Some(Ok(Wire::Pong(_)))=>continue,_=>break};
                    tokio::time::timeout(Duration::from_secs(3),client.send(message)).await.is_ok_and(|r|r.is_ok())
                }
            };
            if !sent {break;}
        }
        // Dropping signaling is not revocation: media uses separate UDP/TCP.
        // The lease reconciler removes unauthorized participants through RoomService.
        let _=tokio::time::timeout(Duration::from_secs(1),client.close()).await;
        let _=tokio::time::timeout(Duration::from_secs(1),upstream.close(None)).await;
    }).into_response())
}
