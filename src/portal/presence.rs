//! Read-only discovery never creates a multiplayer session or a server lease.
use super::*;
use std::collections::BTreeSet;

pub(super) fn capabilities() -> Value {
    json!({"version":1,"experimental":true,"game_activity":["title_screen","single_player","multiplayer"],"joinability":["peer_hosted_webrtc","dedicated_websocket"]})
}

pub(super) async fn library_input(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let profile = p.member(&h, true).await?;
    record_input(&p.db, &profile)?;
    Ok(reply(json!({"ok":true})))
}

pub(super) async fn game_input(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    let (_, launch) = p.game(&h)?;
    if !launch.guest {
        record_input(&p.db, &launch.email)?;
    }
    Ok(reply(json!({"ok":true})))
}

fn record_input(db: &Database, profile: &str) -> Result<()> {
    db.connection.lock().unwrap().execute(
        "INSERT INTO profile_input VALUES(?1,?2) ON CONFLICT(profile) DO UPDATE SET seen=excluded.seen WHERE seen<excluded.seen",
        rusqlite::params![profile,now()])?;
    Ok(())
}

pub(super) fn input_status(last: Option<u64>, time: u64) -> &'static str {
    match last.map(|t| time.saturating_sub(t)) {
        Some(0..120) => "active",
        Some(120..600) => "idle",
        _ => "away",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GameActivity {
    activity: String,
}

pub(super) async fn set_activity(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(report): Json<GameActivity>,
) -> Answer {
    p.origin(&h, host(&h))?;
    let (id, launch) = p.game(&h)?;
    let data = p.db.read()?;
    let manifest = &data
        .builds
        .get(&build_key(&launch.app, &launch.build))
        .ok_or_else(|| fault(410, "Game build expired."))?
        .manifest;
    use crate::manifest::PlayMode;
    let allowed = match report.activity.as_str() {
        "title_screen" => true,
        "single_player" => manifest.play_modes.contains(&PlayMode::Solo),
        "multiplayer" => manifest.play_modes.contains(&PlayMode::Multiplayer),
        _ => false,
    };
    ensure!(
        allowed,
        fault(422, "Choose an activity supported by this game.")
    );
    let updated =
        p.db.connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO launch_modes SELECT id,?2 FROM runtime_launches WHERE id=?1
         ON CONFLICT(launch) DO UPDATE SET mode=excluded.mode",
                rusqlite::params![id, report.activity],
            )
            .map_err(anyhow::Error::from)?;
    ensure!(updated > 0, fault(410, "Game session expired."));
    Ok(reply(json!({"ok":true})))
}

// Read these small social tables only for Library snapshots, never for routine game heartbeats.
#[derive(Default)]
pub(super) struct LibraryActivity {
    pub inputs: BTreeMap<String, u64>,
    pub modes: BTreeMap<String, String>,
}
impl LibraryActivity {
    pub fn read(db: &Database) -> Result<Self> {
        let c = db.connection.lock().unwrap();
        let mut inputs = c.prepare_cached("SELECT profile,seen FROM profile_input")?;
        let mut modes = c.prepare_cached("SELECT launch,mode FROM launch_modes")?;
        Ok(Self {
            inputs: inputs
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?,
            modes: modes
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?,
        })
    }
}

pub(super) fn live_launches(data: &platform::Data, app: &str, time: u64) -> BTreeSet<String> {
    data.launches
        .iter()
        .filter(|(_, l)| {
            l.runtime_id() == app
                && l.exchanged
                && l.expires > time
                && data.members.get(&l.email).is_some_and(|m| m.enabled)
                && data.launch_allowed(l, time)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

pub(super) async fn snapshot(State(p): State<Platform>, h: HeaderMap) -> Answer {
    if h.contains_key("origin") {
        p.origin(&h, host(&h))?;
    }
    ensure!(
        h.get("sec-fetch-site").is_none_or(|v| v == "same-origin"),
        fault(403, "read presence from the game origin")
    );
    let _guard = p.publishing.lock().unwrap();
    let (id, launch) = p.game(&h)?;
    let data = p.db.read()?;
    let time = now();
    let own_id = if launch.guest {
        format!("guest-{id}")
    } else {
        data.player_id(&launch.email)
    };
    let mut players = BTreeMap::new();
    for (email, seen) in &data.presence {
        if seen + 90 <= time
            || !data.group_participant(email, data.deployment_group(launch.runtime_id()), time)
        {
            continue;
        }
        if let Some(member) = data.members.get(email).filter(|m| m.enabled) {
            let player = data.player_id(email);
            players.insert(
                player.clone(),
                json!({"player_id":player,"name":member.name,"guest":false,
                "is_self":player==own_id,"online":true,"game_open":false,"multiplayer":false}),
            );
        }
    }
    let live = live_launches(&data, launch.runtime_id(), time);
    for id in &live {
        let l = &data.launches[id];
        if l.seen + 90 <= time {
            continue;
        }
        let player = if l.guest {
            format!("guest-{id}")
        } else {
            data.player_id(&l.email)
        };
        let name = if l.guest {
            "Guest"
        } else {
            &data.members[&l.email].name
        };
        players.insert(
            player.clone(),
            json!({"player_id":player,"name":name,"guest":l.guest,
            "is_self":player==own_id,"online":true,"game_open":true,"multiplayer":false}),
        );
    }
    p.peers.retain_launches(launch.runtime_id(), &live, time);
    let rooms = p
        .peers
        .discover_all(launch.runtime_id(), &launch.build, time);
    for session in &rooms {
        let members = session["players"].as_array().unwrap();
        for member in members {
            let player = member["player_id"].as_str().unwrap();
            players.insert(
                player.to_owned(),
                json!({"player_id":player,"name":member["name"],"guest":member["guest"],
                "is_self":player==own_id,"online":true,"game_open":true,"multiplayer":true}),
            );
        }
    }
    let build = &data.builds[&build_key(&launch.app, &launch.build)];
    let dedicated = build.manifest.dedicated_sessions();
    let sessions = if dedicated {
        super::dedicated::live_sessions(&p, &data, launch.runtime_id(), &launch.build, time)
    } else {
        vec![]
    };
    for room in &sessions {
        for member in room["players"].as_array().unwrap() {
            let player = member["player_id"].as_str().unwrap();
            players.insert(player.to_owned(),json!({"player_id":player,"name":member["name"],"guest":member["guest"],
                "is_self":player==own_id,"online":true,"game_open":true,"multiplayer":room["activity"] == "multiplayer"}));
        }
    }
    let supported = dedicated
        || build.manifest.multiplayer.as_ref().is_some_and(|m| {
            m.mode == Mode::PeerHosted && m.transport == crate::manifest::Transport::Webrtc
        });
    let unavailable = if !supported {
        Some("unsupported_topology")
    } else if !dedicated && p.relay.is_none() {
        Some("relay_unavailable")
    } else if !data
        .deployed(launch.runtime_id())
        .is_some_and(|a| a.build == launch.build)
    {
        Some("newer_build")
    } else if dedicated && data.pending_promotions.contains_key(launch.runtime_id()) {
        Some("server_draining")
    } else if !dedicated && !p.peers.available(time) {
        Some("coordinator_restarting")
    } else {
        None
    };
    // Older games can auto-match only with the matchmaking room.
    let matchmaking = rooms.iter().find(|room| room["matchmaking"] == true);
    let can_create = unavailable.is_none()
        && (!dedicated
            || sessions.is_empty()
            || build
                .manifest
                .sessions
                .as_ref()
                .is_some_and(|s| s.policy == crate::dedicated::Policy::Multiple));
    Ok(reply(
        json!({"app_id":launch.app,"build_id":launch.build,"self_id":own_id,
        "players":players.into_values().collect::<Vec<_>>(),"session":matchmaking,"peer_sessions":rooms,"sessions":sessions,
        "can_create":can_create,"can_start":can_create && (dedicated || matchmaking.is_none()),"unavailable_reason":unavailable}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Joinability {
    epoch: String,
    joinable: bool,
}

pub(super) async fn set_joinable(
    State(p): State<Platform>,
    Path(peer): Path<String>,
    h: HeaderMap,
    Json(body): Json<Joinability>,
) -> Answer {
    p.origin(&h, host(&h))?;
    let (id, launch) = p.game(&h)?;
    Ok(reply(p.peers.set_joinable(
        launch.runtime_id(),
        &peer,
        &id,
        &body.epoch,
        body.joinable,
        now(),
    )?))
}
