//! Operator-only host state and maintenance coordination. Never a game API.
use crate::{controller::now, portal::Platform};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone)]
struct App {
    platform: Platform,
    token: [u8; 32],
}
fn authorized(app: &App, headers: &HeaderMap) -> bool {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    bool::from(app.token.ct_eq(&digest))
}
pub fn routes(platform: Platform, token: &str) -> Router {
    Router::new()
        .route("/v1/maintenance", post(begin).delete(end))
        .route("/v1/operations", get(status))
        .route("/v1/deployment", post(deployment_report))
        .route("/v1/deployment/admit", post(deployment_admit))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(App {
            platform,
            token: Sha256::digest(token.as_bytes()).into(),
        })
}
#[derive(Deserialize)]
struct IncomingReport {
    release: Option<String>,
    #[serde(default)]
    built_at: Option<u64>,
    phase: crate::deployment::Phase,
    #[serde(default)]
    maintenance_blocked: bool,
    /// Absent when an older updater cannot say. Present, even if empty, when
    /// this release's manifest was read.
    #[serde(default)]
    host_steps: Option<Vec<crate::deployment::HostStep>>,
}
async fn deployment_report(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<IncomingReport>,
) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let announced = body.release.clone();
    let report = crate::deployment::Report {
        release: body.release,
        built_at: body.built_at,
        phase: body.phase,
        maintenance_blocked: body.maintenance_blocked,
    };
    let saved = app.platform.controller.deployment.report(report);
    if let (Ok(()), Some(steps)) = (&saved, body.host_steps.as_ref())
        && let Some(release) = app
            .platform
            .controller
            .deployment
            .read()
            .report
            .as_ref()
            .and_then(|report| report.release.clone())
    {
        let _ = app
            .platform
            .controller
            .deployment
            .note_host(&release, steps);
    }
    if saved.is_ok()
        && let Err(error) = crate::platform::releases::consider(
            &app.platform.db,
            body.phase,
            announced.as_deref(),
            crate::BUILD_SHA,
            &crate::changelog::family_lines(),
            now(),
        )
    {
        tracing::warn!(%error, "platform update was not announced");
    }
    match saved {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(_) => error(StatusCode::BAD_REQUEST, "Could not save deployment report"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentAdmission {
    release: String,
    #[serde(default)]
    force: bool,
}
async fn deployment_admit(
    State(app): State<App>,
    headers: HeaderMap,
    Json(body): Json<DeploymentAdmission>,
) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let p = &app.platform;
    let decision = match crate::update_admission::admission(p) {
        Ok(decision) => decision,
        Err(error_value) => {
            tracing::warn!(error=%error_value, "deployment admission failed");
            return error(
                StatusCode::CONFLICT,
                "deployment admission unavailable; check again",
            );
        }
    };
    let waiting = decision.waiting.clone();
    let mut acquired = None;
    let result = p.controller.deployment.edit(|record| {
        anyhow::ensure!(
            record.report.as_ref().and_then(|r| r.release.as_ref()) == Some(&body.release)
                && now().saturating_sub(record.checked_at) < 180,
            "candidate report changed or expired"
        );
        let permitted = if record.authorized() {
            let actor = &record.grant.as_ref().unwrap().actor;
            let data = p.db.read()?;
            data.operator(actor) && data.profile_enabled(actor)
        } else {
            false
        };
        let force = body.force || permitted;
        let lease = if force {
            p.controller.maintenance.override_update()
        } else if !decision.ready {
            None
        } else {
            p.controller
                .maintenance
                .begin_update("update", decision.interrupt_publishing)?
        };
        if let Some(lease) = &lease {
            acquired = Some(lease.id.clone());
            record.grant = None;
            if let Some(report) = &mut record.report {
                report.phase = crate::deployment::Phase::Activating;
            }
        }
        Ok((lease, force))
    });
    match result {
        Ok((Some(lease), forced)) => {
            tracing::info!(release=%body.release, forced, "deployment admitted");
            Json(json!({"lease":lease,"forced":forced})).into_response()
        }
        Ok((None, _)) => {
            let waiting = if waiting.is_empty() {
                "Something started using the server. The update will try again."
            } else {
                waiting.as_str()
            };
            tracing::info!(reason = waiting, "deployment waiting");
            error(StatusCode::CONFLICT, waiting)
        }
        Err(error_value) => {
            if let Some(id) = acquired {
                p.controller.maintenance.end(&id);
            }
            tracing::warn!(error=%error_value,"deployment admission failed");
            error(
                StatusCode::CONFLICT,
                "deployment admission unavailable; check again",
            )
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    purpose: String,
    #[serde(default)]
    force: bool,
}
fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        [("cache-control", "no-store")],
        Json(json!({"error":message})),
    )
        .into_response()
}
async fn begin(State(app): State<App>, headers: HeaderMap, Json(body): Json<Begin>) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let p = &app.platform;
    // `update` is the legacy name used by already-installed reboot and Ansible
    // scripts. It stays on the strict host policy. Named `reboot` and `ansible`
    // use that same policy unless the release manifest says the step is required
    // for the version to take effect.
    let Some(step) = host_step(&body.purpose) else {
        return error(StatusCode::BAD_REQUEST, "unknown maintenance purpose");
    };
    if body.force && step != HostKind::Legacy {
        return error(
            StatusCode::BAD_REQUEST,
            "only a legacy forced update may skip the strict wait",
        );
    }
    if matches!(step, HostKind::Reboot | HostKind::Ansible)
        && p.controller
            .deployment
            .host_required(step.as_step().unwrap())
    {
        return release_host(p, step.as_step().unwrap());
    }
    strict_host(p, &body.purpose, body.force, step)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum HostKind {
    Backup,
    Legacy,
    Reboot,
    Ansible,
}
impl HostKind {
    fn as_step(self) -> Option<crate::deployment::HostStep> {
        match self {
            Self::Reboot => Some(crate::deployment::HostStep::Reboot),
            Self::Ansible => Some(crate::deployment::HostStep::Ansible),
            _ => None,
        }
    }
    fn tracked(self) -> Option<crate::deployment::HostStep> {
        self.as_step()
    }
}
fn host_step(purpose: &str) -> Option<HostKind> {
    match purpose {
        "backup" => Some(HostKind::Backup),
        "update" => Some(HostKind::Legacy),
        "reboot" => Some(HostKind::Reboot),
        "ansible" => Some(HostKind::Ansible),
        _ => None,
    }
}
fn strict_idle(p: &Platform, force: bool) -> anyhow::Result<bool> {
    let state = p.controller.snapshot();
    let data = p.db.read()?;
    let busy = state.build_reservation.is_some()
        || data.server_builds.values().any(|job| job.active())
        || (!force
            && (state.games.values().any(|game| game.reserved())
                || data.launches.values().any(|launch| {
                    launch.expires > now() && launch.seen.saturating_add(90) > now()
                })
                || !p.peer_rooms().diagnostics(now())["sessions"]
                    .as_array()
                    .is_some_and(Vec::is_empty)));
    Ok(!busy)
}
fn strict_host(p: &Platform, purpose: &str, force: bool, kind: HostKind) -> Response {
    let result = p
        .controller
        .maintenance
        .begin(purpose, || strict_idle(p, force));
    match result {
        Ok(Some(lease)) => {
            if let Some(step) = kind.tracked() {
                let _ = p.controller.deployment.clear_host_wait(step);
            }
            (
                [("cache-control", "no-store")],
                Json(json!({"lease":lease,"policy":"strict"})),
            )
                .into_response()
        }
        Ok(None) => {
            if let Some(step) = kind.tracked() {
                let _ = p
                    .controller
                    .deployment
                    .note_host_wait(step, crate::deployment::HostPolicy::Strict);
            }
            let waiting = crate::update_admission::describe_strict(p)
                .ok()
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| item.holds)
                        .map(|item| item.summary.clone())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|waiting| !waiting.is_empty())
                .unwrap_or_else(|| "Play or publishing is still active.".into());
            error(StatusCode::CONFLICT, &waiting)
        }
        Err(error_value) => {
            tracing::warn!(error=%error_value, "maintenance check failed");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "maintenance check unavailable",
            )
        }
    }
}
fn release_host(p: &Platform, step: crate::deployment::HostStep) -> Response {
    let since = p.controller.deployment.read().host_appeared.or(p
        .controller
        .deployment
        .read()
        .available_since);
    let decision = match crate::update_admission::admission_since(p, since) {
        Ok(decision) => decision,
        Err(error_value) => {
            tracing::warn!(error=%error_value, "release host check failed");
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "maintenance check unavailable",
            );
        }
    };
    let permitted = p.controller.deployment.read().authorized()
        && p.controller
            .deployment
            .read()
            .grant
            .as_ref()
            .is_some_and(|grant| {
                p.db.read().ok().is_some_and(|data| {
                    data.operator(&grant.actor) && data.profile_enabled(&grant.actor)
                })
            });
    let purpose = match step {
        crate::deployment::HostStep::Reboot => "reboot",
        crate::deployment::HostStep::Ansible => "ansible",
    };
    if !decision.ready && !permitted {
        let _ = p
            .controller
            .deployment
            .note_host_wait(step, crate::deployment::HostPolicy::Release);
        let waiting = if decision.waiting.is_empty() {
            "This host step is waiting for the same reasons as the update.".into()
        } else {
            decision.waiting
        };
        return error(StatusCode::CONFLICT, &waiting);
    }
    match p
        .controller
        .maintenance
        .begin_update(purpose, decision.interrupt_publishing || permitted)
    {
        Ok(Some(lease)) => {
            if permitted {
                let _ = p.controller.deployment.edit(|record| {
                    record.grant = None;
                    Ok(())
                });
            }
            let _ = p.controller.deployment.clear_host_wait(step);
            (
                [("cache-control", "no-store")],
                Json(json!({"lease":lease,"policy":"release","forced":permitted})),
            )
                .into_response()
        }
        Ok(None) => {
            let _ = p
                .controller
                .deployment
                .note_host_wait(step, crate::deployment::HostPolicy::Release);
            error(
                StatusCode::CONFLICT,
                "Another job is using the server. This host step will try again.",
            )
        }
        Err(error_value) => {
            tracing::warn!(error=%error_value, "release host admission failed");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "maintenance check unavailable",
            )
        }
    }
}
#[derive(Deserialize)]
struct End {
    id: String,
    #[serde(default)]
    done: bool,
}
async fn end(State(app): State<App>, headers: HeaderMap, Json(body): Json<End>) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let lease = app.platform.controller.maintenance.take(&body.id);
    if body.done
        && let Some(step) = lease
            .as_ref()
            .and_then(|lease| match lease.purpose.as_str() {
                "reboot" => Some(crate::deployment::HostStep::Reboot),
                "ansible" => Some(crate::deployment::HostStep::Ansible),
                _ => None,
            })
    {
        let _ = app.platform.controller.deployment.complete_host_step(step);
    }
    Json(json!({"ok":true})).into_response()
}
async fn status(State(app): State<App>, headers: HeaderMap) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let state = app.platform.controller.snapshot();
    let database_ok = app.platform.db.read().is_ok();
    let activity = crate::update_admission::describe(&app.platform).unwrap_or_else(|_| Vec::new());
    let update_holding: Vec<_> = activity
        .iter()
        .filter(|item| item.holds)
        .map(|item| item.summary.clone())
        .collect();
    Json(json!({"version":1,"build":crate::BUILD_SHA,"maintenance":app.platform.controller.maintenance.active(),
        "database_ok":database_ok,"lifecycle_age_seconds":now().saturating_sub(app.platform.controller.last_tick.load(std::sync::atomic::Ordering::Relaxed)),
        "reserved_games":state.games.values().filter(|g|g.reserved()).count(),"packaging":state.build_reservation.is_some(),
        "update_holding":update_holding,"update_activity":activity})).into_response()
}
