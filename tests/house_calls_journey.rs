use anyhow::{Result, ensure};
use axum::{
    Router,
    extract::Path,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        })),
    ))
}
async fn response(request: reqwest::RequestBuilder) -> Result<(u16, Value)> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}
async fn ok(request: reqwest::RequestBuilder) -> Result<Value> {
    let (status, body) = response(request).await?;
    ensure!(status == 200, "{status}: {body}");
    Ok(body)
}
fn payload(view: &Value) -> Value {
    json!({"images":view["images"],"quiet_hours":view["quiet_hours"]})
}

#[tokio::test]
async fn group_owned_house_calls_secrets_permissions_queue_cooldowns_and_quiet_hours() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let who = h
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("member="))
                .unwrap_or("");
            let known =
                ["admin", "one", "two", "blocked", "away-admin", "away-user"].contains(&who);
            (
                if known {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::UNAUTHORIZED
                },
                [
                    ("X-Auth-Request-Email", format!("{who}@example.com")),
                    ("X-Auth-Request-User", who.to_owned()),
                ],
            )
        }),
    ))
    .await?;
    let config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}",
        root.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    for who in ["admin", "one", "two", "blocked", "away-admin", "away-user"] {
        platform::member(
            &config,
            &format!("{who}@example.com"),
            who,
            who.ends_with("admin"),
            true,
        )?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?))?;
    p.db.edit(|data| {
        data.groups.names.insert("away".into(), "Away".into());
        for who in ["away-admin", "away-user"] {
            data.groups
                .members
                .insert(format!("{who}@example.com"), "away".into());
        }
        Ok(())
    })?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    };
    let settings = "/api/admin/house-calls";
    let initial = ok(req(Method::GET, settings, "admin")).await?;
    assert_eq!(initial["topic_set"], false);
    assert!(initial["topic_url"].is_null());
    assert_eq!(
        ok(req(Method::GET, "/api/house-calls", "one")).await?,
        json!({"images":[]})
    );
    for method in [Method::GET, Method::PUT] {
        assert_eq!(
            response(req(method, settings, "one").json(&payload(&initial)))
                .await?
                .0,
            403
        );
    }
    assert_eq!(
        response(req(Method::GET, settings, "unknown")).await?.0,
        401
    );
    let player = |name: &str| {
        p.db.read()
            .unwrap()
            .player_id(&format!("{name}@example.com"))
    };
    let records = Arc::new(Mutex::new(Vec::new()));
    let copy = records.clone();
    let (ntfy, _ntfy) = serve(Router::new().route(
        "/{topic}",
        post(
            move |Path(topic): Path<String>, h: HeaderMap, body: String| {
                let copy = copy.clone();
                async move {
                    copy.lock().unwrap().push((
                        topic,
                        h["tags"].to_str().unwrap().to_owned(),
                        body,
                    ));
                    StatusCode::OK
                }
            },
        ),
    ))
    .await?;
    let secret_a = format!("{ntfy}/private-topic-a");
    let secret_b = format!("{ntfy}/private-topic-b");
    let mut a = payload(&initial);
    a["topic_url"] = json!(secret_a);
    a["images"][0]["enabled"] = json!(true);
    a["images"][0]["label"] = json!("Family time");
    a["images"][0]["tag"] = json!("INFUSER_TIME");
    a["images"][0]["message"] = json!("{{profile}} says it is Infuser time");
    a["images"][0]["cooldown_minutes"] = json!(60);
    a["images"][0]["profiles"] = json!([player("one"), player("two")]);
    assert_eq!(
        ok(req(Method::PUT, settings, "admin").json(&a)).await?,
        json!({"ok":true})
    );
    let a_view = ok(req(Method::GET, settings, "admin")).await?;
    assert_eq!(a_view["topic_set"], true);
    assert!(!a_view.to_string().contains("private-topic"));
    assert!(!a_view.to_string().contains(&ntfy));
    assert_eq!(
        ok(req(Method::GET, "/api/house-calls", "one")).await?,
        json!({"images":[{"id":"confetti-cannon","label":"Family time"}]})
    );
    for who in ["blocked", "away-user"] {
        assert_eq!(
            ok(req(Method::GET, "/api/house-calls", who)).await?,
            json!({"images":[]})
        );
    }
    let send = |who: &str, nonce: &str, image: &str| {
        req(Method::POST, "/api/chat", who)
            .json(&json!({"client_id":format!("house-call-request-{nonce}"),"house_call":image}))
    };
    for (who, id) in [
        ("blocked", "confetti-cannon"),
        ("one", "not-shipped"),
        ("one", "tea-break"),
        ("away-user", "confetti-cannon"),
    ] {
        assert_eq!(response(send(who, "rejected", id)).await?.0, 422);
    }
    assert_eq!(response(req(Method::POST,"/api/chat","one").json(&json!({"client_id":"house-call-extra-body","house_call":"confetti-cannon","text":"Forged broadcast"}))).await?.0,422);
    assert_eq!(req(Method::POST,"/api/chat","one").json(&json!({"client_id":"house-call-extra-tag","house_call":"confetti-cannon","tag":"FORGED"})).send().await?.status(),422);
    let mut invalid = payload(&a_view);
    invalid["topic_url"] = json!("ftp://ntfy.example/private-secret-topic");
    let (status, error) = response(req(Method::PUT, settings, "admin").json(&invalid)).await?;
    assert_eq!(status, 422);
    assert!(!error.to_string().contains("private-secret-topic"));
    // Queue reservation is committed with the card, without contacting ntfy on this request.
    let first = ok(send("one", "first", "confetti-cannon")).await?;
    assert!(records.lock().unwrap().is_empty());
    assert_eq!(ok(send("one", "first", "confetti-cannon")).await?, first);
    ok(send("two", "second", "confetti-cannon")).await?;
    let chat = ok(req(Method::GET, "/api/chat", "one")).await?;
    assert_eq!(
        chat["messages"][0]["house_call"],
        json!({"id":"confetti-cannon","label":"Family time"})
    );
    assert_eq!(chat["messages"][0]["house_call_triggered"], true);
    assert_eq!(chat["messages"][1]["house_call_triggered"], false);
    assert!(!chat.to_string().contains("private-topic"));
    assert!(!chat.to_string().contains("INFUSER_TIME"));
    // A second group's settings and cooldown never reuse the first group's configuration.
    let mut b = payload(&ok(req(Method::GET, settings, "away-admin")).await?);
    assert_eq!(b["images"][0]["enabled"], false);
    b["topic_url"] = json!(secret_b);
    b["images"][0]["enabled"] = json!(true);
    b["images"][0]["tag"] = json!("away_b");
    b["images"][0]["message"] = json!("Away call");
    b["images"][0]["profiles"] = json!([player("away-user")]);
    ok(req(Method::PUT, settings, "away-admin").json(&b)).await?;
    ok(send("away-user", "away-first", "confetti-cannon")).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "away-user")).await?["messages"][0]["house_call_triggered"],
        true
    );
    assert_eq!(
        p.db.connection.lock().unwrap().query_row(
            "SELECT count(*) FROM house_call_outbox",
            [],
            |r| r.get::<_, u32>(0)
        )?,
        2
    );
    p.dispatch_house_calls().await?;
    p.dispatch_house_calls().await?;
    p.dispatch_house_calls().await?;
    assert_eq!(
        *records.lock().unwrap(),
        vec![
            (
                "private-topic-a".into(),
                "INFUSER_TIME".into(),
                "one says it is Infuser time".into()
            ),
            (
                "private-topic-b".into(),
                "away_b".into(),
                "Away call".into()
            )
        ]
    );
    let mut bad = payload(&a_view);
    bad["images"][0]["profiles"] = json!([player("away-user")]);
    assert_eq!(
        response(req(Method::PUT, settings, "admin").json(&bad))
            .await?
            .0,
        422
    );
    assert!(p.db.house_calls_admin("admin@example.com", "away").is_err());
    assert!(
        p.db.house_calls_set(
            "admin@example.com",
            "away",
            serde_json::from_value(payload(&a_view))?
        )
        .is_err()
    );
    assert_eq!(ok(req(Method::GET, settings, "admin")).await?, a_view);
    // Reopen persistent state: cooldowns survive restart and are shared across senders.
    let reopened = platform::Database::open(&config.controller.state_dir)?;
    let channel: String = reopened.connection.lock().unwrap().query_row(
        "SELECT id FROM chat_channels WHERE default_group='home'",
        [],
        |r| r.get(0),
    )?;
    reopened.chat_send_with_house_call(
        "two@example.com",
        &channel,
        "house-call-after-restart",
        "",
        None,
        Some("confetti-cannon"),
    )?;
    assert_eq!(
        reopened.chat_list("two@example.com")?["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["house_call_triggered"],
        false
    );
    // Quiet hours use a group-admin chosen zone; they don't hide cards or reserve cooldown.
    let mut quiet = payload(&a_view);
    quiet["images"][1]["enabled"] = json!(true);
    quiet["images"][1]["tag"] = json!("tea_a");
    quiet["images"][1]["profiles"] = json!([player("one")]);
    let hour = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .hour();
    quiet["quiet_hours"] = json!({"start":format!("{:02}:00",(hour+23)%24),"end":format!("{:02}:00",(hour+1)%24),"timezone":"UTC"});
    ok(req(Method::PUT, settings, "admin").json(&quiet)).await?;
    ok(send("one", "quiet", "tea-break")).await?;
    let chat = ok(req(Method::GET, "/api/chat", "one")).await?;
    assert_eq!(
        chat["messages"].as_array().unwrap().last().unwrap()["house_call_triggered"],
        false
    );
    assert_eq!(p.db.connection.lock().unwrap().query_row("SELECT count(*) FROM house_call_triggers WHERE private_group='home' AND image='tea-break'",[],|r|r.get::<_,u32>(0))?,0);
    assert!(ok(req(Method::GET, settings, "away-admin")).await?["quiet_hours"].is_null());
    b["images"][1]["enabled"] = json!(true);
    b["images"][1]["tag"] = json!("tea_b");
    b["images"][1]["profiles"] = json!([player("away-user")]);
    ok(req(Method::PUT, settings, "away-admin").json(&b)).await?;
    ok(send("away-user", "quiet-other-group", "tea-break")).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "away-user")).await?["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["house_call_triggered"],
        true,
        "another group's quiet hours cannot suppress this group"
    );
    p.dispatch_house_calls().await?;
    assert_eq!(records.lock().unwrap().last().unwrap().0, "private-topic-b");
    quiet["quiet_hours"] = Value::Null;
    ok(req(Method::PUT, settings, "admin").json(&quiet)).await?;
    ok(send("one", "after-quiet", "tea-break")).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "one")).await?["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["house_call_triggered"],
        true
    );
    // Full queue still allows a card, but cannot claim that a POST was queued.
    quiet["images"][1]["cooldown_minutes"] = json!(0);
    ok(req(Method::PUT, settings, "admin").json(&quiet)).await?;
    p.db.connection.lock().unwrap().execute("INSERT INTO house_call_outbox(private_group,topic_url,tag,body) SELECT 'home',?1,'test','' FROM json_each(?2)",rusqlite::params![secret_a,serde_json::to_string(&vec![0;64])?])?;
    ok(send("one", "queue-full", "tea-break")).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "one")).await?["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["house_call_triggered"],
        false
    );
    // Off means no topic OR no enabled images; clearing A cannot touch B's secret.
    quiet["topic_url"] = json!("");
    ok(req(Method::PUT, settings, "admin").json(&quiet)).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/house-calls", "one")).await?,
        json!({"images":[]})
    );
    assert_eq!(response(send("one", "off", "tea-break")).await?.0, 422);
    assert_eq!(
        ok(req(Method::GET, settings, "away-admin")).await?["topic_set"],
        true
    );
    quiet["topic_url"] = json!(secret_a);
    for image in quiet["images"].as_array_mut().unwrap() {
        image["enabled"] = json!(false);
    }
    ok(req(Method::PUT, settings, "admin").json(&quiet)).await?;
    assert_eq!(
        ok(req(Method::GET, "/api/house-calls", "one")).await?,
        json!({"images":[]})
    );
    assert_eq!(
        response(send("one", "disabled", "confetti-cannon"))
            .await?
            .0,
        422
    );
    for image in platform::house_calls::images() {
        let r = req(
            Method::GET,
            &format!("/assets/house-calls/{}.png", image.id),
            "one",
        )
        .send()
        .await?;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "image/png");
        assert!(!r.bytes().await?.is_empty());
    }
    assert_eq!(
        req(Method::GET, "/assets/house-calls/not-shipped.png", "one")
            .send()
            .await?
            .status(),
        404
    );
    Ok(())
}

#[derive(Clone)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn slow_ntfy_does_not_block_chat_and_failures_do_not_log_the_secret() -> Result<()> {
    let root = tempfile::tempdir()?;
    let config: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}",
        root.path().display().to_string()
    ))?;
    platform::member(&config, "admin@example.com", "Alex", true, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?))?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let ready = entered.clone();
    let (url, _server) = serve(Router::new().route(
        "/private-slow-topic",
        post(move || {
            let ready = ready.clone();
            async move {
                ready.notify_one();
                std::future::pending::<StatusCode>().await
            }
        }),
    ))
    .await?;
    let secret = format!("{url}/private-slow-topic");
    let view = p.db.house_calls_admin("admin@example.com", "home")?;
    let mut settings = payload(&view);
    settings["topic_url"] = json!(secret);
    settings["images"][0]["enabled"] = json!(true);
    settings["images"][0]["tag"] = json!("INFUSER_TIME");
    settings["images"][0]["cooldown_minutes"] = json!(0);
    settings["images"][0]["profiles"] = json!([p.db.read()?.player_id("admin@example.com")]);
    p.db.house_calls_set(
        "admin@example.com",
        "home",
        serde_json::from_value(settings)?,
    )?;
    p.db.chat_send("admin@example.com", "house-call-seed-message", "hello")?;
    let channel: String = p.db.connection.lock().unwrap().query_row(
        "SELECT id FROM chat_channels WHERE default_group='home'",
        [],
        |r| r.get(0),
    )?;
    p.db.chat_send_with_house_call(
        "admin@example.com",
        &channel,
        "house-call-slow-request",
        "",
        None,
        Some("confetti-cannon"),
    )?;
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || LogWriter(writer.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let worker = p.clone();
    let job = tokio::spawn(async move { worker.dispatch_house_calls().await });
    tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified()).await?;
    assert!(!job.is_finished());
    // The HTTP POST is still waiting. New chat writes and reads remain independent.
    p.db.chat_send(
        "admin@example.com",
        "house-call-during-outage",
        "still chatting",
    )?;
    assert_eq!(
        p.db.chat_list("admin@example.com")?["messages"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    tokio::time::timeout(std::time::Duration::from_secs(4), job).await???;
    let log = String::from_utf8(logs.lock().unwrap().clone())?;
    assert!(log.contains("House call notification was not accepted"));
    assert!(!log.contains("private-slow-topic"));
    assert!(!log.contains(&url));
    assert!(
        p.db.take_house_call()?.is_none(),
        "failed delivery does not retry a household action"
    );
    Ok(())
}
