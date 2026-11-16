//! Upgrade real legacy rows, then exercise the same policy for chat and push.
use anyhow::Result;
use infuser::{
    controller::now,
    notifications::Subscription,
    platform::{
        Data, Database, Member,
        visits::{Participant, Visit},
    },
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::Path;

fn subscription(device: &str) -> Subscription {
    Subscription {
        endpoint: format!("https://fcm.googleapis.com/fcm/send/{device}"),
    }
}
fn people() -> Data {
    let mut d = Data::default();
    for email in ["host", "visitor", "other", "operator"] {
        d.members.insert(
            email.into(),
            Member {
                name: email.into(),
                enabled: true,
                admin: email != "host",
                avatar: None,
            },
        );
    }
    d.groups.names.insert("away".into(), "Away".into());
    d.groups.members.insert("visitor".into(), "away".into());
    d.groups.members.insert("other".into(), "away".into());
    d.groups.operators.insert("operator".into());
    d
}
fn legacy(root: &Path, grouped: bool) -> Result<()> {
    let c = Connection::open(root.join("platform.sqlite"))?;
    c.execute_batch("CREATE TABLE library_chat (
        id INTEGER PRIMARY KEY AUTOINCREMENT, author TEXT NOT NULL,
        client_id TEXT NOT NULL, text TEXT, created INTEGER NOT NULL, UNIQUE(author,client_id));
        CREATE TABLE platform(id INTEGER PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE private_groups(id INTEGER PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE visits(id INTEGER PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE push_subscriptions(endpoint TEXT PRIMARY KEY,email TEXT NOT NULL,private_group TEXT NOT NULL,subscription TEXT NOT NULL,updated INTEGER NOT NULL);
        CREATE TABLE push_outbox(endpoint TEXT PRIMARY KEY,message_id INTEGER NOT NULL,created INTEGER NOT NULL,due INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0);")?;
    if grouped {
        c.execute(
            "ALTER TABLE library_chat ADD COLUMN private_group TEXT NOT NULL DEFAULT 'home'",
            [],
        )?;
    }
    let mut d = people();
    d.visits.insert(
        "visit".into(),
        Visit {
            name: "Visit".into(),
            host: "home".into(),
            games: Default::default(),
            ended: false,
            expires: now() + 3600,
            participants: [(
                "visitor".into(),
                Participant {
                    after_message: 8,
                    ended: false,
                },
            )]
            .into(),
        },
    );
    c.execute(
        "INSERT INTO platform VALUES(1,?1)",
        [serde_json::to_string(&d)?],
    )?;
    c.execute(
        "INSERT INTO private_groups VALUES(1,?1)",
        [serde_json::to_string(&d.groups)?],
    )?;
    c.execute(
        "INSERT INTO visits VALUES(1,?1)",
        [serde_json::to_string(&d.visits)?],
    )?;
    for (id, author, text) in [
        (7, "host", Some("Before visit")),
        (8, "host", None),
        (9, "visitor", Some("Hello host")),
        (10, "host", Some("Welcome")),
    ] {
        c.execute(
            "INSERT INTO library_chat(id,author,client_id,text,created) VALUES(?1,?2,?3,?4,?5)",
            params![id, author, format!("legacy-request-{id:04}"), text, now()],
        )?;
    }
    // A pruned/deleted highest row must not allow IDs to be reused on upgrade.
    c.execute(
        "UPDATE sqlite_sequence SET seq=1000 WHERE name='library_chat'",
        [],
    )?;
    for (device, email, message) in [
        ("visitor", "visitor", 10),
        ("old", "visitor", 7),
        ("outside", "other", 10),
        ("host", "host", 9),
    ] {
        let sub = subscription(device);
        c.execute(
            "INSERT INTO push_subscriptions VALUES(?1,?2,?3,?4,?5)",
            params![
                sub.endpoint,
                email,
                d.groups.member(email),
                serde_json::to_string(&sub)?,
                now()
            ],
        )?;
        c.execute(
            "INSERT INTO push_outbox(endpoint,message_id,created,due) VALUES(?1,?2,?3,0)",
            params![sub.endpoint, message, now()],
        )?;
    }
    if grouped {
        c.execute("INSERT INTO library_chat(author,client_id,text,created,private_group) VALUES('other','other-group-request','Other room',?1,'away')", [now()])?;
    }
    Ok(())
}
fn channel(db: &Database, group: &str) -> Result<String> {
    Ok(db.connection.lock().unwrap().query_row(
        "SELECT id FROM chat_channels WHERE default_group=?1",
        [group],
        |r| r.get(0),
    )?)
}
fn ids(value: Value) -> Vec<u64> {
    value["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn upgrade_preserves_visits_retries_history_notifications_and_old_writers() -> Result<()> {
    for grouped in [false, true] {
        let dir = tempfile::tempdir()?;
        legacy(dir.path(), grouped)?;
        let db = Database::open(dir.path())?;
        let home = channel(&db, "home")?;
        assert_ne!(home, "home");
        assert_eq!(ids(db.chat_list("host")?), vec![7, 9, 10]);
        assert_eq!(
            ids(db.chat_list_group("visitor", "home", false)?),
            vec![9, 10]
        );
        assert!(db.chat_list_channel("other", &home, false).is_err());
        assert_eq!(
            db.chat_send("host", "legacy-request-0008", "Removed")?,
            json!({"id":8})
        );
        assert_eq!(
            db.chat_send_group("visitor", "home", "legacy-request-0009", "Hello host")?,
            json!({"id":9})
        );
        assert!(db.chat_remove_group("visitor", "home", 7).is_err());
        assert!(
            db.chat_remove_group("visitor", "home", 10).is_err(),
            "being an admin at home gives no host moderation rights"
        );
        let targets = db.push_targets()?;
        assert_eq!(targets.len(), 2);
        assert!(
            targets
                .iter()
                .any(|(e, id, _)| e == &subscription("visitor").endpoint && *id == 10)
        );
        assert!(
            targets
                .iter()
                .any(|(e, id, _)| e == &subscription("host").endpoint && *id == 9)
        );
        // Expiry invalidates an outbox loaded from the old schema, not only new sends.
        db.edit(|d| {
            d.visits.get_mut("visit").unwrap().expires = now();
            Ok(())
        })?;
        assert!(db.chat_list_group("visitor", "home", false).is_err());
        assert_eq!(db.push_targets()?.len(), 1);
        assert_eq!(ids(db.chat_list("host")?), vec![7, 9, 10]);
        // A second visit cannot reveal old history through a retry or old push.
        db.edit(|d| {
            let v = d.visits.get_mut("visit").unwrap();
            v.expires = now() + 3600;
            v.participants.get_mut("visitor").unwrap().after_message = 1001;
            Ok(())
        })?;
        assert_eq!(
            ids(db.chat_list_group("visitor", "home", false)?),
            Vec::<u64>::new()
        );
        assert!(
            db.chat_send_group("visitor", "home", "legacy-request-0009", "Hello host")
                .is_err()
        );
        // SQL used by the immediately previous daemon remains writable after migration.
        {
            let c = db.connection.lock().unwrap();
            c.execute("INSERT INTO library_chat(author,client_id,text,created,private_group) VALUES('host','old-binary-request','After upgrade',?1,'home')",[now()])?;
            let id = c.last_insert_rowid();
            assert!(id > 1000);
            let mapped: String = c.query_row(
                "SELECT channel_id FROM library_chat WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            assert_eq!(mapped, home);
            c.execute("UPDATE library_chat SET text=NULL WHERE id=7", [])?;
        }
        let next = db.chat_send("host", "new-binary-request", "New server")?["id"]
            .as_u64()
            .unwrap();
        assert!(next > 1001);
        let before = db.chat_list("host")?;
        drop(db);
        let reopened = Database::open(dir.path())?;
        assert_eq!(channel(&reopened, "home")?, home);
        assert_eq!(reopened.chat_list("host")?, before);
        assert_eq!(
            ids(reopened.chat_list_group("visitor", "home", false)?).last(),
            Some(&next)
        );
        reopened.edit(|d| {
            d.groups.names.insert("home".into(), "Renamed group".into());
            Ok(())
        })?;
        assert_eq!(channel(&reopened, "home")?, home);
    }
    Ok(())
}

#[test]
fn failed_migration_rolls_back_schema_and_rows_then_retries_cleanly() -> Result<()> {
    let dir = tempfile::tempdir()?;
    legacy(dir.path(), true)?;
    {
        let c = Connection::open(dir.path().join("platform.sqlite"))?;
        c.execute_batch("CREATE TRIGGER simulate_failure BEFORE UPDATE ON library_chat BEGIN SELECT RAISE(ABORT,'migration failure'); END;")?;
    }
    assert!(Database::open(dir.path()).is_err());
    {
        let c = Connection::open(dir.path().join("platform.sqlite"))?;
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM pragma_table_info('library_chat') WHERE name='channel_id'",
                [],
                |r| r.get::<_, u64>(0)
            )?,
            0
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='chat_channels'",
                [],
                |r| r.get::<_, u64>(0)
            )?,
            0
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM library_chat", [], |r| r
                .get::<_, u64>(0))?,
            5
        );
        c.execute_batch("DROP TRIGGER simulate_failure;")?;
    }
    let db = Database::open(dir.path())?;
    assert_eq!(ids(db.chat_list("host")?), vec![7, 9, 10]);
    Ok(())
}

#[test]
fn explicit_channel_uses_same_policy_for_messages_and_notifications_without_new_routes()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(dir.path())?;
    db.edit(|d| {
        *d = people();
        Ok(())
    })?;
    for who in ["host", "visitor", "other", "operator"] {
        db.push_subscribe(who, &subscription(who))?;
    }
    // No product creation API yet: install an internal audience to test the foundation.
    {
        let c = db.connection.lock().unwrap();
        c.execute(
            "INSERT INTO chat_channels(id,expires) VALUES('conversation',?1)",
            [now() + 3600],
        )?;
        for who in ["host", "visitor"] {
            c.execute(
                "INSERT INTO chat_participants(channel_id,email) VALUES('conversation',?1)",
                [who],
            )?;
        }
    }
    let first = db.chat_send_channel(
        "host",
        "conversation",
        "explicit-message-01",
        "Private conversation",
    )?["id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        ids(db.chat_list_channel("visitor", "conversation", false)?),
        vec![first]
    );
    assert_eq!(db.push_targets()?.len(), 1);
    assert!(db.push_targets()?[0].0.ends_with("visitor"));
    assert!(
        db.chat_list_channel("other", "conversation", false)
            .is_err()
    );
    assert!(
        db.chat_list_channel("operator", "conversation", true)
            .is_err()
    );
    assert!(
        db.chat_send_channel("other", "conversation", "outsider-message-01", "No")
            .is_err()
    );
    assert!(
        db.chat_remove_channel("visitor", "conversation", first)
            .is_err()
    );
    assert!(ids(db.chat_list("host")?).is_empty());
    assert!(ids(db.chat_list("visitor")?).is_empty());
    assert!(
        db.chat_send("host", "explicit-message-01", "Private conversation")
            .is_err(),
        "request IDs remain sender-wide"
    );
    db.connection.lock().unwrap().execute(
        "UPDATE chat_participants SET expires=?1 WHERE email='visitor'",
        [now()],
    )?;
    assert!(
        db.chat_list_channel("visitor", "conversation", false)
            .is_err()
    );
    assert!(db.push_targets()?.is_empty());
    db.connection.lock().unwrap().execute(
        "UPDATE chat_participants SET expires=NULL,after_message=?1 WHERE email='visitor'",
        [first],
    )?;
    assert!(ids(db.chat_list_channel("visitor", "conversation", false)?).is_empty());
    db.chat_send_channel("host", "conversation", "explicit-message-02", "New window")?;
    assert_eq!(db.push_targets()?.len(), 1);
    db.connection.lock().unwrap().execute(
        "UPDATE chat_channels SET expires=?1 WHERE id='conversation'",
        [now()],
    )?;
    assert!(db.chat_list_channel("host", "conversation", false).is_err());
    assert!(
        db.chat_send_channel("host", "conversation", "explicit-message-03", "Expired")
            .is_err()
    );
    assert!(
        db.chat_remove_channel("host", "conversation", first)
            .is_err()
    );
    assert!(db.push_targets()?.is_empty());
    Ok(())
}
