//! Real offline dependency install, image assembly, readiness and two sockets.
//! Invoked explicitly in the existing manual Docker integration lane.
use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Docker, Engine},
    server_package::{
        Recipe,
        worker::{DockerWorker, Engine as PackageEngine},
    },
    store::secret,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;

async fn snapshot(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    players: usize,
) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let message = socket.next().await.context("socket ended")??;
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text)?;
                if value["type"] == "state"
                    && value["players"]
                        .as_array()
                        .is_some_and(|p| p.len() == players)
                {
                    return Ok(value);
                }
            }
        }
    })
    .await
    .context("shared-room snapshot timed out")?
}
fn source(path: &Path) -> Result<()> {
    std::fs::create_dir(path)?;
    let mut package: Value = serde_json::from_slice(include_bytes!(
        "../examples/dedicated-node/server/package.json"
    ))?;
    package["scripts"] = json!({"install":"node -e 'process.exit(99)'"});
    std::fs::write(path.join("package.json"), serde_json::to_vec(&package)?)?;
    std::fs::write(
        path.join("package-lock.json"),
        include_bytes!("../examples/dedicated-node/server/package-lock.json"),
    )?;
    std::fs::write(
        path.join("server.js"),
        format!(
            "console.log('diagnostics fixture started');\n{}",
            include_str!("../examples/dedicated-node/server/server.js")
        ),
    )?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires Docker access and INFUSER_NODE_TEST_IMAGE pinned to a staged Node 24 image"]
async fn offline_package_runs_two_players_sleeps_and_restarts_fresh() -> Result<()> {
    let image =
        std::env::var("INFUSER_NODE_TEST_IMAGE").context("stage a pinned Node 24 image first")?;
    let root = tempfile::tempdir()?;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = reservation.local_addr()?.port();
    let mut cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\nmemory_budget_mb=2048\ncpu_budget_millis=1000\nport_start={port}\nport_end={port}",
        root.path().to_str().unwrap()
    ))?;
    cfg.controller.node_packaging = Some(infuser::server_package::Settings {
        images: BTreeMap::from([("24".into(), image)]),
        image_budget_mb: 2048,
    });
    let docker = Arc::new(Docker {
        settings: cfg.controller.clone(),
    });
    let controller = Controller::open(cfg.clone(), docker.clone())?;
    let worker = DockerWorker {
        settings: cfg.controller.clone(),
    };
    let owner = controller.snapshot().owner;
    let key = format!("fixture/b-{}", &secret()[..12]);
    let recipe: Recipe = serde_json::from_value(
        json!({"runtime":"node","node_major":24,"entry":"server.js","memory_mb":128,"cpu_millis":250,"idle_seconds":1}),
    )?;
    let input = root.path().join("source");
    source(&input)?;
    let journey = async {
        let artifact = worker
            .build(
                &owner,
                &key,
                &recipe,
                &input,
                Arc::new(|phase, line| {
                    eprintln!("{phase:?}: {line}");
                    Ok(())
                }),
            )
            .await?;
        assert!(artifact.image.starts_with("sha256:"));
        controller.register("fixture", recipe.game(artifact.image.clone()))?;
        drop(reservation);
        let mut previous_boot = None;
        for _ in 0..2 {
            let lease = controller.acquire("fixture", "two-player-test", now())?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
            loop {
                controller.reconcile(now()).await?;
                let state = controller.lease(
                    "fixture",
                    lease["lease_id"].as_str().unwrap(),
                    lease["lease_token"].as_str().unwrap(),
                    "renew",
                    now(),
                )?;
                if state["state"] == "ready" {
                    break;
                }
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "packaged game did not wake"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let state = controller.snapshot();
            let address = docker
                .web_address("fixture", &state.games["fixture"], &owner)
                .await?;
            // Internal runtimes deliberately expose no unauthenticated host port.
            assert!(
                tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_err()
            );
            let health: Value = reqwest::get(format!("http://{address}/healthz"))
                .await?
                .error_for_status()?
                .json()
                .await?;
            let live = docker
                .diagnostics("fixture", &state.games["fixture"], &owner)
                .await?
                .context("live diagnostics missing")?;
            assert_eq!(live["health"], "healthy");
            assert_eq!(live["oom_killed"], false);
            assert!(
                live["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("diagnostics fixture started")
            );
            assert!(live.get("Config").is_none());
            if let Some(boot) = previous_boot {
                assert_ne!(health["boot"], boot);
            }
            previous_boot = Some(health["boot"].clone());
            let (mut first, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/")).await?;
            let (mut second, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/")).await?;
            for socket in [&mut first, &mut second] {
                socket
                    .send(Message::Text(
                        json!({"type":"join","room":"shared"}).to_string().into(),
                    ))
                    .await?;
            }
            let a = snapshot(&mut first, 2)
                .await
                .context("first player sees shared room")?;
            let b = snapshot(&mut second, 2)
                .await
                .context("second player sees shared room")?;
            assert_eq!(
                a["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p["id"].clone())
                    .collect::<Vec<_>>(),
                b["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p["id"].clone())
                    .collect::<Vec<_>>()
            );
            first.close(None).await?;
            // Closing queues a WebSocket close handshake. Dropping the stream
            // also ends TCP; an unpolled test client otherwise leaves ws waiting
            // for its 30-second close timeout instead of behaving like a browser.
            drop(first);
            snapshot(&mut second, 1)
                .await
                .context("remaining player sees departure")?;
            second.close(None).await?;
            drop(second);
            controller.lease(
                "fixture",
                lease["lease_id"].as_str().unwrap(),
                lease["lease_token"].as_str().unwrap(),
                "delete",
                now(),
            )?;
            controller.reconcile(now() + 2).await?;
            assert_eq!(controller.list(now())["reserved_memory_mb"], 0);
            let reports = infuser::docker::retained_diagnostics(&cfg.controller, "fixture")?;
            assert_eq!(reports[0]["attempt"], state.games["fixture"].attempt);
            assert_eq!(reports[0]["container_state"], "exited");
            assert!(
                reports[0]["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("diagnostics fixture started")
            );
        }
        // A readiness failure must remove the candidate image and all its workers.
        let bad = root.path().join("broken");
        source(&bad)?;
        std::fs::write(bad.join("server.js"), "process.exit(1)")?;
        let bad_key = format!("{key}-bad");
        assert!(
            worker
                .build(&owner, &bad_key, &recipe, &bad, Arc::new(|_, _| Ok(())))
                .await
                .is_err()
        );
        worker.cleanup(&owner, &bad_key, true).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let mut cleanup = Ok(());
    for (id, instance) in &controller.snapshot().games {
        if !instance.name.is_empty()
            && let Err(error) = docker.remove(id, instance, &owner).await
        {
            cleanup = Err(error);
        }
    }
    worker.cleanup(&owner, &key, true).await?;
    cleanup?;
    journey
}
