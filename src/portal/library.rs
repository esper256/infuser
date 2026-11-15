//! The Library response is shared by the website and operator inspection.
//! Keep its group filtering and public fields together; never include credentials.
use super::*;

struct View<'a> {
    data: &'a platform::Data,
    email: &'a str,
    group: &'a str,
    inspecting: bool,
    time: u64,
}
impl View<'_> {
    fn includes(&self, id: &str, app: &App) -> bool {
        self.data.groups.app(id) == self.group
            && (app.listed || self.inspecting || self.data.members[self.email].admin)
    }

    fn card(&self, p: &Platform, id: &str, app: &App) -> Value {
        let data = self.data;
        let build = &data.builds[&build_key(id, &app.build)];
        let players: Vec<_> = data
            .launches
            .values()
            .filter(|launch| {
                launch.app == id
                    && launch.seen + 90 > self.time
                    && data.groups.allows(&launch.email, id)
                    && data
                        .members
                        .get(&launch.email)
                        .is_some_and(|member| member.enabled)
            })
            .map(|launch| {
                json!({
                    "id":hash(&launch.email),
                    "name":data.members[&launch.email].name,
                    "guest":launch.guest,
                })
            })
            .collect();
        let reactions: Vec<_> = data
            .reactions
            .get(id)
            .into_iter()
            .flat_map(|r| r.iter())
            .filter(|(email, _)| {
                data.groups.member(email) == self.group
                    && data
                        .members
                        .get(*email)
                        .is_some_and(|member| member.enabled)
            })
            .map(|(email, reaction)| {
                json!({
                    "name":data.members[email].name,
                    "mine":email == self.email,
                    "emoji":reaction.emoji,
                    "text":reaction.text,
                    "updated":reaction.updated,
                })
            })
            .collect();
        p.peers
            .retain_launches(id, &presence::live_launches(data, id, self.time), self.time);
        let room = p.peers.discovery(id, &app.build, self.time);
        let join_targets: Vec<_> = if !self.inspecting
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
                .filter(|player| player["player_id"] != hash(self.email))
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
        let desktop = native::bundle(p, id, &app.build, build).ok().flatten();
        let author = data.owners.get(id).and_then(|owner| {
            data.members
                .get(owner)
                .map(|member| json!({"id":hash(owner),"name":member.name,"avatar":member.avatar}))
        });
        let card_art_url = card_art::url(id, &app.build, build, data.metadata.get(id)).map(|url| {
            if self.inspecting {
                format!("{url}?inspect_group={}", self.group)
            } else {
                url
            }
        });
        json!({
            "id":id,
            "title":metadata::title(data,id,&build.manifest.title),
            "description":data.metadata.get(id).and_then(|m|m.description.as_deref()),
            "author":author,
            "slug":data.game_urls.get(id).map(|u|&u.slug),
            "url":data.game_urls.get(id).map(|u|format!("https://{}/games/{}",p.domain(),u.slug)),
            "card_art_url":card_art_url,
            "native_platforms":desktop.as_ref().map(|m|m.platforms.keys().collect::<Vec<_>>()).unwrap_or_default(),
            "native_only":desktop.as_ref().is_some_and(|m|m.native_only),
            "play_modes":build.manifest.play_modes,
            "multiplayer":build.manifest.multiplayer,
            "build":app.build,
            "updated":app.updated,
            "preview":!app.listed,
            "players":players,
            "reactions":reactions,
            "join_targets":join_targets,
            "tags":data.tags.get(id).cloned().unwrap_or_default(),
            "can_edit_tags":!self.inspecting && (data.members[self.email].admin || data.owners.get(id).is_some_and(|owner|owner==self.email)),
        })
    }

    fn friends(&self, apps: &[Value]) -> Vec<Value> {
        self.data
            .presence
            .iter()
            .filter(|(email, seen)| {
                **seen + 90 > self.time
                    && self.data.groups.member(email) == self.group
                    && self
                        .data
                        .members
                        .get(*email)
                        .is_some_and(|member| member.enabled)
            })
            .map(|(email, _)| {
                let id = hash(email);
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
    p.db.cleanup()?;
    let time = now();
    let data = p.db.edit(|data| {
        if !inspecting {
            data.presence.insert(email.clone(), time);
        }
        Ok(data.clone())
    })?;
    let view = View {
        data: &data,
        email: &email,
        group: &group,
        inspecting,
        time,
    };
    let apps: Vec<_> = data
        .apps
        .iter()
        .filter(|(id, app)| view.includes(id, app))
        .map(|(id, app)| view.card(&p, id, app))
        .collect();
    let friends = view.friends(&apps);
    Ok(reply(json!({
        "apps":apps,
        "friends":friends,
        "tags":crate::tags::catalog_in_group(&data,data.members[&email].admin||inspecting,&group),
    })))
}
