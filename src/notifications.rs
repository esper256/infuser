//! Device subscriptions and a bounded, coalescing chat notification outbox.
use crate::{
    controller::now,
    platform::{Data, Database, channels::Channel},
    portal::fault,
};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    pub endpoint: String,
}
impl Subscription {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| fault(422, "Invalid push endpoint."))?;
        let host = url.host_str().unwrap_or("");
        // A subscription must never turn the controller into an arbitrary HTTP proxy.
        let trusted = host == "fcm.googleapis.com"
            || host == "updates.push.services.mozilla.com"
            || host.ends_with(".push.services.mozilla.com")
            || host == "web.push.apple.com"
            || host.ends_with(".notify.windows.com");
        ensure!(
            trusted
                && url.scheme() == "https"
                && url.port_or_known_default() == Some(443)
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
                && self.endpoint.len() <= 2048,
            fault(422, "This browser's push service is not supported.")
        );
        Ok(())
    }
}

// RFC 8292 VAPID: ring signs the standard ES256 JWT. Notifications carry no
// payload, so RFC 8030 needs no content-encryption keys or custom encryption.
pub struct Push {
    key: EcdsaKeyPair,
    pub public_key: String,
}
impl Push {
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("web-push.key");
        let rng = SystemRandom::new();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                    .map_err(|_| anyhow::anyhow!("Cannot generate push signing key"))?;
                crate::private_file::replace(&path, pkcs8.as_ref())?;
                pkcs8.as_ref().to_vec()
            }
            Err(e) => return Err(e.into()),
        };
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &bytes, &rng)
            .map_err(|_| anyhow::anyhow!("Invalid push signing key"))?;
        let public_key = B64.encode(key.public_key().as_ref());
        Ok(Self { key, public_key })
    }
    pub fn request(&self, sub: &Subscription, subject: &str) -> Result<reqwest::Request> {
        sub.validate()?;
        let url = reqwest::Url::parse(&sub.endpoint)?;
        let claims = serde_json::json!({"aud":url.origin().ascii_serialization(),"exp":now()+3600,"sub":subject});
        let signing_input = format!(
            "{}.{}",
            B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#),
            B64.encode(serde_json::to_vec(&claims)?)
        );
        let signature = self
            .key
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| anyhow::anyhow!("Cannot sign push request"))?;
        let authorization = format!(
            "vapid t={}.{}, k={}",
            signing_input,
            B64.encode(signature.as_ref()),
            self.public_key
        );
        let mut request = reqwest::Request::new(reqwest::Method::POST, url);
        let headers = request.headers_mut();
        headers.insert("authorization", authorization.parse()?);
        headers.insert("ttl", "60".parse()?);
        headers.insert("urgency", "normal".parse()?);
        headers.insert("topic", "infuser-chat".parse()?);
        headers.insert("content-length", "0".parse()?);
        Ok(request)
    }
}

pub fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS push_subscriptions (
        endpoint TEXT PRIMARY KEY, email TEXT NOT NULL, private_group TEXT NOT NULL,
        subscription TEXT NOT NULL, updated INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS push_outbox (
        endpoint TEXT PRIMARY KEY, message_id INTEGER NOT NULL, created INTEGER NOT NULL,
        due INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0);",
    )?;
    Ok(())
}
pub(crate) fn enqueue(
    c: &Connection,
    data: &Data,
    sender: &str,
    channel: &Channel,
    id: i64,
    time: u64,
) -> Result<()> {
    let mut s = c.prepare("SELECT endpoint,email FROM push_subscriptions")?;
    let rows = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (endpoint, email) = row?;
        if email != sender
            && channel
                .access(data, &email, false, time)
                .is_some_and(|access| access.sees(id as u64))
        {
            c.execute("INSERT INTO push_outbox(endpoint,message_id,created,due) VALUES(?1,?2,?3,?3)
                ON CONFLICT(endpoint) DO UPDATE SET message_id=excluded.message_id,created=excluded.created",
                params![endpoint,id,time])?;
        }
    }
    Ok(())
}
impl Database {
    pub fn push_subscribe(&self, email: &str, sub: &Subscription) -> Result<()> {
        sub.validate()?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let data: Data = super::platform::decode_catalog(
            &tx,
            &tx.query_row("SELECT data FROM platform WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })?,
        )?;
        ensure!(
            data.profile_enabled(email),
            fault(403, "Membership is disabled.")
        );
        let owner: Option<String> = tx
            .query_row(
                "SELECT email FROM push_subscriptions WHERE endpoint=?1",
                [&sub.endpoint],
                |r| r.get(0),
            )
            .optional()?;
        // The page unsubscribes and creates a fresh endpoint on account changes.
        ensure!(
            owner.as_deref().is_none_or(|o| o == email),
            fault(409, "Turn notifications off, then try again.")
        );
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM push_subscriptions WHERE email=?1",
            [email],
            |r| r.get(0),
        )?;
        ensure!(
            owner.is_some() || count < 16,
            fault(
                429,
                "Too many notification devices. Turn off an old one first."
            )
        );
        tx.execute("INSERT INTO push_subscriptions(endpoint,email,private_group,subscription,updated) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(endpoint) DO UPDATE SET private_group=excluded.private_group,subscription=excluded.subscription,updated=excluded.updated",
            params![sub.endpoint,email,data.groups.member(email),serde_json::to_string(sub)?,now()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn push_unsubscribe(&self, email: &str, endpoint: &str) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM push_outbox WHERE endpoint IN (SELECT endpoint FROM push_subscriptions WHERE endpoint=?1 AND email=?2)",params![endpoint,email])?;
        tx.execute(
            "DELETE FROM push_subscriptions WHERE endpoint=?1 AND email=?2",
            params![endpoint, email],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn push_targets(&self) -> Result<Vec<(String, u64, Subscription)>> {
        let mut connection = self.connection.lock().unwrap();
        let c = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = self.cached_catalog(&c)?;
        c.execute(
            "DELETE FROM push_outbox WHERE created<?1 OR attempts>=3",
            [now().saturating_sub(300)],
        )?;
        // Expired devices are garbage collected without changing other devices' settings.
        c.execute(
            "DELETE FROM push_subscriptions WHERE updated<?1",
            [now().saturating_sub(180 * 86400)],
        )?;
        c.execute("DELETE FROM push_outbox WHERE endpoint NOT IN (SELECT endpoint FROM push_subscriptions)",[])?;
        let mut s = c.prepare(
            "SELECT s.endpoint,o.message_id,s.email,m.channel_id,s.subscription,m.author,m.text
            FROM push_outbox o JOIN push_subscriptions s USING(endpoint)
            LEFT JOIN library_chat m ON m.id=o.message_id
            WHERE o.due<=?1 ORDER BY o.due LIMIT 32",
        )?;
        let rows = s.query_map([now()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut targets = Vec::new();
        for row in rows {
            let (endpoint, id, email, channel_id, subscription, author, text) = row?;
            let channel = channel_id.map(|id| Channel::load(&c, &id)).transpose()?;
            let member_ok = |email: &str| data.members.get(email).is_some_and(|m| m.enabled);
            if channel
                .and_then(|ch| ch.access(&data, &email, false, now()))
                .is_some_and(|access| access.sees(id))
                && author.as_deref().is_some_and(member_ok)
                && text.is_some()
            {
                targets.push((endpoint, id, serde_json::from_str(&subscription)?));
            } else {
                c.execute(
                    "DELETE FROM push_outbox WHERE endpoint=?1 AND message_id=?2",
                    params![endpoint, id],
                )?;
            }
        }
        drop(s);
        c.commit()?;
        Ok(targets)
    }
    pub fn push_result(&self, endpoint: &str, id: u64, status: Option<u16>) -> Result<()> {
        let c = self.connection.lock().unwrap();
        if matches!(status, Some(404 | 410)) {
            c.execute(
                "DELETE FROM push_subscriptions WHERE endpoint=?1",
                [endpoint],
            )?;
            c.execute("DELETE FROM push_outbox WHERE endpoint=?1", [endpoint])?;
        } else if status
            .is_some_and(|s| (200..300).contains(&s) || (400..500).contains(&s) && s != 429)
        {
            c.execute(
                "DELETE FROM push_outbox WHERE endpoint=?1 AND message_id=?2",
                params![endpoint, id],
            )?;
        } else {
            c.execute("UPDATE push_outbox SET attempts=attempts+1,due=?3 WHERE endpoint=?1 AND message_id=?2",params![endpoint,id,now()+30])?;
        }
        Ok(())
    }
}
