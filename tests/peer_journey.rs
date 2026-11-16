use anyhow::Result;
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    peers::{Identity, Join, Rooms},
    platform::{self, App, Build, Launch},
    portal,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
fn identity(id: &str) -> Identity {
    Identity {
        launch: id.into(),
        player: id.into(),
        name: "Player".into(),
        guest: true,
    }
}
fn join<'a>(app: &'a str, build: &'a str) -> Join<'a> {
    Join {
        app,
        build,
        connection: "connection-123456",
        max_players: 2,
        expected_epoch: None,
        expected_player: None,
        joinable: None,
    }
}
#[test]
fn host_expiry_fences_old_session_and_signals() -> Result<()> {
    let recovery = Rooms::recovering(130);
    assert!(
        recovery
            .join(identity("old"), join("pond", "one"), 129)
            .is_err()
    );
    assert!(
        recovery
            .join(identity("new"), join("pond", "one"), 130)
            .is_ok()
    );
    let rooms = Rooms::default();
    let a = rooms.join(identity("a"), join("pond", "one"), 100)?;
    let b = rooms.join(identity("b"), join("pond", "one"), 100)?;
    let aid = a["self_id"].as_str().unwrap();
    let bid = b["self_id"].as_str().unwrap();
    assert_eq!(a["epoch"], b["epoch"]);
    assert_eq!(
        rooms.discovery("pond", "one", 100)["joinable"],
        false,
        "legacy rooms may accept ordinary joins without advertising Join friend"
    );
    assert!(
        rooms
            .health("pond", aid, "b", json!({"connections":[]}), 101)
            .is_err()
    );
    rooms.health(
        "pond",
        aid,
        "a",
        json!({"connections":[{"round_trip_ms":40}]}),
        101,
    )?;
    assert_eq!(rooms.diagnostics(101)["sessions"][0]["app_id"], "pond");
    assert_eq!(
        rooms.join(identity("a"), join("pond", "one"), 101)?["self_id"],
        a["self_id"]
    );
    assert!(rooms.join(identity("c"), join("pond", "one"), 101).is_err());
    assert!(rooms.join(identity("a"), join("pond", "two"), 101).is_err());
    let mut message =
        json!({"epoch":a["epoch"],"to":bid,"payload":{"type":"offer","description":"test"}});
    assert!(rooms.signal("pond", aid, "b", &message, 101).is_err());
    assert!(rooms.signal("other", aid, "a", &message, 101).is_err());
    message["epoch"] = json!("stale");
    assert!(rooms.signal("pond", aid, "a", &message, 101).is_err());
    message["epoch"] = a["epoch"].clone();
    for _ in 0..32 {
        rooms.signal("pond", aid, "a", &message, 101)?;
    }
    assert!(rooms.signal("pond", aid, "a", &message, 101).is_err());
    let poll = rooms.poll("pond", bid, "b", 0, 120)?;
    assert_eq!(poll["signals"].as_array().unwrap().len(), 32);
    assert_eq!(
        rooms.poll("pond", bid, "b", 0, 120)?["signals"],
        poll["signals"],
        "unacknowledged signaling must replay"
    );
    assert_eq!(
        rooms.poll("pond", bid, "b", poll["cursor"].as_u64().unwrap(), 120)?["signals"],
        json!([])
    );
    assert!(
        rooms.poll("pond", bid, "b", 0, 131).is_err(),
        "member polling cannot preserve an expired host"
    );
    let next = rooms.join(identity("b"), join("pond", "two"), 131)?;
    assert_eq!(next["role"], "host");
    assert_ne!(next["epoch"], a["epoch"]);
    assert!(rooms.signal("pond", aid, "a", &message, 131).is_err());
    let mut stale = join("pond", "two");
    stale.expected_epoch = a["epoch"].as_str();
    assert!(rooms.join(identity("c"), stale, 131).is_err());
    assert_eq!(rooms.discovery("pond", "two", 131)["player_count"], 1);
    rooms.retain_launches("pond", &Default::default(), 131);
    assert!(rooms.discovery("pond", "two", 131).is_null());
    Ok(())
}
#[tokio::test]
async fn authenticated_peers_pin_build_until_host_leaves() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let secret = dir.path().join("turn.secret");
    std::fs::write(&secret, "test-secret-material-with-32-characters")?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nport_end=8699\ndisk_reserve_mb=64\n[controller.turn]\nurls=['turn:games.example.com:8700?transport=udp']\nsecret_file={:?}\nrelay_port_start=8800\nrelay_port_end=8899",
        dir.path().display().to_string(),
        secret.display().to_string()
    ))?;
    cfg.validate()?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    platform::member(&cfg, "friend@example.com", "Friend", false, true)?;
    platform::member(&cfg, "disabled@example.com", "Disabled", false, false)?;
    platform::member(&cfg, "stale@example.com", "Stale", false, true)?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let platform = portal::Platform::open(controller)?;
    let manifest = json!({"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo","multiplayer"],"solo_runtime":"client_only","multiplayer":{"mode":"peer_hosted","transport":"webrtc","max_players":2,"join_policy":"between_rounds","direct_connect":"disabled","relay_fallback":"required"}});
    for build in ["one", "two"] {
        let content = dir.path().join("content/pond").join(build);
        std::fs::create_dir_all(&content)?;
        std::fs::write(
            content.join("index.html"),
            "<script type=module src=\"/_infuser/sdk.js\"></script>",
        )?;
    }
    let publisher = platform.db.edit(|d| {
        for build in ["one", "two"] {
            d.builds.insert(
                format!("pond/{build}"),
                Build {
                    manifest: serde_json::from_value(manifest.clone())?,
                    server: None,
                    spa: false,
                    cross_origin_isolation: false,
                    ready: build == "one",
                    created: now(),
                    files: BTreeMap::from([("index.html".into(), 64)]).into(),
                },
            );
        }
        d.apps.insert(
            "pond".into(),
            App {
                build: "one".into(),
                listed: false,
                updated: now(),
            },
        );
        for id in ["a", "b", "c"] {
            d.launches.insert(
                id.into(),
                Launch {
                    deployment: None,
                    visit: None,
                    intent: None,
                    app: "pond".into(),
                    build: "one".into(),
                    email: "admin@example.com".into(),
                    guest: id != "a",
                    host: format!("{id}.games.example.com"),
                    token_hash: platform::hash(id),
                    exchanged: true,
                    expires: now() + 300,
                    grant_expires: now() + 300,
                    seen: now(),
                    lease_id: None,
                    lease_token: None,
                },
            );
        }
        for email in ["friend@example.com", "disabled@example.com"] {
            d.presence.insert(email.into(), now());
        }
        d.presence.insert("stale@example.com".into(), now() - 91);
        platform::credential(d, "pond", "publish", "test")
    })?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let router = portal::router(platform.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let call = |id: &str, method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}/_infuser/peers{path}"))
            .header("host", format!("{id}.games.example.com"))
            .header("origin", format!("https://{id}.games.example.com"))
            .bearer_auth(id)
    };
    let (get, post, delete) = (
        reqwest::Method::GET,
        reqwest::Method::POST,
        reqwest::Method::DELETE,
    );
    let presence = |id: &str| {
        http.get(format!("{url}/_infuser/presence/v1"))
            .header("host", format!("{id}.games.example.com"))
            .bearer_auth(id)
    };
    let before: Value = presence("c")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(before["session"].is_null());
    assert_eq!(before["can_start"], true);
    assert_eq!(before["self_id"], "guest-c");
    assert_eq!(before["players"].as_array().unwrap().len(), 4);
    assert_eq!(
        before["players"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["game_open"] == true)
            .count(),
        3
    );
    assert!(
        before["players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["multiplayer"] == false)
    );
    assert!(!before.to_string().contains("@example.com"));
    assert_eq!(platform.controller.list(now())["reserved_memory_mb"], 0);
    assert_eq!(
        presence("c")
            .header("origin", "https://other.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(presence("invalid").send().await?.status(), 401);
    assert_eq!(
        http.post(format!("{url}/_infuser/peers"))
            .json(&json!({"connection_id":"connection-123456"}))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        call("a", post.clone(), "")
            .header("origin", "https://evil.example")
            .json(&json!({"connection_id":"connection-123456"}))
            .send()
            .await?
            .status(),
        403
    );
    let a: Value = call("a", post.clone(), "")
        .json(&json!({"connection_id":"connection-123456","relay_only":false,"joinable":false}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let aid = a["self_id"].as_str().unwrap();
    let closed: Value = presence("c").send().await?.json().await?;
    assert_eq!(closed["session"]["reason"], "closed");
    assert_eq!(closed["session"]["player_count"], 1);
    assert_eq!(closed["can_start"], false);
    assert_eq!(
        closed["players"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["multiplayer"] == true)
            .count(),
        1
    );
    assert_eq!(
        call("b", post.clone(), "")
            .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"]}))
            .send()
            .await?
            .status(),
        409
    );
    let set = |id: &str, peer: &str, epoch: &Value, joinable: bool| {
        call(id, reqwest::Method::PUT, &format!("/{peer}/joinability"))
            .json(&json!({"epoch":epoch,"joinable":joinable}))
    };
    assert_eq!(set("b", aid, &a["epoch"], true).send().await?.status(), 403);
    assert_eq!(
        set("a", aid, &json!("old"), true).send().await?.status(),
        409
    );
    assert_eq!(
        set("a", aid, &a["epoch"], true)
            .header("origin", "https://other.example")
            .send()
            .await?
            .status(),
        403
    );
    set("a", aid, &a["epoch"], true)
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        presence("c").send().await?.json::<Value>().await?["session"]["joinable"],
        true
    );
    let b: Value = call("b", post.clone(), "")
        .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"],"joinable":false}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(a["epoch"], b["epoch"]);
    assert_eq!(a["ice_transport_policy"], "relay");
    assert!(a["ice_servers"][0]["credential"].as_str().is_some());
    assert!(!a.to_string().contains("admin@example.com"));
    let aid = a["self_id"].as_str().unwrap();
    let bid = b["self_id"].as_str().unwrap();
    assert_eq!(
        set("b", bid, &a["epoch"], false).send().await?.status(),
        403
    );
    let full: Value = presence("c").send().await?.json().await?;
    assert_eq!(full["session"]["reason"], "full");
    assert_eq!(full["session"]["joinable"], false);
    assert_eq!(
        call("c", post.clone(), "")
            .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"]}))
            .send()
            .await?
            .status(),
        409
    );
    call("b", delete.clone(), &format!("/{bid}"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        presence("c").send().await?.json::<Value>().await?["session"]["joinable"],
        true
    );
    call("b", post.clone(), "")
        .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"]}))
        .send()
        .await?
        .error_for_status()?;
    set("a", aid, &a["epoch"], false)
        .send()
        .await?
        .error_for_status()?;
    // Existing peers can retry/poll while the host closes admission between rounds.
    call("b", post.clone(), "")
        .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        call("b", get.clone(), &format!("/{aid}"))
            .send()
            .await?
            .status(),
        403
    );
    call("a", post.clone(), &format!("/{aid}/signals"))
        .json(&json!({"epoch":a["epoch"],"to":bid,"payload":{"type":"offer","description":"test"}}))
        .send()
        .await?
        .error_for_status()?;
    let received: Value = call("b", get.clone(), &format!("/{bid}"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(received["signals"][0]["from"], aid);
    let response = http
        .post(format!("{url}/api/publisher/apps/pond/builds/two/publish"))
        .header("host", "games.example.com")
        .bearer_auth(&publisher.1)
        .json(&json!({"listed":false}))
        .send()
        .await?;
    assert_eq!(response.status(), 202, "{}", response.text().await?);
    platform.activate_pending()?;
    assert_eq!(platform.db.read()?.apps["pond"].build, "one");
    call("a", delete.clone(), &format!("/{aid}"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        call("c", post.clone(), "")
            .json(&json!({"connection_id":"connection-123456","expected_epoch":a["epoch"]}))
            .send()
            .await?
            .status(),
        410
    );
    assert!(
        presence("c").send().await?.json::<Value>().await?["session"].is_null(),
        "stale Join friend must not create a replacement world"
    );
    assert_eq!(
        call("b", get.clone(), &format!("/{bid}"))
            .send()
            .await?
            .status(),
        410
    );
    platform.activate_pending()?;
    assert_eq!(platform.db.read()?.apps["pond"].build, "two");
    let old: Value = presence("c").send().await?.json().await?;
    assert_eq!(old["can_start"], false);
    assert_eq!(old["unavailable_reason"], "newer_build");
    assert_eq!(
        call("b", post.clone(), "")
            .json(&json!({"connection_id":"connection-123456"}))
            .send()
            .await?
            .status(),
        409,
        "old client may not create a new session after an update"
    );
    // Bring the same source game to another group. Its room and exact-join
    // targets must remain independent from the home deployment.
    let deployment = platform.db.edit(|d| {
        use infuser::platform::visits::{Participant, Visit};
        d.apps.get_mut("pond").unwrap().listed = true;
        for l in d.launches.values_mut() {
            l.build = "two".into();
        }
        d.groups.names.insert("away".into(), "Away".into());
        d.visits.insert(
            "visit".into(),
            Visit {
                name: "Peer visit".into(),
                host: "away".into(),
                participants: BTreeMap::from([(
                    "admin@example.com".into(),
                    Participant {
                        after_message: 0,
                        ended: false,
                    },
                )]),
                games: ["pond".into()].into(),
                expires: now() + 3600,
                ended: false,
            },
        );
        let deployment = d.ensure_deployment("pond", "away")?;
        for id in ["b", "c"] {
            let l = d.launches.get_mut(id).unwrap();
            l.deployment = Some(deployment.clone());
            l.visit = Some("visit".into());
        }
        Ok(deployment)
    })?;
    let home: Value = call("a", post.clone(), "")
        .json(&json!({"connection_id":"home-connection-1234","joinable":true}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        call("b", post.clone(), "")
            .json(&json!({"connection_id":"visit-connection-123","expected_epoch":home["epoch"]}))
            .send()
            .await?
            .status(),
        410
    );
    let borrowed: Value = call("b", post.clone(), "")
        .json(&json!({"connection_id":"visit-connection-123","joinable":true}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_ne!(borrowed["epoch"], home["epoch"]);
    call("c", post.clone(), "")
        .json(&json!({"connection_id":"visit-connection-456","expected_epoch":borrowed["epoch"]}))
        .send()
        .await?
        .error_for_status()?;
    platform.db.edit(|d| {
        d.visits.get_mut("visit").unwrap().expires = now();
        Ok(())
    })?;
    assert_eq!(
        call(
            "b",
            get.clone(),
            &format!("/{}", borrowed["self_id"].as_str().unwrap())
        )
        .send()
        .await?
        .status(),
        403
    );
    platform.reconcile_publications()?;
    assert!(
        platform
            .peer_rooms()
            .discovery(&deployment, "two", now())
            .is_null()
    );
    call(
        "a",
        get.clone(),
        &format!("/{}", home["self_id"].as_str().unwrap()),
    )
    .send()
    .await?
    .error_for_status()?;
    task.abort();
    Ok(())
}
