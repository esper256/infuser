//! Small activity rows behind a writable compatibility view. Genesis-compatible
//! binaries read and write `platform` without losing launches or guest saves.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct SessionEnded {
    pub status: u16,
    pub reason: &'static str,
    pub launch: Option<String>,
}

impl std::fmt::Display for SessionEnded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Game session ended.")
    }
}
impl std::error::Error for SessionEnded {}

pub(crate) fn denied(id: Option<&str>, status: u16, reason: &'static str) -> anyhow::Error {
    SessionEnded {
        status,
        reason,
        launch: id.map(crate::portal::telemetry::launch_ref),
    }
    .into()
}

impl Data {
    pub(crate) fn launch_build(&self, id: &str, launch: &Launch) -> Result<&Build> {
        self.builds
            .get(&format!("{}/{}", launch.app, launch.build))
            .ok_or_else(|| denied(Some(id), 410, "build_removed"))
    }

    pub(crate) fn authorize_launch(&self, id: &str, launch: &Launch, time: u64) -> Result<()> {
        if !self.profile_enabled(&launch.email) {
            return Err(denied(Some(id), 403, "profile_disabled"));
        }
        if !self.launch_allowed(launch, time) {
            return Err(denied(Some(id), 403, "access_revoked"));
        }
        if !launch.authorized(time) {
            let reason = if launch.ended {
                "ended"
            } else if !launch.exchanged {
                "unexchanged"
            } else {
                "stale"
            };
            return Err(denied(Some(id), 401, reason));
        }
        Ok(())
    }
}

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
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS runtime_launch_token ON runtime_launches(json_extract(data,'$.token_hash'));
        CREATE TABLE IF NOT EXISTS profile_input(profile TEXT PRIMARY KEY, seen INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS launch_modes(launch TEXT PRIMARY KEY, mode TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS player_pages(page TEXT PRIMARY KEY,host TEXT NOT NULL,closed INTEGER NOT NULL DEFAULT 0);
        CREATE TRIGGER IF NOT EXISTS remove_player_pages AFTER DELETE ON runtime_launches
        BEGIN DELETE FROM player_pages WHERE host=json_extract(OLD.data,'$.host')
        AND NOT EXISTS(SELECT 1 FROM runtime_launches WHERE json_extract(data,'$.host')=player_pages.host); END;
        CREATE TRIGGER IF NOT EXISTS remove_launch_mode AFTER DELETE ON runtime_launches
        BEGIN DELETE FROM launch_modes WHERE launch=OLD.id; END;",
    )?;
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
    // Metadata and flag tables do not use a single declaration column.
    for table in [
        "game_metadata",
        "portrait_builds",
        "native_resolution_builds",
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS revision_{table}_{operation} AFTER {operation} ON {table} BEGIN UPDATE catalog_revision SET revision=revision+1; END;"))?;
        }
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
fn lookup(c: &Connection, token_hash: &str) -> Result<Option<(String, Launch)>> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT id,data FROM runtime_launches WHERE json_extract(data,'$.token_hash')=?1",
            [token_hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(id, raw)| Ok((id, serde_json::from_str(&raw)?)))
        .transpose()
}

impl Database {
    pub(crate) fn diagnostic_launch(
        &self,
        token_hash: &str,
        host: &str,
    ) -> Result<Option<(String, Launch)>> {
        Ok(lookup(&self.connection.lock().unwrap(), token_hash)?.filter(|(_, l)| l.host == host))
    }
    pub fn see_member(&self, email: &str, time: u64) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO runtime_presence VALUES(?1,?2)
            ON CONFLICT(email) DO UPDATE SET seen=excluded.seen WHERE seen<excluded.seen",
            params![email, time],
        )?;
        Ok(())
    }
    pub fn find_launch(&self, token_hash: &str, host: &str, time: u64) -> Result<(String, Launch)> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data = self.cached_catalog(&tx)?;
        let (id, l) = lookup(&tx, token_hash)?.ok_or_else(|| denied(None, 401, "unknown"))?;
        if l.host != host {
            return Err(denied(Some(&id), 401, "wrong_host"));
        }
        data.authorize_launch(&id, &l, time)?;
        Ok((id, l))
    }
    pub fn insert_launch(&self, id: &str, launch: &Launch) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO runtime_launches(id,data) VALUES(?1,?2)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE data!=excluded.data",
            params![id, serde_json::to_string(launch)?],
        )?;
        Ok(())
    }
    pub fn extend_launch_grant(&self, id: &str, time: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(raw) = tx
            .query_row("SELECT data FROM runtime_launches WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            let mut launch: Launch = serde_json::from_str(&raw)?;
            if launch.live(time) && !launch.exchanged && launch.grant_expires != 0 {
                launch.grant_expires = time + 120;
                launch.seen = time;
                launch.expires = time + 300;
                tx.execute(
                    "UPDATE runtime_launches SET data=?2 WHERE id=?1",
                    params![id, serde_json::to_string(&launch)?],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn exchange_launch(&self, token_hash: &str, host: &str, time: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let policy = self.cached_catalog(&tx)?;
        let (id, mut l) = lookup(&tx, token_hash)?
            .filter(|(_, l)| l.host == host && !l.ended && !l.exchanged && l.grant_expires > time)
            .ok_or_else(|| crate::portal::fault(401, "launch link expired or already used"))?;
        ensure!(
            policy.profile_enabled(&l.email),
            crate::portal::fault(403, "membership disabled")
        );
        ensure!(
            policy.launch_allowed(&l, time),
            crate::portal::fault(403, "Game membership changed.")
        );
        l.exchanged = true;
        l.seen = time;
        l.expires = time + 300;
        tx.execute(
            "UPDATE runtime_launches SET data=?2 WHERE id=?1",
            params![id, serde_json::to_string(&l)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn take_launch_intent(
        &self,
        id: &str,
        time: u64,
    ) -> Result<Option<crate::launch_intent::Intent>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<String> = tx
            .query_row("SELECT data FROM runtime_launches WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let mut launch: Launch =
            serde_json::from_str(&raw.ok_or_else(|| denied(Some(id), 410, "unknown"))?)?;
        self.cached_catalog(&tx)?
            .authorize_launch(id, &launch, time)?;
        let intent = launch.intent.take();
        if intent.is_some() {
            tx.execute(
                "UPDATE runtime_launches SET data=?2 WHERE id=?1",
                params![id, serde_json::to_string(&launch)?],
            )?;
        }
        tx.commit()?;
        Ok(intent)
    }
    pub fn expire_launch(&self, id: &str, time: u64) -> Result<bool> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = false;
        if let Some(raw) = tx
            .query_row("SELECT data FROM runtime_launches WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            let mut launch: Launch = serde_json::from_str(&raw)?;
            changed = launch.live(time);
            launch.expires = time;
            tx.execute(
                "UPDATE runtime_launches SET data=?2 WHERE id=?1",
                params![id, serde_json::to_string(&launch)?],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
    pub fn renew_launch(&self, id: &str, time: u64, ended_lease: Option<&str>) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: Option<String> = tx
            .query_row("SELECT data FROM runtime_launches WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let mut launch: Launch =
            serde_json::from_str(&raw.ok_or_else(|| denied(Some(id), 410, "unknown"))?)?;
        self.cached_catalog(&tx)?
            .authorize_launch(id, &launch, time)?;
        if ended_lease.is_some() && launch.lease_id.as_deref() == ended_lease {
            launch.lease_id = None;
            launch.lease_token = None;
        }
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
