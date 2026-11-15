//! Mutable presentation metadata lives outside immutable builds and legacy catalog JSON.
//! One bounded artwork blob per app; replacement and its revision commit atomically.
use super::*;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct GameMetadata {
    pub title: Option<String>,
    pub card_art_revision: Option<String>,
    pub card_art_bytes: u64,
    pub updated: u64,
}

pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_metadata (
        app TEXT PRIMARY KEY, title TEXT, card_art_revision TEXT, card_art BLOB,
        updated INTEGER NOT NULL DEFAULT 0
    );",
    )?;
    Ok(())
}

pub(super) fn read(c: &Connection) -> Result<BTreeMap<String, GameMetadata>> {
    let mut statement = c.prepare("SELECT app,title,card_art_revision,COALESCE(length(card_art),0),updated FROM game_metadata")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get(0)?,
            GameMetadata {
                title: r.get(1)?,
                card_art_revision: r.get(2)?,
                card_art_bytes: r.get(3)?,
                updated: r.get(4)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

impl Database {
    pub fn metadata_update(
        &self,
        app: &str,
        title: Option<Option<&str>>,
        art: Option<Option<&[u8]>>,
    ) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
        let data: Data = serde_json::from_str(&raw)?;
        ensure!(
            data.apps.contains_key(app),
            crate::portal::fault(404, "unknown game")
        );
        tx.execute("INSERT OR IGNORE INTO game_metadata(app) VALUES(?1)", [app])?;
        if let Some(title) = title {
            tx.execute(
                "UPDATE game_metadata SET title=?2,updated=?3 WHERE app=?1",
                params![app, title, now()],
            )?;
        }
        if let Some(art) = art {
            let revision = art.map(|bytes| format!("{:x}", Sha256::digest(bytes)));
            tx.execute(
                "UPDATE game_metadata SET card_art=?2,card_art_revision=?3,updated=?4 WHERE app=?1",
                params![app, art, revision, now()],
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
