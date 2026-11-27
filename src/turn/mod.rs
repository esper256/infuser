//! Embedded TURN/STUN relay. `infuser turn` is a separate process so a normal
//! controller deploy does not drop relayed games.

mod auth;
mod book;
mod bucket;
mod deny;
mod ipc;
mod label;
mod listen;
mod metrics;
mod quota;
mod relay;
mod server;
mod stun;
mod tcp;

pub use auth::credential;
pub use book::Snapshot;
pub use ipc::{Admission, Admitted, serve as serve_bridge};
pub use label::{Memory, Participant, Query, Seat, View};
pub use metrics::ClientPath;
pub use server::{Params, listen_shape, restart_needed, run, start};

pub const FINGERPRINT: &str = env!("INFUSER_TURN_FINGERPRINT");

pub fn client_metrics_path(state_dir: &std::path::Path) -> std::path::PathBuf {
    state_dir.join("turn-client.prom")
}

pub fn relay_metrics_path(state_dir: &std::path::Path) -> std::path::PathBuf {
    state_dir.join("turn-relay.prom")
}
