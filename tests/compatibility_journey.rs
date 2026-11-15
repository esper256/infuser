use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", l.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(l, router).await.unwrap()
        })),
    ))
}
const HOST: &str = "games.example.com";
async fn value(r: reqwest::RequestBuilder) -> Result<Value> {
    let r = r.send().await?;
    let status = r.status();
    let v = r.json().await?;
    anyhow::ensure!(status.is_success(), "{status}: {v}");
    Ok(v)
}
#[tokio::test]
async fn old_client_versioned_admission_and_durable_game_urls() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (auth_url, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=adult") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "adult@example.com")],
                ),
                Some("member=kid") => (
                    StatusCode::ACCEPTED,
                    [("X-Auth-Request-Email", "kid@example.com")],
                ),
                _ => (StatusCode::UNAUTHORIZED, [("X-Auth-Request-Email", "")]),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64",
        dir.path().display().to_string(),
        format!("{auth_url}/auth")
    ))?;
    platform::member(&cfg, "adult@example.com", "Adult", true, true)?;
    platform::member(&cfg, "kid@example.com", "Kid", false, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let db = p.db.clone();
    let token = db.edit(|d| Ok(platform::credential(d, "pond", "publish", "Pond")?.1))?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let member = |who: &str, method: Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("origin", format!("https://{HOST}"))
            .header("cookie", format!("member={who}"))
    };
    let publisher = |method: Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .bearer_auth(&token)
    };
    let manifest = json!({"schema_version":1,"app_id":"pond","title":"Moon Pond","play_modes":["solo"],"solo_runtime":"client_only"});
    let caps = value(publisher(Method::GET, "/api/publisher/capabilities")).await?;
    assert_eq!(caps["sdk"]["majors"], json!([1]));
    assert_eq!(caps["sdk"]["status"], "preview");
    for sdk in [
        json!({"major":9,"api_major":1}),
        json!({"major":1,"api_major":9}),
        json!({"major":1,"api_major":1,"required_capabilities":["unknown.v9"]}),
        json!({"major":1,"api_major":1,"required_capabilities":["peers.webrtc.v1"]}),
    ] {
        let mut m = manifest.clone();
        m["sdk"] = sdk;
        assert_eq!(
            publisher(Method::PUT, "/api/publisher/apps/pond/builds/rejected")
                .json(&json!({"manifest":m}))
                .send()
                .await?
                .status(),
            422
        );
    }
    assert!(!db.read()?.builds.contains_key("pond/rejected"));
    for (build, versioned) in [("old", false), ("new", true)] {
        let mut m = manifest.clone();
        if versioned {
            m["sdk"] =
                json!({"major":1,"api_major":1,"required_capabilities":["identity.v1","saves.v1"]});
        }
        value(
            publisher(
                Method::PUT,
                &format!("/api/publisher/apps/pond/builds/{build}"),
            )
            .json(&json!({"manifest":m})),
        )
        .await?;
        let script = if versioned {
            "/_infuser/sdk/v1.js"
        } else {
            "/_infuser/sdk.js"
        };
        value(
            publisher(
                Method::PUT,
                &format!("/api/publisher/apps/pond/builds/{build}/files/index.html"),
            )
            .body(format!(
                "<!doctype html><script type=module src='{script}'></script>"
            )),
        )
        .await?;
    }
    value(publisher(Method::POST, "/api/publisher/apps/pond/builds/old/publish").json(&json!({})))
        .await?;
    // The old JS bytes are intentionally frozen; execute them against this daemon.
    let output = tokio::process::Command::new(
        std::env::var("INFUSER_TEST_NODE").unwrap_or_else(|_| "node".into()),
    )
    .args(["tests/compat_client.mjs", &url])
    .output()
    .await?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for path in [
        "/_infuser/sdk/v1.js",
        "/_infuser/sdk/v1/peers.js",
        "/_infuser/sdk/v1/cloud.js",
    ] {
        let r = http
            .get(format!("{url}{path}"))
            .header("host", HOST)
            .send()
            .await?;
        assert_eq!(r.status(), 200);
        let source = r.text().await?;
        assert!(!source.contains("fetch('/_infuser/'"));
        assert!(!source.contains("import('/_infuser/peers.js')"));
    }
    for path in ["/_infuser/sdk/v2.js", "/_infuser/api/v2/context"] {
        assert_eq!(
            http.get(format!("{url}{path}"))
                .header("host", HOST)
                .send()
                .await?
                .status(),
            404
        );
    }
    let original = value(member("adult", Method::GET, "/api/games/moon-pond")).await?;
    assert_eq!(original["app_id"], "pond");
    assert_eq!(
        http.get(format!("{url}/api/games/moon-pond"))
            .header("host", HOST)
            .send()
            .await?
            .status(),
        401
    );
    // Two identical names reserve distinct stable URLs before either game is uploaded.
    let (a, b) = tokio::join!(
        value(member("kid", Method::POST, "/api/my-games").json(&json!({"title":"Moon Pond"}))),
        value(member("kid", Method::POST, "/api/my-games").json(&json!({"title":"Moon Pond"})))
    );
    let (a, b) = (a?, b?);
    let data = db.read()?;
    let mut slugs = vec![
        data.game_urls[a["app_id"].as_str().unwrap()].slug.clone(),
        data.game_urls[b["app_id"].as_str().unwrap()].slug.clone(),
    ];
    slugs.sort();
    assert_eq!(slugs, vec!["moon-pond-2", "moon-pond-3"]);
    for name in [
        "admin",
        "g-test",
        "two--dashes",
        "ends-",
        "CAPITAL",
        "123",
        "a-name-that-is-far-too-long",
    ] {
        assert_eq!(
            publisher(Method::PUT, "/api/publisher/apps/pond/slug")
                .json(&json!({"slug":name}))
                .send()
                .await?
                .status(),
            400
        );
    }
    assert_eq!(
        publisher(Method::PUT, "/api/publisher/apps/pond/slug")
            .json(&json!({"slug":"moon-pond-2"}))
            .send()
            .await?
            .status(),
        409
    );
    assert_eq!(
        member("kid", Method::PUT, "/api/apps/pond/slug")
            .json(&json!({"slug":"stolen"}))
            .send()
            .await?
            .status(),
        403
    );
    let before =
        value(member("adult", Method::POST, "/api/apps/pond/launch").json(&json!({}))).await?;
    assert!(
        before["url"]
            .as_str()
            .unwrap()
            .starts_with("https://moon-pond--g-")
    );
    let renamed = value(
        publisher(Method::PUT, "/api/publisher/apps/pond/slug").json(&json!({"slug":"night-pond"})),
    )
    .await?;
    assert_eq!(renamed["url"], "https://games.example.com/games/night-pond");
    assert_eq!(
        value(member("adult", Method::GET, "/api/games/moon-pond")).await?["slug"],
        "night-pond"
    );
    // A restart and a legacy catalog rewrite preserve SDK declarations and URL aliases.
    let reopened = platform::Database::open(dir.path())?;
    assert_eq!(reopened.read()?.game_urls["pond"].slug, "night-pond");
    assert!(reopened.read()?.builds["pond/new"].manifest.sdk.is_some());
    {
        let c = db.connection.lock().unwrap();
        let raw: String = c.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let legacy: Value = serde_json::from_str(&raw)?;
        assert!(
            legacy["builds"]["pond/new"]["manifest"]
                .get("sdk")
                .is_none()
        );
        assert!(legacy.get("game_urls").is_none());
        c.execute("UPDATE platform SET data=?1 WHERE id=1", [raw])?;
    }
    assert_eq!(
        platform::Database::open(dir.path())?.read()?.game_urls["pond"].aliases,
        vec!["moon-pond"]
    );
    value(
        publisher(Method::POST, "/api/publisher/apps/pond/builds/new/publish")
            .json(&json!({"listed":false})),
    )
    .await?;
    assert_eq!(
        member("kid", Method::GET, "/api/games/moon-pond")
            .send()
            .await?
            .status(),
        404
    );
    let new =
        value(member("adult", Method::POST, "/api/apps/pond/launch").json(&json!({}))).await?;
    let guest =
        value(member("adult", Method::POST, "/api/apps/pond/launch").json(&json!({"guest":true})))
            .await?;
    let guest2 =
        value(member("adult", Method::POST, "/api/apps/pond/launch").json(&json!({"guest":true})))
            .await?;
    assert!(
        new["url"]
            .as_str()
            .unwrap()
            .starts_with("https://night-pond--g-")
    );
    assert_ne!(
        reqwest::Url::parse(before["url"].as_str().unwrap())?.host_str(),
        reqwest::Url::parse(new["url"].as_str().unwrap())?.host_str()
    );
    assert_ne!(
        reqwest::Url::parse(guest["url"].as_str().unwrap())?.host_str(),
        reqwest::Url::parse(guest2["url"].as_str().unwrap())?.host_str()
    );
    // An old exchanged launch still works after the rename and new publication.
    let u = reqwest::Url::parse(before["url"].as_str().unwrap())?;
    let token = u.fragment().unwrap().strip_prefix("infuser=").unwrap();
    let old = |method: Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", u.host_str().unwrap())
            .header("origin", format!("https://{}", u.host_str().unwrap()))
            .bearer_auth(token)
    };
    value(old(Method::POST, "/_infuser/session").json(&json!({"token":token}))).await?;
    assert_eq!(old(Method::GET, "/").send().await?.status(), 200);
    assert_eq!(
        old(Method::POST, "/_infuser/api/v1/heartbeat")
            .header("origin", "https://other.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.get(format!("{url}/_infuser/api/v1/context"))
            .header("host", "another.games.example.com")
            .bearer_auth(token)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        old(Method::GET, "/_infuser/api/v9/context")
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        value(old(Method::GET, "/_infuser/context")).await?["build_id"],
        "old"
    );
    value(publisher(Method::DELETE, "/api/publisher/apps/pond")).await?;
    assert_eq!(
        member("adult", Method::GET, "/api/games/night-pond")
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        infuser::game_urls::resolve(&db.read()?, "moon-pond"),
        Some("pond")
    );
    Ok(())
}
