use anyhow::{Result, ensure};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, peers, platform, portal};
use reqwest::Method;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        })),
    ))
}
async fn send(request: reqwest::RequestBuilder) -> Result<(u16, Value)> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    let body = response.json().await?;
    Ok((status, body))
}

#[tokio::test]
async fn hidden_delete_locked_rename_and_duplicate_title_warning() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=admin") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "admin@example.com"),
                        ("X-Auth-Request-User", "admin@example.com"),
                    ],
                ),
                Some("member=user") => (
                    StatusCode::ACCEPTED,
                    [
                        ("X-Auth-Request-Email", "user@example.com"),
                        ("X-Auth-Request-User", "user@example.com"),
                    ],
                ),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    [("X-Auth-Request-Email", ""), ("X-Auth-Request-User", "")],
                ),
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64\npublish_capture_mb=8",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    platform::member(&cfg, "user@example.com", "User", false, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let pond =
        p.db.edit(|d| Ok(platform::credential(d, "pond", "publish", "Pond")?.1))?;
    let kept =
        p.db.edit(|d| Ok(platform::credential(d, "kept", "publish", "Kept")?.1))?;
    let (url, _server) = serve(portal::router(p.clone())).await?;
    let http = reqwest::Client::new();
    let publish = |token: &str, app: &str, build: &str, title: &str, listed: bool, allow: bool| {
        let token = token.to_owned();
        let app = app.to_owned();
        let build = build.to_owned();
        let title = title.to_owned();
        let http = http.clone();
        let url = url.clone();
        async move {
            let call = |method: Method, path: String| {
                http.request(method, format!("{url}{path}"))
                    .header("host", "games.example.com")
                    .bearer_auth(&token)
            };
            let manifest = json!({"schema_version":1,"app_id":app,"title":title,"play_modes":["solo"],"solo_runtime":"client_only"});
            let base = format!("/api/publisher/apps/{app}/builds/{build}");
            let created = call(Method::PUT, base.clone())
                .json(&json!({"manifest": manifest}))
                .send()
                .await?;
            ensure!(
                created.status().is_success(),
                "create build {}",
                created.status()
            );
            let uploaded = call(Method::PUT, format!("{base}/files/index.html"))
                .body("<script type=module src='/_infuser/sdk.js'></script>")
                .send()
                .await?;
            ensure!(
                uploaded.status().is_success(),
                "upload {}",
                uploaded.status()
            );
            send(
                call(Method::POST, format!("{base}/publish"))
                    .json(&json!({"listed": listed, "allow_duplicate_title": allow})),
            )
            .await
        }
    };
    let admin = |method: Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", "member=admin")
            .header("origin", "https://games.example.com")
    };
    let user = |method: Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", "member=user")
            .header("origin", "https://games.example.com")
    };
    let metadata = |token: &str, app: &str| {
        http.get(format!("{url}/api/publisher/apps/{app}/metadata"))
            .header("host", "games.example.com")
            .bearer_auth(token)
    };
    let (status, body) = publish(&pond, "pond", "one", "Pond", true, false).await?;
    ensure!(status == 200, "first pond publish {status}: {body}");
    assert!(body.get("warnings").is_none());
    let (status, body) = publish(&kept, "kept", "one", "Kept", true, false).await?;
    ensure!(status == 200, "kept publish {status}: {body}");
    let slug = body["slug"].as_str().unwrap().to_owned();
    // An update may reuse another game's title. It is not a second copy.
    let (status, body) = publish(&pond, "pond", "two", "Kept", true, false).await?;
    ensure!(status == 200, "pond update {status}: {body}");
    assert!(body.get("warnings").is_none());
    let renamed_by_manifest = send(metadata(&pond, "pond")).await?.1;
    assert_eq!(renamed_by_manifest["title"], "Kept");
    assert_eq!(renamed_by_manifest["title_locked"], false);
    let (status, locked) =
        send(admin(Method::PUT, "/api/admin/apps/pond/title").json(&json!({"title": " Pond "})))
            .await?;
    ensure!(status == 200, "rename {status}: {locked}");
    assert_eq!(locked["title"], "Pond");
    assert_eq!(locked["title_locked"], true);
    assert_eq!(locked["slug"], renamed_by_manifest["slug"]);
    assert_eq!(locked["slug_unchanged"], true);
    let (status, _) =
        send(user(Method::PUT, "/api/admin/apps/pond/title").json(&json!({"title": "Nope"})))
            .await?;
    assert_eq!(status, 403);
    let (status, body) = publish(&pond, "pond", "three", "Pond Returns", true, false).await?;
    ensure!(status == 200, "republish {status}: {body}");
    let after = send(metadata(&pond, "pond")).await?.1;
    assert_eq!(after["title"], "Pond");
    assert_eq!(after["title_locked"], true);
    assert_eq!(after["slug"], locked["slug"]);
    assert_eq!(after["build"], "three");
    let patch = |body: Value| {
        http.patch(format!("{url}/api/publisher/apps/pond/metadata"))
            .header("host", "games.example.com")
            .bearer_auth(&pond)
            .json(&body)
    };
    let (status, rejected) = send(patch(json!({"title": "Pond Returns"}))).await?;
    assert_eq!(status, 409, "{rejected}");
    assert!(
        rejected["error"]
            .as_str()
            .unwrap()
            .contains("admin renamed")
    );
    let (status, same) = send(patch(json!({"title": "Pond"}))).await?;
    assert_eq!(status, 200, "{same}");
    assert_eq!(same["title"], "Pond");
    let (status, described) = send(patch(json!({"description": "A small pond."}))).await?;
    assert_eq!(status, 200, "{described}");
    assert_eq!(described["title"], "Pond");
    assert_eq!(described["description"], "A small pond.");
    let creek =
        p.db.edit(|d| Ok(platform::credential(d, "creek", "publish", "Creek")?.1))?;
    let (status, duplicate) = publish(&creek, "creek", "one", " pond ", true, false).await?;
    assert_eq!(status, 409, "{duplicate}");
    assert_eq!(duplicate["code"], "duplicate_title");
    assert_eq!(duplicate["matches"][0]["app_id"], "pond");
    assert_eq!(duplicate["matches"][0]["hidden"], false);
    assert_eq!(duplicate["matches"][0]["title"], "Pond");
    assert!(
        duplicate["error"]
            .as_str()
            .unwrap()
            .contains("allow_duplicate_title")
    );
    let reed =
        p.db.edit(|d| Ok(platform::credential(d, "reed", "publish", "Reed")?.1))?;
    let (status, spaced) = publish(&reed, "reed", "one", "Pond  Life", true, false).await?;
    ensure!(
        status == 200,
        "internal spaces are a different title {status}: {spaced}"
    );
    let (status, hidden_game) =
        send(admin(Method::PUT, "/api/admin/apps/pond/visibility").json(&json!({"listed": false})))
            .await?;
    ensure!(status == 200, "hide {status}: {hidden_game}");
    let (status, hidden_match) = publish(&creek, "creek", "two", "POND", true, false).await?;
    assert_eq!(status, 409, "{hidden_match}");
    assert_eq!(hidden_match["matches"][0]["hidden"], true);
    let (status, allowed) = send(
        http.post(format!("{url}/api/publisher/apps/creek/builds/two/publish"))
            .header("host", "games.example.com")
            .bearer_auth(&creek)
            .json(&json!({"listed": true, "allow_duplicate_title": true})),
    )
    .await?;
    ensure!(status == 200, "confirmed duplicate {status}: {allowed}");
    assert_eq!(allowed["warnings"][0]["code"], "duplicate_title");
    let (status, update) = publish(&creek, "creek", "three", "Pond", true, false).await?;
    ensure!(status == 200, "creek update {status}: {update}");
    assert!(update.get("warnings").is_none());
    let (status, visible) = send(
        admin(Method::POST, "/api/admin/apps/kept/deletion").json(&json!({"confirm": "Kept"})),
    )
    .await?;
    assert_eq!(status, 409, "{visible}");
    assert!(
        visible["error"]
            .as_str()
            .unwrap()
            .contains("Hide this game")
    );
    let (status, _) =
        send(user(Method::POST, "/api/admin/apps/pond/deletion").json(&json!({"confirm": "Pond"})))
            .await?;
    assert_eq!(status, 403);
    let db = rusqlite::Connection::open(temp.path().join("platform.sqlite"))?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute(
        "INSERT INTO saves(owner,app,slot,version,body) VALUES('admin@example.com','pond','slot',1,'{\"level\":1}')",
        [],
    )?;
    db.execute(
        "INSERT INTO saves(owner,app,slot,version,body) VALUES('admin@example.com','kept','slot',1,'{\"level\":2}')",
        [],
    )?;
    db.execute(
        "INSERT INTO screenshots(id,owner,app,created,width,height,image,thumbnail) VALUES('shot-pond','admin@example.com','pond',1,1,1,X'11',X'11')",
        [],
    )?;
    db.execute(
        "INSERT INTO screenshots(id,owner,app,created,width,height,image,thumbnail) VALUES('shot-kept','admin@example.com','kept',2,1,1,X'22',X'22')",
        [],
    )?;
    db.execute(
        "INSERT INTO cloud_revisions(owner,app,group_id,revision,body,bytes) VALUES('admin@example.com','pond','slot','r1','{}',12)",
        [],
    )?;
    db.execute(
        "INSERT INTO cloud_heads(owner,app,group_id,revision) VALUES('admin@example.com','pond','slot','r1')",
        [],
    )?;
    db.execute(
        "INSERT INTO cloud_revisions(owner,app,group_id,revision,body,bytes) VALUES('admin@example.com','kept','slot','r1','{}',5)",
        [],
    )?;
    db.execute(
        "INSERT INTO cloud_heads(owner,app,group_id,revision) VALUES('admin@example.com','kept','slot','r1')",
        [],
    )?;
    db.execute("UPDATE cloud_usage SET bytes=17 WHERE id=1", [])?;
    db.execute(
        "INSERT INTO library_chat(author,client_id,text,created,private_group,screenshot) VALUES('admin@example.com','note-1','hello pond',1,'home','shot-pond')",
        [],
    )?;
    drop(db);
    p.db.edit(|data| {
        let mut pond_notes = BTreeMap::new();
        pond_notes.insert(
            "admin@example.com".into(),
            platform::Reaction {
                emoji: "⭐".into(),
                text: "pond".into(),
                updated: 1,
                screenshot: Some("shot-pond".into()),
            },
        );
        data.reactions.insert("pond".into(), pond_notes);
        let mut kept_notes = BTreeMap::new();
        kept_notes.insert(
            "admin@example.com".into(),
            platform::Reaction {
                emoji: "⭐".into(),
                text: "kept".into(),
                updated: 1,
                screenshot: Some("shot-pond".into()),
            },
        );
        data.reactions.insert("kept".into(), kept_notes);
        Ok(())
    })?;
    std::fs::write(temp.path().join("content/pond/extra.txt"), b"pond-only")?;
    std::fs::write(temp.path().join("content/kept/extra.txt"), b"kept-only")?;
    let (status, preview) = send(admin(Method::GET, "/api/admin/apps/pond/deletion")).await?;
    ensure!(status == 200, "preview {status}: {preview}");
    assert_eq!(preview["blocked"], Value::Null);
    assert_eq!(preview["confirm"], "Pond");
    assert!(preview["removes"]["saves"].as_u64().unwrap() >= 1);
    assert!(preview["removes"]["screenshots"].as_u64().unwrap() >= 1);
    assert_eq!(preview["removes"]["leaderboards"], 0);
    assert!(preview["removes"]["publish_captures"].as_u64().unwrap() >= 1);
    assert!(preview["removes"]["web_files"].as_u64().unwrap() >= 1);
    assert!(
        preview["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("leaderboard"))
    );
    p.peer_rooms().join(
        peers::Identity {
            launch: "launch".into(),
            player: "player".into(),
            name: "Ada".into(),
            guest: false,
        },
        peers::Join {
            app: "pond",
            build: "three",
            connection: "connection-alpha-1",
            max_players: 4,
            create: true,
            expected_epoch: None,
            expected_player: None,
            joinable: Some(true),
        },
        infuser::controller::now(),
    )?;
    let (status, live) = send(
        admin(Method::POST, "/api/admin/apps/pond/deletion").json(&json!({"confirm": "Pond"})),
    )
    .await?;
    assert_eq!(status, 409, "{live}");
    assert!(live["error"].as_str().unwrap().contains("live room"));
    p.peer_rooms().end("pond");
    let (status, wrong) = send(
        admin(Method::POST, "/api/admin/apps/pond/deletion").json(&json!({"confirm": "nope"})),
    )
    .await?;
    assert_eq!(status, 400, "{wrong}");
    assert_eq!(send(metadata(&pond, "pond")).await?.0, 200);
    let (status, deleted) = send(
        admin(Method::POST, "/api/admin/apps/pond/deletion").json(&json!({"confirm": "Pond"})),
    )
    .await?;
    ensure!(status == 200, "delete {status}: {deleted}");
    assert_eq!(send(metadata(&pond, "pond")).await?.0, 401);
    assert_eq!(
        http.get(format!("{url}/api/publisher/capabilities"))
            .header("host", "games.example.com")
            .bearer_auth(&pond)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("{url}/api/publisher/capabilities"))
            .header("host", "games.example.com")
            .bearer_auth(&kept)
            .send()
            .await?
            .status(),
        200
    );
    let games = send(admin(Method::GET, "/api/admin/games")).await?.1;
    assert!(
        games["games"]
            .as_array()
            .unwrap()
            .iter()
            .all(|game| game["id"] != "pond")
    );
    assert!(
        games["games"]
            .as_array()
            .unwrap()
            .iter()
            .any(|game| game["id"] == "kept" && game["title"] == "Kept" && game["slug"] == slug)
    );
    assert!(!temp.path().join("content/pond").exists());
    assert_eq!(
        std::fs::read(temp.path().join("content/kept/extra.txt"))?,
        b"kept-only"
    );
    let db = rusqlite::Connection::open(temp.path().join("platform.sqlite"))?;
    db.busy_timeout(Duration::from_secs(5))?;
    let count = |sql: &str| db.query_row(sql, [], |row| row.get::<_, i64>(0));
    assert_eq!(count("SELECT COUNT(*) FROM saves WHERE app='pond'")?, 0);
    assert_eq!(count("SELECT COUNT(*) FROM saves WHERE app='kept'")?, 1);
    assert_eq!(
        count("SELECT COUNT(*) FROM screenshots WHERE app='pond'")?,
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM screenshots WHERE app='kept'")?,
        1
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM cloud_revisions WHERE app='pond'")?,
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM cloud_heads WHERE app='pond'")?,
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM cloud_revisions WHERE app='kept'")?,
        1
    );
    assert_eq!(count("SELECT bytes FROM cloud_usage WHERE id=1")?, 5);
    assert_eq!(
        count("SELECT COUNT(*) FROM game_metadata WHERE app='pond'")?,
        0
    );
    assert_eq!(count("SELECT COUNT(*) FROM game_urls WHERE app='pond'")?, 0);
    assert_eq!(count("SELECT COUNT(*) FROM game_urls WHERE app='kept'")?, 1);
    let note: String = db.query_row(
        "SELECT text FROM library_chat WHERE client_id='note-1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(note, "hello pond");
    assert_eq!(
        count("SELECT COUNT(*) FROM library_chat WHERE client_id='note-1' AND screenshot IS NULL")?,
        1
    );
    let catalog = p.db.catalog()?;
    assert!(!catalog.apps.contains_key("pond"));
    assert!(catalog.apps.contains_key("kept"));
    assert!(!catalog.reactions.contains_key("pond"));
    assert_eq!(catalog.reactions["kept"]["admin@example.com"].text, "kept");
    assert!(
        catalog.reactions["kept"]["admin@example.com"]
            .screenshot
            .is_none()
    );
    assert!(!catalog.credentials.values().any(|c| c.app == "pond"));
    assert!(catalog.credentials.values().any(|c| c.app == "kept"));
    let captures =
        rusqlite::Connection::open(temp.path().join("publish-captures/requests.sqlite"))?;
    captures.busy_timeout(Duration::from_secs(5))?;
    let captured = |app: &str| {
        captures.query_row(
            "SELECT COUNT(*) FROM requests WHERE json_extract(request,'$.app')=?1",
            [app],
            |row| row.get::<_, i64>(0),
        )
    };
    assert_eq!(captured("pond")?, 0);
    assert!(captured("kept")? > 0);
    let audit = std::fs::read_to_string(temp.path().join("admin-audit.jsonl"))?;
    assert!(audit.contains("admin@example.com"));
    assert!(audit.contains("\"app_id\":\"pond\""));
    assert!(audit.contains("delete_game"));
    assert!(audit.contains("\"title\":\"Pond\""));
    Ok(())
}
