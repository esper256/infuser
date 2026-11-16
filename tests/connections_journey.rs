//! Real HTTP/WebSocket boundary with an instant engine. No Google login or Docker.
use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Router,
    extract::ws::{Message as ServerMessage, WebSocketUpgrade},
    http::HeaderMap,
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use futures_util::{SinkExt, StreamExt};
use infuser::{
    config::Config,
    connections::{CAPABILITY, PUBLIC_KEY_FILE},
    controller::{Controller, now},
    docker::{Engine, Health},
    platform::{self, App, Build, Launch},
    portal,
    store::Instance,
};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::time::{Duration, timeout};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

struct Runtime(SocketAddr);
#[async_trait]
impl Engine for Runtime {
    async fn preflight(&self) -> Result<()> {
        Ok(())
    }
    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }
    async fn ensure(&self, _: &str, _: &Instance, _: &str) -> Result<Health> {
        Ok(Health::Ready)
    }
    async fn health(&self, _: &Instance, _: &str, _: &str) -> Result<Health> {
        Ok(Health::Ready)
    }
    async fn remove(&self, _: &str, _: &Instance, _: &str) -> Result<()> {
        Ok(())
    }
    async fn web_address(&self, _: &str, _: &Instance, _: &str) -> Result<SocketAddr> {
        Ok(self.0)
    }
}
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn request(
    http: &reqwest::Client,
    base: &str,
    who: &str,
    method: reqwest::Method,
    suffix: &str,
) -> reqwest::RequestBuilder {
    http.request(
        method,
        format!("{base}/_infuser/api/v1/connections/v1{suffix}"),
    )
    .header("host", format!("{who}.games.example.com"))
    .header("origin", format!("https://{who}.games.example.com"))
    .bearer_auth(who)
}
fn handshake(
    base: &str,
    who: &str,
    attempt: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut request = format!(
        "{}/_infuser/api/v1/connections/v1/{attempt}/socket",
        base.replacen("http", "ws", 1)
    )
    .into_client_request()
    .unwrap();
    let headers = request.headers_mut();
    headers.insert("host", format!("{who}.games.example.com").parse().unwrap());
    headers.insert(
        "origin",
        format!("https://{who}.games.example.com").parse().unwrap(),
    );
    headers.insert("authorization", format!("Bearer {who}").parse().unwrap());
    headers.insert("cookie", "unrelated=private".parse().unwrap());
    headers.insert(
        "x-infuser-connection",
        "forged-caller-identity".parse().unwrap(),
    );
    request
}

#[tokio::test]
async fn verified_gateway_keeps_frames_and_enforces_access_without_game_cooperation() -> Result<()>
{
    const GET: reqwest::Method = reqwest::Method::GET;
    const PUT: reqwest::Method = reqwest::Method::PUT;
    const DELETE: reqwest::Method = reqwest::Method::DELETE;
    let dir = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let backend_address = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let record = seen.clone();
    let key_file = dir.path().join(PUBLIC_KEY_FILE);
    let backend = Router::new().route(
        "/",
        get(move |h: HeaderMap, ws: WebSocketUpgrade| {
            let record = record.clone();
            let key_file = key_file.clone();
            async move {
                // The backend deliberately implements no revocation/control endpoint.
                assert!(
                    !h.contains_key("authorization")
                        && !h.contains_key("cookie")
                        && !h.contains_key("origin")
                );
                let proof = h["x-infuser-connection"].to_str().unwrap();
                let parts = proof.split('.').collect::<Vec<_>>();
                assert_eq!(parts.len(), 3);
                let key = B64
                    .decode(std::fs::read_to_string(key_file).unwrap())
                    .unwrap();
                UnparsedPublicKey::new(&ED25519, key)
                    .verify(
                        parts[..2].join(".").as_bytes(),
                        &B64.decode(parts[2]).unwrap(),
                    )
                    .unwrap();
                let identity: Value =
                    serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
                assert_eq!(identity["aud"], "infuser-server-connection");
                assert_eq!(identity["app_id"], "pond");
                assert!(identity["exp"].as_u64().unwrap() > now());
                record.lock().unwrap().push(identity);
                ws.on_upgrade(|mut socket| async move {
                    while let Some(Ok(frame)) = socket.next().await {
                        if matches!(frame, ServerMessage::Text(_) | ServerMessage::Binary(_))
                            && socket.send(frame).await.is_err()
                        {
                            break;
                        }
                    }
                })
            }
        }),
    );
    let _backend = Running(tokio::spawn(async move {
        axum::serve(listener, backend).await.unwrap()
    }));
    let config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        dir.path().to_string_lossy()
    ))?;
    platform::member(&config, "a@example.test", "River", false, true)?;
    platform::member(&config, "b@example.test", "Sky", false, true)?;
    let controller = Arc::new(Controller::open(
        config,
        Arc::new(Runtime(backend_address)),
    )?);
    let p = portal::Platform::open(controller.clone())?;
    let game = serde_json::from_value(
        json!({"image":format!("sha256:{}","a".repeat(64)),"container_port":7000,"web":true}),
    )?;
    controller.register("pond", game)?;
    let game = controller.snapshot().games["pond"].spec.clone();
    p.db.edit(|d| {
        d.apps.insert("pond".into(),App{build:"one".into(),listed:true,updated:now()});
        d.builds.insert("pond/one".into(),Build{manifest:serde_json::from_value(json!({"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["multiplayer"],"server_start":"on_demand","multiplayer":{"mode":"dedicated","transport":"websocket","max_players":8,"join_policy":"anytime"},"sdk":{"major":1,"api_major":1,"required_capabilities":[CAPABILITY]}}))?,server:Some(game),spa:false,cross_origin_isolation:false,ready:true,created:now(),files:BTreeMap::new()});
        for who in ["a","b","guest"] {
            let lease = controller.acquire("pond",who,now())?;
            d.launches.insert(who.into(),Launch{intent:None,app:"pond".into(),build:"one".into(),email:if who=="b"{"b@example.test"}else{"a@example.test"}.into(),guest:who=="guest",host:format!("{who}.games.example.com"),token_hash:platform::hash(who),exchanged:true,expires:now()+300,grant_expires:now()+120,seen:now(),lease_id:lease["lease_id"].as_str().map(str::to_owned),lease_token:lease["lease_token"].as_str().map(str::to_owned)});
        }
        // Publication must continue to reject this incomplete integration.
        assert!(d.builds["pond/one"].manifest.sdk.as_ref().unwrap().admit(&infuser::sdk_contract::capabilities(false)).is_err());
        Ok(())
    })?;
    controller.reconcile(now()).await?;
    let db = p.db.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let _server = Running(tokio::spawn(async move {
        axum::serve(listener, portal::router(p)).await.unwrap()
    }));
    let http = reqwest::Client::new();
    let options: Value = request(&http, &base, "a", GET, "")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let epoch = options["epoch"].as_str().unwrap();
    let mut ids = Vec::new();
    let mut sockets = Vec::new();
    for (n, who) in ["a", "b", "guest"].into_iter().enumerate() {
        let id = format!("{epoch}{:032x}", n + 1);
        for _ in 0..2 {
            // A lost prepare response is safely retried.
            let result: Value = request(&http, &base, who, PUT, &format!("/{id}"))
                .json(&json!({}))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            assert_eq!(result["state"], "pending");
        }
        assert_eq!(
            request(
                &http,
                &base,
                if who == "a" { "b" } else { "a" },
                GET,
                &format!("/{id}")
            )
            .send()
            .await?
            .status(),
            404
        );
        let mut foreign = handshake(&base, who, &id);
        foreign
            .headers_mut()
            .insert("origin", "https://evil.example".parse()?);
        assert!(connect_async(foreign).await.is_err());
        let mut anonymous = handshake(&base, who, &id);
        anonymous.headers_mut().remove("authorization");
        assert!(connect_async(anonymous).await.is_err());
        let (mut socket, _) = connect_async(handshake(&base, who, &id)).await?;
        socket
            .send(Message::Text("unchanged gameplay".into()))
            .await?;
        loop {
            let message = timeout(Duration::from_secs(2), socket.next())
                .await?
                .unwrap()?;
            if let Message::Text(text) = message {
                assert_eq!(text, "unchanged gameplay");
                break;
            }
        }
        socket
            .send(Message::Binary(vec![0, 255, 1, 128].into()))
            .await?;
        loop {
            let message = timeout(Duration::from_secs(2), socket.next())
                .await?
                .unwrap()?;
            if let Message::Binary(data) = message {
                assert_eq!(&data[..], &[0, 255, 1, 128]);
                break;
            }
        }
        assert!(
            connect_async(handshake(&base, who, &id)).await.is_err(),
            "replayed upgrade accepted"
        );
        sockets.push(socket);
        ids.push(id);
    }
    let identities = seen.lock().unwrap().clone();
    assert_eq!(identities.len(), 3);
    assert_eq!(identities[0]["name"], "River");
    assert_eq!(identities[1]["name"], "Sky");
    assert_eq!(identities[2]["guest"], true);
    assert_ne!(identities[0]["player_id"], identities[2]["player_id"]);
    assert_eq!(
        identities[0]["instance_id"],
        controller.snapshot().games["pond"].attempt
    );

    // A consumed/cancelled handle cannot restart, including an identical PUT retry.
    request(&http, &base, "b", DELETE, &format!("/{}", ids[1]))
        .send()
        .await?
        .error_for_status()?;
    let cancelled: Value = request(&http, &base, "b", PUT, &format!("/{}", ids[1]))
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(cancelled["state"], "cancelled");
    assert!(connect_async(handshake(&base, "b", &ids[1])).await.is_err());
    // Membership revocation closes both the member and their sponsored guest.
    db.edit(|d| {
        d.members.get_mut("a@example.test").unwrap().enabled = false;
        Ok(())
    })?;
    for mut socket in sockets {
        timeout(Duration::from_secs(8), async {
            loop {
                match socket.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Text(_) | Message::Binary(_))) => {
                        panic!("unexpected gameplay after revocation")
                    }
                    _ => {}
                }
            }
        })
        .await?;
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        3,
        "denied attempts reached the backend"
    );

    // Releasing demand closes the transport even when the game ignores it.
    let released_id = format!("{epoch}{:032x}", 4);
    request(&http, &base, "b", PUT, &format!("/{released_id}"))
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?;
    let (mut socket, _) = connect_async(handshake(&base, "b", &released_id)).await?;
    let lease = db.read()?.launches["b"].lease_id.clone().unwrap();
    http.delete(format!("{base}/_infuser/api/v1/runtime/v1/{lease}"))
        .header("host", "b.games.example.com")
        .header("origin", "https://b.games.example.com")
        .bearer_auth("b")
        .send()
        .await?
        .error_for_status()?;
    timeout(Duration::from_secs(8), async {
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await?;
    let ended: Value = request(&http, &base, "b", GET, &format!("/{released_id}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(ended["state"], "failed");
    assert_eq!(ended["error"]["code"], "authorization_ended");

    // A failed upgrade has an independently readable outcome, even though a
    // browser's WebSocket error event cannot reveal its HTTP rejection body.
    http.post(format!("{base}/_infuser/api/v1/runtime/v1"))
        .header("host", "b.games.example.com")
        .header("origin", "https://b.games.example.com")
        .bearer_auth("b")
        .send()
        .await?
        .error_for_status()?;
    let failed_id = format!("{epoch}{:032x}", 5);
    request(&http, &base, "b", PUT, &format!("/{failed_id}"))
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?;
    drop(_backend);
    tokio::task::yield_now().await;
    assert!(
        connect_async(handshake(&base, "b", &failed_id))
            .await
            .is_err()
    );
    let failed: Value = request(&http, &base, "b", GET, &format!("/{failed_id}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["error"]["code"], "backend_unavailable");
    let retry: Value = request(&http, &base, "b", PUT, &format!("/{failed_id}"))
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry, failed);
    Ok(())
}
