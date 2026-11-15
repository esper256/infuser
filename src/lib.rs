pub mod api;
pub mod caddy;
pub mod card_art;
pub mod cloud;
pub mod config;
pub mod controller;
pub mod docker;
pub mod game_urls;
pub mod launch_intent;
pub mod manifest;
pub mod native;
pub mod sdk_contract;
pub mod store;
pub mod tags;
pub mod updater;

pub const BUILD_SHA: &str = match option_env!("INFUSER_BUILD_SHA") {
    Some(value) => value,
    None => "development",
};
pub mod firewall;
pub mod peers;
pub mod platform;
pub mod portal;
pub mod private_file;
pub mod publisher;
