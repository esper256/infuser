//! Small persistent platform store. SQLite transactions also coordinate operator CLI edits.
use crate::{
    config::{Config, Game},
    controller::now,
    manifest::Manifest,
    store::secret,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

pub fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub admin: bool,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub app: String,
    pub role: String,
    pub label: String,
    pub hash: String,
    #[serde(default)]
    pub owner: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SetupClaim {
    pub app: String,
    pub owner: String,
    pub label: String,
    pub expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Build {
    pub manifest: Manifest,
    pub server: Option<Game>,
    pub spa: bool,
    pub cross_origin_isolation: bool,
    pub ready: bool,
    pub created: u64,
    pub files: BTreeMap<String, u64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct App {
    pub build: String,
    pub updated: u64,
    #[serde(default = "listed")]
    pub listed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PendingPromotion {
    pub build: String,
    pub listed: bool,
    pub queued_at: u64,
}
fn listed() -> bool {
    true
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Launch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<crate::launch_intent::Intent>,
    pub app: String,
    pub build: String,
    pub email: String,
    pub guest: bool,
    pub host: String,
    pub token_hash: String,
    pub exchanged: bool,
    pub expires: u64,
    pub grant_expires: u64,
    pub seen: u64,
    pub lease_id: Option<String>,
    pub lease_token: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub text: String,
    pub updated: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(skip)]
    pub game_urls: BTreeMap<String, crate::game_urls::GameUrl>,
    #[serde(default)]
    pub pending_promotions: BTreeMap<String, PendingPromotion>,
    #[serde(default)]
    pub tags: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub setup_claims: BTreeMap<String, SetupClaim>,
    #[serde(default)]
    pub owners: BTreeMap<String, String>,
    pub members: BTreeMap<String, Member>,
    pub credentials: BTreeMap<String, Credential>,
    pub apps: BTreeMap<String, App>,
    pub builds: BTreeMap<String, Build>,
    pub launches: BTreeMap<String, Launch>,
    pub presence: BTreeMap<String, u64>,
    pub reactions: BTreeMap<String, BTreeMap<String, Reaction>>,
}
// Keep the legacy catalog JSON readable by older daemons. New declarations
// live separately so an old binary rewriting that catalog cannot erase them.
fn decode_catalog(c: &Connection, raw: &str) -> Result<Data> {
    let mut data: Data = serde_json::from_str(raw)?;
    let mut statement = c.prepare("SELECT app,declaration FROM game_urls")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (app, declaration) = row?;
        data.game_urls
            .insert(app, serde_json::from_str(&declaration)?);
    }
    let mut statement = c.prepare("SELECT build_key,declaration FROM sdk_manifests")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        if let Some(build) = data.builds.get_mut(&key) {
            build.manifest.sdk = Some(serde_json::from_str(&declaration)?);
        }
    }
    let mut statement = c.prepare("SELECT build_key,declaration FROM cloud_manifests")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        if let Some(build) = data.builds.get_mut(&key) {
            build.manifest.cloud_saves = Some(serde_json::from_str(&declaration)?);
        }
    }
    let mut statement = c.prepare("SELECT build_key,server_start FROM runtime_manifests")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        if let Some(build) = data.builds.get_mut(&key) {
            build.manifest.server_start = Some(serde_json::from_str(&declaration)?);
        }
    }
    let mut statement = c.prepare("SELECT build_key,declaration FROM intent_manifests")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        if let Some(build) = data.builds.get_mut(&key) {
            build.manifest.launch_intents = serde_json::from_str(&declaration)?;
        }
    }
    Ok(data)
}
fn encode_catalog(tx: &Transaction<'_>, data: &Data) -> Result<String> {
    let mut value = serde_json::to_value(data)?;
    for (app, url) in &data.game_urls {
        tx.execute("INSERT INTO game_urls(app,declaration) VALUES(?1,?2) ON CONFLICT(app) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![app,serde_json::to_string(url)?])?;
    }
    for (key, build) in &data.builds {
        if let Some(sdk) = &build.manifest.sdk {
            tx.execute("INSERT INTO sdk_manifests(build_key,declaration) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![key,serde_json::to_string(sdk)?])?;
        }
        value["builds"][key]["manifest"]
            .as_object_mut()
            .unwrap()
            .remove("sdk");
        if !build.manifest.launch_intents.is_empty() {
            tx.execute("INSERT INTO intent_manifests(build_key,declaration) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![key,serde_json::to_string(&build.manifest.launch_intents)?])?;
        }
        value["builds"][key]["manifest"]
            .as_object_mut()
            .unwrap()
            .remove("launch_intents");
        if let Some(declaration) = &build.manifest.server_start {
            tx.execute("INSERT INTO runtime_manifests(build_key,server_start) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET server_start=excluded.server_start WHERE server_start!=excluded.server_start",params![key,serde_json::to_string(declaration)?])?;
        }
        value["builds"][key]["manifest"]
            .as_object_mut()
            .unwrap()
            .remove("server_start");
        if let Some(declaration) = &build.manifest.cloud_saves {
            tx.execute("INSERT INTO cloud_manifests(build_key,declaration) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![key,serde_json::to_string(declaration)?])?;
        }
        value["builds"][key]["manifest"]
            .as_object_mut()
            .unwrap()
            .remove("cloud_saves");
    }
    let keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM runtime_manifests")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in keys {
        if !data.builds.contains_key(&key) {
            tx.execute("DELETE FROM runtime_manifests WHERE build_key=?1", [key])?;
        }
    }
    let keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM cloud_manifests")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in keys {
        if !data.builds.contains_key(&key) {
            tx.execute("DELETE FROM cloud_manifests WHERE build_key=?1", [key])?;
        }
    }
    let keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM intent_manifests")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in keys {
        if !data.builds.contains_key(&key) {
            tx.execute("DELETE FROM intent_manifests WHERE build_key=?1", [key])?;
        }
    }
    let keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM sdk_manifests")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in keys {
        if !data.builds.contains_key(&key) {
            tx.execute("DELETE FROM sdk_manifests WHERE build_key=?1", [key])?;
        }
    }
    Ok(serde_json::to_string(&value)?)
}
pub struct Database {
    pub connection: Mutex<Connection>,
}
impl Database {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let c = Connection::open(root.join("platform.sqlite"))?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
          CREATE TABLE IF NOT EXISTS platform (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS saves (owner TEXT, app TEXT, slot TEXT, version INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(owner,app,slot));")?;
        crate::cloud::initialize(&c)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS sdk_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS game_urls(app TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS intent_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS runtime_manifests(build_key TEXT PRIMARY KEY, server_start TEXT NOT NULL);")?;
        c.execute(
            "INSERT OR IGNORE INTO platform(id,data) VALUES(1,?1)",
            [serde_json::to_string(&Data::default())?],
        )?;
        let db = Self {
            connection: Mutex::new(c),
        };
        db.edit(|d| {
            let games: Vec<_> = d
                .apps
                .iter()
                .map(|(app, a)| {
                    (
                        app.clone(),
                        d.builds[&format!("{app}/{}", a.build)]
                            .manifest
                            .title
                            .clone(),
                    )
                })
                .collect();
            for (app, title) in games {
                crate::game_urls::assign(d, &app, &title);
            }
            Ok(())
        })?;
        Ok(db)
    }
    pub fn read(&self) -> Result<Data> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let data = decode_catalog(&tx, &raw)?;
        tx.commit()?;
        Ok(data)
    }
    pub fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let mut data = decode_catalog(&tx, &raw)?;
        let result = f(&mut data)?;
        tx.execute(
            "UPDATE platform SET data=?1 WHERE id=1",
            [encode_catalog(&tx, &data)?],
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn save_read(
        &self,
        owner: &str,
        app: &str,
        slot: &str,
    ) -> Result<(u64, serde_json::Value)> {
        let row: Option<(u64, String)> = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT version,body FROM saves WHERE owner=?1 AND app=?2 AND slot=?3",
                params![owner, app, slot],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((v, b)) => Ok((v, serde_json::from_str(&b)?)),
            None => Ok((0, serde_json::Value::Null)),
        }
    }
    pub fn save_write(
        &self,
        owner: &str,
        app: &str,
        slot: &str,
        expected: u64,
        body: &serde_json::Value,
    ) -> Result<u64> {
        let body = serde_json::to_string(body)?;
        ensure!(
            body.len() <= 1024 * 1024,
            crate::portal::fault(413, "save limit is 1 MiB per slot")
        );
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u64 = tx
            .query_row(
                "SELECT version FROM saves WHERE owner=?1 AND app=?2 AND slot=?3",
                params![owner, app, slot],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        ensure!(
            version == expected,
            crate::portal::fault(409, "save changed on another device; reload before saving")
        );
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM saves WHERE owner=?1 AND app=?2",
            params![owner, app],
            |r| r.get(0),
        )?;
        ensure!(
            version > 0 || count < 16,
            crate::portal::fault(413, "limit of 16 save slots per game")
        );
        tx.execute("INSERT INTO saves(owner,app,slot,version,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,app,slot) DO UPDATE SET version=excluded.version,body=excluded.body",params![owner,app,slot,version+1,body])?;
        tx.commit()?;
        Ok(version + 1)
    }
    pub fn cleanup(&self) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let mut data = decode_catalog(&tx, &raw)?;
        let time = now();
        data.launches.retain(|_, l| l.expires > time);
        data.presence.retain(|_, t| *t + 300 > time);
        let active: std::collections::BTreeSet<_> = data
            .launches
            .iter()
            .filter(|(_, l)| l.guest)
            .map(|(id, _)| format!("guest-{id}"))
            .collect();
        // Also catch guest rows orphaned by an older binary or interrupted cleanup.
        let owners = {
            let mut statement = tx.prepare("SELECT owner FROM saves WHERE owner LIKE 'guest-%' UNION SELECT owner FROM cloud_revisions WHERE owner LIKE 'guest-%' UNION SELECT owner FROM cloud_receipts WHERE owner LIKE 'guest-%'")?;
            statement
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for owner in owners.into_iter().filter(|owner| !active.contains(owner)) {
            tx.execute("DELETE FROM saves WHERE owner=?1", [&owner])?;
            crate::cloud::delete_owner(&tx, &owner)?;
        }
        tx.execute(
            "UPDATE platform SET data=?1 WHERE id=1",
            [encode_catalog(&tx, &data)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}
pub fn member(config: &Config, email: &str, name: &str, admin: bool, enabled: bool) -> Result<()> {
    let email = email.trim().to_lowercase();
    ensure!(
        email.contains('@') && email.len() < 254 && !email.chars().any(char::is_whitespace),
        "invalid email"
    );
    ensure!(
        !name.trim().is_empty() && name.chars().count() <= 40,
        "name must contain 1–40 characters"
    );
    Database::open(&config.controller.state_dir)?.edit(|d| {
        d.members.insert(
            email,
            Member {
                name: name.into(),
                admin,
                enabled,
            },
        );
        Ok(())
    })
}
pub fn credential(data: &mut Data, app: &str, role: &str, label: &str) -> Result<(String, String)> {
    ensure!(crate::config::slug(app), "invalid app id");
    ensure!(
        role == "publish" || role == "server",
        "invalid credential role"
    );
    let id = secret()[..16].to_string();
    let token = secret();
    data.credentials.insert(
        id.clone(),
        Credential {
            app: app.into(),
            role: role.into(),
            label: label.into(),
            hash: hash(&token),
            owner: None,
        },
    );
    Ok((id, token))
}

/// Local operator-only dogfood entry point. Produces a single-use guest launch,
/// scoped to one hidden preview, without handing automation a Google session.
pub fn preview(
    config: &Config,
    app: &str,
    email: &str,
    intent: Option<crate::launch_intent::Intent>,
) -> Result<serde_json::Value> {
    let email = email.to_lowercase();
    Database::open(&config.controller.state_dir)?.edit(|d| {
        ensure!(d.members.get(&email).is_some_and(|m|m.enabled&&m.admin),"preview requires an existing enabled adult admin");
        let selected=d.apps.get(app).context("unknown app")?;
        ensure!(!selected.listed,"operator preview only accepts hidden test games");
        let build=d.builds.get(&format!("{app}/{}",selected.build)).context("missing build")?;
        if let Some(sdk) = &build.manifest.sdk { sdk.admit(&crate::sdk_contract::capabilities(config.controller.turn.is_some()))?; }
        ensure!(build.server.is_none() || build.manifest.server_start.is_some(),"operator preview requires a client-only or on-demand web game");
        if let Some(intent) = &intent { intent.validate(&build.manifest)?; }
        let id=secret()[..32].to_string();let token=secret();
        let host=crate::game_urls::host(d, app, &id, &config.controller.domain);
        d.launches.insert(id.clone(),Launch {intent,app:app.into(),build:selected.build.clone(),email,guest:true,host:host.clone(),token_hash:hash(&token),exchanged:false,expires:now()+300,grant_expires:now()+120,seen:now(),lease_id:None,lease_token:None});
        Ok(serde_json::json!({"launch_id":id,"url":format!("https://{host}/_infuser/launch#infuser={token}")}))
    })
}
