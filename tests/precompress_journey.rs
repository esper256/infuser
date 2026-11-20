use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path, sync::Arc};

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap()
        })),
    ))
}

fn uleb(mut value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn wasm_module(padding: usize) -> Vec<u8> {
    let mut module = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    module.extend_from_slice(&[0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]);
    module.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
    let mut export = vec![0x01, 0x04];
    export.extend_from_slice(b"ping");
    export.extend_from_slice(&[0x00, 0x00]);
    module.push(0x07);
    module.extend(uleb(export.len() as u32));
    module.extend(export);
    let code = vec![0x01, 0x04, 0x00, 0x41, 0x07, 0x0b];
    module.push(0x0a);
    module.extend(uleb(code.len() as u32));
    module.extend(code);
    let mut payload = uleb(7);
    payload.extend_from_slice(b"fixture");
    payload.extend(std::iter::repeat_n(b'g', padding));
    module.push(0x00);
    module.extend(uleb(payload.len() as u32));
    module.extend(payload);
    module
}

fn decode_gzip(bytes: &[u8]) -> Vec<u8> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    out
}

fn decode_brotli(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut &bytes[..], &mut out).unwrap();
    out
}

fn node() -> String {
    std::env::var("INFUSER_TEST_NODE").unwrap_or_else(|_| {
        if Path::new("/exec-daemon/node").is_file() {
            "/exec-daemon/node".into()
        } else {
            "node".into()
        }
    })
}

#[test]
fn opening_the_server_compresses_an_already_published_build() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state");
    let wasm = wasm_module(48 * 1024);
    let build = state.join("content/pond/one");
    fs::create_dir_all(&build)?;
    fs::write(build.join("index.wasm"), &wasm)?;
    {
        let db = platform::Database::open(&state)?;
        db.edit(|data| {
            data.builds.insert(
                "pond/one".into(),
                serde_json::from_value(json!({
                    "manifest": {"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},
                    "server": null,
                    "spa": false,
                    "cross_origin_isolation": true,
                    "ready": true,
                    "created": 1,
                    "files": {"index.wasm": wasm.len()}
                }))?,
            );
            data.apps.insert(
                "pond".into(),
                serde_json::from_value(json!({"build":"one","updated":1,"listed":false}))?,
            );
            Ok(())
        })?;
    }
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        state.display().to_string()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let opened = portal::Platform::open(controller)?;
    assert!(
        !build.join("index.wasm.br").exists(),
        "opening the server must not compress before it can answer health checks"
    );
    opened.precompress_ready()?;
    let brotli = fs::read(build.join("index.wasm.br"))?;
    assert_eq!(decode_brotli(&brotli), wasm);
    let gzip = fs::read(build.join("index.wasm.gz"))?;
    assert_eq!(decode_gzip(&gzip), wasm);
    let brotli_modified = fs::metadata(build.join("index.wasm.br"))?.modified()?;
    fs::remove_file(state.join("precompress/pond/one.json"))?;
    opened.precompress_ready()?;
    assert_eq!(fs::read(build.join("index.wasm.br"))?, brotli);
    assert_eq!(
        fs::metadata(build.join("index.wasm.br"))?.modified()?,
        brotli_modified
    );
    assert!(
        !opened.db.catalog()?.builds["pond/one"]
            .files
            .contains_key("index.wasm.br")
    );
    drop(opened);
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        state.display().to_string()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller,
        }),
    )?);
    portal::Platform::open(controller)?;
    assert_eq!(fs::read(build.join("index.wasm.br"))?, brotli);
    Ok(())
}

#[tokio::test]
async fn publish_negotiates_precompressed_files_and_keeps_other_games() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let auth = Router::new().route(
        "/oauth2/auth",
        get(|headers: HeaderMap| async move {
            let email = if headers.get("cookie").and_then(|value| value.to_str().ok())
                == Some("member=admin")
            {
                "admin@example.com"
            } else {
                ""
            };
            (
                if email.is_empty() {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::ACCEPTED
                },
                [
                    ("X-Auth-Request-Email", email),
                    ("X-Auth-Request-User", email),
                    ("Set-Cookie", ""),
                ],
            )
        }),
    );
    let (auth_url, _auth) = serve(auth).await?;
    let state = dir.path().join("state");
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64",
        state.display().to_string(),
        format!("{auth_url}/oauth2/auth")
    ))?;
    platform::member(&cfg, "admin@example.com", "Admin", true, true)?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let platform = portal::Platform::open(controller)?;
    let backfill = platform.clone();
    let (url, _server) = serve(portal::router(platform)).await?;
    let http = reqwest::Client::new();
    let admin = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .header("cookie", "member=admin")
            .header("origin", "https://games.example.com")
    };
    let connection: Value = admin(reqwest::Method::POST, "/api/admin/credentials")
        .json(&json!({"app":"moon","label":"Publisher"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = connection["token"].as_str().unwrap();
    let publisher = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", "games.example.com")
            .bearer_auth(token)
    };
    let wasm = wasm_module(48 * 1024);
    let script = b"function start(){return \"three\";}\n".repeat(800);
    let pack = b"pck-resource-".repeat(2_000);
    let mut noise_state = 0x9E37_79B9_7F4A_7C15u64;
    let noise: Vec<u8> = (0..32 * 1024)
        .map(|_| {
            noise_state ^= noise_state << 13;
            noise_state ^= noise_state >> 7;
            noise_state ^= noise_state << 17;
            noise_state as u8
        })
        .collect();
    let html = "<!doctype html><script type=module src=\"/_infuser/sdk.js\"></script>";
    publisher(reqwest::Method::PUT, "/api/publisher/apps/moon/builds/b-one")
        .json(&json!({"manifest":{"schema_version":1,"app_id":"moon","title":"Moon","play_modes":["solo"],"solo_runtime":"client_only"},"server":null,"spa":false,"cross_origin_isolation":true}))
        .send()
        .await?
        .error_for_status()?;
    for (path, bytes) in [
        ("index.html", html.as_bytes()),
        ("index.wasm", wasm.as_slice()),
        ("bundle.js", script.as_slice()),
        ("game.pck", pack.as_slice()),
        ("noise.pck", noise.as_slice()),
        ("sprite.png", &noise),
        ("Build/kept.wasm", wasm.as_slice()),
        ("Build/kept.wasm.br", b"unity-kept"),
    ] {
        publisher(
            reqwest::Method::PUT,
            &format!("/api/publisher/apps/moon/builds/b-one/files/{path}"),
        )
        .body(bytes.to_vec())
        .send()
        .await?
        .error_for_status()?;
    }
    publisher(
        reqwest::Method::POST,
        "/api/publisher/apps/moon/builds/b-one/publish",
    )
    .json(&json!({"listed":true}))
    .send()
    .await?
    .error_for_status()?;
    let status: Value = publisher(
        reqwest::Method::GET,
        "/api/publisher/apps/moon/builds/b-one",
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;
    assert!(status["files"].get("index.wasm.br").is_none());
    assert!(status["files"].get("bundle.js.gz").is_none());
    let content = state.join("content/moon/b-one");
    assert_eq!(
        decode_brotli(&fs::read(content.join("index.wasm.br"))?),
        wasm
    );
    assert_eq!(decode_gzip(&fs::read(content.join("index.wasm.gz"))?), wasm);
    assert_eq!(
        decode_brotli(&fs::read(content.join("bundle.js.br"))?),
        script
    );
    assert_eq!(decode_gzip(&fs::read(content.join("game.pck.gz"))?), pack);
    assert!(!content.join("noise.pck.br").exists());
    assert!(!content.join("sprite.png.br").exists());
    assert!(!content.join("index.html.br").exists());
    assert_eq!(fs::read(content.join("Build/kept.wasm.br"))?, b"unity-kept");

    let launch: Value = admin(reqwest::Method::POST, "/api/apps/moon/launch")
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let link = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
    let host = link.host_str().unwrap().to_string();
    let ticket = link
        .fragment()
        .unwrap()
        .strip_prefix("infuser=")
        .unwrap()
        .to_string();
    http.post(format!("{url}/_infuser/session"))
        .header("host", &host)
        .header("origin", format!("https://{host}"))
        .json(&json!({"token": ticket}))
        .send()
        .await?
        .error_for_status()?;
    let game = |method: reqwest::Method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", &host)
            .bearer_auth(&ticket)
            .header("origin", format!("https://{host}"))
    };
    let plain = game(reqwest::Method::GET, "/index.wasm").send().await?;
    assert_eq!(plain.status(), 200);
    assert!(plain.headers().get("content-encoding").is_none());
    assert_eq!(plain.headers()["content-type"], "application/wasm");
    assert_eq!(plain.headers()["vary"], "accept-encoding");
    assert_eq!(plain.headers()["cache-control"], "private, no-cache");
    assert_eq!(plain.headers()["referrer-policy"], "no-referrer");
    assert_eq!(plain.headers()["x-content-type-options"], "nosniff");
    assert_eq!(plain.headers()["cross-origin-opener-policy"], "same-origin");
    assert_eq!(
        plain.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert_eq!(plain.bytes().await?, wasm);

    let identity = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "identity")
        .send()
        .await?;
    assert!(identity.headers().get("content-encoding").is_none());
    assert_eq!(identity.bytes().await?, wasm);

    let brotli = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "br")
        .send()
        .await?;
    assert_eq!(brotli.status(), 200);
    assert_eq!(brotli.headers()["content-encoding"], "br");
    assert_eq!(brotli.headers()["content-type"], "application/wasm");
    assert_eq!(brotli.headers()["vary"], "accept-encoding");
    assert_eq!(brotli.headers()["cache-control"], "private, no-cache");
    assert_eq!(
        brotli.headers()["cross-origin-opener-policy"],
        "same-origin"
    );
    assert_eq!(
        brotli.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    let brotli_len = brotli.headers()["content-length"]
        .to_str()?
        .parse::<usize>()?;
    let brotli_body = brotli.bytes().await?;
    assert_eq!(brotli_body.len(), brotli_len);
    assert!(brotli_len < wasm.len());
    assert_eq!(decode_brotli(&brotli_body), wasm);

    let gzip = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "gzip")
        .send()
        .await?;
    assert_eq!(gzip.headers()["content-encoding"], "gzip");
    assert_eq!(gzip.headers()["content-type"], "application/wasm");
    assert_eq!(decode_gzip(&gzip.bytes().await?), wasm);

    let preferred = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "gzip, deflate, br")
        .send()
        .await?;
    assert_eq!(preferred.headers()["content-encoding"], "br");
    let etag = preferred.headers()["etag"].to_str()?.to_string();
    let modified = preferred.headers()["last-modified"].to_str()?.to_string();
    assert_eq!(decode_brotli(&preferred.bytes().await?), wasm);

    let deflate = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "deflate")
        .send()
        .await?;
    assert!(deflate.headers().get("content-encoding").is_none());
    assert_eq!(deflate.bytes().await?, wasm);

    let head = game(reqwest::Method::HEAD, "/index.wasm")
        .header("accept-encoding", "br")
        .send()
        .await?;
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-encoding"], "br");
    assert_eq!(head.headers()["content-type"], "application/wasm");
    assert_eq!(head.headers()["content-length"], brotli_len.to_string());
    assert_eq!(head.headers()["vary"], "accept-encoding");
    assert_eq!(
        head.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert!(head.bytes().await?.is_empty());

    let not_modified = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "br")
        .header("if-none-match", &etag)
        .send()
        .await?;
    assert_eq!(not_modified.status(), 304);
    assert_eq!(not_modified.headers()["etag"], etag);
    assert_eq!(not_modified.headers()["vary"], "accept-encoding");
    assert_eq!(not_modified.headers()["cache-control"], "private, no-cache");
    assert_eq!(
        not_modified.headers()["cross-origin-opener-policy"],
        "same-origin"
    );
    assert_eq!(
        not_modified.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert!(not_modified.bytes().await?.is_empty());
    let other_representation = game(reqwest::Method::GET, "/index.wasm")
        .header("if-none-match", &etag)
        .send()
        .await?;
    assert_eq!(other_representation.status(), 200);
    assert_eq!(other_representation.bytes().await?, wasm);
    let fresh = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "br")
        .header("if-modified-since", &modified)
        .send()
        .await?;
    assert_eq!(fresh.status(), 304);
    assert_eq!(fresh.headers()["vary"], "accept-encoding");
    let stale = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "br")
        .header("if-modified-since", "Tue, 01 Jan 2000 00:00:00 GMT")
        .send()
        .await?;
    assert_eq!(stale.status(), 200);
    assert_eq!(decode_brotli(&stale.bytes().await?), wasm);

    let range = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "br, gzip")
        .header("range", "bytes=0-3")
        .send()
        .await?;
    assert_eq!(range.status(), 206);
    assert!(range.headers().get("content-encoding").is_none());
    assert_eq!(range.headers()["content-type"], "application/wasm");
    assert_eq!(
        range.headers()["content-range"],
        format!("bytes 0-3/{}", wasm.len())
    );
    assert_eq!(range.headers()["vary"], "accept-encoding");
    assert_eq!(range.headers()["cache-control"], "private, no-cache");
    assert_eq!(
        range.headers()["cross-origin-embedder-policy"],
        "require-corp"
    );
    assert_eq!(range.bytes().await?, &wasm[..4]);

    let script_response = game(reqwest::Method::GET, "/bundle.js")
        .header("accept-encoding", "br")
        .send()
        .await?;
    assert_eq!(script_response.headers()["content-encoding"], "br");
    assert!(
        script_response.headers()["content-type"]
            .to_str()?
            .contains("javascript")
    );
    assert_eq!(decode_brotli(&script_response.bytes().await?), script);
    let picture = game(reqwest::Method::GET, "/sprite.png")
        .header("accept-encoding", "br, gzip")
        .send()
        .await?;
    assert!(picture.headers().get("content-encoding").is_none());
    assert_eq!(picture.bytes().await?, noise);
    let kept = game(reqwest::Method::GET, "/Build/kept.wasm.br")
        .send()
        .await?;
    assert_eq!(kept.headers()["content-encoding"], "br");
    assert_eq!(kept.headers()["content-type"], "application/wasm");
    assert_eq!(kept.bytes().await?.as_ref(), b"unity-kept");
    assert_eq!(
        game(reqwest::Method::GET, "/index.wasm.br")
            .send()
            .await?
            .status(),
        404
    );

    let output = tokio::process::Command::new(node())
        .arg("tests/godot_encoding_loader.mjs")
        .arg(&url)
        .arg(&host)
        .arg(&ticket)
        .arg("/index.wasm")
        .arg(wasm.len().to_string())
        .output()
        .await?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let loader: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(loader["encoding"], "br");
    assert_eq!(loader["loaded"], wasm.len());
    assert!(loader["compressed"].as_u64().unwrap() < wasm.len() as u64);

    fs::remove_file(content.join("index.wasm.br"))?;
    fs::remove_file(content.join("index.wasm.gz"))?;
    fs::remove_file(state.join("precompress/moon/b-one.json"))?;
    backfill.precompress_ready()?;
    let restored = game(reqwest::Method::GET, "/index.wasm")
        .header("accept-encoding", "gzip")
        .send()
        .await?;
    assert_eq!(restored.headers()["content-encoding"], "gzip");
    assert_eq!(decode_gzip(&restored.bytes().await?), wasm);
    backfill.precompress_ready()?;
    assert_eq!(fs::read(content.join("index.wasm.br"))?, brotli_body);
    Ok(())
}

#[tokio::test]
async fn health_answers_while_existing_builds_are_still_compressing() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let state = dir.path().join("state");
    let wasm = wasm_module(16 * 1024);
    let build = state.join("content/pond/one");
    fs::create_dir_all(&build)?;
    fs::write(build.join("index.wasm"), &wasm)?;
    {
        let db = platform::Database::open(&state)?;
        db.edit(|data| {
            data.builds.insert(
                "pond/one".into(),
                serde_json::from_value(json!({
                    "manifest": {"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},
                    "server": null,
                    "spa": false,
                    "cross_origin_isolation": false,
                    "ready": true,
                    "created": 1,
                    "files": {"index.wasm": wasm.len()}
                }))?,
            );
            data.apps.insert(
                "pond".into(),
                serde_json::from_value(json!({"build":"one","updated":1,"listed":false}))?,
            );
            Ok(())
        })?;
    }
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\ndisk_reserve_mb=64",
        state.display().to_string()
    ))?;
    let controller = Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?);
    let platform = portal::Platform::open(controller)?;
    assert!(!build.join("index.wasm.br").exists());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/healthz", listener.local_addr()?);
    let app = infuser::api::router(
        platform.controller.clone(),
        "health-check-token-health-check-token",
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let worker = platform.clone();
    let thread = std::thread::Builder::new()
        .name("precompress-test".into())
        .spawn(move || {
            worker
                .precompress_ready_with(|_| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                })
                .unwrap();
        })?;
    // The server is already running. This returns only once the backfill
    // thread is inside a file and waiting to be released.
    tokio::task::spawn_blocking(move || entered_rx.recv())
        .await?
        .unwrap();
    let response = reqwest::Client::new().get(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await?;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["build"], infuser::BUILD_SHA);
    assert!(
        !build.join("index.wasm.br").exists(),
        "health answered before the backfill wrote a sibling"
    );
    release_tx.send(()).unwrap();
    thread.join().unwrap();
    assert_eq!(decode_brotli(&fs::read(build.join("index.wasm.br"))?), wasm);
    server.abort();
    Ok(())
}
