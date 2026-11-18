use super::*;
use std::sync::OnceLock;

pub(super) fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        hash(&format!(
            "{}{}",
            concat!(
                env!("CARGO_PKG_VERSION"),
                include_str!("../../web/index.html"),
                include_str!("../../web/clubhouse.js"),
                include_str!("../../web/clubhouse.css"),
                include_str!("../../web/chat.js"),
                include_str!("../../web/chat-emoji.js"),
                include_str!("../../web/screenshots.js"),
                include_str!("../../web/vendor/emoji-picker/picker.js"),
                include_str!("../../web/vendor/emoji-picker/database.js"),
                include_str!("../../web/vendor/emoji-picker/picker.css"),
                include_str!("../../web/vendor/emoji-picker/data.json"),
                include_str!("../../web/feedback.js"),
                include_str!("../../web/pwa.js"),
                include_str!("../../web/profiles.js"),
                include_str!("../../web/profile-chooser.js"),
                include_str!("../../web/profiles.html"),
                include_str!("../../web/desktop.js"),
                include_str!("../../web/sw.js"),
                include_str!("../../web/groups.js"),
                include_str!("../../web/client-updates.js"),
                include_str!("../../web/server-updates.js"),
                include_str!("../../web/themes.js"),
                include_str!("../../web/themes/nightglass.css"),
                include_str!("../../web/themes/cartridge-club.css"),
            ),
            crate::BUILD_SHA
        ))
    })
}

pub(super) async fn state(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (_, inspecting) = p.group_view(&h, &email)?;
    let time = now();
    let game_open =
        p.db.read()?
            .launches
            .values()
            .any(|l| l.email == email && l.expires > time && l.seen.saturating_add(90) > time);
    Ok(reply(
        json!({"version":version(),"game_open":game_open&&!inspecting}),
    ))
}
