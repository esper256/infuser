//! A bounded shared room, independent of the legacy catalog and game saves.
use super::*;
use crate::portal::fault;
use serde_json::{Value, json};

const RETENTION: u64 = 7 * 24 * 60 * 60;

pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS library_chat (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        author TEXT NOT NULL, client_id TEXT NOT NULL, text TEXT,
        created INTEGER NOT NULL, UNIQUE(author,client_id)
    );",
    )?;
    Ok(())
}

fn members(c: &Connection, email: &str) -> Result<Data> {
    let raw: String = c.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let data: Data = serde_json::from_str(&raw)?;
    ensure!(
        data.members.get(email).is_some_and(|m| m.enabled),
        fault(403, "Membership is disabled.")
    );
    Ok(data)
}

impl Database {
    pub fn chat_list(&self, email: &str) -> Result<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data = members(&tx, email)?;
        let mut statement = tx.prepare("SELECT id,author,text,created FROM library_chat WHERE text IS NOT NULL AND created>?1 ORDER BY id")?;
        let rows = statement.query_map([now().saturating_sub(RETENTION)], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u64>(3)?,
            ))
        })?;
        let mut messages = Vec::new();
        for row in rows {
            let (id, author, text, created) = row?;
            if let Some(member) = data.members.get(&author).filter(|m| m.enabled) {
                messages.push(json!({"id":id,"text":text,"created":created,
                    "author":{"id":hash(&author),"name":member.name,"avatar":member.avatar},
                    "can_delete":author==email||data.members[email].admin}));
            }
        }
        Ok(json!({"messages":messages}))
    }

    pub fn chat_send(&self, email: &str, client_id: &str, text: &str) -> Result<Value> {
        let text = text.trim();
        ensure!(
            !text.is_empty()
                && text.chars().count() <= 280
                && !text.chars().any(|c| c.is_control() && c != '\n'),
            fault(422, "Use 1–280 characters.")
        );
        ensure!(
            (16..=64).contains(&client_id.len())
                && client_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            fault(422, "Invalid message ID.")
        );
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        members(&tx, email)?;
        let time = now();
        tx.execute(
            "DELETE FROM library_chat WHERE created<=?1",
            [time.saturating_sub(RETENTION)],
        )?;
        // Retrying a lost response must not create a duplicate or resurrect a removed message.
        if let Some((id, original)) = tx
            .query_row(
                "SELECT id,text FROM library_chat WHERE author=?1 AND client_id=?2",
                params![email, client_id],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?
        {
            ensure!(
                original.as_deref().is_none_or(|s| s == text),
                fault(409, "Message ID already used.")
            );
            return Ok(json!({"id":id}));
        }
        let recent: u64 = tx.query_row(
            "SELECT count(*) FROM library_chat WHERE author=?1 AND created>?2",
            params![email, time.saturating_sub(60)],
            |r| r.get(0),
        )?;
        ensure!(
            recent < 10,
            fault(429, "Give it a moment. Try again in a minute.")
        );
        tx.execute(
            "INSERT INTO library_chat(author,client_id,text,created) VALUES(?1,?2,?3,?4)",
            params![email, client_id, text, time],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute("DELETE FROM library_chat WHERE id NOT IN (SELECT id FROM library_chat ORDER BY id DESC LIMIT 100)", [])?;
        tx.commit()?;
        Ok(json!({"id":id}))
    }

    pub fn chat_remove(&self, email: &str, id: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = members(&tx, email)?;
        let author: String = tx
            .query_row("SELECT author FROM library_chat WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| fault(404, "Message not found."))?;
        ensure!(
            author == email || data.members[email].admin,
            fault(403, "Only the sender or an adult can remove this message.")
        );
        // Keep a bounded tombstone for retries and rate limiting.
        tx.execute("UPDATE library_chat SET text=NULL WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}
