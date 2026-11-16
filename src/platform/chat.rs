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
    let columns = c
        .prepare("PRAGMA table_info(library_chat)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|c| c == "private_group") {
        c.execute(
            "ALTER TABLE library_chat ADD COLUMN private_group TEXT NOT NULL DEFAULT 'home'",
            [],
        )?;
    }
    c.execute(
        "CREATE INDEX IF NOT EXISTS library_chat_group ON library_chat(private_group,id)",
        [],
    )?;
    Ok(())
}

fn members(db: &Database, c: &Connection, email: &str) -> Result<Data> {
    let data = db.cached_catalog(c)?;
    ensure!(
        data.members.get(email).is_some_and(|m| m.enabled),
        fault(403, "Membership is disabled.")
    );
    Ok(data)
}

impl Database {
    pub fn chat_list(&self, email: &str) -> Result<Value> {
        let group = self.read()?.groups.member(email).to_owned();
        self.chat_list_group(email, &group, false)
    }
    pub fn chat_list_group(&self, email: &str, group: &str, inspecting: bool) -> Result<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data = members(self, &tx, email)?;
        ensure!(
            if inspecting {
                data.groups.operator(email)
            } else {
                data.groups.member(email) == group
            },
            fault(403, "Group access denied.")
        );
        let mut statement = tx.prepare("SELECT id,author,text,created FROM library_chat WHERE private_group=?2 AND text IS NOT NULL AND created>?1 ORDER BY id")?;
        let rows = statement.query_map(params![now().saturating_sub(RETENTION), group], |r| {
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
            if let Some(member) = data
                .members
                .get(&author)
                .filter(|m| m.enabled && data.groups.member(&author) == group)
            {
                messages.push(json!({"id":id,"text":text,"created":created,
                    "author":{"id":hash(&author),"name":member.name,"avatar":member.avatar},
                    "can_delete":!inspecting&&(author==email||data.members[email].admin)}));
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
        let data = members(self, &tx, email)?;
        let group = data.groups.member(email);
        let time = now();
        tx.execute(
            "DELETE FROM library_chat WHERE private_group=?2 AND created<=?1",
            params![time.saturating_sub(RETENTION), group],
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
            "INSERT INTO library_chat(author,client_id,text,created,private_group) VALUES(?1,?2,?3,?4,?5)",
            params![email, client_id, text, time,group],
        )?;
        let id = tx.last_insert_rowid();
        crate::notifications::enqueue(&tx, &data, email, group, id, time)?;
        tx.execute("DELETE FROM library_chat WHERE private_group=?1 AND id NOT IN (SELECT id FROM library_chat WHERE private_group=?1 ORDER BY id DESC LIMIT 100)", [group])?;
        tx.commit()?;
        Ok(json!({"id":id}))
    }

    pub fn chat_remove(&self, email: &str, id: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = members(self, &tx, email)?;
        let author: String = tx
            .query_row(
                "SELECT author FROM library_chat WHERE id=?1 AND private_group=?2",
                params![id, data.groups.member(email)],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| fault(404, "Message not found."))?;
        ensure!(
            author == email || data.members[email].admin,
            fault(403, "Only the sender or an admin can remove this message.")
        );
        // Keep a bounded tombstone for retries and rate limiting.
        tx.execute("UPDATE library_chat SET text=NULL WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}
