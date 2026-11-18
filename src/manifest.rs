//! App intent is explicit; engine choice never determines multiplayer topology.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub app_id: String,
    pub title: String,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<crate::sdk_contract::Requirement>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub play_modes: Vec<PlayMode>,
    pub solo_runtime: Option<SoloRuntime>,
    pub multiplayer: Option<Multiplayer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_intents: Vec<crate::launch_intent::Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_start: Option<ServerStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_saves: Option<crate::cloud::Declaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<crate::dedicated::Declaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_connection: Option<crate::dedicated::SocketDeclaration>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerStart {
    OnDemand,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    Solo,
    Multiplayer,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SoloRuntime {
    ClientOnly,
    SharedServer,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    PeerHosted,
    Relayed,
    Dedicated,
    PersistentWorld,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Webrtc,
    Websocket,
    Tcp,
    Udp,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectConnect {
    Optional,
    Disabled,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayFallback {
    Required,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinPolicy {
    Anytime,
    BetweenRounds,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Multiplayer {
    pub mode: Mode,
    pub transport: Transport,
    pub max_players: u32,
    pub join_policy: JoinPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<PeerAdapter>,
    pub direct_connect: Option<DirectConnect>,
    pub relay_fallback: Option<RelayFallback>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeerAdapter {
    Peerjs,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Manifest {
    pub fn dedicated_sessions(&self) -> bool {
        self.sessions.is_some()
            && self.sdk.as_ref().is_some_and(|sdk| {
                [crate::connections::CAPABILITY, crate::dedicated::CAPABILITY]
                    .iter()
                    .all(|c| sdk.required_capabilities.iter().any(|v| v == c))
            })
    }
    pub fn server_activity(&self, activity: &crate::dedicated::Activity) -> bool {
        match activity {
            crate::dedicated::Activity::Solo => {
                self.play_modes.contains(&PlayMode::Solo)
                    && matches!(self.solo_runtime, Some(SoloRuntime::SharedServer))
            }
            crate::dedicated::Activity::Multiplayer => {
                self.play_modes.contains(&PlayMode::Multiplayer)
            }
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(sdk) = &self.sdk {
            sdk.validate()?;
            if sdk
                .required_capabilities
                .iter()
                .any(|c| c == crate::connections::CAPABILITY || c == crate::dedicated::CAPABILITY)
            {
                ensure!(
                    self.sessions.is_some() && self.dedicated_sessions(),
                    "managed connection capabilities require a sessions declaration and both sessions.v1 and connections.websocket.v1"
                );
            }
        }
        ensure!(
            self.launch_intents.len() <= 1
                && (self.launch_intents.is_empty()
                    || self
                        .multiplayer
                        .as_ref()
                        .is_some_and(|m| (m.mode == Mode::PeerHosted
                            && m.transport == Transport::Webrtc)
                            || (self.dedicated_sessions()
                                && m.mode == Mode::Dedicated
                                && m.transport == Transport::Websocket))),
            "join_friend requires peer-hosted WebRTC or managed dedicated WebSocket sessions; declare it at most once"
        );
        if let Some(sessions) = &self.sessions {
            ensure!(
                self.dedicated_sessions(),
                "sessions requires sessions.v1 and connections.websocket.v1 SDK capabilities"
            );
            ensure!(
                (1..=256).contains(&sessions.max_players_per_session),
                "session capacity must be 1–256"
            );
            ensure!(
                self.server_start == Some(ServerStart::OnDemand),
                "sessions requires on-demand server startup"
            );
            ensure!(
                self.multiplayer.as_ref().is_none_or(
                    |m| m.mode == Mode::Dedicated && m.transport == Transport::Websocket
                ),
                "managed sessions currently support dedicated WebSocket servers"
            );
            ensure!(
                self.multiplayer
                    .as_ref()
                    .is_none_or(|m| m.max_players == sessions.max_players_per_session),
                "multiplayer.max_players must match max_players_per_session"
            );
        }
        if let Some(connection) = &self.server_connection {
            ensure!(
                self.dedicated_sessions(),
                "server_connection requires managed sessions"
            );
            connection.validate()?;
        }
        ensure!(
            self.server_start.is_none()
                || matches!(self.solo_runtime, Some(SoloRuntime::SharedServer))
                || self
                    .multiplayer
                    .as_ref()
                    .is_some_and(|m| matches!(m.mode, Mode::Dedicated | Mode::PersistentWorld)),
            "server_start applies only to games with a managed backend"
        );
        if let Some(saves) = &self.cloud_saves {
            saves.validate()?;
        }
        ensure!(
            self.schema_version == 1,
            "unsupported manifest schema_version"
        );
        ensure!(crate::config::slug(&self.app_id), "invalid app_id");
        ensure!(
            self.tags.len() <= 8 && self.tags.iter().all(|t| crate::tags::valid(t)),
            "use up to 8 lowercase tag IDs, each 1–32 letters, numbers or dashes"
        );
        ensure!(
            self.tags
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.tags.len(),
            "duplicate tags"
        );
        ensure!(
            !self.title.trim().is_empty() && self.title.chars().count() <= 80,
            "title must contain 1-80 characters"
        );
        ensure!(
            !self.play_modes.is_empty()
                && self.play_modes.len() <= 2
                && (self.play_modes.len() == 1 || self.play_modes[0] != self.play_modes[1]),
            "play_modes must contain solo, multiplayer, or both without duplicates"
        );
        let solo = self.play_modes.contains(&PlayMode::Solo);
        let multiplayer = self.play_modes.contains(&PlayMode::Multiplayer);
        ensure!(
            solo == self.solo_runtime.is_some(),
            "declare solo_runtime exactly when solo is supported"
        );
        ensure!(
            multiplayer == self.multiplayer.is_some(),
            "declare multiplayer settings exactly when multiplayer is supported"
        );
        if let Some(m) = &self.multiplayer {
            ensure!(
                m.adapter.is_none()
                    || (m.mode == Mode::PeerHosted && m.transport == Transport::Webrtc),
                "PeerJS adapter requires peer_hosted WebRTC"
            );
            ensure!(
                (2..=4096).contains(&m.max_players),
                "max_players must be between 2 and 4096"
            );
            if m.mode == Mode::PeerHosted {
                ensure!(
                    m.relay_fallback.is_some() && m.direct_connect.is_some(),
                    "peer_hosted requires relay_fallback=required and an explicit direct_connect policy"
                );
                ensure!(
                    matches!(m.transport, Transport::Webrtc | Transport::Websocket),
                    "peer_hosted must use an Infuser relay-capable transport; raw TCP/UDP and LAN-only connections are unsupported"
                );
            } else {
                ensure!(
                    m.relay_fallback.is_none() && m.direct_connect.is_none(),
                    "direct_connect and relay_fallback apply only to peer_hosted games"
                );
                if m.mode == Mode::Relayed {
                    ensure!(
                        m.transport == Transport::Websocket,
                        "relayed games use the Infuser WebSocket relay protocol"
                    );
                }
            }
        }
        Ok(())
    }
}
