//! Opt-in local TLS fixture runner. The installed app has no test CA override.
#[allow(dead_code)]
#[path = "../src/client.rs"]
mod client;
#[path = "../src/install.rs"]
mod install;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "acceptance ROOT CA_FILE LAUNCH_URL");
    let request = client::parse(&args[3])?;
    anyhow::ensure!(
        request.server == "https://games.localhost:17878",
        "only the local fixture is allowed"
    );
    let http = reqwest::blocking::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(&args[2])?)?)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let reporter = std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            match event {
                client::Event::Status(message) => println!("{message}"),
                client::Event::Playing => println!("Game started"),
                client::Event::Done => println!("Game finished"),
                client::Event::Error(message) => eprintln!("{message}"),
            }
        }
    });
    let result = client::run_with_http(args[1].clone().into(), request, tx, http);
    reporter.join().ok();
    result
}
