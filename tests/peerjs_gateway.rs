use anyhow::Result;
use axum::{
    Router,
    extract::ws::{Message as ServerMessage, WebSocketUpgrade},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build, Launch},
    portal,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::Request},
};
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn handshake(url: &str, who: &str, wire: &str) -> Request<()> {
    let mut request =
        format!("{url}/_infuser/peerjs/peerjs?key=peerjs&id={wire}&token=connection-token-123456")
            .into_client_request()
            .unwrap();
    let h = request.headers_mut();
    h.insert("host", format!("{who}.games.example.com").parse().unwrap());
    h.insert(
        "origin",
        format!("https://{who}.games.example.com").parse().unwrap(),
    );
    h.insert("authorization", format!("Bearer {who}").parse().unwrap());
    request
}
#[tokio::test]
async fn peerjs_gateway_scopes_sockets_and_signals_to_live_session() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (sent, mut received) = tokio::sync::mpsc::channel(8);
    let upstream = Router::new().route(
        "/peerjs",
        get(move |ws: WebSocketUpgrade| {
            let sent = sent.clone();
            async move {
                ws.on_upgrade(move |mut socket| async move {
                    socket
                        .send(ServerMessage::Text("{\"type\":\"OPEN\"}".into()))
                        .await
                        .unwrap();
                    while let Some(Ok(ServerMessage::Text(text))) = socket.next().await {
                        let _ = sent.send(text.to_string()).await;
                    }
                })
            }
        }),
    );
    let _upstream = Running(tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap()
    }));
    let dir = tempfile::tempdir()?;
    let secret = dir.path().join("turn.secret");
    std::fs::write(&secret, "fixture-turn-secret-0123456789012345")?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\npeerjs_port={port}\n[controller.turn]\nsecret_file={:?}\nurls=['turn:games.example.com:18700?transport=udp']\nrelay_port_start=18800\nrelay_port_end=18899",
        dir.path().display().to_string(),
        secret.display().to_string()
    ))?;
    platform::member(&cfg, "player@example.com", "Player", true, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let rooms = p.peer_rooms();
    p.db.edit(|d|{
        for app in ["pond","other"] {
            d.builds.insert(format!("{app}/one"),Build{manifest:serde_json::from_value(json!({"schema_version":1,"app_id":app,"title":app,"play_modes":["multiplayer"],"multiplayer":{"mode":"peer_hosted","transport":"webrtc","adapter":"peerjs","max_players":4,"join_policy":"anytime","direct_connect":"optional","relay_fallback":"required"}}))?,server:None,spa:false,cross_origin_isolation:false,ready:true,created:now(),files:Default::default()});
            d.apps.insert(app.into(),App{publication:None,build:"one".into(),listed:false,updated:now()});
        }
        for id in ["a","b","c","x"] {d.launches.insert(id.into(),Launch{deployment:None,visit:None,intent:None,app:if id=="x"{"other"}else{"pond"}.into(),build:"one".into(),email:"player@example.com".into(),guest:true,host:format!("{id}.games.example.com"),token_hash:platform::hash(id),exchanged:true,expires:now()+300,authorized_until:0,grant_expires:now()+300,seen:now(),lease_id:None,lease_token:None});}
        Ok(())
    })?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = Running(tokio::spawn(async move {
        axum::serve(listener, portal::router(p)).await.unwrap()
    }));
    let http = reqwest::Client::new();
    let mut peers = BTreeMap::new();
    for id in ["a", "b", "c", "x"] {
        let value: Value = http
            .post(format!("http://{address}/_infuser/api/v1/peers"))
            .header("host", format!("{id}.games.example.com"))
            .header("origin", format!("https://{id}.games.example.com"))
            .bearer_auth(id)
            .json(
                &json!({"adapter":"peerjs","connection_id":"connection-id-123456","joinable":true}),
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        peers.insert(id, value["peerjs"]["id"].as_str().unwrap().to_owned());
    }
    let url = format!("ws://{address}");
    // Neither another launch, another origin, nor an anonymous client may claim an ID.
    assert!(
        connect_async(handshake(&url, "b", &peers["a"]))
            .await
            .is_err()
    );
    let mut foreign = handshake(&url, "a", &peers["a"]);
    foreign
        .headers_mut()
        .insert("origin", "https://evil.example".parse()?);
    assert!(connect_async(foreign).await.is_err());
    let mut anonymous = handshake(&url, "a", &peers["a"]);
    anonymous.headers_mut().remove("authorization");
    assert!(connect_async(anonymous).await.is_err());
    let (mut a, _) = connect_async(handshake(&url, "a", &peers["a"])).await?;
    assert_eq!(a.next().await.unwrap()?.into_text()?, "{\"type\":\"OPEN\"}");
    assert!(
        connect_async(handshake(&url, "a", &peers["a"]))
            .await
            .is_err(),
        "duplicate socket acquired same peer ID"
    );
    let message = json!({"type":"OFFER","dst":peers["b"],"payload":{"type":"data","connectionId":"data-test"}});
    a.send(Message::Text(message.to_string().into())).await?;
    assert_eq!(
        serde_json::from_str::<Value>(
            &timeout(Duration::from_secs(2), received.recv())
                .await?
                .unwrap()
        )?,
        message
    );
    // A valid game launch still cannot signal another game's room.
    a.send(Message::Text(
        json!({"type":"OFFER","dst":peers["x"],"payload":{"type":"data"}})
            .to_string()
            .into(),
    ))
    .await?;
    let closed = timeout(Duration::from_secs(2), a.next()).await?;
    assert!(!matches!(closed, Some(Ok(Message::Text(_)))));
    // Members cannot form an extra room/mesh edge around the common host.
    let (mut b, _) = connect_async(handshake(&url, "b", &peers["b"])).await?;
    b.next().await.unwrap()?;
    b.send(Message::Text(
        json!({"type":"OFFER","dst":peers["c"],"payload":{"type":"data"}})
            .to_string()
            .into(),
    ))
    .await?;
    assert!(!matches!(
        timeout(Duration::from_secs(2), b.next()).await?,
        Some(Ok(Message::Text(_)))
    ));
    // Ending the shared session closes remaining signaling sockets, even idle ones.
    let (mut c, _) = connect_async(handshake(&url, "c", &peers["c"])).await?;
    c.next().await.unwrap()?;
    rooms.end("pond");
    assert!(!matches!(
        timeout(Duration::from_secs(4), c.next()).await?,
        Some(Ok(Message::Text(_)))
    ));
    assert!(
        connect_async(handshake(&url, "a", &peers["a"]))
            .await
            .is_err()
    );
    assert!(
        received.try_recv().is_err(),
        "forbidden signaling reached PeerServer"
    );
    Ok(())
}
