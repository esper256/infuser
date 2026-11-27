//! Authenticated gateway to a private upstream PeerServer. No game bytes transit here.
use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeSet;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::tungstenite::Message as Upstream;

#[derive(Deserialize)]
pub(super) struct Connection {
    id: String,
    key: String,
    token: String,
}
fn parts(id: &str) -> Result<(&str, &str)> {
    let (epoch, peer) = id
        .strip_prefix("i-")
        .and_then(|v| v.split_once('-'))
        .ok_or_else(|| fault(400, "invalid PeerJS ID"))?;
    ensure!(
        [epoch, peer]
            .iter()
            .all(|v| v.len() == 32 && v.bytes().all(|c| c.is_ascii_hexdigit())),
        fault(400, "invalid PeerJS ID")
    );
    Ok((epoch, peer))
}
fn authorize(p: &Platform, h: &HeaderMap, wire: &str, target: Option<&str>) -> Result<()> {
    let (epoch, peer) = parts(wire)?;
    let (launch_id, launch) = p.game(h)?;
    let data = p.db.read()?;
    ensure!(
        data.launch_build(&launch_id, &launch)?
            .manifest
            .multiplayer
            .as_ref()
            .is_some_and(|m| m.adapter == Some(crate::manifest::PeerAdapter::Peerjs)),
        fault(422, "this game does not declare PeerJS")
    );
    p.peers.retain_launches(
        launch.runtime_id(),
        &presence::live_launches(&data, launch.runtime_id(), now()),
        now(),
    );
    let to = if let Some(wire) = target {
        let (target_epoch, target_peer) = parts(wire)?;
        ensure!(
            target_epoch == epoch,
            fault(403, "different shared session")
        );
        Some(target_peer)
    } else {
        None
    };
    p.peers
        .authorize(launch.runtime_id(), peer, &launch_id, epoch, to, now())
}
struct Reservation(Arc<Mutex<BTreeSet<String>>>, String);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
    }
}
pub(super) async fn socket(
    State(p): State<Platform>,
    h: HeaderMap,
    Query(c): Query<Connection>,
    ws: WebSocketUpgrade,
) -> Answer {
    p.origin(&h, host(&h))?;
    authorize(&p, &h, &c.id, None)?;
    ensure!(
        c.key == "peerjs"
            && (16..=128).contains(&c.token.len())
            && c.token
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || v == b'-'),
        fault(400, "invalid PeerJS connection parameters")
    );
    let port = p
        .config()
        .controller
        .peerjs_port
        .ok_or_else(|| fault(503, "PeerJS is not configured"))?;
    ensure!(
        p.peerjs_sockets.lock().unwrap().insert(c.id.clone()),
        fault(409, "PeerJS connection already open")
    );
    let reservation = Reservation(p.peerjs_sockets.clone(), c.id.clone());
    // Never forward game cookies, player tickets, or arbitrary URLs to the sidecar.
    let upstream = format!(
        "ws://127.0.0.1:{port}/peerjs?key=peerjs&id={}&token={}",
        c.id, c.token
    );
    let upstream = timeout(
        Duration::from_secs(4),
        tokio_tungstenite::connect_async(upstream),
    )
    .await
    .map_err(|_| fault(503, "PeerServer unavailable"))?
    .map_err(|_| fault(503, "PeerServer unavailable"))?
    .0;
    Ok(ws
        .max_message_size(65536)
        .max_frame_size(65536)
        .on_upgrade(move |socket| async move {
            let _reservation = reservation;
            let _ = proxy(socket, upstream, p, h, c.id).await;
        })
        .into_response())
}
async fn proxy(
    mut client: WebSocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    p: Platform,
    h: HeaderMap,
    id: String,
) -> Result<()> {
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut window = now();
    let mut count = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => { authorize(&p,&h,&id,None)?; }
            frame = client.next() => {
                let Some(Ok(frame)) = frame else { break };
                let Message::Text(text) = frame else { if matches!(frame,Message::Close(_)) {break} else {continue} };
                if window != now() {window=now();count=0;}
                count+=1;
                ensure!(count<=120,fault(429,"too many signaling messages"));
                let message: Value = serde_json::from_str(text.as_str())?;
                match message["type"].as_str() {
                    Some("HEARTBEAT") => { authorize(&p,&h,&id,None)?; }
                    Some("OFFER"|"ANSWER"|"CANDIDATE") => {
                        let to = message["dst"].as_str().ok_or_else(||fault(400,"destination required"))?;
                        authorize(&p,&h,&id,Some(to))?;
                        ensure!(message["type"] != "OFFER" || message["payload"]["type"] == "data",fault(422,"PeerJS data connections only"));
                    }
                    _ => return Err(fault(400,"unsupported PeerJS signaling message")),
                }
                timeout(Duration::from_secs(3),upstream.send(Upstream::Text(text.to_string().into()))).await??;
            }
            frame = upstream.next() => {
                let Some(Ok(frame)) = frame else {break};
                if let Upstream::Text(text) = frame {
                    ensure!(text.len() <= 65536, fault(413,"signaling message too large"));
                    let message: Value = serde_json::from_str(&text)?;
                    authorize(&p,&h,&id,None)?;
                    if matches!(message["type"].as_str(),Some("OFFER"|"ANSWER"|"CANDIDATE")) {
                        authorize(&p,&h,&id,Some(message["src"].as_str().ok_or_else(||fault(400,"source required"))?))?;
                    }
                    timeout(Duration::from_secs(3),client.send(Message::Text(text.to_string().into()))).await??;
                } else if matches!(frame,Upstream::Close(_)) {break;}
            }
        }
    }
    Ok(())
}
