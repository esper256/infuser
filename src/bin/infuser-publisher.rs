//! Portable Unix publisher entry point. Hosting/operator commands stay in infuser.
use anyhow::{Result, bail};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("connect") => infuser::publisher::connect(&args).await,
        Some("publish" | "unpublish") => infuser::publisher::run(&args).await,
        Some("version" | "--version" | "-V") => {
            println!("Infuser publisher {}", infuser::BUILD_SHA);
            Ok(())
        }
        None | Some("help" | "--help" | "-h") => {
            println!(
                "Infuser publishing CLI\n\n\
                 connect --server HTTPS_ORIGIN --app APP [--output FILE] < SETUP_CODE_FILE\n\
                 publish CONNECTION.credentials.json PROJECT [--solo] [--web-dir EXPORT] [--preview]\n\
                 unpublish CONNECTION.credentials.json\n\n\
                 Same publishing options as the Linux infuser CLI.\n\
                 Private certificate authorities: --ca-cert PEM_FILE (HTTPS verification stays enabled).\n\
                 Agent guide: https://github.com/esper256/infuser/blob/main/docs/agents.md"
            );
            Ok(())
        }
        Some(command) => bail!("unknown publishing command: {command}; use --help"),
    }
}
