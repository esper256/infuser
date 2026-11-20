//! A candidate on trial serves reads and refuses user writes. Promotion is what
//! opens writes. A failed trial restores the pre-update snapshot without a
//! committed user write to discard, including after a candidate migration.

use anyhow::{Result, ensure};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    api,
    config::Config,
    controller::Controller,
    deploy_outcomes::{self, Kind, Outcome},
    docker::Docker,
    operations, platform, portal, probation, rollback,
};
use reqwest::Method;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::sync::Arc;

const TOKEN: &str = "probation-test-control-token-value";

struct Running {
    task: tokio::task::JoinHandle<()>,
    controller: Arc<Controller>,
}

impl Running {
    async fn shutdown(self) {
        self.task.abort();
        let _ = self.task.await;
        drop(self.controller);
    }
}

struct World {
    root: tempfile::TempDir,
    config: Config,
    auth: String,
    _auth: tokio::task::JoinHandle<()>,
}

impl World {
    async fn open() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let state = root.path().join("state");
        std::fs::create_dir(&state)?;
        let (auth, auth_task) = auth_server().await?;
        let token_file = root.path().join("control.secret");
        std::fs::write(&token_file, TOKEN)?;
        let config: Config = toml::from_str(&format!(
            "[controller]\nstate_dir={:?}\nauth_url={:?}\ntoken_file={:?}",
            state.display().to_string(),
            format!("{auth}/auth"),
            token_file.display().to_string(),
        ))?;
        for (email, name, admin) in [
            ("ada@example.com", "Ada", true),
            ("sam@example.com", "Sam", false),
            ("host@example.com", "Host", true),
        ] {
            platform::member(&config, email, name, admin, true)?;
        }
        platform::Database::open(&config.controller.state_dir)?
            .edit(|data| data.set_operator("ada@example.com", true))?;
        Ok(Self {
            root,
            config,
            auth,
            _auth: auth_task,
        })
    }

    async fn start(&self) -> Result<(String, Running)> {
        let controller = Arc::new(Controller::open(
            self.config.clone(),
            Arc::new(Docker {
                settings: self.config.controller.clone(),
            }),
        )?);
        let platform = portal::Platform::open(controller.clone())?;
        let router = api::router(controller.clone(), TOKEN)
            .merge(operations::routes(platform.clone(), TOKEN))
            .merge(portal::router(platform));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Ok((url, Running { task, controller }))
    }
}

async fn auth_server() -> Result<(String, tokio::task::JoinHandle<()>)> {
    let router = Router::new().route(
        "/auth",
        get(|headers: HeaderMap| async move {
            let who = headers
                .get("cookie")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("member="))
                .unwrap_or("");
            let identity = match who {
                "ada" => "ada@example.com",
                "sam" => "sam@example.com",
                "host" => "host@example.com",
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
                    ("X-Auth-Request-Email", identity),
                    ("X-Auth-Request-User", identity),
                ],
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Ok((url, task))
}

fn call(
    http: &reqwest::Client,
    url: &str,
    method: Method,
    path: &str,
    who: &str,
) -> reqwest::RequestBuilder {
    http.request(method, format!("{url}{path}"))
        .header("host", "games.example.com")
        .header("cookie", format!("member={who}"))
        .header("origin", "https://games.example.com")
}

async fn body(request: reqwest::RequestBuilder) -> Result<(StatusCode, Value)> {
    let response = request.send().await?;
    let status = response.status();
    let payload: Value = response.json().await?;
    Ok((status, payload))
}

fn texts(db: &std::path::Path) -> Result<Vec<String>> {
    let connection = Connection::open(db)?;
    let mut statement =
        connection.prepare("SELECT text FROM library_chat WHERE text IS NOT NULL ORDER BY id")?;
    let rows = statement.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn table_exists(db: &std::path::Path, name: &str) -> Result<bool> {
    let connection = Connection::open(db)?;
    Ok(connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |row| row.get::<_, i64>(0),
    )? == 1)
}

#[tokio::test]
async fn failed_candidate_keeps_earlier_writes_and_rejects_new_ones() -> Result<()> {
    let world = World::open().await?;
    let db = world.config.controller.state_dir.join("platform.sqlite");
    let (url, server) = world.start().await?;
    let http = reqwest::Client::new();
    let (status, saved) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "earlier-message01",
            "text": "earlier message"
        })),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {saved}");
    let message_id = saved["id"].as_u64().unwrap();
    let (status, profile) = body(
        call(&http, &url, Method::PUT, "/api/profile", "sam")
            .json(&json!({"avatar": "circuit-fox"})),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {profile}");
    server.shutdown().await;

    let snapshots = world.root.path().join("rollback");
    assert!(
        !snapshots.starts_with(&world.config.controller.state_dir),
        "snapshot directory must stay outside platform state"
    );
    rollback::capture(
        &world.config.controller.state_dir,
        &snapshots,
        &"a".repeat(40),
    )?;
    Connection::open(&db)?.execute_batch(
        "CREATE TABLE candidate_migration(id INTEGER PRIMARY KEY, note TEXT);
         INSERT INTO candidate_migration VALUES (1, 'backfill');",
    )?;
    probation::arm(&world.config.controller.state_dir, infuser::BUILD_SHA)?;

    let (url, server) = world.start().await?;
    let (status, health) = body(http.get(format!("{url}/healthz"))).await?;
    ensure!(status.is_success(), "{health}");
    assert_eq!(health["build"], infuser::BUILD_SHA);
    assert_eq!(health["probation"], true);
    let (status, chat) = body(call(&http, &url, Method::GET, "/api/chat", "sam")).await?;
    ensure!(status.is_success(), "{chat}");
    assert_eq!(chat["messages"][0]["text"], "earlier message");
    assert!(chat["reads"].as_array().unwrap().is_empty());

    let (status, rejected) = body(call(&http, &url, Method::POST, "/api/chat", "sam").json(
        &json!({
            "client_id": "trial-message-0001",
            "text": "written during the trial"
        }),
    ))
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejected["error"], probation::MESSAGE);
    assert_eq!(rejected["code"], probation::CODE);
    let (status, receipt) = body(
        call(&http, &url, Method::POST, "/api/chat/read", "sam")
            .json(&json!({"message": message_id})),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(receipt["code"], probation::CODE);
    let (status, avatar) = body(
        call(&http, &url, Method::PUT, "/api/profile", "sam")
            .json(&json!({"avatar": "moss-knight"})),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(avatar["code"], probation::CODE);
    let (status, reaction) = body(
        call(&http, &url, Method::PUT, "/api/apps/moon/reaction", "sam")
            .json(&json!({"emoji": "⭐", "text": ""})),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reaction["code"], probation::CODE);
    let form = http
        .post(format!(
            "{url}/member-link/abcdefghijklmnopqrstuvwxyz012345"
        ))
        .header("host", "games.example.com")
        .header("accept", "text/html")
        .send()
        .await?;
    assert_eq!(form.status(), StatusCode::SERVICE_UNAVAILABLE);
    let page = form.text().await?;
    assert!(page.contains(probation::MESSAGE), "{page}");
    assert!(
        page.contains("Nothing you just submitted was saved."),
        "{page}"
    );
    assert_eq!(texts(&db)?, vec!["earlier message".to_string()]);
    assert!(table_exists(&db, "candidate_migration")?);
    server.shutdown().await;

    rollback::restore(
        &snapshots.join("a".repeat(40)),
        &world.config.controller.state_dir,
    )?;
    probation::disarm(&world.config.controller.state_dir)?;
    assert!(!table_exists(&db, "candidate_migration")?);
    assert_eq!(texts(&db)?, vec!["earlier message".to_string()]);

    let (url, server) = world.start().await?;
    let (status, chat) = body(call(&http, &url, Method::GET, "/api/chat", "sam")).await?;
    ensure!(status.is_success(), "{chat}");
    assert_eq!(chat["messages"].as_array().unwrap().len(), 1);
    assert_eq!(chat["messages"][0]["text"], "earlier message");
    assert!(chat["reads"].as_array().unwrap().is_empty());
    let (status, me) = body(call(&http, &url, Method::GET, "/api/me", "sam")).await?;
    ensure!(status.is_success(), "{me}");
    assert_eq!(me["avatar"], "circuit-fox");
    let (status, kept) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "kept-message-00001",
            "text": "kept after rollback"
        })),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {kept}");
    server.shutdown().await;
    let (url, server) = world.start().await?;
    let (status, chat) = body(call(&http, &url, Method::GET, "/api/chat", "sam")).await?;
    ensure!(status.is_success(), "{chat}");
    let saved: Vec<_> = chat["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        saved,
        vec![
            "earlier message".to_string(),
            "kept after rollback".to_string()
        ]
    );
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn promoted_candidate_accepts_writes() -> Result<()> {
    let world = World::open().await?;
    probation::arm(&world.config.controller.state_dir, infuser::BUILD_SHA)?;
    let (url, server) = world.start().await?;
    let http = reqwest::Client::new();
    let (status, rejected) = body(call(&http, &url, Method::POST, "/api/chat", "sam").json(
        &json!({
            "client_id": "before-promote-001",
            "text": "not yet"
        }),
    ))
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejected["code"], probation::CODE);
    assert_eq!(
        http.post(format!("{url}/v1/deployment/promote"))
            .bearer_auth("nope")
            .json(&json!({"release": infuser::BUILD_SHA}))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let wrong = http
        .post(format!("{url}/v1/deployment/promote"))
        .bearer_auth(TOKEN)
        .json(&json!({"release": "a".repeat(40)}))
        .send()
        .await?;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body(http.get(format!("{url}/healthz"))).await?.1["probation"],
        true
    );
    let promoted = http
        .post(format!("{url}/v1/deployment/promote"))
        .bearer_auth(TOKEN)
        .json(&json!({"release": infuser::BUILD_SHA}))
        .send()
        .await?;
    let promoted_status = promoted.status();
    let promoted: Value = promoted.json().await?;
    ensure!(
        promoted_status.is_success(),
        "{promoted_status}: {promoted}"
    );
    assert_eq!(promoted["probation"], false);
    assert_eq!(promoted["build"], infuser::BUILD_SHA);
    assert_eq!(
        body(http.get(format!("{url}/healthz"))).await?.1["probation"],
        false
    );
    let (status, saved) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "after-promote-0001",
            "text": "accepted after promotion"
        })),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {saved}");
    let (status, reaction) = body(
        call(&http, &url, Method::PUT, "/api/apps/moon/reaction", "sam")
            .json(&json!({"emoji": "⭐", "text": ""})),
    )
    .await?;
    assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(reaction["code"], probation::CODE);
    server.shutdown().await;

    assert!(!probation::holds_this_build(
        &world.config.controller.state_dir
    ));
    let (url, server) = world.start().await?;
    let (status, chat) = body(call(&http, &url, Method::GET, "/api/chat", "sam")).await?;
    ensure!(status.is_success(), "{chat}");
    assert_eq!(chat["messages"][0]["text"], "accepted after promotion");
    let (status, again) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "still-open-000001",
            "text": "still open"
        })),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {again}");
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn rollback_notice_is_shown_to_operators_only() -> Result<()> {
    let world = World::open().await?;
    let release = "d".repeat(40);
    deploy_outcomes::record(
        &world.config.controller.state_dir,
        Outcome {
            release: "a".repeat(40),
            at: 1_699_000_000,
            result: Kind::Activated,
            reason: String::new(),
        },
    )?;
    deploy_outcomes::record(
        &world.config.controller.state_dir,
        Outcome {
            release: release.clone(),
            at: 1_700_000_000,
            result: Kind::RolledBack,
            reason: probation::HEALTH_FAILURE.into(),
        },
    )?;
    let (url, server) = world.start().await?;
    let http = reqwest::Client::new();
    let (status, update) = body(call(
        &http,
        &url,
        Method::GET,
        "/api/operator/update",
        "ada",
    ))
    .await?;
    ensure!(status.is_success(), "{status}: {update}");
    assert_eq!(update["rollback_notice"]["release"], release);
    assert_eq!(
        update["rollback_notice"]["reason"],
        probation::HEALTH_FAILURE
    );
    assert_eq!(update["outcomes"][0]["result"], "rolled_back");
    assert_eq!(update["outcomes"][1]["result"], "activated");
    assert_eq!(
        call(&http, &url, Method::GET, "/api/operator/update", "host")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&http, &url, Method::GET, "/api/operator/update", "sam")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    for path in ["/api/me", "/api/library"] {
        let (status, payload) = body(call(&http, &url, Method::GET, path, "sam")).await?;
        ensure!(status.is_success(), "{path} {status}: {payload}");
        assert!(
            !payload.to_string().contains(probation::HEALTH_FAILURE),
            "{path} exposed the operator notice"
        );
    }
    assert_eq!(
        call(
            &http,
            &url,
            Method::POST,
            "/api/operator/update/notice",
            "host"
        )
        .json(&json!({"release": release}))
        .send()
        .await?
        .status(),
        StatusCode::FORBIDDEN
    );
    let (status, dismissed) = body(
        call(
            &http,
            &url,
            Method::POST,
            "/api/operator/update/notice",
            "ada",
        )
        .json(&json!({"release": release})),
    )
    .await?;
    ensure!(status.is_success(), "{dismissed}");
    let (status, update) = body(call(
        &http,
        &url,
        Method::GET,
        "/api/operator/update",
        "ada",
    ))
    .await?;
    ensure!(status.is_success(), "{update}");
    assert!(update["rollback_notice"].is_null());
    assert_eq!(update["outcomes"][0]["result"], "rolled_back");
    assert_eq!(update["outcomes"][0]["reason"], probation::HEALTH_FAILURE);
    server.shutdown().await;
    let _keep_auth = &world.auth;
    Ok(())
}

#[tokio::test]
async fn operator_can_authorize_an_update_while_writes_are_held() -> Result<()> {
    let world = World::open().await?;
    let (url, server) = world.start().await?;
    let http = reqwest::Client::new();
    let staged = "b".repeat(40);
    let reported = http
        .post(format!("{url}/v1/deployment"))
        .bearer_auth(TOKEN)
        .json(&json!({
            "release": staged,
            "phase": "waiting",
            "maintenance_blocked": false
        }))
        .send()
        .await?;
    ensure!(reported.status().is_success(), "{}", reported.text().await?);
    server.shutdown().await;
    probation::arm(&world.config.controller.state_dir, infuser::BUILD_SHA)?;
    deploy_outcomes::record(
        &world.config.controller.state_dir,
        Outcome {
            release: "c".repeat(40),
            at: 1_700_000_000,
            result: Kind::RolledBack,
            reason: probation::HEALTH_FAILURE.into(),
        },
    )?;
    let (url, server) = world.start().await?;

    let (status, chat) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "held-chat-0000001",
            "text": "still held"
        })),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(chat["code"], probation::CODE);

    let (status, group) = body(
        call(&http, &url, Method::POST, "/api/operator/groups", "ada").json(&json!({
            "name": "Side room",
            "admin_email": "ada@example.com",
            "admin_name": "Ada"
        })),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{group}");
    assert_eq!(group["code"], probation::CODE);
    let (status, link) = body(
        call(
            &http,
            &url,
            Method::POST,
            "/api/operator/members/sam@example.com/sign-in-link",
            "ada",
        )
        .json(&json!({})),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{link}");
    assert_eq!(link["code"], probation::CODE);

    let (status, authorized) = body(
        call(&http, &url, Method::POST, "/api/operator/update", "ada").json(&json!({
            "request_id": "a".repeat(32),
            "current_build": infuser::BUILD_SHA
        })),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {authorized}");
    let (status, stranger) = body(
        call(&http, &url, Method::POST, "/api/operator/update", "sam").json(&json!({
            "request_id": "b".repeat(32),
            "current_build": infuser::BUILD_SHA
        })),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN, "{stranger}");
    assert_ne!(stranger["code"], probation::CODE);
    let (status, dismissed) = body(
        call(
            &http,
            &url,
            Method::POST,
            "/api/operator/update/notice",
            "ada",
        )
        .json(&json!({"release": "c".repeat(40)})),
    )
    .await?;
    ensure!(status.is_success(), "{status}: {dismissed}");

    let (status, chat) = body(
        call(&http, &url, Method::POST, "/api/chat", "sam").json(&json!({
            "client_id": "held-chat-0000002",
            "text": "still held after the operator acted"
        })),
    )
    .await?;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(chat["code"], probation::CODE);
    server.shutdown().await;
    Ok(())
}
