use crate::platform::Data;
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
pub const DEFAULTS: &[&str] = &[
    "2d",
    "3d",
    "action",
    "adventure",
    "arcade",
    "chill",
    "co-op",
    "competitive",
    "exploration",
    "fighting",
    "horror",
    "music",
    "platformer",
    "puzzle",
    "racing",
    "rhythm",
    "roguelike",
    "rpg",
    "sandbox",
    "shooter",
    "simulation",
    "sports",
    "strategy",
    "survival",
];
#[derive(Serialize)]
pub struct Tag {
    pub id: String,
    pub label: String,
    pub count: usize,
}
pub fn valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub fn normalize(tags: &[String]) -> Result<Vec<String>> {
    ensure!(
        tags.len() <= 8,
        crate::portal::fault(400, "Choose up to 8 tags.")
    );
    let mut result = Vec::new();
    for tag in tags {
        let id = tag
            .trim()
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-");
        ensure!(
            valid(&id),
            crate::portal::fault(400, "Tags use 1–32 letters, numbers or dashes.")
        );
        if !result.contains(&id) {
            result.push(id);
        }
    }
    Ok(result)
}
pub fn label(id: &str) -> String {
    match id {
        "2d" => "2D".into(),
        "3d" => "3D".into(),
        "rpg" => "RPG".into(),
        "co-op" => "Co-op".into(),
        _ => id
            .split('-')
            .map(|s| {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}
pub fn catalog(data: &Data, include_previews: bool) -> Vec<Tag> {
    catalog_in_group(data, include_previews, crate::platform::groups::HOME)
}
pub fn catalog_in_group(data: &Data, include_previews: bool, group: &str) -> Vec<Tag> {
    catalog_for_apps(data, data.apps.iter().filter(|(id, app)|
        data.groups.app(id) == group && (app.listed || include_previews)
    ).map(|(id,_)|id.as_str()))
}
pub fn catalog_for_apps<'a>(data: &Data, apps: impl Iterator<Item = &'a str>) -> Vec<Tag> {
    let mut counts: BTreeMap<String, usize> =
        DEFAULTS.iter().map(|id| (id.to_string(), 0)).collect();
    for id in apps {
        for tag in data.tags.get(id).into_iter().flatten() {
            *counts.entry(tag.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(id, count)| Tag {
            label: label(&id),
            id,
            count,
        })
        .collect()
}
