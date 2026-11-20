//! A candidate release serves reads and its health check, and refuses user
//! writes, until the updater promotes it. Restoring the pre-update snapshot
//! then cannot discard a committed user write. Schema changes the candidate
//! applied at startup are undone with that same file, because the snapshot is
//! the database from before the candidate opened it.

use anyhow::{Result, ensure};
use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub const MESSAGE: &str = "Infuser is updating. Try again in a moment.";
pub const CODE: &str = "updating";
pub const HEALTH_FAILURE: &str = "candidate failed its version-aware health check";
pub const OPERATOR_RESTORE: &str = "operator restored the previous release";

const FILE_NAME: &str = "update-probation.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    version: u32,
    release: String,
}

pub struct Gate {
    accepting: AtomicBool,
}

impl Gate {
    pub fn load(state_dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            accepting: AtomicBool::new(!holds_this_build(state_dir)),
        })
    }

    pub fn accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    /// Accept user writes. The marker is removed first, so a crash after this
    /// returns cannot come back up still holding them, and a restore can see
    /// that the trial is no longer safe to roll back.
    pub fn promote(&self, state_dir: &Path, release: &str) -> Result<()> {
        ensure!(
            release == crate::BUILD_SHA,
            "promotion does not match this build"
        );
        disarm(state_dir)?;
        self.accepting.store(true, Ordering::Release);
        Ok(())
    }
}

pub fn marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE_NAME)
}

pub fn arm(state_dir: &Path, release: &str) -> Result<()> {
    ensure!(release_name(release), "invalid probation release");
    fs::create_dir_all(state_dir)?;
    let path = marker_path(state_dir);
    let bytes = serde_json::to_vec(&Marker {
        version: 1,
        release: release.to_owned(),
    })?;
    crate::private_file::replace(&path, &bytes)?;
    if let Err(error) = share_with_owner(state_dir, &path) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(())
}

pub fn disarm(state_dir: &Path) -> Result<()> {
    match fs::remove_file(marker_path(state_dir)) {
        Ok(()) => {
            if let Some(parent) = marker_path(state_dir).parent() {
                let _ = fs::File::open(parent).and_then(|file| file.sync_all());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn holds_this_build(state_dir: &Path) -> bool {
    holds_release(state_dir, crate::BUILD_SHA)
}

pub fn holds_release(state_dir: &Path, release: &str) -> bool {
    armed(state_dir).is_some_and(|armed| armed == release)
}

/// The release named by the marker, if a readable marker is present.
pub fn armed(state_dir: &Path) -> Option<String> {
    let bytes = fs::read(marker_path(state_dir)).ok()?;
    let marker: Marker = serde_json::from_slice(&bytes).ok()?;
    (marker.version == 1 && release_name(&marker.release)).then_some(marker.release)
}

fn release_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn share_with_owner(state_dir: &Path, path: &Path) -> Result<()> {
    let owner = state_dir.symlink_metadata()?;
    std::os::unix::fs::chown(path, Some(owner.uid()), Some(owner.gid()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o640))?;
    Ok(())
}

pub async fn guard(State(gate): State<Arc<Gate>>, request: Request, next: Next) -> Response {
    if gate.accepting() || safe(&request) {
        return next.run(request).await;
    }
    held(&request)
}

fn safe(request: &Request) -> bool {
    request.method() == Method::GET
        || request.method() == Method::HEAD
        || request.method() == Method::OPTIONS
        || request.uri().path() == "/v1/deployment/promote"
        || operator_recovery(request.method(), request.uri().path())
}

/// Exact operator recovery writes. A prefix would also let through group
/// creation, sign-in links, and visits, which stay held with everything else.
fn operator_recovery(method: &Method, path: &str) -> bool {
    method == Method::POST && matches!(path, "/api/operator/update" | "/api/operator/update/notice")
}

fn held(request: &Request) -> Response {
    if document(request) {
        let path = request.uri().path();
        let link = if path.starts_with('/') && !path.starts_with("//") && !path.contains('\\') {
            format!("<p><a href=\"{}\">Try again</a></p>", escape(path))
        } else {
            String::new()
        };
        let body = format!(
            "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Infuser is updating</title><h1>Infuser is updating</h1><p>{MESSAGE} Nothing you just submitted was saved.</p>{link}"
        );
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(body))
            .unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"error": MESSAGE, "code": CODE})),
    )
        .into_response()
}

fn document(request: &Request) -> bool {
    let path = request.uri().path();
    if path.starts_with("/api/") || path.starts_with("/v1/") || path.starts_with("/_infuser/") {
        return false;
    }
    accepts_html(request.headers()) || path.starts_with("/member-link/")
}

fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"))
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_holds_only_the_named_build() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let other = "a".repeat(40);
        assert!(!holds_release(dir.path(), crate::BUILD_SHA));
        arm(dir.path(), &other)?;
        assert!(holds_release(dir.path(), &other));
        assert!(Gate::load(dir.path()).accepting());
        arm(dir.path(), crate::BUILD_SHA)?;
        let gate = Gate::load(dir.path());
        assert!(!gate.accepting());
        gate.promote(dir.path(), crate::BUILD_SHA)?;
        assert!(gate.accepting());
        assert!(!holds_release(dir.path(), crate::BUILD_SHA));
        assert!(Gate::load(dir.path()).accepting());
        Ok(())
    }

    #[test]
    fn operator_recovery_is_an_exact_allowlist() {
        assert!(operator_recovery(&Method::POST, "/api/operator/update"));
        assert!(operator_recovery(
            &Method::POST,
            "/api/operator/update/notice"
        ));
        assert!(!operator_recovery(&Method::POST, "/api/operator/groups"));
        assert!(!operator_recovery(
            &Method::POST,
            "/api/operator/update/extra"
        ));
        assert!(!operator_recovery(&Method::DELETE, "/api/operator/update"));
        assert!(!operator_recovery(&Method::POST, "/api/chat"));
        assert!(!operator_recovery(
            &Method::POST,
            "/api/operator/members/ada/sign-in-link"
        ));
    }
}
