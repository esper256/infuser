use anyhow::Result;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{
    config::Config,
    controller::{Controller, now},
    docker::Docker,
    platform::{self, Data, Database, Member, hash, member_link},
    portal,
    store::secret,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const HOST: &str = "games.example.com";
const GOB: &str = "gob@example.test";
const KID: &str = "kid@example.test";
const GOOGLE_PERSON: &str = "google@example.test";

struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct World {
    root: tempfile::TempDir,
    url: String,
    db: Arc<Database>,
    hits: Arc<AtomicUsize>,
    http: reqwest::Client,
    _auth: Running,
    _app: Running,
}

async fn serve(router: Router) -> Result<(String, Running)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    Ok((
        url,
        Running(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        })),
    ))
}

async fn world() -> Result<World> {
    let root = tempfile::tempdir()?;
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let (auth_url, auth) = serve(Router::new().route(
        "/auth",
        get(move |headers: HeaderMap| {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                let header = headers
                    .get("cookie")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                let member = header.split(';').find_map(|part| {
                    let (name, value) = part.trim().split_once('=')?;
                    (name == "member").then_some(value)
                });
                let identity = match member {
                    Some("google") => Some((GOOGLE_PERSON, "google-subject")),
                    Some("operator") => Some(("eric@example.test", "eric-subject")),
                    Some("admin") => Some(("ada@example.test", "ada-subject")),
                    _ => None,
                };
                if let Some((email, subject)) = identity {
                    let mut headers = HeaderMap::new();
                    headers.insert("x-auth-request-email", email.parse().unwrap());
                    headers.insert("x-auth-request-user", subject.parse().unwrap());
                    headers.insert(
                        "set-cookie",
                        "__Host-infuser_session=refreshed; Secure; HttpOnly; Path=/"
                            .parse()
                            .unwrap(),
                    );
                    return (StatusCode::ACCEPTED, headers);
                }
                (StatusCode::UNAUTHORIZED, HeaderMap::new())
            }
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\nprivate_groups=true",
        root.path(),
        format!("{auth_url}/auth")
    ))?;
    let platform = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let db = platform.db.clone();
    let (url, app) = serve(portal::router(platform)).await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    Ok(World {
        root,
        url,
        db,
        hits,
        http,
        _auth: auth,
        _app: app,
    })
}

fn invite(db: &Database, group: &str, email: &str, name: &str) -> Result<()> {
    db.edit(|data| {
        if group != "home" {
            data.groups.names.insert(group.into(), group.into());
        }
        data.invite_to_group(group, email, name, false, true)
    })
}

fn token_of(link: &str) -> &str {
    link.rsplit_once('/').unwrap().1
}

async fn preview_link(
    world: &World,
    link: &str,
    extra: Option<(&str, &str)>,
) -> Result<reqwest::Response> {
    let mut request = world
        .http
        .get(format!("{}/member-link/{}", world.url, token_of(link)))
        .header("host", HOST);
    if let Some((name, value)) = extra {
        request = request.header(name, value);
    }
    Ok(request.send().await?)
}

async fn open_link(world: &World, link: &str) -> Result<reqwest::Response> {
    Ok(world
        .http
        .post(format!("{}/member-link/{}", world.url, token_of(link)))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .send()
        .await?)
}

fn session_cookie(response: &reqwest::Response) -> String {
    let raw = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(raw.starts_with("__Host-infuser_member="));
    assert!(raw.contains("Secure"));
    assert!(raw.contains("HttpOnly"));
    assert!(raw.contains("SameSite=Lax"));
    assert!(raw.contains("Path=/"));
    assert!(raw.contains("Max-Age=7200"));
    assert!(!raw.to_ascii_lowercase().contains("domain="));
    raw.split(';').next().unwrap().to_owned()
}

fn assert_secret_absent(db: &Database, root: &Path, secret: &str) -> Result<()> {
    db.connection
        .lock()
        .unwrap()
        .execute_batch("PRAGMA wal_checkpoint(FULL);")?;
    let mut bytes = std::fs::read(root.join("platform.sqlite"))?;
    if let Ok(extra) = std::fs::read(root.join("platform.sqlite-wal")) {
        bytes.extend(extra);
    }
    assert!(
        !bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes()),
        "sign-in secret was written to the database"
    );
    Ok(())
}

async fn me(world: &World, cookie: &str, profile: Option<&str>) -> Result<reqwest::Response> {
    let mut request = world
        .http
        .get(format!("{}/api/me", world.url))
        .header("host", HOST)
        .header("cookie", cookie);
    if let Some(profile) = profile {
        request = request.header("x-infuser-profile", profile);
    }
    Ok(request.send().await?)
}

#[tokio::test]
async fn link_is_single_use_expires_and_revokes() -> Result<()> {
    let world = world().await?;
    invite(&world.db, "home", GOB, "Gob")?;
    let first = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    let replacement = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    assert_ne!(first, replacement);
    let replaced = open_link(&world, &first).await?;
    assert_eq!(replaced.status(), 404);
    assert!(replaced.headers().get("set-cookie").is_none());

    let prefetched = preview_link(
        &world,
        &replacement,
        Some(("sec-purpose", "prefetch;anonymous-client-ip")),
    )
    .await?;
    assert_eq!(prefetched.status(), 404);
    assert!(prefetched.headers().get("set-cookie").is_none());
    assert!(
        world
            .db
            .describe_member_link(GOB)?
            .contains("Unused links: 1")
    );

    let wrong_host = world
        .http
        .get(format!(
            "{}/member-link/{}",
            world.url,
            token_of(&replacement)
        ))
        .header("host", "pond.games.example.com")
        .send()
        .await?;
    assert_eq!(wrong_host.status(), 404);
    assert!(
        world
            .db
            .describe_member_link(GOB)?
            .contains("Unused links: 1")
    );

    let shown = preview_link(&world, &replacement, None).await?;
    assert_eq!(shown.status(), 200);
    let page = shown.text().await?;
    assert!(page.contains("Gob in Home"));
    assert!(page.contains("Continue"));
    assert!(page.contains("no-referrer"));
    assert!(!page.contains("set-cookie"));
    assert!(
        world
            .db
            .describe_member_link(GOB)?
            .contains("Unused links: 1")
    );
    let previewed = preview_link(&world, &replacement, Some(("purpose", "preview"))).await?;
    assert_eq!(previewed.status(), 404);
    assert!(
        world
            .db
            .describe_member_link(GOB)?
            .contains("Unused links: 1")
    );

    let redeemed = open_link(&world, &replacement).await?;
    assert_eq!(redeemed.status(), 303);
    assert_eq!(
        redeemed.headers().get("location").unwrap(),
        "https://games.example.com/library"
    );
    assert_eq!(
        redeemed.headers().get("referrer-policy").unwrap(),
        "no-referrer"
    );
    let cookie = session_cookie(&redeemed);
    let session_token = cookie.split_once('=').unwrap().1.to_owned();
    let body: Value = me(&world, &cookie, None)
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(body["name"], "Gob");
    let again = open_link(&world, &replacement).await?;
    assert_eq!(again.status(), 404);
    let status = world.db.describe_member_link(GOB)?;
    assert!(status.contains("Sign-in: member link"));
    assert!(status.contains("Unused links: 0"));
    assert!(status.contains("Sessions: 1"));
    assert!(!status.contains(token_of(&replacement)));
    assert!(!status.contains(session_token.as_str()));
    assert!(!status.contains("account-"));
    assert_secret_absent(&world.db, world.root.path(), token_of(&replacement))?;
    assert_secret_absent(&world.db, world.root.path(), &session_token)?;

    let expiring = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    world.db.connection.lock().unwrap().execute(
        "UPDATE member_links SET expires=0 WHERE token_hash=?1",
        params![hash(token_of(&expiring))],
    )?;
    assert_eq!(open_link(&world, &expiring).await?.status(), 404);
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_links WHERE redeemed IS NULL",
            [],
            |row| row.get(0)
        )?,
        1
    );
    world.db.cleanup()?;
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_links WHERE redeemed IS NULL",
            [],
            |row| row.get(0)
        )?,
        0
    );

    let names = world.db.revoke_member_links(GOB)?;
    assert_eq!(names, "Gob");
    assert_eq!(me(&world, &cookie, None).await?.status(), 401);
    let cleared = world.db.describe_member_link(GOB)?;
    assert!(cleared.contains("Sessions: 0"));
    assert!(cleared.contains("Unused links: 0"));
    let revoked = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    world.db.revoke_member_links(GOB)?;
    assert_eq!(open_link(&world, &revoked).await?.status(), 404);
    Ok(())
}

#[tokio::test]
async fn google_and_member_link_refuse_each_other() -> Result<()> {
    let world = world().await?;
    invite(&world.db, "home", KID, "Kid")?;
    invite(&world.db, "home", GOB, "Gob")?;
    world.db.google_account("kid-subject", KID)?;
    let refused = world
        .db
        .mint_member_link(HOST, KID, "server operator")
        .unwrap_err();
    assert!(refused.to_string().contains("Google"));
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_links",
            [],
            |row| row.get(0),
        )?,
        0
    );
    let planted = secret();
    let kid = world.db.read()?.identities.profiles[KID].account_id.clone();
    world.db.connection.lock().unwrap().execute(
        "INSERT INTO member_links(token_hash, account_id, expires) VALUES(?1,?2,?3)",
        params![hash(&planted), kid, now() + 600],
    )?;
    let response = world
        .http
        .get(format!("{}/member-link/{planted}", world.url))
        .header("host", HOST)
        .send()
        .await?;
    assert_eq!(response.status(), 403);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(
        response.json::<Value>().await?["error"],
        "This sign-in link cannot be used for this account."
    );
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_links",
            [],
            |row| row.get(0),
        )?,
        1,
        "looking at the link must not redeem it"
    );
    let consumed = world
        .http
        .post(format!("{}/member-link/{planted}", world.url))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .send()
        .await?;
    assert_eq!(consumed.status(), 403);
    assert!(consumed.headers().get("set-cookie").is_none());
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_links",
            [],
            |row| row.get(0),
        )?,
        0
    );
    let after = world.db.read()?;
    assert!(
        after
            .identities
            .logins
            .values()
            .any(|login| login.account_id == kid && login.issuer == platform::identity::GOOGLE)
    );
    assert!(
        !after
            .identities
            .logins
            .values()
            .any(|login| login.account_id == kid && login.issuer == platform::identity::LINK)
    );
    assert_eq!(
        world.db.connection.lock().unwrap().query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM member_sessions",
            [],
            |row| row.get(0),
        )?,
        0
    );

    let link = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    let cookie = session_cookie(&open_link(&world, &link).await?);
    let me = me(&world, &cookie, None).await?.json::<Value>().await?;
    assert_eq!(me["name"], "Gob");
    let claimed = world.db.google_account("gob-subject", GOB).unwrap_err();
    assert!(claimed.to_string().contains("invitation"));
    let data = world.db.read()?;
    let gob = data.identities.profiles[GOB].account_id.clone();
    assert!(data.identities.pending_google.is_empty());
    assert!(
        data.identities
            .logins
            .values()
            .all(|login| { login.account_id != gob || login.issuer == platform::identity::LINK })
    );
    assert_eq!(data.identity_version, 1);
    platform::identity::check_state(world.root.path())?;
    drop(data);
    let reopened = Database::open(world.root.path())?;
    let data = reopened.read()?;
    assert!(
        data.identities
            .logins
            .values()
            .any(|login| { login.account_id == gob && login.issuer == platform::identity::LINK })
    );
    assert_eq!(data.identity_version, 1);
    Ok(())
}

#[tokio::test]
async fn link_claims_the_invited_group_and_profile() -> Result<()> {
    let world = world().await?;
    world.db.edit(|data| {
        data.groups.names.insert("away".into(), "Away".into());
        data.invite_to_group("home", GOB, "Gob", false, true)?;
        data.invite_to_group("away", GOB, "Away Gob", false, true)
    })?;
    let away = world.db.read()?.group_member_key(GOB, "away")?.unwrap();
    let account = member_link::resolve(&world.db.read()?, GOB)?;
    assert_eq!(member_link::resolve(&world.db.read()?, &away)?, account);
    let link = world.db.mint_member_link(HOST, &away, "server operator")?.0;
    let before = world.db.describe_member_link(&away)?;
    assert!(before.contains("Sign-in: invitation pending"));
    assert!(before.contains("Gob (home)"));
    assert!(before.contains(&format!("Away Gob (away) {away}")));
    let cookie = session_cookie(&open_link(&world, &link).await?);
    let required = me(&world, &cookie, None).await?;
    assert_eq!(required.status(), 409);
    assert_eq!(required.headers()["x-infuser-profile-required"], "1");
    let profiles: Value = world
        .http
        .get(format!("{}/api/profiles", world.url))
        .header("host", HOST)
        .header("cookie", &cookie)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(profiles["profiles"].as_array().unwrap().len(), 2);
    for profile in profiles["profiles"].as_array().unwrap() {
        let selected: Value = me(
            &world,
            &cookie,
            Some(profile["player_id"].as_str().unwrap()),
        )
        .await?
        .error_for_status()?
        .json()
        .await?;
        assert_eq!(selected["name"], profile["name"]);
        assert_eq!(selected["group"]["id"], profile["group"]["id"]);
    }
    let data = world.db.read()?;
    assert!(data.identities.pending_google.is_empty());
    assert_eq!(data.contact_email(GOB), GOB);
    assert_eq!(data.contact_email(&away), GOB);
    assert_eq!(data.members[GOB].name, "Gob");
    assert_eq!(data.members[&away].name, "Away Gob");
    assert_eq!(data.groups.member(GOB), "home");
    assert_eq!(data.groups.member(&away), "away");
    assert_eq!(data.identity_version, 1);
    Ok(())
}

#[test]
fn ambiguous_accounts_need_a_profile_handle() -> Result<()> {
    let root = tempfile::tempdir()?;
    let db = Database::open(root.path())?;
    db.edit(|data| {
        data.invite_to_group("home", "ada@example.test", "Ada", false, true)?;
        data.invite_to_group("home", "bea@example.test", "Bea", false, true)
    })?;
    db.google_account("ada-subject", "ada@example.test")?;
    db.google_account("bea-subject", "bea@example.test")?;
    db.edit(|data| {
        let key = data
            .identities
            .logins
            .iter()
            .find(|(_, login)| login.email == "bea@example.test")
            .map(|(key, _)| key.clone())
            .unwrap();
        data.identities.logins.get_mut(&key).unwrap().email = "ada@example.test".into();
        Ok(())
    })?;
    let data = db.read()?;
    let error = member_link::resolve(&data, "ada@example.test").unwrap_err();
    assert!(error.to_string().contains("profile handle"));
    let bea = member_link::resolve(&data, "bea@example.test")?;
    assert_eq!(data.identities.profiles["bea@example.test"].account_id, bea);
    assert_ne!(bea, data.identities.profiles["ada@example.test"].account_id);
    Ok(())
}

#[tokio::test]
async fn google_sign_in_is_unchanged_without_the_member_cookie() -> Result<()> {
    let world = world().await?;
    invite(&world.db, "home", GOOGLE_PERSON, "Google Person")?;
    invite(&world.db, "home", GOB, "Gob")?;
    let anonymous = world
        .http
        .get(format!("{}/api/me", world.url))
        .header("host", HOST)
        .send()
        .await?;
    assert_eq!(anonymous.status(), 401);
    assert_eq!(
        anonymous.json::<Value>().await?["error"],
        "Please sign in with Google."
    );
    assert_eq!(world.hits.load(Ordering::SeqCst), 0);

    let google = world
        .http
        .get(format!("{}/api/me", world.url))
        .header("host", HOST)
        .header("cookie", "member=google")
        .send()
        .await?;
    assert_eq!(google.status(), 200);
    assert!(
        google.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("__Host-infuser_session=refreshed")
    );
    assert_eq!(google.json::<Value>().await?["name"], "Google Person");
    assert_eq!(world.hits.load(Ordering::SeqCst), 1);

    let link = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    let cookie = session_cookie(&open_link(&world, &link).await?);
    let member = me(&world, &cookie, None).await?;
    assert_eq!(member.status(), 200);
    assert!(member.headers().get("set-cookie").is_none());
    assert_eq!(member.json::<Value>().await?["name"], "Gob");
    assert_eq!(world.hits.load(Ordering::SeqCst), 1);

    let both = me(&world, &format!("{cookie}; member=google"), None).await?;
    assert_eq!(both.status(), 200);
    assert_eq!(both.json::<Value>().await?["name"], "Gob");
    assert_eq!(world.hits.load(Ordering::SeqCst), 1);

    let mixed = me(
        &world,
        "__Host-infuser_member=not-a-session; member=google",
        None,
    )
    .await?;
    assert_eq!(mixed.status(), 401);
    assert_eq!(mixed.json::<Value>().await?["error"], "Sign in again.");
    assert_eq!(world.hits.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn member_session_can_chat_and_launch_a_game() -> Result<()> {
    let world = world().await?;
    invite(&world.db, "home", GOB, "Gob")?;
    world.db.edit(|data| {
        data.builds.insert(
            "pond/one".into(),
            serde_json::from_value(json!({
                "manifest": {
                    "schema_version": 1,
                    "app_id": "pond",
                    "title": "Pond",
                    "play_modes": ["solo"],
                    "solo_runtime": "client_only",
                    "multiplayer": null
                },
                "server": null,
                "spa": false,
                "cross_origin_isolation": false,
                "ready": true,
                "created": now(),
                "files": {"index.html": 4}
            }))?,
        );
        data.apps.insert(
            "pond".into(),
            serde_json::from_value(json!({"build":"one","updated":now(),"listed":true}))?,
        );
        Ok(())
    })?;
    let link = world.db.mint_member_link(HOST, GOB, "server operator")?.0;
    let cookie = session_cookie(&open_link(&world, &link).await?);
    let api = |method: reqwest::Method, path: &str| {
        world
            .http
            .request(method, format!("{}{path}", world.url))
            .header("host", HOST)
            .header("origin", format!("https://{HOST}"))
            .header("cookie", &cookie)
    };
    let sent: Value = api(reqwest::Method::POST, "/api/chat")
        .json(&json!({"client_id":"gob-message-0001","text":"Hello from Gob"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(sent["id"].as_u64().is_some());
    let chat: Value = api(reqwest::Method::GET, "/api/chat")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(chat["messages"][0]["author"]["name"], "Gob");
    let voice: Value = api(reqwest::Method::GET, "/api/voice")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(voice["configured"], false);

    let launch: Value = api(reqwest::Method::POST, "/api/apps/pond/launch")
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let game_url = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
    let game_host = game_url.host_str().unwrap().to_owned();
    let game_token = game_url
        .fragment()
        .unwrap()
        .strip_prefix("infuser=")
        .unwrap()
        .to_owned();
    let exchange = world
        .http
        .post(format!("{}/_infuser/session", world.url))
        .header("host", &game_host)
        .header("origin", format!("https://{game_host}"))
        .json(&json!({"token": game_token}))
        .send()
        .await?;
    assert!(exchange.status().is_success());
    let game_cookie = exchange.headers()["set-cookie"].to_str()?.to_owned();
    assert!(game_cookie.starts_with(&format!("__Host-infuser_game={game_token}")));
    assert!(game_cookie.contains("HttpOnly") && game_cookie.contains("Secure"));
    let context: Value = world
        .http
        .get(format!("{}/_infuser/context", world.url))
        .header("host", &game_host)
        .header("cookie", game_cookie.split(';').next().unwrap())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(context["name"], "Gob");
    assert_eq!(context["app_id"], "pond");
    assert_eq!(
        context["player_id"],
        me(&world, &cookie, None).await?.json::<Value>().await?["player_id"]
    );
    let heartbeat: Value = world
        .http
        .post(format!("{}/_infuser/heartbeat", world.url))
        .header("host", &game_host)
        .header("origin", format!("https://{game_host}"))
        .header("cookie", game_cookie.split(';').next().unwrap())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(heartbeat["state"], "ready");
    let proxied = world
        .http
        .get(format!("{}/_infuser/proxy-auth", world.url))
        .header("host", &game_host)
        .header("cookie", game_cookie.split(';').next().unwrap())
        .send()
        .await?;
    assert_eq!(proxied.status(), 404);
    assert_eq!(
        proxied.json::<Value>().await?["error"],
        "no server for this game"
    );
    let anonymous = world
        .http
        .get(format!("{}/_infuser/proxy-auth", world.url))
        .header("host", &game_host)
        .send()
        .await?;
    assert_eq!(anonymous.status(), 401);
    Ok(())
}

#[tokio::test]
async fn operator_admin_tools_record_the_creator_and_sign_out_ends_the_session() -> Result<()> {
    let world = world().await?;
    world.db.edit(|data| {
        data.invite_to_group("home", "eric@example.test", "Eric", true, true)?;
        data.invite_to_group("home", "ada@example.test", "Ada", true, true)?;
        data.invite_to_group("home", GOB, "Gob", false, true)?;
        data.invite_to_group("home", KID, "Kid", false, true)?;
        data.set_operator("eric@example.test", true)
    })?;
    world.db.google_account("kid-subject", KID)?;
    let denied = world
        .http
        .post(format!(
            "{}/api/operator/members/gob%40example.test/sign-in-link",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=admin")
        .body("{}")
        .send()
        .await?;
    assert_eq!(denied.status(), 403);
    let ada_members: Value = world
        .http
        .get(format!("{}/api/admin/members", world.url))
        .header("host", HOST)
        .header("cookie", "member=admin")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(ada_members["members"][GOB].get("sign_in").is_none());

    let minted: Value = world
        .http
        .post(format!(
            "{}/api/operator/members/gob%40example.test/sign-in-link",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=operator")
        .body("{}")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let url = minted["url"].as_str().unwrap();
    let google_mint = world
        .http
        .post(format!(
            "{}/api/operator/members/kid%40example.test/sign-in-link",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=operator")
        .body("{}")
        .send()
        .await?;
    assert_eq!(google_mint.status(), 403);
    let members: Value = world
        .http
        .get(format!("{}/api/admin/members", world.url))
        .header("host", HOST)
        .header("cookie", "member=operator")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(members["members"][KID]["sign_in"]["kind"], "google");
    assert!(
        members["members"][KID]["sign_in"]["links"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let record = &members["members"][GOB]["sign_in"];
    assert_eq!(record["kind"], "pending");
    assert_eq!(record["links"][0]["created_by"], "Eric");
    assert!(record["links"][0]["redeemed"].is_null());
    assert_eq!(
        record["links"][0]["expires"].as_u64().unwrap()
            - record["links"][0]["created"].as_u64().unwrap(),
        30 * 60
    );
    assert_eq!(minted["expires_at"], record["links"][0]["expires"]);
    assert!(!members.to_string().contains(token_of(url)));

    let cookie = session_cookie(&open_link(&world, url).await?);
    let after: Value = world
        .http
        .get(format!("{}/api/admin/members", world.url))
        .header("host", HOST)
        .header("cookie", "member=operator")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let record = &after["members"][GOB]["sign_in"];
    assert_eq!(record["kind"], "member_link");
    assert!(record["links"][0]["redeemed"].as_u64().is_some());
    assert_eq!(record["links"][0]["created_by"], "Eric");
    assert_eq!(
        record["sessions"][0]["expires"].as_u64().unwrap()
            - record["sessions"][0]["created"].as_u64().unwrap(),
        2 * 60 * 60
    );
    let signed_out = world
        .http
        .get(format!("{}/sign-out", world.url))
        .header("host", HOST)
        .header("cookie", &cookie)
        .send()
        .await?;
    assert_eq!(signed_out.status(), 303);
    assert_eq!(signed_out.headers()["location"], "/oauth2/sign_out?rd=%2F");
    assert!(
        signed_out.headers()["set-cookie"]
            .to_str()?
            .contains("Max-Age=0")
    );
    assert_eq!(me(&world, &cookie, None).await?.status(), 401);

    let minted: Value = world
        .http
        .post(format!(
            "{}/api/operator/members/gob%40example.test/sign-in-link",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=operator")
        .body("{}")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let cookie = session_cookie(&open_link(&world, minted["url"].as_str().unwrap()).await?);
    let revoked = world
        .http
        .delete(format!(
            "{}/api/operator/members/gob%40example.test/sign-in",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=operator")
        .send()
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(me(&world, &cookie, None).await?.status(), 401);
    let admin_revoke = world
        .http
        .delete(format!(
            "{}/api/operator/members/gob%40example.test/sign-in",
            world.url
        ))
        .header("host", HOST)
        .header("origin", format!("https://{HOST}"))
        .header("cookie", "member=admin")
        .send()
        .await?;
    assert_eq!(admin_revoke.status(), 403);
    Ok(())
}

#[test]
fn existing_catalog_gains_tables_without_rewriting_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    legacy(root.path())?;
    let db = Database::open(root.path())?;
    let tables: Vec<String> = {
        let connection = db.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('member_links','member_sessions') ORDER BY name",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    assert_eq!(
        tables,
        vec!["member_links".to_string(), "member_sessions".to_string()]
    );
    let data = db.read()?;
    assert_eq!(data.identity_version, 1);
    assert_eq!(
        data.player_id("original@example.test"),
        hash("original@example.test")
    );
    assert!(
        data.identities
            .pending_google
            .contains_key("original@example.test")
    );
    drop(data);
    let account = db.google_account("stable-subject", "original@example.test")?;
    let data = db.read()?;
    assert_eq!(
        data.identities
            .login(platform::identity::GOOGLE, "stable-subject")
            .unwrap()
            .account_id,
        account
    );
    assert!(data.identities.pending_google.is_empty());
    assert_eq!(
        data.members["original@example.test"].name,
        "Original profile"
    );
    drop(data);
    platform::identity::check_state(root.path())?;
    assert_eq!(platform::identity::rehearse(root.path())?, 1);
    assert_eq!(
        Database::open(root.path())?.read()?.identity_version,
        platform::identity::VERSION
    );
    Ok(())
}

fn legacy(root: &Path) -> Result<()> {
    let email = "original@example.test";
    let mut data = Data::default();
    data.members.insert(
        email.into(),
        Member {
            name: "Original profile".into(),
            admin: true,
            enabled: true,
            avatar: None,
            avatar_prompt_seen: false,
        },
    );
    let mut value = serde_json::to_value(&data)?;
    let profiles = value.as_object_mut().unwrap().remove("profiles").unwrap();
    value["members"] = profiles;
    value.as_object_mut().unwrap().remove("identities");
    value.as_object_mut().unwrap().remove("identity_version");
    let connection = Connection::open(root.join("platform.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE platform(id INTEGER PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE private_groups(id INTEGER PRIMARY KEY, data TEXT NOT NULL);
        CREATE TABLE saves(owner TEXT, app TEXT, slot TEXT, version INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(owner, app, slot));",
    )?;
    connection.execute("INSERT INTO platform VALUES(1,?1)", [value.to_string()])?;
    connection.execute(
        "INSERT INTO private_groups VALUES(1,?1)",
        [serde_json::to_string(&data.groups)?],
    )?;
    connection.execute(
        "INSERT INTO saves VALUES(?1,'pond','progress',7,'{\"level\":9}')",
        [hash(email)],
    )?;
    Ok(())
}
