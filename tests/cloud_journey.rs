use anyhow::Result;
use infuser::{
    cloud::{Commit, Declaration, Group, Outcome, Scope, WriteOptions},
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, App, Build, Database, Launch},
    portal,
};
use serde_json::{Value, json};
use std::sync::Arc;
fn declaration() -> Declaration {
    serde_json::from_value(json!({"groups":[{"id":"campaign","label":"Adventure","files":[{"path":"progress.json","format":"json"},{"path":"inventory.json","format":"json"}]}]})).unwrap()
}
fn checkpoint(id: usize, base: Option<&str>, level: u64) -> Commit {
    serde_json::from_value(json!({"commit_id":format!("checkpoint-{id:016}"),"expected_revision":base,"files":{"progress.json":{"schema":1,"level":level},"inventory.json":{"items":[format!("item-{level}")]}},"summary":format!("Level {level}")})).unwrap()
}
fn write(db: &Database, scope: Scope<'_>, group: &Group, c: &Commit) -> Result<Outcome> {
    db.cloud_commit(
        scope,
        group,
        c,
        WriteOptions {
            build: "one",
            budget_bytes: 64 * 1024 * 1024,
            disk_reserve_bytes: 0,
            time: now(),
        },
    )
}
fn saved(result: Outcome) -> String {
    match result {
        Outcome::Saved { revision } => revision,
        other => panic!("expected saved, got {other:?}"),
    }
}
#[test]
fn retention_retries_quotas_and_concurrent_writers() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(dir.path())?;
    let group = declaration().groups.remove(0);
    let scope = Scope {
        owner: "player",
        app: "pond",
        group: "campaign",
    };
    let c = checkpoint(0, None, 1);
    assert!(
        db.cloud_commit(
            scope,
            &group,
            &c,
            WriteOptions {
                build: "one",
                budget_bytes: 1,
                disk_reserve_bytes: 0,
                time: 1
            }
        )
        .is_err()
    );
    assert_eq!(db.cloud_load(scope, None)?["revision"], Value::Null);
    let first = saved(write(&db, scope, &group, &c)?);
    let mut current = first.clone();
    let branch = checkpoint(999, None, 9);
    let conflict = write(&db, scope, &group, &branch)?;
    let candidate = match &conflict {
        Outcome::Conflict {
            candidate_revision, ..
        } => candidate_revision.clone(),
        _ => panic!(),
    };
    for id in 1..36 {
        current = saved(write(
            &db,
            scope,
            &group,
            &checkpoint(id, Some(&current), id as u64),
        )?);
    }
    assert_eq!(
        db.cloud_load(scope, Some(&first))?["files"]["progress.json"]["level"],
        1,
        "original head is pinned while the conflict remains open"
    );
    assert_eq!(
        write(&db, scope, &group, &c)?,
        Outcome::Saved {
            revision: first.clone()
        }
    );
    assert_eq!(write(&db, scope, &group, &branch)?, conflict);
    assert_eq!(
        db.cloud_load(scope, None)?["revision"],
        current,
        "old retry cannot move the head backwards"
    );
    let mut reused = c.clone();
    reused.summary = "Different".into();
    assert!(write(&db, scope, &group, &reused).is_err());
    let history = db.cloud_history(scope)?;
    assert!(history["revisions"].as_array().unwrap().len() <= infuser::cloud::HISTORY + 2);
    // A stale resolution becomes another preserved candidate, leaving both originals.
    let mut resolution = checkpoint(500, Some(&first), 10);
    resolution.resolve_candidate = Some(candidate.clone());
    let stale = write(&db, scope, &group, &resolution)?;
    let pending = match stale {
        Outcome::Conflict {
            candidate_revision, ..
        } => candidate_revision,
        _ => panic!(),
    };
    let mut resolution = checkpoint(501, Some(&current), 11);
    resolution.resolve_candidate = Some(pending.clone());
    let final_revision = saved(write(&db, scope, &group, &resolution)?);
    assert_eq!(db.cloud_load(scope, None)?["conflicts"], json!([]));
    assert_eq!(
        db.cloud_load(scope, Some(&final_revision))?["parents"],
        json!([current, pending])
    );
    // Separate connections race the same idempotent write; exactly one revision survives.
    let other = Arc::new(Database::open(dir.path())?);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut request = checkpoint(700, Some(&final_revision), 12);
    request.files.remove("inventory.json");
    let db = Arc::new(db);
    let handles = [db.clone(), other]
        .into_iter()
        .map(|db| {
            let barrier = barrier.clone();
            let group = group.clone();
            let request = request.clone();
            std::thread::spawn(move || {
                barrier.wait();
                write(&db, scope, &group, &request)
            })
        })
        .collect::<Vec<_>>();
    let mut outcomes = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(outcomes[0], outcomes[1]);
    let last = saved(outcomes.remove(0));
    assert_eq!(
        db.cloud_load(scope, Some(&candidate))?["files"]["progress.json"]["level"],
        9,
        "an older conflict alternative remains readable after resolution and the next save"
    );
    assert_eq!(
        db.cloud_load(scope, Some(&last))?["files"]
            .as_object()
            .unwrap()
            .len(),
        1,
        "omission deliberately removes a file in a complete checkpoint"
    );
    let mut old_group = group.clone();
    old_group.files.retain(|f| f.path != "inventory.json");
    assert!(
        write(
            &db,
            scope,
            &old_group,
            &checkpoint(900, Some(&final_revision), 1)
        )
        .is_err()
    );
    // Fill the conflict limit; failed writes leave neither receipts nor partial snapshots.
    let mut accepted = 0;
    for id in 1000..1010 {
        if write(&db, scope, &group, &checkpoint(id, Some(&last), 1)).is_ok() {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 1 + infuser::cloud::CONFLICTS);
    assert_eq!(
        db.cloud_load(scope, None)?["conflicts"]
            .as_array()
            .unwrap()
            .len(),
        infuser::cloud::CONFLICTS
    );
    let total: u64 = db.connection.lock().unwrap().query_row(
        "SELECT bytes FROM cloud_usage WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    let measured: u64 = db.connection.lock().unwrap().query_row(
        "SELECT (SELECT SUM(bytes) FROM cloud_revisions)+(SELECT SUM(bytes) FROM cloud_receipts)",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(total, measured);
    Ok(())
}
struct Server {
    url: String,
    db: Arc<Database>,
    task: tokio::task::JoinHandle<()>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(signal) = self.shutdown.take() {
            let _ = signal.send(());
        }
        self.task.abort();
    }
}
async fn serve(cfg: &Config) -> Result<Server> {
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let p = portal::Platform::open(controller)?;
    let db = p.db.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, portal::router(p))
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap()
    });
    Ok(Server {
        url,
        db,
        task,
        shutdown: Some(shutdown),
    })
}
#[tokio::test]
async fn two_devices_conflict_restart_resolve_and_guest_cleanup() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        dir.path().display().to_string()
    ))?;
    platform::member(&cfg, "admin@example.com", "Player", true, true)?;
    let mut server = serve(&cfg).await?;
    server.db.edit(|d|{
        for app in ["pond","other"] {
            let manifest=serde_json::from_value(json!({"schema_version":1,"app_id":app,"title":"Pond","play_modes":["solo"],"solo_runtime":"client_only","cloud_saves":declaration()}))?;
            d.builds.insert(format!("{app}/one"),Build{manifest,server:None,spa:false,cross_origin_isolation:false,ready:true,created:now(),files:Default::default()});
            d.apps.insert(app.into(),App {publication: None,build:"one".into(),listed:false,updated:now()});
        }
        for id in ["a","b","guest","fresh","other"] {d.launches.insert(id.into(),Launch{deployment:None,visit:None,intent:None,app:if id=="other"{"other"}else{"pond"}.into(),build:"one".into(),email:"admin@example.com".into(),guest:matches!(id,"guest"|"fresh"),host:format!("{id}.games.example.com"),token_hash:platform::hash(id),exchanged:true,expires:now()+300,ended:false, library_session: String::new(),grant_expires:now()+300,seen:now(),lease_id:None,lease_token:None});}
        Ok(())
    })?;
    let http = reqwest::Client::new();
    let call = |url: &str, id: &str, method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}/_infuser/{path}"))
            .header("host", format!("{id}.games.example.com"))
            .header("origin", format!("https://{id}.games.example.com"))
            .bearer_auth(id)
    };
    let load =
        |url: &str, id: &str| call(url, id, reqwest::Method::GET, "cloud/v1/groups/campaign");
    let commit =
        |url: &str, id: &str| call(url, id, reqwest::Method::POST, "cloud/v1/groups/campaign");
    for id in ["a", "b"] {
        let value: Value = load(&server.url, id)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(value["revision"].is_null());
    }
    // Exercise binary bytes through the real API, restart, conflicts and guest scope.
    server.db.edit(|data| {
        for build in data.builds.values_mut() {
            build.manifest.cloud_saves.as_mut().unwrap().groups[0]
                .files
                .push(infuser::cloud::File {
                    path: "world.bin".into(),
                    format: "binary".into(),
                });
        }
        Ok(())
    })?;
    let mut first = checkpoint(1, None, 2);
    first
        .files
        .insert("world.bin".into(), json!({"base64":"AP8BgA=="}));
    let result: Value = commit(&server.url, "a")
        .json(&first)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let revision = result["revision"].as_str().unwrap().to_owned();
    let mut incoming = checkpoint(2, None, 3);
    incoming
        .files
        .insert("world.bin".into(), json!({"base64":"AQID"}));
    let response = commit(&server.url, "b").json(&incoming).send().await?;
    assert_eq!(response.status(), 409);
    let conflict: Value = response.json().await?;
    let candidate = conflict["candidate_revision"].as_str().unwrap().to_owned();
    assert_eq!(conflict["current_revision"], revision);
    let current: Value = load(&server.url, "b").send().await?.json().await?;
    assert_eq!(current["files"]["progress.json"]["level"], 2);
    assert_eq!(
        current["files"]["inventory.json"]["items"],
        json!(["item-2"])
    );
    // Catalog remains readable by old binaries and their rewrites preserve declarations.
    let raw: String =
        server
            .db
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT data FROM platform", [], |r| r.get(0))?;
    assert!(
        serde_json::from_str::<Value>(&raw)?["builds"]["pond/one"]["manifest"]
            .get("cloud_saves")
            .is_none()
    );
    server
        .db
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE platform SET data=?1", [raw])?;
    let _ = server.shutdown.take().unwrap().send(());
    (&mut server.task).await?;
    drop(server);
    let server = serve(&cfg).await?;
    assert!(
        server.db.read()?.builds["pond/one"]
            .manifest
            .cloud_saves
            .is_some()
    );
    assert_eq!(
        commit(&server.url, "b")
            .json(&incoming)
            .send()
            .await?
            .json::<Value>()
            .await?,
        conflict,
        "lost conflict response replays after restart"
    );
    let preserved: Value = call(
        &server.url,
        "b",
        reqwest::Method::GET,
        &format!("cloud/v1/groups/campaign/revisions/{candidate}"),
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert_eq!(preserved["files"]["progress.json"]["level"], 3);
    assert_eq!(preserved["files"]["world.bin"], json!({"base64":"AQID"}));
    assert_eq!(preserved["formats"]["world.bin"], "binary");
    let mut resolution = checkpoint(3, Some(&revision), 3);
    resolution
        .files
        .insert("world.bin".into(), preserved["files"]["world.bin"].clone());
    resolution.resolve_candidate = Some(candidate.clone());
    let result: Value = commit(&server.url, "b")
        .json(&resolution)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        commit(&server.url, "b")
            .json(&resolution)
            .send()
            .await?
            .json::<Value>()
            .await?,
        result
    );
    let current: Value = load(&server.url, "a").send().await?.json().await?;
    assert_eq!(current["parents"], json!([revision, candidate]));
    assert_eq!(current["conflicts"], json!([]));
    // A newer game schema is returned unchanged; an older reader can stop safely.
    let mut newer = checkpoint(4, current["revision"].as_str(), 4);
    newer.files.get_mut("progress.json").unwrap()["schema"] = json!(2);
    commit(&server.url, "a")
        .json(&newer)
        .send()
        .await?
        .error_for_status()?;
    let old_reader: Value = load(&server.url, "b").send().await?.json().await?;
    assert_eq!(old_reader["files"]["progress.json"]["schema"], 2);
    assert_ne!(old_reader["revision"], current["revision"]);
    for id in ["guest", "fresh", "other"] {
        let empty: Value = load(&server.url, id)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(empty["revision"].is_null());
        assert_eq!(
            call(
                &server.url,
                id,
                reqwest::Method::GET,
                &format!("cloud/v1/groups/campaign/revisions/{revision}")
            )
            .send()
            .await?
            .status(),
            404
        );
    }
    let guest: Value = commit(&server.url, "guest")
        .json(&first)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        load(&server.url, "guest")
            .send()
            .await?
            .json::<Value>()
            .await?["revision"],
        guest["revision"],
        "refreshing a guest launch preserves its namespace"
    );
    assert!(
        load(&server.url, "fresh")
            .send()
            .await?
            .json::<Value>()
            .await?["revision"]
            .is_null()
    );
    let mut bad = checkpoint(20, None, 1);
    bad.files.insert("../escape.json".into(), json!({}));
    assert_eq!(
        commit(&server.url, "a").json(&bad).send().await?.status(),
        400
    );
    let missing = json!({"commit_id":"missing-base-123456","files":{}});
    assert_eq!(
        commit(&server.url, "a")
            .json(&missing)
            .send()
            .await?
            .status(),
        422
    );
    assert_eq!(
        commit(&server.url, "a")
            .header("origin", "https://other.games.example.com")
            .json(&first)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(load(&server.url, "unknown").send().await?.status(), 401);
    // Frozen old slot API remains independent and usable.
    call(&server.url, "a", reqwest::Method::PUT, "saves/legacy")
        .json(&json!({"version":0,"data":{"level":7}}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        call(&server.url, "a", reqwest::Method::GET, "saves/legacy")
            .send()
            .await?
            .json::<Value>()
            .await?["data"]["level"],
        7
    );
    server.db.edit(|d| {
        let guest = d.launches.get_mut("guest").unwrap();
        guest.expires = now();
        guest.seen = now() - platform::STALE;
        Ok(())
    })?;
    server.db.cleanup()?;
    assert!(
        server.db.cloud_load(
            Scope {
                owner: "guest-guest",
                app: "pond",
                group: "campaign"
            },
            None
        )?["revision"]
            .is_null()
    );
    assert_eq!(
        load(&server.url, "a").send().await?.json::<Value>().await?["revision"],
        old_reader["revision"]
    );
    Ok(())
}

#[test]
fn binary_validation_size_and_old_build_protection() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(dir.path())?;
    let mut declaration = declaration();
    declaration.groups[0].files.push(infuser::cloud::File {
        path: "world.bin".into(),
        format: "binary".into(),
    });
    declaration.validate()?;
    let group = &declaration.groups[0];
    let scope = Scope {
        owner: "player",
        app: "game",
        group: "campaign",
    };
    let mut request = checkpoint(1, None, 1);
    for value in [
        json!("AP8="),
        json!({"base64":"AA"}),
        json!({"base64":"AB=="}),
        json!({"base64":"@@=="}),
        json!({"base64":"AA==","extra":true}),
    ] {
        request.files.insert("world.bin".into(), value);
        assert!(write(&db, scope, group, &request).is_err());
        assert!(db.cloud_load(scope, None)?["revision"].is_null());
    }
    request
        .files
        .insert("world.bin".into(), json!({"base64":"AP8BgA=="}));
    // A binary-shaped JSON object stays ordinary JSON.
    request
        .files
        .insert("progress.json".into(), json!({"base64":"not base64"}));
    let revision = saved(write(&db, scope, group, &request)?);
    let snapshot = db.cloud_load(scope, None)?;
    assert_eq!(snapshot["files"], serde_json::to_value(&request.files)?);
    assert_eq!(snapshot["formats"], json!({"world.bin":"binary"}));
    let mut older = group.clone();
    older.files.last_mut().unwrap().format = "json".into();
    request.commit_id = "old-build-write-0001".into();
    request.expected_revision = Some(revision.clone());
    assert!(write(&db, scope, &older, &request).is_err());
    assert_eq!(db.cloud_load(scope, None)?["revision"], revision);
    request
        .files
        .insert("world.bin".into(), json!({"base64":"AAAA".repeat(262144)}));
    assert!(write(&db, scope, group, &request).is_err());
    request
        .files
        .insert("world.bin".into(), json!({"base64":""}));
    saved(write(&db, scope, group, &request)?);
    assert_eq!(
        db.cloud_load(scope, None)?["files"]["world.bin"],
        json!({"base64":""})
    );
    Ok(())
}
