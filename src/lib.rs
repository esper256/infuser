pub mod api;
pub mod backup;
pub mod caddy;
pub mod card_art;
pub mod changelog;
pub mod cloud;
pub mod config;
pub mod connections;
pub mod controller;
pub mod dedicated;
pub mod deploy_outcomes;
pub mod deployment;
pub mod docker;
pub mod game_urls;
pub mod host_health;
pub mod launch_intent;
pub mod maintenance;
pub mod manifest;
pub mod native;
pub mod network_path;
pub mod operations;
pub mod sdk_contract;
pub mod server_package;
pub mod store;
pub mod tags;
pub mod update_admission;
pub mod updater;
pub mod voice;

pub fn build_timestamp() -> u64 {
    env!("INFUSER_BUILD_TIMESTAMP")
        .parse()
        .expect("validated build timestamp")
}

pub const BUILD_SHA: &str = match option_env!("INFUSER_BUILD_SHA") {
    Some(value) => value,
    None => "development",
};
pub mod firewall;
pub mod notifications;
pub mod peers;
pub mod platform;
pub mod portal;
pub mod precompress;
pub mod private_file;
pub mod probation;
pub mod publish_capture;
pub mod publisher;
pub mod rollback;
