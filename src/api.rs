use crate::controller::{Controller, Fault, now};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;

#[derive(Clone)]
struct App {
    controller: Arc<Controller>,
    token_hash: [u8; 32],
}

pub fn router(controller: Arc<Controller>, token: &str) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(json!({"status": "ok", "build": crate::BUILD_SHA})) }),
        )
        .route("/v1/apps", get(list))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/apps/{id}/leases", post(acquire))
        .route(
            "/v1/apps/{id}/leases/{lease}",
            get(status).put(renew).delete(release),
        )
        .route("/v1/apps/{id}/instance", delete(stop))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(App {
            controller,
            token_hash: Sha256::digest(token.as_bytes()).into(),
        })
}

struct ApiError(anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, message) = if let Some(f) = self.0.downcast_ref::<Fault>() {
            (StatusCode::from_u16(f.status).unwrap(), f.message.clone())
        } else {
            tracing::error!(error = %self.0, "API operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal controller error".into(),
            )
        };
        (
            code,
            [("Cache-Control", "no-store")],
            Json(json!({"error": message})),
        )
            .into_response()
    }
}
fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| {
            ApiError(
                Fault {
                    status: 401,
                    message: "bearer token required".into(),
                }
                .into(),
            )
        })
}
fn admin(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    let hash: [u8; 32] = Sha256::digest(bearer(headers)?.as_bytes()).into();
    if !bool::from(hash.ct_eq(&app.token_hash)) {
        return Err(ApiError(
            Fault {
                status: 401,
                message: "invalid control token".into(),
            }
            .into(),
        ));
    }
    Ok(())
}
fn reply(value: Value) -> Response {
    let status = if value["state"] == "starting" {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    (status, [("Cache-Control", "no-store")], Json(value)).into_response()
}
async fn list(State(app): State<App>, headers: HeaderMap) -> Result<Response, ApiError> {
    admin(&app, &headers)?;
    Ok(reply(app.controller.list(now())))
}
async fn capabilities(State(app): State<App>, headers: HeaderMap) -> Result<Response, ApiError> {
    admin(&app, &headers)?;
    Ok(reply(json!({
        "manifest_version":1,
        "public_publishing_available":true,
        "play_modes":["solo","multiplayer"],
        "session_policy":"one_per_app",
        "multiplayer_modes":{
            "peer_hosted":{"available":false,"reason":"Infuser signaling and relay integration is not installed yet."},
            "relayed":{"available":false,"reason":"Infuser WebSocket relay is not installed yet."},
            "dedicated":{"available":true},
            "persistent_world":{"available":true}
        },
        "peer_connection_policy":{"direct_connect":"optional","relay_fallback":"required"},
        "deployment":"web_publishing_api_and_operator_staged_server_images",
        "architecture":std::env::consts::ARCH
    })))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    session_id: String,
}
async fn acquire(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Launch>,
) -> Result<Response, ApiError> {
    admin(&app, &headers)?;
    Ok(reply(app.controller.acquire(
        &id,
        &body.session_id,
        now(),
    )?))
}
async fn status(
    State(app): State<App>,
    Path((id, lease)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    Ok(reply(app.controller.lease(
        &id,
        &lease,
        bearer(&headers)?,
        "get",
        now(),
    )?))
}
async fn renew(
    State(app): State<App>,
    Path((id, lease)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    Ok(reply(app.controller.lease(
        &id,
        &lease,
        bearer(&headers)?,
        "renew",
        now(),
    )?))
}
async fn release(
    State(app): State<App>,
    Path((id, lease)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    Ok(reply(app.controller.lease(
        &id,
        &lease,
        bearer(&headers)?,
        "delete",
        now(),
    )?))
}
async fn stop(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    admin(&app, &headers)?;
    app.controller.stop(&id)?;
    Ok(reply(json!({"state": "stopping"})))
}
