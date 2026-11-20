//! Allocations and STUN bindings the player bar and admin page read.

use crate::turn::{
    label::{Allocation, View},
    metrics::SharedRelay,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PeerTraffic {
    pub peer: String,
    pub packets: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub bindings: Vec<IpAddr>,
    pub peers: Vec<PeerTraffic>,
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
                })
                .collect(),
        }
    }
}

pub struct Book {
    bindings: Mutex<BTreeSet<IpAddr>>,
    peers: Mutex<BTreeMap<u64, PeerTraffic>>,
    next: AtomicU64,
    metrics: SharedRelay,
    metrics_path: Option<PathBuf>,
    on_change: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Book {
    pub fn new(metrics: SharedRelay, metrics_path: Option<PathBuf>) -> Self {
        Self {
            bindings: Mutex::new(BTreeSet::new()),
            peers: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            metrics,
            metrics_path,
            on_change: Mutex::new(None),
        }
    }

    pub fn watch(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.on_change.lock().unwrap() = Some(hook);
    }

    pub fn note_binding(&self, ip: IpAddr) {
        let mut bindings = self.bindings.lock().unwrap();
        if !bindings.insert(ip) {
            return;
        }
        drop(bindings);
        self.changed();
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
            },
        );
        self.recount();
        self.changed();
        id
    }

    pub fn close(&self, id: u64) {
        if self.peers.lock().unwrap().remove(&id).is_none() {
            return;
        }
        self.recount();
        self.changed();
    }

    pub fn packet(&self, id: u64, bytes: u64, incoming: bool) {
        let mut peers = self.peers.lock().unwrap();
        let Some(peer) = peers.get_mut(&id) else {
            return;
        };
        let first = peer.packets == 0;
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
        let publish = first || peer.packets.is_multiple_of(32);
        drop(peers);
        if first {
            self.recount();
        }
        if publish {
            self.changed();
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            bindings: self.bindings.lock().unwrap().iter().copied().collect(),
            peers: self.peers.lock().unwrap().values().cloned().collect(),
        }
    }

    fn recount(&self) {
        let peers = self.peers.lock().unwrap();
        let active = peers.values().filter(|peer| peer.packets > 0).count() as u64;
        let idle = peers.len() as u64 - active;
        self.metrics.active.store(active, Ordering::Relaxed);
        self.metrics.idle.store(idle, Ordering::Relaxed);
    }

    fn changed(&self) {
        if let Some(path) = &self.metrics_path {
            self.metrics.publish(path);
        }
        if let Some(hook) = self.on_change.lock().unwrap().clone() {
            hook();
        }
    }
}
