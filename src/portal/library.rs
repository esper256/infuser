//! The Library response is shared by the website and operator inspection.
//! Keep its group filtering and public fields together; never include credentials.
use super::*;

struct View<'a> {
    data: &'a platform::Data,
    email: &'a str,
    group: &'a str,
    inspecting: bool,
    moderates_reactions: bool,
    time: u64,
}
impl View<'_> {
    fn includes(&self, id: &str, app: &App) -> bool {
        (self.data.groups.app(id) == self.group
            || self
                .data
                .active_visit(self.group, self.time)
                .is_some_and(|(_, v)| v.games.contains(id)))
            && self.data.deployment_for(id, self.group).is_some()
            && (app.listed
                || self.inspecting
                || (self.data.groups.app(id) == self.group
                    && self.data.groups.member(self.email) == self.group
                    && self.data.members[self.email].admin))
    }

    fn card(&self, p: &Platform, id: &str, app: &App) -> Value {
        let data = self.data;
        let listed = app.listed;
        let runtime = data.deployment_for(id, self.group).unwrap_or(id);
        let app = data.deployed(runtime).unwrap_or(app);
        let build = &data.builds[&build_key(id, &app.build)];
        let players: Vec<_> = data
            .launches
            .iter()
            .filter(|(_, launch)| {
                launch.runtime_id() == runtime
                    && launch.seen + 90 > self.time
                    && data.launch_allowed(launch, self.time)
                    && data
                        .members
                        .get(&launch.email)
                        .is_some_and(|member| member.enabled)
            })
            .map(|(launch_id, launch)| {
                json!({
                    "id":data.player_id(&launch.email),
                    "player_id":if launch.guest {format!("guest-{launch_id}")} else {data.player_id(&launch.email)},
                    "name":data.members[&launch.email].name,
                    "avatar":data.members[&launch.email].avatar,
                    "guest":launch.guest,
                })
            })
            .collect();
        let reactions: Vec<_> = data
            .reactions
            .get(runtime)
            .into_iter()
            .flat_map(|r| r.iter())
            .filter(|(email, _)| {
                data.members
                    .get(*email)
                    .is_some_and(|member| member.enabled)
            })
            .map(|(email, reaction)| {
                json!({
                    "id":data.player_id(email),
                    "name":data.members[email].name,
                    "avatar":data.members[email].avatar,
                    "mine":email == self.email,
                    "emoji":reaction.emoji,
                    "text":reaction.text,
                    "updated":reaction.updated,
                })
            })
            .collect();
        p.peers.retain_launches(
            runtime,
            &presence::live_launches(data, runtime, self.time),
            self.time,
        );
        let room = p.peers.discovery(runtime, &app.build, self.time);
        let mut join_targets: Vec<_> = if !self.inspecting
            && build
                .manifest
                .launch_intents
                .contains(&crate::launch_intent::Kind::JoinFriend)
            && room["joinable"] == true
        {
            room["players"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|player| player["player_id"] != data.player_id(self.email))
                .map(|player| {
                    json!({
                        "session_id":room["id"],
                        "player_id":player["player_id"],
                        "name":player["name"],
                        "guest":player["guest"],
                    })
                })
                .collect()
        } else {
            vec![]
        };
        if !self.inspecting
            && build.manifest.dedicated_sessions()
            && build
                .manifest
                .launch_intents
                .contains(&crate::launch_intent::Kind::JoinFriend)
        {
            for session in dedicated::live_sessions(p, data, runtime, &app.build, self.time) {
                if session["joinable"] != true || session["activity"] != "multiplayer" {
                    continue;
                }
                for player in session["players"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|player| player["player_id"] != data.player_id(self.email))
                {
                    join_targets.push(json!({"session_id":session["id"],"player_id":player["player_id"],"name":player["name"],"guest":player["guest"]}));
                }
            }
        }
        let desktop = native::bundle(p, id, &app.build, build).ok().flatten();
        let author = data.owners.get(id).and_then(|owner| {
            data.members
                .get(owner)
                .map(|member| json!({"id":data.player_id(owner),"name":member.name,"avatar":member.avatar}))
        });
        let card_art_url = card_art::url(id, &app.build, build, data.metadata.get(id))
            .map(|url| {
                if self.inspecting {
                    format!("{url}?inspect_group={}", self.group)
                } else {
                    if data.groups.member(self.email) != self.group {
                        data.active_visit(self.group, self.time)
                            .map_or(url.clone(), |(id, _)| format!("{url}?visit={id}"))
                    } else {
                        url
                    }
                }
            })
            .map(|url| {
                let profile = &data.identities.profiles[self.email];
                if data
                    .identities
                    .profiles
                    .values()
                    .any(|p| p.account_id == profile.account_id && p.id != profile.id)
                {
                    format!(
                        "{url}{}profile={}",
                        if url.contains('?') { "&" } else { "?" },
                        profile.id
                    )
                } else {
                    url
                }
            });
        json!({
            "id":id,
            "deployment_id":runtime,
            "borrowed":runtime!=id,
            "play_unavailable":if runtime!=id || data.groups.member(self.email)!=self.group {build.visit_unavailable()}else{None},
            "title":metadata::title(data,id,&build.manifest.title),
            "description":data.metadata.get(id).and_then(|m|m.description.as_deref()),
            "author":author,
            "slug":data.game_urls.get(id).map(|u|&u.slug),
            "url":if runtime!=id {Some(format!("https://{}/library?app={id}",p.domain()))}else{data.game_urls.get(id).map(|u|format!("https://{}/games/{}",p.domain(),u.slug))},
            "card_art_pending":card_art_url.is_none() && metadata::art_pending(data,id),
            "card_art_url":card_art_url,
            "native_platforms":desktop.as_ref().map(|m|m.platforms.keys().collect::<Vec<_>>()).unwrap_or_default(),
            "native_only":desktop.as_ref().is_some_and(|m|m.native_only),
            "play_modes":build.manifest.play_modes,
            "multiplayer":build.manifest.multiplayer,
            "build":app.build,
            "updated":app.updated,
            "preview":!listed,
            "players":players,
            "reactions":reactions,
            "can_moderate_reactions":self.moderates_reactions,
            "can_manage_visibility":self.moderates_reactions && runtime == id && data.groups.allows(self.email,id),
            "join_targets":join_targets,
            "tags":data.tags.get(id).cloned().unwrap_or_default(),
            "can_edit_tags":!self.inspecting && data.groups.allows(self.email,id) && (data.members[self.email].admin || data.owners.get(id).is_some_and(|owner|owner==self.email)),
        })
    }

    fn friends(&self, apps: &[Value]) -> Vec<Value> {
        self.data
            .presence
            .iter()
            .filter(|(email, seen)| {
                **seen + 90 > self.time
                    && self.data.group_participant(email, self.group, self.time)
                    && self
                        .data
                        .members
                        .get(*email)
                        .is_some_and(|member| member.enabled)
            })
            .map(|(email, _)| {
                let id = self.data.player_id(email);
                let targets: Vec<_> = apps
                    .iter()
                    .flat_map(|app| {
                        app["join_targets"].as_array().unwrap().iter()
                        .filter(|target| target["player_id"] == id)
                        .map(|target| json!({
                            "app_id":app["id"],"title":app["title"],
                            "session_id":target["session_id"],"player_id":target["player_id"],
                        }))
                    })
                    .collect();
                let member = &self.data.members[email];
                json!({"id":id,"name":member.name,"avatar":member.avatar,"join_targets":targets})
            })
            .collect()
    }
}

pub(super) async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    if !inspecting {
        p.admit(&h).await;
    }
    let time = now();
    if !inspecting {
        p.db.see_member(&email, time)?;
    }
    let data = p.db.read()?;
    let view = View {
        data: &data,
        email: &email,
        group: &group,
        inspecting,
        moderates_reactions: !inspecting
            && !h.contains_key(groups::VISIT)
            && data.members[&email].admin,
        time,
    };
    let apps: Vec<_> = data
        .apps
        .iter()
        .filter(|(id, app)| view.includes(id, app))
        .map(|(id, app)| view.card(&p, id, app))
        .collect();
    let friends = view.friends(&apps);
    Ok(conditional::json(
        json!({
            "apps":apps,
            "friends":friends,
            "visit":data.active_visit(&group,time).map(|(id,v)|json!({"id":id,"name":v.name,"expires_at":v.expires,"visitors":v.participants.iter().filter(|(e,p)|!p.ended&&data.members.get(*e).is_some_and(|m|m.enabled)).map(|(e,_)|json!({"id":data.player_id(e),"name":data.members[e].name})).collect::<Vec<_>>()})),
            "tags":crate::tags::catalog_for_apps(&data,apps.iter().filter_map(|a|a["id"].as_str())),
        }),
        &h,
    ))
}
