use anyhow::Result;
use infuser::platform::{self, Database};
use serde_json::json;

#[test]
fn activity_is_small_and_genesis_writers_remain_authoritative() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(dir.path())?;
    let time = infuser::controller::now();
    db.edit(|d| {
        d.members.insert("player@example.test".into(),serde_json::from_value(json!({"name":"Player","admin":false,"enabled":true}))?);
        d.launches.insert("test-launch".into(),serde_json::from_value(json!({"app":"pond","build":"one","email":"player@example.test","guest":true,"host":"pond.games.example.com","token_hash":"fixture","exchanged":true,"expires":time+10,"grant_expires":time+10,"seen":time,"lease_id":null,"lease_token":null}))?);
        Ok(())
    })?;
    // Warm the snapshot, then verify activity never changes its revision or bytes.
    db.read()?;
    let before: (String, u64) = db.connection.lock().unwrap().query_row(
        "SELECT data,revision FROM platform_catalog,catalog_revision",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    db.see_member("player@example.test", time + 1)?;
    db.renew_launch("test-launch", time + 1, None)?;
    db.insert_launch(
        "lifecycle-launch",
        &serde_json::from_value(json!({
            "app":"pond","build":"one","email":"player@example.test","guest":false,
            "host":"pond.games.example.com","token_hash":"lifecycle-token",
            "exchanged":false,"expires":time+60,"grant_expires":time+30,"seen":time,
            "intent":{"kind":"join_friend","session_id":"room-1","player_id":"friend-1"},
            "lease_id":null,"lease_token":null
        }))?,
    )?;
    db.extend_launch_grant("lifecycle-launch", time + 120)?;
    db.exchange_launch("lifecycle-token", "pond.games.example.com", time + 1)?;
    assert_eq!(
        db.find_launch("lifecycle-token", "pond.games.example.com", time + 1)?
            .0,
        "lifecycle-launch"
    );
    assert!(
        db.take_launch_intent("lifecycle-launch", time + 1)?
            .is_some()
    );
    assert!(
        db.take_launch_intent("lifecycle-launch", time + 1)?
            .is_none()
    );
    db.expire_launch("lifecycle-launch", time + 1)?;
    db.native_edit(|_, n| {
        n.devices.insert(
            "dev-1".into(),
            infuser::native::Device {
                email: "player@example.test".into(),
                created: time,
            },
        );
        Ok(())
    })?;
    assert_eq!(
        db.native_devices("player@example.test")?["devices"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(std::sync::Arc::ptr_eq(&db.catalog()?, &db.catalog()?));
    let after: (String, u64) = db.connection.lock().unwrap().query_row(
        "SELECT data,revision FROM platform_catalog,catalog_revision",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(before, after, "activity rewrote the durable catalog");
    let other = rusqlite::Connection::open(dir.path().join("platform.sqlite"))?;
    // The view remains writable by genesis-aware CLI processes.
    other.execute_batch("CREATE TABLE IF NOT EXISTS platform(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL);")?;
    other.execute(
        "INSERT OR IGNORE INTO platform VALUES(1,?1)",
        [serde_json::to_string(&platform::Data::default())?],
    )?;
    let mut legacy: serde_json::Value = serde_json::from_str(&other.query_row::<String, _, _>(
        "SELECT data FROM platform WHERE id=1",
        [],
        |r| r.get(0),
    )?)?;
    assert_eq!(legacy["launches"]["test-launch"]["expires"], time + 301);
    legacy["profiles"]["player@example.test"]["enabled"] = false.into();
    other.execute(
        "UPDATE platform SET data=?1 WHERE id=1",
        [legacy.to_string()],
    )?;
    assert!(
        !db.read()?.members["player@example.test"].enabled,
        "external writer did not invalidate the snapshot"
    );
    assert_eq!(db.read()?.launches["test-launch"].expires, time + 301);
    // Old insert must not reset runtime state; expired cleanup must not erase a renewed guest.
    db.cleanup()?;
    assert!(db.read()?.launches.contains_key("test-launch"));

    // A build with native resolution strips the new manifest field from platform_catalog
    // so previous genesis binaries with deny_unknown_fields on Manifest can read and write it.
    db.edit(|d| {
        d.builds.insert(
            "pond/one".into(),
            serde_json::from_value(json!({
                "manifest":{
                    "schema_version":1,"app_id":"pond","title":"Pond",
                    "resolution":"native","play_modes":["solo"],"solo_runtime":"client_only"
                },
                "server":null,"spa":false,"cross_origin_isolation":false,"ready":true,"created":time,"files":{}
            }))?,
        );
        Ok(())
    })?;
    let raw_view: String =
        other.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let view_json: serde_json::Value = serde_json::from_str(&raw_view)?;
    assert!(
        view_json["builds"]["pond/one"]["manifest"]
            .get("resolution")
            .is_none(),
        "rollback compatibility view must not expose unknown manifest fields to older binaries"
    );
    other.execute("UPDATE platform SET data=?1 WHERE id=1", [&raw_view])?;
    assert_eq!(
        db.catalog()?.builds["pond/one"].manifest.resolution,
        infuser::manifest::Resolution::Native,
        "rollback binary rewrite of platform view must preserve native_resolution_builds"
    );

    // An active read transaction pins a single WAL snapshot even if an operator CLI
    // process commits across multiple tables before the transaction finishes.
    let reader = Database::open(dir.path())?;
    {
        let mut conn = reader.connection.lock().unwrap();
        let tx = conn.transaction()?;
        let pinned_rev: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        // External operator CLI commits changes across both platform and private_groups while tx is open.
        let mut updated = view_json.clone();
        updated["profiles"]["player@example.test"]["name"] = "Renamed by operator".into();
        let mut groups: serde_json::Value =
            serde_json::from_str(&other.query_row::<String, _, _>(
                "SELECT data FROM private_groups WHERE id=1",
                [],
                |r| r.get(0),
            )?)?;
        groups["names"]["away"] = "Away Group".into();
        other.execute(
            "UPDATE platform SET data=?1 WHERE id=1",
            [updated.to_string()],
        )?;
        other.execute(
            "UPDATE private_groups SET data=?1 WHERE id=1",
            [groups.to_string()],
        )?;
        // Force a cache reload inside the pinned read transaction.
        let tx_rev: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(tx_rev, pinned_rev);
        let tx_raw: String =
            tx.query_row("SELECT data FROM platform_catalog WHERE id=1", [], |r| {
                r.get(0)
            })?;
        let tx_groups: String =
            tx.query_row("SELECT data FROM private_groups WHERE id=1", [], |r| {
                r.get(0)
            })?;
        assert!(!tx_raw.contains("Renamed by operator"));
        assert!(!tx_groups.contains("Away Group"));
    }
    let after_commit = reader.catalog()?;
    assert_eq!(
        after_commit.members["player@example.test"].name,
        "Renamed by operator"
    );
    assert_eq!(after_commit.groups.names["away"], "Away Group");
    Ok(())
}

#[test]
fn pre_genesis_binary_cannot_write_after_migration() -> Result<()> {
    let Some(binary) = std::env::var_os("INFUSER_PRE_GENESIS_BINARY") else {
        return Ok(());
    };
    let root = tempfile::tempdir()?;
    let cfg = root.path().join("config.toml");
    let state = root.path().join("state");
    std::fs::write(
        &cfg,
        format!("[controller]\nstate_dir={:?}", state.to_str().unwrap()),
    )?;
    let old_member = |name: &str| -> Result<()> {
        let result = std::process::Command::new(&binary)
            .arg("member")
            .arg(&cfg)
            .args(["player@example.test", name])
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "old binary failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(())
    };
    old_member("Before upgrade")?;
    let old = rusqlite::Connection::open(state.join("platform.sqlite"))?;
    let db = Database::open(&state)?;
    let time = infuser::controller::now();
    db.edit(|d| {
        d.launches.insert("guest".into(),serde_json::from_value(json!({"app":"pond","build":"one","email":"player@example.test","guest":true,"host":"pond.games.example.com","token_hash":"fixture","exchanged":true,"expires":time+1,"grant_expires":time+1,"seen":time,"lease_id":null,"lease_token":null}))?); Ok(())
    })?;
    db.renew_launch("guest", time, None)?;
    db.read()?;
    assert!(old_member("Must be rejected").is_err());
    assert_eq!(
        db.read()?.members["player@example.test"].name,
        "Before upgrade"
    );
    let raw: String = old.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let catalog: serde_json::Value = serde_json::from_str(&raw)?;
    assert_eq!(catalog["identity_version"], 1);
    assert!(catalog.get("members").is_none());
    assert_eq!(db.read()?.launches["guest"].expires, time + 300);
    drop(db);
    assert_eq!(
        Database::open(&state)?.read()?.launches["guest"].expires,
        time + 300
    );
    Ok(())
}

#[test]
fn heartbeat_slides_only_a_member_browser_authorization() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(dir.path())?;
    let start = 1_000_000;
    let window = 7 * 24 * 60 * 60;
    let host = "pond.games.example.test";
    db.edit(|d| {
        d.invite_to_group("home", "player@example.test", "Player", false, true)?;
        Ok(())
    })?;
    let launch: platform::Launch = serde_json::from_value(json!({
        "email":"player@example.test","host":host,"app":"pond","build":"one","guest":false,
        "token_hash":"first","exchanged":false,"expires":start+300,"seen":start,
        "grant_expires":start+120,"lease_id":null,"lease_token":null
    }))?;
    assert_eq!(launch.authorized_until, 0);
    assert!(!launch.authorized(start + 300));
    db.insert_launch("first", &launch)?;
    db.exchange_launch("first", host, start)?;
    assert_eq!(
        db.read()?.launches["first"].authorized_until,
        start + window
    );
    db.expire_launch("first", start + 1)?;
    let wake = start + 600;
    db.find_launch("first", host, wake)?;
    db.renew_launch("first", wake, None)?;
    let row = &db.read()?.launches["first"];
    assert_eq!(
        (row.seen, row.expires, row.authorized_until),
        (wake, wake + 300, wake + window)
    );
    assert!(db.renew_launch("first", wake + window, None).is_err());
    let second = platform::Launch {
        token_hash: "second".into(),
        grant_expires: wake + 120,
        ..launch
    };
    db.insert_launch("second", &second)?;
    db.exchange_launch("second", host, wake + 1)?;
    assert!(db.find_launch("first", host, wake + 1).is_err());
    assert!(db.renew_launch("first", wake + 1, None).is_err());
    db.find_launch("second", host, wake + 1)?;

    let guest = platform::Launch {
        guest: true,
        token_hash: "guest".into(),
        ..second
    };
    db.insert_launch("guest", &guest)?;
    db.exchange_launch("guest", host, wake + 2)?;
    db.save_write("guest-guest", "pond", "progress", 0, &json!({"level":4}))?;
    assert_eq!(db.read()?.launches["guest"].authorized_until, 0);
    assert!(db.find_launch("guest", host, wake + 302).is_err());
    db.cleanup()?;
    assert!(!db.read()?.launches.contains_key("guest"));
    assert_eq!(db.save_read("guest-guest", "pond", "progress")?.0, 0);

    Ok(())
}

#[test]
fn launch_auth_paths_use_the_authorization_clock() {
    let activity = include_str!("../src/platform/activity.rs");
    let portal = include_str!("../src/portal.rs");
    for (source, name) in [
        (activity, "find_launch"),
        (activity, "take_launch_intent"),
        (activity, "renew_launch"),
        (portal, "refresh_locked"),
        (portal, "verify_ticket"),
    ] {
        let (prefix, tail) = source.split_once(&format!("fn {name}(")).unwrap();
        let declaration = prefix.rsplit('\n').next().unwrap();
        let indent = &declaration[..declaration.len() - declaration.trim_start().len()];
        let end = format!("{indent}}}");
        let body = tail
            .lines()
            .take_while(|line| *line != end)
            .collect::<String>();
        assert!(
            body.contains(".authorized("),
            "{name} must use Launch::authorized"
        );
        assert!(
            !body
                .split_whitespace()
                .collect::<String>()
                .contains(".expires>"),
            "{name} checks liveness as authorization"
        );
    }
}
