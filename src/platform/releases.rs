//! One feed record per Infuser release, folded into a single Recently entry
//! when several healthy releases land within six hours.
use super::Database;
use crate::deployment::Phase;
use anyhow::Result;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Releases that become healthy inside this window share one Recently entry.
pub const BATCH_WINDOW_SECS: u64 = 6 * 60 * 60;
const MAX_LINES: usize = 12;
const MAX_COMBINED: usize = 24;

#[derive(Debug, PartialEq, Eq)]
pub enum Announcement {
    /// First visible entry in a new window.
    Posted,
    /// Added to the entry that is already open.
    Joined,
    /// Remembered so a restart does not ask again, and kept out of the feed.
    Recorded,
    /// This release was already announced.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedUpdate {
    pub id: String,
    pub at: u64,
    pub lines: Vec<String>,
    pub counts: BTreeMap<String, usize>,
    pub mine: Option<String>,
}

pub fn release_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The updater reports `Current` only after the new build has passed its health
/// check. A failed or rolled-back release never reaches that report on the
/// build that was rejected.
pub fn should_announce(phase: Phase, reported: Option<&str>, running: &str) -> bool {
    phase == Phase::Current && reported == Some(running) && release_sha(running)
}

pub fn consider(
    db: &Database,
    phase: Phase,
    reported: Option<&str>,
    running: &str,
    family_lines: &[String],
    now: u64,
) -> Result<Option<Announcement>> {
    if !should_announce(phase, reported, running) {
        return Ok(None);
    }
    Ok(Some(db.announce_release(running, family_lines, now)?))
}

pub(super) fn initialize(connection: &rusqlite::Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS platform_releases(
            release TEXT PRIMARY KEY,
            batch TEXT NOT NULL,
            announced_at INTEGER NOT NULL,
            lines TEXT NOT NULL,
            visible INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS platform_release_reactions(
            batch TEXT NOT NULL,
            profile TEXT NOT NULL,
            emoji TEXT NOT NULL,
            PRIMARY KEY(batch, profile)) WITHOUT ROWID;",
    )?;
    Ok(())
}

impl Database {
    pub fn announce_release(
        &self,
        release: &str,
        family_lines: &[String],
        now: u64,
    ) -> Result<Announcement> {
        let lines = clean_lines(family_lines);
        let visible = !lines.is_empty();
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT release FROM platform_releases WHERE release=?1",
                [release],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(Announcement::Duplicate);
        }
        let (batch, joined) = if visible {
            match open_batch(&tx, now)? {
                Some(batch) => (batch, true),
                None => (release.to_string(), false),
            }
        } else {
            (release.to_string(), false)
        };
        tx.execute(
            "INSERT INTO platform_releases(release,batch,announced_at,lines,visible) VALUES(?1,?2,?3,?4,?5)",
            params![release, batch, now as i64, serde_json::to_string(&lines)?, visible as i64],
        )?;
        tx.commit()?;
        Ok(if !visible {
            Announcement::Recorded
        } else if joined {
            Announcement::Joined
        } else {
            Announcement::Posted
        })
    }

    pub fn platform_feed(
        &self,
        viewer: &str,
        enabled: impl Fn(&str) -> bool,
    ) -> Result<Vec<FeedUpdate>> {
        let connection = self.connection.lock().unwrap();
        let mut rows = connection.prepare(
            "SELECT batch,announced_at,lines FROM platform_releases WHERE visible=1 ORDER BY announced_at ASC",
        )?;
        let mut batches: Vec<FeedUpdate> = Vec::new();
        let mut mapped = rows.query([])?;
        while let Some(row) = mapped.next()? {
            let id: String = row.get(0)?;
            let at: i64 = row.get(1)?;
            let raw: String = row.get(2)?;
            let parsed: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
            if let Some(batch) = batches.iter_mut().find(|batch| batch.id == id) {
                batch.at = at as u64;
                for line in parsed {
                    if batch.lines.len() == MAX_COMBINED {
                        break;
                    }
                    if !batch.lines.contains(&line) {
                        batch.lines.push(line);
                    }
                }
            } else {
                let mut lines = Vec::new();
                for line in parsed {
                    if lines.len() == MAX_COMBINED {
                        break;
                    }
                    if !lines.contains(&line) {
                        lines.push(line);
                    }
                }
                batches.push(FeedUpdate {
                    id,
                    at: at as u64,
                    lines,
                    counts: BTreeMap::new(),
                    mine: None,
                });
            }
        }
        drop(mapped);
        drop(rows);
        let mut reactions = connection
            .prepare("SELECT profile,emoji FROM platform_release_reactions WHERE batch=?1")?;
        for batch in &mut batches {
            let mut mapped = reactions.query([&batch.id])?;
            while let Some(row) = mapped.next()? {
                let profile: String = row.get(0)?;
                if !enabled(&profile) {
                    continue;
                }
                let emoji: String = row.get(1)?;
                if viewer == profile {
                    batch.mine = Some(emoji.clone());
                }
                *batch.counts.entry(emoji).or_default() += 1;
            }
        }
        batches.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
        batches.truncate(12);
        Ok(batches)
    }

    pub fn set_platform_reaction(&self, batch: &str, profile: &str, emoji: &str) -> Result<bool> {
        let connection = self.connection.lock().unwrap();
        let visible: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM platform_releases WHERE batch=?1 AND visible=1 LIMIT 1",
                [batch],
                |row| row.get(0),
            )
            .optional()?;
        if visible.is_none() {
            return Ok(false);
        }
        connection.execute(
            "INSERT INTO platform_release_reactions(batch,profile,emoji) VALUES(?1,?2,?3)
             ON CONFLICT(batch,profile) DO UPDATE SET emoji=excluded.emoji",
            params![batch, profile, emoji],
        )?;
        Ok(true)
    }
}

fn open_batch(tx: &rusqlite::Transaction<'_>, now: u64) -> Result<Option<String>> {
    let open: Option<(String, i64)> = tx
        .query_row(
            "SELECT batch, MIN(announced_at) FROM platform_releases WHERE visible=1
             GROUP BY batch ORDER BY MIN(announced_at) DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match open {
        Some((batch, at)) if now.saturating_sub(at as u64) < BATCH_WINDOW_SECS => Some(batch),
        _ => None,
    })
}

fn clean_lines(lines: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    let mut seen = BTreeSet::new();
    for line in lines {
        let text = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || !seen.insert(text.clone()) {
            continue;
        }
        let text = if text.chars().count() > 160 {
            let mut shortened: String = text.chars().take(159).collect();
            shortened.truncate(shortened.trim_end().len());
            shortened.push('…');
            shortened
        } else {
            text
        };
        cleaned.push(text);
        if cleaned.len() == MAX_LINES {
            break;
        }
    }
    cleaned
}

pub fn feed_json(updates: &[FeedUpdate]) -> Vec<Value> {
    updates
        .iter()
        .map(|update| {
            json!({
                "id": update.id,
                "at": update.at,
                "lines": update.lines,
                "reactions": {"counts": update.counts, "mine": update.mine},
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rollback;

    fn sha(byte: u8) -> String {
        format!("{byte:02x}").repeat(20)
    }

    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let database = Database::open(dir.path()).unwrap();
        (dir, database)
    }

    fn lines(text: &str) -> Vec<String> {
        vec![text.to_string()]
    }

    #[test]
    fn one_event_per_release_and_a_restart_does_not_add_another() {
        let (_dir, database) = db();
        let release = sha(1);
        assert_eq!(
            database
                .announce_release(&release, &lines("Games remember where you left off"), 1_000)
                .unwrap(),
            Announcement::Posted
        );
        assert_eq!(
            database
                .announce_release(&release, &lines("Games remember where you left off"), 1_100)
                .unwrap(),
            Announcement::Duplicate
        );
        assert_eq!(
            consider(
                &database,
                Phase::Current,
                Some(&release),
                &release,
                &lines("Games remember where you left off"),
                1_200,
            )
            .unwrap(),
            Some(Announcement::Duplicate)
        );
        let feed = database.platform_feed("player", |_| true).unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].lines, lines("Games remember where you left off"));
        let count: i64 = database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM platform_releases", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_failed_or_different_build_does_not_announce() {
        let (_dir, database) = db();
        let release = sha(2);
        for phase in [
            Phase::Failed,
            Phase::Activating,
            Phase::Waiting,
            Phase::Checking,
        ] {
            assert_eq!(
                consider(
                    &database,
                    phase,
                    Some(&release),
                    &release,
                    &lines("Nope"),
                    50
                )
                .unwrap(),
                None
            );
        }
        assert_eq!(
            consider(
                &database,
                Phase::Current,
                Some(&release),
                &sha(3),
                &lines("Nope"),
                50,
            )
            .unwrap(),
            None
        );
        assert_eq!(
            consider(
                &database,
                Phase::Current,
                Some("development"),
                "development",
                &lines("Nope"),
                50,
            )
            .unwrap(),
            None
        );
        assert!(
            database
                .platform_feed("player", |_| true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_internal_release_is_remembered_and_stays_out_of_the_feed() {
        let (_dir, database) = db();
        let release = sha(4);
        assert_eq!(
            database.announce_release(&release, &[], 10).unwrap(),
            Announcement::Recorded
        );
        assert_eq!(
            database.announce_release(&release, &[], 11).unwrap(),
            Announcement::Duplicate
        );
        assert!(
            database
                .platform_feed("player", |_| true)
                .unwrap()
                .is_empty()
        );
        let visible = database
            .announce_release(&sha(5), &lines("You can react to a friend's game"), 12)
            .unwrap();
        assert_eq!(visible, Announcement::Posted);
        assert_eq!(database.platform_feed("player", |_| true).unwrap().len(), 1);
    }

    #[test]
    fn releases_inside_six_hours_share_one_entry_and_then_a_new_one_starts() {
        let (_dir, database) = db();
        let first = sha(6);
        let second = sha(7);
        let third = sha(8);
        database
            .announce_release(&first, &lines("Games remember where you left off"), 10_000)
            .unwrap();
        assert_eq!(
            database
                .announce_release(
                    &second,
                    &lines("You can react to a friend's game"),
                    10_000 + 3_600
                )
                .unwrap(),
            Announcement::Joined
        );
        let during = database.platform_feed("player", |_| true).unwrap();
        assert_eq!(during.len(), 1);
        assert_eq!(during[0].id, first);
        assert_eq!(
            during[0].lines,
            vec![
                "Games remember where you left off".to_string(),
                "You can react to a friend's game".to_string(),
            ]
        );
        assert_eq!(
            database
                .announce_release(
                    &third,
                    &lines("Voice chat got clearer"),
                    10_000 + BATCH_WINDOW_SECS,
                )
                .unwrap(),
            Announcement::Posted
        );
        let after = database.platform_feed("player", |_| true).unwrap();
        assert_eq!(after.len(), 2);
        assert_eq!(after[0].id, third);
        assert_eq!(after[0].lines, lines("Voice chat got clearer"));
        assert_eq!(after[1].id, first);
        assert_eq!(after[1].lines.len(), 2);
    }

    #[test]
    fn rolling_back_the_database_removes_the_release_event() {
        let state = tempfile::tempdir().unwrap();
        let snaps = tempfile::tempdir().unwrap();
        let release = sha(9);
        {
            let database = Database::open(state.path()).unwrap();
            drop(database);
        }
        rollback::capture(state.path(), snaps.path(), "bootstrap").unwrap();
        {
            let database = Database::open(state.path()).unwrap();
            assert_eq!(
                consider(
                    &database,
                    Phase::Current,
                    Some(&release),
                    &release,
                    &lines("Games remember where you left off"),
                    80,
                )
                .unwrap(),
                Some(Announcement::Posted)
            );
            assert_eq!(database.platform_feed("player", |_| true).unwrap().len(), 1);
        }
        rollback::restore(&snaps.path().join("bootstrap"), state.path()).unwrap();
        let database = Database::open(state.path()).unwrap();
        assert!(
            database
                .platform_feed("player", |_| true)
                .unwrap()
                .is_empty(),
            "a restored snapshot is the database from before this release was announced"
        );
        let count: i64 = database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM platform_releases", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn a_reaction_sticks_to_the_batched_entry() {
        let (_dir, database) = db();
        let first = sha(10);
        database
            .announce_release(&first, &lines("Games remember where you left off"), 1)
            .unwrap();
        assert!(database.set_platform_reaction(&first, "ada", "⭐").unwrap());
        database
            .announce_release(&sha(11), &lines("You can react to a friend's game"), 2)
            .unwrap();
        let feed = database.platform_feed("ada", |_| true).unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].mine.as_deref(), Some("⭐"));
        assert_eq!(feed[0].counts.get("⭐"), Some(&1));
        assert!(
            !database
                .set_platform_reaction(&sha(12), "ada", "⭐")
                .unwrap()
        );
    }
}
