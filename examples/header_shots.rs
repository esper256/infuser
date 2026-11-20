//! Serves the real player page with labels computed from a live relay.
//! Loopback only. Not part of the daemon.

use anyhow::{Result, bail};
use axum::{Router, extract::Request, middleware::Next};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build, Launch},
    portal,
    turn::credential,
};
use serde_json::json;
use std::{net::Ipv4Addr, sync::Arc};
use tokio::net::UdpSocket;
use turn::client::{Client, ClientConfig};
use util::Conn;

#[tokio::main]
async fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let secret_path = dir.path().join("turn.secret");
    let secret = b"fixture-only-turn-secret-0123456789".to_vec();
    std::fs::write(&secret_path, &secret)?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\n[controller.turn]\nengine='embedded'\nlisten_ip='127.0.0.1'\npublic_ip='127.0.0.1'\nsecret_file={:?}\nurls=['turn:127.0.0.1:18710?transport=udp']\nrelay_port_start=18900\nrelay_port_end=18940",
        dir.path().display().to_string(),
        secret_path.display().to_string()
    ))?;
    platform::member(&cfg, "fixture@example.com", "Fixture", true, true)?;
    let socket = dir.path().join("turn.sock");
    let platform = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("peerjs-fixture/infuser.game.json"))?;
    manifest["title"] = json!("Nightglass");
    manifest["schema_version"] = json!(1);
    manifest["app_id"] = json!("header");
    platform.db.edit(|data| {
        data.builds.insert(
            "header/test".into(),
            Build {
                manifest: serde_json::from_value(manifest)?,
                server: None,
                spa: false,
                cross_origin_isolation: false,
                ready: true,
                created: now(),
                files: Default::default(),
            },
        );
        data.apps.insert(
            "header".into(),
            App {
                publication: None,
                build: "test".into(),
                listed: false,
                updated: now(),
            },
        );
        for id in [
            "lan", "server", "stun", "relayed", "host", "a", "b", "c", "d",
        ] {
            data.launches.insert(
                id.into(),
                Launch {
                    deployment: None,
                    visit: None,
                    intent: None,
                    app: "header".into(),
                    build: "test".into(),
                    email: "fixture@example.com".into(),
                    guest: true,
                    host: format!("{id}.games.example.com"),
                    token_hash: platform::hash(id),
                    exchanged: true,
                    expires: now() + 600,
                    grant_expires: now() + 600,
                    seen: now(),
                    lease_id: None,
                    lease_token: None,
                },
            );
        }
        Ok(())
    })?;
    platform.note_connected_session("server", "header");
    platform.listen_turn_bridge()?;
    let _relay = infuser::turn::start(infuser::turn::Params {
        secret: secret.clone(),
        realm: cfg.controller.domain.clone(),
        listen_ip: Ipv4Addr::LOCALHOST,
        listen_port: 18710,
        public_ip: Ipv4Addr::LOCALHOST,
        relay_start: 18900,
        relay_end: 18940,
        udp: true,
        tcp: false,
        admit: infuser::turn::Admission::new(),
        metrics_path: None,
        socket: Some(socket),
    })
    .await?;
    let rooms = platform.peer_rooms();
    let stun_peer = join(&rooms, "stun", true, None)?;
    rooms.note_address(
        "header",
        &stun_peer,
        "stun",
        Ipv4Addr::new(127, 0, 0, 2).into(),
        now(),
    );
    let relayed_peer = join(&rooms, "relayed", true, None)?;
    rooms.note_address(
        "header",
        &relayed_peer,
        "relayed",
        Ipv4Addr::new(127, 0, 0, 3).into(),
        now(),
    );
    let host_peer = join(&rooms, "host", true, None)?;
    rooms.note_address(
        "header",
        &host_peer,
        "host",
        Ipv4Addr::new(127, 0, 0, 10).into(),
        now(),
    );
    let epoch = rooms.poll("header", &host_peer, "host", 0, now())?["epoch"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut others = Vec::new();
    for (launch, ip) in [
        ("a", Ipv4Addr::new(127, 0, 0, 11)),
        ("b", Ipv4Addr::new(127, 0, 0, 12)),
        ("c", Ipv4Addr::new(127, 0, 0, 13)),
        ("d", Ipv4Addr::new(127, 0, 0, 14)),
    ] {
        let peer = join(&rooms, launch, false, Some(epoch.as_str()))?;
        rooms.note_address("header", &peer, launch, ip.into(), now());
        others.push((peer, ip));
    }
    platform.publish_admission();
    for _ in 0..2_000 {
        tokio::task::yield_now().await;
    }
    bind_stun(Ipv4Addr::new(127, 0, 0, 2)).await?;
    for (_, ip) in others.iter().take(3) {
        bind_stun(*ip).await?;
    }
    let expires = now() + 3600;
    let played = carry(&secret, &relayed_peer, expires).await?;
    let breakdown = carry(&secret, &others[3].0, expires).await?;
    let refresher = platform.clone();
    let stay = vec![
        (stun_peer, "stun".to_owned()),
        (relayed_peer.clone(), "relayed".to_owned()),
        (host_peer, "host".to_owned()),
        (others[0].0.clone(), "a".to_owned()),
        (others[1].0.clone(), "b".to_owned()),
        (others[2].0.clone(), "c".to_owned()),
        (others[3].0.clone(), "d".to_owned()),
    ];
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            refresher.note_connected_session("server", "header");
            let rooms = refresher.peer_rooms();
            for (peer, launch) in &stay {
                let _ = rooms.poll("header", peer, launch, 0, now());
            }
            refresher.publish_admission();
            let _ = bind_stun(Ipv4Addr::new(127, 0, 0, 2)).await;
            for ip in [
                Ipv4Addr::new(127, 0, 0, 11),
                Ipv4Addr::new(127, 0, 0, 12),
                Ipv4Addr::new(127, 0, 0, 13),
            ] {
                let _ = bind_stun(ip).await;
            }
            let _ = played
                .send_to(b"play", breakdown.local_addr().unwrap())
                .await;
            let _ = breakdown
                .send_to(b"play", played.local_addr().unwrap())
                .await;
        }
    });
    let router = Router::new()
        .merge(portal::router(platform))
        .layer(axum::middleware::from_fn(
            |mut req: Request, next: Next| async move {
                let cookie = req
                    .headers()
                    .get("cookie")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                let id = ["lan", "server", "stun", "relayed", "host"]
                    .into_iter()
                    .find(|id| cookie.contains(&format!("fixture_player={id}")))
                    .unwrap_or("lan");
                req.headers_mut()
                    .insert("host", format!("{id}.games.example.com").parse().unwrap());
                req.headers_mut()
                    .insert("authorization", format!("Bearer {id}").parse().unwrap());
                next.run(req).await
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:17879").await?;
    println!("header shots ready on 127.0.0.1:17879");
    axum::serve(listener, router).await?;
    Ok(())
}

fn join(
    rooms: &infuser::peers::Rooms,
    launch: &str,
    create: bool,
    epoch: Option<&str>,
) -> Result<String> {
    let joined = rooms.join(
        infuser::peers::Identity {
            launch: launch.into(),
            player: format!("player-{launch}"),
            name: "Guest".into(),
            guest: true,
        },
        infuser::peers::Join {
            app: "header",
            build: "test",
            connection: &format!("connection-{launch}-0001"),
            max_players: 8,
            create,
            expected_epoch: epoch,
            expected_player: None,
            joinable: Some(true),
        },
        now(),
    )?;
    joined["self_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("peer id"))
}

async fn bind_stun(ip: Ipv4Addr) -> Result<()> {
    let socket = UdpSocket::bind((ip, 0)).await?;
    let mut request = [0u8; 20];
    request[1] = 0x01;
    request[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
    socket.send_to(&request, "127.0.0.1:18710").await?;
    Ok(())
}

async fn carry(
    secret: &[u8],
    peer: &str,
    expires: u64,
) -> Result<impl util::Conn + Send + Sync + use<>> {
    let username = format!("{expires}:{peer}");
    let socket = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?);
    let client = Client::new(ClientConfig {
        stun_serv_addr: "127.0.0.1:18710".into(),
        turn_serv_addr: "127.0.0.1:18710".into(),
        username: username.clone(),
        password: credential(secret, &username),
        realm: String::new(),
        software: String::new(),
        rto_in_ms: 100,
        conn: socket,
        vnet: None,
    })
    .await
    .map_err(|error| anyhow::anyhow!(error))?;
    client
        .listen()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    match client.allocate().await {
        Ok(relay) => Ok(relay),
        Err(error) => bail!("allocate {peer}: {error}"),
    }
}
