//! Ephemeral signaling only. Game simulation and traffic stay in browser WebRTC peers.
use crate::{config::Turn, platform::hash, portal::fault, store::secret};
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha1::Sha1;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

pub struct Relay {
    urls: Vec<String>,
    secret: Vec<u8>,
}
impl Relay {
    pub fn open(config: &Turn) -> Result<Self> {
        let value = std::fs::read_to_string(&config.secret_file)?;
        ensure!(value.trim().len() >= 32, "TURN shared secret is too short");
        Ok(Self {
            urls: config.urls.clone(),
            secret: value.trim().as_bytes().to_vec(),
        })
    }
    pub fn credentials(&self, peer: &str, time: u64) -> Value {
        let expires = time + 3600;
        let username = format!("{expires}:{peer}");
        let mut mac =
            Hmac::<Sha1>::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(username.as_bytes());
        json!({"urls":self.urls,"username":username,"credential":STANDARD.encode(mac.finalize().into_bytes()),"expires_at":expires})
    }
}
#[derive(Clone)]
pub struct Identity {
    pub launch: String,
    pub player: String,
    pub name: String,
    pub guest: bool,
}
pub struct Join<'a> {
    pub app: &'a str,
    pub build: &'a str,
    pub connection: &'a str,
    pub max_players: u32,
}
struct Peer {
    identity: Identity,
    expires: u64,
    signals: VecDeque<Value>,
}
struct Room {
    epoch: String,
    build: String,
    host: String,
    peers: BTreeMap<String, Peer>,
    sequence: u64,
}
#[derive(Default)]
pub struct Rooms {
    rooms: Mutex<BTreeMap<String, Room>>,
    resume_after: u64,
}
fn reap(rooms: &mut BTreeMap<String, Room>, time: u64) {
    rooms.retain(|_, r| {
        r.peers.retain(|_, p| p.expires > time);
        r.peers.contains_key(&r.host)
    });
}
fn view(r: &Room, peer: &str) -> Value {
    json!({"epoch":r.epoch,"build_id":r.build,"host_id":r.host,"self_id":peer,"role":if r.host==peer{"host"}else{"member"},"lease_ms":25000,"peers":r.peers.iter().map(|(id,p)|json!({"id":id,"player_id":p.identity.player,"name":p.identity.name,"guest":p.identity.guest})).collect::<Vec<_>>()})
}
fn member<'a>(
    rooms: &'a mut BTreeMap<String, Room>,
    app: &str,
    peer: &str,
    launch: &str,
) -> Result<&'a mut Room> {
    let room = rooms
        .get_mut(app)
        .ok_or_else(|| fault(410, "shared session ended; join again"))?;
    ensure!(
        room.peers
            .get(peer)
            .is_some_and(|p| p.identity.launch == launch),
        fault(403, "not your peer connection")
    );
    Ok(room)
}
impl Rooms {
    pub fn recovering(resume_after: u64) -> Self {
        Self {
            resume_after,
            ..Self::default()
        }
    }
    pub fn active(&self, app: &str, time: u64) -> bool {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        rooms.contains_key(app)
    }
    pub fn join(&self, who: Identity, request: Join<'_>, time: u64) -> Result<Value> {
        ensure!(
            time >= self.resume_after,
            fault(503, "coordinator restarted; wait 30 seconds before joining")
        );
        ensure!(
            (16..=64).contains(&request.connection.len())
                && request
                    .connection
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            fault(400, "invalid connection ID")
        );
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let peer = hash(&format!("{}/{}", who.launch, request.connection))[..32].to_owned();
        let existing = rooms
            .get(request.app)
            .is_some_and(|r| r.peers.contains_key(&peer));
        ensure!(
            existing || rooms.values().map(|r| r.peers.len()).sum::<usize>() < 128,
            fault(503, "peer connection budget is full")
        );
        let room = rooms.entry(request.app.into()).or_insert_with(|| Room {
            epoch: secret()[..32].into(),
            build: request.build.into(),
            host: peer.clone(),
            peers: BTreeMap::new(),
            sequence: 0,
        });
        ensure!(
            room.build == request.build,
            fault(
                409,
                "this shared session uses another build; reopen the game"
            )
        );
        ensure!(
            existing || room.peers.len() < (request.max_players.min(8) as usize),
            fault(409, "the shared session is full")
        );
        room.peers
            .entry(peer.clone())
            .or_insert(Peer {
                identity: who,
                expires: time + 30,
                signals: VecDeque::new(),
            })
            .expires = time + 30;
        Ok(view(room, &peer))
    }
    pub fn poll(
        &self,
        app: &str,
        peer: &str,
        launch: &str,
        after: u64,
        time: u64,
    ) -> Result<Value> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = member(&mut rooms, app, peer, launch)?;
        let p = room.peers.get_mut(peer).unwrap();
        p.expires = time + 30;
        p.signals
            .retain(|s| s["sequence"].as_u64().unwrap() > after);
        let signals = p.signals.iter().cloned().collect::<Vec<_>>();
        let next = signals
            .last()
            .and_then(|s| s["sequence"].as_u64())
            .unwrap_or(after);
        let mut result = view(room, peer);
        result["signals"] = json!(signals);
        result["cursor"] = json!(next);
        Ok(result)
    }
    pub fn signal(
        &self,
        app: &str,
        peer: &str,
        launch: &str,
        body: &Value,
        time: u64,
    ) -> Result<()> {
        ensure!(
            body.to_string().len() <= 20000,
            fault(413, "signaling message too large")
        );
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = member(&mut rooms, app, peer, launch)?;
        ensure!(
            body["epoch"] == room.epoch,
            fault(409, "stale shared session")
        );
        let to = body["to"]
            .as_str()
            .ok_or_else(|| fault(400, "target peer required"))?;
        ensure!(
            to != peer && (room.host == peer || room.host == to),
            fault(403, "signal only between host and members")
        );
        ensure!(
            matches!(
                body["payload"]["type"].as_str(),
                Some("offer" | "answer" | "candidate")
            ),
            fault(400, "unsupported signaling message")
        );
        let target = room
            .peers
            .get_mut(to)
            .ok_or_else(|| fault(404, "peer left"))?;
        ensure!(
            target.signals.len() < 32,
            fault(429, "peer signaling queue is full")
        );
        room.sequence += 1;
        target
            .signals
            .push_back(json!({"sequence":room.sequence,"from":peer,"payload":body["payload"]}));
        Ok(())
    }
    pub fn leave(&self, app: &str, peer: &str, launch: &str, time: u64) -> Result<()> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = member(&mut rooms, app, peer, launch)?;
        if room.host == peer {
            rooms.remove(app);
        } else {
            room.peers.remove(peer);
        }
        Ok(())
    }
    pub fn end(&self, app: &str) {
        self.rooms.lock().unwrap().remove(app);
    }
    pub fn leave_launch(&self, app: &str, launch: &str) {
        let mut rooms = self.rooms.lock().unwrap();
        if let Some(room) = rooms.get_mut(app) {
            room.peers.retain(|_, p| p.identity.launch != launch);
            if !room.peers.contains_key(&room.host) {
                rooms.remove(app);
            }
        }
    }
}
