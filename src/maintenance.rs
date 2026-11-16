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
}
#[derive(Default)]
pub struct Gate(Mutex<StateData>);
pub struct Work(Arc<Gate>);
impl Drop for Work {
    fn drop(&mut self) {
        self.0.0.lock().unwrap().working -= 1;
    }
}
impl Gate {
    pub fn enter(self: &Arc<Self>) -> Result<Work> {
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
        state.working += 1;
        Ok(Work(self.clone()))
    }
    pub fn begin(
        &self,
        purpose: &str,
        idle: impl FnOnce() -> Result<bool>,
    ) -> Result<Option<Lease>> {
        ensure!(
            matches!(purpose, "backup" | "update"),
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
        let mut state = self.0.lock().unwrap();
        if state.lease.as_ref().is_some_and(|l| l.id == id) {
            state.lease = None;
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
        "/healthz" | "/v1/maintenance" | "/v1/operations"
    ) {
        return next.run(request).await;
    }
    let work = match gate.enter() {
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

pub struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl Client {
    pub fn new(config: &crate::config::Config) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            url: format!(
                "http://127.0.0.1:{}/v1/maintenance",
                config.controller.api_port
            ),
            token: std::fs::read_to_string(&config.controller.token_file)?
                .trim()
                .into(),
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
