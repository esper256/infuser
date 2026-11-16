use crate::install;
use anyhow::{Context, Result, ensure};
use directories::{BaseDirs, ProjectDirs};
use infuser_native_protocol::{Artifact, Manifest};
use reqwest::{Url, blocking::Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

pub enum Event {
    Status(String),
    Playing,
    Done,
    Error(String),
}
#[derive(Clone)]
pub struct Request {
    pub server: String,
    pub id: String,
}
pub fn parse(value: &str) -> Result<Request> {
    let link = Url::parse(value)?;
    ensure!(
        link.scheme() == "infuser"
            && link.host_str() == Some("play")
            && link.username().is_empty()
            && link.password().is_none()
            && link.port().is_none()
            && link.fragment().is_none(),
        "invalid Infuser launch link"
    );
    let pairs: Vec<_> = link.query_pairs().collect();
    ensure!(
        pairs.len() == 2
            && pairs.iter().filter(|(k, _)| k == "server").count() == 1
            && pairs.iter().filter(|(k, _)| k == "request").count() == 1,
        "invalid Infuser launch parameters"
    );
    let server = Url::parse(&pairs.iter().find(|(k, _)| k == "server").unwrap().1)?;
    ensure!(
        server.scheme() == "https"
            && server.host_str().is_some()
            && server.username().is_empty()
            && server.password().is_none()
            && server.path() == "/"
            && server.query().is_none()
            && server.fragment().is_none(),
        "Infuser requires an HTTPS server origin"
    );
    let id = pairs
        .iter()
        .find(|(k, _)| k == "request")
        .unwrap()
        .1
        .to_string();
    ensure!(
        infuser_native_protocol::digest(&id),
        "invalid game launch request"
    );
    Ok(Request {
        server: server.origin().ascii_serialization(),
        id,
    })
}
pub fn root() -> Result<PathBuf> {
    let path = ProjectDirs::from("org", "Infuser", "Infuser")
        .context("cannot locate your application data directory")?
        .data_local_dir()
        .to_path_buf();
    private_dir(&path)?;
    Ok(path)
}
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn connection_path(root: &Path, server: &str) -> PathBuf {
    root.join("connections")
        .join(format!("{}.json", install::hash(server.as_bytes())))
}
pub fn connected(root: &Path, request: &Request) -> bool {
    connection_path(root, &request.server).is_file()
}
#[derive(Deserialize, Serialize)]
struct Connection {
    server: String,
    token: String,
    name: String,
}
fn private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    private_dir(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut file, value)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn http() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn response(response: reqwest::blocking::Response) -> Result<serde_json::Value> {
    let status = response.status();
    let mut bytes = Vec::new();
    response.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 256 * 1024, "Infuser response is too large");
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let code = value["error"]["code"].as_str().map(|c|format!(" ({c})")).unwrap_or_default();
    ensure!(
        status.is_success(),
        "{}{code}: {}",
        status,
        value["error"]["message"].as_str()
            .or_else(|| value["error"].as_str())
            .unwrap_or("Infuser could not open this game. Try Play again.")
    );
    Ok(value)
}
fn random() -> String {
    use rand::RngCore;
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn pair(
    http: &Client,
    root: &Path,
    request: &Request,
    events: &Sender<Event>,
) -> Result<Connection> {
    let verifier = random();
    let value = response(
        http.post(format!("{}/api/native/v1/device/start", request.server))
            .json(&json!({"challenge":install::hash(verifier.as_bytes())}))
            .send()?,
    )?;
    let id = value["request_id"]
        .as_str()
        .context("missing device request")?;
    ensure!(
        infuser_native_protocol::digest(id),
        "invalid device request"
    );
    // Construct this URL locally. A server response cannot send the browser to
    // another origin or executable URL scheme during login.
    open::that(format!("{}/desktop?pair={id}", request.server))?;
    events
        .send(Event::Status("Finish connecting in your browser…".into()))
        .ok();
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        let value = response(
            http.post(format!("{}/api/native/v1/device/token", request.server))
                .json(&json!({"request_id":id,"verifier":verifier}))
                .send()?,
        )?;
        if value["status"] == "connected" {
            let token = value["token"]
                .as_str()
                .context("missing computer credential")?
                .to_owned();
            ensure!(
                infuser_native_protocol::digest(&token),
                "invalid computer credential"
            );
            let connection = Connection {
                server: request.server.clone(),
                token,
                name: value["name"].as_str().unwrap_or("Player").into(),
            };
            private_json(&connection_path(root, &request.server), &connection)?;
            return Ok(connection);
        }
        ensure!(
            value["status"] == "pending",
            "unexpected device connection response"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    anyhow::bail!("Connection timed out. Press Play again to reconnect.")
}
pub fn forget(root: &Path, request: &Request) -> Result<()> {
    let path = connection_path(root, &request.server);
    if path.exists() {
        let connection: Connection = serde_json::from_slice(&fs::read(&path)?)?;
        ensure!(
            connection.server == request.server,
            "computer connection belongs to another server"
        );
        response(
            http()?
                .post(format!("{}/api/native/v1/device/revoke", request.server))
                .bearer_auth(&connection.token)
                .timeout(Duration::from_secs(5))
                .send()?,
        )?;
        fs::remove_file(path)?;
    }
    Ok(())
}
#[derive(Deserialize, Serialize)]
pub struct Launch {
    version: u8,
    pub app_id: String,
    pub build_id: String,
    pub title: String,
    pub player_id: String,
    pub guest: bool,
    pub launch_id: String,
    pub game_origin: String,
    pub token: String,
    pub artifact: Artifact,
}
pub fn platform() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}
pub fn run(root: PathBuf, request: Request, events: Sender<Event>) -> Result<()> {
    let http = http()?;
    run_with_http(root, request, events, http)
}
pub fn run_with_http(
    root: PathBuf,
    request: Request,
    events: Sender<Event>,
    http: Client,
) -> Result<()> {
    let path = connection_path(&root, &request.server);
    let connection: Connection = if path.exists() {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        pair(&http, &root, &request, &events)?
    };
    ensure!(
        connection.server == request.server,
        "computer connection belongs to a different server"
    );
    events.send(Event::Status("Opening your game…".into())).ok();
    let value = response(
        http.post(format!("{}/api/native/v1/claim", request.server))
            .bearer_auth(&connection.token)
            .json(&json!({"request_id":request.id,"platform":platform()}))
            .send()?,
    )?;
    let launch: Launch = serde_json::from_value(value)?;
    ensure!(
        launch.version == 1
            && infuser_native_protocol::safe_path(&launch.app_id)
            && !launch.app_id.contains('/')
            && infuser_native_protocol::digest(&launch.token),
        "invalid native launch"
    );
    Manifest {
        version: 1,
        engine: "godot".into(),
        native_only: true,
        platforms: [(platform().to_owned(), launch.artifact.clone())].into(),
    }
    .validate()?;
    let origin = Url::parse(&launch.game_origin)?;
    let server = Url::parse(&request.server)?;
    ensure!(
        origin.scheme() == "https"
            && origin.username().is_empty()
            && origin.password().is_none()
            && origin.path() == "/"
            && origin.query().is_none()
            && origin.fragment().is_none()
            && origin.port() == server.port()
            && origin
                .host_str()
                .is_some_and(|h| h.ends_with(&format!(".{}", server.host_str().unwrap()))),
        "game origin is outside this Infuser"
    );
    let site = install::hash(request.server.as_bytes());
    let player = install::hash(launch.player_id.as_bytes());
    let namespace = format!(
        "Infuser/{}/{}/{}",
        &site[..16],
        &player[..24],
        launch.app_id
    );
    let game_root = root.join("games").join(&site).join(&launch.app_id);
    private_dir(&game_root)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(game_root.join(format!("{player}.lock")))?;
    lock.try_lock()
        .context("This game is already open for this player.")?;
    let versions = game_root.join("versions");
    private_dir(&versions)?;
    let cache = versions.join(&launch.artifact.sha256);
    // The same lock covers verification, repair, run preparation and pruning.
    // Another player cannot change the cache between verification and copying.
    let cache_lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(game_root.join("install.lock"))?;
    cache_lock.lock()?;
    let verified = match install::verified(&cache, &launch.artifact) {
        Ok(verified) => verified,
        Err(_) => {
        events
            .send(Event::Status(format!("Downloading {}…", launch.title)))
            .ok();
        let download = http
            .get(format!(
                "{}/{}",
                launch.game_origin, launch.artifact.archive
            ))
            .bearer_auth(&launch.token)
            .send()?
            .error_for_status()?;
        let mut bytes = Vec::new();
        download
            .take(infuser_native_protocol::MAX_ARCHIVE + 1)
            .read_to_end(&mut bytes)?;
        events
            .send(Event::Status("Checking the update…".into()))
            .ok();
        install::install(&versions, &launch.artifact, &bytes)?;
            install::verified(&cache, &launch.artifact)?
        }
    };
    events.send(Event::Status(format!("Starting {}…", launch.title))).ok();
    let run = install::prepare_verified(verified, &game_root.join("runs"), &namespace)?;
    // A cache cleanup failure must not prevent playing an already verified game.
    let _ = crate::run_storage::prune(&game_root, &launch.artifact.sha256);
    drop(cache_lock);
    let envelope = run.path().join("infuser-launch.json");
    private_json(
        &envelope,
        &json!({"version":1,"game_origin":launch.game_origin,"token":launch.token,"save_namespace":namespace,"guest":launch.guest}),
    )?;
    run.spawning()?;
    let child = Command::new(run.path().join(&launch.artifact.entrypoint))
        .current_dir(run.path())
        .env("INFUSER_LAUNCH_FILE", &envelope)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child)=>child,
        Err(error)=>{let _=run.finished();return Err(error).context("The game could not start. Its creator may need to update this build.");}
    };
    // If the process exits immediately, keep the uncertain marker until wait
    // confirms exit; never delete a possibly-running game because inspection failed.
    let _ = run.started(child.id());
    events.send(Event::Playing).ok();
    let mut heartbeat = Instant::now() - Duration::from_secs(25);
    let exit = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if heartbeat.elapsed() >= Duration::from_secs(25) {
            heartbeat = Instant::now();
            // Session failure never changes identity or reuses an owner's saves
            // for a guest. The game receives explicit API errors on its next save.
            let _ = http
                .post(format!("{}/_infuser/api/v1/heartbeat", launch.game_origin))
                .header("Origin", &launch.game_origin)
                .bearer_auth(&launch.token)
                .timeout(Duration::from_secs(5))
                .send();
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let _ = run.finished();
    let _ = http
        .delete(format!("{}/_infuser/api/v1/heartbeat", launch.game_origin))
        .header("Origin", &launch.game_origin)
        .bearer_auth(&launch.token)
        .timeout(Duration::from_secs(5))
        .send();
    if launch.guest
        && let Some(base) = BaseDirs::new()
    {
        let mut dir = base.data_dir().to_path_buf();
        let mut safe = true;
        for part in namespace.split('/') {
            dir.push(part);
            if dir
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
            {
                safe = false;
                break;
            }
        }
        if safe && dir.is_dir() {
            let _ = fs::remove_dir_all(dir);
        }
    }
    ensure!(
        exit.success(),
        "The game stopped unexpectedly. Your installed update and personal saves have been kept."
    );
    events.send(Event::Done).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn launch_links_require_one_https_origin_and_one_nonce() {
        let id = "a".repeat(64);
        assert!(
            super::parse(&format!(
                "infuser://play?server=https%3A%2F%2Fgames.example.com&request={id}"
            ))
            .is_ok()
        );
        for server in [
            "http://example.com",
            "https://u:p@example.com",
            "https://example.com/path",
            "file:///tmp/game",
        ] {
            let mut url = reqwest::Url::parse("infuser://play").unwrap();
            url.query_pairs_mut()
                .append_pair("server", server)
                .append_pair("request", &id);
            assert!(super::parse(url.as_str()).is_err());
        }
    }
}
