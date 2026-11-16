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
    for (email, name) in [("host@example.com", "River"), ("friend@example.com", "Sky")] {
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
            d.apps.insert(app.into(),App{build:"one".into(),updated:now(),listed:app!="hidden"});
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
    Ok(())
}
