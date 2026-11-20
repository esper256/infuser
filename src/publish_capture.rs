//! Private, bounded publication fixtures. Never an audit log or a live replay queue.
use crate::config::Settings;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, fs, io::Write, os::unix::fs::PermissionsExt, path::Path, time::Duration,
};

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub route: String,
    pub app: String,
    pub group: String,
    pub headers: BTreeMap<String, String>,
}
fn digest(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn connect(settings: &Settings) -> Result<Connection> {
    let dir = settings.state_dir.join("publish-captures");
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    let db = dir.join("requests.sqlite");
    let connection = Connection::open(&db)?;
    fs::set_permissions(db, fs::Permissions::from_mode(0o600))?;
    connection.busy_timeout(Duration::from_secs(1))?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS bodies(sha256 TEXT PRIMARY KEY, body BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS requests(id INTEGER PRIMARY KEY, at INTEGER NOT NULL, controller_build TEXT NOT NULL, request TEXT NOT NULL, body_sha256 TEXT NOT NULL REFERENCES bodies(sha256), status INTEGER);
        CREATE TABLE IF NOT EXISTS capture_usage(id INTEGER PRIMARY KEY CHECK(id=1), bytes INTEGER NOT NULL, skipped INTEGER NOT NULL);
        INSERT OR IGNORE INTO capture_usage VALUES(1,0,0);")?;
    Ok(connection)
}
/// Record before handling, so interruption leaves an explicit unknown outcome.
pub fn begin(settings: &Settings, request: &Request, body: &[u8]) -> Result<Option<i64>> {
    begin_reader(
        settings,
        request,
        &mut std::io::Cursor::new(body),
        body.len() as u64,
        digest(body),
    )
}
/// The upload has already been spooled and hashed; copy it into SQLite in chunks.
pub fn begin_file(
    settings: &Settings,
    request: &Request,
    path: &Path,
    bytes: u64,
    sha256: &str,
) -> Result<Option<i64>> {
    begin_reader(
        settings,
        request,
        &mut fs::File::open(path)?,
        bytes,
        sha256.to_owned(),
    )
}
fn begin_reader(
    settings: &Settings,
    request: &Request,
    input: &mut impl std::io::Read,
    bytes: u64,
    hash: String,
) -> Result<Option<i64>> {
    if settings.publish_capture_mb == 0 {
        return Ok(None);
    }
    let budget = settings
        .publish_capture_mb
        .checked_mul(1024 * 1024)
        .context("capture budget overflow")?;
    let mut c = connect(settings)?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM bodies WHERE sha256=?1)",
        [&hash],
        |r| r.get(0),
    )?;
    let request = serde_json::to_string(request)?;
    let cost = request.len() as u64 + 512 + if exists { 0 } else { bytes + 256 };
    let used: u64 = tx.query_row("SELECT bytes FROM capture_usage WHERE id=1", [], |r| {
        r.get(0)
    })?;
    let disk = rustix::fs::statvfs(&settings.state_dir)?;
    let reserve = settings.disk_reserve_mb.saturating_mul(1024 * 1024);
    if used.saturating_add(cost) > budget
        || disk.f_bavail.saturating_mul(disk.f_frsize)
            < reserve
                .saturating_add(cost.saturating_mul(2))
                .saturating_add(65536)
    {
        tx.execute("UPDATE capture_usage SET skipped=skipped+1 WHERE id=1", [])?;
        tx.commit()?;
        return Ok(None);
    }
    if !exists {
        tx.execute(
            "INSERT INTO bodies VALUES(?1,?2)",
            params![hash, rusqlite::blob::ZeroBlob(bytes)],
        )?;
        let row = tx.last_insert_rowid();
        let mut blob = tx.blob_open("main", "bodies", "body", row, false)?;
        ensure!(
            std::io::copy(input, &mut blob)? == bytes,
            "capture body size changed"
        );
        blob.close()?;
    }
    tx.execute("INSERT INTO requests(at,controller_build,request,body_sha256,status) VALUES(?1,?2,?3,?4,NULL)",params![crate::controller::now(),crate::BUILD_SHA,request,hash])?;
    let id = tx.last_insert_rowid();
    tx.execute("UPDATE capture_usage SET bytes=bytes+?1 WHERE id=1", [cost])?;
    tx.commit()?;
    Ok(Some(id))
}
fn capture_db(settings: &Settings) -> std::path::PathBuf {
    settings.state_dir.join("publish-captures/requests.sqlite")
}
pub fn count_app(settings: &Settings, app: &str) -> Result<u64> {
    if !capture_db(settings).exists() {
        return Ok(0);
    }
    let c = connect(settings)?;
    Ok(c.query_row(
        "SELECT COUNT(*) FROM requests WHERE json_extract(request,'$.app')=?1",
        [app],
        |row| row.get(0),
    )?)
}
/// Drop this game's captured requests and any bodies no other game still uses.
pub fn forget_app(settings: &Settings, app: &str) -> Result<u64> {
    if !capture_db(settings).exists() {
        return Ok(0);
    }
    let mut c = connect(settings)?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let removed = tx.execute(
        "DELETE FROM requests WHERE json_extract(request,'$.app')=?1",
        [app],
    )?;
    tx.execute(
        "DELETE FROM bodies WHERE sha256 NOT IN (SELECT body_sha256 FROM requests)",
        [],
    )?;
    let bytes: u64 = tx.query_row(
        "SELECT COALESCE(SUM(length(request)+length(body)+768),0) FROM requests JOIN bodies ON bodies.sha256=requests.body_sha256",
        [],
        |row| row.get(0),
    )?;
    tx.execute("UPDATE capture_usage SET bytes=?1 WHERE id=1", [bytes])?;
    tx.commit()?;
    Ok(removed as u64)
}
pub fn finish(settings: &Settings, id: i64, status: u16) -> Result<()> {
    connect(settings)?.execute(
        "UPDATE requests SET status=?1 WHERE id=?2",
        params![status, id],
    )?;
    Ok(())
}
/// A consistent, credential-free envelope with exact request bytes. No networking.
pub fn export(settings: &Settings, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "choose a new private export directory");
    let db = settings.state_dir.join("publish-captures/requests.sqlite");
    let mut c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.busy_timeout(Duration::from_secs(5))?;
    let tx = c.transaction()?;
    let (bytes, skipped): (u64, u64) = tx.query_row(
        "SELECT bytes,skipped FROM capture_usage WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let output = std::path::absolute(output)?;
    let parent = output.parent().context("export directory needs a parent")?;
    let stage = tempfile::tempdir_in(parent)?;
    fs::set_permissions(stage.path(), fs::Permissions::from_mode(0o700))?;
    fs::create_dir(stage.path().join("bodies"))?;
    fs::set_permissions(
        stage.path().join("bodies"),
        fs::Permissions::from_mode(0o700),
    )?;
    let mut events = crate::private_file::reserve(&stage.path().join("requests.jsonl"))?;
    let mut count = 0;
    {
        let mut stmt = tx.prepare(
            "SELECT id,at,controller_build,request,body_sha256,status FROM requests ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let request: serde_json::Value = serde_json::from_str(&row.get::<_, String>(3)?)?;
            let hash: String = row.get(4)?;
            ensure!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid capture body hash"
            );
            let body_path = stage.path().join("bodies").join(&hash);
            if !body_path.exists() {
                let body: Vec<u8> =
                    tx.query_row("SELECT body FROM bodies WHERE sha256=?1", [&hash], |r| {
                        r.get(0)
                    })?;
                ensure!(digest(&body) == hash, "capture body checksum mismatch");
                let mut file = crate::private_file::reserve(&body_path)?;
                file.write_all(&body)?;
                file.as_file().sync_all()?;
                file.keep()?;
            }
            serde_json::to_writer(
                &mut events,
                &serde_json::json!({"format_version":1,"sequence":row.get::<_,i64>(0)?,"received_at":row.get::<_,u64>(1)?,"controller_build":row.get::<_,String>(2)?,"request":request,"body_sha256":hash,"http_status":row.get::<_,Option<u16>>(5)?}),
            )?;
            events.write_all(b"\n")?;
            count += 1;
        }
    }
    events.as_file().sync_all()?;
    events.keep()?;
    crate::private_file::finish_json(
        crate::private_file::reserve(&stage.path().join("capture.json"))?,
        &serde_json::json!({"format_version":1,"requests":count,"logical_bytes":bytes,"skipped_requests":skipped,"purpose":"offline importer fixtures; not a complete audit or backup","body_encoding":"exact bytes; sha256 filenames","unknown_outcome":"http_status=null; never assume failure or replay into production"}),
    )?;
    tx.commit()?;
    fs::rename(stage.path(), &output)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
