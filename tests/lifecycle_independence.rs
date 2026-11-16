use anyhow::Result;
use async_trait::async_trait;
use infuser::{
    config::{Config, Game},
    controller::{Controller, now},
    docker::{Engine, Health},
    store::{Instance, Phase},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    sync::Semaphore,
    time::{Duration, timeout},
};
struct Runtime {
    stop: Semaphore,
    entered: Semaphore,
    lookups: AtomicUsize,
    start: Semaphore,
    starting: Semaphore,
}
#[async_trait]
impl Engine for Runtime {
    async fn preflight(&self) -> Result<()> {
        Ok(())
    }
    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }
    async fn ensure(&self, id: &str, _: &Instance, _: &str) -> Result<Health> {
        if id == "late" {
            self.starting.add_permits(1);
            self.start.acquire().await?.forget();
        }
        Ok(Health::Ready)
    }
    async fn health(&self, _: &Instance, _: &str, _: &str) -> Result<Health> {
        Ok(Health::Ready)
    }
    async fn remove(&self, id: &str, _: &Instance, _: &str) -> Result<()> {
        if id == "slow" {
            self.entered.add_permits(1);
            self.stop.acquire().await?.forget();
        }
        Ok(())
    }
    async fn web_address(&self, _: &str, _: &Instance, _: &str) -> Result<std::net::SocketAddr> {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        Ok("127.0.0.1:12345".parse()?)
    }
}
#[tokio::test]
async fn slow_stop_does_not_block_other_starts_and_retired_ports_are_reusable() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nport_start=8100\nport_end=8101",
        root.path().to_str().unwrap()
    ))?;
    let engine = Arc::new(Runtime {
        stop: Semaphore::new(0),
        entered: Semaphore::new(0),
        lookups: AtomicUsize::new(0),
        start: Semaphore::new(0),
        starting: Semaphore::new(0),
    });
    let controller = Controller::open(cfg, engine.clone())?;
    let game: Game = serde_json::from_value(
        serde_json::json!({"image":format!("sha256:{}","a".repeat(64)),"container_port":7000,"web":true}),
    )?;
    controller.register("slow", game.clone())?;
    controller.acquire("slow", "one", now())?;
    controller.reconcile(now()).await?;
    let attempt = controller.snapshot().games["slow"].attempt.clone();
    let calls = engine.lookups.load(Ordering::Relaxed);
    for _ in 0..100 {
        controller.web_address("slow", &attempt)?;
    }
    assert_eq!(calls, engine.lookups.load(Ordering::Relaxed));
    controller.stop("slow")?;
    assert!(controller.web_address("slow", &attempt).is_err());
    controller.reconcile_step(now()).await?;
    timeout(Duration::from_secs(1), engine.entered.acquire())
        .await??
        .forget();
    controller.register("fast", game.clone())?;
    controller.acquire("fast", "two", now())?;
    timeout(Duration::from_secs(2), async {
        loop {
            controller.reconcile_step(now()).await?;
            if controller.snapshot().games["fast"].phase == Phase::Ready {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    assert!(!controller.unregister("slow")?);
    engine.stop.add_permits(1);
    timeout(Duration::from_secs(2), async {
        loop {
            controller.reconcile_step(now()).await?;
            if controller.unregister("slow")? {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    for n in 0..10 {
        let id = format!("replacement-{n}");
        controller.register(&id, game.clone())?;
        assert!(controller.unregister(&id)?);
    }
    assert_eq!(controller.snapshot().games.len(), 1);
    controller.register("late", game)?;
    controller.acquire("late", "three", now())?;
    controller.reconcile_step(now()).await?;
    timeout(Duration::from_secs(1), engine.starting.acquire())
        .await??
        .forget();
    controller.stop("late")?;
    engine.start.add_permits(1);
    timeout(Duration::from_secs(2), async {
        loop {
            controller.reconcile_step(now()).await?;
            let state = controller.snapshot();
            assert_ne!(
                state.games["late"].phase,
                Phase::Ready,
                "late start completion undid stop"
            );
            if state.games["late"].phase == Phase::Sleeping {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    Ok(())
}
