//! Private-group boundaries are platform policy, never supplied by game manifests.
use super::*;
use std::collections::BTreeSet;
pub const HOME: &str = "home";
#[derive(Clone, Serialize, Deserialize)]
pub struct Groups {
    pub names: BTreeMap<String, String>,
    pub members: BTreeMap<String, String>,
    pub apps: BTreeMap<String, String>,
    pub operators: BTreeSet<String>,
}
impl Default for Groups {
    fn default() -> Self {
        Self {
            names: [(HOME.into(), "Home".into())].into(),
            members: BTreeMap::new(),
            apps: BTreeMap::new(),
            operators: BTreeSet::new(),
        }
    }
}
impl Groups {
    pub fn member(&self, email: &str) -> &str {
        self.members.get(email).map(String::as_str).unwrap_or(HOME)
    }
    pub fn app(&self, app: &str) -> &str {
        self.apps.get(app).map(String::as_str).unwrap_or(HOME)
    }
    pub fn allows(&self, email: &str, app: &str) -> bool {
        self.member(email) == self.app(app)
    }
}
impl Data {
    /// Shared policy for credential inventory and revocation. The caller must
    /// already be an enabled admin in their home group, outside inspection.
    pub fn can_manage_credential(&self, email: &str, credential: &Credential) -> bool {
        match credential.role.as_str() {
            "publish" => self.groups.allows(email, &credential.app),
            "metadata-group" => credential.app == self.groups.member(email),
            "metadata" => self.operator(email),
            _ => false,
        }
    }

    pub fn require_app_group(&self, email: &str, app: &str) -> Result<()> {
        ensure!(
            self.groups.allows(email, app),
            crate::portal::fault(404, "Game not found.")
        );
        Ok(())
    }
    pub fn reserve_app(&mut self, app: &str, group: &str) -> Result<()> {
        ensure!(
            !self.deployments.contains_key(app),
            crate::portal::fault(409, "Reserved deployment identifier.")
        );
        let known = self.groups.apps.contains_key(app)
            || self.apps.contains_key(app)
            || self.owners.contains_key(app)
            || self
                .credentials
                .values()
                .any(|c| c.app == app && matches!(c.role.as_str(), "publish" | "server"));
        ensure!(
            !known || self.groups.app(app) == group,
            crate::portal::fault(404, "Game not found.")
        );
        ensure!(
            self.groups.names.contains_key(group),
            crate::portal::fault(404, "Group not found.")
        );
        self.groups.apps.insert(app.into(), group.into());
        Ok(())
    }
    pub fn invite_to_group(
        &mut self,
        group: &str,
        email: &str,
        name: &str,
        admin: bool,
        enabled: bool,
    ) -> Result<()> {
        let email = email.trim().to_lowercase();
        let name = name.trim();
        ensure!(
            email.contains('@') && email.len() < 254 && !email.chars().any(char::is_whitespace),
            crate::portal::fault(400, "Use a Google email address.")
        );
        let email = self.invitation_key(&email)?;
        ensure!(
            !name.is_empty() && name.chars().count() <= 40 && !name.chars().any(char::is_control),
            crate::portal::fault(400, "Use a name of 1–40 characters.")
        );
        ensure!(
            self.groups.names.contains_key(group),
            crate::portal::fault(404, "Group not found.")
        );
        ensure!(
            !self.members.contains_key(&email) || self.groups.member(&email) == group,
            crate::portal::fault(409, "This account cannot be invited to this group.")
        );
        let avatar = self.members.get(&email).and_then(|m| m.avatar.clone());
        self.members.insert(
            email.clone(),
            Member {
                name: name.into(),
                admin,
                enabled,
                avatar,
            },
        );
        self.groups.members.insert(email, group.into());
        Ok(())
    }
}
