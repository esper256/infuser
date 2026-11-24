use crate::{config::Config, platform, timeline};
use anyhow::{Context, Result, bail, ensure};
use glob::Pattern;
use jiff::{Timestamp, tz::TimeZone};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::Path,
};

#[path = "timeline_reports.rs"]
mod reports;

const HELP: &str = "Usage: infuser logs CONFIG [summary | memory --leaks] [OPTIONS]
       infuser-logs [summary | memory --leaks] [OPTIONS]

  --profile NAME|ID       Include this player and events without a profile
  --game APP_ID           Only this game
  --no-shared             Exclude events without a profile
  --since TIME            Start of window (default: 48h)
  --until TIME            End of window (default: now)
                          TIME: 30s, 5m, 2h, 1d, Unix ms, or ISO time (Pacific unless offset given)
  --kind GLOB             Match event kinds (quote globs, repeat for alternatives)
  --level LEVEL           debug, info, warn, or error
  --source SOURCE         client or server
  --json                  JSON array, or structured report
  --jsonl                 One raw event per line (timeline only)
  -h, --help              Show help";

fn time_arg(value: &str, now: i64) -> Result<i64> {
    if let Ok(ms) = value.parse::<u64>() {
        return Ok(ms.try_into()?);
    }
    if let Ok(ts) = value.parse::<Timestamp>() {
        return Ok(ts.as_millisecond());
    }
    if value.len() >= 10
        && value.get(4..5) == Some("-")
        && value.get(7..8) == Some("-")
        && value.as_bytes()[10..]
            .iter()
            .all(|c| c.is_ascii_digit() || b"Tt :.".contains(c))
    {
        let local = if value.len() == 10 {
            value
                .parse::<jiff::civil::Date>()
                .ok()
                .map(|d| d.at(0, 0, 0, 0))
        } else {
            value.parse::<jiff::civil::DateTime>().ok()
        };
        if let Some(local) = local {
            return Ok(TimeZone::get("America/Los_Angeles")?
                .to_ambiguous_timestamp(local)
                .unambiguous()
                .context("ambiguous or nonexistent Pacific time; specify an explicit offset")?
                .as_millisecond());
        }
    }
    for (suffix, scale) in [
        ('s', 1000),
        ('m', 60_000),
        ('h', 3_600_000),
        ('d', 86_400_000),
    ] {
        if let Some(n) = value
            .strip_suffix(suffix)
            .and_then(|n| n.parse::<u64>().ok())
            && let Some(delta) = n.checked_mul(scale).and_then(|v| i64::try_from(v).ok())
            && let Some(at) = now.checked_sub(delta)
        {
            return Ok(at);
        }
    }
    bail!(
        "invalid time '{value}'; use a duration like 30m, Unix milliseconds, or an ISO timestamp with Z or an offset"
    )
}

#[derive(Default)]
struct Options {
    query: timeline::Filter,
    player: Option<String>,
    app: Option<String>,
    kinds: Vec<Pattern>,
    level: Option<String>,
    source: Option<String>,
    no_shared: bool,
    json: bool,
    jsonl: bool,
    mode: Mode,
    leaks: bool,
}
#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Mode {
    #[default]
    Timeline,
    Summary,
    Memory,
}

impl Options {
    fn parse(args: &[String], now: i64) -> Result<Self> {
        let mut opts = Self::default();
        opts.query.until_ms = Some(now);
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || {
                it.next()
                    .filter(|s| !s.starts_with("--"))
                    .with_context(|| format!("missing value for {arg}"))
            };
            match arg.as_str() {
                "summary" | "memory" if opts.mode == Mode::Timeline => {
                    opts.mode = if arg == "summary" {
                        Mode::Summary
                    } else {
                        Mode::Memory
                    }
                }
                "--leaks" => opts.leaks = true,
                "--json" => opts.json = true,
                "--jsonl" => opts.jsonl = true,
                "--no-shared" => opts.no_shared = true,
                "--profile" => opts.player = Some(value()?.clone()),
                "--game" => opts.app = Some(value()?.clone()),
                "--since" => opts.query.since_ms = Some(time_arg(value()?, now)?),
                "--until" => opts.query.until_ms = Some(time_arg(value()?, now)?),
                "--kind" => opts
                    .kinds
                    .push(Pattern::new(value()?).context("invalid --kind glob")?),
                "--level" => {
                    let v = value()?;
                    ensure!(
                        matches!(v.as_str(), "debug" | "info" | "warn" | "error"),
                        "--level expects debug, info, warn, or error"
                    );
                    opts.level = Some(v.clone());
                }
                "--source" => {
                    let v = value()?;
                    ensure!(
                        matches!(v.as_str(), "client" | "server"),
                        "--source expects client or server"
                    );
                    opts.source = Some(v.clone());
                }
                _ => bail!("unknown logs argument '{arg}'; see logs --help"),
            }
        }
        ensure!(
            !opts.leaks || opts.mode == Mode::Memory,
            "--leaks requires memory"
        );
        ensure!(
            !opts.jsonl || (!opts.json && opts.mode == Mode::Timeline),
            "--jsonl requires timeline output and cannot be combined with --json"
        );
        ensure!(
            opts.query
                .since_ms
                .zip(opts.query.until_ms)
                .is_none_or(|(a, b)| a <= b),
            "--since must be at or before --until"
        );
        Ok(opts)
    }
    fn matches(&self, e: &Value) -> bool {
        (!self.no_shared || e["profile"].is_string())
            && (self.kinds.is_empty() || self.kinds.iter().any(|p| p.matches(field(e, "kind"))))
            && self.level.as_ref().is_none_or(|s| e["level"] == *s)
            && self
                .source
                .as_ref()
                .is_none_or(|s| (s == "server") == (e["src"] == "server"))
    }
}

fn catalog(c: &Config) -> Result<platform::Data> {
    let conn = Connection::open_with_flags(
        c.controller.state_dir.join("platform.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let tx = conn.unchecked_transaction()?;
    let raw: String = tx.query_row("SELECT data FROM platform_catalog WHERE id=1", [], |r| {
        r.get(0)
    })?;
    platform::decode_catalog(&tx, &raw)
}
type Names = BTreeMap<String, String>;
fn resolve(names: &Names, query: &str) -> Result<String> {
    if names.contains_key(query) {
        return Ok(query.into());
    }
    let mut matched = names
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(query));
    let (id, _) = matched
        .next()
        .with_context(|| format!("unknown profile '{query}'"))?;
    ensure!(
        matched.next().is_none(),
        "ambiguous profile '{query}'; use a profile ID"
    );
    Ok(id.clone())
}
fn field<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}
fn display_name(names: &Names, id: &str) -> String {
    names.get(id).cloned().unwrap_or_else(|| short(id))
}
fn line(out: &mut impl Write, e: &Value, names: &Names, zone: &TimeZone) -> Result<()> {
    let time = Timestamp::from_millisecond(e["at"].as_i64().context("missing event time")?)?
        .to_zoned(zone.clone());
    write!(
        out,
        "{} {} {} {}",
        time.strftime("%Y-%m-%d %H:%M:%S%.3f %Z"),
        field(e, "src"),
        field(e, "level"),
        field(e, "kind")
    )?;
    if let Some(id) = e["profile"].as_str() {
        write!(out, " profile={:?}", display_name(names, id))?;
    }
    for key in [
        "app",
        "build",
        "group",
        "launch",
        "attempt",
        "page",
        "visit",
        "guest",
        "inspecting",
        "seq",
    ] {
        if let Some(v) = e.get(key) {
            if let Some(id) = v.as_str() {
                write!(
                    out,
                    " {key}={}",
                    if key == "app" {
                        id.to_owned()
                    } else {
                        short(id)
                    }
                )?;
            } else {
                write!(out, " {key}={v}")?;
            }
        }
    }
    if let Some(data) = e["data"].as_object() {
        for (key, v) in data {
            match key.as_str() {
                "version" | "release" => continue,
                "session" | "tab" | "request" => {
                    write!(out, " {key}={}", short(v.as_str().unwrap_or("")))?
                }
                _ => write!(out, " {key}={v}")?,
            }
        }
    }
    writeln!(out)?;
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() || args.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        println!("{HELP}");
        return Ok(());
    }
    let now = timeline::ms();
    let config = &args[0];
    ensure!(
        !config.starts_with('-'),
        "configuration path required; see logs --help"
    );
    let mut opts = Options::parse(&args[1..], now)?;
    let c = Config::load(Path::new(config))?;
    let catalog = if c
        .controller
        .state_dir
        .join("platform.sqlite")
        .try_exists()?
    {
        catalog(&c)?
    } else {
        platform::Data::default()
    };
    if let Some(app) = &opts.app {
        ensure!(
            c.games.contains_key(app)
                || catalog.apps.contains_key(app)
                || catalog.groups.apps.contains_key(app)
                || catalog.owners.contains_key(app)
                || catalog.deployments.contains_key(app),
            "unknown game '{app}'"
        );
    }
    let names: Names = catalog
        .members
        .iter()
        .map(|(k, m)| (catalog.player_id(k), m.name.clone()))
        .collect();
    let profile = opts
        .player
        .as_deref()
        .map(|p| resolve(&names, p))
        .transpose()?;
    let path = timeline::directory(&c.controller).join("events.sqlite");
    ensure!(
        path.is_file(),
        "diagnostic store not found at {}",
        path.display()
    );
    let mut out = io::BufWriter::new(io::stdout().lock());
    let zone = if !opts.json && !opts.jsonl && opts.mode == Mode::Timeline {
        TimeZone::get("America/Los_Angeles")?
    } else {
        TimeZone::UTC
    };
    let mut report = reports::Report::default();
    let mut count = 0;
    let mut personal = false;
    if opts.json && opts.mode == Mode::Timeline {
        write!(out, "[")?;
    }
    loop {
        let batch = timeline::read_at(
            &path,
            &opts.query,
            opts.app.as_deref(),
            profile.as_deref(),
            false,
            now,
        )?;
        for e in batch["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| opts.matches(e))
        {
            personal |= e["profile"].is_string();
            match opts.mode {
                Mode::Summary => report.add_summary(e),
                Mode::Memory => report.add_sample(e),
                Mode::Timeline if opts.json => {
                    if count > 0 {
                        write!(out, ",")?;
                    }
                    write!(out, "{e}")?;
                }
                Mode::Timeline if opts.jsonl => writeln!(out, "{e}")?,
                Mode::Timeline => line(&mut out, e, &names, &zone)?,
            }
            count += 1;
        }
        let Some(next) = batch["next"].as_str() else {
            break;
        };
        opts.query.cursor = Some(next.into());
    }
    if opts.mode != Mode::Timeline {
        report.print(&mut out, &opts, &names)?;
    } else if opts.json {
        writeln!(out, "]")?;
    }
    if !opts.json && !opts.jsonl && (count == 0 || (profile.is_some() && !personal)) {
        if let Some(id) = profile {
            writeln!(out, "no events for {}", display_name(&names, &id))?;
        } else {
            writeln!(out, "no events")?;
        }
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn options(args: &str, now: i64) -> Result<Options> {
        Options::parse(
            &args
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            now,
        )
    }
    #[test]
    fn filters_and_pacific_text_at_fixed_instants() -> Result<()> {
        let now = time_arg("2026-07-14T12:00:00-07:00", 0)?;
        assert_eq!(time_arg("2026-07-14T12:00:00", 0)?, now);
        assert!(time_arg("2026-11-01T01:30:00", 0).is_err());
        let o = options(
            "--since 1d --until 30s --kind voice.* --source client --level warn --no-shared",
            now,
        )?;
        assert_eq!(o.query.since_ms, Some(now - 86_400_000));
        assert_eq!(o.query.until_ms, Some(now - 30_000));
        let mut e = json!({"at":now,"profile":"profile-id","src":"library","kind":"voice.disconnected","level":"warn","page":"abcdef12345678","received_at":now,"data":{"version":"secret-hash","state":"SIGNAL_CLOSE"}});
        assert!(o.matches(&e));
        let names = Names::from([("profile-id".into(), "Player One".into())]);
        let mut out = vec![];
        let zone = TimeZone::get("America/Los_Angeles")?;
        line(&mut out, &e, &names, &zone)?;
        let line_text = String::from_utf8(out)?;
        assert!(
            line_text.contains("12:00:00.000 PDT")
                && line_text.contains("Player One")
                && line_text.contains("page=abcdef12")
        );
        assert!(!line_text.contains("received_at") && !line_text.contains("secret-hash"));
        e["at"] = time_arg("2026-12-01T20:00:00Z", 0)?.into();
        let mut out = vec![];
        line(&mut out, &e, &names, &zone)?;
        assert!(String::from_utf8(out)?.contains("12:00:00.000 PST"));
        for (k, v) in [
            ("profile", Value::Null),
            ("kind", "api.fail".into()),
            ("src", "server".into()),
            ("level", "info".into()),
        ] {
            let mut different = e.clone();
            different[k] = v;
            assert!(!o.matches(&different));
        }
        for bad in [
            "--until nope",
            "--since -5m",
            "--kind [",
            "--source browser",
            "--level bad",
            "--game",
            "--since 2s --until 1m",
        ] {
            assert!(options(bad, now).is_err(), "{bad}");
        }
        assert!(resolve(&names, "missing").is_err());
        Ok(())
    }
    #[test]
    fn reports_preserve_error_context_and_separate_pages_and_deploys() -> Result<()> {
        let mut r = reports::Report::default();
        let mut e = json!({"kind":"js.error","level":"error","profile":"p1","app":"game","build":"build","data":{"fingerprint":"12345678","message":"example"}});
        r.add_summary(&e);
        e["kind"] = "error.repeat".into();
        e["data"]["count"] = 4.into();
        e["profile"] = "p2".into();
        r.add_summary(&e);
        for (kind, data) in [
            ("voice.stopped", json!({"state":"user_toggle"})),
            (
                "voice.disconnected",
                json!({"state":"CLIENT_INITIATED","stop_reason":"user_toggle"}),
            ),
            ("voice.disconnected", json!({"state":"SIGNAL_CLOSE"})),
            ("api.fail", json!({"status":502,"deploy_restart":true})),
            ("api.fail", json!({"status":503,"request":"shared-id"})),
            ("api.result", json!({"status":503,"request":"shared-id"})),
        ] {
            r.add_summary(&json!({"kind":kind,"data":data}));
        }
        let mut buf = vec![];
        r.print(&mut buf, &options("summary --json", 0)?, &Names::new())?;
        let v: Value = serde_json::from_slice(&buf)?;
        assert_eq!(v["top_errors"][0]["count"], 5);
        assert_eq!(v["top_errors"][0]["profiles"], json!(["p1", "p2"]));
        assert_eq!(v["voice_drops"], json!({"SIGNAL_CLOSE":1}));
        assert_eq!(v["voice_stops"], json!({"user_toggle":1}));
        assert_eq!(v["failed_calls"], json!({"503":1}));
        assert_eq!(v["deploy_restart_502"], 1);
        for (page, at, bytes) in [
            ("a", 0, 100),
            ("b", 100, 1_000_000),
            ("a", 3_600_000, 1_048_676),
            ("b", 3_600_100, 1),
        ] {
            r.add_sample(&json!({"kind":"mem.sample","page":page,"at":at,"data":{"tab":"t","heap_bytes":bytes}}));
        }
        buf.clear();
        r.print(
            &mut buf,
            &options("memory --leaks --json", 0)?,
            &Names::new(),
        )?;
        let v: Value = serde_json::from_slice(&buf)?;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["mib_per_hour"], 1.0);
        assert_eq!(v[0]["page"], "a");
        Ok(())
    }
}
