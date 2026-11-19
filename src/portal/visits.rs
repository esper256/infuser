use super::*;
use crate::platform::visits::{Participant, Visit};
use std::collections::BTreeSet;

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route("/api/visits", get(mine))
        .route("/api/operator/visits", get(list).post(create))
        .route("/api/operator/visits/{id}", put(extend).delete(end))
        .route(
            "/api/operator/visits/{id}/participants/{player}",
            delete(remove),
        )
        .layer(DefaultBodyLimit::max(16384))
}
fn view(d: &platform::Data, id: &str, v: &Visit) -> Value {
    json!({"id":id,"name":v.name,"host_group":v.host,"expires_at":v.expires,"active":v.active(now()),"games":v.games,
        "participants":v.participants.iter().map(|(e,p)|json!({"id":d.player_id(e),"name":d.members.get(e).map(|m|&m.name),"ended":p.ended})).collect::<Vec<_>>()})
}
async fn mine(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let d = p.db.catalog()?;
    Ok(reply(
        json!({"visits":d.visits.iter().filter(|(id,_)|d.visit_access(id,&email,now()).is_some()).map(|(id,v)|view(&d,id,v)).collect::<Vec<_>>()}),
    ))
}
async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.operator(&h, false).await?;
    let d = p.db.catalog()?;
    Ok(reply(
        json!({"enabled":p.config().controller.visits,"groups":d.groups.names,
        "members":d.members.iter().filter(|(e,_)|d.profile_enabled(e)).map(|(e,m)|json!({"id":d.player_id(e),"name":m.name,"group":d.groups.member(e)})).collect::<Vec<_>>(),
        "games":d.apps.iter().filter(|(_,a)|a.listed).map(|(id,a)|json!({"id":id,"group":d.groups.app(id),"title":metadata::title(&d,id,&d.builds[&build_key(id,&a.build)].manifest.title)})).collect::<Vec<_>>(),
        "visits":d.visits.iter().map(|(id,v)|view(&d,id,v)).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    name: String,
    host_group: String,
    participants: Vec<String>,
    games: Vec<String>,
    expires_at: u64,
}
async fn create(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Create>) -> Answer {
    p.operator(&h, true).await?;
    ensure!(
        p.config().controller.visits && p.config().controller.private_groups,
        fault(409, "Enable controller.visits and private_groups first.")
    );
    let time = now();
    ensure!(
        body.expires_at > time && body.expires_at <= time + 7 * 86400,
        fault(422, "Choose an end time within seven days.")
    );
    ensure!(
        !body.name.trim().is_empty()
            && body.name.chars().count() <= 60
            && !body.name.chars().any(char::is_control),
        fault(422, "Use a visit name of 1–60 characters.")
    );
    ensure!(
        !body.participants.is_empty() && body.participants.len() <= 64 && body.games.len() <= 128,
        fault(422, "Select 1–64 visitors and at most 128 games.")
    );
    let id = secret()[..32].to_owned();
    let guard = p.publishing.lock().unwrap();
    p.db.edit_transaction(|d, tx| {
        ensure!(
            d.groups.names.contains_key(&body.host_group),
            fault(404, "Host group not found.")
        );
        ensure!(
            d.active_visit(&body.host_group, time).is_none(),
            fault(409, "This group already has an active visit.")
        );
        let after = crate::platform::channels::admission_boundary(tx, &body.host_group)?;
        let mut participants = BTreeMap::new();
        let mut source_groups = BTreeSet::new();
        for player in &body.participants {
            let (email, _) = d
                .members
                .iter()
                .find(|(e, _)| d.profile_enabled(e) && d.player_id(e) == *player)
                .ok_or_else(|| fault(404, "Visitor not found."))?;
            ensure!(
                d.groups.member(email) != body.host_group,
                fault(422, "Select visitors from another group.")
            );
            ensure!(
                !d.visits
                    .values()
                    .any(|v| v.active(time) && v.participant(email)),
                fault(409, "A selected visitor is already visiting.")
            );
            ensure!(
                participants
                    .insert(
                        email.clone(),
                        Participant {
                            after_message: after,
                            ended: false
                        }
                    )
                    .is_none(),
                fault(422, "Duplicate visitor.")
            );
            source_groups.insert(d.groups.member(email).to_owned());
        }
        let games: BTreeSet<_> = body.games.iter().cloned().collect();
        for app in &games {
            ensure!(
                source_groups.contains(d.groups.app(app))
                    && d.apps.get(app).is_some_and(|a| a.listed),
                fault(
                    422,
                    "Bring a published game from a selected visitor's group."
                )
            );
            d.ensure_deployment(app, &body.host_group)?;
        }
        d.visits.insert(
            id.clone(),
            Visit {
                name: body.name.trim().into(),
                host: body.host_group,
                participants,
                games,
                expires: body.expires_at,
                ended: false,
            },
        );
        Ok(())
    })?;
    drop(guard);
    p.reconcile_publications()?;
    Ok(reply(json!({"id":id})))
}
async fn end(State(p): State<Platform>, Path(id): Path<String>, h: HeaderMap) -> Answer {
    p.operator(&h, true).await?;
    p.db.edit(|d| {
        d.visits
            .get_mut(&id)
            .ok_or_else(|| fault(404, "Visit not found."))?
            .ended = true;
        Ok(())
    })?;
    p.expire_visits()?;
    Ok(reply(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extend {
    expires_at: u64,
}
async fn extend(
    State(p): State<Platform>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(body): Json<Extend>,
) -> Answer {
    p.operator(&h, true).await?;
    p.db.edit(|d| {
        let v = d
            .visits
            .get_mut(&id)
            .ok_or_else(|| fault(404, "Visit not found."))?;
        ensure!(
            v.active(now()) && body.expires_at > now() && body.expires_at <= now() + 7 * 86400,
            fault(409, "Extend an active visit to a time within seven days.")
        );
        v.expires = body.expires_at;
        Ok(())
    })?;
    Ok(reply(json!({"ok":true})))
}
async fn remove(
    State(p): State<Platform>,
    Path((id, player)): Path<(String, String)>,
    h: HeaderMap,
) -> Answer {
    p.operator(&h, true).await?;
    p.db.edit(|d| {
        let v = d
            .visits
            .get_mut(&id)
            .ok_or_else(|| fault(404, "Visit not found."))?;
        let (_, member) = v
            .participants
            .iter_mut()
            .find(|(email, _)| {
                d.identities
                    .profiles
                    .get(*email)
                    .is_some_and(|p| p.id == player)
            })
            .ok_or_else(|| fault(404, "Visitor not found."))?;
        member.ended = true;
        if v.participants.values().all(|p| p.ended) {
            v.ended = true;
        }
        Ok(())
    })?;
    p.expire_visits()?;
    Ok(reply(json!({"ok":true})))
}
impl Platform {
    pub(super) fn sync_deployments(&self) -> Result<()> {
        let _guard = self.publishing.lock().unwrap();
        let state = self.controller.snapshot();
        let data = self.db.read()?;
        let missing = data.visits.values().filter(|v| v.active(now())).any(|v| {
            v.games
                .iter()
                .any(|a| data.apps.contains_key(a) && data.deployment_for(a, &v.host).is_none())
        });
        let changed = data.deployments.iter().any(|(id, dep)| {
            data.apps.get(&dep.app).is_some_and(|a| {
                a.build != dep.active.build
                    && (data
                        .pending_promotions
                        .get(id)
                        .is_none_or(|q| q.build != a.build)
                        || (!state.games.get(id).is_some_and(|i| i.reserved())
                            && !self.peers.active(id, now())))
            })
        });
        if !missing && !changed {
            return Ok(());
        }
        self.db.edit(|d| {
            let pairs: Vec<_> = d
                .visits
                .values()
                .filter(|v| v.active(now()))
                .flat_map(|v| v.games.iter().map(|a| (a.clone(), v.host.clone())))
                .collect();
            for (app, group) in pairs {
                if d.apps.contains_key(&app) {
                    d.ensure_deployment(&app, &group)?;
                }
            }
            for (id, dep) in &mut d.deployments {
                let Some(latest) = d.apps.get(&dep.app) else {
                    continue;
                };
                if dep.active.build == latest.build {
                    continue;
                }
                if state.games.get(id).is_some_and(|i| i.reserved()) || self.peers.active(id, now())
                {
                    d.pending_promotions.insert(
                        id.clone(),
                        platform::PendingPromotion {
                            build: latest.build.clone(),
                            listed: latest.listed,
                            queued_at: now(),
                        },
                    );
                } else {
                    dep.active = latest.clone();
                    d.pending_promotions.remove(id);
                }
            }
            Ok(())
        })
    }
    pub(super) fn expire_visits(&self) -> Result<()> {
        let _guard = self.publishing.lock().unwrap();
        let d = self.db.read()?;
        for (id, l) in &d.launches {
            if l.visit.is_some() && !d.launch_allowed(l, now()) {
                self.peers.leave_launch(l.runtime_id(), id);
                if let (Some(lease), Some(token)) = (&l.lease_id, &l.lease_token) {
                    let _ = self
                        .controller
                        .lease(l.runtime_id(), lease, token, "delete", now());
                }
            }
        }
        for (id, dep) in &d.deployments {
            if (!d.apps.get(&dep.app).is_some_and(|a| a.listed)
                || !d
                    .active_visit(&dep.group, now())
                    .is_some_and(|(_, v)| v.games.contains(&dep.app)))
                && self
                    .controller
                    .snapshot()
                    .games
                    .get(id)
                    .is_some_and(|i| i.reserved())
            {
                self.controller.stop(id)?;
            }
        }
        Ok(())
    }
}
