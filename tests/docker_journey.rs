use anyhow::{Context, Result, ensure};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Docker, Engine},
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires Docker and INFUSER_TEST_IMAGE; uses disposable deployment volumes"]
async fn deployment_worlds_stay_separate_and_resume_after_stop() -> Result<()> {
    async fn docker(args: &[&str]) -> Result<String> {
        let out = tokio::process::Command::new("docker")
            .args(args)
            .output()
            .await?;
        ensure!(
            out.status.success(),
            "Docker fixture failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::from_utf8(out.stdout)?)
    }
    let image = std::env::var("INFUSER_TEST_IMAGE").context("set INFUSER_TEST_IMAGE")?;
    let dir = tempfile::tempdir()?;
    let config: Config = toml::from_str(&format!(
        r#"
[controller]
state_dir={:?}
publish_ip="127.0.0.1"
memory_budget_mb=512
cpu_budget_millis=1000
port_start=42000
port_end=42010
disk_reserve_mb=64
[games.home]
image={image:?}
container_port=7000
memory_mb=128
cpu_millis=250
persistent=true
[games.borrowed]
image={image:?}
container_port=7000
memory_mb=128
cpu_millis=250
persistent=true
"#,
        dir.path()
    ))?;
    std::fs::create_dir(dir.path().join("server-sources"))?;
    std::fs::write(dir.path().join("server-sources/borrowed"), "home")?;
    let engine = Arc::new(Docker {
        settings: config.controller.clone(),
    });
    let controller = Controller::open(config, engine.clone())?;
    engine.preflight().await?;
    let journey = async {
        for id in ["home", "borrowed"] {
            controller.acquire(id, id, now())?;
        }
        let wait = || async {
            tokio::time::timeout(Duration::from_secs(45), async {
                loop {
                    controller.reconcile(now()).await?;
                    if controller
                        .snapshot()
                        .games
                        .values()
                        .all(|i| i.phase == infuser::store::Phase::Ready)
                    {
                        return Ok::<_, anyhow::Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            })
            .await?
        };
        wait().await?;
        let state = controller.snapshot();
        for (id, value) in [("home", "99"), ("borrowed", "7")] {
            let name = &state.games[id].name;
            assert_eq!(
                docker(&["exec", name, "printenv", "INFUSER_GAME_ID"])
                    .await?
                    .trim(),
                "home"
            );
            assert_eq!(
                docker(&["exec", name, "printenv", "INFUSER_DEPLOYMENT_ID"])
                    .await?
                    .trim(),
                id
            );
            docker(&["exec", name, "sh", "-c", "test ! -e /data/world.save"]).await?;
            docker(&[
                "exec",
                name,
                "sh",
                "-c",
                "printf '%s' \"$1\" > /data/world.save",
                "fixture",
                value,
            ])
            .await?;
        }
        controller.stop("borrowed")?;
        controller.reconcile(now()).await?;
        assert_eq!(
            controller.snapshot().games["home"].phase,
            infuser::store::Phase::Ready
        );
        controller.acquire("borrowed", "returning-player", now())?;
        wait().await?;
        let state = controller.snapshot();
        for (id, value) in [("home", "99"), ("borrowed", "7")] {
            assert_eq!(
                docker(&["exec", &state.games[id].name, "cat", "/data/world.save"]).await?,
                value
            );
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let state = controller.snapshot();
    for (id, instance) in &state.games {
        if !instance.name.is_empty() {
            engine.remove(id, instance, &state.owner).await?;
        }
        docker(&[
            "volume",
            "rm",
            &format!("infuser-{}-{id}-data", state.owner),
        ])
        .await?;
    }
    journey
}

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
