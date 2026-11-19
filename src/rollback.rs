//! Consistent platform snapshots taken before a release switch.
//!
//! Copies use SQLite's online backup API so a live WAL is included once, not as
//! a torn pair of database and log files. Each snapshot is integrity-checked and
//! copied again to prove it can be restored. One outgoing release is retained.

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const DATABASES: &[&str] = &["platform.sqlite", "publish-captures/requests.sqlite"];
const PLAIN_FILES: &[&str] = &["state.json"];
const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub release: String,
    pub created: u64,
    pub verified: bool,
    pub databases: Vec<FileRecord>,
    pub files: Vec<FileRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

pub fn valid_release(value: &str) -> bool {
    value == "bootstrap"
        || (value.len() == 40
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// Health checks this marker. It does not open the databases; capture already did.
pub fn record_ok(root: &Path) -> bool {
    if root.join("last-failure.json").is_file() {
        return false;
    }
    let Ok(latest) = read_snapshot(&root.join("latest.json")) else {
        return false;
    };
    if latest.version != 1 || !latest.verified || !valid_release(&latest.release) {
        return false;
    }
    read_snapshot(&root.join(&latest.release).join("snapshot.json")).is_ok_and(|snapshot| {
        snapshot.version == 1 && snapshot.verified && snapshot.release == latest.release
    })
}

pub fn capture(state_dir: &Path, rollback_root: &Path, release: &str) -> Result<Snapshot> {
    let result = capture_inner(state_dir, rollback_root, release);
    if result.is_err() {
        note_failure(rollback_root, release);
    }
    result
}

fn capture_inner(state_dir: &Path, rollback_root: &Path, release: &str) -> Result<Snapshot> {
    ensure!(valid_release(release), "invalid release id for snapshot");
    ensure!(
        state_dir.is_absolute(),
        "platform state directory must be absolute"
    );
    fs::create_dir_all(rollback_root)?;
    fs::set_permissions(rollback_root, fs::Permissions::from_mode(0o700))?;
    let state = state_dir
        .canonicalize()
        .context("platform state directory missing")?;
    let root = rollback_root.canonicalize()?;
    ensure!(
        !root.starts_with(&state) && !state.starts_with(&root),
        "rollback directory must stay outside platform state"
    );
    // Crash leftovers are removed before the free-space check so a retry can see them gone.
    reap_incomplete(&root)?;
    discard_restore_temps(&state)?;
    let mut payload = 0u64;
    for relative in DATABASES.iter().chain(PLAIN_FILES) {
        payload = payload.saturating_add(source_bytes(&state.join(relative))?);
    }
    let disk = rustix::fs::statvfs(&root)?;
    let available = disk.f_bavail.saturating_mul(disk.f_frsize);
    let capacity = disk.f_blocks.saturating_mul(disk.f_frsize);
    if !enough_space(available, capacity, payload) {
        return Err(DiskLimit.into());
    }
    let partial = root.join(format!(".partial-{release}"));
    fs::create_dir_all(&partial)?;
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o700))?;
    let snapshot = match (|| -> Result<Snapshot> {
        let mut databases = Vec::new();
        for relative in DATABASES {
            let source = state.join(relative);
            if !source.exists() {
                continue;
            }
            ensure!(source.is_file(), "database path must be a regular file");
            let dest = partial.join(relative);
            fs::create_dir_all(dest.parent().context("snapshot parent missing")?)?;
            online_backup(&source, &dest)?;
            check_database(&dest)?;
            let rehearsal = partial.join(format!(
                ".{}.restore-verify",
                dest.file_name().and_then(|n| n.to_str()).unwrap_or("db")
            ));
            online_backup(&dest, &rehearsal)?;
            check_database(&rehearsal)?;
            ensure!(
                page_count(&dest)? == page_count(&rehearsal)?
                    && schema_fingerprint(&dest)? == schema_fingerprint(&rehearsal)?,
                "restore rehearsal did not match the snapshot"
            );
            fs::remove_file(&rehearsal)?;
            databases.push(record(relative, &dest)?);
        }
        let mut files = Vec::new();
        for relative in PLAIN_FILES {
            let source = state.join(relative);
            if !source.exists() {
                continue;
            }
            ensure!(source.is_file(), "state file must be a regular file");
            let dest = partial.join(relative);
            copy_file(&source, &dest)?;
            files.push(record(relative, &dest)?);
        }
        let snapshot = Snapshot {
            version: 1,
            release: release.to_owned(),
            created: crate::controller::now(),
            verified: true,
            databases,
            files,
        };
        write_json(&partial.join("snapshot.json"), &snapshot)?;
        File::open(&partial)?.sync_all()?;
        Ok(snapshot)
    })() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(error);
        }
    };
    let final_dir = root.join(release);
    let aside = root.join(format!(".aside-{release}"));
    // Keep the previous verified snapshot until the new directory is in place.
    if final_dir.exists() {
        if aside.exists() {
            remove_tree(&aside)?;
        }
        fs::rename(&final_dir, &aside)?;
    }
    if let Err(error) = fs::rename(&partial, &final_dir) {
        if !final_dir.exists() && aside.exists() {
            let _ = fs::rename(&aside, &final_dir);
        }
        let _ = remove_tree(&partial);
        return Err(error.into());
    }
    if aside.exists() {
        remove_tree(&aside)?;
    }
    prune(&root, release)?;
    write_json(&root.join("latest.json"), &snapshot)?;
    let _ = fs::remove_file(root.join("last-failure.json"));
    File::open(&root)?.sync_all()?;
    let bytes = snapshot
        .databases
        .iter()
        .chain(&snapshot.files)
        .fold(0u64, |total, file| total.saturating_add(file.bytes));
    tracing::info!(release, bytes, "pre-update database snapshot verified");
    Ok(snapshot)
}

pub fn verify(snapshot_dir: &Path) -> Result<Snapshot> {
    let snapshot = read_snapshot(&snapshot_dir.join("snapshot.json"))?;
    ensure!(snapshot.version == 1, "unsupported snapshot version");
    ensure!(snapshot.verified, "snapshot was not marked verified");
    ensure!(valid_release(&snapshot.release), "invalid snapshot release");
    ensure!(
        snapshot
            .databases
            .iter()
            .all(|file| allowed_database(&file.path))
            && snapshot.files.iter().all(|file| allowed_file(&file.path)),
        "snapshot contains an unexpected path"
    );
    for file in snapshot.databases.iter().chain(&snapshot.files) {
        let path = snapshot_dir.join(&file.path);
        ensure!(path.is_file(), "snapshot file {} is missing", file.path);
        let (hash, bytes) = sha256_file(&path)?;
        ensure!(
            hash == file.sha256 && bytes == file.bytes,
            "snapshot file {} changed after it was sealed",
            file.path
        );
    }
    for file in &snapshot.databases {
        check_database(&snapshot_dir.join(&file.path))?;
    }
    Ok(snapshot)
}

/// Replace platform databases with a verified snapshot. The caller must stop the
/// controller first; a held `controller.lock` makes that check local as well.
pub fn restore(snapshot_dir: &Path, state_dir: &Path) -> Result<()> {
    let snapshot = verify(snapshot_dir)?;
    ensure!(
        state_dir.is_absolute(),
        "platform state directory must be absolute"
    );
    let state = state_dir
        .canonicalize()
        .context("platform state directory missing")?;
    if snapshot.databases.is_empty() && snapshot.files.is_empty() {
        return Ok(());
    }
    let lock = hold_controller(&state)?;
    let mut staged = Vec::new();
    let result = (|| -> Result<()> {
        let mut payload = 0u64;
        for file in snapshot.databases.iter().chain(&snapshot.files) {
            payload = payload.saturating_add(file.bytes);
        }
        discard_restore_temps(&state)?;
        let disk = rustix::fs::statvfs(&state)?;
        let available = disk.f_bavail.saturating_mul(disk.f_frsize);
        let capacity = disk.f_blocks.saturating_mul(disk.f_frsize);
        ensure!(
            enough_space(available, capacity, payload),
            "not enough free disk to restore the snapshot beside the live files"
        );
        for file in snapshot.databases.iter().chain(&snapshot.files) {
            let dest = state.join(&file.path);
            ensure!(
                dest.parent()
                    .is_some_and(|parent| parent.starts_with(&state)),
                "snapshot path escapes the state directory"
            );
            if dest
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                bail!("refusing to restore through a symlink");
            }
            let parent = dest.parent().context("restore parent missing")?;
            fs::create_dir_all(parent)?;
            ensure!(
                parent.symlink_metadata()?.is_dir(),
                "restore parent must be a real directory"
            );
            if parent != state.as_path() {
                let state_owner = state.symlink_metadata()?;
                std::os::unix::fs::chown(parent, Some(state_owner.uid()), Some(state_owner.gid()))?;
            }
            let temp = parent.join(format!(
                ".{}.rollback",
                dest.file_name()
                    .and_then(|name| name.to_str())
                    .context("file name")?
            ));
            if temp.exists() {
                fs::remove_file(&temp)?;
            }
            let source = snapshot_dir.join(&file.path);
            if snapshot.databases.iter().any(|db| db.path == file.path) {
                online_backup(&source, &temp)?;
                check_database(&temp)?;
            } else {
                copy_file(&source, &temp)?;
            }
            let (hash, bytes) = sha256_file(&temp)?;
            ensure!(
                hash == file.sha256 && bytes == file.bytes,
                "restored {} did not match the snapshot",
                file.path
            );
            let owner = ownership(&dest, parent)?;
            std::os::unix::fs::chown(&temp, Some(owner.uid), Some(owner.gid))?;
            fs::set_permissions(&temp, fs::Permissions::from_mode(owner.mode))?;
            File::open(&temp)?.sync_all()?;
            staged.push(Staged {
                temp,
                dest,
                sqlite: allowed_database(&file.path),
            });
        }
        for item in &staged {
            if item.sqlite {
                discard_sidecars(&item.dest)?;
            }
            fs::rename(&item.temp, &item.dest)?;
            if item.sqlite {
                discard_sidecars(&item.dest)?;
                check_database(&item.dest)?;
            }
            if let Some(parent) = item.dest.parent() {
                File::open(parent)?.sync_all()?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for item in &staged {
            let _ = fs::remove_file(&item.temp);
        }
    }
    drop(lock);
    result?;
    tracing::info!(release = %snapshot.release, "restored pre-update database snapshot");
    Ok(())
}

struct Staged {
    temp: PathBuf,
    dest: PathBuf,
    sqlite: bool,
}
struct Owner {
    uid: u32,
    gid: u32,
    mode: u32,
}

fn ownership(dest: &Path, parent: &Path) -> Result<Owner> {
    if dest.exists() {
        let meta = dest.symlink_metadata()?;
        return Ok(Owner {
            uid: meta.uid(),
            gid: meta.gid(),
            mode: meta.mode() & 0o777,
        });
    }
    let meta = parent.symlink_metadata()?;
    Ok(Owner {
        uid: meta.uid(),
        gid: meta.gid(),
        mode: 0o600,
    })
}

fn hold_controller(state_dir: &Path) -> Result<Option<File>> {
    let path = state_dir.join("controller.lock");
    if !path.exists() {
        return Ok(None);
    }
    ensure!(
        path.symlink_metadata()?.is_file(),
        "controller lock must be a regular file"
    );
    let file = OpenOptions::new().read(true).write(true).open(&path)?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => {
            anyhow::anyhow!("controller is still running; stop infuser.service before restoring")
        }
        std::fs::TryLockError::Error(error) => error.into(),
    })?;
    Ok(Some(file))
}

fn prune(root: &Path, keep: &str) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == keep || name.starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() && valid_release(&name) {
            remove_tree(&entry.path())?;
        }
    }
    Ok(())
}

/// Drop crash leftovers. A half-published snapshot is put back; it is not deleted
/// until a newer verified copy has replaced it.
fn reap_incomplete(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let names = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    for name in names {
        let text = name.to_string_lossy();
        if text == "latest.json" || text == "last-failure.json" {
            continue;
        }
        let path = root.join(&name);
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if let Some(release) = text.strip_prefix(".aside-") {
            let published = root.join(release);
            if meta.is_dir()
                && !meta.file_type().is_symlink()
                && valid_release(release)
                && !published.exists()
            {
                fs::rename(&path, published)?;
            } else {
                remove_tree(&path)?;
            }
            continue;
        }
        if text.starts_with(".partial-") || meta.file_type().is_symlink() || meta.is_file() {
            remove_tree(&path)?;
        }
    }
    Ok(())
}

fn discard_restore_temps(state: &Path) -> Result<()> {
    for relative in DATABASES.iter().chain(PLAIN_FILES) {
        let dest = state.join(relative);
        let Some(name) = dest.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(parent) = dest.parent() else {
            continue;
        };
        let temp = parent.join(format!(".{name}.rollback"));
        if temp.symlink_metadata().is_ok() {
            remove_tree(&temp)?;
        }
        for suffix in ["-wal", "-shm"] {
            let sidecar = sidecar(&temp, suffix);
            if sidecar.symlink_metadata().is_ok() {
                remove_tree(&sidecar)?;
            }
        }
    }
    Ok(())
}

fn remove_tree(path: &Path) -> Result<()> {
    let meta = match path.symlink_metadata() {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path)?;
    } else if meta.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

/// Raised when a snapshot or its restore rehearsal would leave the filesystem too full.
#[derive(Debug)]
pub struct DiskLimit;

impl std::fmt::Display for DiskLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "not enough free disk for a pre-update snapshot; current release left unchanged",
        )
    }
}

impl std::error::Error for DiskLimit {}

/// Bytes that must remain after a two-copy snapshot: at least 1 GiB, or 5% of the filesystem.
fn minimum_free(capacity: u64) -> u64 {
    GIB.max(capacity.saturating_mul(5) / 100)
}

fn enough_space(available: u64, capacity: u64, payload: u64) -> bool {
    available
        >= payload
            .saturating_mul(2)
            .saturating_add(minimum_free(capacity))
}

fn note_failure(root: &Path, release: &str) {
    let _ = fs::create_dir_all(root);
    let _ = write_json(
        &root.join("last-failure.json"),
        &serde_json::json!({
            "at": crate::controller::now(),
            "release": release,
        }),
    );
}

fn allowed_database(path: &str) -> bool {
    DATABASES.contains(&path)
}
fn allowed_file(path: &str) -> bool {
    PLAIN_FILES.contains(&path)
}

fn source_bytes(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    ensure!(path.is_file(), "snapshot source must be a regular file");
    let mut total = path.metadata()?.len();
    for suffix in ["-wal", "-shm"] {
        let sidecar = sidecar(path, suffix);
        if sidecar.is_file() {
            total = total.saturating_add(sidecar.metadata()?.len());
        }
    }
    Ok(total)
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn discard_sidecars(db: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm"] {
        let path = sidecar(db, suffix);
        if !path.exists() {
            continue;
        }
        ensure!(
            path.symlink_metadata()?.is_file(),
            "refusing to delete a sqlite sidecar that is not a regular file"
        );
        fs::remove_file(path)?;
    }
    Ok(())
}

fn online_backup(source: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    discard_sidecars(dest)?;
    let src = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    src.busy_timeout(Duration::from_secs(30))?;
    src.backup("main", dest, None::<fn(rusqlite::backup::Progress)>)
        .with_context(|| format!("online backup of {} failed", source.display()))?;
    fs::set_permissions(dest, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn check_database(path: &Path) -> Result<()> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let message: String =
        connection.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    ensure!(message == "ok", "database snapshot failed integrity check");
    Ok(())
}

fn page_count(path: &Path) -> Result<i64> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(connection.query_row("PRAGMA page_count", [], |row| row.get(0))?)
}

fn schema_fingerprint(path: &Path) -> Result<String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare("SELECT sql FROM sqlite_master ORDER BY sql")?;
    let mut hasher = Sha256::new();
    let rows = statement.query_map([], |row| row.get::<_, Option<String>>(0))?;
    for row in rows {
        hasher.update(row?.unwrap_or_default().as_bytes());
        hasher.update([0]);
    }
    Ok(hex(&hasher.finalize()))
}

fn copy_file(source: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(dest)?;
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
    }
    output.sync_all()?;
    Ok(())
}

fn record(relative: &str, path: &Path) -> Result<FileRecord> {
    let (sha256, bytes) = sha256_file(path)?;
    Ok(FileRecord {
        path: relative.to_owned(),
        sha256,
        bytes,
    })
}

fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        bytes += u64::try_from(n)?;
    }
    Ok((hex(&hasher.finalize()), bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_snapshot(path: &Path) -> Result<Snapshot> {
    Ok(serde_json::from_slice(&fs::read(path).with_context(
        || format!("snapshot record {} is missing", path.display()),
    )?)?)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    crate::private_file::replace(path, &serde_json::to_vec(value)?)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn row(path: &Path, value: &str) -> Result<i64> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(
            connection.query_row("SELECT COUNT(*) FROM marker WHERE v=?1", [value], |row| {
                row.get(0)
            })?,
        )
    }

    #[test]
    fn online_backup_keeps_committed_wal_rows_and_restore_drops_later_ones() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path().join("state");
        let captures = state.join("publish-captures");
        fs::create_dir_all(&captures)?;
        let db = state.join("platform.sqlite");
        let requests = captures.join("requests.sqlite");
        let live = Connection::open(&db)?;
        live.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE marker(v TEXT);
             INSERT INTO marker VALUES('base');
             PRAGMA wal_checkpoint(TRUNCATE);
             INSERT INTO marker VALUES('after');",
        )?;
        let wal = sidecar(&db, "-wal");
        assert!(
            wal.metadata()?.len() > 0,
            "the committed row must sit in the WAL"
        );
        let naive = dir.path().join("naive.sqlite");
        fs::copy(&db, &naive)?;
        assert_eq!(
            row(&naive, "after")?,
            0,
            "copying the main file misses the live WAL"
        );
        Connection::open(&requests)?
            .execute_batch("CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('capture');")?;
        fs::write(state.join("state.json"), br#"{"marker":"keep"}"#)?;
        fs::set_permissions(&db, fs::Permissions::from_mode(0o640))?;

        live.execute("BEGIN IMMEDIATE", [])?;
        live.execute("INSERT INTO marker VALUES('uncommitted')", [])?;
        let root = dir.path().join("rollback");
        let release = "a".repeat(40);
        let snapshot = capture(&state, &root, &release)?;
        assert!(snapshot.verified);
        assert_eq!(snapshot.databases.len(), 2);
        assert_eq!(snapshot.files.len(), 1);
        let saved = root.join(&release).join("platform.sqlite");
        assert_eq!(row(&saved, "after")?, 1);
        assert_eq!(row(&saved, "uncommitted")?, 0);
        assert_eq!(row(&saved, "base")?, 1);
        check_database(&saved)?;
        live.execute("ROLLBACK", [])?;
        let older = "b".repeat(40);
        fs::create_dir(root.join(&older))?;
        capture(&state, &root, &release)?;
        assert!(
            !root.join(&older).exists(),
            "only the outgoing release is retained"
        );
        assert!(record_ok(&root));
        live.execute("INSERT INTO marker VALUES('later')", [])?;
        assert!(
            wal.metadata()?.len() > 0,
            "the post-snapshot commit is in the live WAL"
        );
        fs::write(state.join("state.json"), br#"{"marker":"lose"}"#)?;
        drop(live);
        fs::write(&wal, b"stale-wal")?;

        fs::set_permissions(&db, fs::Permissions::from_mode(0o600))?;
        let lock = state.join("controller.lock");
        File::create(&lock)?;
        let held = OpenOptions::new().read(true).write(true).open(&lock)?;
        held.try_lock()?;
        assert!(restore(&root.join(&release), &state).is_err());
        drop(held);

        restore(&root.join(&release), &state)?;
        if wal.exists() {
            assert_ne!(fs::read(&wal)?, b"stale-wal");
        }
        assert_eq!(row(&db, "later")?, 0);
        assert_eq!(row(&db, "after")?, 1);
        assert_eq!(row(&requests, "capture")?, 1);
        assert_eq!(fs::read(state.join("state.json"))?, br#"{"marker":"keep"}"#);
        assert_eq!(db.metadata()?.permissions().mode() & 0o777, 0o600);
        let linked = state.join("platform.sqlite.link");
        std::os::unix::fs::symlink(&db, &linked)?;
        // A tampered manifest must not be able to name a new destination.
        let mut forged = read_snapshot(&root.join(&release).join("snapshot.json"))?;
        forged.databases[0].path = "platform.sqlite.link".into();
        write_json(&root.join(&release).join("snapshot.json"), &forged)?;
        assert!(verify(&root.join(&release)).is_err());
        Ok(())
    }

    #[test]
    fn free_space_floor_is_one_gib_or_five_percent() {
        let vm = 47 * GIB;
        let floor = minimum_free(vm);
        assert_eq!(floor, vm * 5 / 100);
        assert!(floor > GIB);
        let payload = 3 * GIB;
        assert!(!enough_space(floor + payload * 2 - 1, vm, payload));
        assert!(enough_space(floor + payload * 2, vm, payload));
        // On a small filesystem the 1 GiB floor is the larger requirement.
        let small = 8 * GIB;
        assert_eq!(minimum_free(small), GIB);
        assert!(!enough_space(GIB + 4, small, 3));
        assert!(enough_space(GIB + 6, small, 3));
    }

    #[test]
    fn interrupted_snapshot_is_removed_without_dropping_the_verified_copy() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path().join("state");
        fs::create_dir(&state)?;
        let db = state.join("platform.sqlite");
        Connection::open(&db)?
            .execute_batch("CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('good');")?;
        let root = dir.path().join("rollback");
        let release = "a".repeat(40);
        let other = "b".repeat(40);
        capture(&state, &root, &release)?;
        // Crash after the verified copy was moved aside and a partial write was left behind.
        fs::rename(root.join(&release), root.join(format!(".aside-{release}")))?;
        let partial = root.join(format!(".partial-{other}"));
        fs::create_dir(&partial)?;
        fs::write(partial.join("junk"), vec![7u8; 4096])?;
        fs::write(root.join(".tmp-crash"), b"temp")?;
        fs::write(state.join(".platform.sqlite.rollback"), b"restore-temp")?;
        fs::create_dir(root.join(&other))?;
        fs::write(root.join(&other).join("blob"), b"previous-generation")?;
        fs::write(&db, b"not a database")?;
        let error = capture(&state, &root, &release).unwrap_err();
        assert!(!error.is::<DiskLimit>());
        assert!(!partial.exists());
        assert!(!root.join(".tmp-crash").exists());
        assert!(!root.join(format!(".aside-{release}")).exists());
        assert!(!state.join(".platform.sqlite.rollback").exists());
        assert_eq!(
            row(&root.join(&release).join("platform.sqlite"), "good")?,
            1,
            "the verified snapshot is restored from the interrupted publish"
        );
        assert!(
            root.join(&other).join("blob").is_file(),
            "an older generation stays until the replacement verifies"
        );
        fs::remove_file(&db)?;
        Connection::open(&db)?
            .execute_batch("CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('next');")?;
        capture(&state, &root, &other)?;
        let mut generations = fs::read_dir(&root)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<Vec<_>>>()?;
        generations.retain(|name| valid_release(name));
        generations.sort();
        assert_eq!(generations, vec![other.clone()]);
        assert!(!root.join(&release).exists());
        assert!(!root.join(format!(".aside-{other}")).exists());
        assert!(!root.join(format!(".partial-{other}")).exists());
        Ok(())
    }

    #[test]
    fn failed_capture_keeps_the_previous_snapshot() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let state = dir.path().join("state");
        fs::create_dir(&state)?;
        let db = state.join("platform.sqlite");
        Connection::open(&db)?
            .execute_batch("CREATE TABLE marker(v TEXT); INSERT INTO marker VALUES('good');")?;
        let root = dir.path().join("rollback");
        let release = "c".repeat(40);
        capture(&state, &root, &release)?;
        fs::write(&db, b"not a database")?;
        assert!(capture(&state, &root, &release).is_err());
        assert_eq!(
            row(&root.join(&release).join("platform.sqlite"), "good")?,
            1
        );
        assert!(root.join("last-failure.json").is_file());
        assert!(!record_ok(&root));
        Ok(())
    }
}
