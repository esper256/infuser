//! Unix-socket conversation between the long-lived relay and the daemon.
//! The relay asks whether a peer is still admitted and pushes its view.
//! A missing daemon rejects new allocations and leaves existing ones alone.

use crate::turn::book::Snapshot;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

#[derive(Serialize, Deserialize)]
struct Traffic {
    peer: String,
    packets: u64,
    bytes_in: u64,
    bytes_out: u64,
}

#[derive(Serialize, Deserialize)]
struct ViewBody {
    bindings: Vec<String>,
    allocations: Vec<Traffic>,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Outbound<'a> {
    Admit {
        peer: &'a str,
    },
    View {
        bindings: Vec<String>,
        allocations: Vec<Traffic>,
    },
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
}

enum Job {
    Admit {
        peer: String,
        reply: mpsc::Sender<bool>,
    },
    View(Snapshot),
}

pub fn remote_admit(path: PathBuf) -> Arc<dyn Fn(&str) -> bool + Send + Sync> {
    let gate = Gate::connect(path);
    Arc::new(move |peer| gate.admit(peer))
}

pub struct Gate {
    tx: mpsc::SyncSender<Job>,
}

impl Gate {
    pub fn connect(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel(16);
        std::thread::Builder::new()
            .name("infuser-turn-ipc".into())
            .spawn(move || pump(path, rx))
            .expect("turn ipc thread");
        Self { tx }
    }

    pub fn admit(&self, peer: &str) -> bool {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .tx
            .send(Job::Admit {
                peer: peer.to_owned(),
                reply: reply_tx,
            })
            .is_err()
        {
            return false;
        }
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(false)
    }

    pub fn publish(&self, snapshot: Snapshot) {
        let _ = self.tx.try_send(Job::View(snapshot));
    }
}

fn pump(path: PathBuf, rx: mpsc::Receiver<Job>) {
    let mut stream = connect(&path);
    while let Ok(job) = rx.recv() {
        if stream.is_none() {
            stream = connect(&path);
        }
        if !exchange(stream.as_mut(), &job) {
            stream = connect(&path);
            if !exchange(stream.as_mut(), &job) {
                stream = None;
                if let Job::Admit { reply, .. } = job {
                    let _ = reply.send(false);
                }
            }
        }
    }
}

fn connect(path: &Path) -> Option<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .ok()?;
    Some(stream)
}

/// True when the daemon answered. Admission itself may still be false.
fn exchange(stream: Option<&mut std::os::unix::net::UnixStream>, job: &Job) -> bool {
    let Some(stream) = stream else {
        return false;
    };
    let encoded = match job {
        Job::Admit { peer, .. } => serde_json::to_vec(&Outbound::Admit { peer }).ok(),
        Job::View(snapshot) => serde_json::to_vec(&Outbound::View {
            bindings: snapshot.bindings.iter().map(ToString::to_string).collect(),
            allocations: snapshot
                .peers
                .iter()
                .map(|peer| Traffic {
                    peer: peer.peer.clone(),
                    packets: peer.packets,
                    bytes_in: peer.bytes_in,
                    bytes_out: peer.bytes_out,
                })
                .collect(),
        })
        .ok(),
    };
    let Some(mut body) = encoded else {
        return false;
    };
    body.push(b'\n');
    if stream.write_all(&body).is_err() {
        return false;
    }
    let Ok(line) = read_line(stream) else {
        return false;
    };
    let ok = serde_json::from_str::<Reply>(line.trim()).is_ok_and(|reply| reply.ok);
    if let Job::Admit { reply, .. } = job {
        let _ = reply.send(ok);
    }
    true
}

fn read_line(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        if line.len() == 65_536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "turn ipc line",
            ));
        }
        line.push(byte[0]);
    }
    String::from_utf8(line)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "turn ipc"))
}

pub fn serve(
    path: &Path,
    admitted: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    view: Arc<Mutex<Option<Snapshot>>>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(listener)?;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let admitted = Arc::clone(&admitted);
            let view = Arc::clone(&view);
            tokio::spawn(async move {
                let _ = session(stream, admitted, view).await;
            });
        }
    });
    Ok(())
}

async fn session(
    stream: UnixStream,
    admitted: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    view: Arc<Mutex<Option<Snapshot>>>,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let response = answer(&line, &admitted, &view);
        write.write_all(response.as_bytes()).await?;
        write.write_all(b"\n").await?;
    }
    Ok(())
}

fn answer(
    line: &str,
    admitted: &Arc<dyn Fn(&str) -> bool + Send + Sync>,
    view: &Mutex<Option<Snapshot>>,
) -> String {
    let value: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
    let ok = match value.get("op").and_then(|op| op.as_str()) {
        Some("admit") => value
            .get("peer")
            .and_then(|peer| peer.as_str())
            .is_some_and(|peer| admitted(peer)),
        Some("view") => {
            if let Ok(body) = serde_json::from_value::<ViewBody>(value) {
                *view.lock().unwrap() = Some(Snapshot {
                    bindings: body
                        .bindings
                        .iter()
                        .filter_map(|ip| ip.parse().ok())
                        .collect(),
                    peers: body
                        .allocations
                        .into_iter()
                        .map(|peer| crate::turn::book::PeerTraffic {
                            peer: peer.peer,
                            packets: peer.packets,
                            bytes_in: peer.bytes_in,
                            bytes_out: peer.bytes_out,
                        })
                        .collect(),
                });
                true
            } else {
                false
            }
        }
        _ => false,
    };
    format!(r#"{{"ok":{ok}}}"#)
}
