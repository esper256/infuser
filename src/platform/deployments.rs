//! Mutable play state is scoped to a deployment; artifacts and ownership stay on the source app.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub app: String,
    pub group: String,
    pub active: App,
}

impl Build {
    pub fn visit_unavailable(&self) -> Option<&'static str> {
        (self.server.is_some() && !self.manifest.dedicated_sessions()).then_some(
            "This server game needs the managed WebSocket adapter before it can join visits.",
        )
    }
}

impl Launch {
    pub fn runtime_id(&self) -> &str {
        self.deployment.as_deref().unwrap_or(&self.app)
    }
}

impl Data {
    pub fn deployment_for<'a>(&'a self, app: &'a str, group: &str) -> Option<&'a str> {
        if self.groups.app(app) == group {
            return Some(app);
        }
        self.deployments
            .iter()
            .find(|(_, d)| d.app == app && d.group == group)
            .map(|(id, _)| id.as_str())
    }
    pub fn source_app<'a>(&'a self, runtime: &'a str) -> &'a str {
        self.deployments.get(runtime).map_or(runtime, |d| &d.app)
    }
    pub fn deployment_group<'a>(&'a self, runtime: &'a str) -> &'a str {
        self.deployments
            .get(runtime)
            .map_or_else(|| self.groups.app(runtime), |d| &d.group)
    }
    pub fn deployed(&self, runtime: &str) -> Option<&App> {
        self.deployments
            .get(runtime)
            .map(|d| &d.active)
            .or_else(|| self.apps.get(runtime))
    }
    pub fn ensure_deployment(&mut self, app: &str, group: &str) -> Result<String> {
        ensure!(
            self.groups.names.contains_key(group),
            crate::portal::fault(404, "Group not found.")
        );
        let active = self
            .apps
            .get(app)
            .ok_or_else(|| crate::portal::fault(404, "Game is not published."))?
            .clone();
        if self.groups.app(app) == group {
            return Ok(app.into());
        }
        if let Some((id, _)) = self
            .deployments
            .iter()
            .find(|(_, d)| d.app == app && d.group == group)
        {
            return Ok(id.clone());
        }
        let id = format!("deployment-{}", &secret()[..20]);
        ensure!(
            !self.apps.contains_key(&id)
                && !self.groups.apps.contains_key(&id)
                && !self.deployments.contains_key(&id),
            "deployment ID collision"
        );
        self.deployments.insert(
            id.clone(),
            Deployment {
                app: app.into(),
                group: group.into(),
                active,
            },
        );
        Ok(id)
    }
}
