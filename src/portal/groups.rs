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
        .route(
            "/api/operator/update",
            get(update_status).post(authorize_update),
        )
        .layer(DefaultBodyLimit::max(4096))
}
async fn update_status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.operator(&h, false).await?;
    let record = p.controller.deployment.read();
    let mut reasons = crate::operations::update_activity(&p)?
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if p.controller.maintenance.publishing() > 0 {
        reasons.push("Waiting for game uploads or publishing to finish".into());
    } else if p.controller.maintenance.working() > 1 {
        reasons.push("Waiting for requests to finish".into());
    }
    if p.controller.maintenance.active()
        || record
            .report
            .as_ref()
            .is_some_and(|r| r.maintenance_blocked)
    {
        reasons.push("Waiting for another host maintenance job".into());
    }
    let stale = now().saturating_sub(record.checked_at) >= 180;
    if stale {
        reasons.insert(0, "Waiting for a fresh update check".into());
    }
    Ok(reply(
        json!({"configured":record.report.is_some(),"current_build":crate::BUILD_SHA,
        "available":record.available(),"available_since":record.available_since,"checked_at":record.checked_at,
        "report":record.report,"stale":stale,"reasons":reasons,
        "authorized_until":record.grant.filter(|g|g.expires>now() && g.current_build==crate::BUILD_SHA).map(|g|g.expires)}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizeUpdate {
    request_id: String,
    current_build: String,
}
async fn authorize_update(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<AuthorizeUpdate>,
) -> Answer {
    let actor = p.operator(&h, true).await?;
    p.controller
        .deployment
        .authorize(&body.request_id, &body.current_build, &actor)?;
    tracing::info!(actor=%actor, "operator authorized one deployment interruption");
    Ok(reply(json!({"ok":true})))
}
async fn health(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.operator(&h, false).await?;
    let path = p.config().controller.state_dir.join("host-health.json");
    let report = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<crate::host_health::Report>(&bytes)
            .context("invalid host health report")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(reply(json!({"configured":false,"voice":p.voice_health()})));
        }
        Err(error) => return Err(error.into()),
    };
    Ok(reply(
        json!({"configured":true,"stale":now().saturating_sub(report.at)>180,"report":report,"voice":p.voice_health()}),
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
        d.groups.names.insert(id.clone(), name.into());
        d.invite_to_group(&id, &body.admin_email, &body.admin_name, true, true)
    })?;
    Ok(reply(json!({"id":id,"name":name})))
}
