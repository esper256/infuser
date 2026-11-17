fn main() {
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=INFUSER_BUILD_TIMESTAMP");
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
