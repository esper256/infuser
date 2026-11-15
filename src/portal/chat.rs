use super::*;

pub(super) async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    Ok(reply(p.db.chat_list_group(&email, &group, inspecting)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    client_id: String,
    text: String,
}

pub(super) async fn send(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Message>,
) -> Answer {
    let email = p.member(&h, true).await?;
    Ok(reply(p.db.chat_send(
        &email,
        &body.client_id,
        &body.text,
    )?))
}

pub(super) async fn remove(State(p): State<Platform>, Path(id): Path<u64>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    p.db.chat_remove(&email, id)?;
    Ok(reply(json!({"ok":true})))
}
