use super::*;
use anyhow::Context;
pub(super) const INSPECT: &str = "x-infuser-inspect-group";
pub(super) const VISIT: &str = "x-infuser-visit";

impl Platform {
    pub(super) fn group_view(&self, h: &HeaderMap, email: &str) -> Result<(String, bool)> {
        let d = self.db.read()?;
        if let Some(value) = h.get(VISIT) {
            ensure!(
                !h.contains_key(INSPECT) && h.get_all(VISIT).iter().count() == 1,
                fault(400, "Choose one play context.")
            );
            let id = value.to_str().map_err(|_| fault(400, "Invalid visit."))?;
            let visit = d
                .visit_access(id, email, now())
                .ok_or_else(|| fault(403, "This visit has ended or is unavailable."))?;
            return Ok((visit.host.clone(), false));
        }
        let Some(value) = h.get(INSPECT) else {
            return Ok((d.groups.member(email).into(), false));
        };
        ensure!(
            h.get_all(INSPECT).iter().count() == 1 && d.operator(email),
            fault(403, "Operator inspection is unavailable.")
        );
        let group = value.to_str().map_err(|_| fault(400, "Invalid group."))?;
        ensure!(
            d.groups.names.contains_key(group),
            fault(404, "Group not found.")
        );
        Ok((group.into(), true))
    }
    pub(super) fn visible_app(&self, h: &HeaderMap, email: &str, app: &str) -> Result<()> {
        let (group, inspecting) = self.group_view(h, email)?;
        let d = self.db.read()?;
        ensure!(
            (d.groups.app(app) == group
                || d.active_visit(&group, now())
                    .is_some_and(|(_, v)| v.games.contains(app)))
                && d.apps.get(app).is_some_and(|a| a.listed
                    || inspecting
                    || (d.groups.app(app) == group
                        && d.groups.member(email) == group
                        && d.members[email].admin)),
            fault(404, "Game not found.")
        );
        Ok(())
    }
    pub(super) async fn operator(&self, h: &HeaderMap, write: bool) -> Result<String> {
        let email = self.member(h, write).await?;
        ensure!(
            self.db.read()?.operator(&email),
            fault(403, "Operator access required.")
        );
        Ok(email)
    }
}
pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route("/api/operator/groups", get(list).post(create))
        .route("/api/operator/health", get(health))
        .layer(DefaultBodyLimit::max(4096))
}
async fn health(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.operator(&h, false).await?;
    let path = p.config().controller.state_dir.join("host-health.json");
    let report = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<crate::host_health::Report>(&bytes)
            .context("invalid host health report")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(reply(json!({"configured":false})));
        }
        Err(error) => return Err(error.into()),
    };
    Ok(reply(
        json!({"configured":true,"stale":now().saturating_sub(report.at)>180,"report":report}),
    ))
}
async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.operator(&h, false).await?;
    let d = p.db.read()?;
    Ok(reply(
        json!({"groups":d.groups.names.iter().map(|(id,name)| json!({"id":id,"name":name,"home":d.groups.member(&email)==id})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    name: String,
    admin_email: String,
    admin_name: String,
}
async fn create(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Create>) -> Answer {
    p.operator(&h, true).await?;
    ensure!(
        p.config().controller.private_groups,
        fault(
            409,
            "Enable private_groups in the host configuration first."
        )
    );
    let name = body.name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 60 && !name.chars().any(char::is_control),
        fault(422, "Use a group name of 1–60 characters.")
    );
    let id = format!("group-{}", &secret()[..12]);
    p.db.edit(|d| {
        ensure!(
            !d.members
                .contains_key(&body.admin_email.trim().to_lowercase()),
            fault(409, "Use an account not already assigned to a group.")
        );
        d.groups.names.insert(id.clone(), name.into());
        d.invite_to_group(&id, &body.admin_email, &body.admin_name, true, true)
    })?;
    Ok(reply(json!({"id":id,"name":name})))
}
