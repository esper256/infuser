//! Unadvertised transport foundation. Room admission is a separate next step.
use super::*;
use crate::connections::{
    Attempt, CAPABILITY, Identity, MAX_MESSAGE_BYTES, PROOF_SECONDS, State as ConnectionState,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use tokio::time::{Duration, Instant, timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as Upstream, client::IntoClientRequest},
};

type Backend = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn prune(p: &Platform) -> Result<()> {
    let live =
        p.db.read()?
            .launches
            .into_iter()
            .filter(|(_, l)| l.expires > now())
            .map(|(id, _)| id)
            .collect();
    p.connections.lock().unwrap().prune(now(), &live);
    Ok(())
}

pub(super) async fn options(State(p): State<Platform>, h: HeaderMap) -> Answer {
    allowed(&p, &h)?;
    Ok(reply(
        json!({"epoch":p.connection_epoch,"max_message_bytes":MAX_MESSAGE_BYTES}),
    ))
}

fn allowed(p: &Platform, h: &HeaderMap) -> Result<(String, Launch)> {
    let (id, launch) = p.game(h)?;
    let data = p.db.read()?;
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    ensure!(
        build.server.as_ref().is_some_and(|server| server.web)
            && build
                .manifest
                .sdk
                .as_ref()
                .is_some_and(|sdk| sdk.required_capabilities.iter().any(|c| c == CAPABILITY)),
        fault(
            422,
            "this build does not declare managed WebSocket connections"
        )
    );
    Ok((id, launch))
}

fn owned(p: &Platform, h: &HeaderMap, id: &str) -> Result<(Launch, Attempt)> {
    let (launch_id, launch) = allowed(p, h)?;
    prune(p)?;
    let attempts = p.connections.lock().unwrap();
    let attempt = attempts
        .0
        .get(id)
        .filter(|a| a.identity.launch_id == launch_id)
        .ok_or_else(|| fault(404, "unknown connection attempt"))?;
    Ok((launch, attempt.clone()))
}

// Call with publishing held, so release/promotion cannot change the lease while
// we validate and renew it. Authentication is rechecked, not cached from upgrade.
fn validate(p: &Platform, h: &HeaderMap, attempt: &Attempt) -> Result<()> {
    let (id, launch) = allowed(p, h)?;
    ensure!(
        id == attempt.identity.launch_id
            && launch.lease_id.as_deref() == Some(attempt.lease_id.as_str()),
        fault(410, "connection lease ended")
    );
    let data = p.db.read()?;
    ensure!(
        launch.app == attempt.identity.app_id
            && launch.build == attempt.identity.build_id
            && data.groups.app(&launch.app) == attempt.identity.group_id
            && data
                .apps
                .get(&launch.app)
                .is_some_and(|a| a.build == launch.build),
        fault(409, "connection build changed")
    );
    let state = p.controller.snapshot();
    let instance = state
        .games
        .get(&launch.app)
        .ok_or_else(|| fault(410, "server ended"))?;
    ensure!(
        instance.attempt == attempt.identity.instance_id
            && instance.phase == crate::store::Phase::Ready,
        fault(410, "server instance ended")
    );
    p.controller.lease(
        &launch.app,
        &attempt.lease_id,
        launch
            .lease_token
            .as_deref()
            .ok_or_else(|| fault(410, "lease ended"))?,
        "get",
        now(),
    )?;
    Ok(())
}

fn view(attempt: &Attempt) -> Value {
    json!({"attempt_id":attempt.identity.connection_id,"state":attempt.state,
        "error":attempt.error.map(|code| json!({"code":code})),"expires_at":attempt.identity.exp})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prepare {}

pub(super) async fn prepare(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(_): Json<Prepare>,
) -> Answer {
    p.origin(&h, host(&h))?;
    ensure!(
        id.len() == 48
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        fault(
            400,
            "attempt ID must be epoch plus 32 lowercase hex characters"
        )
    );
    ensure!(
        id.starts_with(&p.connection_epoch),
        fault(
            409,
            "connection coordinator restarted; request a new attempt"
        )
    );
    let _guard = p.publishing.lock().unwrap();
    let (launch_id, launch) = allowed(&p, &h)?;
    {
        prune(&p)?;
        let attempts = p.connections.lock().unwrap();
        if let Some(existing) = attempts.0.get(&id) {
            ensure!(
                existing.identity.launch_id == launch_id,
                fault(404, "unknown connection attempt")
            );
            return Ok(reply(view(existing)));
        }
    }
    let state = p.controller.snapshot();
    let instance = state
        .games
        .get(&launch.app)
        .ok_or_else(|| fault(409, "acquire the game backend first"))?;
    let data = p.db.read()?;
    let attempt = Attempt {
        identity: Identity {
            iss: format!("https://{}", p.domain()),
            aud: "infuser-server-connection".into(),
            iat: now(),
            exp: now() + PROOF_SECONDS,
            group_id: data.groups.app(&launch.app).into(),
            app_id: launch.app.clone(),
            build_id: launch.build.clone(),
            instance_id: instance.attempt.clone(),
            launch_id: launch_id.clone(),
            connection_id: id.clone(),
            player_id: if launch.guest {
                format!("guest-{launch_id}")
            } else {
                hash(&launch.email)
            },
            name: if launch.guest {
                "Guest".into()
            } else {
                data.members[&launch.email].name.clone()
            },
            guest: launch.guest,
        },
        lease_id: launch
            .lease_id
            .clone()
            .ok_or_else(|| fault(409, "acquire the game backend first"))?,
        state: ConnectionState::Pending,
        error: None,
        updated: now(),
    };
    validate(&p, &h, &attempt)?;
    let response = reply(view(&attempt));
    p.connections
        .lock()
        .unwrap()
        .insert(id, attempt)
        .map_err(|_| fault(429, "connection capacity exhausted"))?;
    Ok(response)
}

pub(super) async fn status(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Answer {
    Ok(reply(view(&owned(&p, &h, &id)?.1)))
}

pub(super) async fn cancel(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.origin(&h, host(&h))?;
    let _guard = p.publishing.lock().unwrap();
    owned(&p, &h, &id)?;
    let mut attempts = p.connections.lock().unwrap();
    attempts.finish(&id, ConnectionState::Cancelled, Some("cancelled"), now());
    Ok(reply(view(&attempts.0[&id])))
}

// Drop runs on failed upgrades, task cancellation and all forwarding error paths.
struct ConnectionGuard {
    p: Platform,
    id: String,
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut attempts = self.p.connections.lock().unwrap();
        attempts.finish(&self.id, ConnectionState::Closed, None, now());
        if let Some(attempt) = attempts.0.get(&self.id) {
            tracing::info!(app_id = %attempt.identity.app_id, build_id = %attempt.identity.build_id,
                instance_id = %attempt.identity.instance_id, connection_id = %self.id,
                state = ?attempt.state, reason = ?attempt.error, "managed connection ended");
        }
    }
}
impl ConnectionGuard {
    fn fail(&self, code: &'static str) {
        self.p.connections.lock().unwrap().finish(
            &self.id,
            ConnectionState::Failed,
            Some(code),
            now(),
        );
    }
}

pub(super) async fn socket(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
    ws: WebSocketUpgrade,
) -> Answer {
    p.origin(&h, host(&h))?;
    // No subprotocol negotiation yet; do not silently mis-negotiate a game's protocol.
    ensure!(
        !h.contains_key("sec-websocket-protocol"),
        fault(
            422,
            "subprotocol negotiation is not supported by this preview"
        )
    );
    let attempt = {
        let _guard = p.publishing.lock().unwrap();
        let (_, attempt) = owned(&p, &h, &id)?;
        validate(&p, &h, &attempt)?;
        p.connections
            .lock()
            .unwrap()
            .claim(&id, now())
            .map_err(|_| fault(409, "connection attempt already consumed or expired"))?
    };
    let guard = ConnectionGuard {
        p: p.clone(),
        id: id.clone(),
    };
    let upstream = async {
        let state = p.controller.snapshot();
        let instance = state
            .games
            .get(&attempt.identity.app_id)
            .ok_or_else(|| fault(410, "server ended"))?;
        let address = p
            .controller
            .engine
            .web_address(&attempt.identity.app_id, instance, &state.owner)
            .await?;
        // The engine supplies this address; the browser cannot select an upstream,
        // path, forwarding headers, credentials, or a destination outside its game.
        let mut request = format!("ws://{address}/").into_client_request()?;
        request.headers_mut().insert(
            "x-infuser-connection",
            p.connection_signer.sign(&attempt.identity)?.parse()?,
        );
        let limits = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES))
            .max_write_buffer_size(MAX_MESSAGE_BYTES * 2);
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(limits), true).await?;
        Ok::<_, anyhow::Error>(socket)
    };
    let upstream = match timeout(Duration::from_secs(4), upstream).await {
        Ok(Ok(upstream)) => upstream,
        _ => {
            guard.fail("backend_unavailable");
            return Err(fault(503, "game connection unavailable").into());
        }
    };
    {
        let _lock = p.publishing.lock().unwrap();
        if validate(&p, &h, &attempt).is_err()
            || p.connections
                .lock()
                .unwrap()
                .0
                .get(&id)
                .is_none_or(|a| a.state != ConnectionState::Connecting)
        {
            guard.fail("authorization_ended");
            return Err(fault(410, "connection authorization ended").into());
        }
    }
    Ok(ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .max_write_buffer_size(MAX_MESSAGE_BYTES * 2)
        .on_upgrade(move |client| async move {
            p.connections
                .lock()
                .unwrap()
                .finish(&id, ConnectionState::Connected, None, now());
            tracing::info!(app_id = %attempt.identity.app_id, build_id = %attempt.identity.build_id,
                instance_id = %attempt.identity.instance_id, connection_id = %id,
                "managed connection established");
            if let Err(code) = proxy(client, upstream, &p, &h, &attempt).await {
                guard.fail(code);
            }
            drop(guard);
        })
        .into_response())
}

async fn proxy(
    mut client: WebSocket,
    mut upstream: Backend,
    p: &Platform,
    h: &HeaderMap,
    attempt: &Attempt,
) -> std::result::Result<(), &'static str> {
    let mut check_at = Instant::now();
    let mut last_pong = Instant::now();
    let mut last_upstream = Instant::now();
    let mut renewed = Instant::now();
    loop {
        // Check deadlines between frames; a busy sender cannot starve access checks.
        // Leave the two data directions unbiased so neither can starve the other.
        if Instant::now() >= check_at {
            if last_pong.elapsed() > Duration::from_secs(30)
                || last_upstream.elapsed() > Duration::from_secs(30)
            {
                return Err("connection_timeout");
            }
            {
                let _lock = p.publishing.lock().unwrap();
                validate(p, h, attempt).map_err(|_| "authorization_ended")?;
                if p.connections
                    .lock()
                    .unwrap()
                    .0
                    .get(&attempt.identity.connection_id)
                    .is_none_or(|a| a.state != ConnectionState::Connected)
                {
                    return Err("cancelled");
                }
                // Renew only the same validated launch/lease under this lock.
                if renewed.elapsed() >= Duration::from_secs(15)
                    && last_pong.elapsed() < Duration::from_secs(10)
                {
                    p.refresh_locked(&attempt.identity.launch_id)
                        .map_err(|_| "authorization_ended")?;
                    renewed = Instant::now();
                }
            }
            timeout(
                Duration::from_secs(3),
                client.send(Message::Ping(Vec::new().into())),
            )
            .await
            .map_err(|_| "connection_timeout")?
            .map_err(|_| "connection_lost")?;
            timeout(
                Duration::from_secs(3),
                upstream.send(Upstream::Ping(Vec::new().into())),
            )
            .await
            .map_err(|_| "connection_timeout")?
            .map_err(|_| "connection_lost")?;
            check_at = Instant::now() + Duration::from_secs(2);
        }
        tokio::select! {
            _ = tokio::time::sleep_until(check_at) => {},
            frame = client.next() => {
                let frame = frame.ok_or("connection_lost")?.map_err(|_| "connection_lost")?;
                let frame = match frame {
                    Message::Text(v) => Upstream::Text(v.to_string().into()),
                    Message::Binary(v) => Upstream::Binary(v),
                    Message::Pong(_) => {last_pong = Instant::now(); continue;},
                    Message::Ping(_) => continue,
                    Message::Close(_) => return Ok(()),
                };
                timeout(Duration::from_secs(3), upstream.send(frame)).await.map_err(|_| "connection_timeout")?.map_err(|_| "connection_lost")?;
            }
            frame = upstream.next() => {
                let frame = frame.ok_or("connection_lost")?.map_err(|_| "connection_lost")?;
                last_upstream = Instant::now();
                let frame = match frame {
                    Upstream::Text(v) => Message::Text(v.to_string().into()),
                    Upstream::Binary(v) => Message::Binary(v),
                    Upstream::Close(v) => {
                        let close = v.map(|v| axum::extract::ws::CloseFrame {code:v.code.into(), reason:v.reason.to_string().into()});
                        let _ = timeout(Duration::from_secs(3), client.send(Message::Close(close))).await;
                        return Ok(());
                    },
                    _ => continue,
                };
                timeout(Duration::from_secs(3), client.send(frame)).await.map_err(|_| "connection_timeout")?.map_err(|_| "connection_lost")?;
            }
        }
    }
}
