//! Dedicated gameplay sessions: game-owned admission, platform-owned identity.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CAPABILITY: &str = "sessions.v1";
pub const REPORT_SECONDS: u64 = 45;
pub const RECONNECT_SECONDS: u64 = 30;
pub const MAX_SESSIONS: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    #[default]
    Shared,
    Multiple,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    #[serde(default)]
    pub policy: Policy,
    pub max_players_per_session: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SocketDeclaration {
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default = "default_message_bytes")]
    pub max_message_bytes: usize,
}
fn default_path() -> String {
    "/".into()
}
fn default_message_bytes() -> usize {
    64 * 1024
}
impl Default for SocketDeclaration {
    fn default() -> Self {
        Self {
            path: default_path(),
            protocols: vec![],
            max_message_bytes: default_message_bytes(),
        }
    }
}
impl SocketDeclaration {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.path.starts_with('/')
                && !self.path.starts_with("//")
                && self.path.len() <= 256
                && self
                    .path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
                && !self.path.split('/').any(|p| p == "." || p == ".."),
            "invalid WebSocket path"
        );
        ensure!(
            (1024..=crate::connections::MAX_MESSAGE_BYTES).contains(&self.max_message_bytes),
            "WebSocket message limit must be 1024–1048576 bytes"
        );
        ensure!(
            self.protocols.len() <= 8
                && self.protocols.iter().all(|p| !p.is_empty()
                    && p.len() <= 64
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)))
                && self.protocols.iter().collect::<BTreeSet<_>>().len() == self.protocols.len(),
            "invalid WebSocket protocols"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Play,
    Create,
    Join,
    Resume,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Solo,
    Multiplayer,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default)]
    pub action: Action,
    pub activity: Option<Activity>,
    pub target: Option<Target>,
}
pub fn session_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub label: String,
    pub activity: Activity,
    pub joinable: bool,
    pub max_players: u32,
    #[serde(default)]
    pub connections: Vec<String>,
}
impl Session {
    pub fn validate(&self, declaration: &Declaration) -> Result<()> {
        ensure!(
            session_id(&self.id),
            "session IDs must be fresh 32-character lowercase hex IDs"
        );
        ensure!(
            !self.label.trim().is_empty()
                && self.label.chars().count() <= 80
                && !self.label.chars().any(char::is_control),
            "session label must contain 1–80 printable characters"
        );
        ensure!(
            self.max_players > 0 && self.max_players <= declaration.max_players_per_session,
            "session exceeds declared capacity"
        );
        ensure!(
            self.activity != Activity::Solo || !self.joinable,
            "single-player sessions cannot be joinable"
        );
        ensure!(
            self.connections.len() <= self.max_players as usize
                && self.connections.iter().collect::<BTreeSet<_>>().len() == self.connections.len(),
            "session has duplicate connections or exceeds capacity"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scope {
    pub app_id: String,
    pub build_id: String,
    pub instance_id: String,
    pub group_id: String,
    pub exp: u64,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub scope: Scope,
    pub seq: u64,
    pub seen: u64,
    pub sessions: BTreeMap<String, Session>,
    pub retired: BTreeSet<String>,
}
#[derive(Default)]
pub struct Directory(pub BTreeMap<String, Report>);

#[derive(Debug)]
pub struct Fault {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message)
    }
}
impl std::error::Error for Fault {}
pub fn fault(status: u16, code: &'static str, message: &'static str) -> anyhow::Error {
    Fault {
        status,
        code,
        message,
    }
    .into()
}

pub fn capabilities() -> serde_json::Value {
    serde_json::json!({"version":1,"status":"candidate","policies":["shared","multiple"],"max_sessions":MAX_SESSIONS,
        "report_interval_seconds":10,"report_expiry_seconds":REPORT_SECONDS,"reconnect_grace_seconds":RECONNECT_SECONDS,
        "max_message_bytes":crate::connections::MAX_MESSAGE_BYTES,"max_connections_per_launch":crate::connections::MAX_PER_LAUNCH,
        "max_active_connections":crate::connections::MAX_CONNECTIONS})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, SoloRuntime};

    #[test]
    fn browser_solo_and_server_multiplayer_are_independent() -> Result<()> {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../examples/dedicated-node/infuser.game.json"))?;
        value["app_id"] = "lab".into();
        value["schema_version"] = 1.into();
        value.as_object_mut().unwrap().remove("web_dir");
        value.as_object_mut().unwrap().remove("server");
        let mut manifest: Manifest = serde_json::from_value(value)?;
        manifest.solo_runtime = Some(SoloRuntime::ClientOnly);
        manifest.validate()?;
        assert!(manifest.dedicated_sessions());
        assert!(manifest.server_activity(&Activity::Multiplayer));
        assert!(!manifest.server_activity(&Activity::Solo));
        manifest.solo_runtime = Some(SoloRuntime::SharedServer);
        manifest.validate()?;
        assert!(manifest.server_activity(&Activity::Solo));
        Ok(())
    }
}
