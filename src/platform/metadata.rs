//! Mutable presentation metadata lives outside immutable builds and legacy catalog JSON.
//! One bounded artwork blob per app; replacement and its revision commit atomically.
use super::*;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct GameMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub art_prompt: Option<String>,
    pub art_prompt_completed_revision: Option<String>,
    pub card_art_revision: Option<String>,
    pub card_art_bytes: u64,
    pub updated: u64,
    /// An admin rename. Later publishes and publisher title edits keep this name.
    #[serde(default)]
    pub title_locked: bool,
}

pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_metadata (
        app TEXT PRIMARY KEY, title TEXT, card_art_revision TEXT, card_art BLOB,
        updated INTEGER NOT NULL DEFAULT 0
    );",
    )?;
    let columns = c
        .prepare("PRAGMA table_info(game_metadata)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in ["description", "art_prompt", "art_prompt_completed_revision"] {
        if !columns.iter().any(|column| column == name) {
            c.execute(
                &format!("ALTER TABLE game_metadata ADD COLUMN {name} TEXT"),
                [],
            )?;
        }
    }
    if !columns.iter().any(|column| column == "title_locked") {
        c.execute(
            "ALTER TABLE game_metadata ADD COLUMN title_locked INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

pub(super) fn read(c: &Connection) -> Result<BTreeMap<String, GameMetadata>> {
    let mut statement = c.prepare("SELECT app,title,card_art_revision,COALESCE(length(card_art),0),updated,description,art_prompt,art_prompt_completed_revision,title_locked FROM game_metadata")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get(0)?,
            GameMetadata {
                title: r.get(1)?,
                description: r.get(5)?,
                art_prompt: r.get(6)?,
                art_prompt_completed_revision: r.get(7)?,
                card_art_revision: r.get(2)?,
                card_art_bytes: r.get(3)?,
                updated: r.get(4)?,
                title_locked: r.get::<_, i64>(8)? != 0,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Default)]
pub struct MetadataUpdate<'a> {
    pub title: Option<Option<&'a str>>,
    pub description: Option<Option<&'a str>>,
    pub art_prompt: Option<Option<&'a str>>,
    pub art: Option<Option<&'a [u8]>>,
    /// Completing artwork must not overwrite a newer brief or a completed request.
    pub complete_prompt: Option<&'a str>,
    /// Admin rename. Publisher title edits cannot set or clear the lock.
    pub override_lock: bool,
}

impl Database {
    pub fn metadata_update(&self, app: &str, update: MetadataUpdate<'_>) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let data: Data = serde_json::from_str(&raw)?;
        ensure!(
            data.apps.contains_key(app),
            crate::portal::fault(404, "unknown game")
        );
        tx.execute("INSERT OR IGNORE INTO game_metadata(app) VALUES(?1)", [app])?;
        if let Some(expected) = update.complete_prompt {
            let (prompt, completed): (Option<String>, Option<String>) = tx.query_row(
                "SELECT art_prompt,art_prompt_completed_revision FROM game_metadata WHERE app=?1",
                [app],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            ensure!(
                prompt.as_deref().map(hash).as_deref() == Some(expected)
                    && completed.as_deref() != Some(expected),
                crate::portal::fault(409, "art prompt changed or was already completed")
            );
            ensure!(
                update.art.is_some_and(|art| art.is_some()),
                "completion requires artwork"
            );
            tx.execute(
                "UPDATE game_metadata SET art_prompt_completed_revision=?2 WHERE app=?1",
                params![app, expected],
            )?;
        }
        if let Some(title) = update.title {
            let locked = tx.query_row(
                "SELECT title_locked FROM game_metadata WHERE app=?1",
                [app],
                |r| r.get::<_, i64>(0),
            )? != 0;
            let current: Option<String> =
                tx.query_row("SELECT title FROM game_metadata WHERE app=?1", [app], |r| {
                    r.get(0)
                })?;
            if locked && !update.override_lock && title != current.as_deref() {
                return Err(crate::portal::fault(
                    409,
                    "An admin renamed this game. The Library title stays through later publishes. Ask an admin to change it.",
                ));
            }
            if update.override_lock || title != current.as_deref() {
                tx.execute(
                    "UPDATE game_metadata SET title=?2,updated=?3,title_locked=CASE WHEN ?4=1 THEN 1 ELSE title_locked END WHERE app=?1",
                    params![app, title, now(), i64::from(update.override_lock)],
                )?;
            }
        }
        if let Some(art) = update.art {
            let revision = art.map(|bytes| {
                Sha256::digest(bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            });
            tx.execute(
                "UPDATE game_metadata SET card_art=?2,card_art_revision=?3,updated=?4 WHERE app=?1",
                params![app, art, revision, now()],
            )?;
        }
        if let Some(description) = update.description {
            tx.execute(
                "UPDATE game_metadata SET description=?2,updated=?3 WHERE app=?1",
                params![app, description, now()],
            )?;
        }
        if let Some(prompt) = update.art_prompt {
            tx.execute(
                "UPDATE game_metadata SET art_prompt=?2,art_prompt_completed_revision=NULL,updated=?3 WHERE app=?1",
                params![app, prompt, now()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn metadata_art(&self, app: &str, revision: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT card_art FROM game_metadata WHERE app=?1 AND card_art_revision=?2 AND card_art IS NOT NULL",
            params![app,revision], |r| r.get(0),
        ).optional()?)
    }
}
