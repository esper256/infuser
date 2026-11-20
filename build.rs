fn main() {
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_TIMESTAMP");
    println!("cargo:rerun-if-changed=CHANGELOG.json");
    println!("cargo:rerun-if-changed=src/turn");
    println!("cargo:rerun-if-changed=vendor/turn");
    println!("cargo:rerun-if-changed=src/config.rs");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/private_file.rs");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!(
        "cargo:rustc-env=INFUSER_TURN_FINGERPRINT={}",
        turn_fingerprint()
    );
    let changelog = std::path::Path::new("CHANGELOG.json");
    let raw = if changelog.is_file() {
        std::fs::read_to_string(changelog).expect("read CHANGELOG.json")
    } else {
        "{\"version\":1,\"changes\":[]}\n".to_string()
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("changelog.json");
    std::fs::write(out, raw).expect("write embedded changelog");
    let timestamp = std::env::var("INFUSER_BUILD_TIMESTAMP").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    });
    assert!(timestamp.parse::<u64>().is_ok(), "invalid build timestamp");
    println!("cargo:rustc-env=INFUSER_BUILD_TIMESTAMP={timestamp}");
}

/// Stable across builds of the same relay code, so the updater can leave the
/// running relay process alone when a release does not change it.
fn turn_fingerprint() -> String {
    let mut files = Vec::new();
    for root in ["src/turn", "vendor/turn/src"] {
        let root = std::path::Path::new(root);
        if root.is_dir() {
            collect(root, &mut files);
        }
    }
    for path in ["src/config.rs", "src/main.rs", "src/private_file.rs"] {
        files.push(std::path::PathBuf::from(path));
    }
    files.sort();
    let mut hash = 0xcbf29ce484222325u64;
    for path in files {
        if let Ok(bytes) = std::fs::read(&path) {
            hash = fnv(path.to_string_lossy().as_bytes(), hash);
            hash = fnv(&bytes, hash);
        }
    }
    if let Ok(lock) = std::fs::read_to_string("Cargo.lock") {
        hash = fnv(lock_packages(&lock).as_bytes(), hash);
    }
    format!("{hash:016x}")
}

/// Name, version, and checksum lines for the crates the relay process links.
fn lock_packages(lock: &str) -> String {
    let names = ["\"turn\"", "\"stun\"", "\"webrtc-util\"", "\"tokio\""];
    let mut out = String::new();
    let mut keep = false;
    for line in lock.lines() {
        if line.starts_with("[[package]]") {
            keep = false;
        }
        if line.starts_with("name = ") {
            keep = names.iter().any(|name| line.contains(name));
        }
        if keep
            && (line.starts_with("name = ")
                || line.starts_with("version = ")
                || line.starts_with("checksum = "))
        {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn collect(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn fnv(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
