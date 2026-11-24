//! Loss-tolerant, short-lived operational diagnostics. No catalog writes.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DAY: i64 = 86_400_000;
pub fn ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn directory(s: &crate::config::Settings) -> PathBuf {
    s.diagnostics_dir.clone().unwrap_or_else(|| {
        let mut name = s.state_dir.file_name().unwrap_or_default().to_os_string();
        name.push("-diagnostics");
        s.state_dir.with_file_name(name)
    })
}
#[derive(Default, Clone, Serialize)]
pub struct ContextFields {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub build: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub launch: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub attempt: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub visit: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub guest: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inspecting: bool,
}
#[derive(Serialize)]
pub struct Event {
    pub at: i64,
    pub received_at: i64,
    pub src: String,
    pub kind: String,
    pub level: String,
    #[serde(flatten)]
    pub context: ContextFields,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub page: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    pub data: Value,
    #[serde(skip)]
    pub author_visible: bool,
}
impl Event {
    pub fn server(kind: &str, context: ContextFields, data: Value, author_visible: bool) -> Self {
        let at = ms();
        Self {
            at,
            received_at: at,
            src: "server".into(),
            kind: kind.into(),
            level: "info".into(),
            context,
            page: String::new(),
            seq: None,
            data,
            author_visible,
        }
    }
}
type Delivery = (Vec<Event>, Option<tokio::sync::oneshot::Sender<bool>>);
pub struct Timeline {
    state_dir: PathBuf,
    path: PathBuf,
    sender: OnceLock<mpsc::SyncSender<Delivery>>,
    lost: AtomicU64,
}
impl Timeline {
    pub fn new(settings: &crate::config::Settings) -> Self {
        Self {
            state_dir: settings.state_dir.clone(),
            path: directory(settings).join("events.sqlite"),
            sender: OnceLock::new(),
            lost: AtomicU64::new(0),
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn record(&self, event: Event) {
        self.enqueue(vec![event], None);
    }
    pub async fn submit(&self, batch: Vec<Event>) -> bool {
        if batch.is_empty() {
            return true;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.enqueue(batch, Some(tx))
            && tokio::time::timeout(Duration::from_secs(3), rx)
                .await
                .is_ok_and(|r| r.unwrap_or(false))
    }
    fn enqueue(
        &self,
        mut batch: Vec<Event>,
        ack: Option<tokio::sync::oneshot::Sender<bool>>,
    ) -> bool {
        if batch.is_empty() {
            return true;
        }
        let lost = self.lost.swap(0, Ordering::Relaxed);
        let count = batch.len() as u64;
        if lost > 0 {
            batch.push(Event::server(
                "collector.dropped",
                Default::default(),
                json!({"count":lost}),
                false,
            ));
        }
        let tx = self.sender.get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel(32);
            let path = self.path.clone();
            let state = self.state_dir.clone();
            if let Err(e) = std::thread::Builder::new()
                .name("diagnostics".into())
                .spawn(move || writer(&path, &state, rx))
            {
                tracing::warn!(error=%e,"Unable to start diagnostic writer");
            }
            tx
        });
        if tx.try_send((batch, ack)).is_err() {
            self.lost.fetch_add(count + lost, Ordering::Relaxed);
            false
        } else {
            true
        }
    }
}
fn open(path: &Path) -> Result<Connection> {
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(path.parent().context("missing diagnostics directory")?)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_millis(250))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=128; PRAGMA journal_size_limit=524288; PRAGMA max_page_count=131072;
        CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, at INTEGER NOT NULL, app TEXT NOT NULL,profile TEXT NOT NULL, dedup TEXT UNIQUE, visible INTEGER NOT NULL, body TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS events_at ON events(at,id);
        CREATE INDEX IF NOT EXISTS events_app ON events(app,at);
        CREATE INDEX IF NOT EXISTS events_profile ON events(profile,at);
        CREATE INDEX IF NOT EXISTS lifecycle_edges ON events(at,id) WHERE app='' AND profile='' AND json_extract(body,'$.kind') IN ('controller.stop','controller.start');
        CREATE INDEX IF NOT EXISTS voice_sessions ON events(profile,json_extract(body,'$.data.session')) WHERE json_extract(body,'$.kind')='voice.grant';
        CREATE INDEX IF NOT EXISTS peer_epochs ON events(json_extract(body,'$.launch'),json_extract(body,'$.data.room')) WHERE json_extract(body,'$.kind')='mp.room';")?;
    Ok(c)
}
fn maintain(c: &Connection) -> Result<()> {
    c.execute("DELETE FROM events WHERE at < ?1", [ms() - 2 * DAY])?;
    let used:i64=c.query_row("SELECT (page_count-freelist_count)*page_size FROM pragma_page_count,pragma_freelist_count,pragma_page_size",[],|r|r.get(0))?;
    if used > 496 * 1024 * 1024 {
        c.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY at,id LIMIT (SELECT max(1,count(*)/4) FROM events))",[])?;
    }
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    Ok(())
}
fn import_deploys(c: &Connection, state: &Path) -> Result<()> {
    for outcome in crate::deploy_outcomes::view(state).1 {
        let at = (outcome.at as i64).saturating_mul(1000);
        if at < ms() - 2 * DAY {
            continue;
        }
        let mut e = Event::server(
            "deploy.outcome",
            Default::default(),
            json!({"release":outcome.release,"result":outcome.result}),
            false,
        );
        e.at = at;
        let key = format!("deploy/{at}/{}/{}", outcome.release, e.data["result"]);
        c.execute("INSERT OR IGNORE INTO events(at,app,profile,dedup,visible,body) VALUES(?1,'','',?2,0,?3)",params![at,key,serde_json::to_string(&e)?])?;
    }
    Ok(())
}
fn writer(path: &Path, state: &Path, rx: mpsc::Receiver<Delivery>) {
    let mut db = None;
    let mut maintenance = Instant::now();
    let mut window = Instant::now();
    let mut counts = HashMap::<String, usize>::new();
    loop {
        let (batch, ack) = match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(b) => b,
            Err(mpsc::RecvTimeoutError::Timeout) => (vec![], None),
            Err(_) => break,
        };
        let result = (|| -> Result<()> {
            if db.is_none() {
                db = Some(open(path)?);
            }
            let c = db.as_mut().unwrap();
            if Instant::now() >= maintenance {
                maintain(c)?;
                import_deploys(c, state)?;
                maintenance = Instant::now() + Duration::from_secs(30);
            }
            if window.elapsed() >= Duration::from_secs(60) {
                counts.clear();
                window = Instant::now();
            }
            if batch.is_empty() {
                return Ok(());
            }
            // A long-running reader can pin WAL pages. Stop adding frames until it releases them.
            if fs::metadata(path.with_extension("sqlite-wal"))
                .is_ok_and(|m| m.len() > 16 * 1024 * 1024)
            {
                maintain(c)?;
                ensure!(
                    fs::metadata(path.with_extension("sqlite-wal"))
                        .map(|m| m.len())
                        .unwrap_or(0)
                        < 16 * 1024 * 1024,
                    "diagnostic WAL busy"
                );
            }
            let tx = c.transaction()?;
            for mut e in batch {
                if e.kind == "runtime.transition"
                    && matches!(e.data["state"].as_str(), Some("sleeping" | "failed"))
                    && let Ok(reports) = crate::docker::retained_at(state, &e.context.app)
                    && let Some(report) = reports.iter().find(|r| r["attempt"] == e.context.attempt)
                {
                    for key in ["exit_code", "oom_killed", "health"] {
                        e.data[key] = report[key].clone();
                    }
                    if let Some(build) = report["build_id"].as_str() {
                        e.context.build = build.into();
                    }
                }
                if e.src != "server" {
                    if let Some(version) = e.data["version"].as_str()
                        && version != crate::portal::client_version()
                        && !tx.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE app='' AND profile='' AND json_extract(body,'$.kind') IN ('controller.stop','controller.start') AND json_extract(body,'$.data.client_version')=?)", [version], |r|r.get::<_,bool>(0))?
                        && let Some(fields) = e.data.as_object_mut() {
                        fields.remove("version");
                    }
                    // Client refs are hints. Only join them to a grant or membership issued
                    // for this profile/launch; never retain arbitrary client-supplied keys.
                    for (key, owner, sql) in [
                        (
                            "session",
                            &e.context.profile,
                            "SELECT EXISTS(SELECT 1 FROM events WHERE json_extract(body,'$.kind')='voice.grant' AND profile=? AND json_extract(body,'$.data.session')=?)",
                        ),
                        (
                            "room",
                            &e.context.launch,
                            "SELECT EXISTS(SELECT 1 FROM events WHERE json_extract(body,'$.kind')='mp.room' AND json_extract(body,'$.launch')=? AND json_extract(body,'$.data.room')=?)",
                        ),
                    ] {
                        if let Some(value) = e.data[key].as_str()
                            && (owner.is_empty()
                                || !tx.query_row(sql, params![owner, value], |r| {
                                    r.get::<_, bool>(0)
                                })?)
                            && let Some(fields) = e.data.as_object_mut()
                        {
                            fields.remove(key);
                        }
                    }
                    let n = counts.entry(e.context.profile.clone()).or_default();
                    *n += 1;
                    if *n > 181 {
                        continue;
                    }
                    if *n == 181 {
                        e = Event::server(
                            "collector.throttled",
                            ContextFields {
                                profile: e.context.profile,
                                ..Default::default()
                            },
                            json!({}),
                            false,
                        );
                    }
                }
                let key = e
                    .seq
                    .map(|s| format!("{}/{}/{}/{s}", e.context.profile, e.context.launch, e.page));
                tx.execute("INSERT INTO events(at,app,profile,dedup,visible,body) VALUES(?,?,?,?,?,?)
                    ON CONFLICT(dedup) DO UPDATE SET body=json_patch(events.body,json_object('data',json_object('count',json_extract(excluded.body,'$.data.count'),'duration_ms',json_extract(excluded.body,'$.data.duration_ms'))))
                    WHERE json_extract(events.body,'$.kind') IN ('fetch.fail','js.error','js.rejection','console.error','resource.error')
                    AND json_extract(events.body,'$.kind')=json_extract(excluded.body,'$.kind')
                    AND json_extract(excluded.body,'$.data.count')>json_extract(events.body,'$.data.count')",
                    params![e.at,e.context.app,e.context.profile,key,e.author_visible,serde_json::to_string(&e)?])?;
            }
            tx.commit()?;
            Ok(())
        })();
        if let Some(ack) = ack {
            let _ = ack.send(result.is_ok());
        }
        if let Err(error) = result {
            tracing::warn!(%error,"Diagnostics unavailable");
            db = None;
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
pub fn read(
    path: &Path,
    filter: &Filter,
    app: Option<&str>,
    profile: Option<&str>,
    publisher: bool,
) -> Result<Value> {
    read_at(path, filter, app, profile, publisher, ms())
}
/// Adjacent lifecycle edges only. An unmatched stop (or two starts) is not a window.
fn lifecycle_windows(c: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = c.prepare("SELECT at,json_extract(body,'$.kind') FROM events WHERE app='' AND profile='' AND json_extract(body,'$.kind') IN ('controller.stop','controller.start') ORDER BY at,id")?;
    let mut edges = stmt.query([])?;
    let mut stop = None;
    let mut windows = Vec::new();
    while let Some(edge) = edges.next()? {
        let at = edge.get(0)?;
        if edge.get::<_, String>(1)? == "controller.stop" {
            stop = Some(at);
        } else if let Some(start) = stop.take()
            && start < at
        {
            windows.push((start, at));
        }
    }
    Ok(windows)
}
pub(crate) fn incident_windows(path: &Path, since: i64, until: i64) -> Result<Vec<(i64, i64)>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(lifecycle_windows(&conn)?
        .into_iter()
        .filter(|&(a, b)| a <= until && since < b)
        .collect())
}
pub(crate) fn read_at(
    path: &Path,
    filter: &Filter,
    app: Option<&str>,
    profile: Option<&str>,
    publisher: bool,
    now: i64,
) -> Result<Value> {
    let limit = filter.limit.unwrap_or(200);
    ensure!((1..=500).contains(&limit), "limit must be 1–500");
    let (cursor_at, cursor_id) = match &filter.cursor {
        Some(v) => {
            let (a, b) = v.split_once(':').context("invalid cursor")?;
            (a.parse::<i64>()?, b.parse::<i64>()?)
        }
        None => (0, 0),
    };
    let floor = now - 2 * DAY;
    let since = filter.since_ms.unwrap_or(floor).max(floor);
    let until = filter.until_ms.unwrap_or(now);
    if !path.exists() {
        return Ok(json!({"events":[],"next":null}));
    }
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = c.prepare(
        "SELECT id, at, body, strftime('%Y-%m-%dT%H:%M:%fZ', at/1000.0, 'unixepoch')
         FROM events WHERE at BETWEEN ?1 AND ?2 AND (at,id) > (?3,?4)
           AND (?5 IS NULL OR app = ?5)
           AND (?6 IS NULL OR profile IN (?6, ''))
           AND (?7 = 0 OR visible = 1)
         ORDER BY at,id LIMIT ?8",
    )?;
    let mut rows = stmt.query(params![
        since,
        until,
        cursor_at,
        cursor_id,
        app,
        profile,
        publisher,
        limit + 1
    ])?;
    let mut events = vec![];
    let mut cursor = None;
    let mut more = false;
    let mut windows = None;
    while let Some(row) = rows.next()? {
        if events.len() == limit {
            more = true;
            break;
        }
        let mut v: Value = serde_json::from_str(&row.get::<_, String>(2)?)?;
        v["utc"] = row.get::<_, String>(3)?.into();
        if v["kind"] == "fetch.fail" {
            if windows.is_none() {
                windows = Some(lifecycle_windows(&c)?);
            }
            let at: i64 = row.get(1)?;
            let windows = windows.as_ref().unwrap();
            if let Some(&(stop, start)) =
                windows.get(windows.partition_point(|&(_, end)| end <= at))
                && stop <= at
            {
                v["data"]["deploy_restart"] = true.into();
                if !publisher {
                    v["data"]["stop_ms"] = stop.into();
                    v["data"]["start_ms"] = start.into();
                }
            }
        }
        if publisher {
            for key in ["profile", "group", "visit", "guest", "inspecting", "page"] {
                v.as_object_mut().unwrap().remove(key);
            }
            if let Some(d) = v["data"].as_object_mut() {
                d.retain(|k, _| {
                    matches!(
                        k.as_str(),
                        "fingerprint"
                            | "name"
                            | "outcome"
                            | "reason"
                            | "stage"
                            | "stages_ms"
                            | "prev"
                            | "held_ms"
                            | "down_ms"
                            | "flaps"
                            | "bytes"
                            | "cached_ratio"
                            | "largest_ms"
                            | "queued_reason"
                            | "line"
                            | "col"
                            | "class"
                            | "status"
                            | "state"
                            | "count"
                            | "duration_ms"
                            | "request"
                            | "route"
                            | "method"
                            | "exit_code"
                            | "oom_killed"
                            | "deploy_restart"
                            | "health"
                    )
                });
            }
        }
        events.push(v);
        cursor = Some(format!(
            "{}:{}",
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(0)?
        ));
    }
    Ok(json!({"events":events,"next":if more {cursor}else{None}}))
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
