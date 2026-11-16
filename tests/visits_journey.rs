//! Real HTTP journeys: selected visitors, durable deployment saves, and chat windows.
use anyhow::{Result, ensure};
use axum::{Router, http::HeaderMap, routing::get};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform, portal,
};
use reqwest::{Client, Method, RequestBuilder};
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
async fn ok(r: RequestBuilder) -> Result<Value> {
    let r = r.send().await?;
    let status = r.status();
    let value: Value = r.json().await?;
    ensure!(status.is_success(), "{status}: {value}");
    Ok(value)
}
async fn denied(r: RequestBuilder) -> Result<()> {
    let r = r.send().await?;
    ensure!(
        [401, 403, 404, 409].contains(&r.status().as_u16()),
        "unexpected {}: {}",
        r.status(),
        r.text().await?
    );
    Ok(())
}
struct Web {
    url: String,
    http: Client,
}
impl Web {
    fn member(&self, who: &str, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("host", "games.example.com")
            .header("origin", "https://games.example.com")
            .header("cookie", format!("{who}@example.com"))
    }
    fn key(&self, key: &str, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("host", "games.example.com")
            .bearer_auth(key)
    }
    async fn publish(&self, who: &str) -> Result<String> {
        let setup = ok(self
            .member(who, Method::POST, "/api/my-games")
            .json(&json!({"title":"Visit game"})))
        .await?;
        let app = setup["app_id"].as_str().unwrap();
        let token = setup["setup_code"]
            .as_str()
            .unwrap()
            .strip_prefix("infuser_setup_")
            .unwrap();
        let claim = ok(self
            .key("", Method::POST, "/api/publisher/claim")
            .json(&json!({"token":token,"app_id":app})))
        .await?;
        let key = claim["token"].as_str().unwrap();
        let path = format!("/api/publisher/apps/{app}/builds/first");
        ok(self.key(key,Method::PUT,&path).json(&json!({"manifest":{"schema_version":1,"app_id":app,"title":"Visit game","play_modes":["solo"],"solo_runtime":"client_only","cloud_saves":{"groups":[{"id":"campaign","label":"Campaign","files":[{"path":"save.json","format":"json"}]}]}}}))).await?;
        ok(self
            .key(key, Method::PUT, &format!("{path}/files/index.html"))
            .body("<script type=module src='/_infuser/sdk.js'></script>"))
        .await?;
        ok(self
            .key(key, Method::POST, &format!("{path}/publish"))
            .json(&json!({"listed":true})))
        .await?;
        Ok(app.into())
    }
    async fn launch(&self, who: &str, app: &str, visit: Option<&str>) -> Result<Game> {
        let mut r = self
            .member(who, Method::POST, &format!("/api/apps/{app}/launch"))
            .json(&json!({}));
        if let Some(v) = visit {
            r = r.header("x-infuser-visit", v);
        }
        let launch = ok(r).await?;
        let url = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
        let token = url
            .fragment()
            .unwrap()
            .strip_prefix("infuser=")
            .unwrap()
            .to_owned();
        let game = Game {
            url: self.url.clone(),
            host: url.host_str().unwrap().into(),
            token,
            http: self.http.clone(),
        };
        ok(game
            .call(Method::POST, "/_infuser/session")
            .json(&json!({"token":game.token})))
        .await?;
        Ok(game)
    }
}
struct Game {
    url: String,
    host: String,
    token: String,
    http: Client,
}
impl Game {
    fn call(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("host", &self.host)
            .header("origin", format!("https://{}", self.host))
            .bearer_auth(&self.token)
    }
}

#[tokio::test]
async fn visits_resume_one_isolated_deployment_and_preserve_only_resident_chat_history()
-> Result<()> {
    let tmp = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            (
                axum::http::StatusCode::ACCEPTED,
                [(
                    "X-Auth-Request-Email",
                    h.get("cookie").unwrap().to_str().unwrap().to_owned(),
                )],
            )
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64\nprivate_groups=true\nvisits=true",
        tmp.path(),
        format!("{auth}/auth")
    ))?;
    platform::member(&cfg, "operator@example.com", "Operator", true, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    p.db.edit(|d| {
        d.groups.operators.insert("operator@example.com".into());
        d.groups.names.insert("away".into(), "Away".into());
        d.invite_to_group("away", "visitor@example.com", "Visitor", true, true)?;
        d.invite_to_group("away", "stays@example.com", "Stays home", false, true)?;
        d.invite_to_group("home", "resident@example.com", "Resident", false, true)?;
        Ok(())
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let w = Web {
        url,
        http: Client::new(),
    };
    let brought = w.publish("visitor").await?;
    let home = w.publish("operator").await?;
    if let Ok(node) = std::env::var("INFUSER_VISITS_BROWSER_NODE") {
        let status = tokio::process::Command::new(node)
            .args(["tests/visits_journey.mjs", &w.url, &brought])
            .status()
            .await?;
        ensure!(status.success(), "visits browser journey failed");
    }
    let original = w.launch("visitor", &brought, None).await?;
    ok(original
        .call(Method::PUT, "/_infuser/saves/progress")
        .json(&json!({"version":0,"data":{"level":99}})))
    .await?;
    let before = ok(w
        .member("operator", Method::POST, "/api/chat")
        .json(&json!({"client_id":"message-before-visit","text":"Before the visit"})))
    .await?;
    let create = json!({"name":"Saturday","host_group":"home","participants":[platform::hash("visitor@example.com")],"games":[brought],"expires_at":now()+3600});
    denied(
        w.member("visitor", Method::POST, "/api/operator/visits")
            .json(&create),
    )
    .await?;
    let visit = ok(w
        .member("operator", Method::POST, "/api/operator/visits")
        .json(&create))
    .await?["id"]
        .as_str()
        .unwrap()
        .to_owned();
    denied(
        w.member("operator", Method::POST, "/api/operator/visits")
            .json(&create),
    )
    .await?;
    p.db.push_subscribe(
        "visitor@example.com",
        &infuser::notifications::Subscription {
            endpoint: "https://fcm.googleapis.com/fcm/send/visit-device".into(),
        },
    )?;
    denied(
        w.member("stays", Method::GET, "/api/library")
            .header("x-infuser-visit", &visit),
    )
    .await?;
    denied(
        w.member("visitor", Method::GET, "/api/admin/members")
            .header("x-infuser-visit", &visit),
    )
    .await?;
    let library = ok(w
        .member("visitor", Method::GET, "/api/library")
        .header("x-infuser-visit", &visit))
    .await?;
    assert_eq!(library["apps"].as_array().unwrap().len(), 2);
    let visit_game = w.launch("visitor", &brought, Some(&visit)).await?;
    let context = ok(visit_game.call(Method::GET, "/_infuser/context")).await?;
    assert_eq!(context["app_id"], brought);
    assert_ne!(context["deployment_id"], brought);
    // Visibility and presentation belong to the source game, even if a borrowed
    // deployment still holds an older release's listing state.
    let mut art = Vec::new();
    image_webp::WebPEncoder::new(&mut art).encode(
        &vec![0; 1024 * 1024 * 3],
        1024,
        1024,
        image_webp::ColorType::Rgb8,
    )?;
    p.db.metadata_update(
        &brought,
        platform::MetadataUpdate {
            title: Some(Some("Renamed visit game")),
            art: Some(Some(&art)),
            ..Default::default()
        },
    )?;
    let revision = p.db.read()?.metadata[&brought]
        .card_art_revision
        .clone()
        .unwrap();
    let art_url = format!("/api/apps/{brought}/card-art/{revision}");
    p.db.edit(|d| {
        d.apps.get_mut(&brought).unwrap().listed = false;
        d.deployments
            .get_mut(context["deployment_id"].as_str().unwrap())
            .unwrap()
            .active
            .listed = false;
        Ok(())
    })?;
    denied(w.member("resident", Method::GET, &art_url)).await?;
    assert!(
        !ok(w.member("operator", Method::GET, "/api/library")).await?["apps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == brought)
    );
    p.db.edit(|d| {
        d.apps.get_mut(&brought).unwrap().listed = true;
        Ok(())
    })?;
    assert_eq!(
        w.member("resident", Method::GET, &art_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .as_ref(),
        art.as_slice()
    );
    assert!(
        ok(w.member("operator", Method::GET, "/api/operator/visits")).await?["games"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == brought && a["title"] == "Renamed visit game")
    );
    assert_eq!(
        ok(visit_game.call(Method::GET, "/_infuser/context")).await?["deployment_id"],
        context["deployment_id"]
    );
    assert_eq!(
        ok(visit_game.call(Method::GET, "/_infuser/saves/progress")).await?["data"],
        Value::Null
    );
    ok(visit_game
        .call(Method::PUT, "/_infuser/saves/progress")
        .json(&json!({"version":0,"data":{"level":7}})))
    .await?;
    let checkpoint = "/_infuser/cloud/v1/groups/campaign";
    ok(visit_game.call(Method::POST,checkpoint).json(&json!({"commit_id":"visit-checkpoint-00000001","expected_revision":null,"files":{"save.json":{"level":7}},"summary":"Level 7"}))).await?;
    assert!(ok(original.call(Method::GET, checkpoint)).await?["revision"].is_null());
    let host_game = w.launch("operator", &brought, None).await?;
    assert_eq!(
        ok(host_game.call(Method::GET, "/_infuser/context")).await?["deployment_id"],
        context["deployment_id"]
    );
    assert_eq!(
        ok(host_game.call(Method::GET, "/_infuser/saves/progress")).await?["data"],
        Value::Null
    );
    let chat = ok(w
        .member("visitor", Method::GET, "/api/chat")
        .header("x-infuser-visit", &visit))
    .await?;
    assert!(chat["messages"].as_array().unwrap().is_empty());
    denied(
        w.member(
            "visitor",
            Method::DELETE,
            &format!("/api/chat/{}", before["id"]),
        )
        .header("x-infuser-visit", &visit),
    )
    .await?;
    ok(w.member("visitor", Method::POST, "/api/chat")
        .header("x-infuser-visit", &visit)
        .json(&json!({"client_id":"message-during-visit","text":"See you next time"})))
    .await?;
    ok(w.member("operator", Method::POST, "/api/chat")
        .json(&json!({"client_id":"host-message-during-visit","text":"Good game"})))
    .await?;
    assert_eq!(
        p.db.push_targets()?.len(),
        1,
        "active visitors receive host chat notifications"
    );
    // Publishing stays home-owned even while the author is visiting.
    let new_game = w.publish("visitor").await?;
    p.reconcile_publications()?;
    assert_eq!(p.db.read()?.groups.app(&new_game), "away");
    assert!(
        ok(w.member("operator", Method::GET, "/api/library")).await?["apps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == new_game)
    );
    ok(w.member(
        "operator",
        Method::DELETE,
        &format!("/api/operator/visits/{visit}"),
    ))
    .await?;
    denied(visit_game.call(Method::GET, "/_infuser/context")).await?;
    denied(host_game.call(Method::GET, "/_infuser/context")).await?;
    denied(
        w.member("visitor", Method::GET, "/api/chat")
            .header("x-infuser-visit", &visit),
    )
    .await?;
    assert_eq!(
        ok(w.member("operator", Method::GET, "/api/chat")).await?["messages"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        p.db.push_targets()?.is_empty(),
        "expiry drops queued notifications before delivery"
    );
    assert_eq!(
        ok(original.call(Method::GET, "/_infuser/saves/progress")).await?["data"]["level"],
        99
    );
    assert_eq!(
        ok(w.member("operator", Method::GET, "/api/library")).await?["apps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let again = ok(w
        .member("operator", Method::POST, "/api/operator/visits")
        .json(&create))
    .await?["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(again, visit);
    denied(visit_game.call(Method::GET, "/_infuser/context")).await?;
    let resumed = w.launch("visitor", &brought, Some(&again)).await?;
    assert_eq!(
        ok(resumed.call(Method::GET, "/_infuser/context")).await?["deployment_id"],
        context["deployment_id"]
    );
    assert_eq!(
        ok(resumed.call(Method::GET, "/_infuser/saves/progress")).await?["data"]["level"],
        7
    );
    assert_eq!(
        ok(resumed.call(Method::GET, checkpoint)).await?["files"]["save.json"]["level"],
        7
    );
    assert!(
        ok(w.member("visitor", Method::GET, "/api/chat")
            .header("x-infuser-visit", &again))
        .await?["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Removing one participant revokes their launch, without changing home membership.
    ok(w.member(
        "operator",
        Method::DELETE,
        &format!(
            "/api/operator/visits/{again}/participants/{}",
            platform::hash("visitor@example.com")
        ),
    ))
    .await?;
    denied(resumed.call(Method::GET, "/_infuser/context")).await?;
    let home_game = w.launch("operator", &home, None).await?;
    ok(home_game.call(Method::GET, "/_infuser/context")).await?;
    assert_eq!(p.db.read()?.groups.member("visitor@example.com"), "away");
    ok(w.member(
        "operator",
        Method::DELETE,
        &format!("/api/operator/visits/{again}"),
    ))
    .await?;
    let third = ok(w
        .member("operator", Method::POST, "/api/operator/visits")
        .json(&create))
    .await?["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let expiring = w.launch("visitor", &brought, Some(&third)).await?;
    p.db.edit(|d| {
        d.visits.get_mut(&third).unwrap().expires = now();
        Ok(())
    })?;
    denied(expiring.call(Method::POST, "/_infuser/api/v1/heartbeat")).await?;
    denied(expiring.call(Method::GET, checkpoint)).await?;
    denied(
        w.member(
            "operator",
            Method::PUT,
            &format!("/api/operator/visits/{third}"),
        )
        .json(&json!({"expires_at":now()+3600})),
    )
    .await?;
    let reopened = platform::Database::open(tmp.path())?;
    assert_eq!(
        reopened.read()?.deployment_for(&brought, "home"),
        context["deployment_id"].as_str()
    );
    assert!(
        reopened
            .read()?
            .visit_access(&third, "visitor@example.com", now())
            .is_none()
    );
    Ok(())
}
