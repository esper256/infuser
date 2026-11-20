fn main() {
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_TIMESTAMP");
    println!("cargo:rerun-if-changed=CHANGELOG.json");
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
