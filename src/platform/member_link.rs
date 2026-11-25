//! One-time operator links for an invited member who does not use Google.
//! Only SHA-256 hashes are stored. Raw link and session tokens stay out of
//! this module's errors and must not be logged.
use super::*;
use crate::portal::fault;
use serde_json::{Value, json};

pub const COOKIE: &str = "__Host-infuser_member";
/// Two hours. The cookie is a bearer credential for a full member, held by an
/// assistant rather than Google. That covers browsing and a slow game, and it
/// is far shorter than the seven-day Google session. Revoke ends it immediately.
pub const SESSION_SECONDS: u64 = 2 * 60 * 60;
/// Thirty minutes and single use. A person copies the link into a chat, so it
/// has to survive that handoff, and it still dies if nobody continues.
pub const LINK_SECONDS: u64 = 30 * 60;

pub enum Redeemed {
    Session(String),
    Refused,
}

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS member_links (
            token_hash TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            expires INTEGER NOT NULL,
            created INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            redeemed INTEGER
        );
        CREATE TABLE IF NOT EXISTS member_sessions (
            token_hash TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            created INTEGER NOT NULL DEFAULT 0,
            expires INTEGER NOT NULL
        );",
    )?;
    // A build of this feature created the tables before the audit columns existed.
    ensure_column(
        connection,
        "member_links",
        "created",
        "ALTER TABLE member_links ADD COLUMN created INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "member_links",
        "created_by",
        "ALTER TABLE member_links ADD COLUMN created_by TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "member_links",
        "redeemed",
        "ALTER TABLE member_links ADD COLUMN redeemed INTEGER",
    )?;
    ensure_column(
        connection,
        "member_sessions",
        "created",
        "ALTER TABLE member_sessions ADD COLUMN created INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    statement: &str,
) -> Result<()> {
    let present = {
        let mut info = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = info
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == column)
    };
    if !present {
        connection.execute_batch(statement)?;
    }
    Ok(())
}

pub(super) fn expire(tx: &Transaction<'_>, time: u64) -> Result<()> {
    tx.execute(
        "DELETE FROM member_links WHERE redeemed IS NULL AND expires<=?1",
        [time],
    )?;
    tx.execute("DELETE FROM member_sessions WHERE expires<=?1", [time])?;
    Ok(())
}

/// Returns the cookie value when a member-link cookie is present.
/// Two copies are treated as present-but-invalid so sign-in fails closed.
pub fn cookie_value(header: &str) -> Option<&str> {
    let mut found = None;
    for part in header.split(';') {
        let Some(value) = part
            .trim()
            .strip_prefix(COOKIE)
            .and_then(|rest| rest.strip_prefix('='))
        else {
            continue;
        };
        if found.is_some() {
            return Some("");
        }
        found = Some(value);
    }
    found
}

pub fn set_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_SECONDS}")
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn stored_hash(token: &str) -> Result<String> {
    let digest = hash(token);
    ensure!(digest != token, "refusing to store a raw sign-in token");
    Ok(digest)
}

fn foreign_login(data: &Data, account: &str) -> bool {
    data.identities
        .logins
        .values()
        .any(|login| login.account_id == account && login.issuer != identity::LINK)
}

/// A profile handle selects that profile's account. A contact email is accepted
/// only when it names exactly one account. Several profiles on that account are
/// the same sign-in; the profile chooser picks the group.
pub fn resolve(data: &Data, query: &str) -> Result<String> {
    let query = query.trim().to_lowercase();
    ensure!(!query.is_empty(), "member required");
    let by_email = accounts_for_email(data, &query);
    if let Some(profile) = data.identities.profiles.get(&query)
        && data.members.contains_key(&query)
    {
        let account = profile.account_id.clone();
        ensure!(
            !by_email.iter().any(|other| other != &account),
            "ambiguous account email; use its existing profile handle"
        );
        return Ok(account);
    }
    ensure!(
        by_email.len() <= 1,
        "ambiguous account email; use its existing profile handle"
    );
    by_email.into_iter().next().context("member not found")
}

fn accounts_for_email(data: &Data, email: &str) -> BTreeSet<String> {
    data.identities
        .logins
        .values()
        .filter(|login| login.email == email)
        .map(|login| login.account_id.clone())
        .chain(data.identities.pending_google.get(email).cloned())
        .collect()
}

fn profile_names(data: &Data, account: &str) -> Vec<String> {
    data.identities
        .profiles
        .iter()
        .filter(|(key, profile)| profile.account_id == account && data.members.contains_key(*key))
        .map(|(key, _)| data.members[key].name.clone())
        .collect()
}

impl Database {
    pub fn member_session(&self, token: &str) -> Result<String> {
        if !valid_token(token) {
            return Err(fault(401, "Sign in again."));
        }
        let digest = stored_hash(token)?;
        let row = {
            let connection = self.connection.lock().unwrap();
            connection
                .query_row(
                    "SELECT account_id, expires FROM member_sessions WHERE token_hash=?1",
                    [&digest],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
                )
                .optional()?
        };
        let Some((account, expires)) = row else {
            return Err(fault(401, "Sign in again."));
        };
        ensure!(expires > now(), fault(401, "Sign in again."));
        let data = self.catalog()?;
        ensure!(
            !foreign_login(&data, &account),
            fault(401, "Sign in again.")
        );
        ensure!(
            data.identities
                .logins
                .values()
                .any(|login| { login.account_id == account && login.issuer == identity::LINK }),
            fault(401, "Sign in again.")
        );
        Ok(account)
    }

    pub fn mint_member_link(
        &self,
        domain: &str,
        member: &str,
        created_by: &str,
    ) -> Result<(String, u64)> {
        ensure!(crate::config::hostname(domain), "invalid site domain");
        ensure!(
            !created_by.is_empty()
                && created_by.len() <= 80
                && !created_by.chars().any(char::is_control),
            "invalid link creator"
        );
        let token = secret();
        let digest = stored_hash(&token)?;
        let created = now();
        let expires = created + LINK_SECONDS;
        self.edit_transaction(|data, tx| {
            let account = resolve(data, member)?;
            ensure!(
                !foreign_login(data, &account),
                "this account signs in with Google"
            );
            ensure!(
                !data.account_profiles(&account)?.is_empty(),
                "enabled member required"
            );
            ensure!(
                data.identities
                    .logins
                    .values()
                    .any(|login| { login.account_id == account && login.issuer == identity::LINK })
                    || data
                        .identities
                        .pending_google
                        .values()
                        .any(|id| id == &account),
                "member not found"
            );
            tx.execute(
                "DELETE FROM member_links WHERE account_id=?1 AND redeemed IS NULL",
                [&account],
            )?;
            tx.execute(
                "INSERT INTO member_links(token_hash, account_id, expires, created, created_by) VALUES(?1,?2,?3,?4,?5)",
                params![digest, account, expires, created, created_by],
            )?;
            Ok(())
        })?;
        Ok((format!("https://{domain}/member-link/{token}"), expires))
    }

    pub fn redeem_member_link(&self, token: &str) -> Result<Redeemed> {
        if !valid_token(token) {
            return Err(fault(404, "This sign-in link is not valid."));
        }
        let digest = stored_hash(token)?;
        let time = now();
        self.edit_transaction(|data, tx| {
            let row: Option<(String, u64, Option<i64>)> = tx
                .query_row(
                    "SELECT account_id, expires, redeemed FROM member_links WHERE token_hash=?1",
                    [&digest],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let Some((account, expires, redeemed)) = row else {
                return Err(fault(404, "This sign-in link is not valid."));
            };
            if redeemed.is_some() || expires <= time {
                return Err(fault(404, "This sign-in link is not valid."));
            }
            if foreign_login(data, &account) {
                tx.execute(
                    "DELETE FROM member_links WHERE token_hash=?1 AND redeemed IS NULL",
                    [&digest],
                )?;
                return Ok(Redeemed::Refused);
            }
            data.bind_member_link(&account)?;
            let marked = tx.execute(
                "UPDATE member_links SET redeemed=?2 WHERE token_hash=?1 AND redeemed IS NULL AND expires>?2",
                params![digest, time],
            )?;
            ensure!(marked == 1, fault(404, "This sign-in link is not valid."));
            let session = secret();
            let session_hash = stored_hash(&session)?;
            tx.execute(
                "INSERT INTO member_sessions(token_hash, account_id, created, expires) VALUES(?1,?2,?3,?4)",
                params![session_hash, account, time, time + SESSION_SECONDS],
            )?;
            Ok(Redeemed::Session(session))
        })
    }

    pub fn revoke_member_links(&self, member: &str) -> Result<String> {
        self.edit_transaction(|data, tx| {
            let account = resolve(data, member)?;
            tx.execute(
                "DELETE FROM member_links WHERE account_id=?1 AND redeemed IS NULL",
                [&account],
            )?;
            tx.execute(
                "DELETE FROM member_sessions WHERE account_id=?1",
                [&account],
            )?;
            let names = profile_names(data, &account);
            Ok(if names.is_empty() {
                "that member".into()
            } else {
                names.join(", ")
            })
        })
    }

    pub fn describe_member_link(&self, member: &str) -> Result<String> {
        let data = self.read()?;
        let account = resolve(&data, member)?;
        let sign_in = if foreign_login(&data, &account) {
            "Google"
        } else if data
            .identities
            .logins
            .values()
            .any(|login| login.account_id == account && login.issuer == identity::LINK)
        {
            "member link"
        } else if data
            .identities
            .pending_google
            .values()
            .any(|id| id == &account)
        {
            "invitation pending"
        } else {
            "unavailable"
        };
        let time = now();
        let (links, sessions) = {
            let connection = self.connection.lock().unwrap();
            let links: i64 = connection.query_row(
                "SELECT COUNT(*) FROM member_links WHERE account_id=?1 AND redeemed IS NULL AND expires>?2",
                params![account, time],
                |row| row.get(0),
            )?;
            let sessions: i64 = connection.query_row(
                "SELECT COUNT(*) FROM member_sessions WHERE account_id=?1 AND expires>?2",
                params![account, time],
                |row| row.get(0),
            )?;
            (links, sessions)
        };
        let profiles = data
            .identities
            .profiles
            .iter()
            .filter(|(key, profile)| {
                profile.account_id == account && data.members.contains_key(*key)
            })
            .map(|(key, _)| {
                format!(
                    "- {} ({}) {key}",
                    data.members[key].name,
                    data.groups.member(key)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let latest = {
            let connection = self.connection.lock().unwrap();
            connection
                .query_row(
                    "SELECT created_by, redeemed FROM member_links WHERE account_id=?1 ORDER BY created DESC LIMIT 1",
                    [&account],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .optional()?
        };
        let record = latest
            .map(|(creator, redeemed)| {
                format!(
                    "\nLatest link: {}, {}",
                    creator_name(&data, &creator),
                    if redeemed.is_some() {
                        "used"
                    } else {
                        "not used yet"
                    }
                )
            })
            .unwrap_or_default();
        Ok(format!(
            "Sign-in: {sign_in}\nUnused links: {links}\nSessions: {sessions}\nProfiles:\n{profiles}{record}"
        ))
    }

    pub fn sign_out(&self, account: Option<&str>, token: Option<&str>, time: u64) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(account) = account {
            let data = self.cached_catalog(&tx)?;
            for (key, profile) in &data.identities.profiles {
                if profile.account_id == account {
                    tx.execute("UPDATE runtime_launches SET data=json_set(data,'$.expires',?2,'$.authorized_until',0,'$.grant_expires',0)
                        WHERE json_extract(data,'$.email')=?1", params![key,time])?;
                }
            }
        }
        if let Some(token) = token.filter(|t| valid_token(t)) {
            tx.execute(
                "DELETE FROM member_sessions WHERE token_hash=?1",
                [stored_hash(token)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Read who a link would sign in as. It does not consume the link.
    pub fn preview_member_link(&self, token: &str) -> Result<Vec<(String, String)>> {
        if !valid_token(token) {
            return Err(fault(404, "This sign-in link is not valid."));
        }
        let digest = stored_hash(token)?;
        let row = {
            let connection = self.connection.lock().unwrap();
            connection
                .query_row(
                    "SELECT account_id, expires, redeemed FROM member_links WHERE token_hash=?1",
                    [&digest],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .optional()?
        };
        let Some((account, expires, redeemed)) = row else {
            return Err(fault(404, "This sign-in link is not valid."));
        };
        if redeemed.is_some() || expires <= now() {
            return Err(fault(404, "This sign-in link is not valid."));
        }
        let data = self.catalog()?;
        ensure!(
            !foreign_login(&data, &account),
            fault(403, "This sign-in link cannot be used for this account.")
        );
        let profiles = data.account_profiles(&account)?;
        ensure!(
            !profiles.is_empty(),
            fault(403, "Ask your admin for an Infuser invitation.")
        );
        Ok(profiles
            .into_iter()
            .map(|(key, _)| {
                (
                    data.members[key].name.clone(),
                    data.groups
                        .names
                        .get(data.groups.member(key))
                        .cloned()
                        .unwrap_or_else(|| data.groups.member(key).to_owned()),
                )
            })
            .collect())
    }

    pub fn account_sign_in(&self, data: &Data, account: &str) -> Result<Value> {
        let time = now();
        let (links, sessions) = {
            let connection = self.connection.lock().unwrap();
            let links = {
                let mut statement = connection.prepare(
                    "SELECT created, created_by, expires, redeemed FROM member_links WHERE account_id=?1 ORDER BY created DESC LIMIT 3",
                )?;
                statement
                    .query_map(params![account], |row| {
                        Ok((
                            row.get::<_, u64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, u64>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let sessions = {
                let mut statement = connection.prepare(
                    "SELECT created, expires FROM member_sessions WHERE account_id=?1 AND expires>?2 ORDER BY created DESC LIMIT 8",
                )?;
                statement
                    .query_map(params![account, time], |row| {
                        Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            (links, sessions)
        };
        let kind = if foreign_login(data, account) {
            "google"
        } else if data
            .identities
            .logins
            .values()
            .any(|login| login.account_id == account && login.issuer == identity::LINK)
        {
            "member_link"
        } else if data
            .identities
            .pending_google
            .values()
            .any(|id| id == account)
        {
            "pending"
        } else {
            "unavailable"
        };
        Ok(json!({
            "kind": kind,
            "sessions": sessions.into_iter().map(|(created, expires)| json!({"created": created, "expires": expires})).collect::<Vec<_>>(),
            "links": links.into_iter().map(|(created, creator, expires, redeemed)| json!({
                "created": created,
                "created_by": creator_name(data, &creator),
                "expires": expires,
                "redeemed": redeemed,
            })).collect::<Vec<_>>(),
        }))
    }
}

fn creator_name(data: &Data, stored: &str) -> String {
    data.members
        .get(stored)
        .map(|member| member.name.clone())
        .unwrap_or_else(|| "Server operator".into())
}
