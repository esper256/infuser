use super::*;

pub(super) async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &email)?;
    Ok(conditional::json(
        p.db.chat_list_group(&email, &group, inspecting)?,
        &h,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    client_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    screenshot: Option<String>,
    #[serde(default)]
    house_call: Option<String>,
}

pub(super) async fn send(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Message>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let channel = p.db.default_chat_channel(&group)?;
    Ok(reply(p.db.chat_send_with_house_call(
        &email,
        &channel,
        &body.client_id,
        &body.text,
        body.screenshot.as_deref(),
        body.house_call.as_deref(),
    )?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
    message: u64,
}

pub(super) async fn advance(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Read>,
) -> Answer {
    let email = p.member(&h, true).await?;
    let (group, _) = p.group_view(&h, &email)?;
    Ok(reply(p.db.chat_advance_read(
        &email,
        &group,
        body.message,
    )?))
}

pub(super) async fn remove(State(p): State<Platform>, Path(id): Path<u64>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    let (group, _) = p.group_view(&h, &email)?;
    p.db.chat_remove_group(&email, &group, id)?;
    Ok(reply(json!({"ok":true})))
}

/// Chat stays in a Library-origin frame, outside the uploaded game's DOM.
pub(super) async fn overlay(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.main_host(&h)?;
    let policy = format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors https://*.{}; base-uri 'none'",
        p.domain()
    );
    Ok((
        [
            ("Content-Security-Policy", policy.as_str()),
            ("Cross-Origin-Embedder-Policy", "require-corp"),
            ("Cross-Origin-Resource-Policy", "same-site"),
            ("Referrer-Policy", "no-referrer"),
            ("Cache-Control", "no-store"),
        ],
        Html(include_str!("../../web/game-chat.html")),
    )
        .into_response())
}

#[derive(Deserialize)]
pub(super) struct Cursor {
    after: Option<u64>,
}

pub(super) async fn previews(
    State(p): State<Platform>,
    Path(launch): Path<String>,
    Query(cursor): Query<Cursor>,
    h: HeaderMap,
) -> Answer {
    let email = p.member(&h, false).await?;
    Ok(reply(p.db.game_chat_previews(
        &launch,
        &email,
        cursor.after,
    )?))
}
