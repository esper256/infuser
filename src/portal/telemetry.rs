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
pub(crate) fn launch_ref(id: &str) -> String {
    hash(&format!("diagnostics/launch/{id}"))[..20].into()
}
fn launch_fields(p: &Platform, id: &str, l: &Launch) -> Result<ContextFields> {
    Ok(launch_context(p, id, l, p.db.catalog()?.as_ref()))
}
pub(super) fn launch_context(
    p: &Platform,
    id: &str,
    l: &Launch,
    d: &platform::Data,
) -> ContextFields {
    let profile = d
        .identities
        .profiles
        .get(&l.email)
        .map(|p| p.id.clone())
        .unwrap_or_default();
    let attempt = p
        .controller
        .runtime_attempt(l.runtime_id())
        .unwrap_or_default();
    ContextFields {
        profile,
        group: d.deployment_group(l.runtime_id()).into(),
        app: l.app.clone(),
        build: l.build.clone(),
        launch: launch_ref(id),
        visit: l.visit.clone().unwrap_or_default(),
        guest: l.guest,
        attempt,
        ..Default::default()
    }
}
pub(super) fn launch_event(p: &Platform, id: &str, l: &Launch, kind: &str, reason: Option<&str>) {
    if let Ok(c) = launch_fields(p, id, l) {
        p.controller.timeline.record(Event::server(
            kind,
            c,
            reason.map_or(json!({}), |r| json!({"reason":r})),
            true,
        ));
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
        json!({"state":if kind=="connection.open"{"connected"}else if a.error.is_some(){"error"}else{"closed"}}),
        true,
    );
    if a.error.is_some() {
        event.level = "warn".into();
    }
    event
}
pub(super) fn voice_event(p: &Platform, l: &crate::voice::Lease, kind: &str, time: u64) {
    if let Ok(d) = p.db.catalog()
        && d.members.contains_key(&l.owner)
    {
        let mut data = json!({"session":launch_ref(&l.id),"channel":l.channel});
        if kind != "voice.grant" {
            data["heartbeat_age_ms"] = time
                .saturating_sub(l.expires.saturating_sub(crate::voice::LEASE_SECONDS))
                .saturating_mul(1000)
                .into();
        }
        p.controller.timeline.record(Event::server(
            kind,
            ContextFields {
                profile: d.player_id(&l.owner),
                group: d.groups.member(&l.owner).into(),
                visit: l.visit.clone().unwrap_or_default(),
                ..Default::default()
            },
            data,
            false,
        ));
    }
}
#[derive(Deserialize)]
struct Batch {
    client_ms: Option<i64>,
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
// Browser IDs are diagnostic UUIDs. Preserve them across upgrades. Older
// non-UUID identifiers still deduplicate without retaining their raw value.
fn page_ref(s: &str) -> String {
    if s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        s.to_owned()
    } else {
        hash(&format!("diagnostics/page/{s}"))[..32].to_owned()
    }
}
pub(crate) fn unreached_fetch(data: &Value) -> bool {
    matches!(data["status"].as_u64(), Some(502..=504))
        || (data["status"].is_null()
            && matches!(
                data["class"].as_str(),
                Some("TypeError" | "NetworkError" | "TimeoutError")
            ))
}
fn kind_level(k: &str, data: &Value) -> Option<&'static str> {
    Some(match k {
        "link" if matches!(data["state"].as_str(), Some("down" | "degraded")) => "warn",
        "op" if data["outcome"] == "fail" => "error",
        "fetch.fail" if unreached_fetch(data) => "warn",
        "op" | "link" | "page.vitals" => "info",
        // Old open tabs still post this poll every second. Discard it at ingestion.
        "mp.connected" | "mp.connecting" | "mp.ended" => return None,
        "voice.disconnected"
            if !matches!(
                data["stop_reason"].as_str(),
                Some("user_toggle" | "pagehide")
            ) =>
        {
            "warn"
        }
        "js.error" | "js.rejection" | "console.error" | "webgl.lost" => "error",
        "resource.error" | "net.offline" | "voice.fail" | "voice.reconnecting"
        | "page.discarded" | "recorder.dropped" => "warn",
        "page.load" | "page.show" | "page.hide" | "page.freeze" | "page.resume" | "page.exit"
        | "page.restore" | "net.online" | "game.frame_loaded" | "webgl.restored"
        | "voice.connected" | "voice.disconnected" | "voice.stopped" | "voice.reconnected"
        | "mem.sample" | "error.repeat" => "info",
        _ => return None,
    })
}
fn one(value: &Value, allowed: &[&str]) -> Option<Value> {
    value
        .as_str()
        .filter(|s| allowed.contains(s))
        .map(Value::from)
}
const LINKS: &[&str] = &["voice", "voice-control", "chat", "mp", "launch"];
const OPS: &[&str] = &["voice.join", "game.load", "mp.join"];
const STATES: &[&str] = &["none", "connecting", "up", "degraded", "down", "ended"];
const STAGES: &[&str] = &[
    "frame",
    "engine",
    "first_frame",
    "microphone",
    "grant",
    "connect",
    "publish",
    "playback",
    "admission",
];
const ERRORS: &[&str] = &[
    "Error",
    "TypeError",
    "SyntaxError",
    "ReferenceError",
    "RangeError",
    "RuntimeError",
    "AbortError",
    "TimeoutError",
    "NetworkError",
    "NotAllowedError",
    "NotFoundError",
    "SecurityError",
    "InvalidStateError",
    "CompileError",
    "LinkError",
    "ResourceError",
];
pub(crate) fn code_file(name: &str) -> bool {
    use regex_automata::meta::Regex;
    use std::sync::LazyLock;
    static FILE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"\A[A-Za-z_][A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]+)*\.(?:[cm]?js|jsx|tsx?|wasm|html?)\z",
        )
        .unwrap()
    });
    static PRIVATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[a-fA-F0-9]{32}|(?:[0-9]{1,3}\.){3}[0-9]{1,3}").unwrap());
    name.len() <= 80 && FILE.is_match(name) && !PRIVATE.is_match(name)
}
fn code_frame(frame: &str) -> bool {
    let mut parts = frame.rsplitn(3, ':');
    let position = |s: &str| (1..=9).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    parts.next().is_some_and(position)
        && parts.next().is_some_and(position)
        && parts.next().is_some_and(code_file)
}
fn clean(kind: &str, data: &Value) -> Value {
    let mut out = json!({});
    if matches!(kind, "js.error" | "js.rejection" | "console.error") {
        if let Some(file) = data["file"].as_str().filter(|s| code_file(s)) {
            out["file"] = file.into();
        }
        if let Some(input) = data["frames"].as_array() {
            let frames: Vec<_> = input
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| code_frame(s))
                .take(5)
                .collect();
            if !frames.is_empty() {
                out["frames"] = json!(frames);
            }
        }
    }
    for key in [
        "line",
        "col",
        "status",
        "count",
        "duration_ms",
        "held_ms",
        "down_ms",
        "flaps",
        "heap_bytes",
        "wasm_bytes",
        "dom_nodes",
        "fps_low",
        "bytes",
        "cached_ratio",
        "largest_ms",
        "uncertainty_ms",
    ] {
        if let Some(n) = data[key].as_u64().filter(|n| *n < 1 << 45) {
            out[key] = n.into();
        }
    }
    for (key, allowed) in [
        ("class", ERRORS),
        ("name", if kind == "link" { LINKS } else { OPS }),
        ("prev", STATES),
        (
            "state",
            if kind == "page.exit" {
                &["departed", "cached"][..]
            } else {
                STATES
            },
        ),
        ("outcome", &["ok", "fail", "abandoned"]),
        ("stage", STAGES),
        (
            "cause",
            &["reload_update", "discarded", "navigate", "reload"],
        ),
        (
            "method",
            &[
                "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "OTHER",
            ],
        ),
    ] {
        if let Some(v) = one(&data[key], allowed) {
            out[key] = v;
        }
    }
    if let Some(s) = data["reason"].as_str()
        && ([
            "user",
            "page_exit",
            "access_lost",
            "server_ended",
            "lease_expired",
            "network",
            "deploy",
            "visit_end",
            "host_left",
            "timeout",
            "error",
        ]
        .contains(&s)
            || s.strip_prefix("error:")
                .is_some_and(|e| ERRORS.contains(&e)))
    {
        out["reason"] = s.into();
    }
    if let Some(r) = one(
        &data["down_reason"],
        &[
            "network",
            "user",
            "access_lost",
            "server_ended",
            "timeout",
            "visit_end",
            "error",
        ],
    ) {
        out["down_reason"] = r;
    }
    for (key, len) in [
        ("fingerprint", 8),
        ("session", 20),
        ("room", 32),
        ("version", 64),
    ] {
        if let Some(s) = data[key]
            .as_str()
            .filter(|s| s.len() == len && s.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            out[key] = s.into();
        }
    }
    if let Some(s) = data["tab"].as_str().filter(|s| id(s)) {
        out["tab"] = page_ref(s).into();
    }
    if let Some(route) = data["route"].as_str() {
        const PARTS: &str = "api _infuser v1 v2 voice control heartbeat profile appearance settings chat presence peers health context launch sessions saves cloud groups joinability ack input apps runtime signals ice player-pages close :id";
        let valid_template = route.len() < 160
            && route.strip_prefix('/').is_some_and(|path| {
                path.split('/')
                    .all(|part| PARTS.split_whitespace().any(|p| p == part))
            });
        if valid_template || ["asset", "external", "unknown"].contains(&route) {
            out["route"] = route.into();
        }
    }
    if kind == "page.load"
        && let Some(client) = data["client"].as_str()
        && let Some((browser, os)) = client.split_once(' ')
        && [
            "Android",
            "iPhone",
            "iPad",
            "Windows",
            "CrOS",
            "Macintosh",
            "Linux",
            "",
        ]
        .contains(&os)
        && (browser == "browser"
            || browser.split_once('/').is_some_and(|(brand, major)| {
                ["Firefox", "Edg", "Chrome", "Version"].contains(&brand)
                    && !major.is_empty()
                    && major.len() <= 3
                    && major.bytes().all(|c| c.is_ascii_digit())
            }))
    {
        out["client"] = client.into();
    }
    if kind == "page.exit"
        && let Some(names) = data["links"].as_array()
    {
        out["links"] = names
            .iter()
            .take(LINKS.len())
            .filter_map(|v| one(v, LINKS))
            .collect();
    }
    if kind == "page.load"
        && let Some(theme) = data["appearance"]["theme"]
            .as_str()
            .filter(|id| themes::known_theme(id))
        && let Some(source) = one(
            &data["appearance"]["source"],
            &["server", "legacy", "default", "other_tab"],
        )
    {
        out["appearance"] = json!({"theme":theme,"source":source});
    }
    for (key, fields) in [("stages_ms", STAGES), ("stalls", &["count", "max_ms"])] {
        for &f in fields {
            if let Some(n) = data[key][f].as_u64().filter(|n| *n < 1 << 45) {
                out[key][f] = n.into();
            }
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
        let Some(level) = kind_level(&e.kind, &e.data) else {
            continue;
        };
        // The first request can arrive from a device with a skewed clock. The
        // difference between event time and send time still preserves its age.
        let at = match b.client_ms {
            Some(sent) if e.data["uncertainty_ms"].is_null() => {
                received_at.saturating_add(e.at.saturating_sub(sent))
            }
            _ => e.at,
        };
        if !id(&e.page) || at < received_at - 48 * 3_600_000 || at > received_at + 60_000 {
            continue;
        }
        let src = if !is_game {
            "library"
        } else if e.src == "game" {
            "game"
        } else {
            "player"
        };
        let data = clean(&e.kind, &e.data);
        let game_signal = match e.kind.as_str() {
            "link" => matches!(data["name"].as_str(), Some("mp" | "launch")),
            "op" => matches!(data["name"].as_str(), Some("mp.join" | "game.load")),
            _ => src == "game" || e.kind.starts_with("game."),
        };
        let visible = is_game && game_signal;
        events.push(Event {
            at: at.min(received_at),
            received_at,
            context: ctx.clone(),
            page: page_ref(&e.page),
            seq: Some(e.seq),
            src: src.into(),
            level: level.into(),
            kind: e.kind,
            data,
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
    if (game
        || library
        || response
            .extensions()
            .get::<platform::SessionEnded>()
            .is_some())
        && (status.is_client_error() || status.is_server_error() || slow)
    {
        let denial = response
            .extensions()
            .get::<platform::SessionEnded>()
            .cloned();
        let mut context = if game || denial.is_some() {
            game_token(&h)
                .and_then(|token| {
                    p.db.diagnostic_launch(&hash(token), host(&h))
                        .ok()
                        .flatten()
                })
                .and_then(|(id, l)| launch_fields(&p, &id, &l).ok())
                .or_else(|| {
                    denial.as_ref().map(|f| ContextFields {
                        launch: f.launch.clone().unwrap_or_default(),
                        ..Default::default()
                    })
                })
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
        {
            let context = context.unwrap_or_default();
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
                json!({"route":route,"method":match method.as_str(){m @ ("GET"|"POST"|"PUT"|"PATCH"|"DELETE"|"OPTIONS"|"HEAD")=>m,_=>"OTHER"},"status":status.as_u16(),"duration_ms":started.elapsed().as_millis() as u64,"request":request_id}),
                visible,
            );
            if let Some(f) = denial {
                e.data["reason"] = json!(f.reason);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_basenames_match_the_browser_fixtures() {
        let fixtures: Vec<(String, bool)> = serde_json::from_str(include_str!(
            "../../tests/fixtures/diagnostic-basenames.json"
        ))
        .unwrap();
        for (name, valid) in fixtures {
            assert_eq!(code_file(&name), valid, "{name:?}");
        }
    }
    #[test]
    fn batch_fields_accept_measurements_and_drop_free_text() {
        let dirty = json!({"class":"TypeError", "route":"/api/chat/customer?secret#home",
            "message":"message body", "stack":"stack body", "access_token":"abc123",
            "file":"https://192.0.2.1/", "device":"device label", "line":4, "reason":"unknown user input",
            "appearance":{"theme":"nightglass","source":"server","colour":"#123456"},
            "client":"Firefox/125 Linux", "session":"not-an-issued-ref"});
        assert_eq!(
            clean("page.load", &dirty),
            json!({"class":"TypeError","line":4,"appearance":{"theme":"nightglass","source":"server"},"client":"Firefox/125 Linux"})
        );
        let payload = json!({"file":"engine.min.js","frames":["engine.min.js:2:9","trace.js?token=secret:1:2",
            "https://server/asset.js:1:2","a192.0.2.1.js:1:2","source.js:1:field","core.wasm:20:8"],
            "message":"private", "stack":"Error: private"});
        assert_eq!(
            clean("js.rejection", &payload),
            json!({"file":"engine.min.js","frames":["engine.min.js:2:9","core.wasm:20:8"]})
        );
        assert_eq!(clean("page.exit", &payload), json!({}));
        assert_eq!(kind_level("fetch.fail", &json!({"status":404})), None);
        assert_eq!(
            kind_level("fetch.fail", &json!({"class":"AbortError"})),
            None
        );
        assert_eq!(kind_level("mp.connected", &json!({})), None);
        assert_eq!(kind_level("api.fail", &json!({"status":502})), None);
    }
}
