//! Ephemeral voice membership. Infuser owns admission; LiveKit owns media.
//! Tokens are accepted only through our gateway, never directly on LiveKit's API port.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub const LEASE_SECONDS: u64 = 90;
pub const ROOM_PREFIX: &str = "infuser-voice-";
pub const MAX_GAIN_PERCENT: u16 = 200;
pub const MAX_PEER_SETTINGS: usize = 128;

fn default_percent() -> u16 {
    100
}
fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerAudioSetting {
    #[serde(default = "default_percent")]
    pub volume: u16,
    #[serde(default)]
    pub muted: bool,
}
impl Default for PeerAudioSetting {
    fn default() -> Self {
        Self {
            volume: 100,
            muted: false,
        }
    }
}
impl PeerAudioSetting {
    pub fn is_default(&self) -> bool {
        self.volume == 100 && !self.muted
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSettings {
    #[serde(default = "default_percent")]
    pub input_gain: u16,
    #[serde(default = "default_percent")]
    pub output_volume: u16,
    #[serde(default = "default_true")]
    pub auto_gain_control: bool,
    #[serde(default = "default_true")]
    pub noise_suppression: bool,
    #[serde(default = "default_true")]
    pub echo_cancellation: bool,
    #[serde(default)]
    pub peer_volumes: BTreeMap<String, PeerAudioSetting>,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input_gain: 100,
            output_volume: 100,
            auto_gain_control: true,
            noise_suppression: true,
            echo_cancellation: true,
            peer_volumes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerAudioPatch {
    pub volume: Option<u16>,
    pub muted: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSettingsPatch {
    pub input_gain: Option<u16>,
    pub output_volume: Option<u16>,
    pub auto_gain_control: Option<bool>,
    pub noise_suppression: Option<bool>,
    pub echo_cancellation: Option<bool>,
    pub peer_volumes: Option<BTreeMap<String, PeerAudioPatch>>,
    pub mic_muted: Option<bool>,
    pub deafened: Option<bool>,
}

fn valid_player_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl AudioSettings {
    pub fn normalize(&mut self) {
        self.input_gain = self.input_gain.min(MAX_GAIN_PERCENT);
        self.output_volume = self.output_volume.min(MAX_GAIN_PERCENT);
        self.peer_volumes.retain(|id, entry| {
            entry.volume = entry.volume.min(MAX_GAIN_PERCENT);
            valid_player_id(id) && !entry.is_default()
        });
        while self.peer_volumes.len() > MAX_PEER_SETTINGS {
            if let Some(last) = self.peer_volumes.keys().next_back().cloned() {
                self.peer_volumes.remove(&last);
            }
        }
    }

    /// Applies durable audio level/processing fields from a patch.
    /// Returns `Ok(true)` if any persistent setting changed.
    pub fn apply_patch(
        &mut self,
        patch: &AudioSettingsPatch,
        allow_processing: bool,
    ) -> Result<bool> {
        let mut changed = false;
        if let Some(gain) = patch.input_gain {
            ensure!(
                gain <= MAX_GAIN_PERCENT,
                "Microphone gain must be between 0% and 200%."
            );
            if self.input_gain != gain {
                self.input_gain = gain;
                changed = true;
            }
        }
        if let Some(volume) = patch.output_volume {
            ensure!(
                volume <= MAX_GAIN_PERCENT,
                "Voice volume must be between 0% and 200%."
            );
            if self.output_volume != volume {
                self.output_volume = volume;
                changed = true;
            }
        }
        if allow_processing {
            if let Some(v) = patch.auto_gain_control
                && self.auto_gain_control != v
            {
                self.auto_gain_control = v;
                changed = true;
            }
            if let Some(v) = patch.noise_suppression
                && self.noise_suppression != v
            {
                self.noise_suppression = v;
                changed = true;
            }
            if let Some(v) = patch.echo_cancellation
                && self.echo_cancellation != v
            {
                self.echo_cancellation = v;
                changed = true;
            }
        } else {
            ensure!(
                patch.auto_gain_control.is_none()
                    && patch.noise_suppression.is_none()
                    && patch.echo_cancellation.is_none(),
                "Voice processing options can only be changed from the Library."
            );
        }
        if let Some(peers) = &patch.peer_volumes {
            ensure!(
                peers.len() <= MAX_PEER_SETTINGS,
                "Too many participant volume entries."
            );
            for (player_id, update) in peers {
                ensure!(valid_player_id(player_id), "Invalid participant ID.");
                if let Some(vol) = update.volume {
                    ensure!(
                        vol <= MAX_GAIN_PERCENT,
                        "Participant volume must be between 0% and 200%."
                    );
                }
                let mut next = self
                    .peer_volumes
                    .get(player_id)
                    .cloned()
                    .unwrap_or_default();
                if let Some(vol) = update.volume {
                    next.volume = vol;
                }
                if let Some(muted) = update.muted {
                    next.muted = muted;
                }
                if next.is_default() {
                    if self.peer_volumes.remove(player_id).is_some() {
                        changed = true;
                    }
                } else if self.peer_volumes.get(player_id) != Some(&next) {
                    if !self.peer_volumes.contains_key(player_id)
                        && self.peer_volumes.len() >= MAX_PEER_SETTINGS
                    {
                        anyhow::bail!("Too many saved participant volume overrides.");
                    }
                    self.peer_volumes.insert(player_id.clone(), next);
                    changed = true;
                }
            }
        }
        Ok(changed)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub port: u16,
    pub key_file: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    api_key: String,
    api_secret: String,
}
#[derive(Clone)]
pub(crate) struct Lease {
    pub id: String,
    pub request_id: String,
    pub owner: String,
    pub channel: String,
    pub visit: Option<String>,
    pub room: String,
    pub expires: u64,
    pub connected: bool,
    pub mic_muted: bool,
    pub deafened: bool,
}
#[derive(Default)]
pub(crate) struct State {
    pub leases: BTreeMap<String, Lease>,
    pub checked_at: u64,
    pub revisions: BTreeMap<String, u64>,
    pub notifiers: BTreeMap<String, Arc<Notify>>,
}
pub(crate) struct Service {
    pub port: u16,
    key: Credentials,
    pub epoch: String,
    pub state: Mutex<State>,
    // Serialize reconciliation and membership mutations, not audio or signaling.
    pub gate: tokio::sync::Mutex<()>,
}
impl Service {
    pub fn open(settings: &Settings) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let file =
            std::fs::File::open(&settings.key_file).context("open private voice credentials")?;
        ensure!(
            file.metadata()?.permissions().mode() & 0o007 == 0,
            "voice credentials must not be world-readable"
        );
        let key: Credentials = serde_json::from_reader(file).context("read voice credentials")?;
        ensure!(
            !key.api_key.is_empty() && key.api_key.len() <= 128 && key.api_secret.len() >= 32,
            "invalid voice credentials"
        );
        Ok(Self {
            port: settings.port,
            key,
            epoch: crate::store::secret()[..16].into(),
            state: Default::default(),
            gate: Default::default(),
        })
    }
    fn sign(&self, claims: Value) -> String {
        let header = B64.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let body = B64.encode(serde_json::to_vec(&claims).unwrap());
        let content = format!("{header}.{body}");
        let signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, self.key.api_secret.as_bytes()),
            content.as_bytes(),
        );
        format!("{content}.{}", B64.encode(signature.as_ref()))
    }
    pub fn participant_token(&self, lease: &Lease, time: u64) -> String {
        self.sign(json!({"iss":self.key.api_key,"sub":lease.id,"nbf":time.saturating_sub(5),"exp":time+300,
            "video":{"roomJoin":true,"room":lease.room,"canPublish":true,"canSubscribe":true,
            "canPublishData":false,"canPublishSources":["microphone"],"canUpdateOwnMetadata":false}}))
    }
    /// LiveKit refreshes JWTs over signaling. Verify those too, then require the
    /// original live lease and channel policy; a valid signature is not admission.
    pub fn claims(&self, token: &str, time: u64) -> Result<(String, String)> {
        ensure!(token.len() <= 8192, "invalid voice token");
        let (content, signature) = token.rsplit_once('.').context("invalid voice token")?;
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, self.key.api_secret.as_bytes()),
            content.as_bytes(),
            &B64.decode(signature)?,
        )
        .map_err(|_| anyhow::anyhow!("invalid voice signature"))?;
        let (header, body) = content.split_once('.').context("invalid voice token")?;
        let header: Value = serde_json::from_slice(&B64.decode(header)?)?;
        let body: Value = serde_json::from_slice(&B64.decode(body)?)?;
        ensure!(
            header["alg"] == "HS256"
                && body["iss"] == self.key.api_key
                && body["exp"].as_u64().is_some_and(|v| v > time)
                && body["nbf"].as_u64().is_none_or(|v| v <= time)
                && body["video"]["roomJoin"] == true,
            "invalid voice claims"
        );
        Ok((
            body["sub"].as_str().context("missing identity")?.into(),
            body["video"]["room"]
                .as_str()
                .context("missing room")?
                .into(),
        ))
    }
    pub async fn call(
        &self,
        http: &reqwest::Client,
        method: &str,
        room: Option<&str>,
        body: Value,
    ) -> Result<Value> {
        let time = crate::controller::now();
        let grant = if method == "DeleteRoom" {
            json!({"roomCreate":true})
        } else if let Some(room) = room {
            json!({"roomAdmin":true,"room":room})
        } else {
            json!({"roomList":true})
        };
        let token=self.sign(json!({"iss":self.key.api_key,"nbf":time.saturating_sub(5),"exp":time+30,"video":grant}));
        let response = http
            .post(format!(
                "http://127.0.0.1:{}/twirp/livekit.RoomService/{method}",
                self.port
            ))
            .bearer_auth(token)
            .json(&body)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await?;
        // A participant/room disappearing concurrently is successful cleanup.
        if response.status() == reqwest::StatusCode::NOT_FOUND
            && matches!(method, "RemoveParticipant" | "DeleteRoom")
        {
            return Ok(json!({}));
        }
        ensure!(
            response.status().is_success(),
            "voice service {method} returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }
    pub fn healthy(&self, time: u64) -> bool {
        self.state.lock().unwrap().checked_at.saturating_add(15) > time
    }
    pub fn revision_for(&self, owner: &str, persisted: u64) -> (u64, Arc<Notify>) {
        let mut state = self.state.lock().unwrap();
        let current = state
            .revisions
            .entry(owner.to_owned())
            .or_insert(persisted.max(1));
        if *current < persisted {
            *current = persisted;
        }
        let rev = *current;
        let notify = state
            .notifiers
            .entry(owner.to_owned())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone();
        (rev, notify)
    }
    pub fn bump_owner(&self, owner: &str, persisted: u64) -> u64 {
        let (rev, notify) = {
            let mut state = self.state.lock().unwrap();
            let entry = state
                .revisions
                .entry(owner.to_owned())
                .or_insert(persisted.max(1));
            *entry = (*entry + 1).max(persisted);
            let rev = *entry;
            let notify = state.notifiers.get(owner).cloned();
            (rev, notify)
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
        rev
    }
}
