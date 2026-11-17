//! Deliberately limited diagnostics: never include launch links, accounts or paths.
use std::time::{SystemTime, UNIX_EPOCH};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD: &str = match option_env!("INFUSER_BUILD_SHA") {
    Some(value) => value,
    None => "development",
};

pub struct UpdateInfo {
    pub status: &'static str,
    pub checked: Option<u64>,
}

impl UpdateInfo {
    pub fn last_check(&self) -> String {
        let Some(checked) = self.checked else {
            return "Not checked yet".into();
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if checked > now {
            return "Check time unavailable (clock changed)".into();
        }
        match now - checked {
            0..60 => "Checked just now".into(),
            seconds @ 60..3600 => format!("Checked {} min ago", seconds / 60),
            seconds @ 3600..86400 => format!("Checked {} h ago", seconds / 3600),
            seconds => format!("Checked {} d ago", seconds / 86400),
        }
    }

    pub fn diagnostics(&self) -> String {
        format!(
            "Infuser Play\nVersion: {VERSION}\nBuild: {BUILD}\nPlatform: {}-{}\nUpdates: {}\nLast check (Unix seconds, UTC): {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.status,
            self.checked
                .map_or_else(|| "none".into(), |time| time.to_string()),
        )
    }
}
