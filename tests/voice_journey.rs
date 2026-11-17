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
