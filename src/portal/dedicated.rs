use super::*;
use crate::connections::State as ConnectionState;
use crate::dedicated::{self as model, Action, Activity, Policy, Report, Scope, Session};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;

fn current(p: &Platform, scope: &Scope) -> Result<Build> {
    let data = p.db.read()?;
    let app = data
        .deployed(scope.runtime_id())
        .ok_or_else(|| fault(401, "server retired"))?;
    ensure!(
        app.build == scope.build_id
            && data.deployment_group(scope.runtime_id()) == scope.group_id
            && data.source_app(scope.runtime_id()) == scope.app_id,
        fault(401, "server scope changed")
    );
    let state = p.controller.snapshot();
    let instance = state
        .games
        .get(scope.runtime_id())
        .ok_or_else(|| fault(401, "server retired"))?;
    ensure!(
        instance.reserved() && instance.attempt == scope.instance_id,
        fault(401, "server instance retired")
    );
    let build = &data.builds[&build_key(&scope.app_id, &scope.build_id)];
    ensure!(
        build.manifest.dedicated_sessions()
            && build
                .server
                .as_ref()
                .is_some_and(|server| server.web && server == &instance.spec),
        fault(401, "server contract unavailable")
    );
    Ok(build.clone())
}
fn authorize(p: &Platform, h: &HeaderMap) -> Result<(Scope, Build)> {
    p.main_host(h)?;
    let token = bearer(h).ok_or_else(|| fault(401, "server access token required"))?;
    let scope = p
        .connection_signer
        .access_scope(token, now())
        .map_err(|_| fault(401, "server access token expired or invalid"))?;
    let build = current(p, &scope)?;
    Ok((scope, build))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Register {
    app_id: String,
    instance_id: String,
}
pub(super) async fn register(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Register>,
) -> Answer {
    p.main_host(&h)?;
    let _guard = p.publishing.lock().unwrap();
    let state = p.controller.snapshot();
    let data = p.db.read()?;
    let (runtime_id, instance) = state
        .games
        .iter()
        .find(|(id, i)| i.attempt == body.instance_id && data.source_app(id) == body.app_id)
        .ok_or_else(|| fault(401, "unknown server"))?;
    ensure!(
        instance.reserved() && instance.attempt == body.instance_id,
        fault(401, "server instance retired")
    );
    let expected = crate::connections::instance_token(
        &p.config().controller.state_dir,
        &state.owner,
        runtime_id,
        &body.instance_id,
    )?;
    ensure!(
        bearer(&h).is_some_and(|token| bool::from(token.as_bytes().ct_eq(expected.as_bytes()))),
        fault(401, "invalid instance credential")
    );
    let data = p.db.read()?;
    let scope = Scope {
        deployment_id: (runtime_id != &body.app_id).then(|| runtime_id.clone()),
        app_id: body.app_id.clone(),
        build_id: data
            .deployed(runtime_id)
            .ok_or_else(|| fault(401, "server retired"))?
            .build
            .clone(),
        instance_id: body.instance_id,
        group_id: data.deployment_group(runtime_id).into(),
        exp: now() + 120,
    };
    let build = current(&p, &scope)?;
    Ok(reply(
        json!({"token":p.connection_signer.access_token(&scope)?,"expires_at":scope.exp,
        "scope":scope,"coordinator":p.connection_epoch,"sessions":build.manifest.sessions,
        "connection":build.manifest.server_connection.unwrap_or_default(),"limits":model::capabilities(),
        "draining":data.pending_promotions.contains_key(runtime_id)}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    coordinator: String,
    seq: u64,
    sessions: Vec<Session>,
}
pub(super) async fn report(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Snapshot>,
) -> Answer {
    let _guard = p.publishing.lock().unwrap();
    let (scope, build) = authorize(&p, &h)?;
    ensure!(
        body.coordinator == p.connection_epoch,
        model::fault(
            409,
            "coordinator_restarted",
            "Refresh server registration and send a new snapshot."
        )
    );
    ensure!(
        body.seq > 0
            && body.seq <= 9_007_199_254_740_991
            && body.sessions.len() <= model::MAX_SESSIONS,
        fault(422, "invalid session snapshot")
    );
    if let Some(previous) = p
        .dedicated
        .lock()
        .unwrap()
        .0
        .get(scope.runtime_id())
        .filter(|r| r.scope.instance_id == scope.instance_id && body.seq <= r.seq)
    {
        return Ok(reply(
            json!({"accepted":false,"seq":previous.seq,"coordinator":p.connection_epoch}),
        ));
    }
    let declaration = build.manifest.sessions.as_ref().unwrap();
    let mut sessions = BTreeMap::new();
    let mut all_connections = BTreeSet::new();
    let attempts = p.connections.lock().unwrap();
    for mut session in body.sessions {
        session
            .validate(declaration)
            .map_err(|e| fault(422, &e.to_string()))?;
        ensure!(
            build.manifest.server_activity(&session.activity),
            fault(422, "session activity is not declared by this game")
        );
        let mut valid = Vec::new();
        for id in &session.connections {
            ensure!(
                all_connections.insert(id.clone()),
                fault(422, "a connection may belong to only one session")
            );
            // Unknown/retired handles are ignored after restart; they never become
            // user-supplied player identities. An admitted handle cannot move rooms.
            if let Some(a) = attempts.0.get(id).filter(|a| {
                a.identity.runtime_id() == scope.runtime_id()
                    && a.identity.instance_id == scope.instance_id
                    && !a.state.terminal()
            }) {
                ensure!(
                    a.session.as_ref().is_some_and(|s| s.id == session.id),
                    fault(422, "connection was not admitted to this session")
                );
                valid.push(id.clone());
            }
        }
        session.connections = valid;
        ensure!(
            !sessions.contains_key(&session.id),
            fault(422, "duplicate session")
        );
        sessions.insert(session.id.clone(), session);
    }
    if declaration.policy == Policy::Shared {
        ensure!(
            sessions
                .values()
                .filter(|s| s.activity == Activity::Multiplayer)
                .count()
                <= 1,
            fault(422, "shared policy permits only one multiplayer session")
        );
    }
    let mut directory = p.dedicated.lock().unwrap();
    let previous = directory
        .0
        .get(scope.runtime_id())
        .filter(|r| r.scope.instance_id == scope.instance_id);
    if previous.is_some_and(|r| body.seq <= r.seq) {
        return Ok(reply(
            json!({"accepted":false,"seq":previous.unwrap().seq,"coordinator":p.connection_epoch}),
        ));
    }
    let mut retired = previous.map(|r| r.retired.clone()).unwrap_or_default();
    if let Some(old) = previous {
        retired.extend(
            old.sessions
                .keys()
                .filter(|id| !sessions.contains_key(*id))
                .cloned(),
        );
    }
    ensure!(
        retired.len() <= 4096,
        fault(429, "session incarnation limit reached; drain this process")
    );
    ensure!(
        sessions.keys().all(|id| !retired.contains(id)),
        fault(409, "retired session IDs cannot be reused")
    );
    directory.0.insert(
        scope.runtime_id().to_owned(),
        Report {
            scope: scope.clone(),
            seq: body.seq,
            seen: now(),
            sessions,
            retired,
        },
    );
    drop(directory);
    drop(attempts);
    finish_drain(&p, &scope)?;
    Ok(reply(
        json!({"accepted":true,"seq":body.seq,"coordinator":p.connection_epoch}),
    ))
}

pub(super) fn live_sessions(
    p: &Platform,
    data: &platform::Data,
    app: &str,
    build: &str,
    time: u64,
) -> Vec<Value> {
    let state = p.controller.snapshot();
    let Some(instance) = state.games.get(app).filter(|i| i.reserved()) else {
        return vec![];
    };
    let live = presence::live_launches(data, app, time);
    let attempts = p.connections.lock().unwrap();
    let directory = p.dedicated.lock().unwrap();
    let Some(report) = directory.0.get(app).filter(|r| {
        r.scope.build_id == build
            && r.scope.instance_id == instance.attempt
            && r.scope.group_id == data.deployment_group(app)
            && r.seen + model::REPORT_SECONDS > time
    }) else {
        return vec![];
    };
    report.sessions.values().map(|session| {
        let mut players = BTreeMap::new();
        for id in &session.connections {
            if let Some(a) = attempts.0.get(id).filter(|a| a.state == ConnectionState::Connected && live.contains(&a.identity.launch_id) && a.session.as_ref().is_some_and(|s|s.id == session.id)) {
                players.insert(a.identity.player_id.clone(),json!({"player_id":a.identity.player_id,"name":a.identity.name,"guest":a.identity.guest}));
            }
        }
        json!({"id":session.id,"label":session.label,"activity":session.activity,
            "joinable":session.joinable && !data.pending_promotions.contains_key(app) && session.connections.len() < session.max_players as usize,
            "max_players":session.max_players,"players":players.into_values().collect::<Vec<_>>()})
    }).collect()
}

pub(super) fn check_request(p: &Platform, launch: &Launch, request: &model::Request) -> Result<()> {
    let data = p.db.read()?;
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    ensure!(
        build.manifest.dedicated_sessions(),
        model::fault(
            422,
            "unsupported_topology",
            "This build does not declare managed sessions."
        )
    );
    let targeted = matches!(request.action, Action::Join | Action::Resume);
    ensure!(
        targeted == request.target.is_some(),
        model::fault(
            422,
            "invalid_target",
            "Join and resume require an exact target; other actions do not accept one."
        )
    );
    if let Some(activity) = &request.activity {
        ensure!(
            build.manifest.server_activity(activity),
            model::fault(
                422,
                "unsupported_activity",
                "This game does not declare that activity."
            )
        );
    }
    ensure!(
        targeted || request.activity.is_some(),
        model::fault(
            422,
            "activity_required",
            "Choose single player or multiplayer in the game."
        )
    );
    if data.pending_promotions.contains_key(launch.runtime_id()) && request.action != Action::Resume
    {
        return Err(model::fault(
            409,
            "server_draining",
            "An update is waiting; existing runs are finishing.",
        ));
    }
    if let Some(target) = &request.target {
        ensure!(
            model::session_id(&target.session_id)
                && target
                    .player_id
                    .as_ref()
                    .is_none_or(|v| !v.is_empty() && v.len() <= 96),
            model::fault(422, "invalid_target", "Invalid session target.")
        );
        // A resume may reach a closed/temporarily unreported room, but the server
        // helper must hold this launch's bounded reconnect reservation. Never create.
        if request.action == Action::Resume {
            return Ok(());
        }
        let sessions = live_sessions(p, &data, launch.runtime_id(), &launch.build, now());
        let session = sessions
            .iter()
            .find(|s| s["id"] == target.session_id)
            .ok_or_else(|| model::fault(409, "session_ended", "That session has ended."))?;
        if let Some(player) = &target.player_id {
            ensure!(
                session["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["player_id"] == *player),
                model::fault(409, "friend_left", "Your friend has left this session.")
            );
        }
        ensure!(
            session["players"].as_array().unwrap().len()
                < session["max_players"].as_u64().unwrap() as usize,
            model::fault(409, "session_full", "This session is full.")
        );
        ensure!(
            session["joinable"] == true,
            model::fault(
                409,
                "session_unavailable",
                "This session cannot accept another player."
            )
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    session: Option<Session>,
    error: Option<String>,
}
pub(super) async fn admission(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(body): Json<Admission>,
) -> Answer {
    let _guard = p.publishing.lock().unwrap();
    let (scope, build) = authorize(&p, &h)?;
    ensure!(
        body.session.is_some() != body.error.is_some(),
        fault(422, "supply a session or an admission error")
    );
    let data = p.db.read()?;
    let mut attempts = p.connections.lock().unwrap();
    let attempt = attempts
        .0
        .get(&id)
        .filter(|a| {
            a.identity.runtime_id() == scope.runtime_id()
                && a.identity.instance_id == scope.instance_id
                && a.identity.build_id == scope.build_id
        })
        .ok_or_else(|| fault(404, "unknown connection"))?
        .clone();
    if attempt.admission.is_some() {
        ensure!(
            attempt.session == body.session
                && attempt.admission.as_deref()
                    == Some(body.error.as_deref().unwrap_or("accepted")),
            fault(409, "admission result already committed")
        );
        return Ok(reply(json!({"accepted":true})));
    }
    ensure!(
        attempt.state == ConnectionState::Connecting && attempt.identity.exp > now(),
        fault(409, "connection attempt ended")
    );
    if let Some(error) = &body.error {
        ensure!(
            [
                "session_ended",
                "session_full",
                "friend_left",
                "session_unavailable",
                "server_draining",
                "capacity_unavailable",
                "resume_expired",
                "already_connected",
                "admission_rejected",
                "session_exists"
            ]
            .contains(&error.as_str()),
            fault(422, "unsupported admission error code")
        );
    }
    if let Some(session) = &body.session {
        let declaration = build.manifest.sessions.as_ref().unwrap();
        session
            .validate(declaration)
            .map_err(|e| fault(422, &e.to_string()))?;
        ensure!(
            build.manifest.server_activity(&session.activity),
            fault(422, "admitted activity is not declared")
        );
        ensure!(
            session.connections.is_empty(),
            fault(422, "admission session descriptor must omit connections")
        );
        if let Some(target) = &attempt.identity.request.target {
            ensure!(
                session.id == target.session_id,
                model::fault(409, "session_ended", "The target session was not admitted.")
            );
            if let Some(friend) = &target.player_id {
                ensure!(
                    attempts
                        .0
                        .values()
                        .any(|a| a.identity.runtime_id() == scope.runtime_id()
                            && a.identity.instance_id == scope.instance_id
                            && a.state == ConnectionState::Connected
                            && a.identity.player_id == *friend
                            && a.session.as_ref().is_some_and(|s| s.id == session.id)),
                    model::fault(409, "friend_left", "Your friend has left this session.")
                );
            }
        }
        ensure!(
            attempt
                .identity
                .request
                .activity
                .as_ref()
                .is_none_or(|a| a == &session.activity),
            fault(422, "admitted activity differs from request")
        );
        if attempt.identity.request.action == Action::Join {
            ensure!(
                session.joinable,
                model::fault(
                    409,
                    "session_unavailable",
                    "This session is no longer joinable."
                )
            );
        }
        ensure!(
            !data.pending_promotions.contains_key(scope.runtime_id())
                || attempt.identity.request.action == Action::Resume,
            model::fault(
                409,
                "server_draining",
                "This server is finishing its existing runs."
            )
        );
        let occupied = attempts
            .0
            .values()
            .filter(|a| {
                a.identity.runtime_id() == scope.runtime_id()
                    && a.identity.instance_id == scope.instance_id
                    && !a.state.terminal()
                    && a.session.as_ref().is_some_and(|s| s.id == session.id)
                    && !(attempt.identity.request.action == Action::Resume
                        && a.identity.launch_id == attempt.identity.launch_id)
            })
            .count();
        ensure!(
            occupied < session.max_players as usize,
            model::fault(409, "session_full", "The session is full.")
        );
        let mut directory = p.dedicated.lock().unwrap();
        let report = directory
            .0
            .entry(scope.runtime_id().to_owned())
            .or_insert_with(|| Report {
                scope: scope.clone(),
                seq: 0,
                seen: now(),
                sessions: BTreeMap::new(),
                retired: BTreeSet::new(),
            });
        if report.scope.instance_id != scope.instance_id {
            *report = Report {
                scope: scope.clone(),
                seq: 0,
                seen: now(),
                sessions: BTreeMap::new(),
                retired: BTreeSet::new(),
            };
        }
        ensure!(
            !report.retired.contains(&session.id),
            model::fault(409, "session_ended", "This session has already ended.")
        );
        if attempt.identity.request.action == Action::Play
            && session.activity == Activity::Multiplayer
            && report
                .sessions
                .values()
                .any(|s| s.activity == Activity::Multiplayer)
        {
            ensure!(
                report.sessions.contains_key(&session.id),
                model::fault(
                    409,
                    "session_exists",
                    "Choose an existing session or explicitly start a separate run."
                )
            );
        }
        if declaration.policy == Policy::Shared && session.activity == Activity::Multiplayer {
            ensure!(
                report
                    .sessions
                    .values()
                    .all(|s| s.activity != Activity::Multiplayer || s.id == session.id),
                model::fault(409, "session_exists", "Join the existing shared session.")
            );
        }
        ensure!(
            report.sessions.contains_key(&session.id)
                || report.sessions.len() < model::MAX_SESSIONS,
            model::fault(
                429,
                "capacity_unavailable",
                "No more sessions fit in this process."
            )
        );
        report
            .sessions
            .entry(session.id.clone())
            .or_insert_with(|| session.clone())
            .connections
            .push(id.clone());
        report.seen = now();
    }
    if body.session.is_some() && attempt.identity.request.action == Action::Resume {
        for (old_id, old) in &mut attempts.0 {
            if old_id != &id
                && old.identity.launch_id == attempt.identity.launch_id
                && old.identity.instance_id == scope.instance_id
                && old
                    .session
                    .as_ref()
                    .is_some_and(|s| Some(&s.id) == body.session.as_ref().map(|s| &s.id))
                && !old.state.terminal()
            {
                old.state = ConnectionState::Cancelled;
                old.error = Some("connection_replaced".into());
                old.updated = now();
            }
        }
    }
    let saved = attempts.0.get_mut(&id).unwrap();
    saved.session = body.session;
    saved.admission = Some(body.error.unwrap_or_else(|| "accepted".into()));
    Ok(reply(json!({"accepted":true})))
}

fn finish_drain(p: &Platform, scope: &Scope) -> Result<()> {
    if !p
        .db
        .read()?
        .pending_promotions
        .contains_key(scope.runtime_id())
    {
        return Ok(());
    }
    let active = p.connections.lock().unwrap().0.values().any(|a| {
        a.identity.runtime_id() == scope.runtime_id()
            && a.identity.instance_id == scope.instance_id
            && !a.state.terminal()
    });
    let empty = p
        .dedicated
        .lock()
        .unwrap()
        .0
        .get(scope.runtime_id())
        .is_some_and(|r| r.scope.instance_id == scope.instance_id && r.sessions.is_empty());
    if !active && empty {
        p.controller.stop(scope.runtime_id())?;
    }
    Ok(())
}
pub(super) async fn control(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let _guard = p.publishing.lock().unwrap();
    let (scope, _) = authorize(&p, &h)?;
    finish_drain(&p, &scope)?;
    let data = p.db.read()?;
    let state = p.controller.snapshot();
    let instance = &state.games[scope.runtime_id()];
    let draining = data.pending_promotions.get(scope.runtime_id());
    Ok(reply(
        json!({"coordinator":p.connection_epoch,"instance_id":scope.instance_id,
        "state":if instance.phase == crate::store::Phase::Stopping {"stopping"} else if draining.is_some() {"draining"} else {"running"},
        "pending_build":draining.map(|d|&d.build),"reconnect_grace_seconds":model::RECONNECT_SECONDS}),
    ))
}
