//! Group-owned house calls. Secrets and the durable delivery queue never enter a client view.
use super::*;
use crate::portal::fault;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct Image {
    pub id: String,
    pub label: String,
}
pub fn images() -> Vec<Image> {
    serde_json::from_str(include_str!("../../web/assets/house-calls/manifest.json"))
        .expect("shipped house-call manifest")
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub id: String,
    pub enabled: bool,
    pub label: String,
    pub tag: String,
    #[serde(default)]
    pub message: String,
    pub cooldown_minutes: u32,
    pub profiles: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    pub start: String,
    pub end: String,
    pub timezone: String,
}
impl QuietHours {
    fn times(&self) -> Result<(jiff::civil::Time, jiff::civil::Time)> {
        let parse = |value: &str| -> Result<jiff::civil::Time> {
            ensure!(
                value.len() == 5 && value.as_bytes()[2] == b':',
                fault(422, "Use HH:MM for quiet hours.")
            );
            value
                .parse()
                .map_err(|_| fault(422, "Use HH:MM for quiet hours."))
        };
        let start = parse(&self.start)?;
        let end = parse(&self.end)?;
        ensure!(
            start != end,
            fault(422, "Choose different start and end times.")
        );
        Ok((start, end))
    }
    fn validate(&self) -> Result<()> {
        self.times()?;
        jiff::tz::TimeZone::get(&self.timezone)
            .map_err(|_| fault(422, "Choose a valid time zone."))?;
        Ok(())
    }
    fn active(&self, seconds: u64) -> Result<bool> {
        let (start, end) = self.times()?;
        let local = jiff::Timestamp::from_second(seconds.try_into()?)?
            .to_zoned(jiff::tz::TimeZone::get(&self.timezone)?)
            .time();
        Ok(if start < end {
            local >= start && local < end
        } else {
            local >= start || local < end
        })
    }
}
fn quiet_hours(c: &Connection, group: &str) -> Result<Option<QuietHours>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT quiet_hours FROM house_call_settings WHERE private_group=?1",
            [group],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into))
        .transpose()
}
fn quiet(c: &Connection, group: &str, time: u64) -> Result<bool> {
    // If installed time-zone data disappears, stay quiet rather than break chat.
    Ok(quiet_hours(c, group)?.is_some_and(|hours| hours.active(time).unwrap_or(true)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    // None preserves the secret; an empty string clears it. Never serialize this input.
    pub topic_url: Option<String>,
    pub images: Vec<Call>,
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
}
pub(super) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS house_call_settings (private_group TEXT PRIMARY KEY, topic_url TEXT, images TEXT NOT NULL, quiet_hours TEXT);
      CREATE TABLE IF NOT EXISTS house_call_triggers (private_group TEXT NOT NULL, image TEXT NOT NULL, triggered INTEGER NOT NULL, PRIMARY KEY(private_group,image));
      CREATE TABLE IF NOT EXISTS house_call_outbox (id INTEGER PRIMARY KEY AUTOINCREMENT, private_group TEXT NOT NULL, topic_url TEXT NOT NULL, tag TEXT NOT NULL, body TEXT NOT NULL);")?;
    let settings_columns = c
        .prepare("PRAGMA table_info(house_call_settings)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !settings_columns.iter().any(|s| s == "quiet_hours") {
        c.execute(
            "ALTER TABLE house_call_settings ADD COLUMN quiet_hours TEXT",
            [],
        )?;
    }
    let columns = c
        .prepare("PRAGMA table_info(library_chat)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (name, definition) in [
        ("house_call", "TEXT"),
        ("house_call_triggered", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.iter().any(|s| s == name) {
            c.execute(
                &format!("ALTER TABLE library_chat ADD COLUMN {name} {definition}"),
                [],
            )?;
        }
    }
    Ok(())
}
fn load(c: &Connection, group: &str) -> Result<(Option<String>, Vec<Call>)> {
    let stored = c
        .query_row(
            "SELECT topic_url,images FROM house_call_settings WHERE private_group=?1",
            [group],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    let (topic, configured) = match stored {
        Some((topic, raw)) => (topic, serde_json::from_str::<Vec<Call>>(&raw)?),
        None => (None, Vec::new()),
    };
    let calls = images()
        .into_iter()
        .map(|image| {
            configured
                .iter()
                .find(|c| c.id == image.id)
                .cloned()
                .unwrap_or(Call {
                    id: image.id,
                    label: image.label,
                    enabled: false,
                    tag: String::new(),
                    message: String::new(),
                    cooldown_minutes: 5,
                    profiles: Vec::new(),
                })
        })
        .collect();
    Ok((topic, calls))
}
fn admin(data: &Data, actor: &str, group: &str) -> Result<()> {
    ensure!(
        data.profile_enabled(actor)
            && data.members[actor].admin
            && data.groups.member(actor) == group,
        fault(403, "An admin of this library is required.")
    );
    Ok(())
}
pub(super) fn label(c: &Connection, group: &str, id: &str) -> Result<Option<String>> {
    Ok(load(c, group)?
        .1
        .into_iter()
        .find(|call| call.id == id)
        .map(|call| call.label))
}
// Called inside the same immediate transaction that saves the message. Queueing,
// cooldown reservation and the visible confetti flag either all commit or none do.
pub(super) fn enqueue(
    c: &Connection,
    data: &Data,
    group: &str,
    sender: &str,
    id: &str,
    time: u64,
) -> Result<bool> {
    let (topic, calls) = load(c, group)?;
    let call = calls.into_iter().find(|call| {
        call.id == id && call.enabled && call.profiles.contains(&data.player_id(sender))
    });
    ensure!(
        topic.is_some() && call.is_some() && data.groups.member(sender) == group,
        fault(422, "That house call is not available.")
    );
    let call = call.unwrap();
    if quiet(c, group, time)? {
        return Ok(false);
    }
    let last: Option<u64> = c
        .query_row(
            "SELECT triggered FROM house_call_triggers WHERE private_group=?1 AND image=?2",
            params![group, id],
            |r| r.get(0),
        )
        .optional()?;
    if last.is_some_and(|last| time < last.saturating_add(u64::from(call.cooldown_minutes) * 60)) {
        return Ok(false);
    }
    let queued: u64 = c.query_row(
        "SELECT count(*) FROM house_call_outbox WHERE private_group=?1",
        [group],
        |r| r.get(0),
    )?;
    if queued >= 64 {
        tracing::warn!("House call queue is full");
        return Ok(false);
    }
    let mut body = call
        .message
        .replace("{{profile}}", &data.members[sender].name);
    // ntfy accepts at most 4 KiB. Expansion is literal, never executable template code.
    if body.len() > 4096 {
        let mut end = 4096;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
    }
    c.execute(
        "INSERT INTO house_call_outbox(private_group,topic_url,tag,body) VALUES(?1,?2,?3,?4)",
        params![group, topic.unwrap(), call.tag, body],
    )?;
    c.execute("INSERT INTO house_call_triggers(private_group,image,triggered) VALUES(?1,?2,?3) ON CONFLICT(private_group,image) DO UPDATE SET triggered=excluded.triggered",params![group,id,time])?;
    Ok(true)
}
pub struct Delivery {
    pub topic_url: String,
    pub tag: String,
    pub body: String,
}
impl Database {
    pub fn house_calls_admin(&self, actor: &str, group: &str) -> Result<Value> {
        let mut connection = self.connection.lock().unwrap();
        let c = connection.transaction()?;
        let data = self.cached_catalog(&c)?;
        admin(&data, actor, group)?;
        let (topic, calls) = load(&c, group)?;
        let profiles: Vec<_> = data
            .members
            .iter()
            .filter(|(key, m)| m.enabled && data.groups.member(key) == group)
            .map(|(key, m)| json!({"id":data.player_id(key),"name":m.name}))
            .collect();
        Ok(
            json!({"topic_set":topic.is_some(),"images":calls,"profiles":profiles,"quiet_hours":quiet_hours(&c,group)?}),
        )
    }
    pub fn house_calls_set(&self, actor: &str, group: &str, settings: Settings) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let data = self.cached_catalog(&tx)?;
        admin(&data, actor, group)?;
        let (mut topic, _) = load(&tx, group)?;
        if let Some(value) = settings.topic_url {
            if value.is_empty() {
                topic = None;
            } else {
                let url = reqwest::Url::parse(&value)
                    .map_err(|_| fault(422, "Use an HTTP or HTTPS ntfy topic URL."))?;
                ensure!(
                    value.len() <= 2048
                        && matches!(url.scheme(), "https" | "http")
                        && url.host_str().is_some()
                        && url.fragment().is_none()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && !url.path().trim_matches('/').is_empty(),
                    fault(422, "Use an HTTP or HTTPS ntfy topic URL.")
                );
                topic = Some(value);
            }
        }
        if let Some(hours) = &settings.quiet_hours {
            hours.validate()?;
        }
        let shipped = images();
        let mut ids = BTreeSet::new();
        ensure!(
            settings.images.len() == shipped.len(),
            fault(422, "Configure each shipped house call.")
        );
        for call in &settings.images {
            ensure!(
                shipped.iter().any(|image| image.id == call.id) && ids.insert(&call.id),
                fault(422, "Unknown or duplicate house call.")
            );
            ensure!(
                !call.label.trim().is_empty()
                    && call.label.chars().count() <= 80
                    && !call.label.chars().any(char::is_control),
                fault(422, "Use a label of 1–80 characters.")
            );
            ensure!(
                call.tag.len() <= 80
                    && (!call.enabled || !call.tag.is_empty())
                    && call
                        .tag
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                fault(
                    422,
                    "Use an ntfy tag containing letters, numbers, dashes or underscores."
                )
            );
            ensure!(
                call.message.len() <= 4096 && call.cooldown_minutes <= 10080,
                fault(
                    422,
                    "Use a message up to 4096 bytes and a cooldown up to one week."
                )
            );
            let mut profiles = BTreeSet::new();
            for id in &call.profiles {
                ensure!(
                    profiles.insert(id)
                        && data.members.iter().any(|(key, m)| m.enabled
                            && data.groups.member(key) == group
                            && data.player_id(key) == *id),
                    fault(422, "Choose profiles from this library.")
                );
            }
        }
        // Replacing/clearing configuration cancels unsent calls for this group only.
        tx.execute(
            "DELETE FROM house_call_outbox WHERE private_group=?1",
            [group],
        )?;
        tx.execute("INSERT INTO house_call_settings(private_group,topic_url,images,quiet_hours) VALUES(?1,?2,?3,?4) ON CONFLICT(private_group) DO UPDATE SET topic_url=excluded.topic_url,images=excluded.images,quiet_hours=excluded.quiet_hours",params![group,topic,serde_json::to_string(&settings.images)?,settings.quiet_hours.map(|hours|serde_json::to_string(&hours)).transpose()?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn house_calls_available(&self, actor: &str, group: &str) -> Result<Value> {
        let mut connection = self.connection.lock().unwrap();
        let c = connection.transaction()?;
        let data = self.cached_catalog(&c)?;
        ensure!(
            data.profile_enabled(actor),
            fault(403, "Membership is disabled.")
        );
        let (topic, calls) = load(&c, group)?;
        let available: Vec<_> = calls
            .into_iter()
            .filter(|call| {
                topic.is_some()
                    && data.groups.member(actor) == group
                    && call.enabled
                    && call.profiles.contains(&data.player_id(actor))
            })
            .map(|call| json!({"id":call.id,"label":call.label}))
            .collect();
        Ok(json!({"images":available}))
    }
    pub fn take_house_call(&self) -> Result<Option<Delivery>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let delivery=tx.query_row("DELETE FROM house_call_outbox WHERE id=(SELECT id FROM house_call_outbox ORDER BY id LIMIT 1) RETURNING topic_url,tag,body,private_group",[],|r|Ok((Delivery{topic_url:r.get(0)?,tag:r.get(1)?,body:r.get(2)?},r.get::<_,String>(3)?))).optional()?;
        let delivery = match delivery {
            Some((call, group)) if !quiet(&tx, &group, now())? => Some(call),
            _ => None,
        };
        tx.commit()?;
        Ok(delivery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn active(hours: &QuietHours, stamp: &str) -> bool {
        hours
            .active(stamp.parse::<jiff::Timestamp>().unwrap().as_second() as u64)
            .unwrap()
    }
    #[test]
    fn quiet_hours_boundaries_midnight_timezone_and_dst() {
        let night = QuietHours {
            start: "22:00".into(),
            end: "08:00".into(),
            timezone: "America/Los_Angeles".into(),
        };
        night.validate().unwrap();
        assert!(!active(&night, "2026-11-25T05:59:59Z"));
        assert!(active(&night, "2026-11-25T06:00:00Z"));
        assert!(active(&night, "2026-11-25T15:59:59Z"));
        assert!(!active(&night, "2026-11-25T16:00:00Z"));
        let day = QuietHours {
            start: "09:00".into(),
            end: "17:00".into(),
            timezone: "UTC".into(),
        };
        assert!(active(&day, "2026-11-25T09:00:00Z"));
        assert!(!active(&day, "2026-11-25T17:00:00Z"));
        let fold = QuietHours {
            start: "01:00".into(),
            end: "02:00".into(),
            timezone: "America/Los_Angeles".into(),
        };
        assert!(active(&fold, "2026-11-01T08:30:00Z"));
        assert!(active(&fold, "2026-11-01T09:30:00Z"));
        assert!(
            QuietHours {
                start: "22:00".into(),
                end: "22:00".into(),
                timezone: "UTC".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            QuietHours {
                start: "25:00".into(),
                end: "08:00".into(),
                timezone: "UTC".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            QuietHours {
                start: "22:00".into(),
                end: "08:00".into(),
                timezone: "Not/AZone".into()
            }
            .validate()
            .is_err()
        );
    }
}
