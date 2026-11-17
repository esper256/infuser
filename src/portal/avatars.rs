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
    avatar!("soccer-comet", "Soccer Comet"),
    avatar!("hoop-hero", "Hoop Hero"),
    avatar!("turbo-kart", "Turbo Kart"),
    avatar!("jurassic-grin", "Jurassic Grin"),
    avatar!("reef-shark", "Reef Shark"),
    avatar!("mech-buddy", "Mech Buddy"),
    avatar!("brick-rocket", "Brick Rocket"),
    avatar!("lab-spark", "Lab Spark"),
    avatar!("treasure-mimic", "Treasure Mimic"),
    avatar!("pizza-planet", "Pizza Planet"),
    avatar!("home-run", "Home Run"),
    avatar!("slapshot", "Slapshot"),
    avatar!("monster-truck", "Monster Truck"),
    avatar!("astro-sloth", "Astro Sloth"),
    avatar!("moon-moth", "Moon Moth"),
    avatar!("ink-captain", "Ink Captain"),
    avatar!("cobalt-cobra", "Cobalt Cobra"),
    avatar!("spell-hat", "Spell Hat"),
    avatar!("ghost-toast", "Ghost Toast"),
    avatar!("power-chord", "Power Chord"),
    avatar!("star-cartographer", "Star Cartographer"),
    avatar!("cloud-rider", "Cloud Rider"),
    avatar!("comet-otter", "Comet Otter"),
    avatar!("aurora-lynx", "Aurora Lynx"),
    avatar!("lantern-dragon", "Lantern Dragon"),
    avatar!("nebula-jelly", "Nebula Jelly"),
    avatar!("copper-kestrel", "Copper Kestrel"),
    avatar!("secret-key", "Secret Key"),
    avatar!("stardust-deck", "Stardust Deck"),
    avatar!("tideglass", "Tideglass"),
    avatar!("pixel-combo-stick", "Combo Stick"),
    avatar!("pixel-turbo-cart", "Turbo Cartridge"),
    avatar!("pixel-save-disk", "Save Disk"),
    avatar!("pixel-mana-flask", "Mana Flask"),
    avatar!("pixel-bonus-heart", "Bonus Heart"),
    avatar!("pixel-loot-chest", "Loot Chest"),
    avatar!("pixel-portal-door", "Portal Door"),
    avatar!("pixel-rocket-boots", "Rocket Boots"),
    avatar!("pixel-neon-pinball", "Neon Pinball"),
    avatar!("pixel-checkpoint", "Checkpoint Flag"),
    avatar!("rooftop-arcade", "Rooftop Arcade"),
    avatar!("moon-greenhouse", "Moon Greenhouse"),
    avatar!("lantern-treehouse", "Lantern Treehouse"),
    avatar!("deepsea-observatory", "Deepsea Observatory"),
    avatar!("sky-garden", "Sky Garden"),
    avatar!("snow-radio", "Snow Radio"),
    avatar!("midnight-bazaar", "Midnight Bazaar"),
    avatar!("crystal-refuge", "Crystal Refuge"),
    avatar!("ember-workshop", "Ember Workshop"),
    avatar!("orbit-hideout", "Orbit Hideout"),
];

pub(super) fn catalog() -> Value {
    // Fixed FNV-1a ranks scatter creation batches without a per-session shuffle.
    // Rank only persistent IDs: additions slot in without reordering existing art.
    // Keep the algorithm/seed fixed so releases and display-name edits stay stable.
    let mut ordered: Vec<_> = AVATARS.iter().collect();
    ordered.sort_unstable_by_key(|a| {
        let rank = a.id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        (rank, a.id)
    });
    json!(
        ordered
            .into_iter()
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

pub(super) async fn usage(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (group, _) = p.group_view(&h, &email)?;
    let data = p.db.read()?;
    let users: Vec<_> = data
        .members
        .iter()
        .filter(|(email, member)| {
            member.enabled && member.avatar.is_some() && data.groups.member(email) == group
        })
        .map(|(_, member)| json!({"name":member.name,"avatar":member.avatar}))
        .collect();
    Ok(reply(json!({"users":users})))
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
        member.avatar_prompt_seen = true;
        Ok(())
    })?;
    Ok(reply(json!({"avatar":avatar})))
}

// Claim once per profile, atomically across tabs/devices. Closing with initials
// is a valid choice, so the reminder does not depend on saving an avatar.
pub(super) async fn picture_prompt(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, true).await?;
    let show = p.db.edit(|d| {
        let member = d
            .members
            .get_mut(&email)
            .filter(|m| m.enabled)
            .ok_or_else(|| fault(403, "Your invitation is no longer active."))?;
        let show = member.avatar.is_none() && !member.avatar_prompt_seen;
        member.avatar_prompt_seen = true;
        Ok(show)
    })?;
    Ok(reply(json!({"show":show})))
}
