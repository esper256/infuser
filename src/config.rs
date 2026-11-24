use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub controller: Settings,
    #[serde(default)]
    pub games: BTreeMap<String, Game>,
    #[serde(default)]
    pub sites: BTreeMap<String, Site>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub state_dir: PathBuf,
    pub diagnostics_dir: Option<PathBuf>,
    pub token_file: PathBuf,
    pub public_host: String,
    pub domain: String,
    pub memory_budget_mb: u64,
    pub cpu_budget_millis: u64,
    pub disk_reserve_mb: u64,
    pub port_start: u16,
    pub port_end: u16,
    pub api_port: u16,
    pub publish_ip: Ipv4Addr,
    pub docker_socket: PathBuf,
    pub lease_seconds: u64,
    pub tick_seconds: u64,
    pub retry_seconds: u64,
    /// Google session validation endpoint, reachable only on this host.
    pub auth_url: String,
    /// Optional Unix socket for the privileged, narrowly scoped firewall helper.
    pub firewall_socket: Option<PathBuf>,
    pub content_budget_mb: u64,
    /// Private publication fixture archive; zero disables capture.
    pub publish_capture_mb: u64,
    pub cloud_save_budget_mb: u64,
    pub turn: Option<Turn>,
    /// Optional private PeerServer listener on IPv4 loopback; never a public port.
    pub peerjs_port: Option<u16>,
    /// Optional private LiveKit API; media ports are configured in its sidecar.
    pub voice: Option<crate::voice::Settings>,
    /// Required before creating groups; older binaries reject this configuration.
    pub private_groups: bool,
    /// Prevent older daemons from serving visit deployments without their access rules.
    pub visits: bool,
    /// Optional managed Node source packaging; older daemons reject this setting.
    pub node_packaging: Option<crate::server_package::Settings>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub urls: Vec<String>,
    pub secret_file: PathBuf,
    pub relay_port_start: u16,
    pub relay_port_end: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            state_dir: "/var/lib/infuser".into(),
            diagnostics_dir: None,
            token_file: "/etc/infuser/control.secret".into(),
            public_host: "games.example.com".into(),
            domain: "games.example.com".into(),
            memory_budget_mb: 8192,
            cpu_budget_millis: 1500,
            disk_reserve_mb: 4096,
            port_start: 8000,
            port_end: 9000,
            api_port: 7777,
            publish_ip: Ipv4Addr::UNSPECIFIED,
            docker_socket: "/var/run/docker.sock".into(),
            lease_seconds: 90,
            tick_seconds: 2,
            retry_seconds: 30,
            auth_url: "http://127.0.0.1:4180/oauth2/auth".into(),
            firewall_socket: None,
            content_budget_mb: 2048,
            publish_capture_mb: 0,
            cloud_save_budget_mb: 512,
            turn: None,
            peerjs_port: None,
            voice: None,
            private_groups: false,
            visits: false,
            node_packaging: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Game {
    pub image: String,
    pub container_port: u16,
    #[serde(default = "memory")]
    pub memory_mb: u64,
    #[serde(default = "cpu")]
    pub cpu_millis: u64,
    #[serde(default = "protocols")]
    pub protocols: Vec<String>,
    #[serde(default = "idle")]
    pub idle_seconds: u64,
    #[serde(default = "startup")]
    pub startup_seconds: u64,
    #[serde(default = "stop")]
    pub stop_seconds: u64,
    #[serde(default = "leases")]
    pub max_leases: usize,
    #[serde(default = "user")]
    pub user: String,
    #[serde(default)]
    pub persistent: bool,
    /// Web servers use authenticated Caddy routing, via loopback or an internal bridge.
    #[serde(default)]
    pub web: bool,
    #[serde(default, skip_serializing_if = "NetworkPolicy::is_default")]
    pub network: NetworkPolicy,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    #[default]
    Internet,
    Internal,
}
impl NetworkPolicy {
    fn is_default(&self) -> bool {
        *self == Self::Internet
    }
}
fn memory() -> u64 {
    256
}
fn cpu() -> u64 {
    500
}
fn protocols() -> Vec<String> {
    vec!["tcp".into()]
}
fn idle() -> u64 {
    120
}
fn startup() -> u64 {
    90
}
fn stop() -> u64 {
    30
}
fn leases() -> usize {
    32
}
fn user() -> String {
    "1000:1000".into()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub root: PathBuf,
    #[serde(default)]
    pub spa: bool,
    #[serde(default)]
    pub cross_origin_isolation: bool,
}

pub fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !s.ends_with('-')
}

pub fn hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let s = &self.controller;
        if let Some(path) = &s.diagnostics_dir {
            ensure!(
                path.is_absolute()
                    && !path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                    && !path.starts_with(&s.state_dir),
                "diagnostics_dir must be an absolute path outside state_dir"
            );
        }
        if let Some(voice) = &s.voice {
            ensure!(
                voice.key_file.is_absolute(),
                "voice key_file must be absolute"
            );
            ensure!(
                voice.port >= 1024
                    && voice.port != s.api_port
                    && s.peerjs_port != Some(voice.port)
                    && !(s.port_start..=s.port_end).contains(&voice.port)
                    && s.turn.as_ref().is_none_or(
                        |t| !(t.relay_port_start..=t.relay_port_end).contains(&voice.port)
                    )
                    && reqwest::Url::parse(&s.auth_url)?.port_or_known_default()
                        != Some(voice.port),
                "invalid private voice API port or overlap"
            );
        }
        ensure!(
            !s.visits || s.private_groups,
            "visits require private_groups"
        );
        if let Some(packaging) = &s.node_packaging {
            packaging.validate()?;
            ensure!(
                s.memory_budget_mb >= crate::server_package::BUILD_MEMORY_MB
                    && s.cpu_budget_millis >= crate::server_package::BUILD_CPU_MILLIS,
                "host budget must accommodate one Node packaging worker"
            );
        }
        if let Some(port) = s.peerjs_port {
            ensure!(s.turn.is_some(), "PeerJS requires TURN");
            ensure!(
                port >= 1024 && port != s.api_port && !(s.port_start..=s.port_end).contains(&port),
                "invalid PeerServer port or overlap"
            );
            let turn = s.turn.as_ref().unwrap();
            ensure!(
                !(turn.relay_port_start..=turn.relay_port_end).contains(&port),
                "PeerServer overlaps TURN relay range"
            );
            for url in &turn.urls {
                ensure!(
                    url.split('?')
                        .next()
                        .and_then(|v| v.rsplit(':').next())
                        .and_then(|v| v.parse::<u16>().ok())
                        != Some(port),
                    "PeerServer overlaps TURN listener"
                );
            }
        }
        ensure!(
            (64..=1048576).contains(&s.cloud_save_budget_mb),
            "invalid cloud save budget"
        );
        if let Some(turn) = &s.turn {
            ensure!(
                turn.secret_file.is_absolute(),
                "TURN secret file must be absolute"
            );
            ensure!(
                !turn.urls.is_empty() && turn.urls.len() <= 4,
                "provide 1–4 TURN URLs"
            );
            ensure!(
                turn.relay_port_start >= 1024 && turn.relay_port_end >= turn.relay_port_start,
                "invalid TURN relay range"
            );
            ensure!(
                turn.relay_port_end < s.port_start || turn.relay_port_start > s.port_end,
                "TURN relay ports overlap game ports"
            );
            for raw in &turn.urls {
                let endpoint = raw
                    .strip_prefix("turn:")
                    .or_else(|| raw.strip_prefix("turns:"))
                    .context("TURN URLs must use turn: or turns:")?;
                let (address, transport) = endpoint
                    .split_once('?')
                    .unwrap_or((endpoint, "transport=udp"));
                ensure!(
                    matches!(transport, "transport=udp" | "transport=tcp"),
                    "invalid TURN transport"
                );
                let (host, port) = address
                    .rsplit_once(':')
                    .context("TURN URL needs a hostname and explicit port")?;
                let port: u16 = port.parse()?;
                ensure!(
                    hostname(host)
                        && port >= 1024
                        && port != s.api_port
                        && !(s.port_start..=s.port_end).contains(&port),
                    "invalid TURN listener or overlap with game ports"
                );
                ensure!(
                    !(turn.relay_port_start..=turn.relay_port_end).contains(&port),
                    "TURN listener overlaps relay range"
                );
            }
            ensure!(
                !(turn.relay_port_start..=turn.relay_port_end).contains(&s.api_port),
                "TURN relay range overlaps API port"
            );
        }
        let auth = reqwest::Url::parse(&s.auth_url)?;
        ensure!(
            s.peerjs_port.is_none() || s.peerjs_port != auth.port_or_known_default(),
            "PeerServer overlaps login proxy"
        );
        ensure!(
            auth.scheme() == "http"
                && auth
                    .host_str()
                    .is_some_and(|h| h == "127.0.0.1" || h == "[::1]")
                && auth.username().is_empty()
                && auth.password().is_none(),
            "auth_url must point to the loopback authentication proxy"
        );
        ensure!(
            (64..=1048576).contains(&s.content_budget_mb),
            "invalid content budget"
        );
        ensure!(
            s.publish_capture_mb <= 1048576,
            "invalid publication capture budget"
        );
        ensure!(
            s.firewall_socket.as_ref().is_none_or(|p| p.is_absolute()),
            "firewall socket must be absolute"
        );
        ensure!(
            hostname(&s.public_host) && hostname(&s.domain),
            "invalid public_host or domain"
        );
        ensure!(
            s.state_dir.is_absolute()
                && s.token_file.is_absolute()
                && s.docker_socket.is_absolute(),
            "controller paths must be absolute"
        );
        ensure!(
            s.port_start >= 1024 && s.port_end >= s.port_start,
            "invalid port range"
        );
        ensure!(
            s.api_port >= 1024 && !(s.port_start..=s.port_end).contains(&s.api_port),
            "API port overlaps game range"
        );
        ensure!(
            (16..=1048576).contains(&s.memory_budget_mb),
            "invalid memory budget"
        );
        ensure!(
            (10..=1024000).contains(&s.cpu_budget_millis),
            "invalid CPU budget"
        );
        ensure!(
            (64..=1048576).contains(&s.disk_reserve_mb),
            "invalid disk reserve"
        );
        ensure!(
            (10..=3600).contains(&s.lease_seconds)
                && (1..=30).contains(&s.tick_seconds)
                && (1..=3600).contains(&s.retry_seconds),
            "invalid lease/tick/retry interval"
        );
        let mut hosts = std::collections::BTreeSet::from(["auth".to_string()]);
        for (id, g) in &self.games {
            ensure!(slug(id), "invalid game id: {id}");
            let digest = g.image.rsplit_once("sha256:").map(|(_, d)| d).unwrap_or("");
            ensure!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    && (g.image.starts_with("sha256:") || g.image.contains("@sha256:"))
                    && g.image
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b)),
                "{id}: pin image to a digest or local sha256 image ID"
            );
            ensure!(
                g.container_port > 0
                    && g.memory_mb >= 16
                    && g.memory_mb <= s.memory_budget_mb
                    && g.cpu_millis >= 10
                    && g.cpu_millis <= s.cpu_budget_millis,
                "{id}: invalid resource request"
            );
            ensure!(
                !g.protocols.is_empty()
                    && g.protocols.len() <= 2
                    && g.protocols.iter().all(|p| p == "tcp" || p == "udp")
                    && (g.protocols.len() == 1 || g.protocols[0] != g.protocols[1]),
                "{id}: invalid protocols"
            );
            ensure!(
                (1..=86400).contains(&g.idle_seconds)
                    && (1..=3600).contains(&g.startup_seconds)
                    && (1..=300).contains(&g.stop_seconds)
                    && (1..=4096).contains(&g.max_leases),
                "{id}: invalid lifecycle limits"
            );
            let ids: Vec<_> = g.user.split(':').collect();
            ensure!(
                ids.len() == 2 && ids.iter().all(|s| s.parse::<u32>().is_ok_and(|v| v > 0)),
                "{id}: user must be non-root UID:GID"
            );
            ensure!(
                g.network != NetworkPolicy::Internal || g.web,
                "{id}: internal networks require the authenticated HTTP/WebSocket proxy"
            );
            if g.web {
                ensure!(
                    g.protocols == ["tcp"],
                    "{id}: web servers must use TCP only"
                );
                ensure!(hosts.insert(format!("{id}-server")), "duplicate hostname");
            }
        }
        for (id, site) in &self.sites {
            ensure!(
                slug(id) && hosts.insert(id.clone()),
                "invalid or duplicate site hostname: {id}"
            );
            ensure!(
                site.root.is_absolute()
                    && site
                        .root
                        .to_string_lossy()
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/_- .".contains(&b)),
                "{id}: invalid absolute site path"
            );
        }
        if self.games.len() > (s.port_end - s.port_start) as usize + 1 {
            bail!("not enough ports for catalog");
        }
        Ok(())
    }
}
