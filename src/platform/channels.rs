//! Conversation identity and policy. Group chat is an adapter onto this layer;
//! explicit audiences are internal foundations, not new website features.
use super::*;
use crate::portal::fault;

pub(crate) struct Channel {
    pub id: String,
    pub group: Option<String>,
    expires: Option<u64>,
    participants: BTreeMap<String, Grant>,
}
struct Grant {
    after_message: u64,
    expires: Option<u64>,
}

pub(crate) struct Access {
    pub after_message: u64,
    pub writable: bool,
    moderate: bool,
}
impl Access {
    pub fn sees(&self, id: u64) -> bool {
        id > self.after_message
    }
    pub fn can_delete(&self, email: &str, author: &str) -> bool {
        self.writable && (email == author || self.moderate)
    }
}

impl Channel {
    pub fn load(c: &Connection, id: &str) -> Result<Self> {
        let (group, expires) = c
            .query_row(
                "SELECT default_group,expires FROM chat_channels WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<u64>>(1)?)),
            )
            .optional()?
            .ok_or_else(|| fault(404, "Conversation not found."))?;
        let participants = if group.is_none() {
            let mut s = c.prepare(
                "SELECT email,after_message,expires FROM chat_participants WHERE channel_id=?1",
            )?;
            s.query_map([id], |r| {
                Ok((
                    r.get(0)?,
                    Grant {
                        after_message: r.get(1)?,
                        expires: r.get(2)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            id: id.into(),
            group,
            expires,
            participants,
        })
    }

    // Legacy binaries route by this column. Non-group conversations use a
    // reserved, non-group value so rolling back cannot expose their messages.
    pub fn legacy_group(&self) -> String {
        self.group
            .clone()
            .unwrap_or_else(|| format!("channel:{}", self.id))
    }

    pub fn access(&self, data: &Data, email: &str, inspecting: bool, time: u64) -> Option<Access> {
        let member = data.members.get(email).filter(|m| m.enabled)?;
        if self.expires.is_some_and(|end| end <= time) {
            return None;
        }
        if let Some(group) = &self.group {
            if !data.groups.names.contains_key(group) {
                return None;
            }
            if inspecting {
                return data.groups.operator(email).then_some(Access {
                    after_message: 0,
                    writable: false,
                    moderate: false,
                });
            }
            if data.groups.member(email) == group {
                return Some(Access {
                    after_message: 0,
                    writable: true,
                    moderate: member.admin,
                });
            }
            // Visits are live grants to the default channel, never copied
            // memberships. Ending a visit immediately changes every consumer.
            let (_, visit) = data.active_visit(group, time)?;
            let participant = visit.participants.get(email).filter(|p| !p.ended)?;
            Some(Access {
                after_message: participant.after_message,
                writable: true,
                moderate: false,
            })
        } else {
            if inspecting {
                return None;
            }
            let grant = self.participants.get(email)?;
            if grant.expires.is_some_and(|end| end <= time) {
                return None;
            }
            Some(Access {
                after_message: grant.after_message,
                writable: true,
                moderate: false,
            })
        }
    }
}

pub(crate) fn default_channel(c: &Connection, group: &str) -> Result<String> {
    // The random ID survives renames and is not a private-group ID in disguise.
    c.execute(
        "INSERT INTO chat_channels(id,default_group) SELECT lower(hex(randomblob(16))),?1
        WHERE NOT EXISTS(SELECT 1 FROM chat_channels WHERE default_group=?1)",
        [group],
    )?;
    Ok(c.query_row(
        "SELECT id FROM chat_channels WHERE default_group=?1",
        [group],
        |r| r.get(0),
    )?)
}

/// Called inside the admission transaction so a concurrent message cannot slip
/// between granting access and recording the start of the visible conversation.
pub(crate) fn admission_boundary(c: &Connection, group: &str) -> Result<u64> {
    let channel = default_channel(c, group)?;
    Ok(c.query_row(
        "SELECT coalesce(max(id),0) FROM library_chat WHERE channel_id=?1",
        [channel],
        |r| r.get(0),
    )?)
}

pub(super) fn initialize(c: &mut Connection) -> Result<()> {
    // One atomic, additive migration: IDs, tombstones, request IDs, sequence
    // high-water mark, visit floors and pending notification references survive.
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS library_chat (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        author TEXT NOT NULL, client_id TEXT NOT NULL, text TEXT,
        created INTEGER NOT NULL, UNIQUE(author,client_id));",
    )?;
    let columns = tx
        .prepare("PRAGMA table_info(library_chat)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|c| c == "private_group") {
        tx.execute(
            "ALTER TABLE library_chat ADD COLUMN private_group TEXT NOT NULL DEFAULT 'home'",
            [],
        )?;
    }
    if !columns.iter().any(|c| c == "channel_id") {
        tx.execute(
            "ALTER TABLE library_chat ADD COLUMN channel_id TEXT REFERENCES chat_channels(id)",
            [],
        )?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS chat_channels (
        id TEXT PRIMARY KEY, default_group TEXT UNIQUE, expires INTEGER);
        CREATE TABLE IF NOT EXISTS chat_participants (
        channel_id TEXT NOT NULL REFERENCES chat_channels(id), email TEXT NOT NULL,
        after_message INTEGER NOT NULL DEFAULT 0, expires INTEGER,
        PRIMARY KEY(channel_id,email));
        INSERT INTO chat_channels(id,default_group)
            SELECT lower(hex(randomblob(16))),private_group FROM library_chat
            WHERE channel_id IS NULL AND private_group NOT IN
                (SELECT default_group FROM chat_channels WHERE default_group IS NOT NULL)
            GROUP BY private_group;
        UPDATE library_chat SET channel_id=(SELECT id FROM chat_channels WHERE default_group=private_group)
            WHERE channel_id IS NULL;
        CREATE INDEX IF NOT EXISTS library_chat_channel ON library_chat(channel_id,id);
        CREATE INDEX IF NOT EXISTS library_chat_group ON library_chat(private_group,id);
        CREATE INDEX IF NOT EXISTS library_chat_sender_time ON library_chat(author,created);
        CREATE TRIGGER IF NOT EXISTS chat_legacy_insert AFTER INSERT ON library_chat
        WHEN NEW.channel_id IS NULL BEGIN
            INSERT INTO chat_channels(id,default_group) SELECT lower(hex(randomblob(16))),NEW.private_group
                WHERE NOT EXISTS(SELECT 1 FROM chat_channels WHERE default_group=NEW.private_group);
            UPDATE library_chat SET channel_id=(SELECT id FROM chat_channels WHERE default_group=NEW.private_group)
                WHERE id=NEW.id;
        END;
        CREATE TRIGGER IF NOT EXISTS chat_channel_insert BEFORE INSERT ON library_chat
        WHEN NEW.channel_id IS NOT NULL AND NOT EXISTS(
            SELECT 1 FROM chat_channels WHERE id=NEW.channel_id
                AND coalesce(default_group,'channel:'||id)=NEW.private_group)
        BEGIN SELECT RAISE(ABORT,'chat channel binding mismatch'); END;")?;
    tx.commit()?;
    Ok(())
}

impl Database {
    pub(crate) fn default_chat_channel(&self, group: &str) -> Result<String> {
        let c = self.connection.lock().unwrap();
        if let Some(id) = c
            .query_row(
                "SELECT id FROM chat_channels WHERE default_group=?1",
                [group],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        default_channel(&c, group)
    }
}
