//! Words in the player bar. The server decides them.
//!
//! Server is a connected, admitted dedicated-session WebSocket. Relayed is an
//! embedded allocation that has forwarded packets, so a dormant backup
//! allocation does not count. STUN is a binding this relay has seen and not
//! forgotten. LAN is everything else, including solo play. Coturn cannot be
//! read, so a peer room hides the label instead of guessing.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Mutex,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub peer: String,
    pub packets: u64,
}

/// What the embedded relay last reported. `None` means it has not reported,
/// which is different from an empty report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub bindings: Vec<IpAddr>,
    pub allocations: Vec<Allocation>,
}

pub struct Query {
    pub server: bool,
    pub embedded: bool,
    pub seat: Option<Seat>,
    pub view: Option<View>,
}

#[derive(Default)]
struct Fact {
    stun: bool,
}

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
        if !query.embedded {
            return None;
        }
        let Some(view) = &query.view else {
            return None;
        };
        Some(self.observe(seat, view))
    }

    fn observe(&self, seat: &Seat, view: &View) -> String {
        let mut facts = self.peers.lock().unwrap();
        let live: BTreeSet<_> = seat.peers.iter().map(|peer| peer.id.as_str()).collect();
        facts.retain(|id, _| live.contains(id.as_str()));
        for peer in &seat.peers {
            let fact = facts.entry(peer.id.clone()).or_default();
            if binding(peer, view) {
                fact.stun = true;
            }
        }
        let kinds: Vec<_> = subjects(seat)
            .into_iter()
            .map(|peer| classify(peer, facts.get(&peer.id), view))
            .collect();
        format_kinds(&kinds)
    }
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

fn carrying(peer: &str, view: &View) -> bool {
    view.allocations
        .iter()
        .any(|allocation| allocation.peer == peer && allocation.packets > 0)
}

fn binding(peer: &Participant, view: &View) -> bool {
    peer.address
        .is_some_and(|address| view.bindings.contains(&address))
}

fn classify(peer: &Participant, fact: Option<&Fact>, view: &View) -> Kind {
    if carrying(&peer.id, view) {
        return Kind::Relayed;
    }
    if fact.is_some_and(|fact| fact.stun) || binding(peer, view) {
        return Kind::Stun;
    }
    Kind::Lan
}

fn format_kinds(kinds: &[Kind]) -> String {
    if kinds.len() == 1 {
        return kinds[0].word().to_owned();
    }
    if kinds.is_empty() {
        return Kind::Lan.word().to_owned();
    }
    let mut parts = Vec::new();
    for (kind, word) in [
        (Kind::Stun, "STUN"),
        (Kind::Relayed, "relayed"),
        (Kind::Lan, "LAN"),
    ] {
        let count = kinds.iter().filter(|item| **item == kind).count();
        if count > 0 {
            parts.push(format!("{count} {word}"));
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, address: Option<&str>) -> Participant {
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

    fn query(server: bool, embedded: bool, room: Option<Seat>, view: Option<View>) -> Query {
        Query {
            server,
            embedded,
            seat: room,
            view,
        }
    }

    #[test]
    fn server_solo_and_coturn_peer_rooms() {
        let memory = Memory::default();
        assert_eq!(
            memory.label(&query(true, false, None, None)).as_deref(),
            Some("Server")
        );
        assert_eq!(
            memory.label(&query(false, false, None, None)).as_deref(),
            Some("LAN")
        );
        let room = seat("member", false, vec![peer("member", None)]);
        assert_eq!(
            memory
                .label(&query(false, false, Some(room.clone()), None))
                .as_deref(),
            None
        );
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), None))
                .as_deref(),
            None
        );
    }

    #[test]
    fn carrying_traffic_is_relayed_and_a_dormant_allocation_is_not() {
        let memory = Memory::default();
        let room = seat("member", false, vec![peer("member", Some("203.0.113.8"))]);
        let dormant = View {
            bindings: vec!["203.0.113.8".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 0,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room.clone()), Some(dormant)))
                .as_deref(),
            Some("STUN")
        );
        let carrying = View {
            bindings: vec![],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 4,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(carrying)))
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn a_seen_binding_stays_stun_and_other_addresses_stay_lan() {
        let memory = Memory::default();
        let room = seat("member", false, vec![peer("member", Some("203.0.113.4"))]);
        let other = View {
            bindings: vec!["198.51.100.20".parse().unwrap()],
            allocations: vec![],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room.clone()), Some(other)))
                .as_deref(),
            Some("LAN")
        );
        let seen = View {
            bindings: vec!["203.0.113.4".parse().unwrap()],
            allocations: vec![],
        };
        memory.label(&query(false, true, Some(room.clone()), Some(seen)));
        let gone = View::default();
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(gone)))
                .as_deref(),
            Some("STUN")
        );
    }

    #[test]
    fn a_host_with_several_peers_sees_the_breakdown() {
        let memory = Memory::default();
        let room = seat(
            "host",
            true,
            vec![
                peer("host", Some("203.0.113.1")),
                peer("a", Some("198.51.100.1")),
                peer("b", Some("198.51.100.2")),
                peer("c", Some("198.51.100.3")),
                peer("d", Some("198.51.100.4")),
            ],
        );
        let view = View {
            bindings: vec![
                "198.51.100.1".parse().unwrap(),
                "198.51.100.2".parse().unwrap(),
                "198.51.100.3".parse().unwrap(),
            ],
            allocations: vec![Allocation {
                peer: "d".into(),
                packets: 8,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(view)))
                .as_deref(),
            Some("3 STUN, 1 relayed")
        );
    }

    #[test]
    fn players_behind_one_address_share_a_binding() {
        let memory = Memory::default();
        let room = seat(
            "host",
            true,
            vec![
                peer("host", Some("203.0.113.8")),
                peer("a", Some("198.51.100.7")),
                peer("b", Some("198.51.100.7")),
            ],
        );
        let view = View {
            bindings: vec!["198.51.100.7".parse().unwrap()],
            allocations: vec![],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(view)))
                .as_deref(),
            Some("2 STUN")
        );
    }

    #[test]
    fn a_dormant_allocation_is_not_relayed_and_a_quiet_snapshot_drops_it() {
        let memory = Memory::default();
        let room = seat("member", false, vec![peer("member", Some("203.0.113.4"))]);
        let stun_only = View {
            bindings: vec!["203.0.113.4".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 0,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room.clone()), Some(stun_only)))
                .as_deref(),
            Some("STUN")
        );
        let carrying = View {
            bindings: vec!["203.0.113.4".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 3,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room.clone()), Some(carrying)))
                .as_deref(),
            Some("Relayed")
        );
        let quiet = View {
            bindings: vec!["203.0.113.4".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 0,
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(quiet)))
                .as_deref(),
            Some("STUN")
        );
    }
}
