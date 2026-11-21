//! Exercise real Infuser authentication and channel policy against a private control fixture.
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{
        self,
        visits::{Participant, Visit},
    },
    portal,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(app: Router) -> Result<(String, Running)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let u = format!("http://{}", l.local_addr()?);
    Ok((
        u,
        Running(tokio::spawn(
            async move { axum::serve(l, app).await.unwrap() },
        )),
    ))
}
fn claims(token: &str) -> Value {
    serde_json::from_slice(&B64.decode(token.split('.').nth(1).unwrap()).unwrap()).unwrap()
}
#[derive(Default)]
struct Media {
    participants: Vec<Value>,
    removed: Vec<String>,
    down: bool,
}
async fn control(
    State(s): State<Arc<Mutex<Media>>>,
    Path(method): Path<String>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert!(h.get("cookie").is_none());
    let token = h["authorization"]
        .to_str()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap();
    let grant = claims(token);
    assert!(grant["video"]["roomJoin"].is_null());
    if method == "DeleteRoom" {
        assert_eq!(grant["video"]["roomCreate"], true);
    }
    let mut s = s.lock().unwrap();
    if s.down {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
    };
    let result = match method.as_str() {
        "ListRooms" => {
            json!({"rooms":s.participants.iter().map(|p|json!({"name":p["room"]})).collect::<Vec<_>>()})
        }
        "ListParticipants" => {
            json!({"participants":s.participants.iter().filter(|p|p["room"]==body["room"]).cloned().collect::<Vec<_>>()})
        }
        "RemoveParticipant" => {
            s.removed.push(body["identity"].as_str().unwrap().into());
            s.participants.retain(|p| p["identity"] != body["identity"]);
            json!({})
        }
        "DeleteRoom" => {
            s.participants.retain(|p| p["room"] != body["room"]);
            json!({})
        }
        _ => panic!("unexpected API"),
    };
    (StatusCode::OK, Json(result))
}
#[tokio::test]
async fn voice_admission_reconnect_revocation_and_restart_follow_channel_access() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let media = Arc::new(Mutex::new(Media::default()));
    let (sidecar, _sidecar) = serve(
        Router::new()
            .route("/twirp/livekit.RoomService/{method}", post(control))
            .with_state(media.clone()),
    )
    .await?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let who = h
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim_start_matches("member=")
                .to_owned();
            if ["host", "visitor", "other", "operator"].contains(&who.as_str()) {
                (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", format!("{who}@example.com")),
                        ("X-Auth-Request-User", who),
                    ],
                )
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    [
                        ("X-Auth-Request-Email", String::new()),
                        ("X-Auth-Request-User", String::new()),
                    ],
                )
            }
        }),
    ))
    .await?;
    let keys = temp.path().join("voice.json");
    infuser::private_file::replace(
        &keys,
        br#"{"api_key":"test-key","api_secret":"a-very-long-disposable-voice-test-secret"}"#,
    )?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nprivate_groups=true\nvisits=true\n[controller.voice]\nport={}\nkey_file={:?}",
        temp.path().display().to_string(),
        format!("{auth}/auth"),
        reqwest::Url::parse(&sidecar)?.port().unwrap(),
        keys.display().to_string()
    ))?;
    for who in ["host", "visitor", "other", "operator"] {
        platform::member(
            &cfg,
            &format!("{who}@example.com"),
            who,
            who == "operator",
            true,
        )?;
    }
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller.clone())?;
    p.db.edit(|d| {
        d.groups.names.insert("away".into(), "Away".into());
        for who in ["visitor", "other"] {
            d.groups
                .members
                .insert(format!("{who}@example.com"), "away".into());
        }
        d.groups.operators.insert("operator@example.com".into());
        d.visits.insert(
            "sleepover".into(),
            Visit {
                name: "Visit".into(),
                host: "home".into(),
                expires: now() + 3600,
                ended: false,
                games: Default::default(),
                participants: [(
                    "visitor@example.com".into(),
                    Participant {
                        after_message: 0,
                        ended: false,
                    },
                )]
                .into(),
            },
        );
        Ok(())
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("origin", "https://games.example.com")
            .header("cookie", format!("member={who}"))
    };
    let post =
        |who: &str, id: &str| req(reqwest::Method::POST, "/api/voice", who).json(&json!({"id":id}));
    assert_eq!(
        post("host", "host-voice-request-001")
            .send()
            .await?
            .status(),
        503,
        "startup must reconcile before minting tokens"
    );
    p.reconcile_voice().await?;
    assert_eq!(
        post("unknown", "unknown-request-001")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        post("host", "host-voice-request-001")
            .header("origin", "https://evil.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        post("operator", "operator-request-001")
            .header("x-infuser-inspect-group", "away")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        post("other", "outsider-request-001")
            .header("x-infuser-visit", "sleepover")
            .send()
            .await?
            .status(),
        403
    );
    let host: Value = post("host", "host-voice-request-001")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let visitor: Value = post("visitor", "visitor-request-001")
        .header("x-infuser-visit", "sleepover")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let other: Value = post("other", "other-voice-request-001")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let hc = claims(host["token"].as_str().unwrap());
    let vc = claims(visitor["token"].as_str().unwrap());
    let oc = claims(other["token"].as_str().unwrap());
    assert_eq!(hc["video"]["room"], vc["video"]["room"]);
    assert_ne!(hc["video"]["room"], oc["video"]["room"]);
    assert_eq!(hc["video"]["canPublishSources"], json!(["microphone"]));
    assert_eq!(hc["video"]["canPublishData"], false);
    assert!(hc["video"]["roomAdmin"].is_null());
    assert_eq!(
        post("host", "host-voice-request-002")
            .send()
            .await?
            .status(),
        409,
        "one microphone per profile"
    );
    let retry: Value = post("host", "host-voice-request-001")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry["id"], host["id"]);
    for c in [&hc, &vc, &oc] {
        media
            .lock()
            .unwrap()
            .participants
            .push(json!({"identity":c["sub"],"room":c["video"]["room"],"state":"ACTIVE"}));
    }
    p.reconcile_voice().await?;
    let status: Value = req(reqwest::Method::GET, "/api/voice", "host")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["participants"].as_array().unwrap().len(), 2);
    let status: Value = req(reqwest::Method::GET, "/api/voice", "other")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["participants"].as_array().unwrap().len(), 1);
    let validate = |token: &str| {
        http.get(format!("{url}/voice/rtc/validate"))
            .header("host", "games.example.com")
            .query(&[("access_token", token)])
    };
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        validate(&format!("{}x", host["token"].as_str().unwrap()))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .query(&[("room", "elsewhere")])
            .send()
            .await?
            .status(),
        400
    );
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .header("origin", "https://evil.example")
            .send()
            .await?
            .status(),
        403
    );
    // Another profile cannot revoke or renew this lease.
    req(reqwest::Method::DELETE, "/api/voice", "other")
        .json(&json!({"id":host["id"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        req(reqwest::Method::POST, "/api/voice/heartbeat", "other")
            .json(&json!({"id":host["id"]}))
            .send()
            .await?
            .status(),
        403
    );
    p.db.edit(|d| {
        d.visits.get_mut("sleepover").unwrap().ended = true;
        Ok(())
    })?;
    assert_eq!(
        validate(visitor["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403
    );
    p.reconcile_voice().await?;
    assert!(
        media
            .lock()
            .unwrap()
            .removed
            .contains(&vc["sub"].as_str().unwrap().to_owned())
    );
    p.db.edit(|d| {
        d.visits.get_mut("sleepover").unwrap().ended = false;
        Ok(())
    })?;
    assert_eq!(
        validate(visitor["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403,
        "resuming a visit never revives a revoked token"
    );
    media.lock().unwrap().down = true;
    assert!(p.reconcile_voice().await.is_err());
    assert_eq!(
        post("visitor", "visitor-request-002")
            .send()
            .await?
            .status(),
        503
    );
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        503
    );
    media.lock().unwrap().down = false;
    p.reconcile_voice().await?;
    req(reqwest::Method::DELETE, "/api/voice", "host")
        .json(&json!({"id":host["id"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403
    );
    assert!(
        media
            .lock()
            .unwrap()
            .removed
            .contains(&hc["sub"].as_str().unwrap().to_owned())
    );
    let new_host: Value = post("host", "host-voice-request-001")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_ne!(
        host["id"], new_host["id"],
        "fresh admission has a server-generated identity even with a reused request ID"
    );
    assert_eq!(
        validate(host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403
    );
    p.db.edit(|d| {
        d.members.get_mut("host@example.com").unwrap().enabled = false;
        Ok(())
    })?;
    assert_eq!(
        validate(new_host["token"].as_str().unwrap())
            .send()
            .await?
            .status(),
        403
    );
    let restarted = portal::Platform::open(controller)?;
    restarted.reconcile_voice().await?;
    assert!(
        media.lock().unwrap().participants.is_empty(),
        "controller restart removes orphan audio sessions"
    );
    Ok(())
}

#[tokio::test]
async fn voice_settings_and_game_header_sync_respect_boundaries() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let media = Arc::new(Mutex::new(Media::default()));
    let (sidecar, _sidecar) = serve(
        Router::new()
            .route("/twirp/livekit.RoomService/{method}", post(control))
            .with_state(media.clone()),
    )
    .await?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let who = h
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim_start_matches("member=")
                .to_owned();
            if ["host", "friend", "operator"].contains(&who.as_str()) {
                (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", format!("{who}@example.com")),
                        ("X-Auth-Request-User", who),
                    ],
                )
            } else {
                (
                    StatusCode::UNAUTHORIZED,
                    [
                        ("X-Auth-Request-Email", String::new()),
                        ("X-Auth-Request-User", String::new()),
                    ],
                )
            }
        }),
    ))
    .await?;
    let keys = temp.path().join("voice.json");
    infuser::private_file::replace(
        &keys,
        br#"{"api_key":"test-key","api_secret":"a-very-long-disposable-voice-test-secret"}"#,
    )?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nprivate_groups=true\n[controller.voice]\nport={}\nkey_file={:?}",
        temp.path().display().to_string(),
        format!("{auth}/auth"),
        reqwest::Url::parse(&sidecar)?.port().unwrap(),
        keys.display().to_string()
    ))?;
    for who in ["host", "friend", "operator"] {
        platform::member(
            &cfg,
            &format!("{who}@example.com"),
            who,
            who == "operator",
            true,
        )?;
    }
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller)?;
    let friend_id = p.db.catalog()?.player_id("friend@example.com");
    p.db.edit(|d| {
        d.groups.names.insert("away".into(), "Away".into());
        d.groups.operators.insert("operator@example.com".into());
        d.apps.insert(
            "pond".into(),
            serde_json::from_value(json!({"build":"one","updated":now(),"listed":true}))?,
        );
        d.builds.insert(
            "pond/one".into(),
            serde_json::from_value(json!({"manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},"server":null,"spa":false,"cross_origin_isolation":false,"ready":true,"created":now(),"files":{}}))?,
        );
        d.launches.insert(
            "host-launch".into(),
            serde_json::from_value(json!({"app":"pond","build":"one","email":"host@example.com","guest":false,"host":"pond.games.example.com","token_hash":platform::hash("host-game-token"),"exchanged":true,"expires":now()+3600,"grant_expires":now()+3600,"seen":now(),"lease_id":null,"lease_token":null}))?,
        );
        d.launches.insert(
            "guest-launch".into(),
            serde_json::from_value(json!({"app":"pond","build":"one","email":"host@example.com","guest":true,"host":"guest-pond.games.example.com","token_hash":platform::hash("guest-game-token"),"exchanged":true,"expires":now()+3600,"grant_expires":now()+3600,"seen":now(),"lease_id":null,"lease_token":null}))?,
        );
        Ok(())
    })?;
    p.reconcile_voice().await?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("origin", "https://games.example.com")
            .header("cookie", format!("member={who}"))
    };
    let game_req = |method, host_name: &str, token: &str| {
        http.request(method, format!("{url}/_infuser/voice"))
            .header("host", host_name)
            .header("origin", format!("https://{host_name}"))
            .bearer_auth(token)
    };

    // Game origin sees active=false and cannot patch when voice is off.
    let off: Value = game_req(
        reqwest::Method::GET,
        "pond.games.example.com",
        "host-game-token",
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(off["configured"], true);
    assert_eq!(off["active"], false);
    assert_eq!(
        game_req(
            reqwest::Method::PATCH,
            "pond.games.example.com",
            "host-game-token"
        )
        .json(&json!({"output_volume": 80}))
        .send()
        .await?
        .status(),
        409
    );

    // Validation & inspection boundaries on /api/voice/settings.
    assert_eq!(
        req(reqwest::Method::PATCH, "/api/voice/settings", "host")
            .json(&json!({"input_gain": 250}))
            .send()
            .await?
            .status(),
        422
    );
    assert_eq!(
        req(reqwest::Method::PATCH, "/api/voice/settings", "operator")
            .header("x-infuser-inspect-group", "away")
            .json(&json!({"input_gain": 90}))
            .send()
            .await?
            .status(),
        403
    );

    // Persist profile audio settings and verify isolation from other profiles.
    let saved: Value = req(reqwest::Method::PATCH, "/api/voice/settings", "host")
        .json(&json!({
            "input_gain": 135,
            "output_volume": 85,
            "auto_gain_control": false,
            "peer_volumes": { &friend_id: { "volume": 160, "muted": false } }
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(saved["settings"]["input_gain"], 135);
    assert_eq!(saved["settings"]["output_volume"], 85);
    assert_eq!(saved["settings"]["auto_gain_control"], false);
    assert_eq!(saved["settings"]["peer_volumes"][&friend_id]["volume"], 160);

    let friend_status: Value = req(reqwest::Method::GET, "/api/voice", "friend")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(friend_status["settings"]["input_gain"], 100);
    assert_eq!(friend_status["settings"]["auto_gain_control"], true);

    // Join voice for host and friend so LiveKit roster has both.
    let host_join: Value = req(reqwest::Method::POST, "/api/voice", "host")
        .json(&json!({"id": "host-voice-request-101"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let friend_join: Value = req(reqwest::Method::POST, "/api/voice", "friend")
        .json(&json!({"id": "friend-voice-request-101"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    for token in [
        host_join["token"].as_str().unwrap(),
        friend_join["token"].as_str().unwrap(),
    ] {
        let c = claims(token);
        media
            .lock()
            .unwrap()
            .participants
            .push(json!({"identity": c["sub"], "room": c["video"]["room"], "state": "ACTIVE"}));
    }
    p.reconcile_voice().await?;

    let host_status: Value = req(reqwest::Method::GET, "/api/voice", "host")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let rev_before = host_status["revision"].as_u64().unwrap();
    assert!(
        host_status["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == friend_id && p["participant_id"] == friend_join["id"])
    );
    assert_eq!(host_status["self"]["mic_muted"], false);
    assert_eq!(host_status["self"]["deafened"], false);

    // Guest game tab never sees or controls owner's voice session.
    let guest_view: Value = game_req(
        reqwest::Method::GET,
        "guest-pond.games.example.com",
        "guest-game-token",
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(guest_view["configured"], false);
    assert_eq!(guest_view["active"], false);
    assert_eq!(
        game_req(
            reqwest::Method::PATCH,
            "guest-pond.games.example.com",
            "guest-game-token"
        )
        .json(&json!({"mic_muted": true}))
        .send()
        .await?
        .status(),
        403
    );

    // Game origin cannot mutate processing flags or cross-origin requests.
    assert_eq!(
        game_req(
            reqwest::Method::PATCH,
            "pond.games.example.com",
            "host-game-token"
        )
        .json(&json!({"auto_gain_control": true}))
        .send()
        .await?
        .status(),
        422
    );
    assert_eq!(
        http.patch(format!("{url}/_infuser/voice"))
            .header("host", "pond.games.example.com")
            .header("origin", "https://evil.example")
            .bearer_auth("host-game-token")
            .json(&json!({"mic_muted": true}))
            .send()
            .await?
            .status(),
        403
    );

    // Library long-poll wakes immediately when game header updates mix + deafen.
    let control_wait = tokio::spawn({
        let http = http.clone();
        let url = url.clone();
        async move {
            http.get(format!("{url}/api/voice/control?since={rev_before}"))
                .header("host", "games.example.com")
                .header("origin", "https://games.example.com")
                .header("cookie", "member=host")
                .send()
                .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let game_patched: Value = game_req(
        reqwest::Method::PATCH,
        "pond.games.example.com",
        "host-game-token",
    )
    .json(&json!({
        "output_volume": 140,
        "deafened": true,
        "peer_volumes": { &friend_id: { "volume": 100, "muted": false } }
    }))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(game_patched["output_volume"], 140);
    assert_eq!(game_patched["deafened"], true);
    assert!(
        game_patched["peer_volumes"].as_object().unwrap().is_empty(),
        "default peer entries are pruned"
    );

    let woke: Value = tokio::time::timeout(std::time::Duration::from_secs(2), control_wait)
        .await???
        .error_for_status()?
        .json()
        .await?;
    assert!(woke["revision"].as_u64().unwrap() > rev_before);
    assert_eq!(woke["settings"]["output_volume"], 140);
    assert_eq!(woke["self"]["deafened"], true);

    // Unmuting mic while deafened also undeafens.
    let unmuted: Value = game_req(
        reqwest::Method::PATCH,
        "pond.games.example.com",
        "host-game-token",
    )
    .json(&json!({"mic_muted": false}))
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(unmuted["mic_muted"], false);
    assert_eq!(unmuted["deafened"], false);

    // Leaving from the game header removes the active lease.
    game_req(
        reqwest::Method::DELETE,
        "pond.games.example.com",
        "host-game-token",
    )
    .send()
    .await?
    .error_for_status()?;
    let after_leave: Value = game_req(
        reqwest::Method::GET,
        "pond.games.example.com",
        "host-game-token",
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(after_leave["active"], false);
    assert!(
        media
            .lock()
            .unwrap()
            .removed
            .contains(&host_join["id"].as_str().unwrap().to_owned())
    );
    Ok(())
}
