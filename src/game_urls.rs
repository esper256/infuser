//! Stable human entry links; execution remains isolated by player, guest and build.
use crate::platform::Data;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameUrl {
    pub slug: String,
    pub aliases: Vec<String>,
}
pub fn valid(value: &str) -> bool {
    crate::config::slug(value)
        && value.len() <= 24
        && !value.ends_with('-')
        && !value.contains("--")
        && !value.starts_with("g-")
        && ![
            "www",
            "api",
            "admin",
            "assets",
            "oauth2",
            "library",
            "clubhouse",
            "games",
            "login",
            "logout",
            "healthz",
            "static",
            "infuser",
        ]
        .contains(&value)
}
pub fn resolve<'a>(data: &'a Data, slug: &str) -> Option<&'a str> {
    resolve_in_group(data, slug, crate::platform::groups::HOME)
}
pub fn resolve_in_group<'a>(data: &'a Data, slug: &str, group: &str) -> Option<&'a str> {
    data.game_urls
        .iter()
        .find(|(app, u)| {
            data.groups.app(app) == group && (u.slug == slug || u.aliases.iter().any(|a| a == slug))
        })
        .map(|(app, _)| app.as_str())
}
pub fn assign(data: &mut Data, app: &str, title: &str) -> String {
    if let Some(url) = data.game_urls.get(app) {
        return url.slug.clone();
    }
    let words: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut base = words
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    base.truncate(24);
    base = base.trim_end_matches('-').into();
    if !valid(&base) {
        base = "game".into();
    }
    let mut slug = base.clone();
    let mut suffix = 2;
    while resolve_in_group(data, &slug, data.groups.app(app)).is_some() {
        let tail = format!("-{suffix}");
        slug = format!(
            "{}{}",
            base[..base.len().min(24 - tail.len())].trim_end_matches('-'),
            tail
        );
        suffix += 1;
    }
    data.game_urls.insert(
        app.into(),
        GameUrl {
            slug: slug.clone(),
            aliases: vec![],
        },
    );
    slug
}
pub fn rename(data: &mut Data, app: &str, slug: &str) -> Result<()> {
    ensure!(
        valid(slug),
        crate::portal::fault(
            400,
            "Use 1–24 lowercase letters, numbers or single dashes; start with a letter and choose a non-reserved name."
        )
    );
    ensure!(
        resolve_in_group(data, slug, data.groups.app(app)).is_none_or(|owner| owner == app),
        crate::portal::fault(409, "That game URL is already reserved.")
    );
    let url = data
        .game_urls
        .get_mut(app)
        .ok_or_else(|| crate::portal::fault(404, "unknown game"))?;
    if url.slug != slug {
        // Bound permanent alias reservations per game. Existing aliases remain reusable.
        ensure!(
            url.aliases.len() < 64 || url.aliases.iter().any(|a| a == slug),
            crate::portal::fault(409, "This game has reached its URL rename limit.")
        );
        url.aliases.retain(|a| a != slug);
        url.aliases
            .push(std::mem::replace(&mut url.slug, slug.into()));
    }
    Ok(())
}
pub fn host(data: &Data, app: &str, id: &str, domain: &str) -> String {
    match data.game_urls.get(app) {
        Some(url) => format!("{}--g-{id}.{domain}", url.slug),
        None => format!("g-{id}.{domain}"),
    }
}
