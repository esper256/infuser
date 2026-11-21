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
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};
mod activity;
pub(crate) mod channels;
mod chat;
pub mod deployments;
pub mod groups;
pub mod identity;
pub mod member_link;
mod metadata;
pub(crate) mod releases;
pub mod screenshots;
pub mod visits;
pub use metadata::{GameMetadata, MetadataUpdate};

pub fn hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub admin: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default)]
    pub avatar_prompt_seen: bool,
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
    pub files: std::sync::Arc<BTreeMap<String, u64>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct App {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication: Option<String>,
    pub build: String,
    pub updated: u64,
    #[serde(default = "listed")]
    pub listed: bool,
}
impl App {
    pub fn update_id(&self, deployment: &str) -> String {
        match &self.publication {
            Some(id) => format!("{deployment}.{id}"),
            None => format!("{deployment}.legacy.{}-{}", self.build, self.updated),
        }
    }
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
    pub deployment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visit: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(skip)]
    pub deployments: BTreeMap<String, deployments::Deployment>,
    #[serde(skip)]
    pub visits: BTreeMap<String, visits::Visit>,
    #[serde(skip)]
    pub server_builds: BTreeMap<String, crate::server_package::Job>,
    #[serde(skip)]
    pub groups: groups::Groups,
    #[serde(skip)]
    pub metadata: BTreeMap<String, GameMetadata>,
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
    // Ordered display credits only; never grants publishing or ownership rights.
    #[serde(default)]
    pub author_credits: BTreeMap<String, Vec<String>>,
    // The renamed catalog field deliberately makes pre-genesis binaries refuse
    // this state rather than interpret profiles as login email identities.
    #[serde(rename = "profiles", alias = "members")]
    pub members: BTreeMap<String, Member>,
    #[serde(default)]
    pub identities: identity::Identities,
    #[serde(default)]
    pub identity_version: u32,
    pub credentials: BTreeMap<String, Credential>,
    pub apps: BTreeMap<String, App>,
    // Admin hides survive publication, including updates already queued.
    #[serde(default)]
    pub admin_hidden_apps: BTreeSet<String>,
    pub builds: BTreeMap<String, Build>,
    pub launches: BTreeMap<String, Launch>,
    pub presence: BTreeMap<String, u64>,
    pub reactions: BTreeMap<String, BTreeMap<String, Reaction>>,
}
// Side tables preserve declarations across compatible binary rollbacks. The
// identity genesis is a deliberate lower bound: pre-genesis daemons cannot read
// the renamed profiles field and must never write this catalog again.
pub(crate) fn decode_catalog(c: &Connection, raw: &str) -> Result<Data> {
    let mut data: Data = serde_json::from_str(raw)?;
    ensure!(
        data.identity_version <= identity::VERSION,
        "unsupported identity state version; do not downgrade"
    );
    if data.identity_version > 0 {
        ensure!(
            data.members
                .keys()
                .all(|key| data.identities.profiles.contains_key(key)),
            "identity catalog is incomplete; restore a healthy backup rather than recreating accounts"
        );
    }
    let mut jobs = c.prepare("SELECT build_key,declaration FROM server_builds")?;
    for row in jobs.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        data.server_builds
            .insert(key, serde_json::from_str(&declaration)?);
    }
    data.metadata = metadata::read(c)?;
    let policy: String = c.query_row("SELECT data FROM private_groups WHERE id=1", [], |r| {
        r.get(0)
    })?;
    data.groups = serde_json::from_str(&policy)?;
    let raw: String = c.query_row("SELECT data FROM deployments WHERE id=1", [], |r| r.get(0))?;
    data.deployments = serde_json::from_str(&raw)?;
    let raw: String = c.query_row("SELECT data FROM visits WHERE id=1", [], |r| r.get(0))?;
    data.visits = serde_json::from_str(&raw)?;
    let mut statement = c.prepare("SELECT app,declaration FROM game_urls")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (app, declaration) = row?;
        data.game_urls
            .insert(app, serde_json::from_str(&declaration)?);
    }
    let mut portraits = c.prepare("SELECT build_key FROM portrait_builds")?;
    for key in portraits.query_map([], |row| row.get::<_, String>(0))? {
        if let Some(build) = data.builds.get_mut(&key?) {
            build.manifest.orientation = crate::manifest::Orientation::Portrait;
        }
    }
    let mut native_res = c.prepare("SELECT build_key FROM native_resolution_builds")?;
    for key in native_res.query_map([], |row| row.get::<_, String>(0))? {
        if let Some(build) = data.builds.get_mut(&key?) {
            build.manifest.resolution = crate::manifest::Resolution::Native;
        }
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
    let mut statement = c.prepare("SELECT build_key,declaration FROM dedicated_manifests")?;
    for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, declaration) = row?;
        if let Some(build) = data.builds.get_mut(&key) {
            let value: serde_json::Value = serde_json::from_str(&declaration)?;
            build.manifest.sessions = serde_json::from_value(value["sessions"].clone())?;
            build.manifest.server_connection =
                serde_json::from_value(value["server_connection"].clone())?;
        }
    }
    data.initialize_identities()?;
    Ok(data)
}
fn encode_catalog(tx: &Transaction<'_>, data: &mut Data) -> Result<String> {
    data.initialize_identities()?;
    data.identity_version = identity::VERSION;
    tx.execute(
        "UPDATE deployments SET data=?1 WHERE id=1",
        [serde_json::to_string(&data.deployments)?],
    )?;
    tx.execute(
        "UPDATE visits SET data=?1 WHERE id=1",
        [serde_json::to_string(&data.visits)?],
    )?;
    for (key, job) in &data.server_builds {
        tx.execute("INSERT INTO server_builds(build_key,declaration) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![key,serde_json::to_string(job)?])?;
    }
    let job_keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM server_builds")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in job_keys {
        if !data.server_builds.contains_key(&key) {
            tx.execute("DELETE FROM server_builds WHERE build_key=?1", [key])?;
        }
    }
    tx.execute(
        "UPDATE private_groups SET data=?1 WHERE id=1",
        [serde_json::to_string(&data.groups)?],
    )?;
    let mut value = serde_json::to_value(&*data)?;
    for (app, url) in &data.game_urls {
        tx.execute("INSERT INTO game_urls(app,declaration) VALUES(?1,?2) ON CONFLICT(app) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration",params![app,serde_json::to_string(url)?])?;
    }
    // Keep the strict legacy manifest readable by rollback binaries. Only
    // portrait builds need a record because landscape is the original layout.
    let mut portraits = Vec::new();
    let mut native_resolutions = Vec::new();
    for (key, build) in &data.builds {
        if build.manifest.orientation == crate::manifest::Orientation::Portrait {
            portraits.push(key);
        }
        if build.manifest.resolution == crate::manifest::Resolution::Native {
            native_resolutions.push(key);
        }
        if build.manifest.sessions.is_some() || build.manifest.server_connection.is_some() {
            let declaration = serde_json::json!({"sessions":build.manifest.sessions,"server_connection":build.manifest.server_connection});
            tx.execute("INSERT INTO dedicated_manifests(build_key,declaration) VALUES(?1,?2) ON CONFLICT(build_key) DO UPDATE SET declaration=excluded.declaration WHERE declaration!=excluded.declaration", params![key, declaration.to_string()])?;
        }
        for field in ["sessions", "server_connection", "orientation", "resolution"] {
            value["builds"][key]["manifest"]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
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
    let portraits = serde_json::to_string(&portraits)?;
    tx.execute(
        "DELETE FROM portrait_builds WHERE build_key NOT IN (SELECT value FROM json_each(?1))",
        [&portraits],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO portrait_builds SELECT value FROM json_each(?1)",
        [&portraits],
    )?;
    let native_resolutions = serde_json::to_string(&native_resolutions)?;
    tx.execute(
        "DELETE FROM native_resolution_builds WHERE build_key NOT IN (SELECT value FROM json_each(?1))",
        [&native_resolutions],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO native_resolution_builds SELECT value FROM json_each(?1)",
        [&native_resolutions],
    )?;
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
    let keys = {
        let mut stmt = tx.prepare("SELECT build_key FROM dedicated_manifests")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for key in keys {
        if !data.builds.contains_key(&key) {
            tx.execute("DELETE FROM dedicated_manifests WHERE build_key=?1", [key])?;
        }
    }
    Ok(serde_json::to_string(&value)?)
}
pub struct Database {
    pub connection: Mutex<Connection>,
    catalog: Mutex<Option<(u64, std::sync::Arc<Data>)>>,
}
impl Database {
    pub fn open(root: &Path) -> Result<Self> {
        identity::check_state(root)?;
        std::fs::create_dir_all(root)?;
        let mut c = Connection::open(root.join("platform.sqlite"))?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
          CREATE TABLE IF NOT EXISTS platform (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS saves (owner TEXT, app TEXT, slot TEXT, version INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(owner,app,slot));")?;
        crate::cloud::initialize(&c)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS native_v1 (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
            INSERT OR IGNORE INTO native_v1(id,data) VALUES(1,'{\"pairs\":{},\"devices\":{},\"requests\":{}}');")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS private_groups(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);")?;
        c.execute(
            "INSERT OR IGNORE INTO private_groups(id,data) VALUES(1,?1)",
            [serde_json::to_string(&groups::Groups::default())?],
        )?;
        metadata::initialize(&c)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS deployments(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL); INSERT OR IGNORE INTO deployments VALUES(1,'{}');")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS visits(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL); INSERT OR IGNORE INTO visits VALUES(1,'{}');")?;
        channels::initialize(&mut c)?;
        screenshots::initialize(&c)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS update_reactions(
            deployment TEXT NOT NULL, profile TEXT NOT NULL, publication TEXT NOT NULL, emoji TEXT NOT NULL,
            PRIMARY KEY(deployment,profile)) WITHOUT ROWID;")?;
        releases::initialize(&c)?;
        crate::notifications::initialize(&c)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS portrait_builds(build_key TEXT PRIMARY KEY);
          CREATE TABLE IF NOT EXISTS native_resolution_builds(build_key TEXT PRIMARY KEY);
          CREATE TABLE IF NOT EXISTS sdk_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS game_urls(app TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS server_builds(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS intent_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS runtime_manifests(build_key TEXT PRIMARY KEY, server_start TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS dedicated_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS voice_settings(profile TEXT PRIMARY KEY, revision INTEGER NOT NULL DEFAULT 1, data TEXT NOT NULL);")?;
        c.execute(
            "INSERT OR IGNORE INTO platform(id,data) VALUES(1,?1)",
            [serde_json::to_string(&Data::default())?],
        )?;
        activity::initialize(&mut c)?;
        member_link::initialize(&c)?;
        let db = Self {
            catalog: Mutex::new(None),
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
    pub fn catalog(&self) -> Result<std::sync::Arc<Data>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        self.cached_catalog(&tx)
    }
    pub fn read(&self) -> Result<Data> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let mut data = (*self.cached_catalog(&tx)?).clone();
        activity::load(&tx, &mut data)?;
        tx.commit()?;
        Ok(data)
    }
    pub(crate) fn cached_catalog(&self, tx: &Transaction<'_>) -> Result<std::sync::Arc<Data>> {
        let revision: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut cached = self.catalog.lock().unwrap();
        if cached.as_ref().is_none_or(|(v, _)| *v != revision) {
            let raw: String =
                tx.query_row("SELECT data FROM platform_catalog WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            let decoded = std::sync::Arc::new(decode_catalog(tx, &raw)?);
            if cached.as_ref().is_none_or(|(v, _)| *v < revision) {
                *cached = Some((revision, decoded.clone()));
            }
            return Ok(decoded);
        }
        Ok(cached.as_ref().unwrap().1.clone())
    }
    pub fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        self.edit_transaction(|data, _| f(data))
    }
    pub(crate) fn edit_transaction<T>(
        &self,
        f: impl FnOnce(&mut Data, &Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut data = (*self.cached_catalog(&tx)?).clone();
        activity::load(&tx, &mut data)?;
        let result = f(&mut data, &tx)?;
        tx.execute(
            "UPDATE platform SET data=?1 WHERE id=1",
            [encode_catalog(&tx, &mut data)?],
        )?;
        let revision: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        data.launches.clear();
        data.presence.clear();
        let mut cached = self.catalog.lock().unwrap();
        tx.commit()?;
        *cached = Some((revision, std::sync::Arc::new(data)));
        Ok(result)
    }
    pub fn voice_settings_load(&self, profile: &str) -> Result<(u64, crate::voice::AudioSettings)> {
        let c = self.connection.lock().unwrap();
        let row = c.query_row(
            "SELECT revision, data FROM voice_settings WHERE profile=?1",
            [profile],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        );
        match row {
            Ok((rev, raw)) => {
                let mut settings: crate::voice::AudioSettings =
                    serde_json::from_str(&raw).context("decode saved voice settings")?;
                settings.normalize();
                Ok((rev.max(1) as u64, settings))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Ok((1, crate::voice::AudioSettings::default()))
            }
            Err(error) => Err(error.into()),
        }
    }
    pub fn voice_settings_patch(
        &self,
        profile: &str,
        patch: &crate::voice::AudioSettingsPatch,
        allow_processing: bool,
    ) -> Result<(u64, crate::voice::AudioSettings, bool)> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (existing_rev, mut settings) = match tx.query_row(
            "SELECT revision, data FROM voice_settings WHERE profile=?1",
            [profile],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ) {
            Ok((rev, raw)) => {
                let mut parsed: crate::voice::AudioSettings =
                    serde_json::from_str(&raw).context("decode saved voice settings")?;
                parsed.normalize();
                (rev.max(1) as u64, parsed)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                (1, crate::voice::AudioSettings::default())
            }
            Err(error) => return Err(error.into()),
        };
        let changed = settings.apply_patch(patch, allow_processing)?;
        let rev = if changed {
            settings.normalize();
            let encoded = serde_json::to_string(&settings)?;
            let next: i64 = tx.query_row(
                "INSERT INTO voice_settings(profile, revision, data) VALUES(?1, 2, ?2)
                 ON CONFLICT(profile) DO UPDATE SET revision = revision + 1, data = excluded.data
                 RETURNING revision",
                rusqlite::params![profile, encoded],
                |r| r.get(0),
            )?;
            next.max(1) as u64
        } else {
            existing_rev
        };
        tx.commit()?;
        Ok((rev, settings, changed))
    }
    pub fn record_build_file(&self, key: &str, path: String, size: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut data = (*self.cached_catalog(&tx)?).clone();
        let build = data
            .builds
            .get_mut(key)
            .ok_or_else(|| crate::portal::fault(404, "create the build first"))?;
        std::sync::Arc::make_mut(&mut build.files).insert(path, size);
        let files = serde_json::to_string(&*build.files)?;
        tx.execute(
            "UPDATE platform_catalog SET data=json_set(data,?1,json(?2)) WHERE id=1",
            params![format!("$.builds.\"{key}\".files"), files],
        )?;
        let revision: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut cached = self.catalog.lock().unwrap();
        tx.commit()?;
        *cached = Some((revision, std::sync::Arc::new(data)));
        Ok(())
    }
    pub fn record_server_build_file(&self, key: &str, path: String, size: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut data = (*self.cached_catalog(&tx)?).clone();
        let job = data
            .server_builds
            .get_mut(key)
            .ok_or_else(|| crate::portal::fault(404, "create a build with server_recipe first"))?;
        job.files.insert(path, size);
        let declaration = serde_json::to_string(job)?;
        tx.execute(
            "UPDATE server_builds SET declaration=?2 WHERE build_key=?1",
            params![key, declaration],
        )?;
        let revision: u64 = tx.query_row(
            "SELECT revision FROM catalog_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut cached = self.catalog.lock().unwrap();
        tx.commit()?;
        *cached = Some((revision, std::sync::Arc::new(data)));
        Ok(())
    }
    pub fn native_devices(&self, email: &str) -> Result<serde_json::Value> {
        let c = self.connection.lock().unwrap();
        let raw: String = c.query_row("SELECT data FROM native_v1 WHERE id=1", [], |r| r.get(0))?;
        let native: crate::native::Data = serde_json::from_str(&raw)?;
        Ok(serde_json::json!({
            "devices": native
                .devices
                .iter()
                .filter(|(_, x)| x.email == email)
                .map(|(id, x)| serde_json::json!({"id": id, "created": x.created}))
                .collect::<Vec<_>>()
        }))
    }
    pub fn native_edit<T>(
        &self,
        f: impl FnOnce(&mut Data, &mut crate::native::Data) -> Result<T>,
    ) -> Result<T> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut data = (*self.cached_catalog(&tx)?).clone();
        activity::load(&tx, &mut data)?;
        let before_launches = data.launches.clone();
        let raw: String =
            tx.query_row("SELECT data FROM native_v1 WHERE id=1", [], |r| r.get(0))?;
        let mut native: crate::native::Data = serde_json::from_str(&raw)?;
        native.pairs.retain(|_, p| p.expires > now());
        native.requests.retain(|_, p| p.expires > now());
        let result = f(&mut data, &mut native)?;
        for (id, launch) in &data.launches {
            if before_launches.get(id).is_none_or(|old| {
                serde_json::to_value(old).ok() != serde_json::to_value(launch).ok()
            }) {
                tx.execute(
                    "INSERT INTO runtime_launches(id,data) VALUES(?1,?2)
                     ON CONFLICT(id) DO UPDATE SET data=excluded.data WHERE data!=excluded.data",
                    params![id, serde_json::to_string(launch)?],
                )?;
            }
        }
        let encoded = serde_json::to_string(&native)?;
        if encoded != raw {
            tx.execute("UPDATE native_v1 SET data=?1 WHERE id=1", [encoded])?;
        }
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
        let time = now();
        tx.execute(
            "DELETE FROM runtime_launches WHERE json_extract(data,'$.expires')<=?1",
            [time],
        )?;
        tx.execute("DELETE FROM runtime_presence WHERE seen+300<=?1", [time])?;
        let mut data = Data::default();
        activity::load(&tx, &mut data)?;
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
        member_link::expire(&tx, time)?;
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
        let email = d.invitation_key(&email)?;
        let avatar = d.members.get(&email).and_then(|m| m.avatar.clone());
        let avatar_prompt_seen = d.members.get(&email).is_some_and(|m| m.avatar_prompt_seen);
        d.members.insert(
            email,
            Member {
                name: name.into(),
                admin,
                enabled,
                avatar,
                avatar_prompt_seen,
            },
        );
        Ok(())
    })
}
pub fn credential(data: &mut Data, app: &str, role: &str, label: &str) -> Result<(String, String)> {
    ensure!(crate::config::slug(app), "invalid app id");
    ensure!(
        role == "publish"
            || role == "server"
            || (role == "metadata" && app == "all-games")
            || (role == "metadata-group" && data.groups.names.contains_key(app)),
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
        let email = d.member_key(&email)?;
        ensure!(d.profile_enabled(&email)&&d.members[&email].admin,"preview requires an existing enabled admin");
        d.require_app_group(&email, app)?;
        let selected=d.apps.get(app).context("unknown app")?;
        ensure!(!selected.listed,"operator preview only accepts hidden test games");
        let build=d.builds.get(&format!("{app}/{}",selected.build)).context("missing build")?;
        if let Some(sdk) = &build.manifest.sdk { sdk.admit(&crate::sdk_contract::with_peerjs(config.controller.turn.is_some(), config.controller.peerjs_port.is_some()))?; }
        ensure!(build.server.is_none() || build.manifest.server_start.is_some(),"operator preview requires a client-only or on-demand web game");
        if let Some(intent) = &intent { intent.validate(&build.manifest)?; }
        let id=secret()[..32].to_string();let token=secret();
        let host=crate::game_urls::host(d, app, &id, &config.controller.domain);
        d.launches.insert(id.clone(),Launch { deployment: None, visit: None,intent,app:app.into(),build:selected.build.clone(),email,guest:true,host:host.clone(),token_hash:hash(&token),exchanged:false,expires:now()+300,grant_expires:now()+120,seen:now(),lease_id:None,lease_token:None});
        Ok(serde_json::json!({"launch_id":id,"url":format!("https://{host}/_infuser/launch#infuser={token}")}))
    })
}
