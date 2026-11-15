//! Versioned, atomic JSON and binary checkpoints. Games own serialization and conflict choices.
use crate::{
    platform::{Database, hash},
    store::secret,
};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const SNAPSHOT_BYTES: u64 = 1024 * 1024;
pub const GROUP_BYTES: u64 = 16 * 1024 * 1024;
pub const HISTORY: usize = 16;
pub const CONFLICTS: usize = 8;
#[derive(Debug)]
pub struct Fault {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message)
    }
}
impl std::error::Error for Fault {}
pub fn fault(status: u16, code: &'static str, message: &'static str) -> anyhow::Error {
    Fault {
        status,
        code,
        message,
    }
    .into()
}
pub fn capabilities(budget_mb: u64) -> Value {
    json!({"version":1,"experimental":true,"formats":["json","binary"],"binary_encoding":"base64","snapshot_size_basis":"serialized_files_json","max_groups":8,"max_files_per_group":16,"max_snapshot_bytes":SNAPSHOT_BYTES,"max_group_bytes":GROUP_BYTES,"history_target":HISTORY,"max_unresolved_conflicts":CONFLICTS,"installation_budget_bytes":budget_mb*1024*1024,"guest_lifetime":"launch","offline_durability":false})
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub groups: Vec<Group>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub files: Vec<File>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub format: String,
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 160
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with("_infuser")
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
}
impl Declaration {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.groups.is_empty() && self.groups.len() <= 8,
            "declare 1–8 save groups"
        );
        let mut groups = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for group in &self.groups {
            ensure!(
                crate::config::slug(&group.id) && groups.insert(&group.id),
                "invalid or duplicate save group"
            );
            ensure!(
                !group.label.trim().is_empty()
                    && group.label.chars().count() <= 80
                    && !group.label.chars().any(char::is_control),
                "save group needs a short label"
            );
            ensure!(
                !group.files.is_empty() && group.files.len() <= 16,
                "declare 1–16 files per save group"
            );
            for file in &group.files {
                ensure!(
                    matches!(file.format.as_str(), "json" | "binary")
                        && valid_path(&file.path)
                        && paths.insert(&file.path),
                    "save files need unique, safe relative paths and format=json or binary"
                );
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Commit {
    pub commit_id: String,
    #[serde(deserialize_with = "required_revision")]
    pub expected_revision: Option<String>,
    pub files: BTreeMap<String, Value>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub resolve_candidate: Option<String>,
}
fn required_revision<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    /// Missing entries are JSON, including snapshots written before binary support.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub formats: BTreeMap<String, String>,
    pub revision: Option<String>,
    pub files: BTreeMap<String, Value>,
    pub summary: String,
    pub build_id: Option<String>,
    pub created_at: Option<u64>,
    pub parents: Vec<String>,
    pub resolves: Vec<String>,
}
impl Snapshot {
    fn empty() -> Self {
        Self {
            revision: None,
            formats: BTreeMap::new(),
            files: BTreeMap::new(),
            summary: String::new(),
            build_id: None,
            created_at: None,
            parents: vec![],
            resolves: vec![],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Saved {
        revision: String,
    },
    Conflict {
        current_revision: String,
        candidate_revision: String,
    },
}
impl Outcome {
    pub fn status(&self) -> u16 {
        match self {
            Self::Saved { .. } => 200,
            Self::Conflict { .. } => 409,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub owner: &'a str,
    pub app: &'a str,
    pub group: &'a str,
}
pub struct WriteOptions<'a> {
    pub build: &'a str,
    pub budget_bytes: u64,
    pub disk_reserve_bytes: u64,
    pub time: u64,
}
fn key(scope: Scope<'_>) -> [&str; 3] {
    [scope.owner, scope.app, scope.group]
}
pub fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS cloud_heads(owner TEXT NOT NULL, app TEXT NOT NULL, group_id TEXT NOT NULL, revision TEXT NOT NULL, PRIMARY KEY(owner,app,group_id));
    CREATE TABLE IF NOT EXISTS cloud_revisions(owner TEXT NOT NULL,app TEXT NOT NULL,group_id TEXT NOT NULL,revision TEXT NOT NULL,body TEXT NOT NULL,bytes INTEGER NOT NULL,PRIMARY KEY(owner,app,group_id,revision));
    CREATE TABLE IF NOT EXISTS cloud_receipts(owner TEXT NOT NULL,app TEXT NOT NULL,group_id TEXT NOT NULL,commit_id TEXT NOT NULL,fingerprint TEXT NOT NULL,outcome TEXT NOT NULL,bytes INTEGER NOT NULL,PRIMARY KEY(owner,app,group_id,commit_id));
    CREATE TABLE IF NOT EXISTS cloud_conflicts(owner TEXT NOT NULL,app TEXT NOT NULL,group_id TEXT NOT NULL,candidate TEXT NOT NULL,head TEXT NOT NULL,PRIMARY KEY(owner,app,group_id,candidate));
    CREATE TABLE IF NOT EXISTS cloud_usage(id INTEGER PRIMARY KEY CHECK(id=1),bytes INTEGER NOT NULL CHECK(bytes>=0));
    INSERT OR IGNORE INTO cloud_usage(id,bytes) VALUES(1,0);")?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS cloud_manifests(build_key TEXT PRIMARY KEY, declaration TEXT NOT NULL);")?;
    Ok(())
}
fn head(c: &Connection, scope: Scope<'_>) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT revision FROM cloud_heads WHERE owner=?1 AND app=?2 AND group_id=?3",
        key(scope),
        |r| r.get(0),
    )
    .optional()?)
}
fn snapshot(c: &Connection, scope: Scope<'_>, revision: &str) -> Result<Snapshot> {
    let body:Option<String>=c.query_row("SELECT body FROM cloud_revisions WHERE owner=?1 AND app=?2 AND group_id=?3 AND revision=?4",params![scope.owner,scope.app,scope.group,revision],|r|r.get(0)).optional()?;
    Ok(serde_json::from_str(&body.ok_or_else(|| {
        fault(
            404,
            "revision_unavailable",
            "checkpoint is not available in this save group",
        )
    })?)?)
}
fn conflicts(c: &Connection, scope: Scope<'_>) -> Result<Vec<(String, String)>> {
    let mut statement=c.prepare("SELECT candidate,head FROM cloud_conflicts WHERE owner=?1 AND app=?2 AND group_id=?3 ORDER BY rowid")?;
    Ok(statement
        .query_map(key(scope), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
impl Database {
    pub fn cloud_load(&self, scope: Scope<'_>, revision: Option<&str>) -> Result<Value> {
        // One read transaction gives a coherent head, snapshot, and conflicts even
        // when another process commits through a separate SQLite connection.
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let selected = revision.map(str::to_owned).or(head(&tx, scope)?);
        let mut value = serde_json::to_value(match selected {
            Some(id) => snapshot(&tx, scope, &id)?,
            None => Snapshot::empty(),
        })?;
        value["conflicts"]=json!(conflicts(&tx,scope)?.into_iter().map(|(candidate,head)|json!({"candidate_revision":candidate,"head_at_conflict":head})).collect::<Vec<_>>());
        tx.commit()?;
        Ok(value)
    }
    pub fn cloud_history(&self, scope: Scope<'_>) -> Result<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let revisions = {
            let mut stmt=tx.prepare("SELECT body FROM cloud_revisions WHERE owner=?1 AND app=?2 AND group_id=?3 ORDER BY rowid DESC")?;
            let rows = stmt
                .query_map(key(scope), |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|body| {
                    let mut v: Value = serde_json::from_str(&body)?;
                    v.as_object_mut().unwrap().remove("files");
                    Ok(v)
                })
                .collect::<Result<Vec<_>>>()?
        };
        let value = json!({"head_revision":head(&tx,scope)?,"revisions":revisions});
        tx.commit()?;
        Ok(value)
    }
    pub fn cloud_commit(
        &self,
        scope: Scope<'_>,
        group: &Group,
        request: &Commit,
        options: WriteOptions<'_>,
    ) -> Result<Outcome> {
        let WriteOptions {
            build,
            budget_bytes,
            disk_reserve_bytes,
            time,
        } = options;
        ensure!(
            (16..=96).contains(&request.commit_id.len())
                && request
                    .commit_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            fault(
                400,
                "invalid_commit_id",
                "use a unique 16–96 character commit ID"
            )
        );
        let fingerprint = hash(&serde_json::to_string(request)?);
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let receipt:Option<(String,String)>=tx.query_row("SELECT fingerprint,outcome FROM cloud_receipts WHERE owner=?1 AND app=?2 AND group_id=?3 AND commit_id=?4",params![scope.owner,scope.app,scope.group,request.commit_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((prior, outcome)) = receipt {
            ensure!(
                prior == fingerprint,
                fault(
                    409,
                    "commit_id_reused",
                    "this commit ID was already used with different content"
                )
            );
            return Ok(serde_json::from_str(&outcome)?);
        }
        if let Some(path) = tx.path() {
            let disk = rustix::fs::statvfs(path)?;
            ensure!(
                disk.f_bavail * disk.f_frsize >= disk_reserve_bytes + 2 * SNAPSHOT_BYTES + 65536,
                fault(
                    507,
                    "disk_reserve",
                    "host disk reserve is low; keep the pending checkpoint and retry later"
                )
            );
        }
        ensure!(
            request.summary.chars().count() <= 80 && !request.summary.chars().any(char::is_control),
            fault(
                400,
                "invalid_summary",
                "use a plain-text summary of at most 80 characters"
            )
        );
        ensure!(
            request
                .files
                .keys()
                .all(|path| group.files.iter().any(|f| f.path == *path)),
            fault(
                400,
                "undeclared_file",
                "checkpoint contains a file not declared by this build"
            )
        );
        ensure!(
            serde_json::to_vec(&request.files)?.len() as u64 <= SNAPSHOT_BYTES,
            fault(
                413,
                "snapshot_too_large",
                "checkpoint exceeds the advertised size limit"
            )
        );
        for file in &group.files {
            if file.format == "binary"
                && let Some(value) = request.files.get(&file.path)
            {
                let valid = value
                    .as_object()
                    .filter(|object| object.len() == 1)
                    .and_then(|object| object.get("base64"))
                    .and_then(Value::as_str)
                    .is_some_and(|encoded| STANDARD.decode(encoded).is_ok());
                ensure!(
                    valid,
                    fault(
                        400,
                        "invalid_binary_file",
                        "binary files must be an object containing only canonical padded base64 in the base64 field"
                    )
                );
            }
        }
        let current = head(&tx, scope)?;
        if let Some(base) = &request.expected_revision {
            let prior = snapshot(&tx, scope, base)?;
            ensure!(
                prior
                    .files
                    .keys()
                    .all(|path| group.files.iter().any(|f| f.path == *path
                        && f.format
                            == prior
                                .formats
                                .get(path)
                                .map(String::as_str)
                                .unwrap_or("json"))),
                fault(
                    409,
                    "unsupported_save_files",
                    "this build cannot write all files in its base checkpoint; use a compatible build"
                )
            );
        }
        let mut parents = request
            .expected_revision
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        let mut resolves = Vec::new();
        if let Some(candidate) = &request.resolve_candidate {
            ensure!(
                conflicts(&tx, scope)?.iter().any(|(id, _)| id == candidate),
                fault(
                    409,
                    "conflict_already_resolved",
                    "reload the save group before resolving this conflict"
                )
            );
            let previous = snapshot(&tx, scope, candidate)?;
            resolves = previous.resolves;
            resolves.push(candidate.clone());
            resolves.sort();
            resolves.dedup();
            if !parents.contains(candidate) {
                parents.push(candidate.clone());
            }
        }
        let won = current == request.expected_revision;
        ensure!(
            won || conflicts(&tx, scope)?.len() < CONFLICTS,
            fault(
                507,
                "conflict_limit",
                "resolve existing save conflicts before creating another candidate"
            )
        );
        let revision = secret()[..32].to_owned();
        let record = Snapshot {
            formats: group
                .files
                .iter()
                .filter(|file| file.format == "binary" && request.files.contains_key(&file.path))
                .map(|file| (file.path.clone(), file.format.clone()))
                .collect(),
            revision: Some(revision.clone()),
            files: request.files.clone(),
            summary: request.summary.clone(),
            build_id: Some(build.into()),
            created_at: Some(time),
            parents,
            resolves: resolves.clone(),
        };
        let body = serde_json::to_string(&record)?;
        let bytes = body.len() as u64 + 256;
        tx.execute("INSERT INTO cloud_revisions(owner,app,group_id,revision,body,bytes) VALUES(?1,?2,?3,?4,?5,?6)",params![scope.owner,scope.app,scope.group,revision,body,bytes])?;
        let outcome = if won {
            tx.execute("INSERT INTO cloud_heads(owner,app,group_id,revision) VALUES(?1,?2,?3,?4) ON CONFLICT(owner,app,group_id) DO UPDATE SET revision=excluded.revision",params![scope.owner,scope.app,scope.group,revision])?;
            for candidate in &resolves {
                tx.execute("DELETE FROM cloud_conflicts WHERE owner=?1 AND app=?2 AND group_id=?3 AND candidate=?4",params![scope.owner,scope.app,scope.group,candidate])?;
            }
            Outcome::Saved {
                revision: revision.clone(),
            }
        } else {
            let current = current.clone().ok_or_else(|| {
                fault(
                    409,
                    "invalid_base",
                    "reload the save group before committing",
                )
            })?;
            tx.execute("INSERT INTO cloud_conflicts(owner,app,group_id,candidate,head) VALUES(?1,?2,?3,?4,?5)",params![scope.owner,scope.app,scope.group,revision,current])?;
            Outcome::Conflict {
                current_revision: current,
                candidate_revision: revision.clone(),
            }
        };
        let result = serde_json::to_string(&outcome)?;
        let receipt_bytes =
            result.len() as u64 + fingerprint.len() as u64 + request.commit_id.len() as u64 + 256;
        tx.execute("INSERT INTO cloud_receipts(owner,app,group_id,commit_id,fingerprint,outcome,bytes) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![scope.owner,scope.app,scope.group,request.commit_id,fingerprint,result,receipt_bytes])?;
        let mut total: u64 =
            tx.query_row("SELECT bytes FROM cloud_usage WHERE id=1", [], |r| r.get(0))?;
        total += bytes + receipt_bytes;
        let mut protected = BTreeSet::from([revision]);
        protected.extend(current);
        for (candidate, head) in conflicts(&tx, scope)? {
            let record = snapshot(&tx, scope, &candidate)?;
            protected.insert(candidate);
            protected.insert(head);
            protected.extend(record.parents);
            protected.extend(record.resolves);
        }
        let records = {
            let mut stmt=tx.prepare("SELECT revision,bytes FROM cloud_revisions WHERE owner=?1 AND app=?2 AND group_id=?3 ORDER BY rowid DESC")?;
            stmt.query_map(key(scope), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        // Resolving a long-lived conflict must not immediately discard the
        // alternatives. Keep them while that resolution is in recent history.
        for (id, _) in records.iter().take(HISTORY) {
            let recent = snapshot(&tx, scope, id)?;
            if !recent.resolves.is_empty() {
                protected.extend(recent.parents);
                protected.extend(recent.resolves);
            }
        }
        let mut group_bytes: u64 = records.iter().map(|(_, bytes)| *bytes).sum();
        for (index, (id, size)) in records.iter().enumerate().rev() {
            if !protected.contains(id)
                && (index >= HISTORY || group_bytes > GROUP_BYTES || total > budget_bytes)
            {
                tx.execute("DELETE FROM cloud_revisions WHERE owner=?1 AND app=?2 AND group_id=?3 AND revision=?4",params![scope.owner,scope.app,scope.group,id])?;
                group_bytes -= size;
                total -= size;
            }
        }
        ensure!(
            group_bytes <= GROUP_BYTES && total <= budget_bytes,
            fault(
                507,
                "storage_full",
                "save storage is full; keep the pending checkpoint and retry after space is available"
            )
        );
        tx.execute("UPDATE cloud_usage SET bytes=?1 WHERE id=1", [total])?;
        tx.commit()?;
        Ok(outcome)
    }
}
pub fn delete_owner(tx: &Transaction<'_>, owner: &str) -> Result<()> {
    let bytes:u64=tx.query_row("SELECT (SELECT COALESCE(SUM(bytes),0) FROM cloud_revisions WHERE owner=?1)+(SELECT COALESCE(SUM(bytes),0) FROM cloud_receipts WHERE owner=?1)",[owner],|r|r.get(0))?;
    for table in [
        "cloud_heads",
        "cloud_revisions",
        "cloud_receipts",
        "cloud_conflicts",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE owner=?1"), [owner])?;
    }
    tx.execute("UPDATE cloud_usage SET bytes=bytes-?1 WHERE id=1", [bytes])?;
    Ok(())
}
