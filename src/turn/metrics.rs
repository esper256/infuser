//! Low-cardinality histograms. Peer ids never become labels.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const BOUNDS_MS: &[u64] = &[5, 10, 20, 40, 80, 120, 200, 400, 800, 1600];

pub struct Histogram {
    buckets: Vec<AtomicU64>,
    sum: AtomicU64,
    count: AtomicU64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    pub fn new() -> Self {
        Self {
            buckets: (0..=BOUNDS_MS.len()).map(|_| AtomicU64::new(0)).collect(),
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, ms: u64) {
        let index = BOUNDS_MS
            .iter()
            .position(|bound| ms <= *bound)
            .unwrap_or(BOUNDS_MS.len());
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        self.sum.fetch_add(ms, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn write(&self, out: &mut String, name: &str, labels: &str) {
        let mut cumulative = 0u64;
        for (index, bound) in BOUNDS_MS.iter().enumerate() {
            cumulative += self.buckets[index].load(Ordering::Relaxed);
            out.push_str(&format!(
                "{name}_bucket{{{labels}le=\"{bound}\"}} {cumulative}\n"
            ));
        }
        cumulative += self.buckets[BOUNDS_MS.len()].load(Ordering::Relaxed);
        out.push_str(&format!(
            "{name}_bucket{{{labels}le=\"+Inf\"}} {cumulative}\n"
        ));
        let suffix = if labels.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", labels.trim_end_matches(','))
        };
        out.push_str(&format!(
            "{name}_sum{suffix} {}\n{name}_count{suffix} {}\n",
            self.sum.load(Ordering::Relaxed),
            self.count.load(Ordering::Relaxed)
        ));
    }
}

pub struct ClientPath {
    pub relay: Histogram,
    pub stun: Histogram,
    pub lan: Histogram,
    warned: AtomicBool,
}

impl Default for ClientPath {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientPath {
    pub fn new() -> Self {
        Self {
            relay: Histogram::new(),
            stun: Histogram::new(),
            lan: Histogram::new(),
            warned: AtomicBool::new(false),
        }
    }

    pub fn observe(&self, local: Option<&str>, remote: Option<&str>, ms: u64) {
        let histogram = if local == Some("relay") || remote == Some("relay") {
            &self.relay
        } else if matches!(local, Some("srflx" | "prflx"))
            || matches!(remote, Some("srflx" | "prflx"))
        {
            &self.stun
        } else {
            &self.lan
        };
        histogram.observe(ms);
    }

    pub fn text(&self) -> String {
        let mut out = String::from("# TYPE infuser_path_rtt_ms histogram\n");
        self.relay
            .write(&mut out, "infuser_path_rtt_ms", "path=\"relay\",");
        self.stun
            .write(&mut out, "infuser_path_rtt_ms", "path=\"stun\",");
        self.lan
            .write(&mut out, "infuser_path_rtt_ms", "path=\"lan\",");
        out
    }

    pub fn publish(&self, path: &Path) {
        publish(path, &self.text(), &self.warned);
    }
}

pub struct RelayMetrics {
    pub peer_leg: Histogram,
    pub client_leg: Histogram,
    pub jitter: Histogram,
    pub tcp_rtt: Histogram,
    pub stalls: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub packets_in: AtomicU64,
    pub packets_out: AtomicU64,
    pub denied: AtomicU64,
    pub active: AtomicU64,
    pub idle: AtomicU64,
    warned: AtomicBool,
}

impl Default for RelayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayMetrics {
    pub fn new() -> Self {
        Self {
            peer_leg: Histogram::new(),
            client_leg: Histogram::new(),
            jitter: Histogram::new(),
            tcp_rtt: Histogram::new(),
            stalls: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            packets_in: AtomicU64::new(0),
            packets_out: AtomicU64::new(0),
            denied: AtomicU64::new(0),
            active: AtomicU64::new(0),
            idle: AtomicU64::new(0),
            warned: AtomicBool::new(false),
        }
    }

    pub fn text(&self) -> String {
        let mut out = String::from("# TYPE infuser_turn_leg_rtt_ms histogram\n");
        self.peer_leg
            .write(&mut out, "infuser_turn_leg_rtt_ms", "leg=\"peer\",");
        self.client_leg
            .write(&mut out, "infuser_turn_leg_rtt_ms", "leg=\"client\",");
        out.push_str("# TYPE infuser_turn_jitter_ms histogram\n");
        self.jitter.write(&mut out, "infuser_turn_jitter_ms", "");
        out.push_str("# TYPE infuser_turn_tcp_rtt_ms histogram\n");
        self.tcp_rtt.write(&mut out, "infuser_turn_tcp_rtt_ms", "");
        out.push_str("# TYPE infuser_turn_stalls_total counter\n");
        out.push_str(&format!(
            "infuser_turn_stalls_total {}\n",
            self.stalls.load(Ordering::Relaxed)
        ));
        out.push_str("# TYPE infuser_turn_bytes_total counter\n");
        out.push_str(&format!(
            "infuser_turn_bytes_total{{direction=\"in\"}} {}\ninfuser_turn_bytes_total{{direction=\"out\"}} {}\n",
            self.bytes_in.load(Ordering::Relaxed),
            self.bytes_out.load(Ordering::Relaxed)
        ));
        out.push_str("# TYPE infuser_turn_packets_total counter\n");
        out.push_str(&format!(
            "infuser_turn_packets_total{{direction=\"in\"}} {}\ninfuser_turn_packets_total{{direction=\"out\"}} {}\n",
            self.packets_in.load(Ordering::Relaxed),
            self.packets_out.load(Ordering::Relaxed)
        ));
        out.push_str("# TYPE infuser_turn_denied_total counter\n");
        out.push_str(&format!(
            "infuser_turn_denied_total {}\n",
            self.denied.load(Ordering::Relaxed)
        ));
        out.push_str("# TYPE infuser_turn_allocations gauge\n");
        out.push_str(&format!(
            "infuser_turn_allocations{{traffic=\"active\"}} {}\ninfuser_turn_allocations{{traffic=\"idle\"}} {}\n",
            self.active.load(Ordering::Relaxed),
            self.idle.load(Ordering::Relaxed)
        ));
        out
    }

    pub fn publish(&self, path: &Path) {
        publish(path, &self.text(), &self.warned);
    }
}

pub type SharedRelay = Arc<RelayMetrics>;

fn publish(path: &Path, text: &str, warned: &AtomicBool) {
    if crate::private_file::replace(path, text.as_bytes()).is_err()
        && !warned.swap(true, Ordering::Relaxed)
    {
        tracing::warn!("TURN metrics were not written");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histograms_use_path_labels_and_no_peer_id() {
        let metrics = ClientPath::new();
        metrics.observe(Some("relay"), Some("relay"), 42);
        metrics.observe(Some("srflx"), Some("host"), 12);
        metrics.observe(Some("host"), Some("host"), 3);
        let text = metrics.text();
        assert!(text.contains("path=\"relay\""));
        assert!(text.contains("path=\"stun\""));
        assert!(text.contains("path=\"lan\""));
        assert!(!text.contains("peer-"));
        assert!(text.contains("le=\"40\""));
        assert!(text.contains("le=\"+Inf\""));
    }
}
