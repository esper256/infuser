use anyhow::{Result, ensure};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
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
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Server)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", l.local_addr()?);
    Ok((
        url,
        Server(tokio::spawn(async move {
            axum::serve(l, router).await.unwrap()
        })),
    ))
}
async fn ok(request: reqwest::RequestBuilder) -> Result<Value> {
    let r = request.send().await?;
    let status = r.status();
    let value: Value = r.json().await?;
    ensure!(status.is_success(), "{status}: {value}");
    Ok(value)
}
#[tokio::test]
async fn captures_rotate_and_attachments_follow_chat_reaction_and_visit_access() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let who = h
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("member="))
                .unwrap_or("")
                .to_owned();
            if ["one", "two", "visitor"].contains(&who.as_str()) {
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
    let config: Config = toml::from_str(&format!(
        "[controller]\nprivate_groups=true\nstate_dir={:?}\nauth_url={:?}",
        dir.path().to_str().unwrap(),
        format!("{auth}/auth")
    ))?;
    for who in ["one", "two", "visitor"] {
        platform::member(&config, &format!("{who}@example.com"), who, false, true)?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?))?;
    p.db.edit(|d|{
   d.groups.names.insert("away".into(),"Away".into());d.groups.members.insert("visitor@example.com".into(),"away".into());
   d.apps.insert("pond".into(),serde_json::from_value(json!({"build":"one","updated":now(),"listed":true}))?);
   d.builds.insert("pond/one".into(),serde_json::from_value(json!({"manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},"server":null,"spa":false,"cross_origin_isolation":false,"ready":true,"created":now(),"files":{}}))?);
   d.launches.insert("launch".into(),serde_json::from_value(json!({"app":"pond","build":"one","email":"one@example.com","guest":false,"host":"pond.games.example.com","token_hash":platform::hash("game-token"),"exchanged":true,"expires":now()+3600,"grant_expires":now()+3600,"seen":now(),"lease_id":null,"lease_token":null}))?);Ok(())
 })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("origin", "https://games.example.com")
            .header("cookie", format!("member={who}"))
    };
    let capture = |body: &Value| {
        http.post(format!("{url}/_infuser/screenshots"))
            .header("host", "pond.games.example.com")
            .header("origin", "https://pond.games.example.com")
            .bearer_auth("game-token")
            .json(body)
    };
    let bytes = include_bytes!("../assets/avatars/circuit-fox-128.webp");
    let encoded = STANDARD.encode(bytes);
    let id = "00000000-0000-4000-8000-000000000001";
    let upload = json!({"id":id,"image":encoded,"thumbnail":encoded});
    let captured = ok(capture(&upload)).await?;
    let id = captured["id"].as_str().unwrap();
    ok(capture(&upload)).await?;
    assert_eq!(
        req(Method::GET, "/api/screenshots", "one")
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        ok(req(Method::GET, "/api/screenshots", "two")).await?["screenshots"],
        json!([])
    );
    assert_eq!(
        req(Method::GET, &format!("/api/screenshots/{id}"), "two")
            .send()
            .await?
            .status(),
        404
    );
    let message = json!({"client_id":"message-123456789","text":"Look!","screenshot":id});
    assert_eq!(
        req(Method::POST, "/api/chat", "two")
            .json(&message)
            .send()
            .await?
            .status(),
        422
    );
    let sent = ok(req(Method::POST, "/api/chat", "one").json(&message)).await?;
    let message_id = sent["id"].as_u64().unwrap();
    let image_path = format!("/api/screenshots/{id}?chat={message_id}&thumbnail=true");
    let first_read = req(Method::GET, &image_path, "two").send().await?;
    assert_eq!(first_read.status(), 200);
    assert_eq!(first_read.headers()["cache-control"], "private, no-cache");
    let etag = first_read.headers()["etag"].clone();
    let first_body: Value = first_read.json().await?;
    assert!(
        first_body["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/webp;base64,")
    );
    assert_eq!(
        req(Method::GET, &image_path, "two")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        304
    );
    assert_eq!(
        req(Method::GET, &image_path, "visitor")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        404
    );
    p.db.edit(|d| {
        d.visits.insert(
            "visit".into(),
            Visit {
                name: "Visit".into(),
                host: "home".into(),
                participants: [(
                    "visitor@example.com".into(),
                    Participant {
                        after_message: message_id,
                        ended: false,
                    },
                )]
                .into(),
                games: Default::default(),
                expires: now() + 3600,
                ended: false,
            },
        );
        Ok(())
    })?;
    assert_eq!(
        req(Method::GET, &image_path, "visitor")
            .header("x-infuser-visit", "visit")
            .send()
            .await?
            .status(),
        404,
        "visit cannot see earlier attachments"
    );
    let next = ok(req(Method::POST, "/api/chat", "one")
        .json(&json!({"client_id":"message-1234567890","text":"","screenshot":id})))
    .await?;
    let shared = format!("/api/screenshots/{id}?chat={}", next["id"]);
    ok(req(Method::GET, &shared, "visitor").header("x-infuser-visit", "visit")).await?;
    p.db.edit(|d| {
        d.visits.get_mut("visit").unwrap().ended = true;
        Ok(())
    })?;
    assert_eq!(
        req(Method::GET, &shared, "visitor")
            .header("x-infuser-visit", "visit")
            .send()
            .await?
            .status(),
        403
    );
    ok(req(
        Method::DELETE,
        &format!("/api/chat/{message_id}"),
        "one",
    ))
    .await?;
    assert_eq!(
        req(Method::GET, &image_path, "two").send().await?.status(),
        404
    );
    ok(req(Method::POST, "/api/chat", "one").json(&message)).await?; // tombstone retry stays deleted
    assert_eq!(
        req(Method::GET, &image_path, "two").send().await?.status(),
        404
    );
    let reaction = json!({"emoji":"⭐","text":"Great!","screenshot":id});
    assert_eq!(
        req(Method::PUT, "/api/apps/pond/reaction", "two")
            .json(&reaction)
            .send()
            .await?
            .status(),
        422
    );
    ok(req(Method::PUT, "/api/apps/pond/reaction", "one").json(&reaction)).await?;
    ok(req(
        Method::GET,
        &format!("/api/screenshots/{id}?app=pond"),
        "two",
    ))
    .await?;
    assert_eq!(
        req(
            Method::GET,
            &format!("/api/screenshots/{id}?app=pond"),
            "visitor"
        )
        .send()
        .await?
        .status(),
        404
    );
    p.db.edit(|d| {
        d.members
            .get_mut("two@example.com")
            .unwrap()
            .limited_communication = true;
        Ok(())
    })?;
    let library = ok(req(Method::GET, "/api/library", "two")).await?;
    assert_eq!(library["apps"][0]["reactions"][0]["emoji"], "⭐");
    assert!(library["apps"][0]["reactions"][0]["screenshot"].is_null());
    for path in [&shared, &format!("/api/screenshots/{id}?app=pond")] {
        assert_eq!(req(Method::GET, path, "two").send().await?.status(), 404);
    }
    p.db.edit(|d| {
        d.members
            .get_mut("two@example.com")
            .unwrap()
            .limited_communication = false;
        Ok(())
    })?;
    for n in 2..=11 {
        p.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE screenshots SET created=0", [])?;
        let shot = json!({"id":format!("00000000-0000-4000-8000-{n:012}"),"image":encoded,"thumbnail":encoded});
        ok(capture(&shot)).await?;
    }
    let list = ok(req(Method::GET, "/api/screenshots", "one")).await?;
    assert_eq!(list["screenshots"].as_array().unwrap().len(), 10);
    assert!(!list.to_string().contains(id));
    assert_eq!(
        req(Method::GET, &shared, "two").send().await?.status(),
        404,
        "eviction removes shared originals too"
    );
    let retained = format!("/api/screenshots/{id}?app=pond");
    ok(req(Method::GET, &retained, "two")).await?;
    let edited = json!({"emoji":"🚀","text":"Still here","screenshot":id});
    ok(req(Method::PUT, "/api/apps/pond/reaction", "one").json(&edited)).await?;
    ok(req(Method::DELETE, "/api/apps/pond/reaction", "one")).await?;
    assert_eq!(
        req(Method::GET, &retained, "two").send().await?.status(),
        404
    );
    assert_eq!(
        p.db.connection.lock().unwrap().query_row(
            "SELECT count(*) FROM screenshots",
            [],
            |row| row.get::<_, usize>(0)
        )?,
        10
    );
    let bad = json!({"id":"00000000-0000-4000-8000-000000000012","image":"not-an-image","thumbnail":encoded});
    p.db.connection
        .lock()
        .unwrap()
        .execute("UPDATE screenshots SET created=0", [])?;
    let replacement = STANDARD.encode(include_bytes!("../assets/avatars/loot-raccoon-128.webp"));
    let reuse = json!({"id":upload["id"],"image":replacement,"thumbnail":replacement});
    let new_image = ok(capture(&reuse)).await?;
    assert_ne!(
        new_image["id"], id,
        "reusing a nonce with different pixels cannot revive old attachments"
    );
    assert_eq!(
        req(Method::GET, &format!("/api/screenshots/{id}"), "one")
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(capture(&bad).send().await?.status(), 422);
    let large_bad = json!({"id":"00000000-0000-4000-8000-000000000012","image":STANDARD.encode(vec![0; 2 * 1024 * 1024]),"thumbnail":encoded});
    assert_eq!(
        capture(&large_bad).send().await?.status(),
        422,
        "capture route accepts bodies beyond the default 2 MiB, then validates the image"
    );
    let mut wide = Vec::new();
    image_webp::WebPEncoder::new(&mut wide).encode(
        &vec![0; 4097 * 3],
        4097,
        1,
        image_webp::ColorType::Rgb8,
    )?;
    let oversized = json!({"id":"00000000-0000-4000-8000-000000000013","image":STANDARD.encode(wide),"thumbnail":encoded});
    assert_eq!(
        capture(&oversized).send().await?.status(),
        422,
        "screenshots above 4K bounds are rejected before full-frame allocation"
    );
    assert_eq!(
        capture(&upload)
            .header("origin", "https://wrong.example")
            .send()
            .await?
            .status(),
        403
    );
    p.db.edit(|d| {
        d.members.get_mut("one@example.com").unwrap().enabled = false;
        Ok(())
    })?;
    assert_eq!(capture(&upload).send().await?.status(), 403);
    // Reopening performs the additive migration safely and retains the ten images.
    let reopened = platform::Database::open(dir.path())?;
    assert_eq!(
        reopened.screenshot_list("one@example.com")?["screenshots"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    Ok(())
}
