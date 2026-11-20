//! Player-bar network path. The server decides it.
//!
//! Coturn's loopback telnet CLI (`ps`) is how Infuser sees the relay. The CLI
//! is inside the coturn process, so a small ARM host does not grow another
//! service. Prometheus counters are not per peer, and its username labels leak
//! memory with the short-lived usernames Infuser issues. Redis would be another
//! process and still would not list STUN bindings. `ps` lists each live
//! session: the TURN username on an allocation, and the client address on a
//! STUN binding that has no username.
//!
//! The CLI listens on 127.0.0.1 only. Infuser dials that address and no other.

use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Mutex,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Server,
    Relayed,
    Stun,
    Lan,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Relayed => "Relayed",
            Self::Stun => "STUN",
            Self::Lan => "LAN",
        }
    }
}

/// One coturn session, parsed from `ps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub username: String,
    pub client: Option<IpAddr>,
    pub relay: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub truncated: bool,
}

/// A peer Infuser currently has in a room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub address: Option<IpAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub self_id: String,
    pub host: bool,
    pub peers: Vec<Participant>,
}

/// A dedicated-container connection Infuser issued for one launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub connection_id: String,
    pub runtime_id: String,
    pub instance_id: String,
    pub connected: bool,
}

/// One fresh dedicated-session report from a game container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub runtime_id: String,
    pub instance_id: String,
    pub fresh: bool,
    pub connections: Vec<String>,
}

pub enum Reading {
    NotNeeded,
    Unconfigured,
    Failed,
    Ready(Snapshot),
}

pub struct Query {
    pub server: bool,
    pub seat: Option<Seat>,
    pub reading: Reading,
}

#[derive(Default)]
struct Fact {
    stun: bool,
    kind: Option<Kind>,
}

/// Remembers a STUN binding after coturn drops the short-lived session.
#[derive(Default)]
pub struct Memory {
    peers: Mutex<BTreeMap<String, Fact>>,
}

impl Memory {
    pub fn label(&self, query: &Query) -> Option<String> {
        if query.server {
            return Some(Kind::Server.word().to_owned());
        }
        let Some(seat) = &query.seat else {
            return Some(Kind::Lan.word().to_owned());
        };
        match &query.reading {
            Reading::NotNeeded | Reading::Unconfigured => None,
            Reading::Failed => self.recall(seat),
            Reading::Ready(snapshot) => self.observe(seat, snapshot),
        }
    }

    fn recall(&self, seat: &Seat) -> Option<String> {
        let facts = self.peers.lock().unwrap();
        remembered(&facts, seat).map(|kinds| format_kinds(&kinds))
    }

    fn observe(&self, seat: &Seat, snapshot: &Snapshot) -> Option<String> {
        let mut facts = self.peers.lock().unwrap();
        if facts.len() > 256 {
            let live: BTreeSet<_> = seat.peers.iter().map(|peer| peer.id.as_str()).collect();
            facts.retain(|id, _| live.contains(id.as_str()));
        }
        for peer in &seat.peers {
            let fact = facts.entry(peer.id.clone()).or_default();
            if let Some(kind) = classify(peer, fact, snapshot) {
                fact.kind = Some(kind);
            }
        }
        remembered(&facts, seat).map(|kinds| format_kinds(&kinds))
    }
}

/// True when this launch's connection is listed on a live dedicated session.
pub fn on_server(links: &[Link], reports: &[Report]) -> bool {
    links.iter().any(|link| {
        link.connected
            && reports.iter().any(|report| {
                report.fresh
                    && report.runtime_id == link.runtime_id
                    && report.instance_id == link.instance_id
                    && report
                        .connections
                        .iter()
                        .any(|id| id == &link.connection_id)
            })
    })
}

fn remembered(facts: &BTreeMap<String, Fact>, seat: &Seat) -> Option<Vec<Kind>> {
    let people = subjects(seat);
    if people.is_empty() {
        return None;
    }
    people
        .into_iter()
        .map(|peer| facts.get(&peer.id).and_then(|fact| fact.kind))
        .collect()
}

fn subjects(seat: &Seat) -> Vec<&Participant> {
    if seat.host {
        let others: Vec<_> = seat
            .peers
            .iter()
            .filter(|peer| peer.id != seat.self_id)
            .collect();
        if !others.is_empty() {
            return others;
        }
    }
    seat.peers
        .iter()
        .filter(|peer| peer.id == seat.self_id)
        .collect()
}

fn classify(peer: &Participant, fact: &mut Fact, snapshot: &Snapshot) -> Option<Kind> {
    let allocated = snapshot
        .sessions
        .iter()
        .any(|session| session.relay && username_is(&peer.id, &session.username));
    let binding = snapshot.sessions.iter().any(|session| {
        !session.relay
            && (username_is(&peer.id, &session.username)
                || (session.username.is_empty()
                    && peer.address.is_some()
                    && session.client == peer.address))
    });
    if binding {
        fact.stun = true;
    }
    if allocated {
        return Some(Kind::Relayed);
    }
    if fact.stun {
        return Some(Kind::Stun);
    }
    if snapshot.truncated {
        return None;
    }
    Some(Kind::Lan)
}

fn username_is(peer: &str, username: &str) -> bool {
    let Some((expires, id)) = username.split_once(':') else {
        return false;
    };
    id == peer && !expires.is_empty() && expires.bytes().all(|byte| byte.is_ascii_digit())
}

fn format_kinds(kinds: &[Kind]) -> String {
    if kinds.len() == 1 {
        return kinds[0].word().to_owned();
    }
    let stun = kinds.iter().filter(|kind| **kind == Kind::Stun).count();
    let relayed = kinds.iter().filter(|kind| **kind == Kind::Relayed).count();
    let lan = kinds.iter().filter(|kind| **kind == Kind::Lan).count();
    let mut parts = Vec::new();
    if stun > 0 {
        parts.push(format!("{stun} STUN"));
    }
    if relayed > 0 {
        parts.push(format!("{relayed} relayed"));
    }
    if lan > 0 {
        parts.push(format!("{lan} LAN"));
    }
    if parts.is_empty() {
        Kind::Lan.word().to_owned()
    } else {
        parts.join(", ")
    }
}

pub fn parse(text: &str) -> Snapshot {
    let mut sessions = Vec::new();
    let mut current: Option<Session> = None;
    let mut truncated = false;
    for line in text.lines() {
        let line = line.trim().trim_end_matches('\r');
        if line == "..." {
            truncated = true;
            continue;
        }
        if let Some(username) = username_from(line) {
            if let Some(done) = current.take() {
                sessions.push(done);
            }
            current = Some(Session {
                username,
                client: None,
                relay: false,
            });
            continue;
        }
        let Some(session) = current.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("client addr ") {
            let token = rest.split(',').next().unwrap_or("");
            session.client = socket_ip(token);
        } else if let Some(rest) = line.strip_prefix("relay addr ")
            && socket_port(rest).is_some_and(|port| port > 0)
        {
            session.relay = true;
        }
    }
    if let Some(done) = current {
        sessions.push(done);
    }
    Snapshot {
        sessions,
        truncated,
    }
}

fn username_from(line: &str) -> Option<String> {
    let rest = line.split_once("user <")?.1;
    Some(rest.split_once(">:")?.0.to_owned())
}

fn socket_ip(token: &str) -> Option<IpAddr> {
    let token = token.trim();
    if let Some(rest) = token.strip_prefix('[') {
        return rest.split_once(']')?.0.parse().ok();
    }
    token.rsplit_once(':')?.0.parse().ok()
}

fn socket_port(token: &str) -> Option<u16> {
    let token = token.trim();
    let port = if let Some(rest) = token.strip_prefix('[') {
        rest.split_once(']')?.1.strip_prefix(':')?
    } else {
        token.rsplit_once(':')?.1
    };
    port.parse().ok()
}

pub async fn read_cli(port: u16, password: &str) -> Result<Snapshot> {
    let password = password.trim();
    ensure!(
        !password.is_empty()
            && password.len() <= 128
            && !password.bytes().any(|byte| byte == b'\n' || byte == b'\r'),
        "invalid TURN CLI password"
    );
    tokio::time::timeout(Duration::from_secs(2), read_cli_once(port, password))
        .await
        .context("coturn CLI timed out")?
}

async fn read_cli_once(port: u16, password: &str) -> Result<Snapshot> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .context("coturn CLI is not listening on 127.0.0.1")?;
    stream.set_nodelay(true)?;
    let mut telnet = Telnet::default();
    let mut step = Step::Login;
    let mut cursor = 0;
    let mut buf = [0u8; 8192];
    loop {
        let read = stream
            .read(&mut buf)
            .await
            .context("coturn CLI read failed")?;
        if read == 0 {
            bail!("coturn CLI closed before the session list");
        }
        let replies = telnet.push(&buf[..read]);
        if !replies.is_empty() {
            stream.write_all(&replies).await?;
        }
        match step {
            Step::Login => {
                let rest = &telnet.text[cursor..];
                if let Some(at) = rest.find("Enter password:") {
                    cursor += at + "Enter password:".len();
                    stream
                        .write_all(format!("{password}\r\n").as_bytes())
                        .await?;
                    step = Step::Ready;
                } else if rest.contains("> ") {
                    bail!("coturn CLI has no password");
                }
            }
            Step::Ready => {
                let rest = &telnet.text[cursor..];
                if rest.contains("Enter password:") {
                    bail!("coturn CLI rejected the password");
                }
                if let Some(at) = rest.find("> ") {
                    cursor += at + "> ".len();
                    stream.write_all(b"ps\r\n").await?;
                    step = Step::Dump;
                }
            }
            Step::Dump => {
                if telnet.text[cursor..].contains("Total sessions") {
                    return Ok(parse(&telnet.text[cursor..]));
                }
            }
        }
    }
}

enum Step {
    Login,
    Ready,
    Dump,
}

#[derive(Default)]
struct Telnet {
    pending: Vec<u8>,
    text: String,
}

impl Telnet {
    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut replies = Vec::new();
        let mut index = 0;
        let mut plain = Vec::new();
        while index < self.pending.len() {
            if self.pending[index] != 255 {
                plain.push(self.pending[index]);
                index += 1;
                continue;
            }
            if index + 1 >= self.pending.len() {
                break;
            }
            match self.pending[index + 1] {
                255 => {
                    plain.push(255);
                    index += 2;
                }
                250 => {
                    let mut end = index + 2;
                    let mut found = false;
                    while end + 1 < self.pending.len() {
                        if self.pending[end] == 255 && self.pending[end + 1] == 240 {
                            end += 2;
                            found = true;
                            break;
                        }
                        end += 1;
                    }
                    if !found {
                        break;
                    }
                    index = end;
                }
                251 => {
                    if index + 2 >= self.pending.len() {
                        break;
                    }
                    replies.extend([255, 254, self.pending[index + 2]]);
                    index += 3;
                }
                253 => {
                    if index + 2 >= self.pending.len() {
                        break;
                    }
                    replies.extend([255, 252, self.pending[index + 2]]);
                    index += 3;
                }
                252 | 254 => {
                    if index + 2 >= self.pending.len() {
                        break;
                    }
                    index += 3;
                }
                _ => index += 2,
            }
        }
        self.pending.drain(..index);
        self.text.push_str(&String::from_utf8_lossy(&plain));
        replies
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn session(username: &str, client: Option<&str>, relay: bool) -> Session {
        Session {
            username: username.into(),
            client: client.map(|ip| ip.parse().unwrap()),
            relay,
        }
    }

    fn participant(id: &str, address: Option<&str>) -> Participant {
        Participant {
            id: id.into(),
            address: address.map(|ip| ip.parse().unwrap()),
        }
    }

    fn seat(self_id: &str, host: bool, peers: Vec<Participant>) -> Seat {
        Seat {
            self_id: self_id.into(),
            host,
            peers,
        }
    }

    const LISTING: &str = "
 1) id=000000000000000001, user <1710000000:relayed-peer>:
 client addr 203.0.113.8:54321, server addr 10.0.0.5:8700
 relay addr 10.0.0.5:8804
 2) id=000000000000000002, user <1710000000:stun-peer>:
 client addr 203.0.113.9:40000, server addr 10.0.0.5:8700
 relay addr 0.0.0.0:0
 3) id=000000000000000003, user <>:
 client addr 198.51.100.20:41000, server addr 10.0.0.5:8700
 4) id=000000000000000004, user <1710000099:v6-peer>:
 client addr [2001:db8::10]:5000, server addr 10.0.0.5:8700
 relay addr [2001:db8::1]:8808
 ...
 Total sessions: 9
";

    #[test]
    fn parse_reads_allocations_bindings_and_truncation() {
        let snapshot = parse(LISTING);
        assert!(snapshot.truncated);
        assert_eq!(
            snapshot.sessions,
            vec![
                session("1710000000:relayed-peer", Some("203.0.113.8"), true),
                session("1710000000:stun-peer", Some("203.0.113.9"), false),
                session("", Some("198.51.100.20"), false),
                session("1710000099:v6-peer", Some("2001:db8::10"), true),
            ]
        );
        assert_eq!(
            snapshot.sessions[3].client,
            Some(IpAddr::V6(Ipv6Addr::new(
                0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x10
            )))
        );
    }

    #[test]
    fn allocation_is_relayed_and_a_named_binding_is_stun() {
        let memory = Memory::default();
        let room = seat(
            "member",
            false,
            vec![participant("member", Some("203.0.113.9"))],
        );
        let snapshot = parse(
            "1) id=1, user <171:member>:\n client addr 203.0.113.9:1, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        let label = memory.label(&Query {
            server: false,
            seat: Some(room),
            reading: Reading::Ready(snapshot),
        });
        assert_eq!(label.as_deref(), Some("STUN"));
    }

    #[test]
    fn anonymous_binding_matches_the_player_address() {
        let memory = Memory::default();
        let room = seat(
            "member",
            false,
            vec![
                participant("host", Some("203.0.113.1")),
                participant("member", Some("198.51.100.20")),
            ],
        );
        let snapshot = parse(
            "1) id=1, user <>:\n client addr 198.51.100.20:9, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("STUN")
        );
    }

    #[test]
    fn a_different_address_stays_lan() {
        let memory = Memory::default();
        let room = seat(
            "member",
            false,
            vec![participant("member", Some("203.0.113.4"))],
        );
        let snapshot = parse(
            "1) id=1, user <>:\n client addr 198.51.100.20:9, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("LAN")
        );
    }

    #[test]
    fn players_on_one_address_share_an_anonymous_binding() {
        let memory = Memory::default();
        let room = seat(
            "host",
            true,
            vec![
                participant("host", Some("203.0.113.8")),
                participant("a", Some("198.51.100.7")),
                participant("b", Some("198.51.100.7")),
            ],
        );
        let snapshot = parse(
            "1) id=1, user <>:\n client addr 198.51.100.7:9, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("2 STUN")
        );
    }

    #[test]
    fn allocation_wins_over_a_binding_and_uses_the_issued_username() {
        let memory = Memory::default();
        let room = seat(
            "member",
            false,
            vec![participant("member", Some("203.0.113.8"))],
        );
        let snapshot = parse(
            "1) id=1, user <171:member>:\n client addr 203.0.113.8:1, server addr 10.0.0.1:8700\n relay addr 10.0.0.1:8801\n 2) id=2, user <>:\n client addr 203.0.113.8:2, server addr 10.0.0.1:8700\n Total sessions: 2\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn a_seen_binding_stays_stun_after_coturn_forgets_it() {
        let memory = Memory::default();
        let room = seat("member", false, vec![participant("member", None)]);
        let seen = parse(
            "1) id=1, user <171:member>:\n client addr 203.0.113.8:1, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        memory.label(&Query {
            server: false,
            seat: Some(room.clone()),
            reading: Reading::Ready(seen),
        });
        let gone = parse("Total sessions: 0\n");
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(gone),
                })
                .as_deref(),
            Some("STUN")
        );
    }

    #[test]
    fn a_dropped_allocation_without_a_binding_is_lan() {
        let memory = Memory::default();
        let room = seat("member", false, vec![participant("member", None)]);
        let allocated =
            parse("1) id=1, user <171:member>:\n relay addr 10.0.0.1:8810\n Total sessions: 1\n");
        memory.label(&Query {
            server: false,
            seat: Some(room.clone()),
            reading: Reading::Ready(allocated),
        });
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(parse("Total sessions: 0\n")),
                })
                .as_deref(),
            Some("LAN")
        );
    }

    #[test]
    fn a_truncated_snapshot_does_not_erase_relayed() {
        let memory = Memory::default();
        let room = seat("member", false, vec![participant("member", None)]);
        memory.label(&Query {
            server: false,
            seat: Some(room.clone()),
            reading: Reading::Ready(parse(
                "1) id=1, user <171:member>:\n relay addr 10.0.0.1:8801\n Total sessions: 1\n",
            )),
        });
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(parse("...\n Total sessions: 400\n")),
                })
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn a_failed_read_keeps_the_last_label_and_never_invents_lan() {
        let memory = Memory::default();
        let room = seat("member", false, vec![participant("member", None)]);
        assert_eq!(
            memory.label(&Query {
                server: false,
                seat: Some(room.clone()),
                reading: Reading::Failed,
            }),
            None
        );
        memory.label(&Query {
            server: false,
            seat: Some(room.clone()),
            reading: Reading::Ready(parse(
                "1) id=1, user <171:member>:\n client addr 1.1.1.1:1, server addr 10.0.0.1:8700\n Total sessions: 1\n",
            )),
        });
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Failed,
                })
                .as_deref(),
            Some("STUN")
        );
    }

    #[test]
    fn host_breakdown_counts_every_other_peer() {
        let memory = Memory::default();
        let room = seat(
            "host",
            true,
            vec![
                participant("host", None),
                participant("a", None),
                participant("b", None),
                participant("c", None),
                participant("d", None),
            ],
        );
        let snapshot = parse(
            "\
1) id=1, user <171:a>:
 client addr 1.0.0.1:1, server addr 10.0.0.1:8700
2) id=2, user <171:b>:
 client addr 1.0.0.2:1, server addr 10.0.0.1:8700
3) id=3, user <171:c>:
 client addr 1.0.0.3:1, server addr 10.0.0.1:8700
4) id=4, user <171:d>:
 client addr 1.0.0.4:1, server addr 10.0.0.1:8700
 relay addr 10.0.0.1:8809
5) id=5, user <171:host>:
 relay addr 10.0.0.1:8811
Total sessions: 5
",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("3 STUN, 1 relayed")
        );
    }

    #[test]
    fn one_guest_is_a_single_word_and_a_member_sees_only_themselves() {
        let memory = Memory::default();
        let host = seat(
            "host",
            true,
            vec![participant("host", None), participant("guest", None)],
        );
        let guest = seat(
            "guest",
            false,
            vec![participant("host", None), participant("guest", None)],
        );
        let snapshot = parse(
            "1) id=1, user <171:guest>:\n relay addr 10.0.0.1:8802\n 2) id=2, user <171:host>:\n client addr 1.1.1.1:1, server addr 10.0.0.1:8700\n Total sessions: 2\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(host),
                    reading: Reading::Ready(snapshot.clone()),
                })
                .as_deref(),
            Some("Relayed")
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(guest),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn lan_counts_stay_in_the_breakdown() {
        let memory = Memory::default();
        let room = seat(
            "host",
            true,
            vec![
                participant("host", None),
                participant("a", None),
                participant("b", None),
            ],
        );
        let snapshot = parse(
            "1) id=1, user <171:a>:\n client addr 1.0.0.1:1, server addr 10.0.0.1:8700\n Total sessions: 1\n",
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(snapshot),
                })
                .as_deref(),
            Some("1 STUN, 1 LAN")
        );
    }

    #[test]
    fn a_waiting_host_uses_their_own_observation() {
        let memory = Memory::default();
        let room = seat("host", true, vec![participant("host", None)]);
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room.clone()),
                    reading: Reading::Ready(parse("Total sessions: 0\n")),
                })
                .as_deref(),
            Some("LAN")
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: Some(room),
                    reading: Reading::Ready(parse(
                        "1) id=1, user <171:host>:\n relay addr 10.0.0.1:8801\n Total sessions: 1\n",
                    )),
                })
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn solo_play_is_lan_and_a_session_record_is_server() {
        let memory = Memory::default();
        assert_eq!(
            memory
                .label(&Query {
                    server: false,
                    seat: None,
                    reading: Reading::NotNeeded,
                })
                .as_deref(),
            Some("LAN")
        );
        assert_eq!(
            memory
                .label(&Query {
                    server: true,
                    seat: Some(seat("member", false, vec![participant("member", None)])),
                    reading: Reading::Ready(parse(
                        "1) id=1, user <171:member>:\n relay addr 10.0.0.1:8801\n Total sessions: 1\n",
                    )),
                })
                .as_deref(),
            Some("Server")
        );
    }

    #[test]
    fn an_unconfigured_relay_does_not_pretend_the_room_is_lan() {
        let memory = Memory::default();
        assert_eq!(
            memory.label(&Query {
                server: false,
                seat: Some(seat("member", false, vec![participant("member", None)])),
                reading: Reading::Unconfigured,
            }),
            None
        );
    }

    #[test]
    fn server_requires_a_connected_link_on_a_fresh_report() {
        let link = Link {
            connection_id: "c".into(),
            runtime_id: "game".into(),
            instance_id: "instance".into(),
            connected: true,
        };
        let report = Report {
            runtime_id: "game".into(),
            instance_id: "instance".into(),
            fresh: true,
            connections: vec!["c".into()],
        };
        assert!(on_server(
            std::slice::from_ref(&link),
            std::slice::from_ref(&report)
        ));
        let stale = Report {
            fresh: false,
            ..report.clone()
        };
        assert!(!on_server(
            std::slice::from_ref(&link),
            std::slice::from_ref(&stale)
        ));
        let elsewhere = Report {
            connections: vec!["other".into()],
            ..report.clone()
        };
        assert!(!on_server(
            std::slice::from_ref(&link),
            std::slice::from_ref(&elsewhere)
        ));
        let closed = Link {
            connected: false,
            ..link.clone()
        };
        assert!(!on_server(
            std::slice::from_ref(&closed),
            std::slice::from_ref(&report)
        ));
        let replaced = Report {
            instance_id: "next".into(),
            ..report.clone()
        };
        assert!(!on_server(
            std::slice::from_ref(&link),
            std::slice::from_ref(&replaced)
        ));
        assert!(!on_server(&[], std::slice::from_ref(&report)));
        assert!(!on_server(std::slice::from_ref(&link), &[]));
    }

    #[tokio::test]
    async fn cli_reads_one_password_and_one_listing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = "\
 1) id=000000000000000009, user <171:peer-9>:
 client addr 203.0.113.9:2222, server addr 10.0.0.5:8700
 relay addr 10.0.0.5:8809
 Total sessions: 1
> ";
        let serve = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(b"\xff\xfb\x01TURN Server\r\nType '?' for help\r\nEnter password: \r\n")
                .await
                .unwrap();
            let mut got = Vec::new();
            loop {
                let mut buf = [0u8; 64];
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                got.extend(&buf[..n]);
                if got.windows(2).any(|pair| pair == b"\r\n") {
                    break;
                }
            }
            let password = String::from_utf8_lossy(&got);
            assert!(password.contains("top-secret"));
            assert!(!password.contains("ps"));
            socket.write_all(b"> ").await.unwrap();
            got.clear();
            loop {
                let mut buf = [0u8; 64];
                let n = socket.read(&mut buf).await.unwrap();
                got.extend(&buf[..n]);
                if got.windows(2).any(|pair| pair == b"ps") {
                    break;
                }
            }
            socket.write_all(body.as_bytes()).await.unwrap();
        });
        let snapshot = read_cli(port, "top-secret\n").await.unwrap();
        serve.await.unwrap();
        assert!(!snapshot.truncated);
        assert_eq!(
            snapshot.sessions,
            vec![session("171:peer-9", Some("203.0.113.9"), true)]
        );
    }

    #[tokio::test]
    async fn cli_does_not_send_the_password_again() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let serve = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.write_all(b"Enter password: \n").await.unwrap();
            let mut buf = [0u8; 64];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).contains("secret"));
            socket.write_all(b"Enter password: \n").await.unwrap();
            let again = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buf)).await;
            let n = again.expect("client should close").unwrap();
            assert_eq!(n, 0);
        });
        let error = read_cli(port, "secret").await.unwrap_err();
        assert!(error.to_string().contains("rejected"));
        serve.await.unwrap();
    }
}
