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
pub fn update_activity(p: &Platform) -> anyhow::Result<Vec<&'static str>> {
    let state = p.controller.snapshot();
    let data = p.db.read()?;
    let mut reasons = Vec::new();
    if state.build_reservation.is_some() || data.server_builds.values().any(|j| j.active()) {
        reasons.push("Waiting for game publishing or server builds to finish");
    }
    if state.games.values().any(|g| g.reserved())
        || data
            .launches
            .values()
            .any(|l| l.expires > now() && l.seen.saturating_add(90) > now())
        || !p.peer_rooms().diagnostics(now())["sessions"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        reasons.push("Waiting for games to finish");
    }
    Ok(reasons)
}
async fn deployment_report(
    State(app): State<App>,
    headers: HeaderMap,
    Json(report): Json<crate::deployment::Report>,
) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    match app.platform.controller.deployment.report(report) {
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
        } else {
            p.controller
                .maintenance
                .begin("update", || Ok(update_activity(p)?.is_empty()))?
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
        Ok((None, _)) => error(
            StatusCode::CONFLICT,
            "waiting for games, publishing or maintenance",
        ),
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
    if !matches!(body.purpose.as_str(), "backup" | "update")
        || body.force && body.purpose != "update"
    {
        return error(
            StatusCode::BAD_REQUEST,
            "only updates allow a forced restart",
        );
    }
    let p = &app.platform;
    let result = p.controller.maintenance.begin(&body.purpose, || {
        let state = p.controller.snapshot();
        let data = p.db.read()?;
        let busy = state.build_reservation.is_some()
            || data.server_builds.values().any(|j| j.active())
            || (!body.force
                && (state.games.values().any(|g| g.reserved())
                    || data
                        .launches
                        .values()
                        .any(|l| l.expires > now() && l.seen.saturating_add(90) > now())
                    || !p.peer_rooms().diagnostics(now())["sessions"]
                        .as_array()
                        .is_some_and(Vec::is_empty)));
        Ok(!busy)
    });
    match result {
        Ok(Some(lease)) => (
            [("cache-control", "no-store")],
            Json(json!({"lease":lease})),
        )
            .into_response(),
        Ok(None) => error(
            StatusCode::CONFLICT,
            "players or background work are active; retry later",
        ),
        Err(error_value) => {
            tracing::warn!(error=%error_value, "maintenance check failed");
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
}
async fn end(State(app): State<App>, headers: HeaderMap, Json(body): Json<End>) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    app.platform.controller.maintenance.end(&body.id);
    Json(json!({"ok":true})).into_response()
}
async fn status(State(app): State<App>, headers: HeaderMap) -> Response {
    if !authorized(&app, &headers) {
        return error(StatusCode::UNAUTHORIZED, "control token required");
    }
    let state = app.platform.controller.snapshot();
    let database_ok = app.platform.db.read().is_ok();
    Json(json!({"version":1,"build":crate::BUILD_SHA,"maintenance":app.platform.controller.maintenance.active(),
        "database_ok":database_ok,"lifecycle_age_seconds":now().saturating_sub(app.platform.controller.last_tick.load(std::sync::atomic::Ordering::Relaxed)),
        "reserved_games":state.games.values().filter(|g|g.reserved()).count(),"packaging":state.build_reservation.is_some()})).into_response()
}
