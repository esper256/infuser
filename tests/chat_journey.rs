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
    platform, portal,
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        })),
    ))
}
async fn ok(request: reqwest::RequestBuilder) -> Result<Value> {
    let r = request.send().await?;
    let status = r.status();
    let body: Value = r.json().await?;
    anyhow::ensure!(status.is_success(), "{status}: {body}");
    Ok(body)
}

#[tokio::test]
async fn shared_chat_permissions_retries_retention_and_update_detection() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=admin") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "admin@example.com"),
                        ("X-Auth-Request-User", "admin@example.com"),
                    ],
                ),
                Some("member=one") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "one@example.com"),
                        ("X-Auth-Request-User", "one@example.com"),
                    ],
                ),
                Some("member=two") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "two@example.com"),
                        ("X-Auth-Request-User", "two@example.com"),
                    ],
                ),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                ),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    for who in ["admin", "one", "two"] {
        platform::member(
            &cfg,
            &format!("{who}@example.com"),
            who,
            who == "admin",
            true,
        )?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let req = |method, path: &str, who: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
    };
    let send = |who: &str, id: &str, text: &str| {
        req(Method::POST, "/api/chat", who)
            .header("origin", "https://games.example.com")
            .json(&json!({"client_id":id,"text":text}))
    };
    let remove = |who: &str, id: u64| {
        req(Method::DELETE, &format!("/api/chat/{id}"), who)
            .header("origin", "https://games.example.com")
    };
    assert_eq!(
        req(Method::GET, "/api/chat", "unknown")
            .header("x-infuser-verified-email", "admin@example.com")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        req(Method::POST, "/api/chat", "one")
            .json(&json!({"client_id":"request-00000001","text":"hi"}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        send("one", "request-00000001", "hi")
            .header("origin", "https://other.example.com")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        send("one", "request-00000001", "hi")
            .json(&json!({"client_id":"request-00000001","text":"hi","author":"admin"}))
            .send()
            .await?
            .status(),
        422
    );
    for text in [" ".into(), "a".repeat(281), "bad\u{0000}text".into()] {
        assert_eq!(
            send("one", "request-00000001", &text)
                .send()
                .await?
                .status(),
            422
        );
    }
    assert_eq!(
        send("one", "request-00000001", "hi")
            .body("x".repeat(5000))
            .send()
            .await?
            .status(),
        413
    );
    let first = ok(send(
        "one",
        "request-00000001",
        "  <img src=x onerror=alert(1)> 🎮\nhi  ",
    ))
    .await?;
    let cached = req(Method::GET, "/api/chat", "two").send().await?;
    let etag = cached.headers()["etag"].clone();
    assert_eq!(cached.headers()["cache-control"], "private, no-cache");
    assert_eq!(
        req(Method::GET, "/api/chat", "two")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        304
    );
    assert_eq!(
        req(Method::GET, "/api/chat", "unknown")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        401
    );
    p.db.edit(|d| {
        d.members.get_mut("one@example.com").unwrap().name = "Renamed".into();
        Ok(())
    })?;
    assert_eq!(
        req(Method::GET, "/api/chat", "two")
            .header("if-none-match", &etag)
            .send()
            .await?
            .status(),
        200
    );
    p.db.edit(|d| {
        d.members.get_mut("one@example.com").unwrap().name = "one".into();
        Ok(())
    })?;
    let library = req(Method::GET, "/api/library", "one").send().await?;
    let tag = library.headers()["etag"].clone();
    assert_eq!(
        req(Method::GET, "/api/library", "one")
            .header("if-none-match", &tag)
            .send()
            .await?
            .status(),
        304
    );
    assert_eq!(
        ok(send(
            "one",
            "request-00000001",
            "<img src=x onerror=alert(1)> 🎮\nhi"
        ))
        .await?,
        first
    );
    assert_eq!(
        send("one", "request-00000001", "different")
            .send()
            .await?
            .status(),
        409
    );
    let listing = ok(req(Method::GET, "/api/chat", "two")).await?;
    assert_eq!(listing["messages"].as_array().unwrap().len(), 1);
    assert_eq!(listing["messages"][0]["author"]["name"], "one");
    assert_eq!(listing["messages"][0]["can_delete"], false);
    assert!(!listing.to_string().contains("@example.com"));
    let id = first["id"].as_u64().unwrap();
    assert_eq!(remove("two", id).send().await?.status(), 403);
    assert_eq!(
        req(Method::DELETE, &format!("/api/chat/{id}"), "admin")
            .send()
            .await?
            .status(),
        403
    );
    ok(remove("admin", id)).await?;
    ok(send(
        "one",
        "request-00000001",
        "<img src=x onerror=alert(1)> 🎮\nhi",
    ))
    .await?;
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "two")).await?["messages"],
        json!([])
    );
    let own = ok(send("two", "request-00000002", "hello")).await?;
    ok(remove("two", own["id"].as_u64().unwrap())).await?;
    for i in 2..=10 {
        ok(send("one", &format!("request-{i:08}"), "hi")).await?;
    }
    assert_eq!(
        send("one", "request-00000011", "hi").send().await?.status(),
        429
    );
    p.db.edit(|d| {
        let member = d.members.get_mut("one@example.com").unwrap();
        member.name = "Comet".into();
        member.avatar = Some("pulse-pad".into());
        Ok(())
    })?;
    let changed = ok(req(Method::GET, "/api/chat", "two")).await?;
    assert_eq!(changed["messages"][0]["author"]["name"], "Comet");
    assert_eq!(changed["messages"][0]["author"]["avatar"], "pulse-pad");
    p.db.edit(|d| {
        d.members.get_mut("one@example.com").unwrap().enabled = false;
        Ok(())
    })?;
    assert_eq!(
        send("one", "request-00000012", "hi").send().await?.status(),
        403
    );
    assert_eq!(
        req(Method::GET, "/api/chat", "one").send().await?.status(),
        403
    );
    assert_eq!(
        ok(req(Method::GET, "/api/chat", "two")).await?["messages"],
        json!([])
    );

    // HTML and API agree before polling; activity blocks refresh for both guest and member launches.
    let state = ok(req(Method::GET, "/api/client-state", "two")).await?;
    let page = req(Method::GET, "/library", "two").send().await?;
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert!(page.text().await?.contains(&format!(
        "content=\"{}\"",
        state["version"].as_str().unwrap()
    )));
    assert_eq!(state["game_open"], false);
    p.db.edit(|d| {
        d.launches.insert(
            "test".into(),
            platform::Launch {
                deployment: None,
                visit: None,
                intent: None,
                app: "test".into(),
                build: "one".into(),
                email: "two@example.com".into(),
                guest: true,
                host: "test.example.com".into(),
                token_hash: "test".into(),
                exchanged: true,
                expires: now() + 300,
                grant_expires: 0,
                seen: now(),
                lease_id: None,
                lease_token: None,
            },
        );
        Ok(())
    })?;
    assert_eq!(
        ok(req(Method::GET, "/api/client-state", "two")).await?["game_open"],
        true
    );
    assert_eq!(
        ok(req(Method::GET, "/api/client-state", "admin")).await?["game_open"],
        false
    );
    p.db.edit(|d| {
        d.launches.get_mut("test").unwrap().seen = now() - 91;
        Ok(())
    })?;
    assert_eq!(
        ok(req(Method::GET, "/api/client-state", "two")).await?["game_open"],
        false
    );

    // Bounded durable storage survives catalog edits and reopen. Age the test sender to avoid sleeping.
    for i in 0..110 {
        p.db.connection
            .lock()
            .unwrap()
            .execute("UPDATE library_chat SET created=?1", [now() - 61])?;
        p.db.chat_send(
            "admin@example.com",
            &format!("retention-{i:08}"),
            "retained",
        )?;
    }
    let reopened = platform::Database::open(temp.path())?;
    assert_eq!(
        reopened.chat_list("two@example.com")?["messages"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
    reopened
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE library_chat SET created=0", [])?;
    assert_eq!(
        reopened.chat_list("two@example.com")?["messages"],
        json!([])
    );
    reopened.chat_send("two@example.com", "request-retained", "fresh")?;
    let count: u64 = reopened.connection.lock().unwrap().query_row(
        "SELECT count(*) FROM library_chat",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 1);
    Ok(())
}
