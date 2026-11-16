use anyhow::Result;
use infuser::{backup, config::Config, controller::Controller, docker::Docker, platform};
use serde_json::json;
use std::{collections::BTreeMap, fs, process::Command, sync::Arc};

#[tokio::test]
#[ignore = "requires INFUSER_TEST_RESTIC; uses a disposable local encrypted repository"]
async fn encrypted_replacement_restores_saves_and_only_retires_after_verification() -> Result<()> {
    let restic = std::env::var_os("INFUSER_TEST_RESTIC").expect("INFUSER_TEST_RESTIC required");
    let root = tempfile::tempdir()?;
    let staged = root.path().join("snapshot");
    let state = staged.join("state");
    fs::create_dir_all(&state)?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}",
        state.to_str().unwrap()
    ))?;
    let controller = Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?;
    let owner = controller.snapshot().owner;
    drop(controller);
    let db = platform::Database::open(&state)?;
    db.edit(|d| {
        d.members.insert("player@example.test".into(),serde_json::from_value(json!({"name":"Player","admin":true,"enabled":true}))?);
        d.builds.insert("pond/one".into(),serde_json::from_value(json!({"manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},"server":null,"spa":false,"cross_origin_isolation":false,"ready":true,"created":0,"files":{"index.html":4}}))?);
        d.apps.insert("pond".into(),platform::App{build:"one".into(),updated:0,listed:true});
        Ok(())
    })?;
    db.save_write(
        &platform::hash("player@example.test"),
        "pond",
        "progress",
        0,
        &json!({"level":7}),
    )?;
    drop(db);
    fs::create_dir_all(state.join("content/pond/one"))?;
    fs::write(state.join("content/pond/one/index.html"), b"game")?;
    fs::create_dir_all(staged.join("volumes/pond"))?;
    fs::write(staged.join("volumes/pond/world.save"), b"persistent world")?;
    let work = root.path().join("work");
    fs::create_dir(&work)?;
    let repository_file = root.path().join("repository");
    fs::write(
        &repository_file,
        root.path().join("encrypted").to_str().unwrap(),
    )?;
    let password_file = root.path().join("password");
    fs::write(&password_file, "disposable test repository password")?;
    let settings = backup::Settings {
        controller_config: root.path().join("config"),
        config_dir: root.path().join("config-dir"),
        work_dir: work,
        repository_file,
        password_file,
        rclone_config: None,
        restic: restic.into(),
        service: "infuser.service".into(),
        extra_paths: vec![],
    };
    settings.initialize()?;
    let volumes = BTreeMap::from([(
        "pond".into(),
        backup::Volume {
            path: "volumes/pond".into(),
            game: "pond".into(),
        },
    )]);
    backup::seal(&staged, &owner, volumes.clone(), vec![])?;
    let first = backup::publish(&settings, &staged)?;
    fs::write(staged.join("volumes/pond/world.save"), b"new world")?;
    assert!(
        backup::publish(&settings, &staged).is_err(),
        "unsealed files must not replace healthy backup"
    );
    backup::seal(&staged, &owner, volumes, vec![])?;
    let second = backup::publish(&settings, &staged)?;
    assert_ne!(first, second);
    let invoke = |args: &[&str]| -> Result<Vec<u8>> {
        let output = Command::new(&settings.restic)
            .arg("--repository-file")
            .arg(&settings.repository_file)
            .arg("--password-file")
            .arg(&settings.password_file)
            .env("RESTIC_CACHE_DIR", settings.work_dir.join("cache"))
            .args(args)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "restic test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output.stdout)
    };
    let snapshots: Vec<serde_json::Value> =
        serde_json::from_slice(&invoke(&["snapshots", "--json"])?)?;
    assert_eq!(
        snapshots.len(),
        1,
        "only the verified replacement should remain"
    );
    let restored = root.path().join("restored");
    invoke(&[
        "restore",
        &format!("{second}:{}", staged.display()),
        "--target",
        restored.to_str().unwrap(),
        "--verify",
    ])?;
    backup::verify(&restored)?;
    assert_eq!(
        fs::read(restored.join("volumes/pond/world.save"))?,
        b"new world"
    );
    let db = platform::Database::open(&restored.join("state"))?;
    assert_eq!(
        db.save_read(&platform::hash("player@example.test"), "pond", "progress")?
            .1,
        json!({"level":7})
    );
    assert_eq!(db.read()?.apps["pond"].build, "one");
    assert_eq!(
        fs::read(restored.join("state/content/pond/one/index.html"))?,
        b"game"
    );
    drop(db);
    let mut restored_config = cfg;
    restored_config.controller.state_dir = restored.join("state");
    let p = infuser::portal::Platform::open(Arc::new(Controller::open(
        restored_config.clone(),
        Arc::new(Docker {
            settings: restored_config.controller.clone(),
        }),
    )?))?;
    p.db.edit(|d|{
        d.launches.insert("restored-player".into(),serde_json::from_value(json!({
            "app":"pond","build":"one","email":"player@example.test","guest":false,
            "host":"pond--g-00000000000000000000000000000000.games.example.com","token_hash":platform::hash("restored-player"),"exchanged":true,
            "expires":infuser::controller::now()+300,"grant_expires":infuser::controller::now()+300,
            "seen":infuser::controller::now(),"lease_id":null,"lease_token":null
        }))?);Ok(())
    })?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, infuser::portal::router(p))
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let game = http
        .get(format!("{base}/index.html"))
        .header(
            "host",
            "pond--g-00000000000000000000000000000000.games.example.com",
        )
        .bearer_auth("restored-player")
        .send()
        .await?;
    assert_eq!(game.error_for_status()?.text().await?, "game");
    let save: serde_json::Value = http
        .get(format!("{base}/_infuser/saves/progress"))
        .header(
            "host",
            "pond--g-00000000000000000000000000000000.games.example.com",
        )
        .bearer_auth("restored-player")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(save["data"]["level"], 7);
    server.abort();
    Ok(())
}
