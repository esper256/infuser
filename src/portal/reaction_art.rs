//! Local illustrated game reactions; persisted values remain Unicode emoji IDs.
use super::*;

pub(super) async fn image(Path(file): Path<String>, h: HeaderMap) -> Answer {
    let bytes: &'static [u8] = match file.as_str() {
        "atmosphere.webp" => include_bytes!("../../assets/reactions/atmosphere.webp"),
        "cozy.webp" => include_bytes!("../../assets/reactions/cozy.webp"),
        "gem.webp" => include_bytes!("../../assets/reactions/gem.webp"),
        "clever.webp" => include_bytes!("../../assets/reactions/clever.webp"),
        "wild.webp" => include_bytes!("../../assets/reactions/wild.webp"),
        "surprises.webp" => include_bytes!("../../assets/reactions/surprises.webp"),
        "art.webp" => include_bytes!("../../assets/reactions/art.webp"),
        "music.webp" => include_bytes!("../../assets/reactions/music.webp"),
        "smile.webp" => include_bytes!("../../assets/reactions/smile.webp"),
        "together.webp" => include_bytes!("../../assets/reactions/together.webp"),
        "fire.webp" => include_bytes!("../../assets/reactions/fire.webp"),
        "spooky.webp" => include_bytes!("../../assets/reactions/spooky.webp"),
        "trophy.webp" => include_bytes!("../../assets/reactions/trophy.webp"),
        "celebrate.webp" => include_bytes!("../../assets/reactions/celebrate.webp"),
        _ => return Err(fault(404, "Unknown reaction artwork.").into()),
    };
    Ok(conditional::static_asset(&h, "image/webp", bytes))
}
