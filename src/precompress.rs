//! Gzip and Brotli siblings for large published web files.
//!
//! The portal negotiates `file.br` and `file.gz` when a player requests `file`.
//! Those siblings are not catalog entries. An uploaded sibling, such as Unity's
//! `Build/game.wasm.br`, is left untouched.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Originals smaller than this stay as uploaded.
const MIN_SOURCE_BYTES: u64 = 8 * 1024;
/// A sibling is kept only when it is at most this percent of the original.
const MAX_RATIO_PERCENT: u64 = 90;
/// And only when it saves at least this many bytes.
const MIN_SAVED_BYTES: u64 = 4 * 1024;
/// Quality 5 with a 4 MiB window. On the 39.5 MB Godot 4.7.2 web template this
/// is close to quality 11, without the minutes or the memory that would press
/// the service's 512 MiB limit. Publish also has to finish inside the
/// publisher client's two-minute timeout.
const BROTLI_QUALITY: i32 = 5;
const BROTLI_LGWIN: i32 = 22;
const GZIP_LEVEL: u32 = 6;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    pub wrote: Vec<String>,
    pub disk_skipped: Vec<String>,
}

#[derive(Clone, Copy)]
enum Coding {
    Brotli,
    Gzip,
}

impl Coding {
    fn suffix(self) -> &'static str {
        match self {
            Self::Brotli => "br",
            Self::Gzip => "gz",
        }
    }

    fn compress(self, input: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(&mut out, flate2::Compression::new(GZIP_LEVEL));
                encoder.write_all(input)?;
                encoder.finish()?;
            }
            Self::Brotli => {
                let params = brotli::enc::BrotliEncoderParams {
                    quality: BROTLI_QUALITY,
                    lgwin: BROTLI_LGWIN,
                    size_hint: input.len(),
                    ..brotli::enc::BrotliEncoderParams::default()
                };
                brotli::BrotliCompress(&mut &input[..], &mut out, &params)?;
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Decision {
    Written { bytes: u64 },
    Rejected,
    Foreign,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    secs: u64,
    nanos: u32,
    br: Decision,
    gz: Decision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Stamp {
    version: u32,
    files: BTreeMap<String, FileStamp>,
}

impl Default for Stamp {
    fn default() -> Self {
        Self {
            version: 1,
            files: BTreeMap::new(),
        }
    }
}

impl FileStamp {
    fn decision(&self, coding: Coding) -> &Decision {
        match coding {
            Coding::Brotli => &self.br,
            Coding::Gzip => &self.gz,
        }
    }

    fn set_decision(&mut self, coding: Coding, decision: Decision) {
        match coding {
            Coding::Brotli => self.br = decision,
            Coding::Gzip => self.gz = decision,
        }
    }
}

pub(crate) fn stamp_path(state_dir: &Path, build_key: &str) -> PathBuf {
    state_dir
        .join("precompress")
        .join(build_key)
        .with_extension("json")
}

pub(crate) fn remove_stamp(state_dir: &Path, build_key: &str) -> Result<()> {
    match fs::remove_file(stamp_path(state_dir, build_key)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// `strict` fails the caller when a catalogued file is missing or not a regular
/// file. Startup passes `false` so one damaged build does not stop the process.
pub(crate) fn ensure(
    dir: &Path,
    files: &BTreeMap<String, u64>,
    disk_reserve: u64,
    stamp_path: &Path,
    strict: bool,
) -> Result<Report> {
    let catalog: BTreeSet<&str> = files.keys().map(String::as_str).collect();
    let loaded = load_stamp(stamp_path);
    let mut next = Stamp::default();
    let mut report = Report::default();
    for relative in files.keys() {
        if !candidate(relative) {
            continue;
        }
        let Some(source) = inside(dir, relative) else {
            if strict {
                bail!("invalid published path: {relative}");
            }
            tracing::warn!(path = %relative, "skipping an unsafe published path");
            continue;
        };
        let meta = match fs::symlink_metadata(&source) {
            Ok(meta) if meta.file_type().is_file() => meta,
            Ok(_) => {
                if strict {
                    bail!("published file is not a regular file: {relative}");
                }
                tracing::warn!(path = %relative, "skipping a published file that is not regular");
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if strict {
                    bail!("published file is missing: {relative}");
                }
                tracing::warn!(path = %relative, "skipping a missing published file");
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let len = meta.len();
        let Some((secs, nanos)) = modified(&meta) else {
            tracing::warn!(path = %relative, "skipping a file with no modification time");
            continue;
        };
        if let Some(previous) = loaded.files.get(relative)
            && previous.len == len
            && previous.secs == secs
            && previous.nanos == nanos
            && holds(dir, relative, &catalog, Coding::Brotli, &previous.br)
            && holds(dir, relative, &catalog, Coding::Gzip, &previous.gz)
        {
            next.files.insert(relative.clone(), previous.clone());
            continue;
        }
        let mut stamp = FileStamp {
            len,
            secs,
            nanos,
            br: Decision::Rejected,
            gz: Decision::Rejected,
        };
        if len < MIN_SOURCE_BYTES {
            clear_owned(dir, relative, &catalog)?;
            next.files.insert(relative.clone(), stamp);
            continue;
        }
        let bytes = fs::read(&source).with_context(|| format!("read {relative}"))?;
        let mut blocked = false;
        for coding in [Coding::Brotli, Coding::Gzip] {
            let sibling_rel = format!("{relative}.{}", coding.suffix());
            if catalog.contains(sibling_rel.as_str()) {
                stamp.set_decision(coding, Decision::Foreign);
                continue;
            }
            let compressed = coding
                .compress(&bytes)
                .with_context(|| format!("compress {relative}"))?;
            let stored = compressed.len() as u64;
            if !worth(len, stored) {
                remove_owned(dir, &sibling_rel)?;
                stamp.set_decision(coding, Decision::Rejected);
                continue;
            }
            if !room(dir, disk_reserve, stored)? {
                blocked = true;
                report.disk_skipped.push(sibling_rel.clone());
                tracing::warn!(
                    path = %sibling_rel,
                    "left a file uncompressed; writing it would cross the disk reserve"
                );
                continue;
            }
            write_atomic(dir, &sibling_rel, &compressed)?;
            stamp.set_decision(coding, Decision::Written { bytes: stored });
            report.wrote.push(sibling_rel);
            tracing::info!(
                path = %relative,
                encoding = coding.suffix(),
                source = len,
                stored,
                "stored a precompressed web file"
            );
        }
        if !blocked {
            next.files.insert(relative.clone(), stamp);
        }
    }
    for (relative, previous) in &loaded.files {
        if files.contains_key(relative) {
            continue;
        }
        for coding in [Coding::Brotli, Coding::Gzip] {
            if matches!(previous.decision(coding), Decision::Written { .. }) {
                let sibling_rel = format!("{relative}.{}", coding.suffix());
                if !catalog.contains(sibling_rel.as_str()) {
                    remove_owned(dir, &sibling_rel)?;
                }
            }
        }
    }
    report.wrote.sort();
    report.disk_skipped.sort();
    store_stamp(stamp_path, &next)?;
    Ok(report)
}

fn candidate(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.ends_with(".br") || name.ends_with(".gz") || name.ends_with(".unityweb") {
        return false;
    }
    const EXTENSIONS: &[&str] = &[
        "wasm", "js", "mjs", "cjs", "css", "html", "json", "svg", "txt", "xml", "pck",
    ];
    EXTENSIONS.iter().any(|ext| {
        name.len() > ext.len() + 1
            && name.as_bytes()[name.len() - ext.len() - 1] == b'.'
            && name.ends_with(ext)
    })
}

fn worth(original: u64, compressed: u64) -> bool {
    original >= MIN_SOURCE_BYTES
        && compressed < original
        && original - compressed >= MIN_SAVED_BYTES
        && compressed.saturating_mul(100) <= original.saturating_mul(MAX_RATIO_PERCENT)
}

fn holds(
    dir: &Path,
    relative: &str,
    catalog: &BTreeSet<&str>,
    coding: Coding,
    decision: &Decision,
) -> bool {
    let sibling_rel = format!("{relative}.{}", coding.suffix());
    match decision {
        Decision::Foreign => catalog.contains(sibling_rel.as_str()),
        Decision::Rejected => {
            !catalog.contains(sibling_rel.as_str()) && !owned_exists(dir, &sibling_rel)
        }
        Decision::Written { bytes } => {
            !catalog.contains(sibling_rel.as_str())
                && inside(dir, &sibling_rel).is_some_and(|path| {
                    fs::symlink_metadata(&path)
                        .ok()
                        .is_some_and(|meta| meta.file_type().is_file() && meta.len() == *bytes)
                })
        }
    }
}

fn clear_owned(dir: &Path, relative: &str, catalog: &BTreeSet<&str>) -> Result<()> {
    for coding in [Coding::Brotli, Coding::Gzip] {
        let sibling_rel = format!("{relative}.{}", coding.suffix());
        if !catalog.contains(sibling_rel.as_str()) {
            remove_owned(dir, &sibling_rel)?;
        }
    }
    Ok(())
}

fn owned_exists(dir: &Path, relative: &str) -> bool {
    inside(dir, relative).is_some_and(|path| path.is_file())
}

fn remove_owned(dir: &Path, relative: &str) -> Result<()> {
    let Some(path) = inside(dir, relative) else {
        return Ok(());
    };
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn room(dir: &Path, reserve: u64, extra: u64) -> Result<bool> {
    let stat = rustix::fs::statvfs(dir)?;
    let avail = stat.f_bavail.saturating_mul(stat.f_frsize);
    Ok(avail >= reserve.saturating_add(extra))
}

fn write_atomic(dir: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let dest = inside(dir, relative).context("invalid compressed path")?;
    let parent = dest.parent().context("compressed file has no directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&dest).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn modified(meta: &fs::Metadata) -> Option<(u64, u32)> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((since.as_secs(), since.subsec_nanos()))
}

fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(dir.join(relative))
}

fn load_stamp(path: &Path) -> Stamp {
    let Ok(bytes) = fs::read(path) else {
        return Stamp::default();
    };
    match serde_json::from_slice::<Stamp>(&bytes) {
        Ok(stamp) if stamp.version == 1 => stamp,
        _ => Stamp::default(),
    }
}

fn store_stamp(path: &Path, stamp: &Stamp) -> Result<()> {
    let bytes = serde_json::to_vec(stamp)?;
    if fs::read(path).ok().as_ref() == Some(&bytes) {
        return Ok(());
    }
    let parent = path
        .parent()
        .context("precompress record has no directory")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn repetitive(len: usize) -> Vec<u8> {
        b"godot-web-engine-fixture\n"
            .iter()
            .cycle()
            .take(len)
            .copied()
            .collect()
    }

    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x1234_5678_u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 16) as u8
            })
            .collect()
    }

    fn decode_gzip(bytes: &[u8]) -> Vec<u8> {
        let mut decoder = flate2::read::GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).unwrap();
        out
    }

    fn decode_brotli(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        brotli::BrotliDecompress(&mut &bytes[..], &mut out).unwrap();
        out
    }

    fn layout(dir: &Path, name: &str, bytes: &[u8]) -> BTreeMap<String, u64> {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
        BTreeMap::from([(name.to_string(), bytes.len() as u64)])
    }

    #[test]
    fn compressible_engine_files_round_trip_and_stay_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("build");
        let wasm = repetitive(64 * 1024);
        let script = repetitive(24 * 1024);
        let pack = repetitive(24 * 1024);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.wasm"), &wasm).unwrap();
        fs::write(dir.join("index.js"), &script).unwrap();
        fs::write(dir.join("game.pck"), &pack).unwrap();
        fs::write(dir.join("icon.png"), repetitive(24 * 1024)).unwrap();
        let mut files = BTreeMap::from([
            ("index.wasm".into(), wasm.len() as u64),
            ("index.js".into(), script.len() as u64),
            ("game.pck".into(), pack.len() as u64),
            ("icon.png".into(), 24 * 1024),
        ]);
        let stamp = root.path().join("stamp.json");
        let first = ensure(&dir, &files, 0, &stamp, true).unwrap();
        assert_eq!(
            first.wrote,
            vec![
                "game.pck.br",
                "game.pck.gz",
                "index.js.br",
                "index.js.gz",
                "index.wasm.br",
                "index.wasm.gz",
            ]
        );
        assert!(first.disk_skipped.is_empty());
        assert_eq!(
            decode_brotli(&fs::read(dir.join("index.wasm.br")).unwrap()),
            wasm
        );
        assert_eq!(
            decode_gzip(&fs::read(dir.join("index.wasm.gz")).unwrap()),
            wasm
        );
        assert!(!dir.join("icon.png.br").exists());
        let before = fs::read(dir.join("index.wasm.br")).unwrap();
        let second = ensure(&dir, &files, 0, &stamp, true).unwrap();
        assert!(second.wrote.is_empty());
        assert_eq!(fs::read(dir.join("index.wasm.br")).unwrap(), before);

        let replacement = repetitive(70 * 1024);
        fs::write(dir.join("index.wasm"), &replacement).unwrap();
        files.insert("index.wasm".into(), replacement.len() as u64);
        let third = ensure(&dir, &files, 0, &stamp, true).unwrap();
        assert!(third.wrote.iter().any(|path| path == "index.wasm.br"));
        assert_eq!(
            decode_brotli(&fs::read(dir.join("index.wasm.br")).unwrap()),
            replacement
        );
    }

    #[test]
    fn small_and_incompressible_files_are_not_stored() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("build");
        let tiny = b"\0asm\x01\0\0\0".to_vec();
        let packed = noise(32 * 1024);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tiny.wasm"), &tiny).unwrap();
        fs::write(dir.join("noise.pck"), &packed).unwrap();
        let files = BTreeMap::from([
            ("tiny.wasm".into(), tiny.len() as u64),
            ("noise.pck".into(), packed.len() as u64),
        ]);
        let report = ensure(&dir, &files, 0, &root.path().join("stamp.json"), true).unwrap();
        assert!(report.wrote.is_empty());
        assert!(!dir.join("tiny.wasm.br").exists());
        assert!(!dir.join("noise.pck.gz").exists());
        let again = ensure(&dir, &files, 0, &root.path().join("stamp.json"), true).unwrap();
        assert!(again.wrote.is_empty());
    }

    #[test]
    fn an_uploaded_sibling_is_not_replaced() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("build");
        let wasm = repetitive(32 * 1024);
        fs::create_dir_all(dir.join("Build")).unwrap();
        fs::write(dir.join("Build/game.wasm"), &wasm).unwrap();
        fs::write(dir.join("Build/game.wasm.br"), b"unity-bytes").unwrap();
        let files = BTreeMap::from([
            ("Build/game.wasm".into(), wasm.len() as u64),
            ("Build/game.wasm.br".into(), 11),
        ]);
        ensure(&dir, &files, 0, &root.path().join("stamp.json"), true).unwrap();
        assert_eq!(
            fs::read(dir.join("Build/game.wasm.br")).unwrap(),
            b"unity-bytes"
        );
        assert_eq!(
            decode_gzip(&fs::read(dir.join("Build/game.wasm.gz")).unwrap()),
            wasm
        );
    }

    #[test]
    fn disk_reserve_skips_without_remembering_the_skip() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("build");
        let files = layout(&dir, "index.wasm", &repetitive(32 * 1024));
        let stamp = root.path().join("stamp.json");
        let blocked = ensure(&dir, &files, u64::MAX, &stamp, true).unwrap();
        assert!(blocked.wrote.is_empty());
        assert_eq!(
            blocked.disk_skipped,
            vec!["index.wasm.br".to_string(), "index.wasm.gz".to_string()]
        );
        assert!(!dir.join("index.wasm.br").exists());
        let written = ensure(&dir, &files, 0, &stamp, true).unwrap();
        assert_eq!(written.wrote, vec!["index.wasm.br", "index.wasm.gz"]);
    }

    #[test]
    fn a_missing_file_fails_a_publish_and_is_skipped_on_startup() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("build");
        fs::create_dir_all(&dir).unwrap();
        let files = BTreeMap::from([("index.wasm".into(), 1)]);
        let stamp = root.path().join("stamp.json");
        assert!(ensure(&dir, &files, 0, &stamp, true).is_err());
        assert!(ensure(&dir, &files, 0, &stamp, false).is_ok());
    }
}
