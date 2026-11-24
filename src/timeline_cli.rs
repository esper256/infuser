use crate::{config::Config, timeline};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
fn since(v: &str, end: i64) -> Result<i64> {
    let (n, scale) = if let Some(n) = v.strip_suffix('h') {
        (n, 3_600_000)
    } else if let Some(n) = v.strip_suffix('m') {
        (n, 60_000)
    } else {
        return Ok(v.parse()?);
    };
    let count: i64 = n.parse()?;
    ensure!(count >= 0, "negative duration");
    Ok(end.saturating_sub(count.saturating_mul(scale)))
}
fn profile(c: &Config, query: &str) -> Result<String> {
    let conn = Connection::open_with_flags(
        c.controller.state_dir.join("platform.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let tx = conn.unchecked_transaction()?;
    let raw: String = tx.query_row("SELECT data FROM platform_catalog WHERE id=1", [], |r| {
        r.get(0)
    })?;
    let d = crate::platform::decode_catalog(&tx, &raw)?;
    let mut matches = d
        .members
        .iter()
        .filter(|(key, m)| d.player_id(key) == query || m.name.eq_ignore_ascii_case(query))
        .map(|(k, _)| d.player_id(k));
    let id = matches
        .next()
        .context("unknown profile; use the display name or profile ID")?;
    ensure!(
        matches.next().is_none(),
        "ambiguous display name; use profile ID"
    );
    Ok(id)
}
pub fn run(args: &[String]) -> Result<()> {
    let c=Config::load(Path::new(args.first().context("usage: infuser logs CONFIG [--since 2h] [--until EPOCH_MS] [--profile NAME|ID] [--game APP_ID] [--jsonl]")?))?;
    let end = timeline::ms();
    let mut filter = timeline::Filter {
        until_ms: Some(end),
        ..Default::default()
    };
    let (mut app, mut player, mut jsonl) = (None, None, false);
    let mut it = args[1..].iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--jsonl" => jsonl = true,
            "--since" => filter.since_ms = Some(since(it.next().context("missing since")?, end)?),
            "--until" => filter.until_ms = Some(it.next().context("missing until")?.parse()?),
            "--game" => app = Some(it.next().context("missing app ID")?.as_str()),
            "--profile" => player = Some(profile(&c, it.next().context("missing profile")?)?),
            _ => bail!("unknown logs option {arg}"),
        }
    }
    let path = timeline::directory(&c.controller).join("events.sqlite");
    ensure!(
        path.is_file(),
        "diagnostic store not found at {}",
        path.display()
    );
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    loop {
        let batch = timeline::read(&path, &filter, app, player.as_deref(), false)?;
        for e in batch["events"].as_array().unwrap() {
            if jsonl {
                writeln!(out, "{e}")?;
            } else {
                write!(
                    out,
                    "{} {} level={} {}",
                    e["utc"].as_str().unwrap(),
                    e["src"].as_str().unwrap(),
                    e["level"].as_str().unwrap(),
                    e["kind"].as_str().unwrap()
                )?;
                for key in [
                    "profile",
                    "group",
                    "app",
                    "build",
                    "launch",
                    "attempt",
                    "page",
                    "seq",
                    "received_at",
                    "visit",
                    "guest",
                    "inspecting",
                ] {
                    if let Some(v) = e.get(key) {
                        write!(out, " {key}={v}")?;
                    }
                }
                writeln!(out, " data={}", e["data"])?;
            }
        }
        let Some(next) = batch["next"].as_str() else {
            break;
        };
        filter.cursor = Some(next.to_owned());
    }
    out.flush()?;
    Ok(())
}
