use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build},
    portal,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", l.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(l, router).await.unwrap();
        })),
    ))
}
const HOST: &str = "games.example.com";
struct Player {
    host: String,
    token: String,
}
impl Player {
    fn call(
        &self,
        http: &reqwest::Client,
        url: &str,
        method: Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        http.request(method, format!("{url}{path}"))
            .header("host", &self.host)
            .header("origin", format!("https://{}", self.host))
            .bearer_auth(&self.token)
    }
}
fn member(
    http: &reqwest::Client,
    url: &str,
    who: &str,
    method: Method,
    path: &str,
) -> reqwest::RequestBuilder {
    http.request(method, format!("{url}{path}"))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", format!("member={who}"))
}
async fn value(r: reqwest::RequestBuilder) -> Result<Value> {
    let r = r.send().await?;
    let status = r.status();
    let v = r.json().await?;
    anyhow::ensure!(status.is_success(), "{status}: {v}");
    Ok(v)
}
async fn launch(
    http: &reqwest::Client,
    url: &str,
    who: &str,
    guest: bool,
    intent: Option<&Value>,
) -> Result<Player> {
    let l = value(
        member(http, url, who, Method::POST, "/api/apps/pond/launch")
            .json(&json!({"guest":guest,"intent":intent})),
    )
    .await?;
    assert_eq!(l["state"], "ready");
    let u = reqwest::Url::parse(l["url"].as_str().unwrap())?;
    let p = Player {
        host: u.host_str().unwrap().into(),
        token: u
            .fragment()
            .unwrap()
            .strip_prefix("infuser=")
            .unwrap()
            .into(),
    };
    value(
        p.call(http, url, Method::POST, "/_infuser/session")
            .json(&json!({"token":p.token})),
    )
    .await?;
    Ok(p)
}
#[tokio::test]
async fn library_join_friend_is_opt_in_once_and_bound_to_friend_and_room() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (auth_url, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let email = match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=host") => "host@example.com",
                Some("member=host2") => "host2@example.com",
                Some("member=friend2") => "friend2@example.com",
                Some("member=friend") => "friend@example.com",
                _ => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                    );
                }
            };
            (
                StatusCode::ACCEPTED,
                [
                    ("X-Auth-Request-Email", email),
                    ("X-Auth-Request-User", email),
                ],
            )
        }),
    ))
    .await?;
    let secret = dir.path().join("turn.secret");
    std::fs::write(&secret, "test-relay-secret-at-least-32-characters")?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nport_end=8699\n[controller.turn]\nurls=['turn:games.example.com:8700?transport=udp']\nsecret_file={:?}\nrelay_port_start=8800\nrelay_port_end=8899",
        dir.path().display().to_string(),
        format!("{auth_url}/auth"),
        secret.display().to_string()
    ))?;
    for (email, name) in [
        ("host@example.com", "River"),
        ("friend@example.com", "Sky"),
        ("host2@example.com", "Lake"),
        ("friend2@example.com", "Ash"),
    ] {
        platform::member(&cfg, email, name, false, true)?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let db = p.db.clone();
    db.edit(|d|{
        for app in ["pond","plain","hidden"] {
            let manifest=json!({"schema_version":1,"app_id":app,"title":app,"play_modes":["solo","multiplayer"],"solo_runtime":"client_only","multiplayer":{"mode":"peer_hosted","transport":"webrtc","max_players":2,"join_policy":"anytime","direct_connect":"disabled","relay_fallback":"required"},"launch_intents":if app=="plain"{json!([])}else{json!(["join_friend"])}});
            d.builds.insert(format!("{app}/one"),Build{manifest:serde_json::from_value(manifest)?,server:None,spa:false,cross_origin_isolation:false,ready:true,created:now(),files:Default::default()});
            d.apps.insert(app.into(),App {publication: None,build:"one".into(),updated:now(),listed:app!="hidden"});
        }Ok(())
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let host = launch(&http, &url, "host", false, None).await?;
    value(host.call(&http, &url, Method::GET, "/_infuser/context")).await?;
    assert!(
        value(host.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
        .await?["intent"]
            .is_null()
    );
    let library_view = || value(member(&http, &url, "friend", Method::GET, "/api/library"));
    let game_mode = |mode: &str| {
        host.call(
            &http,
            &url,
            Method::PUT,
            "/_infuser/api/v1/presence/v1/activity",
        )
        .json(&json!({"activity":mode}))
    };
    for mode in ["title_screen", "single_player"] {
        value(game_mode(mode)).await?;
        let view = library_view().await?;
        let card = view["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "pond")
            .unwrap();
        assert_eq!(card["players"][0]["activity"], mode);
        assert_eq!(card["join_targets"], json!([]));
    }
    assert_eq!(game_mode("made_up").send().await?.status(), 422);
    value(member(
        &http,
        &url,
        "host",
        Method::POST,
        "/api/presence/input",
    ))
    .await?;
    for (age, expected) in [(0, "active"), (150, "idle"), (650, "away")] {
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE profile_input SET seen=?1", [now() - age])?;
        // Both the Library poll and the SDK heartbeat renew reachability only.
        value(member(&http, &url, "host", Method::GET, "/api/library")).await?;
        value(host.call(&http, &url, Method::POST, "/_infuser/heartbeat")).await?;
        let view = library_view().await?;
        let river = view["friends"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "River")
            .unwrap();
        assert_eq!(river["status"], expected);
    }
    let guest = launch(&http, &url, "host", true, None).await?;
    value(guest.call(&http, &url, Method::POST, "/_infuser/input")).await?;
    let view = library_view().await?;
    assert_eq!(
        view["friends"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "River")
            .unwrap()["status"],
        "away",
        "Guest play must not reset the profile's idle clock"
    );
    value(guest.call(&http, &url, Method::DELETE, "/_infuser/heartbeat")).await?;
    value(host.call(&http, &url, Method::POST, "/_infuser/input")).await?;
    let view = library_view().await?;
    assert_eq!(
        view["friends"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "River")
            .unwrap()["status"],
        "active"
    );
    let room = value(
        host.call(&http, &url, Method::POST, "/_infuser/peers")
            .json(&json!({"connection_id":"host-connection-0001","joinable":true})),
    )
    .await?;
    let library = value(member(&http, &url, "friend", Method::GET, "/api/library")).await?;
    assert_eq!(
        library["apps"].as_array().unwrap().len(),
        2,
        "hidden preview leaked"
    );
    let target = &library["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "pond")
        .unwrap()["join_targets"][0];
    assert_eq!(target["name"], "River");
    assert_eq!(target["session_id"], room["epoch"]);
    let avatar = &library["friends"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "River")
        .unwrap()["join_targets"][0];
    assert_eq!(avatar["app_id"], "pond");
    assert_eq!(avatar["player_id"], target["player_id"]);
    let intent = json!({"kind":"join_friend","session_id":target["session_id"],"player_id":target["player_id"]});
    assert_eq!(
        member(
            &http,
            &url,
            "friend",
            Method::POST,
            "/api/apps/plain/launch"
        )
        .json(&json!({"intent":intent}))
        .send()
        .await?
        .status(),
        422
    );
    assert_eq!(
        member(&http, &url, "friend", Method::POST, "/api/apps/pond/launch")
            .json(&json!({"intent":intent,"mode":"solo"}))
            .send()
            .await?
            .status(),
        400
    );
    let normal = launch(&http, &url, "friend", false, None).await?;
    let joiner = launch(&http, &url, "friend", false, Some(&intent)).await?;
    assert_ne!(
        normal.host, joiner.host,
        "a Join friend launch cannot replace an already-open tab's cookie"
    );
    assert!(
        value(normal.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
        .await?["intent"]
            .is_null()
    );
    let fresh_db = platform::Database::open(dir.path())?;
    assert_eq!(
        fresh_db
            .read()?
            .launches
            .values()
            .filter(|l| l.intent.is_some())
            .count(),
        1,
        "pending intent must be durable"
    );
    for _ in 0..2 {
        assert!(
            value(joiner.call(&http, &url, Method::GET, "/_infuser/context"))
                .await?
                .get("intent")
                .is_none(),
            "embedding wrapper must not consume the intent"
        );
    }
    assert_eq!(
        joiner
            .call(
                &http,
                &url,
                Method::POST,
                "/_infuser/launch-intent/v1/consume"
            )
            .header("origin", "https://other.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.post(format!("{url}/_infuser/launch-intent/v1/consume"))
            .header("host", &normal.host)
            .header("origin", format!("https://{}", normal.host))
            .bearer_auth(&joiner.token)
            .send()
            .await?
            .status(),
        401
    );
    let (one, two) = tokio::join!(
        value(joiner.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        )),
        value(joiner.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
    );
    let results = [one?, two?];
    assert_eq!(results.iter().filter(|r| r["intent"] == intent).count(), 1);
    assert_eq!(results.iter().filter(|r| r["intent"].is_null()).count(), 1);
    assert!(
        fresh_db
            .read()?
            .launches
            .values()
            .all(|l| l.intent.is_none())
    );
    let member_room=value(joiner.call(&http,&url,Method::POST,"/_infuser/peers").json(&json!({"connection_id":"friend-connection-0001","expected_epoch":intent["session_id"],"expected_player":intent["player_id"]}))).await?;
    assert_eq!(member_room["role"], "member");
    assert_eq!(member_room["epoch"], room["epoch"]);
    let full = value(member(&http, &url, "friend", Method::GET, "/api/library")).await?;
    assert!(
        full["apps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["join_targets"].as_array().unwrap().is_empty())
    );
    let departing = member_room["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == member_room["self_id"])
        .unwrap()["player_id"]
        .clone();
    value(joiner.call(
        &http,
        &url,
        Method::DELETE,
        &format!(
            "/_infuser/peers/{}",
            member_room["self_id"].as_str().unwrap()
        ),
    ))
    .await?;
    let no_friend=normal.call(&http,&url,Method::POST,"/_infuser/peers").json(&json!({"connection_id":"stale-friend-connection","expected_epoch":room["epoch"],"expected_player":departing})).send().await?;
    assert_eq!(
        no_friend.status(),
        410,
        "joining a departed friend must not join the remaining host"
    );
    value(
        host.call(
            &http,
            &url,
            Method::PUT,
            &format!(
                "/_infuser/peers/{}/joinability",
                room["self_id"].as_str().unwrap()
            ),
        )
        .json(&json!({"epoch":room["epoch"],"joinable":false})),
    )
    .await?;
    let closed = launch(&http, &url, "friend", true, Some(&intent)).await?;
    assert_eq!(
        value(closed.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
        .await?["intent"],
        intent
    );
    assert_eq!(closed.call(&http,&url,Method::POST,"/_infuser/peers").json(&json!({"connection_id":"closed-connection-0001","expected_epoch":intent["session_id"],"expected_player":intent["player_id"]})).send().await?.status(),409);
    assert_eq!(
        value(closed.call(&http, &url, Method::GET, "/_infuser/context")).await?["runtime"]["state"],
        "ready"
    );
    value(host.call(
        &http,
        &url,
        Method::DELETE,
        &format!("/_infuser/peers/{}", room["self_id"].as_str().unwrap()),
    ))
    .await?;
    let late = launch(&http, &url, "friend", true, Some(&intent)).await?;
    assert_eq!(
        value(late.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
        .await?["intent"],
        intent
    );
    assert_eq!(late.call(&http,&url,Method::POST,"/_infuser/peers").json(&json!({"connection_id":"late-connection-00001","expected_epoch":intent["session_id"],"expected_player":intent["player_id"]})).send().await?.status(),410);
    assert!(!p.peer_rooms().active("pond", now()));
    assert!(
        value(late.call(
            &http,
            &url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume"
        ))
        .await?["intent"]
            .is_null(),
        "refresh must not replay a failed join"
    );
    let raw: String =
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT data FROM platform", [], |r| r.get(0))?;
    assert!(
        serde_json::from_str::<Value>(&raw)?["builds"]["pond/one"]["manifest"]
            .get("launch_intents")
            .is_none()
    );
    assert_eq!(
        fresh_db.read()?.builds["pond/one"]
            .manifest
            .launch_intents
            .len(),
        1
    );
    two_independent_parties(&http, &url).await
}

async fn two_independent_parties(http: &reqwest::Client, url: &str) -> Result<()> {
    const PEERS: &str = "/_infuser/api/v1/peers";
    const PRESENCE: &str = "/_infuser/api/v1/presence/v1";
    let host_a = launch(http, url, "host", false, None).await?;
    let host_b = launch(http, url, "host2", false, None).await?;
    let create = |host: &Player| {
        host.call(http, url, Method::POST, PEERS)
            .json(&json!({"connection_id":"a-new-host-connection","create":true}))
    };
    let (a, b) = tokio::try_join!(value(create(&host_a)), value(create(&host_b)))?;
    assert_eq!(a["role"], "host");
    assert_eq!(b["role"], "host");
    assert_ne!(a["epoch"], b["epoch"]);
    let retry = value(create(&host_a)).await?;
    assert_eq!(retry["epoch"], a["epoch"]);
    assert_eq!(retry["self_id"], a["self_id"]);
    let snapshot = value(host_a.call(http, url, Method::GET, PRESENCE)).await?;
    assert_eq!(snapshot["peer_sessions"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["can_create"], true);
    assert!(snapshot["session"].is_null());
    assert_eq!(snapshot["can_start"], true);
    let library = value(member(http, url, "friend", Method::GET, "/api/library")).await?;
    let card = library["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["id"] == "pond")
        .unwrap();
    let targets = card["join_targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    let intent_for = |name: &str| {
        let target = targets
            .iter()
            .find(|target| target["name"] == name)
            .unwrap();
        json!({"kind":"join_friend","session_id":target["session_id"],"player_id":target["player_id"]})
    };
    let river = intent_for("River");
    let lake = intent_for("Lake");
    assert_eq!(river["session_id"], a["epoch"]);
    assert_eq!(lake["session_id"], b["epoch"]);
    let (sky, ash) = tokio::try_join!(
        launch(http, url, "friend", false, Some(&river)),
        launch(http, url, "friend2", false, Some(&lake))
    )?;
    async fn join_selected(http: &reqwest::Client, url: &str, guest: &Player) -> Result<Value> {
        let request = value(guest.call(
            http,
            url,
            Method::POST,
            "/_infuser/launch-intent/v1/consume",
        ))
        .await?;
        let target = &request["intent"];
        value(guest.call(http,url,Method::POST,PEERS).json(&json!({
            "connection_id":"friend-room-connection","expected_epoch":target["session_id"],"expected_player":target["player_id"]
        }))).await
    }
    let wrong=sky.call(http,url,Method::POST,PEERS).json(&json!({
        "connection_id":"wrong-room-connection","expected_epoch":a["epoch"],"expected_player":lake["player_id"]
    })).send().await?;
    assert_eq!(wrong.status(), 410);
    let (joined_a, joined_b) = tokio::try_join!(
        join_selected(http, url, &sky),
        join_selected(http, url, &ash)
    )?;
    for (joined, host, names) in [
        (&joined_a, &a, ["River", "Sky"]),
        (&joined_b, &b, ["Lake", "Ash"]),
    ] {
        assert_eq!(joined["epoch"], host["epoch"]);
        assert_eq!(joined["role"], "member");
        let members = joined["peers"].as_array().unwrap();
        assert_eq!(members.len(), 2);
        assert!(
            names
                .iter()
                .all(|name| members.iter().any(|p| p["name"] == *name))
        );
    }
    let send = |recipient: &Value| {
        host_a.call(http,url,Method::POST,
        &format!("{PEERS}/{}/signals",a["self_id"].as_str().unwrap())).json(&json!({
            "epoch":a["epoch"], "to":recipient["self_id"], "payload":{"type":"offer","description":"test"}
        }))
    };
    assert_eq!(send(&joined_b).send().await?.status(), 404);
    value(send(&joined_a)).await?;
    let poll = |player: &Player, room: &Value| {
        player.call(
            http,
            url,
            Method::GET,
            &format!("{PEERS}/{}", room["self_id"].as_str().unwrap()),
        )
    };
    assert_eq!(
        value(poll(&sky, &joined_a)).await?["signals"][0]["from"],
        a["self_id"]
    );
    assert_eq!(value(poll(&ash, &joined_b)).await?["signals"], json!([]));
    let full = value(host_b.call(http, url, Method::GET, PRESENCE)).await?;
    assert!(
        full["peer_sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["reason"] == "full")
    );
    assert_eq!(full["can_create"], true);
    value(host_a.call(
        http,
        url,
        Method::DELETE,
        &format!("{PEERS}/{}", a["self_id"].as_str().unwrap()),
    ))
    .await?;
    let remaining = value(ash.call(http, url, Method::GET, PRESENCE)).await?;
    assert_eq!(remaining["peer_sessions"].as_array().unwrap().len(), 1);
    assert_eq!(remaining["peer_sessions"][0]["id"], b["epoch"]);
    Ok(())
}
