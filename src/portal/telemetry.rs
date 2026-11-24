use super::*;
use crate::timeline::{self, ContextFields, Event, Filter};
use axum::extract::MatchedPath;

pub(super) fn routes() -> Router<Platform> {
    let asset = get(|h: HeaderMap| async move {
        conditional::static_asset(
            &h,
            "text/javascript",
            include_bytes!("../../web/telemetry.js"),
        )
    });
    Router::new()
        .route("/assets/telemetry.js", asset.clone())
        .route("/_infuser/telemetry.js", asset)
        .route("/api/telemetry", post(library))
        .route("/_infuser/telemetry", post(game))
        .route("/api/publisher/apps/{app}/diagnostic-events", get(author))
        .layer(DefaultBodyLimit::max(32 * 1024))
}
pub(super) fn launch_ref(id: &str) -> String {
    hash(&format!("diagnostics/launch/{id}"))[..20].into()
}
fn launch_fields(p: &Platform, id: &str, l: &Launch) -> Result<ContextFields> {
    let d = p.db.catalog()?;
    let profile = d.player_id(&l.email);
    let attempt = p
        .controller
        .runtime_attempt(l.runtime_id())
        .unwrap_or_default();
    Ok(ContextFields {
        profile,
        group: d.deployment_group(l.runtime_id()).into(),
        app: l.app.clone(),
        build: l.build.clone(),
        launch: launch_ref(id),
        visit: l.visit.clone().unwrap_or_default(),
        guest: l.guest,
        attempt,
        ..Default::default()
    })
}
pub(super) fn launch_event(p: &Platform, id: &str, l: &Launch, kind: &str) {
    if let Ok(c) = launch_fields(p, id, l) {
        p.controller
            .timeline
            .record(Event::server(kind, c, json!({}), true));
    }
}
pub(super) fn connection_event(a: &crate::connections::Attempt, kind: &str) -> Event {
    let i = &a.identity;
    let mut event = Event::server(
        kind,
        ContextFields {
            app: i.app_id.clone(),
            build: i.build_id.clone(),
            launch: launch_ref(&i.launch_id),
            attempt: i.instance_id.clone(),
            group: i.group_id.clone(),
            guest: i.guest,
            ..Default::default()
        },
        json!({"state":if kind=="connection.open"{"connected"}else{a.error.as_deref().unwrap_or("closed")}}),
        true,
    );
    if a.error.is_some() {
        event.level = "warn".into();
    }
    event
}
pub(super) fn voice_event(p: &Platform, l: &crate::voice::Lease, kind: &str) {
    if let Ok(d) = p.db.catalog()
        && d.members.contains_key(&l.owner)
    {
        p.controller.timeline.record(Event::server(
            kind,
            ContextFields {
                profile: d.player_id(&l.owner),
                group: d.groups.member(&l.owner).into(),
                visit: l.visit.clone().unwrap_or_default(),
                ..Default::default()
            },
            json!({"session":launch_ref(&l.id),"channel":l.channel}),
            false,
        ));
    }
}
#[derive(Deserialize)]
struct Batch {
    scope: String,
    events: Vec<ClientEvent>,
}
#[derive(Deserialize)]
struct ClientEvent {
    page: String,
    seq: u32,
    at: i64,
    src: String,
    kind: String,
    #[serde(default)]
    data: Value,
}
async fn library(State(p): State<Platform>, h: HeaderMap, Json(b): Json<Batch>) -> Answer {
    p.origin(&h, p.domain())?;
    let member = p.member(&h, false).await?;
    let d = p.db.catalog()?;
    let profile = d.player_id(&member);
    let (group, inspecting) = p.group_view(&h, &member)?;
    ensure!(
        b.scope == profile,
        fault(409, "Diagnostic profile changed.")
    );
    let visit = h
        .get(groups::VISIT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    ingest(
        &p,
        ContextFields {
            profile,
            group,
            visit,
            inspecting,
            ..Default::default()
        },
        b,
        false,
    )
    .await
}
async fn game(State(p): State<Platform>, h: HeaderMap, Json(b): Json<Batch>) -> Answer {
    p.origin(&h, host(&h))?;
    let (id, l) = p.game(&h)?;
    let c = launch_fields(&p, &id, &l)?;
    ensure!(
        c.launch == b.scope,
        fault(409, "Diagnostic launch changed.")
    );
    ingest(&p, c, b, true).await
}
fn id(s: &str) -> bool {
    (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn scrub(s: &str, limit: usize) -> String {
    // Drop complete URLs, identifiers, credential-like words and query/fragment values.
    // Never stringify objects supplied by the game or accept entire arbitrary JSON.
    let mut out = String::new();
    let mut secret_next = false;
    for word in s.split_whitespace().take(100) {
        let sensitive = [
            "bearer",
            "cookie",
            "authorization",
            "password",
            "secret",
            "token",
            "credential",
        ]
        .iter()
        .any(|x| word.to_ascii_lowercase().contains(x));
        let opaque = word
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .any(|s| s.len() >= 24);
        let hide = secret_next
            || sensitive
            || opaque
            || word.contains(['@', '?', '#', '='])
            || word.contains("://")
            || word.starts_with('/')
            || word.parse::<IpAddr>().is_ok()
            || word.contains("\\");
        secret_next = sensitive;
        if !out.is_empty() {
            out.push(' ');
        }
        if hide {
            out.push_str("[redacted]");
        } else {
            out.extend(word.chars().filter(|c| !c.is_control()).take(120));
        }
        if out.len() >= limit {
            break;
        }
    }
    out.chars().take(limit).collect()
}
fn kind_level(k: &str) -> Option<&'static str> {
    Some(match k {
        "js.error" | "js.rejection" | "console.error" | "webgl.lost" => "error",
        "api.fail" | "resource.error" | "net.offline" | "mp.ended" | "voice.fail"
        | "voice.reconnecting" | "page.discarded" | "recorder.dropped" => "warn",
        "page.load" | "page.show" | "page.hide" | "page.freeze" | "page.resume" | "page.exit"
        | "page.restore" | "net.online" | "game.frame_loaded" | "webgl.restored"
        | "mp.connected" | "mp.connecting" | "voice.connected" | "voice.disconnected"
        | "voice.reconnected" | "mem.sample" | "error.repeat" => "info",
        _ => return None,
    })
}
fn clean(data: &Value) -> Value {
    let mut out = json!({});
    for k in [
        "line",
        "col",
        "status",
        "count",
        "duration_ms",
        "heap_bytes",
        "uptime_ms",
        "uncertainty_ms",
    ] {
        if let Some(n) = data[k].as_u64()
            && n < (1 << 45)
        {
            out[k] = n.into();
        }
    }
    for k in [
        "message", "class", "stack", "file", "client", "version", "state", "route", "method",
        "request", "session", "tab",
    ] {
        if let Some(s) = data[k].as_str() {
            if k == "version" {
                if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                    out[k] = s.into();
                }
                continue;
            }
            if matches!(k, "request" | "session" | "tab") {
                if id(s) {
                    out[k] = s.into();
                }
                continue;
            }
            if k == "file" && (s.contains(['/', '\\', '?', '#', '@']) || s.len() > 100) {
                continue;
            }
            out[k] = scrub(s, if k == "stack" { 900 } else { 250 }).into();
        }
    }
    out
}
async fn ingest(p: &Platform, mut ctx: ContextFields, b: Batch, is_game: bool) -> Answer {
    ensure!(
        b.events.len() <= 32,
        fault(413, "Too many diagnostic events.")
    );
    // An offline batch may arrive after another runtime has started. Link through server events.
    ctx.attempt.clear();
    let received_at = timeline::ms();
    let mut events = Vec::new();
    for e in b.events {
        let Some(level) = kind_level(&e.kind) else {
            continue;
        };
        if !id(&e.page) || e.at < received_at - 48 * 3_600_000 || e.at > received_at + 60_000 {
            continue;
        }
        let src = if !is_game {
            "library"
        } else if e.src == "game" {
            "game"
        } else {
            "player"
        };
        let visible =
            is_game && (src == "game" || e.kind.starts_with("game.") || e.kind.starts_with("mp."));
        events.push(Event {
            at: e.at.min(received_at),
            received_at,
            context: ctx.clone(),
            page: e.page,
            seq: Some(e.seq),
            src: src.into(),
            level: level.into(),
            kind: e.kind,
            data: clean(&e.data),
            author_visible: visible,
        });
    }
    ensure!(
        p.controller.timeline.submit(events).await,
        fault(503, "Diagnostic writer is busy.")
    );
    Ok(reply(json!({"server_ms":received_at})))
}
async fn author(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Query(filter): Query<Filter>,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    ensure!(
        filter.limit.is_none_or(|n| (1..=500).contains(&n))
            && filter.cursor.as_ref().is_none_or(|s| s
                .split_once(':')
                .is_some_and(|(a, b)| a.parse::<u64>().is_ok() && b.parse::<u64>().is_ok())),
        fault(400, "Invalid diagnostics limit or cursor.")
    );
    let path = p.controller.timeline.path().to_owned();
    let output =
        tokio::task::spawn_blocking(move || timeline::read(&path, &filter, Some(&app), None, true))
            .await
            .map_err(anyhow::Error::from)??;
    Ok(reply(output))
}
/// Instrument framework-matched game API operations. Never record raw paths or traffic bodies.
pub(super) async fn observe(
    State(p): State<Platform>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default();
    let game =
        route.starts_with("/_infuser/") && !route.ends_with(".js") && !route.contains("telemetry");
    let library = route.starts_with("/api/")
        && !route.contains("telemetry")
        && !route.contains("diagnostic-events");
    if !game && !library {
        return next.run(request).await;
    }
    let launching = if route == "/api/apps/{app}/launch" {
        request
            .uri()
            .path()
            .strip_prefix("/api/apps/")
            .and_then(|path| path.strip_suffix("/launch"))
            .filter(|app| slug(app))
            .map(str::to_owned)
    } else {
        None
    };
    let started = std::time::Instant::now();
    let h = request.headers().clone();
    let method = request.method().clone();
    let mut response = next.run(request).await;
    let status = response.status();
    let slow = started.elapsed().as_secs() >= 2 && !route.ends_with("/control");
    let operation = [
        "/saves",
        "/cloud",
        "/admissions",
        "/join",
        "/host",
        "/runtime/v1",
        "/connections",
        "/launch-intent",
        "/sessions",
    ]
    .iter()
    .any(|part| route.contains(part));
    if status.is_client_error()
        || status.is_server_error()
        || slow
        || (game
            && ((operation && method != axum::http::Method::GET)
                || route == "/_infuser/peers"
                || route == "/_infuser/saves/{slot}"
                || (route == "/_infuser/cloud/v1/groups/{group}"
                    && method == axum::http::Method::GET)))
    {
        let mut context = if game {
            p.game(&h)
                .ok()
                .and_then(|(id, l)| launch_fields(&p, &id, &l).ok())
        } else {
            h.get("x-infuser-verified-profile")
                .and_then(|v| v.to_str().ok())
                .and_then(|key| {
                    p.db.catalog()
                        .ok()
                        .filter(|d| d.members.contains_key(key))
                        .map(|d| ContextFields {
                            profile: d.player_id(key),
                            ..Default::default()
                        })
                })
        };
        if let Some(app) = launching.as_deref()
            && let Some(key) = h
                .get("x-infuser-verified-profile")
                .and_then(|v| v.to_str().ok())
            && let Some(ctx) = context.as_mut()
            && let Ok(d) = p.db.catalog()
            && let Ok((group, inspecting)) = p.group_view(&h, key)
            && !inspecting
            && p.visible_app(&h, key, app).is_ok()
            && let Some(runtime) = d.deployment_for(app, &group)
            && let Some(active) = d.deployed(runtime)
        {
            ctx.app = app.to_owned();
            ctx.group = group;
            ctx.build = active.build.clone();
            ctx.attempt = p.controller.runtime_attempt(runtime).unwrap_or_default();
        }
        if let Some(context) = context {
            let request_id = secret()[..16].to_owned();
            response
                .headers_mut()
                .insert("x-infuser-request-id", request_id.parse().unwrap());
            let visible = (game || launching.is_some() && !context.app.is_empty())
                && !route.contains("voice")
                && !route.contains("screenshots");
            let mut e = Event::server(
                "api.result",
                context,
                json!({"route":route,"method":method.as_str(),"status":status.as_u16(),"duration_ms":started.elapsed().as_millis() as u64,"request":request_id}),
                visible,
            );
            if !status.is_success() {
                e.level = if status.is_server_error() {
                    "error"
                } else {
                    "warn"
                }
                .into();
            }
            p.controller.timeline.record(e);
        }
    }
    response
}
