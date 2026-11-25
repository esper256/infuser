use super::*;
use std::time::Duration;

pub(super) async fn available(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let actor = p.member(&h, false).await?;
    let (group, inspecting) = p.group_view(&h, &actor)?;
    Ok(reply(if inspecting {
        json!({"images":[]})
    } else {
        p.db.house_calls_available(&actor, &group)?
    }))
}
pub(super) async fn settings(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let actor = p.admin(&h, false).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    Ok(reply(p.db.house_calls_admin(&actor, &group)?))
}
pub(super) async fn save(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(settings): Json<platform::house_calls::Settings>,
) -> Answer {
    let actor = p.admin(&h, true).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    p.db.house_calls_set(&actor, &group, settings)?;
    Ok(reply(json!({"ok":true})))
}
pub(super) async fn image(Path(file): Path<String>, h: HeaderMap) -> Answer {
    let bytes: &'static [u8] = match file.as_str() {
        "confetti-cannon.png" => include_bytes!("../../web/assets/house-calls/confetti-cannon.png"),
        "tea-break.png" => include_bytes!("../../web/assets/house-calls/tea-break.png"),
        "helping-hands.png" => include_bytes!("../../web/assets/house-calls/helping-hands.png"),
        "headphones.png" => include_bytes!("../../web/assets/house-calls/headphones.png"),
        _ => return Err(fault(404, "House call image not found.").into()),
    };
    Ok(conditional::static_asset(&h, "image/png", bytes))
}
impl Platform {
    pub async fn dispatch_house_calls(&self) -> Result<()> {
        // Dedicated client: never follow a redirect carrying the secret topic.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .connect_timeout(Duration::from_secs(2))
            .build()?;
        // One attempt per queued call. A failed/down ntfy must not hold up chat or
        // turn a notification into repeated Home Assistant actions.
        if let Some(call) = self.db.take_house_call()? {
            let result = client
                .post(&call.topic_url)
                .header("Tags", &call.tag)
                .body(call.body)
                .send()
                .await;
            if !result.is_ok_and(|response| response.status().is_success()) {
                // reqwest errors can contain the URL. Deliberately log no error/request.
                tracing::warn!("House call notification was not accepted");
            }
        }
        Ok(())
    }
}
