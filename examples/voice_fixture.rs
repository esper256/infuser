//! Disposable local voice fixture. Never expose these synthetic logins publicly.
use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{
        self,
        visits::{Participant, Visit},
    },
    portal,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("infuser=info")
        .init();
    let mut args = std::env::args().skip(1);
    let livekit = PathBuf::from(args.next().context("LiveKit binary required")?);
    let ready = PathBuf::from(args.next().context("ready file required")?);
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let auth = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let auth_port = auth.local_addr()?.port();
    tokio::spawn(async move {
        axum::serve(
            auth,
            Router::new().route(
                "/auth",
                get(|h: HeaderMap| async move {
                    let who = h
                        .get("cookie")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .split(';')
                        .find_map(|s| s.trim().strip_prefix("member="))
                        .unwrap_or("")
                        .to_owned();
                    if ["river", "sky", "visitor", "other"].contains(&who.as_str()) {
                        (
                            StatusCode::ACCEPTED,
                            [
                                ("X-Auth-Request-Email", format!("{who}@example.com")),
                                ("X-Auth-Request-User", who),
                            ],
                        )
                    } else {
                        (
                            StatusCode::UNAUTHORIZED,
                            [
                                ("X-Auth-Request-Email", String::new()),
                                ("X-Auth-Request-User", String::new()),
                            ],
                        )
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let api_port = listener.local_addr()?.port();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0")?;
    let media_port = reserved.local_addr()?.port();
    let udp = std::net::UdpSocket::bind("127.0.0.1:0")?;
    let udp_port = udp.local_addr()?.port();
    drop(reserved);
    drop(udp);
    let key_file = root.join("voice.json");
    let secret = infuser::store::secret();
    infuser::private_file::replace(
        &key_file,
        &serde_json::to_vec(&json!({"api_key":"fixture","api_secret":secret}))?,
    )?;
    let config_file = root.join("livekit.yaml");
    infuser::private_file::replace(&config_file,format!("port: {media_port}\nbind_addresses: [127.0.0.1]\nrtc:\n  udp_port: {udp_port}\n  use_external_ip: false\n  enable_loopback_candidate: true\nkeys:\n  fixture: {secret}\nlogging:\n  level: warn\nroom:\n  empty_timeout: 10\n  departure_timeout: 5\n").as_bytes())?;
    let _media = tokio::process::Command::new(livekit)
        .arg("--config")
        .arg(config_file)
        .kill_on_drop(true)
        .spawn()?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url=\"http://127.0.0.1:{auth_port}/auth\"\nprivate_groups=true\nvisits=true\n[controller.voice]\nport={media_port}\nkey_file={:?}",
        root.display().to_string(),
        key_file.display().to_string()
    ))?;
    for who in ["river", "sky", "visitor", "other"] {
        platform::member(&cfg, &format!("{who}@example.com"), who, false, true)?;
    }
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    p.db.edit(|d| {
        d.groups.names.insert("away".into(), "Away".into());
        for who in ["visitor", "other"] {
            d.groups
                .members
                .insert(format!("{who}@example.com"), "away".into());
        }
        d.visits.insert(
            "visit".into(),
            Visit {
                name: "Visit".into(),
                host: "home".into(),
                expires: now() + 3600,
                ended: false,
                games: Default::default(),
                participants: [(
                    "visitor@example.com".into(),
                    Participant {
                        after_message: 0,
                        ended: false,
                    },
                )]
                .into(),
            },
        );
        Ok(())
    })?;
    let worker = p.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = worker.reconcile_voice().await {
                eprintln!("Voice fixture: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    let recovery = p.controller.clone();
    let visit = p.clone();
    let extra = Router::new()
        .route(
            "/fixture/end-visit",
            post(move || {
                let p = visit.clone();
                async move {
                    p.db.edit(|d| {
                        d.visits.get_mut("visit").unwrap().ended = true;
                        Ok(())
                    })
                    .unwrap();
                    "ok"
                }
            }),
        )
        .route(
            "/fixture/orphan-cleanup",
            post(move || {
                let controller = recovery.clone();
                async move {
                    let reopened = portal::Platform::open(controller).unwrap();
                    reopened.reconcile_voice().await.unwrap();
                    "ok"
                }
            }),
        );
    let cert = rcgen::generate_simple_self_signed(vec!["games.example.com".into()])?;
    let cert_path = root.join("cert.pem");
    let key_path = root.join("key.pem");
    std::fs::write(&cert_path, cert.cert.pem())?;
    infuser::private_file::replace(&key_path, cert.signing_key.serialize_pem().as_bytes())?;
    infuser::private_file::replace(
        &ready,
        &serde_json::to_vec(
            &json!({"api":format!("http://127.0.0.1:{api_port}"),"cert":cert_path,"key":key_path,"livekit_port":media_port}),
        )?,
    )?;
    axum::serve(listener, portal::router(p).merge(extra)).await?;
    Ok(())
}
