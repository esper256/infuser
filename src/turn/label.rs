//! Words in the player bar. The server decides them.
//!
//! Server is a connected, admitted dedicated-session WebSocket. Relayed means
//! this pair's game packets went through an allocation since the last snapshot,
//! including when only the other side's allocation carried them. STUN is a
//! binding in that snapshot. LAN is everything else, including solo play.
//! Coturn cannot be read, so a peer room hides the label instead of guessing.

use std::net::IpAddr;

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Allocation {
    pub peer: String,
    pub packets: u64,
    pub remotes: Vec<IpAddr>,
    /// Peer ids reached because the remote address was this relay's public IP.
    pub reached: Vec<String>,
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
pub struct Memory;

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
        let kinds: Vec<_> = subjects(seat)
            .into_iter()
            .map(|peer| classify(peer, seat, view))
            .collect();
        Some(format_kinds(&kinds))
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

fn binding(peer: &Participant, view: &View) -> bool {
    peer.address
        .is_some_and(|address| view.bindings.contains(&address))
}

/// A pair is relayed when either allocation forwarded game packets to the
/// other person's address, or to that person's allocation on this relay.
fn in_room(seat: &Seat, allocation: &Allocation) -> bool {
    seat.peers.iter().any(|peer| peer.id == allocation.peer)
}

fn relayed(subject: &Participant, seat: &Seat, view: &View) -> bool {
    for allocation in view.allocations.iter().filter(|allocation| {
        in_room(seat, allocation) && allocation.peer == subject.id && allocation.packets > 0
    }) {
        // Game packets left this allocation toward someone else, including the
        // relay's own public address when the far side is also relayed here.
        if !allocation.reached.is_empty()
            || allocation
                .remotes
                .iter()
                .any(|ip| subject.address != Some(*ip))
        {
            return true;
        }
    }
    view.allocations.iter().any(|allocation| {
        in_room(seat, allocation)
            && allocation.packets > 0
            && allocation.peer != subject.id
            && (allocation.reached.iter().any(|id| id == &subject.id)
                || subject
                    .address
                    .is_some_and(|address| allocation.remotes.contains(&address)))
    })
}

fn classify(peer: &Participant, seat: &Seat, view: &View) -> Kind {
    if relayed(peer, seat, view) {
        return Kind::Relayed;
    }
    if binding(peer, view) {
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
        (Kind::Relayed, "relayed"),
        (Kind::Stun, "STUN"),
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
        let memory = Memory;
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
        let memory = Memory;
        let room = seat(
            "member",
            false,
            vec![
                peer("member", Some("203.0.113.8")),
                peer("host", Some("203.0.113.9")),
            ],
        );
        let dormant = View {
            bindings: vec!["203.0.113.8".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 0,
                ..Default::default()
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
                remotes: vec!["203.0.113.9".parse().unwrap()],
                ..Default::default()
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
    fn a_binding_in_the_snapshot_is_stun_and_a_missing_one_is_lan() {
        let memory = Memory;
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
            Some("LAN")
        );
    }

    #[test]
    fn a_host_with_several_peers_sees_the_breakdown() {
        let memory = Memory;
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
                remotes: vec!["203.0.113.1".parse().unwrap()],
                ..Default::default()
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(view)))
                .as_deref(),
            Some("1 relayed, 3 STUN")
        );
    }

    #[test]
    fn players_behind_one_address_share_a_binding() {
        let memory = Memory;
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
        let memory = Memory;
        let room = seat(
            "member",
            false,
            vec![
                peer("member", Some("203.0.113.4")),
                peer("host", Some("203.0.113.9")),
            ],
        );
        let stun_only = View {
            bindings: vec!["203.0.113.4".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 0,
                ..Default::default()
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
                remotes: vec!["203.0.113.9".parse().unwrap()],
                ..Default::default()
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
                ..Default::default()
            }],
        };
        assert_eq!(
            memory
                .label(&query(false, true, Some(room), Some(quiet)))
                .as_deref(),
            Some("STUN")
        );
    }

    #[test]
    fn the_host_allocation_relays_a_member_and_both_sides_say_so() {
        let memory = Memory;
        let peers = vec![
            peer("host", Some("203.0.113.1")),
            peer("member", Some("198.51.100.9")),
        ];
        let view = View {
            bindings: vec!["198.51.100.9".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "host".into(),
                packets: 2,
                remotes: vec!["198.51.100.9".parse().unwrap()],
                ..Default::default()
            }],
        };
        assert_eq!(
            memory
                .label(&query(
                    false,
                    true,
                    Some(seat("host", true, peers.clone())),
                    Some(view.clone())
                ))
                .as_deref(),
            Some("Relayed")
        );
        assert_eq!(
            memory
                .label(&query(
                    false,
                    true,
                    Some(seat("member", false, peers)),
                    Some(view)
                ))
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn a_member_allocation_relays_the_host_and_both_sides_say_so() {
        let memory = Memory;
        let peers = vec![
            peer("host", Some("203.0.113.1")),
            peer("member", Some("198.51.100.9")),
        ];
        let view = View {
            bindings: vec!["203.0.113.1".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "member".into(),
                packets: 2,
                remotes: vec!["203.0.113.1".parse().unwrap()],
                reached: vec!["host".into()],
            }],
        };
        assert_eq!(
            memory
                .label(&query(
                    false,
                    true,
                    Some(seat("host", true, peers.clone())),
                    Some(view.clone())
                ))
                .as_deref(),
            Some("Relayed")
        );
        assert_eq!(
            memory
                .label(&query(
                    false,
                    true,
                    Some(seat("member", false, peers)),
                    Some(view)
                ))
                .as_deref(),
            Some("Relayed")
        );
    }

    #[test]
    fn another_rooms_allocation_does_not_mark_a_stranger_relayed() {
        let room = seat(
            "me",
            false,
            vec![
                peer("me", Some("198.51.100.7")),
                peer("host", Some("203.0.113.1")),
            ],
        );
        let view = View {
            bindings: vec!["198.51.100.7".parse().unwrap()],
            allocations: vec![Allocation {
                peer: "someone-in-another-room".into(),
                packets: 9,
                remotes: vec!["198.51.100.7".parse().unwrap()],
                reached: vec![],
            }],
        };
        assert_eq!(
            Memory
                .label(&query(false, true, Some(room), Some(view)))
                .as_deref(),
            Some("STUN")
        );
    }
}
