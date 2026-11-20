//! What's new for the running build.
//!
//! The text is generated in CI and compiled into the daemon. It is not a
//! `release.json` field: the updater installed on a host rejects unknown
//! manifest fields, and that helper is not replaced by the release itself.
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Changelog {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Change {
    pub text: String,
    #[serde(default)]
    pub audience: String,
}

pub fn embedded() -> Changelog {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/changelog.json")))
        .unwrap_or_default()
}

pub fn family_lines() -> Vec<String> {
    lines_for(&embedded(), "family")
}

pub fn notes() -> (Vec<String>, Vec<String>) {
    let log = embedded();
    (lines_for(&log, "family"), lines_for(&log, "internal"))
}

fn lines_for(log: &Changelog, audience: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for change in &log.changes {
        if change.audience != audience {
            continue;
        }
        let Some(text) = clean_line(&change.text) else {
            continue;
        };
        if lines.len() == 12 {
            break;
        }
        lines.push(text);
    }
    lines
}

fn clean_line(value: &str) -> Option<String> {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= 160 {
        return Some(text);
    }
    let mut shortened: String = text.chars().take(159).collect();
    shortened.truncate(shortened.trim_end().len());
    shortened.push('…');
    Some(shortened)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_and_internal_notes_stay_separate() {
        let log: Changelog = serde_json::from_str(
            r#"{"version":1,"since":"abc","changes":[
                {"text":"Games remember where you left off","audience":"family","pr":12},
                {"text":"Cache the release build","audience":"internal"},
                {"text":"  ","audience":"family"},
                {"text":"Mystery","audience":"other"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            lines_for(&log, "family"),
            vec!["Games remember where you left off".to_string()]
        );
        assert_eq!(
            lines_for(&log, "internal"),
            vec!["Cache the release build".to_string()]
        );
    }

    #[test]
    fn a_changelog_field_is_not_part_of_the_release_manifest() {
        let raw = r#"{"commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","artifacts":{},"changelog":[]}"#;
        assert!(serde_json::from_str::<crate::updater::Release>(raw).is_err());
    }
}
