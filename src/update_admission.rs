//! What may delay switching to a staged release, and what that restart does.
//!
//! An open game tab renews `launch.seen` about every 25 seconds (every 5 seconds
//! during a visit). The old check treated any launch seen in the last 90 seconds
//! as "someone is playing", and `Gate::begin` also refused while any response
//! body was still open. A heartbeat is enough to fail every minute. The page is
//! already loaded, so the restart does not end that game.
//!
//! Peer rooms and voice membership live only in this process. Dedicated game
//! containers are adopted again after the restart; the sockets proxied through
//! the controller drop. An agent's publish, and a server image that is already
//! being built, are lost if the process dies mid-way.

use crate::{
    controller::now,
    deployment::{HostPolicy, HostStep, Record},
    maintenance::LiveRequest,
    platform::Data,
    portal::Platform,
    server_package::Phase,
    store::Phase as GamePhase,
};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const OPEN_SECONDS: u64 = 90;
const PLAY_WAIT: u64 = 10 * 60;
const PUBLISH_WAIT: u64 = 2 * 60 * 60;
const LONG_REQUEST: u64 = 3;

#[derive(Clone, Serialize)]
pub struct Item {
    pub kind: &'static str,
    pub holds: bool,
    pub summary: String,
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gives_up_at: Option<u64>,
}

pub struct Admission {
    pub ready: bool,
    /// A publish or server-image build is still in flight, but it has already
    /// been waited for as long as we will. Activation may interrupt it.
    pub interrupt_publishing: bool,
    pub waiting: String,
}

pub fn describe(p: &Platform) -> Result<Vec<Item>> {
    describe_since(p, p.controller.deployment.read().available_since)
}
pub fn describe_since(p: &Platform, available_since: Option<u64>) -> Result<Vec<Item>> {
    let time = now();
    let data = p.db.read()?;
    let state = p.controller.snapshot();
    let requests = p.controller.maintenance.requests();
    let mut items = Vec::new();
    let mut covered = BTreeSet::new();

    items.extend(publish_items(
        &data,
        &requests,
        available_since.is_some(),
        time,
    ));
    items.extend(package_items(
        &data,
        state.build_reservation.is_some(),
        available_since,
        time,
    ));
    for item in peer_items(
        &data,
        &p.peer_rooms().diagnostics(time),
        available_since,
        time,
    ) {
        if let Some(app) = item.summary_app.as_ref() {
            covered.insert(app.clone());
        }
        items.push(item.item);
    }
    for (app, item) in dedicated_items(&data, &state, available_since, time) {
        covered.insert(app);
        items.push(item);
    }
    if p.controller.maintenance.active() {
        items.push(maintenance_lease());
    }
    if let Some(item) = playing_item(&data, &covered, time) {
        items.push(item);
    }
    if let Some(item) = voice_item(p) {
        items.push(item);
    }
    if let Some(item) = stray_request(&requests, &items, time) {
        items.push(item);
    }
    Ok(items)
}

pub fn admission(p: &Platform) -> Result<Admission> {
    admission_since(p, p.controller.deployment.read().available_since)
}
pub fn admission_since(p: &Platform, available_since: Option<u64>) -> Result<Admission> {
    let items = describe_since(p, available_since)?;
    let holding: Vec<_> = items.iter().filter(|item| item.holds).collect();
    let publish_open = items
        .iter()
        .any(|item| matches!(item.kind, "publishing" | "packaging"));
    let waiting = holding
        .iter()
        .map(|item| item.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let waiting = clip(&waiting);
    Ok(Admission {
        ready: holding.is_empty(),
        interrupt_publishing: holding.is_empty() && publish_open,
        waiting,
    })
}

pub fn host_job() -> Item {
    Item {
        kind: "maintenance",
        holds: true,
        summary: "Another job on this computer, such as a backup, has the update waiting.".into(),
        effect: "Update anyway cannot skip that job. The update runs when the job finishes.".into(),
        since: None,
        gives_up_at: None,
    }
}

struct Named {
    item: Item,
    summary_app: Option<String>,
}

fn publish_items(
    data: &Data,
    requests: &[LiveRequest],
    update_waiting: bool,
    time: u64,
) -> Vec<Item> {
    let mut groups: BTreeMap<String, (bool, u64)> = BTreeMap::new();
    for request in requests.iter().filter(|request| request.publishing) {
        let app = publisher_app(&request.path).unwrap_or("").to_owned();
        let receiving = request.path.contains("/files/");
        groups
            .entry(app)
            .and_modify(|(was_receiving, started)| {
                *was_receiving |= receiving;
                *started = (*started).min(request.started);
            })
            .or_insert((receiving, request.started));
    }
    // Each open upload is bounded. A series of short file uploads keeps
    // waiting, which is the receive we don't want to cut off. A single
    // request that stays open for two hours no longer holds the release.
    groups
        .into_iter()
        .map(|(app, (receiving, started))| {
            let (holds, deadline) = within(started, PUBLISH_WAIT, time);
            let title = if app.is_empty() {
                None
            } else {
                Some(game_title(data, &app))
            };
            let age = age(time.saturating_sub(started));
            let summary = match (title.as_deref(), receiving) {
                (Some(title), true) => {
                    format!("Receiving \"{title}\" from an agent, started {age} ago.")
                }
                (Some(title), false) => {
                    format!("Publishing \"{title}\" from an agent, started {age} ago.")
                }
                (None, _) => format!("An agent is sending a game, started {age} ago."),
            };
            let effect = finish(
                "Updating now loses this. The agent has to send the game again.",
                holds,
                update_waiting,
                deadline,
                time,
            );
            Item {
                kind: "publishing",
                holds,
                summary,
                effect,
                since: Some(started),
                gives_up_at: Some(deadline),
            }
        })
        .collect()
}

fn package_items(
    data: &Data,
    reserved: bool,
    available_since: Option<u64>,
    time: u64,
) -> Vec<Item> {
    let building: Vec<_> = data
        .server_builds
        .iter()
        .filter(|(_, job)| matches!(job.state, Phase::Packaging | Phase::Checking))
        .collect();
    if building.is_empty() {
        if !reserved {
            return Vec::new();
        }
        let started = available_since.unwrap_or(time);
        return vec![package_item(None, started, available_since.is_some(), time)];
    }
    building
        .into_iter()
        .map(|(key, job)| {
            let started = if job.started > 0 {
                job.started
            } else {
                available_since.unwrap_or(time)
            };
            let title = key.split('/').next().map(|app| game_title(data, app));
            package_item(title, started, available_since.is_some(), time)
        })
        .collect()
}

fn package_item(title: Option<String>, started: u64, update_waiting: bool, time: u64) -> Item {
    let (holds, deadline) = within(started, PUBLISH_WAIT, time);
    let age = age(time.saturating_sub(started));
    let summary = match title {
        Some(title) => format!("Publishing the server image for \"{title}\", started {age} ago."),
        None => format!("Publishing a server image, started {age} ago."),
    };
    let effect = finish(
        "Restarting now stops that build, and it has to be started again.",
        holds,
        update_waiting,
        deadline,
        time,
    );
    Item {
        kind: "packaging",
        holds,
        summary,
        effect,
        since: Some(started),
        gives_up_at: Some(deadline),
    }
}

fn peer_items(
    data: &Data,
    diagnostics: &Value,
    available_since: Option<u64>,
    time: u64,
) -> Vec<Named> {
    let Some(sessions) = diagnostics["sessions"].as_array() else {
        return Vec::new();
    };
    let (holds, deadline) = play_window(available_since, time);
    let update_waiting = available_since.is_some();
    sessions
        .iter()
        .filter_map(|session| {
            let app = session["app_id"].as_str()?;
            let peers = session["peers"].as_array()?;
            let host = peers
                .iter()
                .find(|peer| peer["host"] == true)
                .and_then(|peer| peer["name"].as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Someone");
            let others: Vec<String> = peers
                .iter()
                .filter(|peer| peer["host"] != true)
                .filter_map(|peer| peer["name"].as_str())
                .filter(|name| !name.is_empty())
                .take(3)
                .map(str::to_owned)
                .collect();
            let title = game_title(data, app);
            let summary = if others.is_empty() {
                format!("{host} is hosting a shared game of \"{title}\".")
            } else {
                format!(
                    "{host} is hosting a shared game of \"{title}\" with {}.",
                    list_names(&others)
                )
            };
            let effect = finish(
                "Updating now ends that shared game. They need to start it again, which takes about half a minute.",
                holds,
                update_waiting,
                deadline.unwrap_or(time),
                time,
            );
            Some(Named {
                summary_app: Some(app.to_owned()),
                item: Item {
                    kind: "peer",
                    holds,
                    summary,
                    effect,
                    since: available_since,
                    gives_up_at: deadline,
                },
            })
        })
        .collect()
}

fn dedicated_items(
    data: &Data,
    state: &crate::store::State,
    available_since: Option<u64>,
    time: u64,
) -> Vec<(String, Item)> {
    let (holds, deadline) = play_window(available_since, time);
    let update_waiting = available_since.is_some();
    state
        .games
        .iter()
        .filter(|(_, game)| {
            matches!(
                game.phase,
                GamePhase::Starting | GamePhase::Ready | GamePhase::Stopping
            ) && game.leases.values().any(|lease| lease.expires > time)
        })
        .map(|(app, _)| {
            let names = launch_names(
                data,
                data.launches
                    .values()
                    .filter(|launch| launch.app == *app && launch_open(launch, time)),
            );
            let title = game_title(data, app);
            let summary = match names.as_slice() {
                [] => format!("Players are connected to the server for \"{title}\"."),
                [one] => format!("{one} is connected to the server for \"{title}\"."),
                _ => format!(
                    "{} are connected to the server for \"{title}\".",
                    list_names(&names)
                ),
            };
            let effect = finish(
                "Their connection drops for a few seconds, then they can rejoin. The game server keeps running.",
                holds,
                update_waiting,
                deadline.unwrap_or(time),
                time,
            );
            (
                app.clone(),
                Item {
                    kind: "dedicated",
                    holds,
                    summary,
                    effect,
                    since: available_since,
                    gives_up_at: deadline,
                },
            )
        })
        .collect()
}

fn playing_item(data: &Data, covered: &BTreeSet<String>, time: u64) -> Option<Item> {
    let mut games: BTreeMap<String, Vec<&crate::platform::Launch>> = BTreeMap::new();
    for launch in data.launches.values() {
        if launch_open(launch, time) && !covered.contains(&launch.app) {
            games.entry(launch.app.clone()).or_default().push(launch);
        }
    }
    if games.is_empty() {
        return None;
    }
    let parts: Vec<String> = games
        .iter()
        .map(|(app, launches)| {
            let names = launch_names(data, launches.iter().copied());
            let (subject, plural) = subject(&names);
            let verb = if plural { "have" } else { "has" };
            format!("{subject} {verb} \"{}\"", game_title(data, app))
        })
        .collect();
    let summary = if parts.len() == 1 {
        format!("{} open in the browser.", parts[0])
    } else {
        format!("{} in the browser.", list_names(&parts))
    };
    let effect = if games.len() == 1 {
        "The game keeps running. The site is unreachable for a few seconds while it restarts. The open tab checks in regularly, and that check-in does not hold the update."
    } else {
        "Those games keep running. The site is unreachable for a few seconds while it restarts. Open tabs check in regularly, and that check-in does not hold the update."
    };
    Some(Item {
        kind: "playing",
        holds: false,
        summary,
        effect: effect.into(),
        since: None,
        gives_up_at: None,
    })
}

fn voice_item(p: &Platform) -> Option<Item> {
    if !p.voice_active() {
        return None;
    }
    let names = p.voice_names();
    let summary = match names.as_slice() {
        [] => "Someone is in voice chat.".into(),
        [one] => format!("{one} is in voice chat."),
        _ => format!("{} are in voice chat.", list_names(&names)),
    };
    Some(Item {
        kind: "voice",
        holds: false,
        summary,
        effect: "Voice chat disconnects. They can tap the headset to join again. This does not hold the update.".into(),
        since: None,
        gives_up_at: None,
    })
}

fn stray_request(requests: &[LiveRequest], items: &[Item], time: u64) -> Option<Item> {
    let voice = items.iter().any(|item| item.kind == "voice");
    let peer = items.iter().any(|item| item.kind == "peer");
    let dedicated = items.iter().any(|item| item.kind == "dedicated");
    let mut screenshot = false;
    let mut download = false;
    let mut other = false;
    for request in requests {
        if request.publishing || time.saturating_sub(request.started) < LONG_REQUEST {
            continue;
        }
        let path = request.path.as_str();
        if path.is_empty()
            || path.contains("heartbeat")
            || path == "/api/operator/update"
            || path == "/api/client-state"
        {
            continue;
        }
        if voice && path.contains("/voice") {
            continue;
        }
        if peer && path.contains("peerjs") {
            continue;
        }
        if dedicated && path.contains("/connections/") {
            continue;
        }
        if path.contains("screenshot") {
            screenshot = true;
        } else if path.starts_with("/api/") || path.starts_with("/_infuser/") {
            other = true;
        } else {
            download = true;
        }
    }
    let (summary, effect) = if screenshot {
        (
            "A screenshot is still uploading.",
            "It may need to be taken again. This does not hold the update.",
        )
    } else if download {
        (
            "A game file is still downloading.",
            "It may need to be fetched again. This does not hold the update.",
        )
    } else if other {
        (
            "A page request is still running.",
            "It may need to be tried again. This does not hold the update.",
        )
    } else {
        return None;
    };
    Some(Item {
        kind: "request",
        holds: false,
        summary: summary.into(),
        effect: effect.into(),
        since: None,
        gives_up_at: None,
    })
}

fn maintenance_lease() -> Item {
    Item {
        kind: "maintenance",
        holds: true,
        summary: "A backup or another update is using the server.".into(),
        effect: "This update waits until that finishes. Update anyway cannot skip it.".into(),
        since: None,
        gives_up_at: None,
    }
}

fn launch_open(launch: &crate::platform::Launch, time: u64) -> bool {
    launch.expires > time && launch.seen.saturating_add(OPEN_SECONDS) > time
}

fn launch_names<'a>(
    data: &Data,
    launches: impl Iterator<Item = &'a crate::platform::Launch>,
) -> Vec<String> {
    let mut guests = 0usize;
    let mut names = BTreeSet::new();
    for launch in launches {
        if launch.guest {
            guests += 1;
            continue;
        }
        names.insert(
            data.members
                .get(&launch.email)
                .map(|member| member.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Someone".into()),
        );
    }
    let mut names: Vec<_> = names.into_iter().collect();
    if guests == 1 {
        names.push("a guest".into());
    } else if guests > 1 {
        names.push(format!("{guests} guests"));
    }
    names
}

fn subject(names: &[String]) -> (String, bool) {
    let plural = names.len() != 1;
    let mut text = list_names(names);
    if text.starts_with("a ") {
        text.replace_range(0..1, "A");
    }
    if text.is_empty() {
        ("Someone".into(), false)
    } else {
        (text, plural)
    }
}

fn list_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [one, two] => format!("{one} and {two}"),
        many => {
            let last = many.last().unwrap();
            format!("{}, and {last}", many[..many.len() - 1].join(", "))
        }
    }
}

fn game_title(data: &Data, app: &str) -> String {
    if let Some(title) = data
        .metadata
        .get(app)
        .and_then(|metadata| metadata.title.clone())
        .filter(|title| !title.is_empty())
    {
        return title;
    }
    if let Some(row) = data.apps.get(app)
        && let Some(build) = data.builds.get(&format!("{app}/{}", row.build))
        && !build.manifest.title.is_empty()
    {
        return build.manifest.title.clone();
    }
    app.to_owned()
}

fn publisher_app(path: &str) -> Option<&str> {
    let app = path
        .strip_prefix("/api/publisher/apps/")?
        .split('/')
        .next()
        .filter(|app| !app.is_empty())?;
    Some(app)
}

fn within(started: u64, limit: u64, time: u64) -> (bool, u64) {
    let deadline = started.saturating_add(limit);
    (time < deadline, deadline)
}

fn play_window(available_since: Option<u64>, time: u64) -> (bool, Option<u64>) {
    // No sliding clock: without a staged release there is nothing to defer,
    // and the ten minutes are measured from when that release first appeared.
    let Some(started) = available_since else {
        return (false, None);
    };
    let (holds, deadline) = within(started, PLAY_WAIT, time);
    (holds, Some(deadline))
}

fn finish(effect: &str, holds: bool, update_waiting: bool, deadline: u64, time: u64) -> String {
    if !update_waiting {
        return effect.to_owned();
    }
    let tail = if holds {
        anyway(deadline, time)
    } else {
        "The update is no longer waiting for this.".into()
    };
    format!("{effect} {tail}")
}

fn anyway(deadline: u64, time: u64) -> String {
    format!(
        "If you do nothing, the update installs anyway in {}.",
        age(deadline.saturating_sub(time))
    )
}

fn age(seconds: u64) -> String {
    if seconds < 60 {
        "less than a minute".into()
    } else if seconds < 3600 {
        format!("{} min", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} h {} min", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{} d {} h", seconds / 86_400, (seconds % 86_400) / 3600)
    }
}

/// Scheduled reboots and routine Ansible. This is the previous idle check,
/// named on its own: open games, shared games, reserved servers, publishing
/// or server builds (including a queued image), and any open request.
/// Voice is not a wait. A reboot still drops it; that was already true.
pub fn describe_strict(p: &Platform) -> Result<Vec<Item>> {
    let time = now();
    let data = p.db.read()?;
    let state = p.controller.snapshot();
    let mut items = Vec::new();
    if state.build_reservation.is_some()
        || data.server_builds.values().any(|job| job.active())
        || p.controller.maintenance.publishing() > 0
    {
        items.push(Item {
            kind: "packaging",
            holds: true,
            summary: "Publishing or a server image is still in progress.".into(),
            effect: "This waits until that work finishes. A reboot or configuration change would interrupt it.".into(),
            since: None,
            gives_up_at: None,
        });
    }
    for (app, game) in &state.games {
        if !matches!(
            game.phase,
            GamePhase::Starting | GamePhase::Ready | GamePhase::Stopping
        ) {
            continue;
        }
        let title = game_title(&data, app);
        items.push(Item {
            kind: "dedicated",
            holds: true,
            summary: format!("The server for \"{title}\" is still running."),
            effect: "This waits until that server is stopped. A reboot stops it.".into(),
            since: None,
            gives_up_at: None,
        });
    }
    if let Some(mut playing) = playing_item(&data, &BTreeSet::new(), time) {
        playing.holds = true;
        playing.effect = "This waits until that game is closed. A reboot stops play.".into();
        items.push(playing);
    }
    let diagnostics = p.peer_rooms().diagnostics(time);
    if diagnostics["sessions"]
        .as_array()
        .is_some_and(|sessions| !sessions.is_empty())
    {
        for named in peer_items(&data, &diagnostics, Some(time), time) {
            let mut item = named.item;
            item.holds = true;
            item.gives_up_at = None;
            item.effect = "This waits until that shared game ends. A reboot ends it.".into();
            items.push(item);
        }
    }
    // Ignore this status request and brief check-ins. A download that is still
    // transferring is what the strict check is actually waiting on.
    if p.controller.maintenance.requests().iter().any(|request| {
        !request.path.is_empty()
            && request.path != "/api/operator/update"
            && !request.path.contains("heartbeat")
            && time.saturating_sub(request.started) >= LONG_REQUEST
    }) {
        items.push(Item {
            kind: "request",
            holds: true,
            summary: "A page or download is still open.".into(),
            effect: "This waits until that request finishes.".into(),
            since: None,
            gives_up_at: None,
        });
    }
    if let Some(voice) = voice_item(p) {
        items.push(voice);
    }
    Ok(items)
}

#[derive(Serialize)]
pub struct HostPanel {
    pub step: HostStep,
    pub policy: HostPolicy,
    pub since: u64,
    pub summary: String,
    pub effect: String,
    pub activity: Vec<Item>,
}

pub fn host_panel(p: &Platform, record: &Record) -> Result<Option<HostPanel>> {
    let Some(wait) = record.host_wait.clone() else {
        return Ok(None);
    };
    let activity = match wait.policy {
        HostPolicy::Strict => describe_strict(p)?,
        HostPolicy::Release => describe_since(p, record.host_appeared.or(record.available_since))?,
    };
    let holding = activity.iter().any(|item| item.holds);
    let (summary, effect) = host_copy(wait.step, wait.policy, holding);
    Ok(Some(HostPanel {
        step: wait.step,
        policy: wait.policy,
        since: wait.since,
        summary,
        effect,
        activity,
    }))
}

fn host_copy(step: HostStep, policy: HostPolicy, holding: bool) -> (String, String) {
    let name = match step {
        HostStep::Reboot => "reboot",
        HostStep::Ansible => "configuration update",
    };
    match policy {
        HostPolicy::Strict => (
            if holding {
                format!("A {name} is waiting until play is idle.")
            } else {
                format!("A {name} was waiting. Nothing is open now.")
            },
            "This is scheduled maintenance, not part of delivering a new version. It waits for open games, shared games, game servers, and publishing. Update anyway does not apply. It tries again on the next maintenance window.".into(),
        ),
        HostPolicy::Release => (
            format!("A {name} is required for the new version to take effect."),
            if holding {
                "It uses the same waits as the update. Update anyway applies to it."
            } else {
                "Nothing is holding it now. It can run on its next try. Update anyway applies to it."
            }
            .into(),
        ),
    }
}

fn clip(message: &str) -> String {
    let mut end = message.len().min(500);
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        controller::Controller,
        docker::Docker,
        platform::{GameMetadata, Member},
    };
    use std::sync::Arc;

    fn platform() -> Platform {
        let root = tempfile::tempdir().unwrap();
        // Leak the temp dir for the process lifetime of this test helper's return.
        let path = root.keep();
        let cfg: Config = toml::from_str(&format!(
            "[controller]\nstate_dir={:?}",
            path.to_str().unwrap()
        ))
        .unwrap();
        let controller = Arc::new(
            Controller::open(
                cfg.clone(),
                Arc::new(Docker {
                    settings: cfg.controller.clone(),
                }),
            )
            .unwrap(),
        );
        Platform::open(controller).unwrap()
    }

    #[test]
    fn an_open_game_is_named_and_does_not_hold_activation() {
        let p = platform();
        p.db.edit(|data| {
            data.members.insert(
                "player@example.test".into(),
                Member {
                    name: "Alex".into(),
                    admin: false,
                    enabled: true,
                    avatar: None,
                    avatar_prompt_seen: false,
                },
            );
            data.metadata.insert(
                "pond".into(),
                GameMetadata {
                    title: Some("Pond".into()),
                    ..GameMetadata::default()
                },
            );
            data.launches.insert(
                "active".into(),
                serde_json::from_value(serde_json::json!({
                    "app":"pond","build":"one","email":"player@example.test","guest":false,
                    "host":"pond.example.test","token_hash":"fixture","exchanged":true,
                    "expires":now()+3600,"grant_expires":now()+3600,"seen":now(),
                    "lease_id":null,"lease_token":null
                }))?,
            );
            Ok(())
        })
        .unwrap();
        let items = describe(&p).unwrap();
        let playing = items.iter().find(|item| item.kind == "playing").unwrap();
        assert!(!playing.holds);
        assert!(playing.summary.contains("Alex"));
        assert!(playing.summary.contains("Pond"));
        assert!(playing.effect.contains("does not hold"));
        let decision = admission(&p).unwrap();
        assert!(decision.ready);
        assert!(!decision.interrupt_publishing);

        let _busy = p.controller.maintenance.enter().unwrap();
        assert!(
            admission(&p).unwrap().ready,
            "an in-flight page request is not a reason to wait"
        );
    }

    #[test]
    fn a_publisher_request_holds_until_its_own_limit() {
        let p = platform();
        p.controller
            .deployment
            .edit(|record| {
                record.available_since = Some(now().saturating_sub(3 * 60 * 60));
                Ok(())
            })
            .unwrap();
        let work = p
            .controller
            .maintenance
            .enter_kind(
                true,
                "/api/publisher/apps/rocket/builds/one/files/index.html",
            )
            .unwrap();
        let decision = admission(&p).unwrap();
        assert!(!decision.ready, "a live upload still waits");
        assert!(decision.waiting.contains("Receiving"));
        drop(work);
        assert!(admission(&p).unwrap().ready);
    }

    #[test]
    fn queued_server_work_does_not_hold_but_packaging_does() {
        let p = platform();
        let recipe = serde_json::from_value(serde_json::json!({
            "runtime":"node","node_major":22,"entry":"server.js"
        }))
        .unwrap();
        p.db.edit(|data| {
            let mut job = crate::server_package::Job::new(recipe);
            job.state = Phase::Queued;
            data.server_builds.insert("pond/one".into(), job);
            Ok(())
        })
        .unwrap();
        assert!(
            admission(&p).unwrap().ready,
            "a queued image survives restart"
        );
        p.db.edit(|data| {
            let job = data.server_builds.get_mut("pond/one").unwrap();
            job.state = Phase::Packaging;
            job.started = now();
            Ok(())
        })
        .unwrap();
        let decision = admission(&p).unwrap();
        assert!(!decision.ready);
        assert!(decision.waiting.contains("Publishing"));
        p.db.edit(|data| {
            data.server_builds.get_mut("pond/one").unwrap().started =
                now().saturating_sub(PUBLISH_WAIT + 5);
            Ok(())
        })
        .unwrap();
        let decision = admission(&p).unwrap();
        assert!(decision.ready);
        assert!(decision.interrupt_publishing);
    }
}
