//! Version 1 adapter. Keep this route table and SDK independent of future majors.
//! Handlers share the legacy implementation while their contracts match. A future
//! breaking change must split its handler instead of changing both generations.
use super::*;
pub(super) fn routes() -> Router<Platform> {
    Router::new()
        // Internal scaffolding: publication does not yet admit the required capability.
        .route("/connections/v1", get(connections::options))
        .route(
            "/connections/v1/{attempt}",
            put(connections::prepare)
                .get(connections::status)
                .delete(connections::cancel)
                .layer(DefaultBodyLimit::max(1024)),
        )
        .route("/connections/v1/{attempt}/socket", get(connections::socket))
        .route("/context", get(context))
        .route("/heartbeat", post(heartbeat).delete(leave))
        .route("/saves/{slot}", get(save_get).put(save_put))
        .route(
            "/presence/v1/activity",
            put(presence::set_activity).layer(DefaultBodyLimit::max(256)),
        )
        .route("/presence/v1", get(presence::snapshot))
        .route("/launch-intent/v1/consume", post(consume_intent))
        .route("/runtime/v1", get(runtime::status).post(runtime::acquire))
        .route("/runtime/v1/{session}", delete(runtime::release))
        .route("/peers", post(peer_join))
        .route("/peers/{peer}", get(peer_poll).delete(peer_leave))
        .route("/peers/{peer}/signals", post(peer_signal))
        .route(
            "/peers/{peer}/joinability",
            put(presence::set_joinable).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/peers/{peer}/ice", post(peer_ice))
        .route(
            "/peers/{peer}/health",
            post(peer_health).layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/cloud/v1/groups/{group}",
            get(cloud_load).post(cloud_commit),
        )
        .route(
            "/cloud/v1/groups/{group}/revisions/{revision}",
            get(cloud_revision),
        )
        .route("/cloud/v1/groups/{group}/history", get(cloud_history))
}
fn javascript(source: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        source,
    )
}
pub(super) async fn core() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/infuser.js"))
}
pub(super) async fn session_fetch() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/session-fetch.js"))
}
pub(super) async fn peers() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/peers.js"))
}
pub(super) async fn peerjs() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/peerjs.js"))
}
pub(super) async fn cloud() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/cloud.js"))
}
pub(super) async fn sessions() -> impl IntoResponse {
    javascript(include_str!("../../sdk/v1/sessions.js"))
}

pub(super) async fn activity_input() -> impl IntoResponse {
    javascript(include_str!("../../web/activity-input.js"))
}
