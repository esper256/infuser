use super::{Mode, Names, Options, display_name, field, short};
use anyhow::Result;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

#[derive(Default)]
struct ErrorGroup {
    count: u64,
    example: String,
    games: BTreeSet<String>,
    builds: BTreeSet<String>,
    profiles: BTreeSet<String>,
}
#[derive(Default)]
struct Series {
    samples: u64,
    first: (i64, u64),
    last: (i64, u64),
    peak: u64,
    mean_t: f64,
    mean_bytes: f64,
    cov: f64,
    var_t: f64,
}
#[derive(Default)]
pub(super) struct Report {
    links: BTreeMap<String, u64>,
    ops: BTreeMap<String, (Vec<u64>, usize, usize)>,
    fetches: BTreeMap<String, u64>,
    errors: BTreeMap<String, ErrorGroup>,
    stops: BTreeMap<String, u64>,
    drops: BTreeMap<String, u64>,
    failures: BTreeMap<String, u64>,
    deploy_502s: u64,
    requests: BTreeSet<String>,
    memory: BTreeMap<(String, String, String, String), Series>,
}
impl Report {
    pub(super) fn add_summary(&mut self, e: &Value) {
        let d = &e["data"];
        let kind = field(e, "kind");
        match kind {
            "link" => {
                let ms = d["down_ms"].as_u64().unwrap_or(0);
                if ms > 0 {
                    *self
                        .links
                        .entry(format!(
                            "{} / {}",
                            field(d, "name"),
                            d["down_reason"].as_str().unwrap_or("unknown")
                        ))
                        .or_default() += ms;
                }
            }
            "op" => {
                let (durations, fail, abandoned) =
                    self.ops.entry(field(d, "name").into()).or_default();
                durations.push(d["duration_ms"].as_u64().unwrap_or(0));
                *fail += usize::from(d["outcome"] == "fail");
                *abandoned += usize::from(d["outcome"] == "abandoned");
            }
            "fetch.fail" => {
                let count = d["count"].as_u64().unwrap_or(1);
                let status = d["status"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| field(d, "class").into());
                let cause = if d["deploy_restart"] == true {
                    " (deploy window)"
                } else {
                    ""
                };
                *self
                    .fetches
                    .entry(format!(
                        "{} {} {status}{cause}",
                        field(d, "method"),
                        field(d, "route")
                    ))
                    .or_default() += count;
            }

            "api.fail" | "api.result"
                if kind == "api.fail" || d["status"].as_u64().is_some_and(|s| s >= 400) =>
            {
                let first = d["request"]
                    .as_str()
                    .is_none_or(|id| self.requests.insert(id.into()));
                if d["deploy_restart"] == true {
                    if first {
                        self.deploy_502s += 1;
                    }
                    return;
                }
                if first {
                    let status = d["status"]
                        .as_u64()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "network".into());
                    *self.failures.entry(status).or_default() += 1;
                }
            }
            "voice.stopped" => *self.stops.entry(reason(d)).or_default() += 1,
            "voice.disconnected"
                if !matches!(field(d, "stop_reason"), "user_toggle" | "pagehide")
                    && !(d["stop_reason"].is_null()
                        && matches!(field(d, "state"), "CLIENT_INITIATED" | "1")
                        && e["level"] == "info") =>
            {
                *self.drops.entry(reason(d)).or_default() += 1;
            }
            _ => {}
        }
        if e["level"] == "error" || matches!(kind, "error.repeat" | "resource.error") {
            let fallback = format!(
                "{kind}|{}|{}|{}|{}|{}|{}",
                field(d, "class"),
                field(d, "message"),
                field(d, "file"),
                d["line"],
                field(d, "route"),
                d["status"]
            );
            let fingerprint = d["fingerprint"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| crate::platform::hash(&fallback)[..12].into());
            let group = self.errors.entry(fingerprint).or_default();
            group.count += d["count"].as_u64().unwrap_or(1);
            if group.example.is_empty()
                || (kind != "error.repeat" && group.example.starts_with("error.repeat"))
            {
                group.example = format!(
                    "{} {}",
                    kind,
                    d["message"].as_str().or(d["route"].as_str()).unwrap_or("")
                )
                .trim_end()
                .into();
            }
            for (key, set) in [
                ("app", &mut group.games),
                ("build", &mut group.builds),
                ("profile", &mut group.profiles),
            ] {
                if let Some(id) = e[key].as_str() {
                    set.insert(id.into());
                }
            }
        }
    }
    pub(super) fn add_sample(&mut self, e: &Value) {
        if e["kind"] != "mem.sample" && e["kind"] != "page.vitals" {
            return;
        }
        let (Some(bytes), Some(at), Some(page)) = (
            e["data"]["heap_bytes"].as_u64(),
            e["at"].as_i64(),
            e["page"].as_str(),
        ) else {
            return;
        };
        let key = (
            field(e, "profile").to_owned(),
            field(e, "app").to_owned(),
            field(&e["data"], "tab").to_owned(),
            page.into(),
        );
        let s = self.memory.entry(key).or_default();
        if s.samples == 0 {
            s.first = (at, bytes);
        }
        s.last = (at, bytes);
        s.peak = s.peak.max(bytes);
        s.samples += 1;
        let t = (at - s.first.0) as f64;
        let dt = t - s.mean_t;
        let dy = bytes as f64 - s.mean_bytes;
        s.mean_t += dt / s.samples as f64;
        s.mean_bytes += dy / s.samples as f64;
        s.var_t += dt * (t - s.mean_t);
        s.cov += dt * (bytes as f64 - s.mean_bytes);
    }
    pub(super) fn print(&self, out: &mut impl Write, opts: &Options, names: &Names) -> Result<()> {
        if opts.mode == Mode::Memory {
            let mut rows = Vec::new();
            for ((profile, app, tab, page), s) in &self.memory {
                let duration = s.last.0 - s.first.0;
                if duration <= 0 {
                    continue;
                }
                let growth = i128::from(s.last.1) - i128::from(s.first.1);
                if opts.leaks && s.cov <= 0.0 {
                    continue;
                }
                let rate = s.cov / s.var_t * 3_600_000.0 / 1_048_576.0;
                rows.push((rate,json!({"profile":profile,"app":app,"tab":tab,"page":page,"samples":s.samples,"first_ms":s.first.0,"last_ms":s.last.0,"first_bytes":s.first.1,"last_bytes":s.last.1,"peak_bytes":s.peak,"growth_bytes":growth as i64,"mib_per_hour":rate})));
            }
            rows.sort_by(|a, b| b.0.total_cmp(&a.0));
            if opts.json {
                writeln!(
                    out,
                    "{}",
                    json!(rows.iter().map(|(_, r)| r).collect::<Vec<_>>())
                )?;
            } else if rows.is_empty() {
                writeln!(out, "No qualifying memory growth trends.")?;
            } else {
                for (rate, r) in rows {
                    writeln!(
                        out,
                        "{rate:+.2} MiB/h  {:.1} → {:.1} MiB over {:.1} min  samples={} profile={:?} game={} tab={} page={}",
                        r["first_bytes"].as_f64().unwrap() / 1_048_576.0,
                        r["last_bytes"].as_f64().unwrap() / 1_048_576.0,
                        (r["last_ms"].as_i64().unwrap() - r["first_ms"].as_i64().unwrap()) as f64
                            / 60_000.0,
                        r["samples"],
                        display_name(names, field(&r, "profile")),
                        field(&r, "app"),
                        short(field(&r, "tab")),
                        short(field(&r, "page"))
                    )?;
                }
            }
            return Ok(());
        }
        let mut errors = self.errors.iter().collect::<Vec<_>>();
        errors.sort_by_key(|(key, g)| (std::cmp::Reverse(g.count), *key));
        errors.truncate(20);
        let mut ops = BTreeMap::new();
        for (name, (durations, failed, abandoned)) in &self.ops {
            let mut sorted = durations.clone();
            sorted.sort_unstable();
            let count = sorted.len();
            let nearest_rank = |percent: usize| sorted[(count * percent).div_ceil(100) - 1];
            ops.insert(
                name,
                json!({
                    "count": count, "failed": failed, "abandoned": abandoned,
                    "failure_rate": *failed as f64 / count as f64,
                    "p50_ms": nearest_rank(50), "p90_ms": nearest_rank(90)
                }),
            );
        }
        if opts.json {
            let top=errors.iter().map(|(fp,g)|json!({"fingerprint":fp,"count":g.count,"example":g.example,"games":g.games,"builds":g.builds,"profiles":g.profiles})).collect::<Vec<_>>();
            writeln!(
                out,
                "{}",
                json!({"top_errors":top,"link_downtime_ms":self.links,"operations":ops,"unreached_fetches":self.fetches,"voice_stops":self.stops,"voice_drops":self.drops,"failed_calls":self.failures,"deploy_restart_502":self.deploy_502s})
            )?;
        } else {
            writeln!(out, "Top errors")?;
            for (fp, g) in errors {
                writeln!(
                    out,
                    "  {} × [{}] {}  games={} builds={} profiles={}",
                    g.count,
                    fp,
                    g.example,
                    g.games.iter().cloned().collect::<Vec<_>>().join(","),
                    g.builds
                        .iter()
                        .map(|id| short(id))
                        .collect::<Vec<_>>()
                        .join(","),
                    g.profiles
                        .iter()
                        .map(|id| display_name(names, id))
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
            for (name, op) in &ops {
                writeln!(
                    out,
                    "{}: {} attempts, {} failed ({}%), {} abandoned, p50 {} ms, p90 {} ms",
                    name,
                    op["count"],
                    op["failed"],
                    (op["failure_rate"].as_f64().unwrap() * 100.0).round(),
                    op["abandoned"],
                    op["p50_ms"],
                    op["p90_ms"]
                )?;
            }
            for (reason, ms) in &self.links {
                writeln!(out, "Link downtime: {reason} {ms} ms")?;
            }
            for (route, count) in &self.fetches {
                writeln!(out, "Unreached: {count} × {route}")?;
            }
            for (label, counts) in [
                ("Voice stops", &self.stops),
                ("Voice drops", &self.drops),
                ("Failed calls", &self.failures),
            ] {
                writeln!(out, "{label}")?;
                for (key, count) in counts {
                    writeln!(out, "  {count} × {key}")?;
                }
            }
            if self.deploy_502s > 0 {
                writeln!(out, "Deploy restart 502s: {}", self.deploy_502s)?;
            }
        }
        Ok(())
    }
}
fn reason(d: &Value) -> String {
    d["state"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .into()
}
