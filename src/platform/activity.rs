//! Small activity rows behind a writable compatibility view. Genesis-compatible
//! binaries read and write `platform` without losing launches or guest saves.
use super::*;

pub(super) fn initialize(c: &mut Connection) -> Result<()> {
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let kind: String = tx.query_row(
        "SELECT type FROM sqlite_master WHERE name='platform'",
        [],
        |r| r.get(0),
    )?;
    if kind == "table" {
        tx.execute_batch("ALTER TABLE platform RENAME TO platform_catalog;
            CREATE TABLE runtime_launches(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE runtime_presence(email TEXT PRIMARY KEY, seen INTEGER NOT NULL);
            INSERT INTO runtime_launches SELECT key,value FROM platform_catalog,json_each(platform_catalog.data,'$.launches');
            INSERT INTO runtime_presence SELECT key,value FROM platform_catalog,json_each(platform_catalog.data,'$.presence');
            UPDATE platform_catalog SET data=json_set(data,'$.launches',json('{}'),'$.presence',json('{}'));
            CREATE VIEW platform AS SELECT id,json_set(data,
                '$.launches',json((SELECT json_group_object(id,json(data)) FROM runtime_launches)),
                '$.presence',json((SELECT json_group_object(email,seen) FROM runtime_presence))) AS data FROM platform_catalog;
            CREATE TRIGGER platform_insert INSTEAD OF INSERT ON platform
            WHEN NOT EXISTS(SELECT 1 FROM platform_catalog WHERE id=NEW.id)
            BEGIN
                INSERT INTO platform_catalog VALUES(NEW.id,json_set(NEW.data,'$.launches',json('{}'),'$.presence',json('{}')));
                INSERT INTO runtime_launches SELECT key,value FROM json_each(NEW.data,'$.launches');
                INSERT INTO runtime_presence SELECT key,value FROM json_each(NEW.data,'$.presence');
            END;
            CREATE TRIGGER platform_update INSTEAD OF UPDATE ON platform BEGIN
                UPDATE platform_catalog SET data=json_set(NEW.data,'$.launches',json('{}'),'$.presence',json('{}'))
                    WHERE id=NEW.id AND data!=json_set(NEW.data,'$.launches',json('{}'),'$.presence',json('{}'));
                DELETE FROM runtime_launches WHERE id NOT IN (SELECT key FROM json_each(NEW.data,'$.launches'));
                INSERT INTO runtime_launches SELECT key,value FROM json_each(NEW.data,'$.launches') WHERE true
                    ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE data!=excluded.data;
                DELETE FROM runtime_presence WHERE email NOT IN (SELECT key FROM json_each(NEW.data,'$.presence'));
                INSERT INTO runtime_presence SELECT key,value FROM json_each(NEW.data,'$.presence') WHERE true
                    ON CONFLICT(email) DO UPDATE SET seen=excluded.seen WHERE seen!=excluded.seen;
            END;")?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS catalog_revision(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL);
        INSERT OR IGNORE INTO catalog_revision VALUES(1,0);")?;
    // Invalidation belongs in SQLite so operator CLI writes and supported older
    // binaries invalidate snapshots too. Activity does not change this revision.
    for (table, column) in [
        ("platform_catalog", "data"),
        ("private_groups", "data"),
        ("deployments", "data"),
        ("visits", "data"),
        ("server_builds", "declaration"),
        ("game_urls", "declaration"),
        ("sdk_manifests", "declaration"),
        ("cloud_manifests", "declaration"),
        ("runtime_manifests", "server_start"),
        ("intent_manifests", "declaration"),
        ("dedicated_manifests", "declaration"),
    ] {
        tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS revision_{table}_update AFTER UPDATE ON {table}
            WHEN OLD.{column} != NEW.{column} BEGIN UPDATE catalog_revision SET revision=revision+1; END;
            CREATE TRIGGER IF NOT EXISTS revision_{table}_insert AFTER INSERT ON {table} BEGIN UPDATE catalog_revision SET revision=revision+1; END;
            CREATE TRIGGER IF NOT EXISTS revision_{table}_delete AFTER DELETE ON {table} BEGIN UPDATE catalog_revision SET revision=revision+1; END;"))?;
    }
    // Metadata has several columns, including independent art bytes.
    for operation in ["INSERT", "UPDATE", "DELETE"] {
        tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS revision_metadata_{operation} AFTER {operation} ON game_metadata BEGIN UPDATE catalog_revision SET revision=revision+1; END;"))?;
    }
    tx.commit()?;
    Ok(())
}

pub(super) fn load(c: &Connection, data: &mut Data) -> Result<()> {
    let mut statement = c.prepare("SELECT id,data FROM runtime_launches")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, raw) = row?;
        data.launches.insert(id, serde_json::from_str(&raw)?);
    }
    let mut statement = c.prepare("SELECT email,seen FROM runtime_presence")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))? {
        let (email, seen) = row?;
        data.presence.insert(email, seen);
    }
    Ok(())
}
impl Database {
    pub fn see_member(&self, email: &str, time: u64) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO runtime_presence VALUES(?1,?2)
            ON CONFLICT(email) DO UPDATE SET seen=excluded.seen WHERE seen<excluded.seen",
            params![email, time],
        )?;
        Ok(())
    }
    pub fn renew_launch(&self, id: &str, time: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<String> = tx
            .query_row("SELECT data FROM runtime_launches WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let mut launch: Launch = serde_json::from_str(
            &raw.ok_or_else(|| crate::portal::fault(410, "game session expired"))?,
        )?;
        ensure!(
            launch.expires > time,
            crate::portal::fault(410, "game session expired")
        );
        ensure!(
            self.cached_catalog(&tx)?.launch_allowed(&launch, time),
            crate::portal::fault(403, "Game authorization ended.")
        );
        launch.seen = time;
        launch.expires = time + 300;
        tx.execute(
            "UPDATE runtime_launches SET data=?2 WHERE id=?1",
            params![id, serde_json::to_string(&launch)?],
        )?;
        if !launch.guest {
            tx.execute("INSERT INTO runtime_presence VALUES(?1,?2) ON CONFLICT(email) DO UPDATE SET seen=excluded.seen WHERE seen<excluded.seen",params![launch.email,time])?;
        }
        tx.commit()?;
        Ok(())
    }
}
