use anyhow::Result;
use async_trait::async_trait;
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Engine, Health},
    platform, portal,
    server_package::{self, Phase},
    store::Instance,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};
#[path = "support/packager.rs"]
mod packaging;
struct Runtime;
#[async_trait]
impl Engine for Runtime {
    async fn web_address(&self, _: &str, _: &Instance, _: &str) -> Result<std::net::SocketAddr> {
        Ok("127.0.0.1:7000".parse()?)
    }
    async fn preflight(&self) -> Result<()> {
        Ok(())
    }
    async fn inventory(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }
    async fn ensure(&self, _: &str, _: &Instance, _: &str) -> Result<Health> {
        Ok(Health::Ready)
    }
    async fn health(&self, _: &Instance, _: &str, _: &str) -> Result<Health> {
        Ok(Health::Ready)
    }
    async fn remove(&self, _: &str, _: &Instance, _: &str) -> Result<()> {
        Ok(())
    }
}
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Api {
    http: reqwest::Client,
    url: String,
    token: String,
}
impl Api {
    fn call(&self, method: &str, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .header("host", "games.example.com")
            .bearer_auth(&self.token)
    }
    async fn create(&self, id: &str) -> Result<String> {
        let path = format!("/api/publisher/apps/pond/builds/{id}");
        let body = json!({"manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo","multiplayer"],"solo_runtime":"shared_server","server_start":"on_demand","multiplayer":{"mode":"dedicated","transport":"websocket","max_players":8,"join_policy":"anytime"}},"server_recipe":{"runtime":"node","node_major":24,"entry":"server.js","memory_mb":128,"cpu_millis":250,"idle_seconds":1}});
        self.call("PUT", &path)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        self.call("PUT", &format!("{path}/files/index.html"))
            .body("<script type=module src=\"/_infuser/sdk.js\"></script>")
            .send()
            .await?
            .error_for_status()?;
        for (name, bytes) in [
            ("server.js", "fixture source"),
            ("package.json", r#"{"name":"fixture","version":"1.0.0"}"#),
            (
                "package-lock.json",
                r#"{"lockfileVersion":3,"packages":{"":{"name":"fixture","version":"1.0.0"}}}"#,
            ),
        ] {
            self.call("PUT", &format!("{path}/server/files/{name}"))
                .body(bytes)
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(path)
    }
    async fn job(&self, path: &str) -> Result<Value> {
        Ok(self
            .call("GET", &format!("{path}/server-build"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
#[tokio::test]
async fn sealing_capacity_retries_recovery_and_pair_activation() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\nmemory_budget_mb=768\ncpu_budget_millis=500",
        root.path().to_str().unwrap()
    ))?;
    cfg.controller.node_packaging = Some(server_package::Settings {
        images: BTreeMap::from([("24".into(), format!("node@sha256:{}", "a".repeat(64)))]),
        image_budget_mb: 2048,
    });
    let controller = Arc::new(Controller::open(cfg.clone(), Arc::new(Runtime))?);
    let worker = Arc::new(packaging::Packager::default());
    let p = portal::Platform::open(controller.clone())?.with_packager(worker.clone());
    let token =
        p.db.edit(|d| Ok(platform::credential(d, "pond", "publish", "fixture")?.1))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api = Api {
        http: reqwest::Client::new(),
        url: format!("http://{}", listener.local_addr()?),
        token,
    };
    let router = portal::router(p.clone());
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap()
    }));
    let first = api.create("one").await?;
    // Hold two incomplete uploads on different apps. Admission must reject a
    // third mutation before reading its body, while status reads still work.
    let other_token =
        p.db.edit(|d| Ok(platform::credential(d, "other", "publish", "fixture")?.1))?;
    let mut uploads = Vec::new();
    for (app, token) in [("pond", &api.token), ("other", &other_token)] {
        use tokio::io::AsyncWriteExt;
        let mut socket =
            tokio::net::TcpStream::connect(api.url.trim_start_matches("http://")).await?;
        socket.write_all(format!("PUT /api/publisher/apps/{app}/builds/one/files/slow.bin HTTP/1.1\r\nHost: games.example.com\r\nAuthorization: Bearer {token}\r\nContent-Length: 1024\r\n\r\nx").as_bytes()).await?;
        uploads.push(socket);
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while std::fs::read_dir(root.path().join("upload-staging"))?.count() != 2 {
            tokio::task::yield_now().await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let busy = api
        .call("POST", &format!("{first}/server-build"))
        .send()
        .await?;
    assert_eq!(busy.status(), 503);
    assert_eq!(busy.headers()["retry-after"], "1");
    tokio::time::timeout(std::time::Duration::from_secs(1), api.job(&first)).await??;
    drop(uploads);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while std::fs::read_dir(root.path().join("upload-staging"))?.count() != 0 {
            tokio::task::yield_now().await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let diagnostics = api
        .call("GET", "/api/publisher/apps/pond/server-diagnostics")
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    assert_eq!(diagnostics["version"], 1);
    assert_eq!(diagnostics["recent"], json!([]));
    assert_eq!(
        api.call("GET", "/api/publisher/apps/another-game/server-diagnostics")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        api.http
            .get(format!(
                "{}/api/publisher/apps/pond/server-diagnostics",
                api.url
            ))
            .header("host", "games.example.com")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        api.call("POST", &format!("{first}/publish"))
            .send()
            .await?
            .status(),
        409
    );
    for _ in 0..2 {
        assert_eq!(
            api.call("POST", &format!("{first}/server-build"))
                .send()
                .await?
                .status(),
            202
        );
    }
    assert_eq!(
        api.call("PUT", &format!("{first}/server/files/server.js"))
            .body("replacement")
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        api.call("PUT", &format!("{first}/files/index.html"))
            .body("replacement")
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(api.call("DELETE", &first).send().await?.status(), 409);
    assert!(!root.path().join("content/pond/one/server.js").exists());
    assert!(
        api.call("GET", "/api/publisher/apps/other/builds/one/server-build")
            .send()
            .await?
            .status()
            .is_client_error()
    );
    p.packaging_step().await?;
    assert_eq!(worker.builds.load(Ordering::SeqCst), 1);
    assert_eq!(api.job(&first).await?["state"], "ready");
    assert_eq!(
        api.call("POST", &format!("{first}/server-build"))
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(
        api.call("POST", &format!("{first}/publish"))
            .send()
            .await?
            .status(),
        200
    );
    assert_eq!(p.db.read()?.apps["pond"].build, "one");
    let lease = controller.acquire("pond", "player", now())?;
    controller.reconcile(now()).await?;
    let second = api.create("two").await?;
    api.call("POST", &format!("{second}/server-build"))
        .send()
        .await?
        .error_for_status()?;
    p.packaging_step().await?;
    assert_eq!(
        api.job(&second).await?["state"],
        "queued",
        "active runtime consumes the available packaging capacity"
    );
    assert_eq!(worker.builds.load(Ordering::SeqCst), 1);
    controller.lease(
        "pond",
        lease["lease_id"].as_str().unwrap(),
        lease["lease_token"].as_str().unwrap(),
        "delete",
        now(),
    )?;
    controller.reconcile(now() + 2).await?;
    p.packaging_step().await?;
    assert_eq!(api.job(&second).await?["state"], "ready");
    let lease = controller.acquire("pond", "player", now())?;
    controller.reconcile(now()).await?;
    assert_eq!(
        api.call("POST", &format!("{second}/publish"))
            .send()
            .await?
            .status(),
        202
    );
    assert_eq!(p.db.read()?.apps["pond"].build, "one");
    controller.lease(
        "pond",
        lease["lease_id"].as_str().unwrap(),
        lease["lease_token"].as_str().unwrap(),
        "delete",
        now(),
    )?;
    controller.reconcile(now() + 2).await?;
    p.activate_pending()?;
    assert_eq!(p.db.read()?.apps["pond"].build, "two");
    assert_eq!(
        controller.snapshot().games["pond"].spec.image,
        p.db.read()?.server_builds["pond/two"]
            .artifact
            .as_ref()
            .unwrap()
            .image
    );
    let failed = api.create("failed").await?;
    worker.fail_next.store(true, Ordering::SeqCst);
    api.call("POST", &format!("{failed}/server-build"))
        .send()
        .await?
        .error_for_status()?;
    p.packaging_step().await?;
    assert_eq!(api.job(&failed).await?["state"], "failed");
    assert_eq!(
        api.call("POST", &format!("{failed}/publish"))
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(p.db.read()?.apps["pond"].build, "two");
    api.call("POST", &format!("{failed}/server-build"))
        .send()
        .await?
        .error_for_status()?;
    // Model a daemon stopping after it durably acquired worker capacity.
    assert!(controller.reserve_build("pond/failed", 768, 500)?);
    p.db.edit(|d| {
        d.server_builds.get_mut("pond/failed").unwrap().state = Phase::Packaging;
        Ok(())
    })?;
    let reopened = platform::Database::open(root.path())?;
    assert_eq!(
        reopened.read()?.server_builds["pond/failed"].state,
        Phase::Packaging
    );
    p.recover_packaging().await?;
    assert!(controller.snapshot().build_reservation.is_none());
    assert_eq!(api.job(&failed).await?["error"], "packaging_interrupted");
    api.call("POST", &format!("{failed}/server-build"))
        .send()
        .await?
        .error_for_status()?;
    p.packaging_step().await?;
    assert_eq!(api.job(&failed).await?["state"], "ready");
    api.call("DELETE", &first)
        .send()
        .await?
        .error_for_status()?;
    p.packaging_step().await?;
    assert!(!p.db.read()?.server_builds.contains_key("pond/one"));
    assert!(!root.path().join("server-sources/pond/one").exists());
    // Simulate controller registration being persisted before a catalog commit.
    let published = controller.snapshot().published_games["pond"].clone();
    let mut interrupted = published.clone();
    interrupted.image = format!("sha256:{}", "f".repeat(64));
    controller.register("pond", interrupted.clone())?;
    p.reconcile_publications()?;
    assert_eq!(controller.snapshot().published_games["pond"], published);
    controller.register("orphan", interrupted)?;
    p.reconcile_publications()?;
    assert!(!controller.snapshot().games.contains_key("orphan"));
    // A durable unpublish followed by a crash must retire the old registration
    // on reopen, without removing the retained world or build.
    let world = root.path().join("volumes/pond");
    std::fs::create_dir_all(&world)?;
    std::fs::write(world.join("world.save"), b"retained world")?;
    p.db.edit(|d| {
        d.apps.remove("pond");
        Ok(())
    })?;
    drop(_server);
    tokio::task::yield_now().await;
    drop(p);
    drop(controller);
    let controller = Arc::new(Controller::open(cfg, Arc::new(Runtime))?);
    let reopened = portal::Platform::open(controller.clone())?;
    assert!(!controller.snapshot().published_games.contains_key("pond"));
    assert!(!controller.snapshot().games.contains_key("pond"));
    assert!(reopened.db.read()?.builds.contains_key("pond/two"));
    assert_eq!(std::fs::read(world.join("world.save"))?, b"retained world");
    Ok(())
}

#[test]
fn source_and_dependency_boundaries_are_explicit() -> Result<()> {
    let package = include_bytes!("../examples/dedicated-node/server/package.json");
    let lock = include_bytes!("../examples/dedicated-node/server/package-lock.json");
    assert_eq!(server_package::dependencies(package, lock)?.len(), 2);
    for path in [
        "../secret",
        ".env",
        "a/.npmrc",
        "node_modules/ws/a.js",
        "Dockerfile",
        "keys.credentials.json",
        "/absolute",
        "a\\b",
    ] {
        assert!(!server_package::source_path(path), "{path}");
    }
    for url in [
        "http://registry.npmjs.org/ws.tgz",
        "https://127.0.0.1/ws.tgz",
        "https://registry.npmjs.org@evil.example/ws.tgz",
        "https://registry.npmjs.org/ws.tgz?token=secret",
    ] {
        let mut invalid: Value = serde_json::from_slice(lock)?;
        invalid["packages"]["node_modules/ws"]["resolved"] = json!(url);
        assert!(server_package::dependencies(package, &serde_json::to_vec(&invalid)?).is_err());
    }
    Ok(())
}
