//! A bounded private gallery. Shared access is derived from live messages/reactions.
use super::*;
use crate::portal::fault;
use serde_json::{Value, json};

pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS screenshots (
      sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
      owner TEXT NOT NULL, app TEXT NOT NULL, created INTEGER NOT NULL,
      width INTEGER NOT NULL, height INTEGER NOT NULL, image BLOB NOT NULL, thumbnail BLOB NOT NULL);
      CREATE INDEX IF NOT EXISTS screenshots_owner ON screenshots(owner,sequence);")?;
    let columns = c
        .prepare("PRAGMA table_info(library_chat)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|s| s == "screenshot") {
        c.execute("ALTER TABLE library_chat ADD COLUMN screenshot TEXT", [])?;
    }
    Ok(())
}
// Gallery membership is always the ten most recent captures. Older pixels can
// remain on disk exclusively for live reactions without reappearing in chat.
fn in_gallery(c: &Connection, owner: &str, id: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT ?2 IN (SELECT id FROM screenshots WHERE owner=?1 ORDER BY sequence DESC LIMIT 10)",
        params![owner, id],
        |row| row.get(0),
    )?)
}

pub(crate) fn prune(c: &Connection, data: &Data, owner: &str) -> Result<()> {
    let attached: Vec<_> = data
        .reactions
        .values()
        .filter_map(|game| game.get(owner)?.screenshot.as_deref())
        .collect();
    c.execute(
        "DELETE FROM screenshots WHERE owner=?1
         AND sequence NOT IN (SELECT sequence FROM screenshots WHERE owner=?1 ORDER BY sequence DESC LIMIT 10)
         AND id NOT IN (SELECT value FROM json_each(?2))",
        params![owner, serde_json::to_string(&attached)?],
    )?;
    Ok(())
}

pub(crate) fn owned(c: &Connection, owner: &str, id: &str) -> Result<()> {
    ensure!(
        in_gallery(c, owner, id)?,
        fault(
            422,
            "That screenshot expired or belongs to another profile. Choose another."
        )
    );
    Ok(())
}
pub struct ScreenshotUpload {
    pub id: String,
    pub image: Vec<u8>,
    pub thumbnail: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
impl Database {
    pub fn screenshot_store(&self, owner: &str, app: &str, upload: ScreenshotUpload) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = self.cached_catalog(&tx)?;
        ensure!(
            data.profile_enabled(owner),
            fault(403, "Profile unavailable.")
        );
        if let Some((who, image, thumb)) = tx
            .query_row(
                "SELECT owner,image,thumbnail FROM screenshots WHERE id=?1",
                [&upload.id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?
        {
            ensure!(
                who == owner && image == upload.image && thumb == upload.thumbnail,
                fault(409, "Screenshot ID already used.")
            );
            return Ok(());
        }
        let last: Option<u64> = tx.query_row(
            "SELECT max(created) FROM screenshots WHERE owner=?1",
            [owner],
            |r| r.get(0),
        )?;
        ensure!(
            last.is_none_or(|t| now() > t),
            fault(429, "Wait a moment before taking another screenshot.")
        );
        tx.execute("INSERT INTO screenshots(id,owner,app,created,width,height,image,thumbnail) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![upload.id,owner,app,now(),upload.width,upload.height,upload.image,upload.thumbnail])?;
        prune(&tx, &data, owner)?;
        tx.commit()?;
        Ok(())
    }
    pub fn screenshot_list(&self, owner: &str) -> Result<Value> {
        let c = self.connection.lock().unwrap();
        let mut s=c.prepare("SELECT id,app,created,width,height FROM screenshots WHERE owner=?1 ORDER BY sequence DESC LIMIT 10")?;
        let rows=s.query_map([owner],|r|Ok(json!({"id":r.get::<_,String>(0)?,"app":r.get::<_,String>(1)?,"created":r.get::<_,u64>(2)?,"width":r.get::<_,u32>(3)?,"height":r.get::<_,u32>(4)?})))?;
        Ok(json!({"screenshots":rows.collect::<rusqlite::Result<Vec<_>>>()?,"limit":10}))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn screenshot_image(
        &self,
        viewer: &str,
        id: &str,
        group: &str,
        inspecting: bool,
        message: Option<u64>,
        runtime: Option<&str>,
        thumbnail: bool,
        load_bytes: bool,
    ) -> Result<Option<Vec<u8>>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data = self.cached_catalog(&tx)?;
        ensure!(
            data.profile_enabled(viewer),
            fault(403, "Profile unavailable.")
        );
        let owner: String = tx
            .query_row("SELECT owner FROM screenshots WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| fault(404, "Screenshot expired."))?;
        let gallery = in_gallery(&tx, &owner, id)?;
        let mut allowed = gallery && owner == viewer && !inspecting;
        if !allowed
            && gallery
            && let Some(message) = message
        {
            let row=tx.query_row("SELECT channel_id,author FROM library_chat WHERE id=?1 AND screenshot=?2 AND text IS NOT NULL AND created>?3",params![message,id,now().saturating_sub(7*86400)],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()?;
            if let Some((channel, author)) = row {
                let channel = channels::Channel::load(&tx, &channel)?;
                allowed = channel.group.as_deref() == Some(group)
                    && data.profile_enabled(&author)
                    && channel
                        .access(&data, viewer, inspecting, now())
                        .is_some_and(|a| a.sees(message));
            }
        }
        if !allowed && let Some(runtime) = runtime {
            // The portal already checks that this deployment is visible in the selected context.
            allowed = data.reactions.get(runtime).is_some_and(|reactions| {
                reactions.iter().any(|(author, r)| {
                    data.profile_enabled(author) && r.screenshot.as_deref() == Some(id)
                })
            });
        }
        ensure!(allowed, fault(404, "Screenshot unavailable."));
        if !load_bytes {
            return Ok(None);
        }
        Ok(Some(tx.query_row(
            if thumbnail {
                "SELECT thumbnail FROM screenshots WHERE id=?1"
            } else {
                "SELECT image FROM screenshots WHERE id=?1"
            },
            [id],
            |r| r.get(0),
        )?))
    }
}
