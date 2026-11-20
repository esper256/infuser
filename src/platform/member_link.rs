//! One-time operator links for an invited member who does not use Google.
//! Only SHA-256 hashes are stored. Raw link and session tokens stay out of
//! this module's errors and must not be logged.
use super::*;
use crate::portal::fault;

pub const COOKIE: &str = "__Host-infuser_member";
/// Two hours. The cookie is a bearer credential for a full member, held by an
/// assistant rather than Google. That covers browsing and a slow game, and it
/// is far shorter than the seven-day Google session. Revoke ends it immediately.
pub const SESSION_SECONDS: u64 = 2 * 60 * 60;
/// Ten minutes and single use. Long enough to carry the link from an SSH
/// session to the assistant, short enough that a forgotten link dies on its own.
pub const LINK_SECONDS: u64 = 10 * 60;

pub enum Redeemed {
    Session(String),
    Refused,
}

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS member_links (
            token_hash TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            expires INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS member_sessions (
            token_hash TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            expires INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub(super) fn expire(tx: &Transaction<'_>, time: u64) -> Result<()> {
    tx.execute("DELETE FROM member_links WHERE expires<=?1", [time])?;
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

    pub fn mint_member_link(&self, domain: &str, member: &str) -> Result<String> {
        ensure!(crate::config::hostname(domain), "invalid site domain");
        let token = secret();
        let digest = stored_hash(&token)?;
        let expires = now() + LINK_SECONDS;
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
            tx.execute("DELETE FROM member_links WHERE account_id=?1", [&account])?;
            tx.execute(
                "INSERT INTO member_links(token_hash, account_id, expires) VALUES(?1,?2,?3)",
                params![digest, account, expires],
            )?;
            Ok(())
        })?;
        Ok(format!("https://{domain}/member-link/{token}"))
    }

    pub fn redeem_member_link(&self, token: &str) -> Result<Redeemed> {
        if !valid_token(token) {
            return Err(fault(404, "This sign-in link is not valid."));
        }
        let digest = stored_hash(token)?;
        let time = now();
        self.edit_transaction(|data, tx| {
            let row: Option<(String, u64)> = tx
                .query_row(
                    "SELECT account_id, expires FROM member_links WHERE token_hash=?1",
                    [&digest],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((account, expires)) = row else {
                return Err(fault(404, "This sign-in link is not valid."));
            };
            if expires <= time {
                return Err(fault(404, "This sign-in link is not valid."));
            }
            if foreign_login(data, &account) {
                tx.execute("DELETE FROM member_links WHERE token_hash=?1", [&digest])?;
                return Ok(Redeemed::Refused);
            }
            data.bind_member_link(&account)?;
            let removed = tx.execute(
                "DELETE FROM member_links WHERE token_hash=?1 AND expires>?2",
                params![digest, time],
            )?;
            ensure!(removed == 1, fault(404, "This sign-in link is not valid."));
            let session = secret();
            let session_hash = stored_hash(&session)?;
            tx.execute(
                "INSERT INTO member_sessions(token_hash, account_id, expires) VALUES(?1,?2,?3)",
                params![session_hash, account, time + SESSION_SECONDS],
            )?;
            Ok(Redeemed::Session(session))
        })
    }

    pub fn revoke_member_links(&self, member: &str) -> Result<String> {
        self.edit_transaction(|data, tx| {
            let account = resolve(data, member)?;
            tx.execute("DELETE FROM member_links WHERE account_id=?1", [&account])?;
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
                "SELECT COUNT(*) FROM member_links WHERE account_id=?1 AND expires>?2",
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
        Ok(format!(
            "Sign-in: {sign_in}\nUnused links: {links}\nSessions: {sessions}\nProfiles:\n{profiles}"
        ))
    }
}
