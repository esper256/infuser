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
    activity: presence::LibraryActivity,
}
impl View<'_> {
    fn includes(&self, id: &str) -> bool {
        groups::visible(
            self.data,
            self.email,
            id,
            self.group,
            self.inspecting,
            self.time,
        ) && self.data.deployment_for(id, self.group).is_some()
    }

    fn card(&self, p: &Platform, id: &str, app: &App) -> Value {
        let data = self.data;
        let listed = app.listed;
        let runtime = data.deployment_for(id, self.group).unwrap_or(id);
        let app = data.deployed(runtime).unwrap_or(app);
        let build = &data.builds[&build_key(id, &app.build)];
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
                    "screenshot":reaction.screenshot,
                })
            })
            .collect();
        p.peers.retain_launches(
            runtime,
            &presence::live_launches(data, runtime, self.time),
            self.time,
        );
        let rooms = p.peers.discover_all(runtime, &app.build, self.time);
        let desktop = native::bundle(p, id, &app.build, build).ok().flatten();
        let supports_join = build
            .manifest
            .launch_intents
            .contains(&crate::launch_intent::Kind::JoinFriend)
            && desktop.as_ref().is_none_or(|m| !m.native_only);
        let dedicated = if build.manifest.dedicated_sessions() {
            dedicated::live_sessions(p, data, runtime, &app.build, self.time)
        } else {
            vec![]
        };
        let mut join_targets = vec![];
        let mut membership = BTreeMap::new();
        let viewer = data.player_id(self.email);
        for session in rooms.iter().chain(&dedicated) {
            let Some(members) = session["players"].as_array() else {
                continue;
            };
            let solo = session["activity"] == "solo";
            let open = !solo && session["joinable"] == true && supports_join;
            let mode = if solo { "single_player" } else { "multiplayer" };
            let reason = if solo || open {
                None
            } else if !supports_join {
                Some("not_supported")
            } else {
                Some(session["reason"].as_str().unwrap_or("closed"))
            };
            for member in members {
                let player = member["player_id"].as_str().unwrap();
                // One profile may occupy several runs. Prefer one a friend can join.
                let entry = membership.entry(player).or_insert((mode, reason));
                if open || (entry.0 == "single_player" && !solo) {
                    *entry = (mode, reason);
                }
                if open && !self.inspecting && player != viewer {
                    join_targets.push(json!({"session_id":session["id"],"player_id":player,"name":member["name"],"guest":member["guest"]}));
                }
            }
        }
        let players: Vec<_> = data
            .launches
            .iter()
            .filter(|(_, launch)| {
                !self.activity.closed_hosts.contains(&launch.host)
                    && launch.runtime_id() == runtime
                    && launch.exchanged
                    && launch.expires > self.time
                    && launch.seen + 90 > self.time
                    && data.launch_allowed(launch, self.time)
                    && data.profile_enabled(&launch.email)
            })
            .map(|(launch_id, launch)| {
                let player = if launch.guest {
                    format!("guest-{launch_id}")
                } else {
                    data.player_id(&launch.email)
                };
                let played_build = data.builds.get(&build_key(id, &launch.build));
                let reported = self
                    .activity
                    .modes
                    .get(launch_id)
                    .map(String::as_str)
                    .unwrap_or_else(|| {
                        if played_build.is_some_and(|b| {
                            b.manifest.play_modes == [crate::manifest::PlayMode::Solo]
                        }) {
                            "single_player"
                        } else {
                            "game_open"
                        }
                    });
                let (activity, reason) = if launch.build != app.build {
                    (reported, Some("different_build"))
                } else {
                    membership
                        .get(player.as_str())
                        .copied()
                        .unwrap_or((reported, None))
                };
                json!({
                    "activity":activity,"join_reason":reason,
                    "id":data.player_id(&launch.email),
                    "player_id":player,
                    "name":data.members[&launch.email].name,
                    "avatar":data.members[&launch.email].avatar,
                    "guest":launch.guest,
                })
            })
            .collect();
        // Room leases can outlive the departing tab. Suppress its stale Join
        // targets while preserving other active players' normal lease behavior.
        if !self.activity.closed_hosts.is_empty() && !join_targets.is_empty() {
            let departed: std::collections::BTreeSet<_> = data
                .launches
                .iter()
                .filter(|(_, l)| {
                    l.runtime_id() == runtime && self.activity.closed_hosts.contains(&l.host)
                })
                .map(|(id, l)| {
                    if l.guest {
                        format!("guest-{id}")
                    } else {
                        data.player_id(&l.email)
                    }
                })
                .collect();
            join_targets.retain(|target| {
                !departed.contains(target["player_id"].as_str().unwrap())
                    || players
                        .iter()
                        .any(|player| player["player_id"] == target["player_id"])
            });
        }
        let authors = authors::credits(data, id);
        let card_art_url = card_art::url(id, &app.build, build, data.metadata.get(id), &p.root())
            .map(|url| {
                if self.inspecting {
                    format!("{url}?inspect_group={}", self.group)
                } else if data.groups.member(self.email) != self.group {
                    data.active_visit(self.group, self.time)
                        .map_or(url.clone(), |(id, _)| format!("{url}?visit={id}"))
                } else {
                    url
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
            "update_id":app.update_id(runtime),
            "borrowed":runtime!=id,
            "play_unavailable":if runtime!=id || data.groups.member(self.email)!=self.group {build.visit_unavailable()}else{None},
            "title":metadata::title(data,id,&build.manifest.title),
            "orientation":build.manifest.orientation,
            "description":data.metadata.get(id).and_then(|m|m.description.as_deref()),
            "author":authors.first(),
            "authors":authors,
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
                    && self.data.profile_enabled(email)
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
                json!({"id":id,"name":member.name,"avatar":member.avatar,"status":presence::input_status(self.activity.inputs.get(email).copied(),self.time),"join_targets":targets})
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
        activity: presence::LibraryActivity::read(&p.db)?,
    };
    let mut apps: Vec<_> = data
        .apps
        .iter()
        .filter(|(id, _)| view.includes(id))
        .map(|(id, app)| view.card(&p, id, app))
        .collect();
    update_reactions::add_summaries(&p.db, &data, &email, &mut apps)?;
    let friends = view.friends(&apps);
    // Platform releases are not a group's game. Every signed-in group sees them.
    let platform_updates = crate::platform::releases::feed_json(
        &p.db
            .platform_feed(&email, |profile| data.profile_enabled(profile))?,
    );
    Ok(conditional::json(
        json!({
            "apps":apps,
            "friends":friends,
            "platform_updates":platform_updates,
            "visit":data.active_visit(&group,time).map(|(id,v)|json!({"id":id,"name":v.name,"expires_at":v.expires,"visitors":v.participants.iter().filter(|(e,p)|!p.ended&&data.profile_enabled(e)).map(|(e,_)|json!({"id":data.player_id(e),"name":data.members[e].name})).collect::<Vec<_>>()})),
            "tags":crate::tags::catalog_for_apps(&data,apps.iter().filter_map(|a|a["id"].as_str())),
        }),
        &h,
    ))
}
