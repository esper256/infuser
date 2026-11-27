//! Allocations and STUN bindings the player bar and admin page read.

use crate::turn::{
    label::{Allocation, View},
    metrics::SharedRelay,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerTraffic {
    pub peer: String,
    pub packets: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    #[serde(default)]
    pub remotes: Vec<std::net::IpAddr>,
    #[serde(default)]
    pub reached: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub bindings: Vec<IpAddr>,
    pub peers: Vec<PeerTraffic>,
    /// Unix seconds when the daemon stored this snapshot. Zero on the relay.
    #[serde(skip)]
    pub at: u64,
    /// Bridge connection that produced it. Zero on the relay.
    #[serde(skip)]
    pub conn: u64,
}

impl Snapshot {
    pub fn is_current(&self, now: u64) -> bool {
        self.at != 0 && now.saturating_sub(self.at) <= 15
    }
}

impl Snapshot {
    pub fn label_view(&self) -> View {
        View {
            bindings: self.bindings.clone(),
            allocations: self
                .peers
                .iter()
                .map(|peer| Allocation {
                    peer: peer.peer.clone(),
                    packets: peer.packets,
                    remotes: peer.remotes.clone(),
                    reached: peer.reached.clone(),
                })
                .collect(),
        }
    }
}

pub struct Book {
    bindings: Mutex<BTreeSet<IpAddr>>,
    allowed: Mutex<BTreeSet<IpAddr>>,
    peers: Mutex<BTreeMap<u64, PeerTraffic>>,
    ports: Mutex<BTreeMap<u16, u64>>,
    public_ip: Mutex<Option<IpAddr>>,
    next: AtomicU64,
    metrics: SharedRelay,
    metrics_path: Option<PathBuf>,
    latest: Mutex<Snapshot>,
}

impl Book {
    pub fn new(metrics: SharedRelay, metrics_path: Option<PathBuf>) -> Self {
        Self {
            bindings: Mutex::new(BTreeSet::new()),
            allowed: Mutex::new(BTreeSet::new()),
            peers: Mutex::new(BTreeMap::new()),
            ports: Mutex::new(BTreeMap::new()),
            public_ip: Mutex::new(None),
            next: AtomicU64::new(1),
            metrics,
            metrics_path,
            latest: Mutex::new(Snapshot::default()),
        }
    }

    pub fn set_public_ip(&self, ip: IpAddr) {
        *self.public_ip.lock().unwrap() = Some(ip);
    }

    /// Bindings outside this set are ignored, and ones that leave it are dropped.
    pub fn allow(&self, ips: BTreeSet<IpAddr>) {
        *self.allowed.lock().unwrap() = ips.clone();
        self.bindings.lock().unwrap().retain(|ip| ips.contains(ip));
    }

    pub fn note_binding(&self, ip: IpAddr) {
        if !self.allowed.lock().unwrap().contains(&ip) {
            return;
        }
        self.bindings.lock().unwrap().insert(ip);
    }

    pub fn open(&self, peer: String) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.peers.lock().unwrap().insert(
            id,
            PeerTraffic {
                peer,
                packets: 0,
                bytes_in: 0,
                bytes_out: 0,
                remotes: Vec::new(),
                reached: Vec::new(),
            },
        );
        id
    }

    pub fn bind_port(&self, id: u64, port: u16) {
        self.ports.lock().unwrap().insert(port, id);
    }

    pub fn close(&self, id: u64) {
        self.peers.lock().unwrap().remove(&id);
        self.ports.lock().unwrap().retain(|_, owner| *owner != id);
    }

    pub fn touch(&self, id: u64, remote: std::net::SocketAddr) {
        let public = *self.public_ip.lock().unwrap();
        let other = if Some(remote.ip()) == public {
            self.ports.lock().unwrap().get(&remote.port()).copied()
        } else {
            None
        };
        let mut peers = self.peers.lock().unwrap();
        let other_name = other.and_then(|other| peers.get(&other).map(|peer| peer.peer.clone()));
        let Some(peer) = peers.get_mut(&id) else {
            return;
        };
        if !peer.remotes.contains(&remote.ip()) {
            peer.remotes.push(remote.ip());
        }
        if let Some(name) = other_name
            && name != peer.peer
            && !peer.reached.contains(&name)
        {
            peer.reached.push(name);
        }
    }

    /// Application traffic only. The caller does not pass STUN.
    pub fn packet(&self, id: u64, bytes: u64, incoming: bool) {
        let mut peers = self.peers.lock().unwrap();
        let Some(peer) = peers.get_mut(&id) else {
            return;
        };
        peer.packets += 1;
        if incoming {
            peer.bytes_in += bytes;
            self.metrics.bytes_in.fetch_add(bytes, Ordering::Relaxed);
            self.metrics.packets_in.fetch_add(1, Ordering::Relaxed);
        } else {
            peer.bytes_out += bytes;
            self.metrics.bytes_out.fetch_add(bytes, Ordering::Relaxed);
            self.metrics.packets_out.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            bindings: self.bindings.lock().unwrap().iter().copied().collect(),
            peers: self.peers.lock().unwrap().values().cloned().collect(),
            ..Default::default()
        }
    }

    /// Interval snapshot. Packet counters reset so a quiet interval is not Relayed.
    pub fn export(&self) -> Snapshot {
        let bindings = self.bindings.lock().unwrap().iter().copied().collect();
        let mut peers = self.peers.lock().unwrap();
        let listed: Vec<_> = peers.values().cloned().collect();
        let active = listed.iter().filter(|peer| peer.packets > 0).count() as u64;
        let idle = listed.len() as u64 - active;
        self.metrics.active.store(active, Ordering::Relaxed);
        self.metrics.idle.store(idle, Ordering::Relaxed);
        for peer in peers.values_mut() {
            peer.packets = 0;
            peer.bytes_in = 0;
            peer.bytes_out = 0;
            peer.remotes.clear();
            peer.reached.clear();
        }
        drop(peers);
        if let Some(path) = &self.metrics_path {
            self.metrics.publish(path);
        }
        let snapshot = Snapshot {
            bindings,
            peers: listed,
            ..Default::default()
        };
        *self.latest.lock().unwrap() = snapshot.clone();
        snapshot
    }

    pub fn latest(&self) -> Snapshot {
        self.latest.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::metrics::RelayMetrics;
    use std::{net::Ipv4Addr, sync::Arc};

    #[test]
    fn spoofed_bindings_do_not_grow_and_a_quiet_interval_drops_relayed() {
        let book = Book::new(Arc::new(RelayMetrics::new()), None);
        let allowed = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8));
        let spoofed = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9));
        book.allow([allowed].into());
        book.note_binding(allowed);
        for host in 1..200 {
            book.note_binding(IpAddr::V4(Ipv4Addr::new(198, 51, 100, host)));
        }
        book.note_binding(spoofed);
        assert_eq!(book.snapshot().bindings, vec![allowed]);
        book.allow(BTreeSet::new());
        assert!(book.snapshot().bindings.is_empty());
        book.allow([allowed].into());
        let id = book.open("alpha".into());
        book.packet(id, 8, true);
        let carrying = book.export();
        assert_eq!(carrying.peers[0].packets, 1);
        let quiet = book.export();
        assert_eq!(quiet.peers[0].packets, 0);
    }
}
