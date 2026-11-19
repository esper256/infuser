//! SDK major selection is independent of daemon releases and wire API majors.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub major: u32,
    pub api_major: u32,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}
impl Requirement {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.required_capabilities.len() <= 16,
            "at most 16 SDK capabilities"
        );
        ensure!(
            self.required_capabilities
                .iter()
                .all(|c| !c.is_empty() && c.len() <= 64),
            "invalid SDK capability name"
        );
        ensure!(
            self.required_capabilities
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.required_capabilities.len(),
            "duplicate SDK capabilities"
        );
        Ok(())
    }
    pub fn admit(&self, advertised: &Value) -> Result<()> {
        ensure!(
            advertised["majors"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(self.major))),
            "unsupported SDK major {}; republish with a supported major or upgrade Infuser",
            self.major
        );
        ensure!(
            advertised["api_majors"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(self.api_major))),
            "unsupported game API major {}",
            self.api_major
        );
        for required in &self.required_capabilities {
            ensure!(
                advertised["capabilities"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(required))),
                "required capability unavailable: {required}"
            );
        }
        Ok(())
    }
}
pub fn capabilities(relay: bool) -> Value {
    let mut available = vec![
        "identity.v1",
        "saves.v1",
        "cloud_checkpoints.v1",
        "runtime_sessions.v1",
        "presence.v1",
        "launch_intents.v1",
        crate::connections::CAPABILITY,
        crate::dedicated::CAPABILITY,
    ];
    if relay {
        available.extend(["peers.webrtc.v1", "peers.rooms.v1"]);
    }
    json!({"status":"preview","majors":[1],"api_majors":[1],"default_major":1,"path":"/_infuser/sdk/v1.js","api_base":"/_infuser/api/v1","capabilities":available,"legacy_path":"/_infuser/sdk.js"})
}
pub fn with_peerjs(relay: bool, peerjs: bool) -> Value {
    let mut value = capabilities(relay);
    if relay && peerjs {
        value["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(json!("peers.peerjs.v1"));
    }
    value
}
pub fn script(manifest: &crate::manifest::Manifest) -> &'static str {
    if manifest.sdk.is_some() {
        "/_infuser/sdk/v1.js"
    } else {
        "/_infuser/sdk.js"
    }
}
