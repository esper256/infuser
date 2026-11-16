use anyhow::Result;
use axum::{Router, routing::get};
use infuser::{
    api,
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    operations, portal,
};
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
