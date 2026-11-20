//! TURN REST credentials. The password is base64(HMAC-SHA1(secret, username))
//! and the username is `{expires}:{peer}`. The server returns the long-term
//! key MD5(username:realm:password), which is what MESSAGE-INTEGRITY checks.

use crate::{controller::now, turn::ipc::Admission};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use std::{net::SocketAddr, sync::Arc};
use turn::Error;

type HmacSha1 = Hmac<Sha1>;

pub fn credential(secret: &[u8], username: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(username.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Peer id inside a still-valid `expires:peer` username.
pub fn peer_from_username(username: &str, time: u64) -> Option<&str> {
    let (expires, peer) = username.split_once(':')?;
    let expires = expires.parse::<u64>().ok()?;
    if expires <= time || !peer_id(peer) {
        return None;
    }
    Some(peer)
}

pub fn peer_id(peer: &str) -> bool {
    (1..=64).contains(&peer.len())
        && peer
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub struct RestAuth {
    secret: Vec<u8>,
    admit: Arc<Admission>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl RestAuth {
    pub fn new(secret: Vec<u8>, admit: Arc<Admission>) -> Self {
        Self {
            secret,
            admit,
            clock: Arc::new(now),
        }
    }

    #[cfg(test)]
    pub fn with_clock(mut self, clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }
}

impl turn::auth::AuthHandler for RestAuth {
    fn auth_handle(
        &self,
        username: &str,
        realm: &str,
        _src_addr: SocketAddr,
    ) -> Result<Vec<u8>, Error> {
        let Some(peer) = peer_from_username(username, (self.clock)()) else {
            return Err(Error::ErrFakeErr);
        };
        if !self.admit.allows(peer) {
            return Err(Error::ErrFakeErr);
        }
        let password = credential(&self.secret, username);
        Ok(turn::auth::generate_auth_key(username, realm, &password))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turn::auth::AuthHandler;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn password_matches_the_published_vector() {
        assert_eq!(
            credential(SECRET, "4600:peer-123"),
            "mfIgcES3ogsv+jQO2P+P2VzuLG8="
        );
    }

    #[test]
    fn expired_malformed_and_unadmitted_peers_get_no_key() {
        let admitted = Admission::new();
        admitted.replace(["peer-123".into()].into());
        let auth = RestAuth::new(SECRET.to_vec(), admitted).with_clock(|| 1000);
        let realm = "games.example.com";
        let key = auth
            .auth_handle("4600:peer-123", realm, "127.0.0.1:1".parse().unwrap())
            .unwrap();
        assert_eq!(
            key,
            turn::auth::generate_auth_key("4600:peer-123", realm, "mfIgcES3ogsv+jQO2P+P2VzuLG8=")
        );
        assert!(
            auth.auth_handle("1000:peer-123", realm, "127.0.0.1:1".parse().unwrap())
                .is_err()
        );
        assert!(
            auth.auth_handle("4600:other", realm, "127.0.0.1:1".parse().unwrap())
                .is_err()
        );
        assert!(
            auth.auth_handle("no-colon", realm, "127.0.0.1:1".parse().unwrap())
                .is_err()
        );
        assert!(
            auth.auth_handle("4600:", realm, "127.0.0.1:1".parse().unwrap())
                .is_err()
        );
    }
}
