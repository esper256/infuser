//! Ephemeral voice membership. Infuser owns admission; LiveKit owns media.
//! Tokens are accepted only through our gateway, never directly on LiveKit's API port.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::hmac;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

pub const LEASE_SECONDS: u64 = 90;
pub const ROOM_PREFIX: &str = "infuser-voice-";
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
}
#[derive(Default)]
pub(crate) struct State {
    pub leases: BTreeMap<String, Lease>,
    pub checked_at: u64,
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
}
