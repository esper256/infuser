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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn borrowed_servers_isolate_admission_and_cut_off_expired_visits() -> Result<()> {
    use infuser::platform::visits::{Participant, Visit};
    let dir = tempfile::tempdir()?;
    let backend = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = backend.local_addr()?;
    let control = Arc::new(Mutex::new((
        String::new(),
        BTreeMap::<String, String>::new(),
    )));
    let backend_control = control.clone();
    let _backend = Running(tokio::spawn(async move {
        axum::serve(backend, Router::new().route("/", get(move |h:HeaderMap,ws: WebSocketUpgrade| {
            let control=backend_control.clone();
            async move {
            let proof=h["x-infuser-connection"].to_str().unwrap();
            let identity:Value=serde_json::from_slice(&B64.decode(proof.split('.').nth(1).unwrap()).unwrap()).unwrap();
            let (base,tokens)=control.lock().unwrap().clone();
            let runtime=identity["deployment_id"].as_str().unwrap_or("pond");
            let http=reqwest::Client::new();
            for (candidate,token) in &tokens {
                let response=http.post(format!("{base}/api/server/v1/connections/{}/admission",identity["connection_id"].as_str().unwrap()))
                    .header("host","games.example.com").bearer_auth(token)
                    .json(&json!({"session":{"id":"a".repeat(32),"label":"Shared","activity":"multiplayer","joinable":true,"max_players":8}})).send().await.unwrap();
                assert_eq!(response.status().is_success(),candidate==runtime,"admission must match deployment");
            }
            ws.on_upgrade(|mut socket| async move {
                while let Some(Ok(frame)) = socket.next().await {
                    if matches!(frame, ServerMessage::Text(_) | ServerMessage::Binary(_)) && socket.send(frame).await.is_err() {break;}
                }
            })
        }}))).await.unwrap();
    }));
    let config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\nprivate_groups=true\nvisits=true",
        dir.path()
    ))?;
    platform::member(&config, "a@example.test", "Visitor", false, true)?;
    let controller = Arc::new(Controller::open(config, Arc::new(Runtime(address)))?);
    let p = portal::Platform::open(controller.clone())?;
    let spec = serde_json::from_value(
        json!({"image":format!("sha256:{}","a".repeat(64)),"container_port":7000,"web":true}),
    )?;
    let deployment = p.db.edit(|d| {
        d.groups.names.insert("host".into(),"Host".into());
        d.apps.insert("pond".into(),App {publication: None,build:"one".into(),listed:true,updated:now()});
        d.builds.insert("pond/one".into(),Build{manifest:serde_json::from_value(json!({"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["multiplayer"],"server_start":"on_demand","sessions":{"policy":"shared","max_players_per_session":8},"multiplayer":{"mode":"dedicated","transport":"websocket","max_players":8,"join_policy":"anytime"},"sdk":{"major":1,"api_major":1,"required_capabilities":[CAPABILITY,"sessions.v1"]}}))?,server:Some(spec),spa:false,cross_origin_isolation:false,ready:true,created:now(),files:Default::default()});
        d.visits.insert("visit".into(),Visit{name:"Saturday".into(),host:"host".into(),participants:BTreeMap::from([("a@example.test".into(),Participant{after_message:0,ended:false})]),games:["pond".into()].into(),expires:now()+3600,ended:false});
        d.ensure_deployment("pond","host")
    })?;
    p.reconcile_publications()?;
    // The same game's two deployments reserve distinct process instances and ports.
    for (who, runtime) in [("home", "pond"), ("away", deployment.as_str())] {
        let lease = controller.acquire(runtime, who, now())?;
        p.db.edit(|d| {
            d.launches.insert(
                who.into(),
                Launch {
                    deployment: (runtime != "pond").then(|| runtime.into()),
                    visit: (runtime != "pond").then(|| "visit".into()),
                    intent: None,
                    app: "pond".into(),
                    build: "one".into(),
                    email: "a@example.test".into(),
                    guest: false,
                    host: format!("{who}.games.example.com"),
                    token_hash: platform::hash(who),
                    exchanged: true,
                    expires: now() + 300,
                    ended: false,
                    library_session: String::new(),
                    grant_expires: now() + 120,
                    seen: now(),
                    lease_id: lease["lease_id"].as_str().map(str::to_owned),
                    lease_token: lease["lease_token"].as_str().map(str::to_owned),
                },
            );
            Ok(())
        })?;
    }
    controller.reconcile(now()).await?;
    let state = controller.snapshot();
    assert_ne!(
        state.games["pond"].attempt,
        state.games[&deployment].attempt
    );
    assert_ne!(state.games["pond"].port, state.games[&deployment].port);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let router = portal::router(p.clone());
    let _server = Running(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::new();
    let mut tokens = Vec::new();
    for runtime in ["pond", deployment.as_str()] {
        let bootstrap = infuser::connections::instance_token(
            dir.path(),
            &state.owner,
            runtime,
            &state.games[runtime].attempt,
        )?;
        // Unmodified server helpers register with the source app ID, not a deployment ID.
        let registration: Value = http
            .post(format!("{base}/api/server/v1/register"))
            .header("host", "games.example.com")
            .bearer_auth(bootstrap)
            .json(&json!({"app_id":"pond","instance_id":state.games[runtime].attempt}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(registration["scope"]["app_id"], "pond");
        assert_eq!(
            registration["scope"]["group_id"],
            if runtime == "pond" { "home" } else { "host" }
        );
        tokens.push(registration["token"].as_str().unwrap().to_owned());
    }
    *control.lock().unwrap() = (
        base.clone(),
        BTreeMap::from([
            ("pond".into(), tokens[0].clone()),
            (deployment.clone(), tokens[1].clone()),
        ]),
    );
    let options: Value = request(&http, &base, "home", reqwest::Method::GET, "")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let epoch = options["epoch"].as_str().unwrap();
    let mut sockets = Vec::new();
    for (n, who) in ["home", "away"].into_iter().enumerate() {
        let id = format!("{epoch}{:032x}", n + 1);
        request(&http, &base, who, reqwest::Method::PUT, &format!("/{id}"))
            .json(&json!({"activity":"multiplayer"}))
            .send()
            .await?
            .error_for_status()?;
        let (mut socket, _) = connect_async(handshake(&base, who, &id)).await?;
        socket.send(Message::Text(who.into())).await?;
        timeout(Duration::from_secs(2), async {
            loop {
                if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                    assert_eq!(text, who);
                    break;
                }
            }
        })
        .await?;
        sockets.push(socket);
    }
    // A real expiry, without requiring an operator action or a cooperative game server.
    p.db.edit(|d| {
        d.visits.get_mut("visit").unwrap().expires = now();
        Ok(())
    })?;
    let mut away = sockets.pop().unwrap();
    timeout(Duration::from_secs(4), async {
        while let Some(Ok(frame)) = away.next().await {
            if matches!(frame, Message::Close(_)) {
                break;
            }
        }
    })
    .await?;
    assert!(
        !request(&http, &base, "away", reqwest::Method::GET, "")
            .send()
            .await?
            .status()
            .is_success()
    );
    let mut home = sockets.pop().unwrap();
    home.send(Message::Text("still playing".into())).await?;
    timeout(Duration::from_secs(2), async {
        loop {
            if let Message::Text(text) = home.next().await.unwrap().unwrap() {
                assert_eq!(text, "still playing");
                break;
            }
        }
    })
    .await?;
    p.reconcile_publications()?;
    assert!(
        !controller.snapshot().games[&deployment]
            .leases
            .values()
            .any(|l| l.expires > now())
    );
    assert!(
        controller.snapshot().games["pond"]
            .leases
            .values()
            .any(|l| l.expires > now())
    );
    // Renames never allocate another world or personal-save namespace.
    p.db.edit(|d| {
        d.builds.get_mut("pond/one").unwrap().manifest.title = "A new title".into();
        assert_eq!(d.ensure_deployment("pond", "host")?, deployment);
        Ok(())
    })?;
    Ok(())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
    let control = Arc::new(Mutex::new((String::new(), String::new())));
    let backend_control = control.clone();
    let key_file = dir.path().join(PUBLIC_KEY_FILE);
    let backend = Router::new().route(
        "/",
        get(move |h: HeaderMap, ws: WebSocketUpgrade| {
            let record = record.clone();
            let key_file = key_file.clone();
            let control = backend_control.clone();
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
                let (base, bootstrap) = control.lock().unwrap().clone();
                let http = reqwest::Client::new();
                let registration: Value = http.post(format!("{base}/api/server/v1/register"))
                    .header("host","games.example.com").bearer_auth(bootstrap)
                    .json(&json!({"app_id":"pond","instance_id":identity["instance_id"]}))
                    .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
                http.post(format!("{base}/api/server/v1/connections/{}/admission",identity["connection_id"].as_str().unwrap()))
                    .header("host","games.example.com").bearer_auth(registration["token"].as_str().unwrap())
                    .json(&json!({"session":{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","label":"Shared","activity":"multiplayer","joinable":true,"max_players":8}}))
                    .send().await.unwrap().error_for_status().unwrap();
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
    let mut config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        dir.path().to_string_lossy()
    ))?;
    let auth_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    config.controller.auth_url = format!("http://{}/oauth2/auth", auth_listener.local_addr()?);
    config.controller.publish_capture_mb = std::env::var("INFUSER_REVIEW_CAPTURE_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let _auth = Running(tokio::spawn(async move {
        axum::serve(
            auth_listener,
            Router::new().route(
                "/oauth2/auth",
                get(|| async {
                    [
                        ("x-auth-request-email", "a@example.test"),
                        ("X-Auth-Request-User", "a@example.test"),
                    ]
                }),
            ),
        )
        .await
        .unwrap();
    }));
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
        d.apps.insert("pond".into(),App {publication: None,build:"one".into(),listed:true,updated:now()});
        d.builds.insert("pond/one".into(),Build{manifest:serde_json::from_value(json!({"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo","multiplayer"],"solo_runtime":"client_only","server_start":"on_demand","sessions":{"policy":"shared","max_players_per_session":8},"multiplayer":{"mode":"dedicated","transport":"websocket","max_players":8,"join_policy":"anytime"},"sdk":{"major":1,"api_major":1,"required_capabilities":[CAPABILITY,"sessions.v1"]}}))?,server:Some(game),spa:false,cross_origin_isolation:false,ready:true,created:now(),files:Default::default()});
        for who in ["a","b","guest"] {
            let lease = controller.acquire("pond",who,now())?;
            d.launches.insert(who.into(),Launch{deployment:None,visit:None,intent:None,app:"pond".into(),build:"one".into(),email:if who=="b"{"b@example.test"}else{"a@example.test"}.into(),guest:who=="guest",host:format!("{who}.games.example.com"),token_hash:platform::hash(who),exchanged:true,expires:now()+300,ended:false, library_session: String::new(),grant_expires:now()+120,seen:now(),lease_id:lease["lease_id"].as_str().map(str::to_owned),lease_token:lease["lease_token"].as_str().map(str::to_owned)});
        }
        d.builds["pond/one"].manifest.validate()?;
        // Managed sessions are now an additive advertised capability.
        assert!(d.builds["pond/one"].manifest.sdk.as_ref().unwrap().admit(&infuser::sdk_contract::capabilities(false)).is_ok());
        Ok(())
    })?;
    controller.reconcile(now()).await?;
    let db = p.db.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let state = controller.snapshot();
    *control.lock().unwrap() = (
        base.clone(),
        infuser::connections::instance_token(
            dir.path(),
            &state.owner,
            "pond",
            &state.games["pond"].attempt,
        )?,
    );
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
    // A mixed game keeps client-only solo while using the server for multiplayer.
    let response = request(
        &http,
        &base,
        "a",
        PUT,
        &format!("/{epoch}{}", "0".repeat(32)),
    )
    .json(&json!({"activity":"solo"}))
    .send()
    .await?;
    assert_eq!(response.status(), 422);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "unsupported_activity"
    );
    let mut ids = Vec::new();
    let mut sockets = Vec::new();
    for (n, who) in ["a", "b", "guest"].into_iter().enumerate() {
        let id = format!("{epoch}{:032x}", n + 1);
        for _ in 0..2 {
            // A lost prepare response is safely retried.
            let result: Value = request(&http, &base, who, PUT, &format!("/{id}"))
                .json(&json!({"activity":"multiplayer"}))
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
    if std::env::var_os("INFUSER_REVIEW_LOAD").is_some() {
        review_load(&http, &base, &db, dir.path(), &mut sockets[..2]).await?;
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

    // Full snapshots are ordered and do not let a late response erase newer rooms.
    let bootstrap = control.lock().unwrap().1.clone();
    let registration: Value = http
        .post(format!("{base}/api/server/v1/register"))
        .header("host", "games.example.com")
        .bearer_auth(bootstrap)
        .json(&json!({"app_id":"pond","instance_id":identities[0]["instance_id"]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = registration["token"].as_str().unwrap();
    let snapshot = json!({"coordinator":epoch,"seq":5,"sessions":[{"id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","label":"Shared","activity":"multiplayer","joinable":true,"max_players":8,"connections":ids}]});
    let report = |value: Value| {
        http.put(format!("{base}/api/server/v1/sessions"))
            .header("host", "games.example.com")
            .bearer_auth(token)
            .json(&value)
            .send()
    };
    assert_eq!(report(snapshot.clone()).await?.status(), 200);
    let late: Value = report(json!({"coordinator":epoch,"seq":4,"sessions":[]}))
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(late["accepted"], false);
    assert_eq!(late["seq"], 5);
    let duplicate: Value = report(snapshot).await?.error_for_status()?.json().await?;
    assert_eq!(duplicate["accepted"], false);
    assert_eq!(
        report(json!({"coordinator":"retired","seq":6,"sessions":[]}))
            .await?
            .status(),
        409
    );
    // Reports remain scoped to the process's original private group.
    let mut wrong_scope: infuser::dedicated::Scope =
        serde_json::from_value(registration["scope"].clone())?;
    wrong_scope.group_id = "other-family".into();
    let signer = infuser::connections::Signer::open(dir.path())?;
    let wrong_token = signer.access_token(&wrong_scope)?;
    assert_eq!(
        http.get(format!("{base}/api/server/v1/control"))
            .header("host", "games.example.com")
            .bearer_auth(wrong_token)
            .send()
            .await?
            .status(),
        401
    );
    wrong_scope = serde_json::from_value(registration["scope"].clone())?;
    wrong_scope.instance_id = "retired-process".into();
    assert_eq!(
        http.get(format!("{base}/api/server/v1/control"))
            .header("host", "games.example.com")
            .bearer_auth(signer.access_token(&wrong_scope)?)
            .send()
            .await?
            .status(),
        401
    );
    wrong_scope = serde_json::from_value(registration["scope"].clone())?;
    wrong_scope.exp = now();
    assert_eq!(
        http.get(format!("{base}/api/server/v1/control"))
            .header("host", "games.example.com")
            .bearer_auth(signer.access_token(&wrong_scope)?)
            .send()
            .await?
            .status(),
        401
    );

    // A consumed/cancelled handle cannot restart, including an identical PUT retry.
    request(&http, &base, "b", DELETE, &format!("/{}", ids[1]))
        .send()
        .await?
        .error_for_status()?;
    db.edit(|data| {
        let l = data.launches.get_mut("b").unwrap();
        l.seen = now() - 600;
        l.expires = l.seen + 300;
        Ok(())
    })?;
    let cancelled: Value = request(&http, &base, "b", PUT, &format!("/{}", ids[1]))
        .json(&json!({"activity":"multiplayer"}))
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
        .json(&json!({"activity":"multiplayer"}))
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
        .json(&json!({"activity":"multiplayer"}))
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
        .json(&json!({"activity":"multiplayer"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry, failed);
    controller.stop("pond")?;
    let stopped = request(&http, &base, "b", PUT, &format!("/{epoch}{:032x}", 6))
        .json(&json!({"activity":"multiplayer"}))
        .send()
        .await?;
    assert_eq!(stopped.status(), 410);
    assert!(!stopped.headers().contains_key("infuser-session"));
    http.get(format!("{base}/_infuser/api/v1/saves/progress"))
        .header("host", "b.games.example.com")
        .bearer_auth("b")
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

// Opt-in interaction benchmark: real gateway frames, large publication, and UI polling.
async fn review_load(
    http: &reqwest::Client,
    base: &str,
    db: &platform::Database,
    root: &std::path::Path,
    sockets: &mut [tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >],
) -> Result<()> {
    use std::time::Instant;
    let large = std::env::var_os("INFUSER_REVIEW_LARGE_CATALOG").is_some();
    let token = db.edit(|d| {
        if large {
            let sample = d.builds["pond/one"].clone();
            for app in 0..25 {
                d.apps.insert(
                    format!("load-{app}"),
                    App {
                        publication: None,
                        build: "build-0".into(),
                        listed: true,
                        updated: now(),
                    },
                );
                for build in 0..5 {
                    let mut item = sample.clone();
                    item.manifest.app_id = format!("load-{app}");
                    item.files = (0..500)
                        .map(|n| (format!("assets/generated-{n:04}.js"), 16))
                        .collect::<BTreeMap<_, _>>()
                        .into();
                    d.builds.insert(format!("load-{app}/build-{build}"), item);
                }
            }
        }
        Ok(platform::credential(d, "load-publish", "publish", "Load fixture")?.1)
    })?;
    let api = format!("{base}/api/publisher/apps/load-publish/builds/one");
    let manifest = json!({"schema_version":1,"app_id":"load-publish","title":"Load fixture","play_modes":["solo"],"solo_runtime":"client_only"});
    http.put(&api)
        .header("host", "games.example.com")
        .bearer_auth(&token)
        .json(&json!({"manifest":manifest}))
        .send()
        .await?
        .error_for_status()?;
    let bytes = vec![0u8; 64 * 1024 * 1024];
    let publisher = {
        let http = http.clone();
        let token = token.clone();
        let api = api.clone();
        tokio::spawn(async move {
            for (path, body) in [
                ("payload.bin", bytes),
                (
                    "index.html",
                    b"<script type=module src=\"/_infuser/sdk.js\"></script>".to_vec(),
                ),
            ] {
                http.put(format!("{api}/files/{path}"))
                    .header("host", "games.example.com")
                    .bearer_auth(&token)
                    .body(body)
                    .send()
                    .await?
                    .error_for_status()?;
            }
            http.post(format!("{api}/publish"))
                .header("host", "games.example.com")
                .bearer_auth(&token)
                .send()
                .await?
                .error_for_status()?;
            Ok::<_, anyhow::Error>(())
        })
    };
    let start = Instant::now();
    let polls = {
        let http = http.clone();
        let base = base.to_owned();
        tokio::spawn(async move {
            let mut count = 0;
            while start.elapsed() < Duration::from_secs(4) {
                for endpoint in ["library", "chat"] {
                    http.get(format!("{base}/api/{endpoint}"))
                        .header("host", "games.example.com")
                        .header("cookie", "fixture=yes")
                        .send()
                        .await?
                        .error_for_status()?;
                    count += 1;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok::<_, anyhow::Error>(count)
        })
    };
    let mut samples = Vec::new();
    let mut longest_gap = 0u128;
    let mut last = Instant::now();
    while start.elapsed() < Duration::from_secs(4) {
        for socket in sockets.iter_mut() {
            let sent = Instant::now();
            socket.send(Message::Text("load-ping".into())).await?;
            loop {
                match timeout(Duration::from_secs(10), socket.next())
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("gateway closed during publication"))??
                {
                    Message::Text(text) if text == "load-ping" => break,
                    Message::Ping(_) => {
                        socket.flush().await?;
                    }
                    Message::Close(_) => anyhow::bail!("gateway closed during publication"),
                    _ => {}
                }
            }
            samples.push(sent.elapsed().as_micros());
            longest_gap = longest_gap.max(last.elapsed().as_micros());
            last = Instant::now();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    publisher.await??;
    let polls = polls.await??;
    samples.sort_unstable();
    let percentile = |p: usize| samples[(samples.len() - 1) * p / 100];
    let size = |file: &str| {
        std::fs::metadata(root.join(file))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    eprintln!(
        "REVIEW_LOAD large={large} samples={} polls={polls} p50_us={} p95_us={} p99_us={} longest_gap_us={longest_gap} db_bytes={} wal_bytes={}",
        samples.len(),
        percentile(50),
        percentile(95),
        percentile(99),
        size("platform.sqlite"),
        size("platform.sqlite-wal")
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmHWM:") || line.starts_with("Threads:"))
        {
            eprintln!("REVIEW_LOAD {line}");
        }
    }
    Ok(())
}
