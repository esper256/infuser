//! Curated, local profile artwork. IDs are persisted; no uploads or remote URLs.
use super::*;

struct Avatar {
    id: &'static str,
    name: &'static str,
    hidden: bool,
    small: &'static [u8],
    large: &'static [u8],
}

macro_rules! avatar {
    ($id:literal, $name:literal) => {
        avatar!($id, $name, false)
    };
    ($id:literal, $name:literal, $hidden:expr) => {
        Avatar {
            id: $id,
            name: $name,
            hidden: $hidden,
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
    avatar!("tinker-goblin", "Tinker Goblin"),
    avatar!("clockwork-owl", "Clockwork Owl"),
    avatar!("circuit-crane", "Circuit Crane"),
    avatar!("signal-kettle", "Signal Kettle"),
    avatar!("volt-ninja", "Volt Ninja"),
    avatar!("skate-bones", "Skate Bones"),
    avatar!("rust-sentinel", "Rust Sentinel"),
    avatar!("magma-golem", "Magma Golem"),
    avatar!("shadow-ranger", "Shadow Ranger"),
    avatar!("oni-ronin", "Oni Ronin"),
    avatar!("titan-mech", "Titan Mech"),
    avatar!("night-strike", "Night Strike"),
    avatar!("glitch-mask", "Glitch Mask"),
    avatar!("venom-scorpion", "Venom Scorpion"),
    // Operator-assigned. Kept out of the picker catalog below.
    avatar!("origami-unicorn", "Origami Unicorn", true),
];

pub(super) fn catalog() -> Value {
    // Fixed FNV-1a ranks scatter creation batches without a per-session shuffle.
    // Rank only persistent IDs: additions slot in without reordering existing art.
    // Keep the algorithm/seed fixed so releases and display-name edits stay stable.
    let mut ordered: Vec<_> = AVATARS.iter().filter(|avatar| !avatar.hidden).collect();
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

/// Pictures that still render when someone already has them, but are not choices.
pub(super) fn portraits() -> Value {
    json!(
        AVATARS
            .iter()
            .filter(|avatar| avatar.hidden)
            .map(|avatar| json!({"id": avatar.id, "name": avatar.name}))
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
    let data = p.db.catalog()?;
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
        body.avatar.is_empty() || AVATARS.iter().any(|a| a.id == body.avatar && !a.hidden),
        fault(400, "Choose a profile picture from the list.")
    );
    let avatar = (!body.avatar.is_empty()).then_some(body.avatar);
    p.db.edit(|d| {
        ensure!(
            d.profile_enabled(&email),
            fault(403, "Your invitation is no longer active.")
        );
        let member = d.members.get_mut(&email).unwrap();
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
        ensure!(
            d.profile_enabled(&email),
            fault(403, "Your invitation is no longer active.")
        );
        let member = d.members.get_mut(&email).unwrap();
        let show = member.avatar.is_none() && !member.avatar_prompt_seen;
        member.avatar_prompt_seen = true;
        Ok(show)
    })?;
    Ok(reply(json!({"show":show})))
}

pub(super) async fn assign(
    State(p): State<Platform>,
    Path(member): Path<String>,
    h: HeaderMap,
    Json(body): Json<Profile>,
) -> Answer {
    let actor = p.operator(&h, true).await?;
    let (group, _) = p.group_view(&h, &actor)?;
    ensure!(
        body.avatar.is_empty() || AVATARS.iter().any(|a| a.id == body.avatar),
        fault(400, "Choose a profile picture from the list.")
    );
    let avatar = (!body.avatar.is_empty()).then_some(body.avatar);
    let saved = p.db.edit(|d| {
        let key = member_in_group(d, &group, &member)?;
        let player_id = d.player_id(&key);
        let record = d.members.get_mut(&key).unwrap();
        record.avatar = avatar.clone();
        Ok(json!({"name": record.name, "avatar": avatar, "player_id": player_id}))
    })?;
    Ok(reply(saved))
}

/// Email, profile handle, or the only display name in this group.
fn member_in_group(data: &platform::Data, group: &str, query: &str) -> Result<String> {
    if let Some(key) = data.group_member_key(query, group)? {
        return Ok(key);
    }
    let query = query.trim();
    ensure!(!query.is_empty(), fault(404, "Member not found."));
    let mut found = None;
    for (key, member) in &data.members {
        if data.groups.member(key) == group && member.name.trim().eq_ignore_ascii_case(query) {
            ensure!(
                found.is_none(),
                fault(409, "More than one member has that name.")
            );
            found = Some(key.clone());
        }
    }
    found.ok_or_else(|| fault(404, "Member not found."))
}
