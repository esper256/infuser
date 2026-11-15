//! Optional, one-use launch requests. An intent is never authority to create a room.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    JoinFriend,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    JoinFriend {
        session_id: String,
        player_id: String,
    },
}
impl Intent {
    pub fn validate(&self, manifest: &crate::manifest::Manifest) -> Result<()> {
        ensure!(
            manifest.launch_intents.contains(&Kind::JoinFriend),
            crate::portal::fault(422, "this game does not support Join friend")
        );
        let Self::JoinFriend {
            session_id,
            player_id,
        } = self;
        ensure!(
            session_id.len() == 32
                && session_id.bytes().all(|b| b.is_ascii_hexdigit())
                && !player_id.is_empty()
                && player_id.len() <= 96
                && player_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            crate::portal::fault(400, "invalid Join friend target")
        );
        Ok(())
    }
}
pub fn capabilities() -> Value {
    json!({"version":1,"experimental":true,"kinds":["join_friend"],"delivery":"consume_once"})
}
