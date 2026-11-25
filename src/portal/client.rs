use super::*;
use std::sync::OnceLock;

pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        hash(&format!(
            "{}{}{}",
            concat!(
                env!("CARGO_PKG_VERSION"),
                include_str!("../../web/index.html"),
                include_str!("../../web/clubhouse.js"),
                include_str!("../../web/telemetry.js"),
                include_str!("../../web/clubhouse.css"),
                include_str!("../../web/chat.js"),
                include_str!("../../web/chat-reads.js"),
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
                include_str!("../../web/themes/funny-pages.css"),
                include_str!("../../web/themes/field-notes.css"),
                include_str!("../../web/themes/castle-keep.css"),
            ),
            super::themes::source_fingerprint(),
            crate::BUILD_SHA
        ))
    })
}

// A future emergency release can advance this generation. Fresh HTML acknowledges
// it, so a persistent directive cannot repeatedly reload the replacement page.
const FORCE_RELOAD_GENERATION: u32 = 0;

pub(super) fn update() -> Value {
    json!({"version":version(),"force_reload":FORCE_RELOAD_GENERATION})
}

pub(super) fn version_meta() -> String {
    format!(
        r#"<meta name="infuser-client-version" content="{}" data-force-reload="{FORCE_RELOAD_GENERATION}">"#,
        version()
    )
}

pub(super) async fn state(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let (_, inspecting) = p.group_view(&h, &email)?;
    let time = now();
    let game_open =
        p.db.read()?
            .launches
            .values()
            .any(|l| l.email == email && l.live(time) && l.seen.saturating_add(90) > time);
    let mut state = update();
    state["game_open"] = json!(game_open && !inspecting);
    Ok(reply(state))
}
