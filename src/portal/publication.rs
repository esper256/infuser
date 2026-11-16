//! Bounded publisher work, with artifact IO outside the gameplay coordination lock.
use super::*;
use axum::{
    extract::{FromRequestParts, MatchedPath},
    middleware::Next,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{OwnedMutexGuard, OwnedSemaphorePermit};

pub(super) struct Mutation {
    _maintenance: Option<Arc<crate::maintenance::Work>>,
    _slot: OwnedSemaphorePermit,
    _app: OwnedMutexGuard<()>,
}
#[derive(Clone)]
pub(super) struct Staged {
    pub file: Arc<tempfile::NamedTempFile>,
    pub bytes: u64,
    pub sha256: String,
}
impl Platform {
    pub(super) fn app_lock(&self, app: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.app_mutations.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(app).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(app.to_owned(), Arc::downgrade(&lock));
        lock
    }
}
// This middleware wraps capture and body extraction, sharing one installation-wide
// admission semaphore across every publisher route. Busy requests never buffer bodies.
pub(super) async fn admit(State(p): State<Platform>, request: Request, next: Next) -> Response {
    if !matches!(
        request.method().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    ) {
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
    let route = parts
        .extensions
        .get::<MatchedPath>()
        .map(|p| p.as_str())
        .unwrap_or("")
        .to_owned();
    let authorization = if route.starts_with("/api/metadata/")
        || route.ends_with("/metadata")
        || route.ends_with("/card-art")
    {
        metadata::authorize(&p, &parts.headers, &app)
    } else {
        p.publisher(&parts.headers, Some(&app), "publish")
            .map(|_| ())
    };
    if let Err(error) = authorization {
        return Error(error).into_response();
    }
    let Ok(slot) = p.publication_slots.clone().try_acquire_owned() else {
        let mut response = Error(fault(
            503,
            "Publishing is busy; retry this request shortly.",
        ))
        .into_response();
        response
            .headers_mut()
            .insert("retry-after", "1".parse().unwrap());
        return response;
    };
    let lock = p.app_lock(&app);
    let guard =
        match tokio::time::timeout(std::time::Duration::from_secs(5), lock.lock_owned()).await {
            Ok(lock) => Arc::new(Mutation {
                _maintenance: parts
                    .extensions
                    .get::<Arc<crate::maintenance::Work>>()
                    .cloned(),
                _slot: slot,
                _app: lock,
            }),
            Err(_) => {
                let mut response = Error(fault(
                    503,
                    "Another update for this game is still running; retry shortly.",
                ))
                .into_response();
                response
                    .headers_mut()
                    .insert("retry-after", "1".parse().unwrap());
                return response;
            }
        };
    parts.extensions.insert(guard.clone());
    if route.ends_with("/files/{*path}") {
        let limit = if route.contains("/server/files/") {
            crate::server_package::SOURCE_FILE_BYTES as u64
        } else {
            64 * 1024 * 1024
        };
        match stage(&p, body, limit).await {
            Ok(staged) => {
                parts.extensions.insert(staged);
                next.run(Request::from_parts(parts, Body::empty())).await
            }
            Err(error) => Error(error).into_response(),
        }
    } else {
        next.run(Request::from_parts(parts, body)).await
    }
}
async fn stage(p: &Platform, body: Body, limit: u64) -> Result<Staged> {
    let root = p.config().controller.state_dir.join("upload-staging");
    tokio::fs::create_dir_all(&root).await?;
    let disk = rustix::fs::statvfs(&root)?;
    // Two admitted uploads may each consume 64 MiB. Reserve both before writing.
    anyhow::ensure!(
        disk.f_bavail.saturating_mul(disk.f_frsize)
            >= p.config().controller.disk_reserve_mb * 1024 * 1024 + 128 * 1024 * 1024,
        fault(507, "host disk reserve would be exhausted")
    );
    let file = Arc::new(tempfile::NamedTempFile::new_in(root)?);
    let mut output = tokio::fs::File::from_std(file.reopen()?);
    let mut input = body.into_data_stream();
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    while let Some(chunk) = tokio::time::timeout_at(deadline, input.next())
        .await
        .map_err(|_| fault(408, "Upload timed out; retry the file."))?
    {
        let chunk = chunk.map_err(|_| fault(400, "Upload was interrupted; retry the file."))?;
        bytes = bytes.saturating_add(chunk.len() as u64);
        anyhow::ensure!(bytes <= limit, fault(413, "file exceeds the upload limit"));
        digest.update(&chunk);
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    output.sync_all().await?;
    Ok(Staged {
        file,
        bytes,
        sha256: digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}
