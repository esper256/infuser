//! Embedded TURN server. One process, the same secret and ports as coturn.

use crate::{
    config::{Config, TurnEngine},
    turn::{
        auth::RestAuth,
        book::{Book, Snapshot},
        ipc::Admission,
        listen::UdpListener,
        metrics::RelayMetrics,
        relay::{Generator, Pool},
        tcp::TcpTurn,
    },
};
use anyhow::{Context, Result, bail};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::UdpSocket;
use turn::server::{
    Server,
    config::{ConnConfig, ServerConfig},
};

pub struct Params {
    pub secret: Vec<u8>,
    pub realm: String,
    pub listen_ip: Ipv4Addr,
    pub listen_port: u16,
    pub public_ip: Ipv4Addr,
    pub relay_start: u16,
    pub relay_end: u16,
    pub udp: bool,
    pub tcp: bool,
    pub admit: Arc<Admission>,
    pub metrics_path: Option<std::path::PathBuf>,
    pub socket: Option<std::path::PathBuf>,
}

pub const OTHER_ENGINE_EXIT: i32 = 78;

pub struct Running {
    pub port: u16,
    servers: Vec<Arc<Server>>,
    book: Arc<Book>,
    metrics: Arc<RelayMetrics>,
    pub admission_generation: tokio::sync::watch::Receiver<u64>,
}

impl Running {
    pub fn snapshot(&self) -> Snapshot {
        self.book.snapshot()
    }

    pub fn metrics(&self) -> &RelayMetrics {
        &self.metrics
    }

    pub async fn close(self) -> Result<()> {
        for server in self.servers {
            server
                .close()
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        Ok(())
    }
}

pub async fn start(params: Params) -> Result<Running> {
    if !params.udp && !params.tcp {
        bail!("embedded TURN needs a UDP or TCP listener");
    }
    if params.secret.len() < 32 {
        bail!("TURN shared secret is too short");
    }
    let metrics = Arc::new(RelayMetrics::new());
    let book = Arc::new(Book::new(Arc::clone(&metrics), params.metrics_path.clone()));
    book.set_public_ip(std::net::IpAddr::V4(params.public_ip));
    let mut nonce_key = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce_key)
        .map_err(|error| anyhow::anyhow!("TURN nonce key: {error}"))?;
    turn::server::ensure_nonce_key(&nonce_key).map_err(|error| anyhow::anyhow!(error))?;
    turn::server::set_relay_hook(crate::turn::relay::account_relay);
    let exporting = Arc::clone(&book);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await;
        loop {
            tick.tick().await;
            exporting.export();
        }
    });
    let (leave_tx, mut leave_rx) = tokio::sync::mpsc::unbounded_channel();
    let (generation_tx, generation_rx) = tokio::sync::watch::channel(0);
    if let Some(path) = params.socket.clone() {
        crate::turn::ipc::link(
            path,
            Arc::clone(&params.admit),
            Arc::clone(&book),
            leave_tx,
            generation_tx,
        );
    }
    crate::turn::deny::install(
        std::net::IpAddr::V4(params.public_ip),
        params.relay_start,
        params.relay_end,
    );
    let auth: Arc<dyn turn::auth::AuthHandler + Send + Sync> = Arc::new(RestAuth::new(
        params.secret.clone(),
        Arc::clone(&params.admit),
    ));
    let pool = Arc::new(Pool::new(
        params.listen_ip,
        params.public_ip,
        params.relay_start,
        params.relay_end,
        Arc::clone(&book),
        Arc::clone(&metrics),
    ));
    let mut port = params.listen_port;
    let mut servers = Vec::new();
    if params.udp {
        let socket = UdpSocket::bind((params.listen_ip, params.listen_port)).await?;
        port = socket.local_addr()?.port();
        let pending = Arc::new(Mutex::new(None));
        let server = Server::new(ServerConfig {
            conn_configs: vec![ConnConfig {
                conn: Arc::new(UdpListener::new(
                    socket,
                    Arc::clone(&pending),
                    Arc::clone(&book),
                )),
                relay_addr_generator: Box::new(Generator::new(Arc::clone(&pool), pending)),
            }],
            realm: params.realm.clone(),
            auth_handler: Arc::clone(&auth),
            channel_bind_timeout: Duration::from_secs(0),
            alloc_close_notify: None,
        })
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
        servers.push(Arc::new(server));
    }
    if params.tcp {
        let pending = Arc::new(Mutex::new(None));
        let tcp = TcpTurn::bind(
            SocketAddr::from((params.listen_ip, port)),
            Arc::clone(&pending),
            Arc::clone(&book),
            Arc::clone(&metrics),
        )
        .await?;
        port = tcp.port();
        let finished = tcp.take_finished();
        let server = Arc::new(
            Server::new(ServerConfig {
                conn_configs: vec![ConnConfig {
                    conn: Arc::new(tcp),
                    relay_addr_generator: Box::new(Generator::new(Arc::clone(&pool), pending)),
                }],
                realm: params.realm,
                auth_handler: auth,
                channel_bind_timeout: Duration::from_secs(0),
                alloc_close_notify: None,
            })
            .await
            .map_err(|error| anyhow::anyhow!(error))?,
        );
        if let Some(mut finished) = finished {
            let server = Arc::clone(&server);
            tokio::spawn(async move {
                while let Some(addr) = finished.recv().await {
                    let _ = server.delete_allocations_from(addr).await;
                }
            });
        }
        servers.push(server);
    }
    let dropping = servers.clone();
    tokio::spawn(async move {
        while let Some(peer) = leave_rx.recv().await {
            for server in &dropping {
                let _ = server.delete_allocations_by_peer(&peer).await;
            }
        }
    });
    Ok(Running {
        port,
        servers,
        book,
        metrics,
        admission_generation: generation_rx,
    })
}

pub async fn run(config: &Path) -> Result<()> {
    let loaded = Config::load(config)?;
    let turn = loaded
        .controller
        .turn
        .as_ref()
        .context("embedded TURN is not configured")?;
    if turn.engine != TurnEngine::Embedded {
        tracing::error!(
            "controller.turn.engine is not embedded; the coturn container keeps these ports"
        );
        std::process::exit(OTHER_ENGINE_EXIT);
    }
    let secret = std::fs::read_to_string(&turn.secret_file)?;
    let secret = secret.trim().as_bytes().to_vec();
    let (listen_port, udp, tcp) = listen_shape(&turn.urls)?;
    let socket = turn
        .socket
        .clone()
        .unwrap_or_else(|| loaded.controller.state_dir.join("turn.sock"));
    let admit = Admission::new();
    let metrics_path = loaded.controller.state_dir.join("turn-relay.prom");
    let running = start(Params {
        secret,
        realm: loaded.controller.domain.clone(),
        listen_ip: turn.listen_ip.context("embedded TURN needs listen_ip")?,
        listen_port,
        public_ip: turn.public_ip.context("embedded TURN needs public_ip")?,
        relay_start: turn.relay_port_start,
        relay_end: turn.relay_port_end,
        udp,
        tcp,
        admit,
        metrics_path: Some(metrics_path),
        socket: Some(socket),
    })
    .await?;
    let fingerprint = loaded.controller.state_dir.join("turn.fingerprint");
    if let Err(error) =
        crate::private_file::replace(fingerprint.as_path(), super::FINGERPRINT.as_bytes())
    {
        tracing::error!(%error, "embedded relay fingerprint was not saved");
        running
            .metrics()
            .fingerprint_errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    tracing::info!(port = running.port, "embedded TURN listening");
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result?,
        _ = terminate.recv() => {}
    }
    running.close().await?;
    Ok(())
}

pub fn listen_shape(urls: &[String]) -> Result<(u16, bool, bool)> {
    let mut port = None;
    let mut udp = false;
    let mut tcp = false;
    for url in urls {
        let endpoint = url
            .strip_prefix("turn:")
            .or_else(|| url.strip_prefix("turns:"))
            .context("TURN URL")?;
        let (address, transport) = endpoint
            .split_once('?')
            .unwrap_or((endpoint, "transport=udp"));
        let found: u16 = address
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .context("TURN URL port")?;
        if let Some(current) = port
            && current != found
        {
            bail!("TURN URLs must share one listener port");
        }
        port = Some(found);
        match transport {
            "transport=udp" => udp = true,
            "transport=tcp" => tcp = true,
            _ => bail!("invalid TURN transport"),
        }
    }
    Ok((port.context("TURN listener port")?, udp, tcp))
}

/// The updater restarts the relay only when this release's relay code differs.
pub fn restart_needed(previous: Option<&str>, current: Option<&str>, embedded: bool) -> bool {
    embedded && current.is_some() && previous != current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::auth::credential;
    use std::net::Ipv4Addr;
    use turn::client::{Client, ClientConfig};
    use util::Conn;

    #[test]
    fn the_unit_does_not_restart_when_this_is_not_the_embedded_engine() {
        let unit = include_str!("../../deploy/infuser-turn.service");
        assert!(unit.contains("RestartPreventExitStatus=78"));
        assert!(unit.contains("StartLimitIntervalSec=60"));
        assert!(unit.contains("StartLimitBurst=5"));
        assert_eq!(OTHER_ENGINE_EXIT, 78);
    }

    #[test]
    fn fingerprint_changes_are_the_only_relay_restarts() {
        assert!(!restart_needed(Some("a"), Some("a"), true));
        assert!(restart_needed(Some("a"), Some("b"), true));
        assert!(restart_needed(None, Some("b"), true));
        assert!(!restart_needed(Some("a"), Some("b"), false));
        assert!(!restart_needed(None, None, true));
    }

    #[tokio::test]
    async fn allocate_permission_and_relay() {
        let secret = b"0123456789abcdef0123456789abcdef".to_vec();
        let running = start(Params {
            secret: secret.clone(),
            realm: "turn.test".into(),
            listen_ip: Ipv4Addr::LOCALHOST,
            listen_port: 0,
            public_ip: Ipv4Addr::LOCALHOST,
            relay_start: 34_200,
            relay_end: 34_220,
            udp: true,
            tcp: true,
            admit: {
                let admitted = Admission::new();
                admitted.replace(["alpha".into(), "beta".into()].into());
                admitted
            },
            metrics_path: None,
            socket: None,
        })
        .await
        .unwrap();
        let port = running.port;
        let expires = crate::controller::now() + 3600;
        let alpha = client(&secret, &format!("{expires}:alpha"), port).await;
        let beta = client(&secret, &format!("{expires}:beta"), port).await;
        let alpha_relay = alpha.allocate().await.unwrap();
        let beta_relay = beta.allocate().await.unwrap();
        let beta_addr = beta_relay.local_addr().unwrap();
        let alpha_addr = alpha_relay.local_addr().unwrap();
        alpha_relay.send_to(b"one", beta_addr).await.unwrap();
        beta_relay.send_to(b"two", alpha_addr).await.unwrap();
        let mut buf = [0u8; 16];
        let (size, _) = alpha_relay.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], b"two");
        alpha_relay.send_to(b"one", beta_addr).await.unwrap();
        let (size, _) = beta_relay.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], b"one");
        let carrying = running
            .snapshot()
            .peers
            .iter()
            .filter(|peer| peer.packets > 0)
            .count();
        assert!(carrying >= 1);
        let rejected = client(&secret, &format!("{expires}:gamma"), port).await;
        assert!(rejected.allocate().await.is_err());
        running.close().await.unwrap();
    }

    async fn client(secret: &[u8], username: &str, port: u16) -> Client {
        let socket = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap());
        let client = Client::new(ClientConfig {
            stun_serv_addr: format!("127.0.0.1:{port}"),
            turn_serv_addr: format!("127.0.0.1:{port}"),
            username: username.into(),
            password: credential(secret, username),
            realm: String::new(),
            software: String::new(),
            rto_in_ms: 100,
            conn: socket,
            vnet: None,
        })
        .await
        .unwrap();
        client.listen().await.unwrap();
        client
    }
}
