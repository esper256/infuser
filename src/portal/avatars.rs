//! Curated, local profile artwork. IDs are persisted; no uploads or remote URLs.
use super::*;

struct Avatar {
    id: &'static str,
    name: &'static str,
    small: &'static [u8],
    large: &'static [u8],
}

macro_rules! avatar {
    ($id:literal, $name:literal) => {
        Avatar {
            id: $id,
            name: $name,
            small: include_bytes!(concat!("../../assets/avatars/", $id, "-128.webp")),
            large: include_bytes!(concat!("../../assets/avatars/", $id, "-256.webp")),
        }
    };
}

const AVATARS: &[Avatar] = &[
    avatar!("circuit-fox", "Circuit Fox"),
    avatar!("orbit-racer", "Orbit Racer"),
    avatar!("moss-knight", "Moss Knight"),
    avatar!("storm-drake", "Storm Drake"),
    avatar!("deepwave", "Deepwave"),
    avatar!("neon-runner", "Neon Runner"),
    avatar!("bass-pilot", "Bass Pilot"),
    avatar!("frost-scout", "Frost Scout"),
    avatar!("forge-rider", "Forge Rider"),
    avatar!("pulse-pad", "Pulse Pad"),
    avatar!("prism-core", "Prism Core"),
    avatar!("photon-loop", "Photon Loop"),
    avatar!("pocket-arcade", "Pocket Arcade"),
    avatar!("critical-gem", "Critical Gem"),
    avatar!("ion-racer", "Ion Racer"),
    avatar!("event-horizon", "Event Horizon"),
    avatar!("tiny-world", "Tiny World"),
    avatar!("midnight-ramen", "Midnight Ramen"),
    avatar!("mixtape", "Mixtape"),
    avatar!("kickflip", "Kickflip"),
    avatar!("jade-beetle", "Jade Beetle"),
    avatar!("voxel-axolotl", "Voxel Axolotl"),
    avatar!("blocky-capybara", "Blocky Capybara"),
    avatar!("bamboo-bandit", "Bamboo Bandit"),
    avatar!("frost-owl", "Frost Owl"),
    avatar!("lime-frog", "Lime Frog"),
    avatar!("pixel-panther", "Pixel Panther"),
    avatar!("moon-wolf", "Moon Wolf"),
    avatar!("loot-raccoon", "Loot Raccoon"),
    avatar!("byte-gecko", "Byte Gecko"),
    avatar!("ember-raven", "Ember Raven"),
];

pub(super) fn catalog() -> Value {
    json!(
        AVATARS
            .iter()
            .map(|a| json!({"id":a.id,"name":a.name}))
            .collect::<Vec<_>>()
    )
}

pub(super) async fn image(Path((id, size)): Path<(String, String)>) -> Answer {
    let avatar = AVATARS
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| fault(404, "Unknown profile picture."))?;
    let bytes = match size.as_str() {
        "128.webp" => avatar.small,
        "256.webp" => avatar.large,
        _ => return Err(fault(404, "Unknown profile picture size.").into()),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    // Empty string explicitly restores initials; omitted/null values are rejected.
    avatar: String,
}

pub(super) async fn update(
    State(p): State<Platform>,
    h: HeaderMap,
    Json(body): Json<Profile>,
) -> Answer {
    let email = p.member(&h, true).await?;
    ensure!(
        body.avatar.is_empty() || AVATARS.iter().any(|a| a.id == body.avatar),
        fault(400, "Choose a profile picture from the list.")
    );
    let avatar = (!body.avatar.is_empty()).then_some(body.avatar);
    p.db.edit(|d| {
        let member = d
            .members
            .get_mut(&email)
            .filter(|m| m.enabled)
            .ok_or_else(|| fault(403, "Your invitation is no longer active."))?;
        member.avatar = avatar.clone();
        Ok(())
    })?;
    Ok(reply(json!({"avatar":avatar})))
}
