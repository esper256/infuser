//! Ephemeral signaling only. Game simulation and traffic stay in browser WebRTC peers.
use crate::{config::Turn, platform::hash, portal::fault, store::secret};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::IpAddr,
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
        json!({"urls":self.urls,"username":username,"credential":crate::turn::credential(&self.secret, &username),"expires_at":expires})
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
    pub create: bool,
    pub expected_epoch: Option<&'a str>,
    pub expected_player: Option<&'a str>,
    pub joinable: Option<bool>,
}
struct Peer {
    connection: String,
    identity: Identity,
    expires: u64,
    signals: VecDeque<Value>,
    health: Value,
    health_seen: u64,
    address: Option<IpAddr>,
}
struct Room {
    app: String,
    automatic: bool,
    epoch: String,
    build: String,
    host: String,
    peers: BTreeMap<String, Peer>,
    sequence: u64,
    max_players: u32,
    // None preserves legacy admission, but never advertises a joinable session.
    accepting_players: Option<bool>,
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
    json!({"epoch":r.epoch,"build_id":r.build,"host_id":r.host,"self_id":peer,"role":if r.host==peer{"host"}else{"member"},"lease_ms":25000,"accepting_players":r.accepting_players,"max_players":r.max_players,"peers":r.peers.iter().map(|(id,p)|json!({"id":id,"player_id":p.identity.player,"name":p.identity.name,"guest":p.identity.guest})).collect::<Vec<_>>()})
}
fn member<'a>(
    rooms: &'a mut BTreeMap<String, Room>,
    app: &str,
    peer: &str,
    launch: &str,
) -> Result<&'a mut Room> {
    let room = rooms
        .values_mut()
        .find(|r| r.app == app && r.peers.contains_key(peer))
        .ok_or_else(|| fault(410, "peer session ended; join again"))?;
    ensure!(
        room.peers
            .get(peer)
            .is_some_and(|p| p.identity.launch == launch),
        fault(403, "not your peer connection")
    );
    Ok(room)
}
impl Rooms {
    /// Authorize signaling without extending the coordinator lease.
    pub fn authorize(
        &self,
        app: &str,
        peer: &str,
        launch: &str,
        epoch: &str,
        to: Option<&str>,
        time: u64,
    ) -> Result<()> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = member(&mut rooms, app, peer, launch)?;
        ensure!(room.epoch == epoch, fault(409, "stale shared session"));
        if let Some(to) = to {
            ensure!(
                to != peer && (room.host == peer || room.host == to) && room.peers.contains_key(to),
                fault(403, "signal only between this session's host and members")
            );
        }
        Ok(())
    }
    pub fn available(&self, time: u64) -> bool {
        time >= self.resume_after
    }
    /// Remove disabled/expired launches without trusting a game's self-report.
    pub fn retain_launches(&self, app: &str, live: &BTreeSet<String>, time: u64) {
        let mut rooms = self.rooms.lock().unwrap();
        for room in rooms.values_mut().filter(|r| r.app == app) {
            room.peers.retain(|_, p| live.contains(&p.identity.launch));
        }
        reap(&mut rooms, time);
    }
    pub fn discover_all(&self, app: &str, build: &str, time: u64) -> Vec<Value> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        rooms
            .values()
            .filter(|room| room.app == app)
            .map(|room| Self::describe(room, build))
            .collect()
    }
    fn describe(room: &Room, build: &str) -> Value {
        let reason = if room.build != build {
            Some("different_build")
        } else if room.peers.len() >= room.max_players as usize {
            Some("full")
        } else {
            match room.accepting_players {
                Some(true) => None,
                Some(false) => Some("closed"),
                None => Some("not_advertised"),
            }
        };
        json!({"id":room.epoch,"matchmaking":room.automatic,"build_id":room.build,"host_player_id":room.peers[&room.host].identity.player,
            "player_count":room.peers.len(),"max_players":room.max_players,"joinable":reason.is_none(),"reason":reason,
            "players":room.peers.values().map(|p|json!({"player_id":p.identity.player,"name":p.identity.name,"guest":p.identity.guest})).collect::<Vec<_>>()})
    }
    pub fn set_joinable(
        &self,
        app: &str,
        peer: &str,
        launch: &str,
        epoch: &str,
        joinable: bool,
        time: u64,
    ) -> Result<Value> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = member(&mut rooms, app, peer, launch)?;
        ensure!(
            room.host == peer,
            fault(403, "only the host may change joinability")
        );
        ensure!(room.epoch == epoch, fault(409, "stale shared session"));
        room.accepting_players = Some(joinable);
        Ok(view(room, peer))
    }
    pub fn recovering(resume_after: u64) -> Self {
        Self {
            resume_after,
            ..Self::default()
        }
    }
    pub fn active(&self, app: &str, time: u64) -> bool {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        rooms.values().any(|r| r.app == app)
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
        ensure!(
            !request.create
                || (request.expected_epoch.is_none() && request.expected_player.is_none()),
            fault(400, "creating a room cannot target another session")
        );
        ensure!(
            request.expected_player.is_none() || request.expected_epoch.is_some(),
            fault(400, "a friend target requires a session ID")
        );

        // Reusing a live connection ID is a retry. Keep peer identities stable within
        // one epoch, so a delayed leave can never close a different room.
        let previous = rooms
            .values()
            .filter(|room| room.app == request.app)
            .find_map(|room| {
                room.peers
                    .iter()
                    .find(|(_, p)| {
                        p.identity.launch == who.launch && p.connection == request.connection
                    })
                    .map(|(id, _)| (room, id))
            });
        let existing = previous.is_some();
        ensure!(
            existing || rooms.values().map(|r| r.peers.len()).sum::<usize>() < 128,
            fault(503, "peer connection budget is full")
        );
        let selected = if let Some(target) = request.expected_epoch {
            let room = rooms
                .get(target)
                .filter(|r| r.app == request.app)
                .ok_or_else(|| fault(410, "that room ended; return to the title screen"))?;
            ensure!(
                previous.as_ref().is_none_or(|(r, _)| r.epoch == target),
                fault(409, "connection already belongs to another room")
            );
            if let Some(player) = request.expected_player {
                ensure!(
                    room.peers.values().any(|p| p.identity.player == player),
                    fault(
                        410,
                        "that friend left the session; return to the title screen"
                    )
                );
            }
            Some(room)
        } else if let Some((room, peer)) = previous {
            ensure!(
                !request.create || (!room.automatic && &room.host == peer),
                fault(409, "connection already belongs to another room")
            );
            Some(room)
        } else if request.create {
            None
        } else {
            rooms.values().find(|r| r.app == request.app && r.automatic)
        };
        let epoch = selected
            .map(|r| r.epoch.clone())
            .unwrap_or_else(|| secret()[..32].to_owned());
        let peer = hash(&format!("{epoch}/{}/{}", who.launch, request.connection))[..32].to_owned();
        let room = rooms.entry(epoch.clone()).or_insert_with(|| Room {
            app: request.app.into(),
            automatic: !request.create,
            epoch,
            build: request.build.into(),
            host: peer.clone(),
            peers: BTreeMap::new(),
            sequence: 0,
            max_players: request.max_players.min(8),
            accepting_players: request.joinable.or(request.create.then_some(true)),
        });
        ensure!(
            room.build == request.build,
            fault(
                409,
                "this shared session uses another build; reopen the game"
            )
        );
        ensure!(
            existing || room.peers.len() < (room.max_players as usize),
            fault(409, "the shared session is full")
        );
        ensure!(
            existing
                || room.host == peer
                || (room.accepting_players != Some(false)
                    && (request.expected_epoch.is_none() || room.accepting_players == Some(true))),
            fault(409, "the shared session is not accepting players right now")
        );
        room.peers
            .entry(peer.clone())
            .or_insert(Peer {
                connection: request.connection.to_owned(),
                identity: who,
                expires: time + 30,
                signals: VecDeque::new(),
                health: Value::Null,
                health_seen: 0,
                address: None,
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
            let epoch = room.epoch.clone();
            rooms.remove(&epoch);
        } else {
            room.peers.remove(peer);
        }
        Ok(())
    }
    pub fn admitted(&self, peer: &str, time: u64) -> bool {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        rooms.values().any(|room| room.peers.contains_key(peer))
    }
    pub fn note_address(&self, app: &str, peer: &str, launch: &str, address: IpAddr, time: u64) {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let Ok(room) = member(&mut rooms, app, peer, launch) else {
            return;
        };
        if let Some(peer) = room.peers.get_mut(peer) {
            peer.address = Some(address);
        }
    }
    pub fn seat(&self, app: &str, launch: &str, time: u64) -> Option<crate::turn::Seat> {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let room = rooms.values().find(|room| {
            room.app == app
                && room
                    .peers
                    .values()
                    .any(|peer| peer.identity.launch == launch)
        })?;
        let self_id = room
            .peers
            .iter()
            .find(|(_, peer)| peer.identity.launch == launch)
            .map(|(id, _)| id.clone())?;
        Some(crate::turn::Seat {
            host: room.host == self_id,
            peers: room
                .peers
                .iter()
                .map(|(id, peer)| crate::turn::Participant {
                    id: id.clone(),
                    address: peer.address,
                })
                .collect(),
            self_id,
        })
    }
    pub fn end(&self, app: &str) {
        self.rooms.lock().unwrap().retain(|_, r| r.app != app);
    }
    pub fn health(
        &self,
        app: &str,
        peer: &str,
        launch: &str,
        report: Value,
        time: u64,
    ) -> Result<()> {
        ensure!(
            report.to_string().len() <= 4096
                && report["connections"]
                    .as_array()
                    .is_some_and(|v| v.len() <= 8),
            fault(400, "invalid network report")
        );
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        let p = member(&mut rooms, app, peer, launch)?
            .peers
            .get_mut(peer)
            .unwrap();
        p.health = report;
        p.health_seen = time;
        Ok(())
    }
    pub fn diagnostics(&self, time: u64) -> Value {
        let mut rooms = self.rooms.lock().unwrap();
        reap(&mut rooms, time);
        json!({"sessions":rooms.values().map(|r|json!({"app_id":r.app,"session_id":r.epoch,"build_id":r.build,"peers":r.peers.iter().map(|(id,p)|json!({"id":id,"name":p.identity.name,"host":*id==r.host,"reported_at":p.health_seen,"health":p.health})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    pub fn leave_launch(&self, app: &str, launch: &str) {
        let mut rooms = self.rooms.lock().unwrap();
        rooms.retain(|_, room| {
            if room.app != app {
                return true;
            }
            room.peers.retain(|_, p| p.identity.launch != launch);
            room.peers.contains_key(&room.host)
        });
    }
}
