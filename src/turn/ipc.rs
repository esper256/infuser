//! The controller pushes the admitted-peer set. The relay checks that set in
//! memory. Snapshots go the other way: a full one when the socket connects, and
//! another on the export interval. Nothing on this socket runs inside the
//! packet read loop.

use crate::turn::book::{Book, PeerTraffic, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::watch,
    time::{Interval, MissedTickBehavior},
};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admitted {
    pub ids: BTreeSet<String>,
    pub ips: BTreeSet<IpAddr>,
}

/// Local copy of the daemon's admitted peers. `allows` only reads this set.
#[derive(Default)]
pub struct Admission {
    ids: Mutex<BTreeSet<String>>,
}

impl Admission {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn allows(&self, peer: &str) -> bool {
        self.ids.lock().unwrap().contains(peer)
    }

    pub fn replace(&self, ids: BTreeSet<String>) {
        *self.ids.lock().unwrap() = ids;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Message {
    v: u32,
    op: String,
    #[serde(default)]
    ids: BTreeSet<String>,
    #[serde(default)]
    ips: BTreeSet<IpAddr>,
    #[serde(default)]
    bindings: Vec<IpAddr>,
    #[serde(default)]
    allocations: Vec<PeerTraffic>,
}

fn line(message: &Message) -> String {
    let mut encoded = serde_json::to_string(message).expect("ipc message");
    encoded.push('\n');
    encoded
}

/// Relay side. One attempt when the interval comes around; the read loop
/// applies a pushed set as soon as it arrives.
pub fn link(path: PathBuf, admitted: Arc<Admission>, book: Arc<Book>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    if drive_relay(stream, &mut interval, &admitted, &book)
                        .await
                        .is_err()
                    {
                        interval.tick().await;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "TURN relay IPC is down");
                    interval.tick().await;
                }
            }
        }
    });
}

async fn drive_relay(
    stream: UnixStream,
    interval: &mut Interval,
    admitted: &Admission,
    book: &Book,
) -> Result<(), ()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    send(
        &mut write,
        &Message {
            v: VERSION,
            op: "hello".into(),
            ..empty()
        },
    )
    .await?;
    let hello = lines.next_line().await.map_err(|_| ())?.ok_or(())?;
    if apply_peers(&hello, admitted, book).is_err() {
        return Err(());
    }
    // A reconnect starts with the whole snapshot, not a delta.
    write_snapshot(&mut write, book).await?;
    loop {
        tokio::select! {
            incoming = lines.next_line() => {
                match incoming {
                    Ok(Some(incoming)) => {
                        if apply_peers(&incoming, admitted, book).is_err() {
                            return Err(());
                        }
                    }
                    _ => return Err(()),
                }
            }
            _ = interval.tick() => write_snapshot(&mut write, book).await?,
        }
    }
}

fn apply_peers(text: &str, admitted: &Admission, book: &Book) -> Result<(), ()> {
    let message: Message = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            tracing::error!(%error, "TURN IPC message was not understood");
            admitted.replace(BTreeSet::new());
            book.allow(BTreeSet::new());
            return Err(());
        }
    };
    if message.v != VERSION || message.op != "peers" {
        tracing::error!(v = message.v, op = %message.op, "TURN IPC version mismatch");
        admitted.replace(BTreeSet::new());
        book.allow(BTreeSet::new());
        return Err(());
    }
    admitted.replace(message.ids);
    book.allow(message.ips);
    Ok(())
}

async fn write_snapshot(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    book: &Book,
) -> Result<(), ()> {
    let snapshot = book.export();
    send(
        write,
        &Message {
            v: VERSION,
            op: "view".into(),
            bindings: snapshot.bindings,
            allocations: snapshot.peers,
            ..empty()
        },
    )
    .await
}

fn empty() -> Message {
    Message {
        v: VERSION,
        op: String::new(),
        ids: BTreeSet::new(),
        ips: BTreeSet::new(),
        bindings: Vec::new(),
        allocations: Vec::new(),
    }
}

/// Daemon side. Sends the full admitted set on connect and whenever it changes.
pub fn serve(
    path: &Path,
    roster: watch::Receiver<Admitted>,
    view: Arc<Mutex<Option<Snapshot>>>,
) -> std::io::Result<()> {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(listener)?;
    let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let roster = roster.clone();
            let view = Arc::clone(&view);
            tokio::spawn(async move {
                let _ = drive_daemon(stream, roster, view).await;
            });
        }
    });
    Ok(())
}

async fn drive_daemon(
    stream: UnixStream,
    mut roster: watch::Receiver<Admitted>,
    view: Arc<Mutex<Option<Snapshot>>>,
) -> Result<(), ()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let hello = lines.next_line().await.map_err(|_| ())?.ok_or(())?;
    let hello: Message = serde_json::from_str(&hello).map_err(|error| {
        tracing::error!(%error, "TURN IPC hello was not understood");
    })?;
    if hello.v != VERSION || hello.op != "hello" {
        tracing::error!(v = hello.v, op = %hello.op, "TURN IPC version mismatch");
        return Err(());
    }
    let current = roster.borrow_and_update().clone();
    write_peers(&mut write, &current).await?;
    loop {
        tokio::select! {
            incoming = lines.next_line() => {
                match incoming {
                    Ok(Some(incoming)) => {
                        if store_view(&incoming, &view).is_err() {
                            return Err(());
                        }
                    }
                    _ => return Err(()),
                }
            }
            changed = roster.changed() => {
                if changed.is_err() {
                    return Err(());
                }
                let current = roster.borrow_and_update().clone();
                write_peers(&mut write, &current).await?;
            }
        }
    }
}

async fn write_peers(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    admitted: &Admitted,
) -> Result<(), ()> {
    send(
        write,
        &Message {
            v: VERSION,
            op: "peers".into(),
            ids: admitted.ids.clone(),
            ips: admitted.ips.clone(),
            ..empty()
        },
    )
    .await
}

async fn send(write: &mut tokio::net::unix::OwnedWriteHalf, message: &Message) -> Result<(), ()> {
    let encoded = line(message);
    write.write_all(encoded.as_bytes()).await.map_err(|_| ())
}

fn store_view(text: &str, view: &Mutex<Option<Snapshot>>) -> Result<(), ()> {
    let message: Message = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            tracing::error!(%error, "TURN IPC snapshot was not understood");
            return Err(());
        }
    };
    if message.v != VERSION || message.op != "view" {
        tracing::error!(v = message.v, op = %message.op, "TURN IPC version mismatch");
        return Err(());
    }
    *view.lock().unwrap() = Some(Snapshot {
        bindings: message.bindings,
        peers: message.allocations,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::metrics::RelayMetrics;

    #[test]
    fn a_version_mismatch_clears_the_admitted_set() {
        let admitted = Admission::new();
        admitted.replace(["alpha".into()].into());
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(metrics, None));
        book.allow(["203.0.113.8".parse().unwrap()].into());
        let stale = r#"{"v":2,"op":"peers","ids":["alpha"],"ips":["203.0.113.8"]}"#;
        assert!(apply_peers(stale, &admitted, &book).is_err());
        assert!(!admitted.allows("alpha"));
        assert!(book.snapshot().bindings.is_empty());
    }

    #[tokio::test]
    async fn reconnect_delivers_the_snapshot_and_the_admitted_set() {
        let admitted = Admission::new();
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        book.allow(["203.0.113.8".parse().unwrap()].into());
        book.note_binding("203.0.113.8".parse().unwrap());
        let id = book.open("alpha".into());
        book.packet(id, 4, true);
        let (daemon_stream, relay_stream) = UnixStream::pair().unwrap();
        let (tx, rx) = watch::channel(Admitted {
            ids: ["alpha".into()].into(),
            ips: ["203.0.113.8".parse().unwrap()].into(),
        });
        let view = Arc::new(Mutex::new(None));
        let stored = Arc::clone(&view);
        let daemon = tokio::spawn(drive_daemon(daemon_stream, rx, stored));
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        let admitted_relay = Arc::clone(&admitted);
        let book_relay = Arc::clone(&book);
        let relay = tokio::spawn(async move {
            drive_relay(relay_stream, &mut interval, &admitted_relay, &book_relay).await
        });
        let snapshot = wait_view(&view).await;
        assert_eq!(
            snapshot.bindings,
            vec!["203.0.113.8".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(snapshot.peers.len(), 1);
        assert_eq!(snapshot.peers[0].packets, 1);
        assert!(admitted.allows("alpha"));
        drop(tx);
        relay.abort();
        daemon.abort();
    }

    async fn wait_view(view: &Mutex<Option<Snapshot>>) -> Snapshot {
        for _ in 0..10_000 {
            if let Some(snapshot) = view.lock().unwrap().clone() {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
        panic!("relay snapshot was not delivered");
    }
}
