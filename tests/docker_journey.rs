use anyhow::{Context, Result, ensure};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Docker, Engine},
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires Docker access and INFUSER_TEST_IMAGE containing the built demo image ID"]
async fn real_game_wakes_serves_sleeps_and_wakes_again() -> Result<()> {
    let image = std::env::var("INFUSER_TEST_IMAGE")
        .context("build examples/demo/Dockerfile and set INFUSER_TEST_IMAGE")?;
    let dir = tempfile::tempdir()?;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = reservation.local_addr()?.port();
    let config: Config = toml::from_str(&format!(
        r#"
[controller]
state_dir = "{}"
public_host = "127.0.0.1"
publish_ip = "127.0.0.1"
memory_budget_mb = 256
cpu_budget_millis = 500
port_start = {port}
port_end = {port}
[games.demo]
image = "{image}"
container_port = 7000
memory_mb = 128
cpu_millis = 250
idle_seconds = 1
"#,
        dir.path().display()
    ))?;
    drop(reservation);
    let engine = Arc::new(Docker {
        settings: config.controller.clone(),
    });
    let c = Controller::open(config, engine.clone())?;
    engine.preflight().await?;
    let journey = async {
        let rounds = std::env::var("INFUSER_TEST_ROUNDS")
            .ok()
            .map(|v| v.parse::<usize>())
            .transpose()?
            .unwrap_or(2);
        ensure!(
            (2..=100).contains(&rounds),
            "INFUSER_TEST_ROUNDS must be 2–100"
        );
        for round in 0..rounds {
            let lease = c.acquire("demo", "test-player", now())?;
            let lease_id = lease["lease_id"].as_str().unwrap();
            let token = lease["lease_token"].as_str().unwrap();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
            loop {
                c.reconcile(now()).await?;
                let state = c.lease("demo", lease_id, token, "renew", now())?;
                if state["state"] == "ready" {
                    break;
                }
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "game did not become ready"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let response: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/"))
                .await?
                .error_for_status()?
                .json()
                .await?;
            ensure!(
                response["visits"] == 1,
                "round {round}: expected a fresh game process"
            );
            c.lease("demo", lease_id, token, "delete", now())?;
            c.reconcile(now() + 2).await?;
            ensure!(
                c.list(now())["reserved_memory_mb"] == 0,
                "resources not released"
            );
            ensure!(
                engine
                    .inventory()
                    .await?
                    .iter()
                    .all(|(_, owner)| owner != &c.snapshot().owner),
                "container still present"
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    // Cleanup is attempted even on a failed journey; never prune unrelated Docker objects.
    let state = c.snapshot();
    for (id, instance) in &state.games {
        if !instance.name.is_empty()
            && let Err(error) = engine.remove(id, instance, &state.owner).await
        {
            let path = dir.keep();
            anyhow::bail!(
                "cleanup failed: {error}; retained recovery state at {}",
                path.display()
            );
        }
    }
    journey
}
