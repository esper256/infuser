//! Tiny HTTP game server used by the opt-in Docker journey.
use anyhow::{Result, ensure};
use axum::{Json, Router, extract::State, routing::get};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[tokio::main]
async fn main() -> Result<()> {
    let port: u16 = std::env::var("INFUSER_PORT")
        .unwrap_or_else(|_| "7000".into())
        .parse()?;
    if std::env::args().any(|s| s == "--healthcheck") {
        let mut socket = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(2),
        )?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        socket
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut response = String::new();
        socket.read_to_string(&mut response)?;
        ensure!(response.starts_with("HTTP/1.1 200"), "not ready");
        return Ok(());
    }
    let count = Arc::new(AtomicU64::new(0));
    let app = Router::new()
        .route("/healthz", get(|| async { "ready" }))
        .route(
            "/",
            get(|State(count): State<Arc<AtomicU64>>| async move {
                Json(json!({"visits": count.fetch_add(1, Ordering::Relaxed) + 1}))
            }),
        )
        .with_state(count);
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .unwrap()
                .recv()
                .await;
        })
        .await?;
    Ok(())
}
