//! Internal dedicated-connection foundation; not an advertised SDK capability yet.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

pub const CAPABILITY: &str = "connections.websocket.v1";
pub const PUBLIC_KEY_FILE: &str = "connection-public-key";
pub const PROOF_SECONDS: u64 = 60;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 256;
pub const MAX_PER_LAUNCH: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub iss: String,
    pub aud: String,
    pub iat: u64,
    pub exp: u64,
    pub group_id: String,
    pub app_id: String,
    pub build_id: String,
    pub instance_id: String,
    pub launch_id: String,
    pub connection_id: String,
    pub player_id: String,
    pub name: String,
    pub guest: bool,
    #[serde(default)]
    pub request: crate::dedicated::Request,
}

pub struct Signer(Ed25519KeyPair);
impl Identity {
    pub fn runtime_id(&self) -> &str {self.deployment_id.as_deref().unwrap_or(&self.app_id)}
}
impl Signer {
    pub fn open(directory: &Path) -> Result<Self> {
        let bootstrap = directory.join("server-bootstrap.key");
        if !bootstrap.exists() {
            crate::private_file::replace(&bootstrap, crate::store::secret().as_bytes())?;
        }
        let path = directory.join("connection-signing-key.pk8");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                    .map_err(|_| anyhow::anyhow!("cannot generate connection signing key"))?;
                crate::private_file::replace(&path, key.as_ref())?;
                key.as_ref().to_vec()
            }
            Err(error) => return Err(error.into()),
        };
        let key = Ed25519KeyPair::from_pkcs8(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid connection signing key"))?;
        // The controller owns the state-directory lock before opening this key.
        // Only the public verification key is injected into game containers.
        crate::private_file::replace(
            &directory.join(PUBLIC_KEY_FILE),
            B64.encode(key.public_key()).as_bytes(),
        )?;
        Ok(Self(key))
    }
    pub fn sign(&self, identity: &Identity) -> Result<String> {
        let header = B64.encode(br#"{"alg":"EdDSA","typ":"infuser-connection+jwt","v":1}"#);
        let payload = B64.encode(serde_json::to_vec(identity)?);
        let input = format!("{header}.{payload}");
        Ok(format!(
            "{input}.{}",
            B64.encode(self.0.sign(input.as_bytes()))
        ))
    }
    pub fn access_token(&self, scope: &crate::dedicated::Scope) -> Result<String> {
        let payload = B64.encode(serde_json::to_vec(scope)?);
        let input = format!("infuser-server-access-v1.{payload}");
        Ok(format!(
            "{input}.{}",
            B64.encode(self.0.sign(input.as_bytes()))
        ))
    }
    pub fn access_scope(&self, token: &str, now: u64) -> Result<crate::dedicated::Scope> {
        ensure!(token.len() <= 4096, "invalid server token");
        let parts: Vec<_> = token.split('.').collect();
        ensure!(
            parts.len() == 3 && parts[0] == "infuser-server-access-v1",
            "invalid server token"
        );
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, self.0.public_key())
            .verify(parts[..2].join(".").as_bytes(), &B64.decode(parts[2])?)
            .map_err(|_| anyhow::anyhow!("invalid server token"))?;
        let scope: crate::dedicated::Scope = serde_json::from_slice(&B64.decode(parts[1])?)?;
        ensure!(
            scope.exp > now && scope.exp <= now.saturating_add(120),
            "expired server token"
        );
        Ok(scope)
    }
}

pub fn instance_token(directory: &Path, owner: &str, app: &str, instance: &str) -> Result<String> {
    let key = ring::hmac::Key::new(
        ring::hmac::HMAC_SHA256,
        &std::fs::read(directory.join("server-bootstrap.key"))?,
    );
    let input = serde_json::to_vec(&["infuser-server-bootstrap-v1", owner, app, instance])?;
    Ok(B64.encode(ring::hmac::sign(&key, &input)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    Connecting,
    Connected,
    Closed,
    Failed,
    Cancelled,
}
impl State {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone)]
pub struct Attempt {
    pub identity: Identity,
    pub lease_id: String,
    pub state: State,
    pub error: Option<String>,
    pub updated: u64,
    pub session: Option<crate::dedicated::Session>,
    pub admission: Option<String>,
}
#[derive(Default)]
pub struct Attempts(pub BTreeMap<String, Attempt>);
impl Attempts {
    pub fn prune(&mut self, now: u64, live: &std::collections::BTreeSet<String>) {
        for attempt in self.0.values_mut() {
            if attempt.state == State::Pending && attempt.identity.exp <= now {
                attempt.state = State::Failed;
                attempt.error = Some("attempt_expired".into());
                attempt.updated = now;
            }
        }
        // Keep consumed/cancelled handles for the entire launch. Expiring a
        // tombstone while its launch is alive would let an old PUT reopen it.
        self.0.retain(|_, a| live.contains(&a.identity.launch_id));
    }
    pub fn insert(&mut self, id: String, attempt: Attempt) -> Result<()> {
        ensure!(!self.0.contains_key(&id), "attempt already exists");
        ensure!(
            self.0.len() < 4096
                && self.0.values().filter(|a| !a.state.terminal()).count() < MAX_CONNECTIONS,
            "connection capacity exhausted"
        );
        ensure!(
            self.0
                .values()
                .filter(
                    |a| a.identity.launch_id == attempt.identity.launch_id && !a.state.terminal()
                )
                .count()
                < MAX_PER_LAUNCH,
            "too many connections for this launch"
        );
        self.0.insert(id, attempt);
        Ok(())
    }
    pub fn claim(&mut self, id: &str, now: u64) -> Result<Attempt> {
        let attempt = self.0.get_mut(id).context("unknown attempt")?;
        ensure!(
            attempt.state == State::Pending && attempt.identity.exp > now,
            "attempt already consumed or expired"
        );
        attempt.state = State::Connecting;
        attempt.updated = now;
        Ok(attempt.clone())
    }
    pub fn finish(&mut self, id: &str, state: State, error: Option<&str>, now: u64) {
        if let Some(attempt) = self.0.get_mut(id).filter(|a| !a.state.terminal()) {
            attempt.state = state;
            attempt.error = error.map(str::to_owned);
            attempt.updated = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};

    fn identity() -> Identity {
        Identity { deployment_id: None,
            iss: "https://games.example.test".into(),
            aud: "infuser-server-connection".into(),
            iat: 100,
            exp: 160,
            group_id: "home".into(),
            app_id: "pond".into(),
            build_id: "one".into(),
            instance_id: "instance".into(),
            launch_id: "launch".into(),
            connection_id: "connection".into(),
            player_id: "player".into(),
            name: "River".into(),
            guest: false,
            request: Default::default(),
        }
    }

    #[test]
    fn restart_preserves_verification_key_without_exposing_signing_material() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let proof = {
            let signer = Signer::open(directory.path())?;
            signer.sign(&identity())?
        };
        let before = std::fs::read(directory.path().join(PUBLIC_KEY_FILE))?;
        let _restarted = Signer::open(directory.path())?;
        assert_eq!(
            before,
            std::fs::read(directory.path().join(PUBLIC_KEY_FILE))?
        );
        let parts = proof.split('.').collect::<Vec<_>>();
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, B64.decode(&before)?)
            .verify(parts[..2].join(".").as_bytes(), &B64.decode(parts[2])?)
            .map_err(|_| anyhow::anyhow!("proof did not survive restart"))?;
        assert_eq!(B64.decode(before)?.len(), 32);
        assert_eq!(
            std::fs::metadata(directory.path().join("connection-signing-key.pk8"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn cancellation_and_expiry_never_allow_a_late_retry_to_reopen_a_live_launch() -> Result<()> {
        let mut attempts = Attempts::default();
        attempts.insert(
            "connection".into(),
            Attempt {
                identity: identity(),
                lease_id: "lease".into(),
                state: State::Pending,
                error: None,
                updated: 100,
                session: None,
                admission: None,
            },
        )?;
        attempts.claim("connection", 110)?;
        assert!(attempts.claim("connection", 111).is_err());
        attempts.finish("connection", State::Cancelled, Some("cancelled"), 112);
        attempts.finish("connection", State::Connected, None, 113); // Late upgrade completion.
        let live = BTreeSet::from(["launch".to_owned()]);
        attempts.prune(1000, &live);
        assert_eq!(attempts.0["connection"].state, State::Cancelled);
        assert!(attempts.claim("connection", 1000).is_err());
        attempts.insert(
            "expired".into(),
            Attempt {
                identity: identity(),
                lease_id: "lease".into(),
                state: State::Pending,
                error: None,
                updated: 100,
                session: None,
                admission: None,
            },
        )?;
        attempts.prune(1000, &live);
        assert_eq!(
            attempts.0["expired"].error.as_deref(),
            Some("attempt_expired")
        );
        assert!(attempts.claim("expired", 1000).is_err());
        attempts.prune(1000, &BTreeSet::new());
        assert!(attempts.0.is_empty());
        Ok(())
    }
}
