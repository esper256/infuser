//! Exercise the game-facing lifecycle through real HTTP, with an instant engine.
use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::{Engine, Health},
    platform, portal,
    store::Instance,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
struct Runtime {
    names: Mutex<BTreeSet<String>>,
    starts: AtomicUsize,
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
        self.names.lock().unwrap().remove(&i.name);
        Ok(())
    }
}
struct Running {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Running {
    async fn close(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    Ok((url, Running { stop, task }))
}
const HOST: &str = "games.example.com";
struct Player {
    host: String,
    token: String,
}
impl Player {
    fn request(
        &self,
        http: &reqwest::Client,
        url: &str,
        method: Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        http.request(method, format!("{url}{path}"))
            .header("host", &self.host)
            .header("origin", format!("https://{}", self.host))
            .bearer_auth(&self.token)
    }
}
async fn launch(http: &reqwest::Client, url: &str, app: &str, guest: bool) -> Result<Player> {
    let value: Value = http
        .post(format!("{url}/api/apps/{app}/launch"))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=yes")
        .json(&json!({"guest":guest}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(value["state"], "ready");
    let link = reqwest::Url::parse(value["url"].as_str().unwrap())?;
    let player = Player {
        host: link.host_str().unwrap().into(),
        token: link
            .fragment()
            .unwrap()
            .strip_prefix("infuser=")
            .unwrap()
            .into(),
    };
    player
        .request(http, url, Method::POST, "/_infuser/session")
        .json(&json!({"token":player.token}))
        .send()
        .await?
        .error_for_status()?;
    Ok(player)
}
async fn publish(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    app: &str,
    build: &str,
    on_demand: bool,
) -> Result<u16> {
    let mut manifest = json!({"schema_version":1,"app_id":app,"title":"Lifecycle test","play_modes":["solo","multiplayer"],"solo_runtime":"client_only","multiplayer":{"mode":"dedicated","transport":"websocket","max_players":8,"join_policy":"anytime"}});
    if on_demand {
        manifest["server_start"] = json!("on_demand");
    }
    let body = json!({"manifest":manifest,"server":{"image":format!("sha256:{}", "a".repeat(64)),"container_port":7000,"web":true,"memory_mb":128,"cpu_millis":250,"idle_seconds":1}});
    let path = format!("{url}/api/publisher/apps/{app}/builds/{build}");
    http.put(&path)
        .header("host", HOST)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    http.put(format!("{path}/files/index.html"))
        .header("host", HOST)
        .bearer_auth(token)
        .body("<!doctype html><script type=module src=\"/_infuser/sdk.js\"></script>")
        .send()
        .await?
        .error_for_status()?;
    Ok(http
        .post(format!("{path}/publish"))
        .header("host", HOST)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .status()
        .as_u16())
}
async fn value(r: reqwest::RequestBuilder) -> Result<Value> {
    Ok(r.send().await?.error_for_status()?.json().await?)
}

#[tokio::test]
async fn title_screen_shared_server_release_expiry_and_update() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (auth_url, auth) = serve(Router::new().route(
        "/oauth2/auth",
        get(|h: HeaderMap| async move {
            if h.get("cookie").is_some_and(|v| v == "member=yes") {
                (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "kid@example.com")],
                )
            } else {
                (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")])
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nmemory_budget_mb=128\ncpu_budget_millis=250\nlease_seconds=90\ndisk_reserve_mb=64",
        dir.path().display().to_string(),
        format!("{auth_url}/oauth2/auth")
    ))?;
    platform::member(&cfg, "kid@example.com", "Kid", false, true)?;
    let engine = Arc::new(Runtime::default());
    let controller = Arc::new(Controller::open(cfg.clone(), engine.clone())?);
    let p = portal::Platform::open(controller.clone())?;
    let db = p.db.clone();
    let token = db.edit(|d| Ok(platform::credential(d, "moon", "publish", "test")?.1))?;
    let legacy_token = db.edit(|d| Ok(platform::credential(d, "legacy", "publish", "test")?.1))?;
    let (url, server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    assert_eq!(
        publish(&http, &url, &token, "moon", "one", true).await?,
        200
    );
    assert_eq!(
        publish(&http, &url, &legacy_token, "legacy", "one", false).await?,
        200
    );
    let caps = value(
        http.get(format!("{url}/api/publisher/capabilities"))
            .header("host", HOST)
            .bearer_auth(&token),
    )
    .await?;
    assert_eq!(caps["runtime_sessions"]["version"], 1);
    let a = launch(&http, &url, "moon", false).await?;
    let b = launch(&http, &url, "moon", true).await?;
    let context = value(a.request(&http, &url, Method::GET, "/_infuser/context")).await?;
    assert_eq!(context["runtime"]["state"], "ready");
    assert_eq!(context["runtime"]["server"]["state"], "idle");
    assert!(context["runtime"]["endpoint"].is_null());
    assert_eq!(controller.list(now())["reserved_memory_mb"], 0);
    controller.reconcile(now()).await?;
    assert_eq!(
        engine.starts.load(Ordering::SeqCst),
        0,
        "title screens cannot start a server"
    );
    assert_eq!(
        a.request(&http, &url, Method::POST, "/_infuser/runtime/v1")
            .header("origin", "https://other.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.post(format!("{url}/_infuser/runtime/v1"))
            .header("host", &a.host)
            .header("origin", format!("https://{}", a.host))
            .send()
            .await?
            .status(),
        401
    );
    let first = value(a.request(&http, &url, Method::POST, "/_infuser/runtime/v1")).await?;
    assert_eq!(first["state"], "starting");
    assert!(first.get("lease_token").is_none());
    let duplicate = value(a.request(&http, &url, Method::POST, "/_infuser/runtime/v1")).await?;
    assert_eq!(
        first["session_id"], duplicate["session_id"],
        "lost acquisition response must not add a player"
    );
    let second = value(b.request(&http, &url, Method::POST, "/_infuser/runtime/v1")).await?;
    assert_ne!(first["session_id"], second["session_id"]);
    assert_eq!(controller.list(now())["reserved_memory_mb"], 128);
    assert_eq!(controller.snapshot().games["moon"].leases.len(), 2);
    controller.reconcile(now()).await?;
    assert_eq!(engine.starts.load(Ordering::SeqCst), 1);
    let ready = value(a.request(&http, &url, Method::GET, "/_infuser/runtime/v1")).await?;
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["endpoint"]["url"],
        format!("https://{}/_infuser/server", a.host)
    );
    let proxy = a.request(&http, &url, Method::GET, "/_infuser/proxy-auth")
        .header("X-Infuser-Upstream", "169.254.169.254:80")
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        proxy.headers()["X-Infuser-Upstream"],
        format!("127.0.0.1:{}", controller.snapshot().games["moon"].port)
    );
    // Reading context/heartbeating never hands out a backend endpoint for a title-only launch.
    let title = launch(&http, &url, "moon", true).await?;
    assert_eq!(
        value(title.request(&http, &url, Method::GET, "/_infuser/runtime/v1")).await?["state"],
        "idle"
    );
    assert_eq!(
        title
            .request(&http, &url, Method::GET, "/_infuser/proxy-auth")
            .send()
            .await?
            .status(),
        403
    );
    // Releasing someone else's ID does nothing; repeating one's own release is safe.
    let release_a = format!(
        "/_infuser/runtime/v1/{}",
        first["session_id"].as_str().unwrap()
    );
    assert_eq!(
        value(b.request(&http, &url, Method::DELETE, &release_a)).await?["session_id"],
        second["session_id"]
    );
    for _ in 0..2 {
        assert_eq!(
            value(a.request(&http, &url, Method::DELETE, &release_a)).await?["state"],
            "idle"
        );
    }
    assert_eq!(controller.snapshot().games["moon"].leases.len(), 1);
    assert_eq!(
        a.request(&http, &url, Method::GET, "/_infuser/proxy-auth")
            .send()
            .await?
            .status(),
        403
    );
    value(
        a.request(&http, &url, Method::PUT, "/_infuser/saves/progress")
            .json(&json!({"version":0,"data":{"stars":7}})),
    )
    .await?;
    let newer = value(a.request(&http, &url, Method::POST, "/_infuser/runtime/v1")).await?;
    assert_ne!(newer["session_id"], first["session_id"]);
    assert_eq!(
        value(a.request(&http, &url, Method::DELETE, &release_a)).await?["session_id"],
        newer["session_id"],
        "late release killed the new lease"
    );
    // Binary/catalog restart adopts the same shared process and per-player leases.
    server.close().await;
    drop(p);
    drop(controller);
    drop(db);
    let controller = Arc::new(Controller::open(cfg.clone(), engine.clone())?);
    let p = portal::Platform::open(controller.clone())?;
    let (url, server) = serve(portal::router(p.clone())).await?;
    controller.reconcile(now()).await?;
    assert_eq!(engine.starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        value(a.request(&http, &url, Method::GET, "/_infuser/runtime/v1")).await?["session_id"],
        newer["session_id"]
    );
    assert_eq!(
        publish(&http, &url, &token, "moon", "two", true).await?,
        202
    );
    p.activate_pending()?;
    assert_eq!(p.db.read()?.apps["moon"].build, "one");
    for (player, session) in [(&a, &newer), (&b, &second)] {
        value(player.request(
            &http,
            &url,
            Method::DELETE,
            &format!(
                "/_infuser/runtime/v1/{}",
                session["session_id"].as_str().unwrap()
            ),
        ))
        .await?;
    }
    p.activate_pending()?;
    assert_eq!(
        p.db.read()?.apps["moon"].build,
        "one",
        "update must wait for server shutdown"
    );
    controller.reconcile(now() + 2).await?;
    p.activate_pending()?;
    assert_eq!(p.db.read()?.apps["moon"].build, "two");
    assert_eq!(controller.list(now())["reserved_memory_mb"], 0);
    assert_eq!(
        a.request(&http, &url, Method::POST, "/_infuser/runtime/v1")
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        value(a.request(&http, &url, Method::POST, "/_infuser/heartbeat")).await?["state"],
        "ready"
    );
    assert_eq!(
        value(a.request(&http, &url, Method::GET, "/_infuser/saves/progress")).await?["data"]["stars"],
        7
    );
    let fresh = launch(&http, &url, "moon", true).await?;
    value(fresh.request(&http, &url, Method::POST, "/_infuser/runtime/v1")).await?;
    controller.reconcile(now()).await?;
    // Expiring server leases must leave guest saves and the title screen alive.
    controller.reconcile(now() + 92).await?;
    assert_eq!(
        value(fresh.request(&http, &url, Method::GET, "/_infuser/runtime/v1")).await?["state"],
        "ended"
    );
    assert_eq!(
        value(fresh.request(&http, &url, Method::POST, "/_infuser/heartbeat")).await?["state"],
        "ready"
    );
    assert_eq!(
        value(fresh.request(&http, &url, Method::GET, "/_infuser/context")).await?["runtime"]["server"]
            ["state"],
        "ended"
    );
    // Legacy clients still warm at launch, and an opt-in title screen opens even
    // while that legacy server consumes the entire budget.
    let legacy: Value = value(
        http.post(format!("{url}/api/apps/legacy/launch"))
            .header("host", HOST)
            .header("origin", format!("https://{HOST}"))
            .header("cookie", "member=yes")
            .json(&json!({})),
    )
    .await?;
    assert_eq!(legacy["state"], "starting");
    let blocked = launch(&http, &url, "moon", true).await?;
    assert_eq!(
        blocked
            .request(&http, &url, Method::POST, "/_infuser/runtime/v1")
            .send()
            .await?
            .status(),
        503
    );
    assert_eq!(
        value(blocked.request(&http, &url, Method::GET, "/_infuser/context")).await?["runtime"]["state"],
        "ready"
    );
    // The old catalog remains parseable; old-style writes cannot erase opt-in metadata.
    {
        let c = p.db.connection.lock().unwrap();
        let raw: String = c.query_row("SELECT data FROM platform", [], |r| r.get(0))?;
        let parsed: Value = serde_json::from_str(&raw)?;
        assert!(
            parsed["builds"]["moon/two"]["manifest"]
                .get("server_start")
                .is_none()
        );
        c.execute("UPDATE platform SET data=?1", [raw])?;
    }
    assert!(
        p.db.read()?.builds["moon/two"]
            .manifest
            .server_start
            .is_some()
    );
    platform::member(&cfg, "kid@example.com", "Kid", false, false)?;
    assert_eq!(
        blocked
            .request(&http, &url, Method::POST, "/_infuser/runtime/v1")
            .send()
            .await?
            .status(),
        403
    );
    server.close().await;
    auth.close().await;
    Ok(())
}
