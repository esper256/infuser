use anyhow::Result;
use async_trait::async_trait;
use infuser::{
    api,
    config::Config,
    controller::{Controller, now},
    docker::{Engine, Health},
    store::Instance,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tempfile::TempDir;

#[derive(Default)]
struct Runtime {
    names: Mutex<BTreeSet<String>>,
    starts: AtomicUsize,
    fail_remove: AtomicBool,
    fail_start: AtomicBool,
}
#[async_trait]
impl Engine for Runtime {
    async fn preflight(&self) -> Result<()> {
        Ok(())
    }
    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }
    async fn ensure(&self, _: &str, i: &Instance, _: &str) -> Result<Health> {
        if self.names.lock().unwrap().insert(i.name.clone()) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
        if self.fail_start.load(Ordering::SeqCst) {
            anyhow::bail!("lost response after container creation");
        }
        Ok(Health::Ready)
    }
    async fn health(&self, i: &Instance, _: &str, _: &str) -> Result<Health> {
        Ok(if self.names.lock().unwrap().contains(&i.name) {
            Health::Ready
        } else {
            Health::Missing
        })
    }
    async fn remove(&self, _: &str, i: &Instance, _: &str) -> Result<()> {
        if self.fail_remove.load(Ordering::SeqCst) {
            anyhow::bail!("Docker unavailable");
        }
        self.names.lock().unwrap().remove(&i.name);
        Ok(())
    }
}

fn config(dir: &TempDir) -> Config {
    let config: Config = toml::from_str(&format!(
        r#"
[controller]
state_dir = "{}"
memory_budget_mb = 256
cpu_budget_millis = 500
lease_seconds = 10
retry_seconds = 1
[games.moon]
image = "sha256:{}"
container_port = 7000
memory_mb = 256
cpu_millis = 500
idle_seconds = 1
[games.stars]
image = "sha256:{}"
container_port = 7000
memory_mb = 256
cpu_millis = 500
idle_seconds = 1
"#,
        dir.path().display(),
        "a".repeat(64),
        "b".repeat(64)
    ))
    .unwrap();
    config.validate().unwrap();
    config
}

const TOKEN: &str = "test-control-credential-not-for-deployment";

#[tokio::test]
async fn player_journey_over_http() -> Result<()> {
    let dir = TempDir::new()?;
    let cfg = config(&dir);
    let runtime = Arc::new(Runtime::default());
    let controller = Arc::new(Controller::open(cfg.clone(), runtime.clone())?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let app = api::router(controller.clone(), TOKEN);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::new();

    assert_eq!(
        http.get(format!("{url}/v1/apps")).send().await?.status(),
        401
    );
    let launch = || {
        http.post(format!("{url}/v1/apps/moon/leases"))
            .bearer_auth(TOKEN)
            .json(&json!({"session_id": "player-one"}))
    };
    let response = launch().send().await?;
    assert_eq!(response.status(), 202);
    let first: Value = response.json().await?;
    assert!(first["endpoint"].is_null());
    let duplicate: Value = launch().send().await?.json().await?;
    assert_eq!(first["lease_id"], duplicate["lease_id"]);
    let path = format!(
        "{url}/v1/apps/moon/leases/{}",
        first["lease_id"].as_str().unwrap()
    );
    let lease_token = first["lease_token"].as_str().unwrap();
    assert_eq!(
        http.put(&path).bearer_auth("wrong").send().await?.status(),
        401
    );
    // A runtime lease must not grant administrative access.
    assert_eq!(
        http.get(format!("{url}/v1/apps"))
            .bearer_auth(lease_token)
            .send()
            .await?
            .status(),
        401
    );

    let t = now();
    controller.reconcile(t).await?;
    let ready: Value = http
        .get(&path)
        .bearer_auth(lease_token)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["endpoint"]["port"], 8000);
    assert!(!ready.to_string().contains(lease_token));
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        http.post(format!("{url}/v1/apps/stars/leases"))
            .bearer_auth(TOKEN)
            .json(&json!({"session_id": "player-two"}))
            .send()
            .await?
            .status(),
        503
    );
    assert_eq!(
        http.put(&path)
            .bearer_auth(lease_token)
            .send()
            .await?
            .status(),
        200
    );

    // A controller restart preserves the lease and adopts the same container.
    server.abort();
    let _ = server.await;
    drop(controller);
    let controller = Controller::open(cfg, runtime.clone())?;
    controller.reconcile(t + 1).await?;
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        controller.lease(
            "moon",
            first["lease_id"].as_str().unwrap(),
            lease_token,
            "get",
            t + 1
        )?["state"],
        "ready"
    );

    // Advance the clock instead of slowing the test with idle sleeps.
    controller.reconcile(t + 30).await?;
    assert!(runtime.names.lock().unwrap().is_empty());
    assert_eq!(controller.list(t + 30)["reserved_memory_mb"], 0);
    assert!(
        controller
            .lease(
                "moon",
                first["lease_id"].as_str().unwrap(),
                lease_token,
                "renew",
                t + 30
            )
            .is_err()
    );
    let again = controller.acquire("moon", "player-one", t + 31)?;
    assert_ne!(again["lease_id"], first["lease_id"]);
    controller.reconcile(t + 31).await?;
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn uncertain_launch_retains_budget_until_removal_is_confirmed() -> Result<()> {
    let dir = TempDir::new()?;
    let runtime = Arc::new(Runtime::default());
    runtime.fail_start.store(true, Ordering::SeqCst);
    runtime.fail_remove.store(true, Ordering::SeqCst);
    let c = Controller::open(config(&dir), runtime.clone())?;
    c.acquire("moon", "player", 100)?;
    c.reconcile(100).await?;
    c.reconcile(101).await?;
    assert_eq!(c.list(101)["reserved_memory_mb"], 256);
    assert!(c.acquire("stars", "another", 101).is_err());
    runtime.fail_remove.store(false, Ordering::SeqCst);
    c.reconcile(102).await?;
    assert_eq!(c.list(102)["reserved_memory_mb"], 0);
    assert!(c.acquire("moon", "player", 102).is_err());
    runtime.fail_start.store(false, Ordering::SeqCst);
    c.acquire("moon", "player", 104)?;
    c.reconcile(104).await?;
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn simultaneous_launches_share_one_instance_and_reserve_once() -> Result<()> {
    let dir = TempDir::new()?;
    let runtime = Arc::new(Runtime::default());
    let c = Arc::new(Controller::open(config(&dir), runtime.clone())?);
    let mut tasks = vec![];
    for n in 0..16 {
        let c = c.clone();
        tasks.push(tokio::spawn(async move {
            c.acquire("moon", &format!("player-{n}"), 100)
        }));
    }
    for t in tasks {
        t.await??;
    }
    c.reconcile(100).await?;
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 1);
    assert_eq!(c.list(100)["apps"][0]["active_leases"], 16);
    assert_eq!(c.list(100)["reserved_memory_mb"], 256);
    Ok(())
}

#[tokio::test]
async fn publishing_registers_one_server_and_defers_updates_until_sleep() -> Result<()> {
    let dir = TempDir::new()?;
    let mut cfg = config(&dir);
    let game = cfg.games.remove("moon").unwrap();
    cfg.games.clear();
    let runtime = Arc::new(Runtime::default());
    let c = Controller::open(cfg.clone(), runtime.clone())?;
    c.register("moon", game.clone())?;
    let lease = c.acquire("moon", "player", 100)?;
    c.reconcile(100).await?;
    let mut newer = game.clone();
    newer.image = format!("sha256:{}", "c".repeat(64));
    assert!(
        c.register("moon", newer.clone()).is_err(),
        "replaced a live server"
    );
    c.register("moon", game)?;
    assert_eq!(runtime.starts.load(Ordering::SeqCst), 1);
    drop(c);
    let c = Controller::open(cfg, runtime.clone())?;
    assert_eq!(
        c.lease(
            "moon",
            lease["lease_id"].as_str().unwrap(),
            lease["lease_token"].as_str().unwrap(),
            "get",
            101
        )?["state"],
        "ready"
    );
    c.reconcile(120).await?;
    c.register("moon", newer.clone())?;
    assert_eq!(c.snapshot().games["moon"].spec, newer);
    Ok(())
}

#[tokio::test]
async fn published_server_updates_wait_survive_restart_and_activate_after_confirmed_stop()
-> Result<()> {
    use infuser::{platform, portal};
    let dir = TempDir::new()?;
    let mut cfg = config(&dir);
    let server_spec = cfg.games["moon"].clone();
    cfg.games.clear();
    cfg.controller.disk_reserve_mb = 64;
    let runtime = Arc::new(Runtime::default());
    let controller = Arc::new(Controller::open(cfg, runtime.clone())?);
    let p = portal::Platform::open(controller.clone())?;
    let (_, token) =
        p.db.edit(|d| platform::credential(d, "world", "publish", "test publisher"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let router = portal::router(p.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = reqwest::Client::new();
    let request = |method, path: &str| {
        http.request(method, format!("{url}/api/publisher/apps/world{path}"))
            .header("host", "games.example.com")
            .bearer_auth(&token)
    };
    for (build, image) in [
        ("b-one", 'a'),
        ("b-two", 'b'),
        ("b-three", 'c'),
        ("b-four", 'd'),
    ] {
        let mut spec = server_spec.clone();
        spec.image = format!("sha256:{}", image.to_string().repeat(64));
        let draft = json!({"manifest":{"schema_version":1,"app_id":"world","title":"World","play_modes":["solo","multiplayer"],"solo_runtime":"client_only","multiplayer":{"mode":"dedicated","transport":"tcp","max_players":8,"join_policy":"anytime"}},"server":spec,"spa":false,"cross_origin_isolation":false});
        assert_eq!(
            request(reqwest::Method::PUT, &format!("/builds/{build}"))
                .json(&draft)
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            request(
                reqwest::Method::PUT,
                &format!("/builds/{build}/files/index.html")
            )
            .body("<script type=module src=\"/_infuser/sdk.js\"></script>World")
            .send()
            .await?
            .status(),
            200
        );
    }
    assert_eq!(
        request(reqwest::Method::POST, "/builds/b-one/publish")
            .send()
            .await?
            .status(),
        200
    );
    let t = now();
    controller.acquire("world", "player", t)?;
    controller.reconcile(t).await?;
    for build in ["b-two", "b-three"] {
        let response = request(reqwest::Method::POST, &format!("/builds/{build}/publish"))
            .send()
            .await?;
        assert_eq!(response.status(), 202);
        assert_eq!(response.json::<Value>().await?["state"], "queued");
        assert_eq!(p.db.read()?.apps["world"].build, "b-one");
    }
    assert_eq!(
        request(reqwest::Method::DELETE, "/builds/b-three")
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        request(reqwest::Method::PUT, "/builds/b-three/files/index.html")
            .body("changed")
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        request(reqwest::Method::DELETE, "/builds/b-two")
            .send()
            .await?
            .status(),
        200,
        "a superseded queue entry should be removable"
    );
    let status: Value = request(reqwest::Method::GET, "/builds/b-three")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["publication_state"], "queued");
    let reopened = portal::Platform::open(controller.clone())?;
    reopened.activate_pending()?;
    assert_eq!(
        reopened.db.read()?.pending_promotions["world"].build,
        "b-three"
    );
    runtime.fail_remove.store(true, Ordering::SeqCst);
    controller.reconcile(t + 30).await?;
    reopened.activate_pending()?;
    assert_eq!(
        reopened.db.read()?.apps["world"].build,
        "b-one",
        "unconfirmed Docker removal must not activate a new client"
    );
    runtime.fail_remove.store(false, Ordering::SeqCst);
    controller.reconcile(t + 31).await?;
    reopened.activate_pending()?;
    assert_eq!(reopened.db.read()?.apps["world"].build, "b-three");
    assert!(reopened.db.read()?.pending_promotions.is_empty());
    assert_eq!(
        controller.snapshot().games["world"].spec.image,
        format!("sha256:{}", "c".repeat(64))
    );
    assert_eq!(
        runtime.starts.load(Ordering::SeqCst),
        1,
        "promotion should not wake an empty server"
    );
    controller.acquire("world", "player", t + 32)?;
    controller.reconcile(t + 32).await?;
    assert_eq!(
        request(reqwest::Method::POST, "/builds/b-four/publish")
            .send()
            .await?
            .status(),
        202
    );
    assert_eq!(
        request(reqwest::Method::DELETE, "").send().await?.status(),
        200
    );
    controller.reconcile(t + 33).await?;
    reopened.activate_pending()?;
    assert!(
        !reopened.db.read()?.apps.contains_key("world"),
        "unpublish must cancel the queued update"
    );
    assert!(reopened.db.read()?.pending_promotions.is_empty());
    task.abort();
    Ok(())
}
