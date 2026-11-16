use anyhow::Result;
use infuser::{
    config::Config, controller::Controller, docker::Docker, platform, portal, publish_capture,
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(config: &Config) -> Result<(Server, String)> {
    let controller = Arc::new(Controller::open(
        config.clone(),
        Arc::new(Docker {
            settings: config.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller)?;
    let (_, token) =
        p.db.edit(|data| platform::credential(data, "pond", "publish", "fixture"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let task = tokio::spawn(async move { axum::serve(listener, portal::router(p)).await.unwrap() });
    Ok((Server { url, task }, token))
}
fn config(root: &Path) -> Result<Config> {
    Ok(toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64\npublish_capture_mb=1",
        root.to_str().unwrap()
    ))?)
}
#[tokio::test]
async fn original_publish_update_and_rejected_requests_replay_with_new_credentials() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let cfg = config(&tmp.path().join("live"))?;
    let (server, token) = serve(&cfg).await?;
    let http = reqwest::Client::new();
    let requests=vec![
        ("PUT","/api/publisher/apps/pond/builds/one","application/json",br#"{ "manifest": {"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"}, "server":null }"#.to_vec()),
        ("PUT","/api/publisher/apps/pond/builds/one/files/index.html","application/octet-stream",br#"<!doctype html><title>Pond</title><script type="module" src="/_infuser/sdk.js"></script>"#.to_vec()),
        ("PUT","/api/publisher/apps/pond/builds/one/files/data.bin","application/octet-stream",vec![0,255,128,1]),
        ("POST","/api/publisher/apps/pond/builds/one/publish","application/json",br#"{"listed":false}"#.to_vec()),
        ("PATCH","/api/publisher/apps/pond/metadata","application/json",br#"{ "description": "An existing game", "art_prompt":"A pond" }"#.to_vec()),
        ("PUT","/api/publisher/apps/pond/builds/two","application/json",br#"{"manifest":{"schema_version":1,"app_id":"pond","title":"Pond update","play_modes":["solo"],"solo_runtime":"client_only"}}"#.to_vec()),
        ("PUT","/api/publisher/apps/pond/builds/two/files/index.html","text/html",br#"<!doctype html><title>Updated Pond</title><script type="module" src="/_infuser/sdk.js"></script>"#.to_vec()),
        ("POST","/api/publisher/apps/pond/builds/two/publish","application/json",b"{}".to_vec()),
        // Preserve the unsupported field and whitespace, not a parsed Manifest.
        ("PUT","/api/publisher/apps/pond/builds/rejected","application/json",br#"{ "manifest": {"future_sdk_field": "keep this"} }"#.to_vec()),
    ];
    let mut statuses = vec![];
    for (method, path, kind, bytes) in &requests {
        let response = http
            .request(
                method.parse()?,
                format!("{}{path}?ignored_secret=never-store", server.url),
            )
            .header("host", "games.example.com")
            .header("content-type", *kind)
            .header("cookie", "never-store-cookie")
            .bearer_auth(&token)
            .body(bytes.clone())
            .send()
            .await?;
        assert_eq!(response.headers()["x-infuser-capture"], "recorded");
        let status = response.status().as_u16();
        if statuses.len() < 8 {
            assert_eq!(status, 200, "{path}: {}", response.text().await?);
        }
        statuses.push(status);
    }
    assert_eq!(&statuses[..8], &[200; 8]);
    assert!(statuses[8] >= 400);
    let unauthorized = http
        .put(format!(
            "{}/api/publisher/apps/another/builds/test",
            server.url
        ))
        .header("host", "games.example.com")
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await?;
    assert!(unauthorized.headers().get("x-infuser-capture").is_none());
    http.post(format!("{}/api/publisher/claim", server.url))
        .header("host", "games.example.com")
        .json(&json!({"code":"never-store-setup-code"}))
        .send()
        .await?;
    let output = tmp.path().join("export");
    publish_capture::export(&cfg.controller, &output)?;
    assert!(publish_capture::export(&cfg.controller, &output).is_err());
    let text = std::fs::read_to_string(output.join("requests.jsonl"))?;
    assert!(
        !text.contains(&token) && !text.contains("never-store") && !text.contains("authorization")
    );
    let events: Vec<Value> = text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(events.len(), requests.len());
    let mut fresh = config(&tmp.path().join("replay"))?;
    fresh.controller.publish_capture_mb = 0;
    let (replay, new_token) = serve(&fresh).await?;
    assert_ne!(new_token, token);
    for (i, event) in events.iter().enumerate() {
        let body = std::fs::read(
            output
                .join("bodies")
                .join(event["body_sha256"].as_str().unwrap()),
        )?;
        assert_eq!(body, requests[i].3);
        let request = &event["request"];
        let mut call = http
            .request(
                request["method"].as_str().unwrap().parse()?,
                format!("{}{}", replay.url, request["path"].as_str().unwrap()),
            )
            .header("host", "games.example.com")
            .bearer_auth(&new_token)
            .body(body);
        for (name, value) in request["headers"].as_object().unwrap() {
            call = call.header(name, value.as_str().unwrap());
        }
        assert_eq!(
            call.send().await?.status().as_u16(),
            event["http_status"].as_u64().unwrap() as u16
        );
    }
    let state = http
        .get(format!("{}/api/publisher/apps/pond/builds/two", replay.url))
        .header("host", "games.example.com")
        .bearer_auth(&new_token)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(state["publication_state"], "published");
    assert!(!fresh.controller.state_dir.join("publish-captures").exists());
    for path in [
        output.clone(),
        cfg.controller.state_dir.join("publish-captures"),
    ] {
        assert_eq!(path.metadata()?.permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(
        output
            .join("requests.jsonl")
            .metadata()?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}
#[tokio::test]
async fn capture_storage_failure_does_not_block_publishing() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let cfg = config(tmp.path())?;
    let (server, token) = serve(&cfg).await?;
    std::fs::write(tmp.path().join("publish-captures"), b"unavailable archive")?;
    let response = reqwest::Client::new()
        .put(format!("{}/api/publisher/apps/pond/builds/one", server.url))
        .header("host", "games.example.com")
        .bearer_auth(token)
        .json(&json!({"manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"}}))
        .send().await?;
    assert_eq!(response.headers()["x-infuser-capture"], "incomplete");
    assert_eq!(
        response.status().as_u16(),
        200,
        "{}",
        response.text().await?
    );
    Ok(())
}
#[test]
fn bounded_deduplicated_archive_retains_unknown_outcomes_and_reports_gaps() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let cfg = config(tmp.path())?;
    let request = publish_capture::Request {
        method: "PUT".into(),
        path: "/artifact".into(),
        route: "fixture".into(),
        app: "pond".into(),
        group: "home".into(),
        headers: Default::default(),
    };
    let bytes = vec![7; 600_000];
    let first = publish_capture::begin(&cfg.controller, &request, &bytes)?.unwrap();
    publish_capture::finish(&cfg.controller, first, 200)?;
    assert!(
        publish_capture::begin(&cfg.controller, &request, &bytes)?.is_some(),
        "identical bodies share storage"
    );
    assert!(publish_capture::begin(&cfg.controller, &request, &vec![8; 600_000])?.is_none());
    let out = tmp.path().join("export");
    publish_capture::export(&cfg.controller, &out)?;
    let meta: Value = serde_json::from_slice(&std::fs::read(out.join("capture.json"))?)?;
    assert_eq!(meta["skipped_requests"], 1);
    assert_eq!(meta["requests"], 2);
    assert_eq!(std::fs::read_dir(out.join("bodies"))?.count(), 1);
    let lines = std::fs::read_to_string(out.join("requests.jsonl"))?;
    let last: Value = serde_json::from_str(lines.lines().last().unwrap())?;
    assert!(last["http_status"].is_null());
    Ok(())
}
