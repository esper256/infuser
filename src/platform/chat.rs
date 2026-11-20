//! Bounded channel messages. Group methods preserve the existing website API.
use super::*;
use crate::portal::fault;
use serde_json::{Value, json};

const RETENTION: u64 = 7 * 24 * 60 * 60;

fn members(db: &Database, tx: &Transaction<'_>, email: &str) -> Result<std::sync::Arc<Data>> {
    let data = db.cached_catalog(tx)?;
    ensure!(
        data.profile_enabled(email),
        fault(403, "Membership is disabled.")
    );
    Ok(data)
}

impl Database {
    pub fn chat_list(&self, email: &str) -> Result<Value> {
        let group = self.catalog()?.groups.member(email).to_owned();
        self.chat_list_group(email, &group, false)
    }
    pub fn chat_list_group(&self, email: &str, group: &str, inspecting: bool) -> Result<Value> {
        self.chat_list_channel(email, &self.default_chat_channel(group)?, inspecting)
    }
    pub fn chat_list_channel(
        &self,
        email: &str,
        channel_id: &str,
        inspecting: bool,
    ) -> Result<Value> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data = members(self, &tx, email)?;
        let channel = channels::Channel::load(&tx, channel_id)?;
        let access = channel
            .access(&data, email, inspecting, now())
            .ok_or_else(|| fault(403, "Group access denied."))?;
        let mut statement = tx.prepare("SELECT id,author,text,created,screenshot FROM library_chat WHERE channel_id=?2 AND text IS NOT NULL AND created>?1 AND id>?3 ORDER BY id")?;
        let rows = statement.query_map(
            params![
                now().saturating_sub(RETENTION),
                channel.id,
                access.after_message
            ],
            |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        let mut messages = Vec::new();
        for row in rows {
            let (id, author, text, created, screenshot) = row?;
            if let Some(member) = data.members.get(&author).filter(|m| m.enabled) {
                messages.push(json!({"id":id,"text":text,"created":created,"screenshot":screenshot,
                    "author":{"id":data.player_id(&author),"name":member.name,"avatar":member.avatar},
                    "can_delete":access.can_delete(email, &author)}));
            }
        }
        let mut read_rows = tx.prepare(
            "SELECT reader, message_id FROM chat_reads WHERE channel_id=?1 ORDER BY message_id, reader",
        )?;
        let mut reads = Vec::new();
        for row in read_rows.query_map([&channel.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
        })? {
            let (reader, message) = row?;
            if let Some(member) = data.members.get(&reader).filter(|m| m.enabled) {
                reads.push(json!({"id":data.player_id(&reader),"name":member.name,"avatar":member.avatar,"message":message}));
            }
        }
        Ok(json!({"messages":messages,"reads":reads}))
    }

    pub fn chat_advance_read(&self, email: &str, group: &str, message: u64) -> Result<Value> {
        let channel_id = self.default_chat_channel(group)?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = members(self, &tx, email)?;
        let channel = channels::Channel::load(&tx, &channel_id)?;
        let access = channel
            .access(&data, email, false, now())
            .ok_or_else(|| fault(403, "Group access denied."))?;
        let visible = tx
            .query_row(
                "SELECT 1 FROM library_chat WHERE id=?1 AND channel_id=?2 AND text IS NOT NULL AND created>?3 AND id>?4",
                params![message, channel.id, now().saturating_sub(RETENTION), access.after_message],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        ensure!(visible, fault(404, "Message not found."));
        tx.execute(
            "INSERT INTO chat_reads(channel_id, reader, message_id) VALUES(?1,?2,?3)
             ON CONFLICT(channel_id, reader) DO UPDATE SET message_id=excluded.message_id
             WHERE excluded.message_id > chat_reads.message_id",
            params![channel.id, email, message],
        )?;
        let stored: u64 = tx.query_row(
            "SELECT message_id FROM chat_reads WHERE channel_id=?1 AND reader=?2",
            params![channel.id, email],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(json!({"message":stored}))
    }

    pub fn chat_send(&self, email: &str, client_id: &str, text: &str) -> Result<Value> {
        let group = self.catalog()?.groups.member(email).to_owned();
        self.chat_send_group(email, &group, client_id, text)
    }
    pub fn chat_send_group(
        &self,
        email: &str,
        group: &str,
        client_id: &str,
        text: &str,
    ) -> Result<Value> {
        self.chat_send_channel(email, &self.default_chat_channel(group)?, client_id, text)
    }
    pub fn chat_send_channel(
        &self,
        email: &str,
        channel_id: &str,
        client_id: &str,
        text: &str,
    ) -> Result<Value> {
        self.chat_send_attachment(email, channel_id, client_id, text, None)
    }
    pub fn chat_send_attachment(
        &self,
        email: &str,
        channel_id: &str,
        client_id: &str,
        text: &str,
        screenshot: Option<&str>,
    ) -> Result<Value> {
        let text = text.trim();
        ensure!(
            (!text.is_empty() || screenshot.is_some())
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
        let channel = channels::Channel::load(&tx, channel_id)?;
        let access = channel
            .access(&data, email, false, now())
            .filter(|a| a.writable)
            .ok_or_else(|| fault(403, "Group access ended."))?;
        let time = now();
        tx.execute(
            "DELETE FROM library_chat WHERE channel_id=?2 AND created<=?1",
            params![time.saturating_sub(RETENTION), channel.id],
        )?;
        // Retrying a lost response must not create a duplicate or resurrect a removed message.
        if let Some((id, original, original_channel, original_screenshot)) = tx
            .query_row(
                "SELECT id,text,channel_id,screenshot FROM library_chat WHERE author=?1 AND client_id=?2",
                params![email, client_id],
                |r| {
                    Ok((
                        r.get::<_, u64>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
        {
            ensure!(
                original_channel == channel.id
                    && access.sees(id)
                    && original.as_deref().is_none_or(|s| s == text && original_screenshot.as_deref()==screenshot),
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
        if let Some(id) = screenshot {
            screenshots::owned(&tx, email, id)?;
        }
        tx.execute(
            "INSERT INTO library_chat(author,client_id,text,created,private_group,channel_id,screenshot) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![email, client_id, text, time, channel.legacy_group(), channel.id, screenshot],
        )?;
        let id = tx.last_insert_rowid();
        crate::notifications::enqueue(&tx, &data, email, &channel, id, time)?;
        tx.execute("DELETE FROM library_chat WHERE channel_id=?1 AND id NOT IN (SELECT id FROM library_chat WHERE channel_id=?1 ORDER BY id DESC LIMIT 100)", [&channel.id])?;
        tx.commit()?;
        Ok(json!({"id":id}))
    }

    pub fn chat_remove(&self, email: &str, id: u64) -> Result<()> {
        let group = self.catalog()?.groups.member(email).to_owned();
        self.chat_remove_group(email, &group, id)
    }
    pub fn chat_remove_group(&self, email: &str, group: &str, id: u64) -> Result<()> {
        self.chat_remove_channel(email, &self.default_chat_channel(group)?, id)
    }
    pub fn chat_remove_channel(&self, email: &str, channel_id: &str, id: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = members(self, &tx, email)?;
        let channel = channels::Channel::load(&tx, channel_id)?;
        let access = channel
            .access(&data, email, false, now())
            .filter(|a| a.writable)
            .ok_or_else(|| fault(403, "Group access ended."))?;
        ensure!(access.sees(id), fault(404, "Message not found."));
        let author: String = tx
            .query_row(
                "SELECT author FROM library_chat WHERE id=?1 AND channel_id=?2",
                params![id, channel.id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| fault(404, "Message not found."))?;
        ensure!(
            access.can_delete(email, &author),
            fault(403, "Only the sender or an admin can remove this message.")
        );
        // Keep a bounded tombstone for retries and rate limiting.
        tx.execute("UPDATE library_chat SET text=NULL WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
}
