//! Permanent accounts and profiles. Legacy member keys are immutable storage
//! handles, not login identifiers. Never derive a profile from a login email.
use super::*;
use std::collections::BTreeSet;

pub const VERSION: u32 = 1;
pub const GOOGLE: &str = "https://accounts.google.com";

#[derive(Clone, Serialize, Deserialize)]
pub struct Account {
    pub created: u64,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub account_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Login {
    pub issuer: String,
    pub subject: String,
    pub account_id: String,
    pub email: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Identities {
    pub accounts: BTreeMap<String, Account>,
    // Key is the existing member/storage handle. New profiles may use opaque
    // handles; neither shape nor spelling has authentication significance.
    pub profiles: BTreeMap<String, Profile>,
    pub logins: BTreeMap<String, Login>,
    pub pending_google: BTreeMap<String, String>,
}
impl Identities {
    fn login_key(issuer: &str, subject: &str) -> String {
        hash(&serde_json::to_string(&(issuer, subject)).expect("string pair"))
    }
    pub fn login(&self, issuer: &str, subject: &str) -> Option<&Login> {
        self.logins.get(&Self::login_key(issuer, subject))
    }
    // Only the legacy migration and the current single-profile invitation path
    // use this. Future profile creation must generate a fresh ID, not reuse email.
    pub(super) fn initialize_profiles(&mut self, members: &BTreeMap<String, Member>) {
        for key in members.keys() {
            if self.profiles.contains_key(key) {
                continue;
            }
            let account = format!("account-{}", &secret()[..32]);
            self.accounts.insert(
                account.clone(),
                Account {
                    created: now(),
                    enabled: true,
                },
            );
            self.profiles.insert(
                key.clone(),
                Profile {
                    id: hash(key),
                    account_id: account.clone(),
                },
            );
            self.pending_google.insert(key.clone(), account);
        }
    }
    pub fn validate(
        &self,
        members: &BTreeMap<String, Member>,
        groups: &groups::Groups,
    ) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut memberships = BTreeSet::new();
        for (key, profile) in &self.profiles {
            ensure!(
                self.accounts.contains_key(&profile.account_id),
                "profile references a missing account"
            );
            ensure!(
                profile.id.len() == 64
                    && profile
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "invalid permanent profile ID"
            );
            ensure!(ids.insert(&profile.id), "duplicate permanent profile ID");
            if members.contains_key(key) {
                ensure!(
                    groups.names.contains_key(groups.member(key)),
                    "profile references a missing group"
                );
                ensure!(
                    memberships.insert((&profile.account_id, groups.member(key))),
                    "an account can have only one profile per group"
                );
            }
        }
        for (key, login) in &self.logins {
            ensure!(
                self.accounts.contains_key(&login.account_id)
                    && *key == Self::login_key(&login.issuer, &login.subject),
                "invalid linked identity"
            );
        }
        for account in self.pending_google.values() {
            ensure!(
                self.accounts.contains_key(account),
                "invitation references a missing account"
            );
            ensure!(
                !self
                    .logins
                    .values()
                    .any(|l| &l.account_id == account && l.issuer == GOOGLE),
                "claimed Google invitation retained"
            );
        }
        Ok(())
    }
}
impl Data {
    pub fn account_profiles(&self, account: &str) -> Result<Vec<(&str, &Profile)>> {
        ensure!(
            self.identities
                .accounts
                .get(account)
                .is_some_and(|a| a.enabled),
            crate::portal::fault(403, "Account disabled.")
        );
        Ok(self
            .identities
            .profiles
            .iter()
            .filter(|(key, p)| {
                p.account_id == account && self.members.get(*key).is_some_and(|m| m.enabled)
            })
            .map(|(k, p)| (k.as_str(), p))
            .collect())
    }
    pub fn select_profile(&self, account: &str, player: Option<&str>) -> Result<String> {
        let profiles = self.account_profiles(account)?;
        ensure!(
            !profiles.is_empty(),
            crate::portal::fault(403, "Ask your admin for an Infuser invitation.")
        );
        if let Some(player) = player {
            return profiles
                .iter()
                .find(|(_, p)| p.id == player)
                .map(|(key, _)| key.to_string())
                .ok_or_else(|| crate::portal::fault(409, "Choose your Infuser profile."));
        }
        ensure!(
            profiles.len() == 1,
            crate::portal::fault(409, "Choose your Infuser profile.")
        );
        Ok(profiles[0].0.to_string())
    }
    /// Invitation lookup only: never authenticates or links provider identities.
    pub fn invitation_account(&self, query: &str) -> Result<Option<String>> {
        let query = query.trim().to_lowercase();
        if let Some(profile) = self.identities.profiles.get(&query) {
            return Ok(Some(profile.account_id.clone()));
        }
        let accounts: BTreeSet<_> = self
            .identities
            .logins
            .values()
            .filter(|l| l.email == query)
            .map(|l| l.account_id.clone())
            .chain(self.identities.pending_google.get(&query).cloned())
            .collect();
        ensure!(
            accounts.len() <= 1,
            crate::portal::fault(409, "Use an existing profile handle for this account.")
        );
        Ok(accounts.into_iter().next())
    }
    pub fn group_member_key(&self, query: &str, group: &str) -> Result<Option<String>> {
        let handle = query.trim().to_lowercase();
        if self.members.contains_key(&handle) && !handle.contains('@') {
            return Ok((self.groups.member(&handle) == group).then_some(handle));
        }
        let Some(account) = self.invitation_account(query)? else {
            return Ok(None);
        };
        Ok(self
            .identities
            .profiles
            .iter()
            .find(|(key, p)| {
                p.account_id == account
                    && self.members.contains_key(*key)
                    && self.groups.member(key) == group
            })
            .map(|(key, _)| key.clone()))
    }
    pub(crate) fn initialize_identities(&mut self) -> Result<()> {
        self.identities.initialize_profiles(&self.members);
        // Old operator entries name member handles. Persist account authority.
        self.groups.operators = self
            .groups
            .operators
            .iter()
            .map(|key| {
                self.identities
                    .profiles
                    .get(key)
                    .map(|p| p.account_id.clone())
                    .unwrap_or_else(|| key.clone())
            })
            .collect();
        self.identities.validate(&self.members, &self.groups)
    }
    pub fn player_id(&self, member_key: &str) -> String {
        self.identities
            .profiles
            .get(member_key)
            .expect("persisted profile identity")
            .id
            .clone()
    }
    pub fn operator(&self, member_key: &str) -> bool {
        self.identities
            .profiles
            .get(member_key)
            .is_some_and(|profile| self.groups.operators.contains(&profile.account_id))
    }
    pub fn set_operator(&mut self, member_key: &str, enabled: bool) -> Result<()> {
        self.initialize_identities()?;
        let account = self
            .identities
            .profiles
            .get(member_key)
            .context("unknown profile")?
            .account_id
            .clone();
        if enabled {
            self.groups.operators.insert(account);
        } else {
            self.groups.operators.remove(&account);
        }
        Ok(())
    }
    pub fn single_profile(&self, account: &str) -> Result<String> {
        self.select_profile(account, None)
    }
    /// Operator/admin lookup only. It never authenticates an incoming login.
    pub fn member_key(&self, email_or_key: &str) -> Result<String> {
        let query = email_or_key.trim().to_lowercase();
        if self.members.contains_key(&query) {
            return Ok(query);
        }
        let accounts: BTreeSet<_> = self
            .identities
            .logins
            .values()
            .filter(|l| l.email == query)
            .map(|l| l.account_id.as_str())
            .collect();
        ensure!(
            accounts.len() <= 1,
            "ambiguous account email; use its existing profile handle"
        );
        if let Some(account) = accounts.first() {
            let profiles: Vec<_> = self
                .identities
                .profiles
                .iter()
                .filter(|(key, p)| p.account_id == **account && self.members.contains_key(*key))
                .collect();
            ensure!(profiles.len() == 1, "profile selection required");
            return Ok(profiles[0].0.clone());
        }
        anyhow::bail!("member not found")
    }
    pub fn invitation_key(&self, email: &str) -> Result<String> {
        let query = email.trim().to_lowercase();
        if self.members.contains_key(&query)
            || self.identities.logins.values().any(|l| l.email == query)
        {
            self.member_key(&query)
        } else {
            Ok(query)
        }
    }
    pub fn profile_enabled(&self, key: &str) -> bool {
        self.members.get(key).is_some_and(|m| m.enabled)
            && self
                .identities
                .profiles
                .get(key)
                .and_then(|p| self.identities.accounts.get(&p.account_id))
                .is_some_and(|a| a.enabled)
    }
    pub fn contact_email<'a>(&'a self, key: &'a str) -> &'a str {
        let Some(profile) = self.identities.profiles.get(key) else {
            return key;
        };
        self.identities
            .logins
            .values()
            .find(|l| l.account_id == profile.account_id && l.issuer == GOOGLE)
            .map(|l| l.email.as_str())
            .or_else(|| {
                self.identities
                    .pending_google
                    .iter()
                    .find(|(_, account)| **account == profile.account_id)
                    .map(|(email, _)| email.as_str())
            })
            .unwrap_or(key)
    }
    fn bind_google(&mut self, subject: &str, email: &str) -> Result<String> {
        if let Some(login) = self.identities.login(GOOGLE, subject) {
            let account = login.account_id.clone();
            self.identities
                .logins
                .get_mut(&Identities::login_key(GOOGLE, subject))
                .unwrap()
                .email = email.into();
            return Ok(account);
        }
        // Email is used once to accept an existing Google invitation. Once
        // claimed, neither an email match nor an email change can link accounts.
        let account = self
            .identities
            .pending_google
            .get(email)
            .cloned()
            .ok_or_else(|| {
                crate::portal::fault(403, "Ask your admin for an Infuser invitation.")
            })?;
        ensure!(
            !self
                .identities
                .logins
                .values()
                .any(|l| l.account_id == account && l.issuer == GOOGLE),
            "Google identity already linked"
        );
        self.identities.logins.insert(
            Identities::login_key(GOOGLE, subject),
            Login {
                issuer: GOOGLE.into(),
                subject: subject.into(),
                account_id: account.clone(),
                email: email.into(),
            },
        );
        self.identities.pending_google.retain(|_, a| a != &account);
        Ok(account)
    }
}
impl Database {
    /// Arguments come only from the successful, configured Google auth proxy
    /// response. No browser-supplied identity header is trusted.
    pub fn google_account(&self, subject: &str, email: &str) -> Result<String> {
        ensure!(
            !subject.is_empty() && subject.len() <= 255 && !subject.chars().any(char::is_control),
            crate::portal::fault(
                401,
                "Sign in again: the provider did not supply a stable identity."
            )
        );
        let email = email.trim().to_lowercase();
        ensure!(
            email.contains('@') && email.len() < 254 && !email.chars().any(char::is_whitespace),
            crate::portal::fault(401, "The provider did not supply a valid email.")
        );
        let data = self.read()?;
        if let Some(login) = data.identities.login(GOOGLE, subject)
            && login.email == email
        {
            return Ok(login.account_id.clone());
        }
        self.edit(|d| d.bind_google(subject, &email))
    }
    pub fn google_profile(&self, subject: &str, email: &str) -> Result<String> {
        let account = self.google_account(subject, email)?;
        self.read()?.single_profile(&account)
    }
}

/// Read-only preflight, usable before activation and rollback. Never migrates.
pub fn check_state(root: &Path) -> Result<()> {
    let path = root.join("platform.sqlite");
    if !path.exists() {
        return Ok(());
    }
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='platform')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let raw: Option<String> = c
        .query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let Some(raw) = raw else {
        return Ok(());
    };
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let version = match value.get("identity_version") {
        None => 0,
        Some(version) => version.as_u64().context("invalid identity state version")?,
    };
    ensure!(
        version <= VERSION as u64,
        "This state requires a newer Infuser identity version; do not downgrade."
    );
    Ok(())
}

/// Migrate a private, disposable SQLite snapshot on the same host. Never open
/// live state through Database::open: that would run the actual migration.
pub fn rehearse(root: &Path) -> Result<usize> {
    check_state(root)?;
    let source = Connection::open_with_flags(
        root.join("platform.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let copy = tempfile::Builder::new()
        .prefix("infuser-identity-")
        .tempdir()?;
    let target = copy.path().join("platform.sqlite");
    source.backup("main", &target, None::<fn(rusqlite::backup::Progress)>)?;
    drop(source);
    let before = Connection::open(&target)?;
    let raw: String = before.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let catalog: serde_json::Value = serde_json::from_str(&raw)?;
    let members = catalog
        .get("profiles")
        .or_else(|| catalog.get("members"))
        .and_then(|v| v.as_object())
        .context("missing profile catalog")?;
    // These contain saves, native devices, channel history, visits and other
    // durable references. Identity migration must not rewrite their contents.
    let tables: Vec<String> = before.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let fingerprints = |c: &Connection| -> Result<BTreeMap<String, String>> {
        let mut result = BTreeMap::new();
        for table in &tables {
            if matches!(
                table.as_str(),
                "platform" | "platform_catalog" | "private_groups" | "catalog_revision"
            ) {
                continue;
            }
            let mut statement =
                c.prepare(&format!("SELECT * FROM \"{}\"", table.replace('"', "\"\"")))?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            let mut values = Vec::new();
            while let Some(row) = rows.next()? {
                let mut value = String::new();
                for i in 0..columns {
                    use std::fmt::Write;
                    write!(&mut value, "{:?};", row.get_ref(i)?)?;
                }
                values.push(hash(&value));
            }
            values.sort();
            result.insert(table.clone(), hash(&values.join("/")));
        }
        Ok(result)
    };
    let saved = fingerprints(&before)?;
    let policy: Option<String> = if tables.iter().any(|t| t == "private_groups") {
        before
            .query_row("SELECT data FROM private_groups WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?
    } else {
        None
    };
    drop(before);
    let database = Database::open(copy.path())?;
    let data = database.read()?;
    ensure!(
        data.members.len() == members.len(),
        "migration changed profile count"
    );
    for key in members.keys() {
        if catalog.get("identity_version").is_none() {
            ensure!(
                data.player_id(key) == hash(key),
                "migration changed an existing player ID"
            );
        }
    }
    let after = Connection::open(&target)?;
    let raw: String = after.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let migrated: serde_json::Value = serde_json::from_str(&raw)?;
    if catalog
        .get("identity_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        > 0
    {
        ensure!(
            migrated["identities"] == catalog["identities"],
            "reopening changed permanent identities"
        );
    }
    if let Some(policy) = policy {
        let mut previous: groups::Groups = serde_json::from_str(&policy)?;
        previous.operators = previous
            .operators
            .iter()
            .map(|key| {
                data.identities
                    .profiles
                    .get(key)
                    .map(|p| p.account_id.clone())
                    .unwrap_or_else(|| key.clone())
            })
            .collect();
        ensure!(
            serde_json::to_value(previous)? == serde_json::to_value(&data.groups)?,
            "migration changed group policy"
        );
    }
    for (key, value) in catalog.as_object().context("invalid catalog")? {
        if matches!(key.as_str(), "identity_version" | "identities") {
            continue;
        }
        let new_key = if key == "members" { "profiles" } else { key };
        ensure!(
            migrated.get(new_key) == Some(value),
            "migration changed catalog field {key}"
        );
    }
    ensure!(
        fingerprints(&after)? == saved,
        "migration changed durable side-table contents"
    );
    Ok(data.members.len())
}
