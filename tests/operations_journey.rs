use anyhow::Result;
use axum::{Router, routing::get};
use infuser::{
    api,
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    operations, peers, portal,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::sync::Arc;

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn maintenance_waits_for_work_and_players_then_closes_admission() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}",
        root.path().to_str().unwrap()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller.clone())?;
    let router = api::router(controller.clone(), "control")
        .merge(operations::routes(p.clone(), "control"))
        .merge(
            Router::new()
                .route("/work", get(|| async { "done" }))
                .layer(axum::middleware::from_fn_with_state(
                    controller.maintenance.clone(),
                    infuser::maintenance::admission,
                )),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::new();
    let begin = |purpose: &str, force: bool| {
        http.post(format!("{base}/v1/maintenance"))
            .bearer_auth("control")
            .json(&json!({"purpose":purpose,"force":force}))
    };
    assert_eq!(
        http.post(format!("{base}/v1/maintenance"))
            .json(&json!({"purpose":"backup"}))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(begin("backup", true).send().await?.status(), 400);
    let work = Arc::new(controller.maintenance.enter()?);
    let blocking_worker = work.clone();
    drop(work);
    // A disconnected HTTP request must not hide its still-running blocking job.
    assert_eq!(begin("update", true).send().await?.status(), 409);
    drop(blocking_worker);
    p.db.edit(|d| {
        d.launches.insert(
            "active".into(),
            serde_json::from_value(json!({
                "app":"pond","build":"one","email":"player@example.test","guest":false,
                "host":"pond.example.test","token_hash":"fixture","exchanged":true,
                "expires":now()+3600,"grant_expires":now()+3600,"seen":now(),
                "lease_id":null,"lease_token":null
            }))?,
        );
        Ok(())
    })?;
    assert_eq!(begin("backup", false).send().await?.status(), 409);
    assert_eq!(begin("update", false).send().await?.status(), 409);
    let forced: Value = begin("update", true)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    http.delete(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"id":forced["lease"]["id"]}))
        .send()
        .await?
        .error_for_status()?;
    p.db.edit(|d| {
        d.launches.clear();
        Ok(())
    })?;
    let lease: Value = begin("backup", false)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(http.get(format!("{base}/work")).send().await?.status(), 503);
    assert_eq!(
        http.get(format!("{base}/healthz")).send().await?.status(),
        200
    );
    assert_eq!(
        http.get(format!("{base}/v1/operations"))
            .send()
            .await?
            .status(),
        401
    );
    let status: Value = http
        .get(format!("{base}/v1/operations"))
        .bearer_auth("control")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["maintenance"], true);
    assert_eq!(status["database_ok"], true);
    http.delete(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"id":"wrong"}))
        .send()
        .await?;
    assert!(controller.maintenance.active());
    http.delete(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"id":lease["lease"]["id"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        http.get(format!("{base}/work"))
            .send()
            .await?
            .text()
            .await?,
        "done"
    );
    Ok(())
}

#[tokio::test]
async fn activation_names_an_open_game_and_does_not_wait_for_its_check_in() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}",
        root.path().to_str().unwrap()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller.clone())?;
    p.db.edit(|data| {
        data.members.insert(
            "player@example.test".into(),
            infuser::platform::Member {
                name: "Alex".into(),
                admin: false,
                enabled: true,
                avatar: None,
                avatar_prompt_seen: false,
                limited_communication: false,
            },
        );
        data.metadata.insert(
            "pond".into(),
            infuser::platform::GameMetadata {
                title: Some("Pond".into()),
                ..infuser::platform::GameMetadata::default()
            },
        );
        data.launches.insert(
            "active".into(),
            serde_json::from_value(json!({
                "app":"pond","build":"one","email":"player@example.test","guest":false,
                "host":"pond.example.test","token_hash":"fixture","exchanged":true,
                "expires":now()+3600,"grant_expires":now()+3600,"seen":now(),
                "lease_id":null,"lease_token":null
            }))?,
        );
        Ok(())
    })?;
    let router = operations::routes(p.clone(), "control");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::new();
    let release = "b".repeat(40);
    http.post(format!("{base}/v1/deployment"))
        .bearer_auth("control")
        .json(&json!({"release":release,"phase":"waiting","maintenance_blocked":false}))
        .send()
        .await?
        .error_for_status()?;
    // A machine reboot still waits. The binary update does not.
    assert_eq!(
        http.post(format!("{base}/v1/maintenance"))
            .bearer_auth("control")
            .json(&json!({"purpose":"update","force":false}))
            .send()
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    let admitted = admit(&http, &base, &release).await?;
    assert_eq!(admitted.0, StatusCode::OK, "{}", admitted.1);
    end_lease(&http, &base, &admitted.1).await?;
    let status: Value = http
        .get(format!("{base}/v1/operations"))
        .bearer_auth("control")
        .send()
        .await?
        .json()
        .await?;
    let activity = status["update_activity"].as_array().unwrap();
    assert!(
        activity.iter().any(|item| {
            item["holds"] == false
                && item["kind"] == "playing"
                && item["summary"]
                    .as_str()
                    .unwrap()
                    .contains("Alex has \"Pond\" open")
                && item["effect"]
                    .as_str()
                    .unwrap()
                    .contains("does not hold the update")
        }),
        "{status}"
    );
    assert!(status["update_holding"].as_array().unwrap().is_empty());
    let inflight = controller.maintenance.enter()?;
    let during = admit(&http, &base, &release).await?;
    assert_eq!(
        during.0,
        StatusCode::OK,
        "a live request must not stall activation: {}",
        during.1
    );
    end_lease(&http, &base, &during.1).await?;
    drop(inflight);

    p.peer_rooms().join(
        peers::Identity {
            diagnostics: Default::default(),
            launch: "active".into(),
            player: "player-1".into(),
            name: "Alex".into(),
            guest: false,
        },
        peers::Join {
            app: "pond",
            build: "one",
            connection: "connection-123456",
            max_players: 4,
            create: true,
            expected_epoch: None,
            expected_player: None,
            joinable: Some(true),
        },
        now(),
    )?;
    let shared = admit(&http, &base, &release).await?;
    assert_eq!(shared.0, StatusCode::CONFLICT, "{}", shared.1);
    assert!(shared.1.contains("shared game"));
    assert!(shared.1.contains("Pond"));
    p.controller.deployment.edit(|record| {
        record.available_since = Some(now().saturating_sub(11 * 60));
        Ok(())
    })?;
    let later = admit(&http, &base, &release).await?;
    assert_eq!(
        later.0,
        StatusCode::OK,
        "a shared game cannot hold a release for the whole session: {}",
        later.1
    );
    end_lease(&http, &base, &later.1).await?;

    let recipe =
        serde_json::from_value(json!({"runtime":"node","node_major":22,"entry":"server.js"}))?;
    p.db.edit(|data| {
        let mut job = infuser::server_package::Job::new(recipe);
        job.state = infuser::server_package::Phase::Queued;
        data.server_builds.insert("pond/one".into(), job);
        Ok(())
    })?;
    let queued = admit(&http, &base, &release).await?;
    assert_eq!(
        queued.0,
        StatusCode::OK,
        "a queued image survives the restart: {}",
        queued.1
    );
    end_lease(&http, &base, &queued.1).await?;
    p.db.edit(|data| {
        let job = data.server_builds.get_mut("pond/one").unwrap();
        job.state = infuser::server_package::Phase::Packaging;
        job.started = now();
        Ok(())
    })?;
    let packaging = admit(&http, &base, &release).await?;
    assert_eq!(packaging.0, StatusCode::CONFLICT, "{}", packaging.1);
    assert!(packaging.1.contains("Publishing"));
    p.db.edit(|data| {
        data.server_builds.get_mut("pond/one").unwrap().started =
            now().saturating_sub(2 * 60 * 60 + 5);
        Ok(())
    })?;
    let expired = admit(&http, &base, &release).await?;
    assert_eq!(
        expired.0,
        StatusCode::OK,
        "a build running for two hours no longer blocks: {}",
        expired.1
    );
    end_lease(&http, &base, &expired.1).await?;
    Ok(())
}

#[tokio::test]
async fn reboot_stays_strict_unless_the_release_requires_it() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}",
        root.path().to_str().unwrap()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller.clone())?;
    p.db.edit(|data| {
        data.metadata.insert(
            "pond".into(),
            infuser::platform::GameMetadata {
                title: Some("Pond".into()),
                ..infuser::platform::GameMetadata::default()
            },
        );
        data.launches.insert(
            "active".into(),
            serde_json::from_value(json!({
                "app":"pond","build":"one","email":"player@example.test","guest":true,
                "host":"pond.example.test","token_hash":"fixture","exchanged":true,
                "expires":now()+3600,"grant_expires":now()+3600,"seen":now(),
                "lease_id":null,"lease_token":null
            }))?,
        );
        Ok(())
    })?;
    let router = operations::routes(p.clone(), "control");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let http = reqwest::Client::new();
    let reboot = http
        .post(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"purpose":"reboot"}))
        .send()
        .await?;
    assert_eq!(reboot.status(), StatusCode::CONFLICT);
    let body = reboot.text().await?;
    assert!(body.contains("Pond"), "{body}");
    assert!(p.controller.deployment.read().host_wait.is_some());
    assert_eq!(
        http.post(format!("{base}/v1/maintenance"))
            .bearer_auth("control")
            .json(&json!({"purpose":"reboot","force":true}))
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let release = "b".repeat(40);
    http.post(format!("{base}/v1/deployment"))
        .bearer_auth("control")
        .json(&json!({"release":release,"phase":"waiting","host_steps":["reboot"]}))
        .send()
        .await?
        .error_for_status()?;
    let required = http
        .post(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"purpose":"reboot"}))
        .send()
        .await?;
    let required_status = required.status();
    let required_body = required.text().await?;
    assert_eq!(required_status, StatusCode::OK, "{required_body}");
    let required: Value = serde_json::from_str(&required_body)?;
    assert_eq!(required["policy"], "release");
    http.delete(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"id":required["lease"]["id"]}))
        .send()
        .await?
        .error_for_status()?;
    let ansible = http
        .post(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"purpose":"ansible"}))
        .send()
        .await?;
    assert_eq!(ansible.status(), StatusCode::CONFLICT);
    assert!(ansible.text().await?.contains("Pond"));
    p.controller.deployment.edit(|record| {
        record.boot_id = Some("previous-boot".into());
        record
            .host_required
            .push(infuser::deployment::HostStep::Reboot);
        Ok(())
    })?;
    p.controller.deployment.observe_boot()?;
    assert!(
        !p.controller
            .deployment
            .read()
            .host_required
            .contains(&infuser::deployment::HostStep::Reboot),
        "a new boot completes a required reboot"
    );
    Ok(())
}

async fn admit(http: &reqwest::Client, base: &str, release: &str) -> Result<(StatusCode, String)> {
    let response = http
        .post(format!("{base}/v1/deployment/admit"))
        .bearer_auth("control")
        .json(&json!({"release":release}))
        .send()
        .await?;
    let status = response.status();
    Ok((status, response.text().await?))
}

async fn end_lease(http: &reqwest::Client, base: &str, body: &str) -> Result<()> {
    let value: Value = serde_json::from_str(body)?;
    http.delete(format!("{base}/v1/maintenance"))
        .bearer_auth("control")
        .json(&json!({"id":value["lease"]["id"]}))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
