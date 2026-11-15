//! Small persistent platform store. SQLite transactions also coordinate operator CLI edits.
use crate::{
    config::{Config, Game},
    controller::now,
    manifest::Manifest,
    store::secret,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
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
fn listed() -> bool {
    true
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Launch {
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
        c.execute(
            "INSERT OR IGNORE INTO platform(id,data) VALUES(1,?1)",
            [serde_json::to_string(&Data::default())?],
        )?;
        Ok(Self {
            connection: Mutex::new(c),
        })
    }
    pub fn read(&self) -> Result<Data> {
        let raw: String = self.connection.lock().unwrap().query_row(
            "SELECT data FROM platform WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let mut data: Data = serde_json::from_str(&raw)?;
        let result = f(&mut data)?;
        tx.execute(
            "UPDATE platform SET data=?1 WHERE id=1",
            [serde_json::to_string(&data)?],
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
        let expired_guests = self.edit(|data| {
            let expired: Vec<_> = data
                .launches
                .iter()
                .filter(|(_, l)| l.guest && l.expires < now())
                .map(|(id, _)| format!("guest-{id}"))
                .collect();
            data.launches.retain(|_, l| l.expires > now());
            data.presence.retain(|_, t| *t + 300 > now());
            Ok(expired)
        })?;
        let c = self.connection.lock().unwrap();
        for owner in expired_guests {
            c.execute("DELETE FROM saves WHERE owner=?1", [owner])?;
        }
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
pub fn preview(config: &Config, app: &str, email: &str) -> Result<serde_json::Value> {
    let email = email.to_lowercase();
    Database::open(&config.controller.state_dir)?.edit(|d| {
        ensure!(d.members.get(&email).is_some_and(|m|m.enabled&&m.admin),"preview requires an existing enabled adult admin");
        let selected=d.apps.get(app).context("unknown app")?;
        ensure!(!selected.listed,"operator preview only accepts hidden test games");
        let build=d.builds.get(&format!("{app}/{}",selected.build)).context("missing build")?;
        ensure!(build.server.is_none(),"operator preview currently supports client-only web games");
        let id=secret()[..32].to_string();let token=secret();
        let host=format!("g-{id}.{}",config.controller.domain);
        d.launches.insert(id.clone(),Launch {app:app.into(),build:selected.build.clone(),email,guest:true,host:host.clone(),token_hash:hash(&token),exchanged:false,expires:now()+300,grant_expires:now()+120,seen:now(),lease_id:None,lease_token:None});
        Ok(serde_json::json!({"launch_id":id,"url":format!("https://{host}/_infuser/launch#infuser={token}")}))
    })
}
