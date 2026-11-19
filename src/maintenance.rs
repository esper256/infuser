//! Short, expiring admission leases shared by backups and deployment.
use anyhow::{Result, ensure};
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize)]
pub struct Lease {
    pub id: String,
    pub purpose: String,
    pub expires: u64,
}
#[derive(Default)]
struct StateData {
    lease: Option<Lease>,
    working: usize,
    publishing: usize,
    next_id: u64,
    inflight: Vec<Inflight>,
}
struct Inflight {
    id: u64,
    path: String,
    started: u64,
    publishing: bool,
}
#[derive(Clone)]
pub struct LiveRequest {
    pub path: String,
    pub started: u64,
    pub publishing: bool,
}
#[derive(Default)]
pub struct Gate(Mutex<StateData>);
pub struct Work {
    gate: Arc<Gate>,
    id: u64,
    publishing: bool,
}
impl Drop for Work {
    fn drop(&mut self) {
        let mut state = self.gate.0.lock().unwrap();
        state.inflight.retain(|request| request.id != self.id);
        state.working -= 1;
        if self.publishing {
            state.publishing -= 1;
        }
    }
}
impl Gate {
    pub fn working(&self) -> usize {
        self.0.lock().unwrap().working
    }
    pub fn publishing(&self) -> usize {
        self.0.lock().unwrap().publishing
    }
    pub fn requests(&self) -> Vec<LiveRequest> {
        self.0
            .lock()
            .unwrap()
            .inflight
            .iter()
            .map(|request| LiveRequest {
                path: request.path.clone(),
                started: request.started,
                publishing: request.publishing,
            })
            .collect()
    }
    /// Explicit operator authorization may interrupt players and publishing,
    /// but never steals an existing backup/update lease.
    pub fn override_update(&self) -> Option<Lease> {
        let mut state = self.0.lock().unwrap();
        let now = crate::controller::now();
        if state.lease.as_ref().is_some_and(|l| l.expires > now) {
            return None;
        }
        let lease = Lease {
            id: crate::store::secret(),
            purpose: "update".into(),
            expires: now + 60,
        };
        state.lease = Some(lease.clone());
        Some(lease)
    }
    pub fn enter(self: &Arc<Self>) -> Result<Work> {
        self.enter_kind(false, "")
    }
    pub(crate) fn enter_kind(self: &Arc<Self>, publishing: bool, path: &str) -> Result<Work> {
        let mut state = self.0.lock().unwrap();
        ensure!(
            state
                .lease
                .as_ref()
                .is_none_or(|l| l.expires <= crate::controller::now()),
            crate::portal::fault(
                503,
                "Infuser is briefly undergoing maintenance. Retry shortly."
            )
        );
        state.next_id += 1;
        let id = state.next_id;
        let mut stored = path.to_owned();
        if stored.len() > 180 {
            stored.truncate(180);
            while !stored.is_char_boundary(stored.len()) {
                stored.pop();
            }
        }
        state.inflight.push(Inflight {
            id,
            path: stored,
            started: crate::controller::now(),
            publishing,
        });
        state.working += 1;
        if publishing {
            state.publishing += 1;
        }
        Ok(Work {
            gate: self.clone(),
            id,
            publishing,
        })
    }
    /// Binary activation. Unlike [`Self::begin`], an ordinary open request does
    /// not block: heartbeats and page loads are not the publish being received.
    pub fn begin_update(&self, purpose: &str, interrupt_publishing: bool) -> Result<Option<Lease>> {
        ensure!(
            matches!(purpose, "update" | "reboot" | "ansible"),
            "invalid maintenance purpose"
        );
        let mut state = self.0.lock().unwrap();
        let now = crate::controller::now();
        if state.lease.as_ref().is_some_and(|l| l.expires > now)
            || (!interrupt_publishing && state.publishing != 0)
        {
            return Ok(None);
        }
        let lease = Lease {
            id: crate::store::secret(),
            purpose: purpose.into(),
            expires: now + 60,
        };
        state.lease = Some(lease.clone());
        Ok(Some(lease))
    }
    pub fn begin(
        &self,
        purpose: &str,
        idle: impl FnOnce() -> Result<bool>,
    ) -> Result<Option<Lease>> {
        ensure!(
            matches!(purpose, "backup" | "update" | "reboot" | "ansible"),
            "invalid maintenance purpose"
        );
        let mut state = self.0.lock().unwrap();
        let now = crate::controller::now();
        if state.lease.as_ref().is_some_and(|l| l.expires > now) || state.working != 0 || !idle()? {
            return Ok(None);
        }
        let lease = Lease {
            id: crate::store::secret(),
            purpose: purpose.into(),
            expires: now + 60,
        };
        state.lease = Some(lease.clone());
        Ok(Some(lease))
    }
    pub fn end(&self, id: &str) {
        self.take(id);
    }
    pub fn take(&self, id: &str) -> Option<Lease> {
        let mut state = self.0.lock().unwrap();
        if state.lease.as_ref().is_some_and(|lease| lease.id == id) {
            state.lease.take()
        } else {
            None
        }
    }
    pub fn active(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .lease
            .as_ref()
            .is_some_and(|l| l.expires > crate::controller::now())
    }
}
pub async fn admission(
    State(gate): State<Arc<Gate>>,
    mut request: Request,
    next: Next,
) -> Response {
    if matches!(
        request.uri().path(),
        "/healthz"
            | "/v1/maintenance"
            | "/v1/operations"
            | "/v1/deployment"
            | "/v1/deployment/admit"
    ) {
        return next.run(request).await;
    }
    let path = request.uri().path().to_owned();
    let publishing =
        path.starts_with("/api/publisher/") && request.method() != axum::http::Method::GET;
    let work = match gate.enter_kind(publishing, &path) {
        Ok(work) => Arc::new(work),
        Err(_) => return (axum::http::StatusCode::SERVICE_UNAVAILABLE,
            [("retry-after", "5"), ("cache-control", "no-store")],
            axum::Json(serde_json::json!({"error":"Infuser is briefly undergoing maintenance. Retry shortly."}))).into_response(),
    };
    // Blocking handlers may retain this extension after HTTP cancellation.
    request.extensions_mut().insert(work.clone());
    let response = next.run(request).await;
    // Streams count until their body completes, not just until headers are sent.
    let (parts, body) = response.into_parts();
    let stream = futures_util::stream::unfold(
        (body.into_data_stream(), work),
        |(mut stream, work)| async move {
            use futures_util::StreamExt;
            stream.next().await.map(|chunk| (chunk, (stream, work)))
        },
    );
    Response::from_parts(parts, Body::from_stream(stream))
}

pub enum UpdateGate {
    Open,
    Wait(String),
}
pub struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl Client {
    pub async fn report_update(
        &self,
        report: &crate::deployment::Report,
        host_steps: Option<&[crate::deployment::HostStep]>,
    ) -> Result<()> {
        let send = |report: &crate::deployment::Report,
                    host_steps: Option<&[crate::deployment::HostStep]>| {
            let mut body = serde_json::to_value(report)?;
            if let Some(steps) = host_steps {
                body["host_steps"] = serde_json::to_value(steps)?;
            }
            Ok::<_, anyhow::Error>(
                self.http
                    .post(self.url.replace("/v1/maintenance", "/v1/deployment"))
                    .bearer_auth(&self.token)
                    .json(&body),
            )
        };
        let response = send(report, host_steps)?.send().await?;
        if response.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY
            && (report.built_at.is_some() || host_steps.is_some())
        {
            // A rollback/older controller may not understand optional metadata.
            let mut legacy = report.clone();
            legacy.built_at = None;
            send(&legacy, None)?.send().await?.error_for_status()?;
        } else {
            response.error_for_status()?;
        }
        Ok(())
    }
    pub async fn admit_update(&self, release: &str, force: bool) -> Result<UpdateGate> {
        let response = self
            .http
            .post(self.url.replace("/v1/maintenance", "/v1/deployment/admit"))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({"release":release,"force":force}))
            .send()
            .await?;
        // Allow adoption by the older daemon; it has no web override to consume.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(if self.acquire("update", force).await?.is_some() {
                UpdateGate::Open
            } else {
                UpdateGate::Wait("waiting before activation".into())
            });
        }
        if response.status() == reqwest::StatusCode::CONFLICT {
            let body = response.text().await.unwrap_or_default();
            let reason = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned))
                .filter(|reason| !reason.is_empty())
                .unwrap_or_else(|| "waiting before activation".into());
            return Ok(UpdateGate::Wait(reason));
        }
        response.error_for_status()?;
        Ok(UpdateGate::Open)
    }
    pub fn new(config: &crate::config::Config) -> Result<Self> {
        Self::connect(config.controller.api_port, &config.controller.token_file)
    }
    /// Stable helpers only need the control connection. New daemon features must
    /// not prevent an older helper from asking the running daemon for admission.
    /// The candidate binary still validates the complete configuration itself.
    pub(crate) fn from_config_file(path: &std::path::Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Config {
            controller: Connection,
        }
        #[derive(serde::Deserialize)]
        #[serde(default)]
        struct Connection {
            api_port: u16,
            token_file: std::path::PathBuf,
        }
        impl Default for Connection {
            fn default() -> Self {
                let defaults = crate::config::Settings::default();
                Self {
                    api_port: defaults.api_port,
                    token_file: defaults.token_file,
                }
            }
        }
        let config: Config = toml::from_str(&std::fs::read_to_string(path)?)?;
        ensure!(
            config.controller.api_port >= 1024,
            "invalid control API port"
        );
        ensure!(
            config.controller.token_file.is_absolute(),
            "control token path must be absolute"
        );
        Self::connect(config.controller.api_port, &config.controller.token_file)
    }
    fn connect(api_port: u16, token_file: &std::path::Path) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            url: format!("http://127.0.0.1:{api_port}/v1/maintenance"),
            token: std::fs::read_to_string(token_file)?.trim().into(),
        })
    }
    pub async fn acquire(&self, purpose: &str, force: bool) -> Result<Option<String>> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({"purpose":purpose,"force":force}))
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Ok(None);
        }
        let value: serde_json::Value = response.error_for_status()?.json().await?;
        Ok(Some(
            value["lease"]["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("maintenance lease missing"))?
                .into(),
        ))
    }
    pub async fn release(&self, id: &str) -> Result<()> {
        self.http
            .delete(&self.url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({"id":id}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abandoned_lease_expires_without_releasing_a_new_owners_lease() -> Result<()> {
        let gate = Arc::new(Gate::default());
        let old = gate.begin("backup", || Ok(true))?.unwrap();
        assert!(gate.enter().is_err());
        gate.0.lock().unwrap().lease.as_mut().unwrap().expires = 0;
        drop(gate.enter()?);
        let new = gate.begin("update", || Ok(true))?.unwrap();
        gate.end(&old.id);
        assert!(gate.active());
        gate.end(&new.id);
        assert!(!gate.active());
        Ok(())
    }
}
