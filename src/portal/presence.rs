//! Read-only discovery never creates a multiplayer session or a server lease.
use super::*;
use std::collections::BTreeSet;

pub(super) fn capabilities() -> Value {
    json!({"version":1,"experimental":true,"joinability":["peer_hosted_webrtc","dedicated_websocket"]})
}

pub(super) fn live_launches(data: &platform::Data, app: &str, time: u64) -> BTreeSet<String> {
    data.launches
        .iter()
        .filter(|(_, l)| {
            l.runtime_id() == app
                && l.exchanged
                && l.expires > time
                && data.members.get(&l.email).is_some_and(|m| m.enabled)
                && data.launch_allowed(l,time)
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
        hash(&launch.email)
    };
    let mut players = BTreeMap::new();
    for (email, seen) in &data.presence {
        if seen + 90 <= time || !data.group_participant(email,data.deployment_group(launch.runtime_id()),time) {
            continue;
        }
        if let Some(member) = data.members.get(email).filter(|m| m.enabled) {
            let player = hash(email);
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
            hash(&l.email)
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
    let session = p.peers.discovery(launch.runtime_id(), &launch.build, time);
    if let Some(members) = session["players"].as_array() {
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
    let sessions = if dedicated {super::dedicated::live_sessions(&p,&data,launch.runtime_id(),&launch.build,time)} else {vec![]};
    for room in &sessions {
        for member in room["players"].as_array().unwrap() {
            let player = member["player_id"].as_str().unwrap();
            players.insert(player.to_owned(),json!({"player_id":player,"name":member["name"],"guest":member["guest"],
                "is_self":player==own_id,"online":true,"game_open":true,"multiplayer":room["activity"] == "multiplayer"}));
        }
    }
    let supported = dedicated || build.manifest.multiplayer.as_ref().is_some_and(|m| {
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
    Ok(reply(
        json!({"app_id":launch.app,"build_id":launch.build,"self_id":own_id,
        "players":players.into_values().collect::<Vec<_>>(),"session":session,"sessions":sessions,
        "can_start":unavailable.is_none() && if dedicated {build.manifest.sessions.as_ref().is_some_and(|s| s.policy == crate::dedicated::Policy::Multiple) || sessions.is_empty()} else {session.is_null()},"unavailable_reason":unavailable}),
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
