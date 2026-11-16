//! Capture only authenticated game publication mutations, before JSON parsing.
use super::*;
use axum::{
    extract::{FromRequestParts, MatchedPath},
    middleware::Next,
};
use std::sync::atomic::{AtomicBool, Ordering};
static WARNED: AtomicBool = AtomicBool::new(false);
fn warn() {
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "publication capture skipped or incomplete; inspect capture budget, disk reserve and private archive permissions"
        );
    }
}
pub(super) async fn record(State(p): State<Platform>, request: Request, next: Next) -> Response {
    if p.config().controller.publish_capture_mb == 0
        || !matches!(
            request.method().as_str(),
            "POST" | "PUT" | "PATCH" | "DELETE"
        )
    {
        return next.run(request).await;
    }
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_default();
    // Never capture setup codes, token exchanges, member data or gameplay saves.
    if !route.starts_with("/api/publisher/apps/{app}")
        && !route.starts_with("/api/metadata/apps/{app}")
    {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let app = Path::<BTreeMap<String, String>>::from_request_parts(&mut parts, &p)
        .await
        .ok()
        .and_then(|Path(mut values)| values.remove("app"));
    let Some(app) = app else {
        return next.run(Request::from_parts(parts, body)).await;
    };
    let metadata = route.starts_with("/api/metadata/")
        || route.ends_with("/metadata")
        || route.ends_with("/card-art");
    let authorized = if metadata {
        super::metadata::authorize(&p, &parts.headers, &app)
    } else {
        p.publisher(&parts.headers, Some(&app), "publish")
            .map(|_| ())
    };
    if authorized.is_err() {
        return next.run(Request::from_parts(parts, body)).await;
    }
    let group = match p.db.read() {
        Ok(data) => data.groups.app(&app).to_owned(),
        Err(_) => return next.run(Request::from_parts(parts, body)).await,
    };
    let headers = [
        "content-type",
        "content-encoding",
        "x-infuser-art-prompt-revision",
    ]
    .into_iter()
    .filter_map(|name| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| (name.into(), value.into()))
    })
    .collect();
    let event = crate::publish_capture::Request {
        method: parts.method.to_string(),
        path: parts.uri.path().into(),
        route,
        app,
        group,
        headers,
    };
    let settings = p.config().controller.clone();
    let (id, body) = if let Some(staged) = parts.extensions.get::<publication::Staged>().cloned() {
        // Keep admission and per-app coordination alive if the request disconnects.
        let mutation = parts
            .extensions
            .get::<Arc<publication::Mutation>>()
            .cloned();
        let id = tokio::task::spawn_blocking(move || {
            let _mutation = mutation;
            crate::publish_capture::begin_file(
                &settings,
                &event,
                staged.file.path(),
                staged.bytes,
                &staged.sha256,
            )
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
        (id, body)
    } else {
        let limit = if event.route.ends_with("/card-art") {
            crate::card_art::MAX_BYTES
        } else {
            96 * 1024
        };
        let bytes = match axum::body::to_bytes(body, limit).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "request body too large or interrupted",
                )
                    .into_response();
            }
        };
        let copy = bytes.clone();
        let mutation = parts
            .extensions
            .get::<Arc<publication::Mutation>>()
            .cloned();
        let id = tokio::task::spawn_blocking(move || {
            let _mutation = mutation;
            crate::publish_capture::begin(&settings, &event, &copy)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
        (id, Body::from(bytes))
    };
    if id.is_none() {
        warn();
    }
    let mut response = next.run(Request::from_parts(parts, body)).await;
    let mut recorded = false;
    if let Some(id) = id {
        let settings = p.config().controller.clone();
        let status = response.status().as_u16();
        recorded = matches!(
            tokio::task::spawn_blocking(move || crate::publish_capture::finish(
                &settings, id, status
            ))
            .await,
            Ok(Ok(()))
        );
        if !recorded {
            warn();
        }
        response
            .headers_mut()
            .insert("x-infuser-capture-id", id.to_string().parse().unwrap());
    }
    response.headers_mut().insert(
        "x-infuser-capture",
        if recorded { "recorded" } else { "incomplete" }
            .parse()
            .unwrap(),
    );
    response
}
