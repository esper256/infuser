//! Reactions on an Infuser update in Recently. The entry is the whole site,
//! not one group's game.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vote {
    emoji: String,
}

pub(super) async fn set(
    State(p): State<Platform>,
    Path(batch): Path<String>,
    h: HeaderMap,
    Json(vote): Json<Vote>,
) -> Answer {
    let profile = p.member(&h, true).await?;
    ensure!(
        p.db.read()?.profile_enabled(&profile),
        fault(403, "Membership is disabled.")
    );
    ensure!(
        valid_emoji(&vote.emoji),
        fault(400, "Choose a reaction emoji.")
    );
    ensure!(
        p.db.set_platform_reaction(&batch, &profile, &vote.emoji)?,
        fault(404, "That Infuser update is not in Recently.")
    );
    Ok(reply(json!({"ok":true})))
}
