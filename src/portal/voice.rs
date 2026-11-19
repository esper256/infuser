//! Only this gateway is public. LiveKit's HTTP API must listen on loopback.
use super::*;
use crate::voice::{LEASE_SECONDS, Lease, ROOM_PREFIX, Service};
use axum::extract::{
    OriginalUri,
    ws::{Message, WebSocketUpgrade},
};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as Wire;

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route("/api/voice", get(status).post(join).delete(leave))
        .route("/api/voice/heartbeat", post(heartbeat))
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
        .layer(DefaultBodyLimit::max(1024))
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
async fn status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let (owner, channel, _) = context(&p, &h, false).await?;
    let Some(v) = &p.voice else {
        return Ok(reply(json!({"configured":false,"participants":[]})));
    };
    let data = p.db.catalog()?;
    let healthy = v.healthy(now());
    let state = v.state.lock().unwrap();
    let people:Vec<_>=state.leases.values().filter(|l|l.channel==channel && l.connected && healthy && allowed(&p,l,now()))
        .map(|l|json!({"id":data.player_id(&l.owner),"name":data.members[&l.owner].name,"avatar":data.members[&l.owner].avatar})).collect();
    let own = state
        .leases
        .values()
        .find(|l| l.owner == owner && allowed(&p, l, now()));
    Ok(reply(
        json!({"configured":true,"available":healthy,"channel":channel,"participants":people,
        "self":own.map(|l|json!({"id":l.id,"channel":l.channel,"connected":l.connected&&healthy}))}),
    ))
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
            json!({"id":existing.id,"url":format!("wss://{}/voice",p.domain()),"token":v.participant_token(&existing,now())}),
        ));
    }
    ensure!(
        !v.state.lock().unwrap().leases.contains_key(&body.id),
        fault(409, "Voice request already in use.")
    );
    let lease = Lease {
        id: secret()[..32].into(),
        request_id: body.id,
        owner,
        room: format!("{ROOM_PREFIX}{}-{channel}", v.epoch),
        channel,
        visit,
        expires: now() + LEASE_SECONDS,
        connected: false,
    };
    let token = v.participant_token(&lease, now());
    let id = lease.id.clone();
    v.state.lock().unwrap().leases.insert(id.clone(), lease);
    p.admit(&h).await;
    Ok(reply(
        json!({"id":id,"url":format!("wss://{}/voice",p.domain()),"token":token}),
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
async fn leave(State(p): State<Platform>, h: HeaderMap, Json(body): Json<RequestId>) -> Answer {
    // Leaving must work after a visit ends. Check identity+origin, not stale visit context.
    let mut home = h.clone();
    home.remove(groups::VISIT);
    let owner = p.member(&home, true).await?;
    let v = service(&p)?;
    let _gate = v.gate.lock().await;
    let removed = {
        let mut state = v.state.lock().unwrap();
        let key = state
            .leases
            .values()
            .find(|l| l.owner == owner && (l.id == body.id || l.request_id == body.id))
            .map(|l| l.id.clone());
        key.and_then(|id| state.leases.remove(&id))
    };
    if let Some(l) = removed {
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
