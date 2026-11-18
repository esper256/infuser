//! Display authorship is independent of the publishing owner and credentials.
use super::*;

pub(super) fn credits(data: &platform::Data, app: &str) -> Vec<Value> {
    let keys: Vec<_> = data.author_credits.get(app).map_or_else(
        || data.owners.get(app).into_iter().collect(),
        |authors| authors.iter().collect(),
    );
    keys.into_iter()
        .filter_map(|key| {
            data.members.get(key).map(|member| {
                json!({
                    "id":data.player_id(key),"name":member.name,"avatar":member.avatar
                })
            })
        })
        .collect()
}

pub(super) async fn inventory(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let actor = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    let data = p.db.read()?;
    let games: Vec<_> = data.apps.iter().filter(|(id, _)| data.groups.app(id) == group)
        .map(|(id, app)| json!({"id":id,
            "title":metadata::title(&data,id,&data.builds[&build_key(id,&app.build)].manifest.title),
            "authors":credits(&data,id)})).collect();
    let members: Vec<_> = data.members.iter().filter(|(key, _)| data.groups.member(key) == group)
        .map(|(key, member)| json!({"id":data.player_id(key),"name":member.name,"avatar":member.avatar})).collect();
    Ok(reply(json!({"games":games,"members":members})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Authors {
    authors: Vec<String>,
}

pub(super) async fn set(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
    Json(body): Json<Authors>,
) -> Answer {
    let actor = p.admin(&h, true).await?;
    let authors = p.db.edit(|data| {
        data.require_app_group(&actor, &app)?;
        ensure!(data.apps.contains_key(&app), fault(404, "Game not found."));
        ensure!(
            !body.authors.is_empty(),
            fault(400, "Choose at least one author.")
        );
        let mut keys = Vec::new();
        for id in &body.authors {
            let key = data
                .members
                .keys()
                .find(|key| {
                    data.groups.member(key) == data.groups.member(&actor)
                        && data.player_id(key) == *id
                })
                .ok_or_else(|| fault(400, "Choose authors from this library."))?;
            ensure!(
                !keys.contains(key),
                fault(400, "Each author can appear only once.")
            );
            keys.push(key.clone());
        }
        data.author_credits.insert(app.clone(), keys);
        Ok(credits(data, &app))
    })?;
    Ok(reply(json!({"app_id":app,"authors":authors})))
}
