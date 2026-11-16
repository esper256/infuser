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
    platform::{
        self, Data, Database, Member,
        identity::{GOOGLE, Profile},
    },
    portal,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const EMAIL: &str = "original@example.test";
const HOST: &str = "games.example.com";

#[test]
fn rehearsal_migrates_only_a_disposable_copy() -> Result<()> {
    let root = tempfile::tempdir()?;
    legacy(root.path())?;
    let path = root.path().join("platform.sqlite");
    let original = std::fs::read(&path)?;
    assert_eq!(platform::identity::rehearse(root.path())?, 1);
    assert_eq!(std::fs::read(&path)?, original);
    let db = Database::open(root.path())?;
    let original_account = db.read()?.identities.profiles[EMAIL].account_id.clone();
    assert_eq!(platform::identity::rehearse(root.path())?, 1);
    assert_eq!(
        db.read()?.identities.profiles[EMAIL].account_id,
        original_account
    );
    Ok(())
}
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.abort();
    }
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
fn legacy(root: &Path) -> Result<()> {
    let mut data = Data::default();
    data.members.insert(
        EMAIL.into(),
        Member {
            name: "Original profile".into(),
            admin: true,
            enabled: true,
            avatar: None,
        },
    );
    data.groups.operators.insert(EMAIL.into());
    data.owners.insert("pond".into(), EMAIL.into());
    data.builds.insert("pond/one".into(), serde_json::from_value(json!({
        "manifest":{"schema_version":1,"app_id":"pond","title":"Pond","play_modes":["solo"],"solo_runtime":"client_only"},
        "server":null,"spa":false,"cross_origin_isolation":false,"ready":true,"created":now(),"files":{"index.html":4}
    }))?);
    data.apps.insert(
        "pond".into(),
        serde_json::from_value(json!({"build":"one","updated":now(),"listed":true}))?,
    );
    data.reactions.insert(
        "pond".into(),
        [(
            EMAIL.into(),
            serde_json::from_value(json!({"emoji":"star","text":"Kept", "updated":now()}))?,
        )]
        .into(),
    );
    let mut value = serde_json::to_value(&data)?;
    let profiles = value.as_object_mut().unwrap().remove("profiles").unwrap();
    value["members"] = profiles;
    value.as_object_mut().unwrap().remove("identities");
    value.as_object_mut().unwrap().remove("identity_version");
    let c = Connection::open(root.join("platform.sqlite"))?;
    c.execute_batch("CREATE TABLE platform(id INTEGER PRIMARY KEY,data TEXT NOT NULL);
        CREATE TABLE private_groups(id INTEGER PRIMARY KEY,data TEXT NOT NULL);
        CREATE TABLE saves(owner TEXT,app TEXT,slot TEXT,version INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(owner,app,slot));")?;
    c.execute("INSERT INTO platform VALUES(1,?1)", [value.to_string()])?;
    c.execute(
        "INSERT INTO private_groups VALUES(1,?1)",
        [serde_json::to_string(&data.groups)?],
    )?;
    c.execute(
        "INSERT INTO saves VALUES(?1,'pond','progress',7,'{\"level\":9}')",
        [platform::hash(EMAIL)],
    )?;
    Ok(())
}

#[tokio::test]
async fn migrate_login_change_email_and_keep_existing_game_identity_and_progress() -> Result<()> {
    let root = tempfile::tempdir()?;
    legacy(root.path())?;
    let auth = Arc::new(Mutex::new((
        "google-subject-123".to_owned(),
        EMAIL.to_owned(),
        true,
    )));
    let state = auth.clone();
    let (auth_url, _auth) = serve(Router::new().route(
        "/auth",
        get(move |h: HeaderMap| {
            let state = state.clone();
            async move {
                if h.get("cookie").is_none() {
                    return (StatusCode::UNAUTHORIZED, HeaderMap::new());
                }
                let (subject, email, include_subject) = state.lock().unwrap().clone();
                let mut headers = HeaderMap::new();
                headers.insert("x-auth-request-email", email.parse().unwrap());
                if include_subject {
                    headers.insert("x-auth-request-user", subject.parse().unwrap());
                }
                (StatusCode::ACCEPTED, headers)
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
    let (url, _server) = serve(portal::router(platform)).await?;
    let http = reqwest::Client::new();
    let request = |method, path: &str| {
        http.request(method, format!("{url}{path}"))
            .header("host", HOST)
            .header("origin", format!("https://{HOST}"))
            .header("cookie", "fixture")
    };
    let me: Value = request(reqwest::Method::GET, "/api/me")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(me["player_id"], platform::hash(EMAIL));
    assert_eq!(me["operator"], true);
    let before = db.read()?;
    let account = before.identities.profiles[EMAIL].account_id.clone();
    assert_eq!(
        before
            .identities
            .login(GOOGLE, "google-subject-123")
            .unwrap()
            .account_id,
        account
    );
    assert!(before.identities.pending_google.is_empty());
    assert_eq!(before.owners["pond"], EMAIL);
    assert_eq!(before.reactions["pond"][EMAIL].text, "Kept");
    let launch: Value = request(reqwest::Method::POST, "/api/apps/pond/launch")
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let link = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
    let host = link.host_str().unwrap();
    let token = link.fragment().unwrap().strip_prefix("infuser=").unwrap();
    http.post(format!("{url}/_infuser/session"))
        .header("host", host)
        .header("origin", format!("https://{host}"))
        .json(&json!({"token":token}))
        .send()
        .await?
        .error_for_status()?;
    let game = |path: &str| {
        http.get(format!("{url}{path}"))
            .header("host", host)
            .bearer_auth(token)
    };
    let context: Value = game("/_infuser/context")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(context["player_id"], me["player_id"]);
    auth.lock().unwrap().1 = "renamed@example.test".into();
    let renamed: Value = request(reqwest::Method::GET, "/api/me")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        renamed, me,
        "provider email must not change profile, group, avatar, or permissions"
    );
    let saved: Value = game("/_infuser/saves/progress")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(saved["version"], 7);
    assert_eq!(saved["data"]["level"], 9);
    assert_eq!(
        db.google_profile("google-subject-123", "renamed@example.test")?,
        EMAIL
    );
    assert_eq!(db.read()?.member_key("renamed@example.test")?, EMAIL);
    assert_eq!(
        Database::open(root.path())?.read()?.identities.profiles[EMAIL].account_id,
        account
    );
    // A new subject cannot steal the old email or attach itself to the new one.
    for email in [EMAIL, "renamed@example.test"] {
        *auth.lock().unwrap() = ("different-subject".into(), email.into(), true);
        assert_eq!(
            request(reqwest::Method::GET, "/api/me")
                .send()
                .await?
                .status(),
            403
        );
    }
    *auth.lock().unwrap() = ("google-subject-123".into(), EMAIL.into(), false);
    assert_eq!(
        request(reqwest::Method::GET, "/api/me")
            .header("x-infuser-verified-profile", EMAIL)
            .header("x-auth-request-user", "google-subject-123")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(db.read()?.identities.logins.len(), 1);
    // Genesis metadata is permanent; corrupt identity mappings must not be reseeded.
    let c = Connection::open(root.path().join("platform.sqlite"))?;
    let raw: String = c.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let value: Value = serde_json::from_str(&raw)?;
    assert_eq!(value["identity_version"], 1);
    assert!(value.get("members").is_none());
    let mut corrupt = value.clone();
    corrupt["identities"]["profiles"] = json!({});
    c.execute(
        "UPDATE platform SET data=?1 WHERE id=1",
        [corrupt.to_string()],
    )?;
    assert!(Database::open(root.path()).is_err());
    c.execute("UPDATE platform SET data=?1 WHERE id=1", [raw])?;
    Ok(())
}

#[test]
fn separate_profiles_require_selection_and_do_not_inherit_identity_or_group_privileges()
-> Result<()> {
    let root = tempfile::tempdir()?;
    legacy(root.path())?;
    let db = Database::open(root.path())?;
    let account = db.google_account("google-subject-123", EMAIL)?;
    let second = "profile-storage-two";
    db.edit(|d| {
        d.groups.names.insert("away".into(), "Away".into());
        d.groups.members.insert(second.into(), "away".into());
        d.members.insert(
            second.into(),
            Member {
                name: "Separate name".into(),
                avatar: None,
                admin: false,
                enabled: true,
            },
        );
        d.identities.profiles.insert(
            second.into(),
            Profile {
                id: "f".repeat(64),
                account_id: account.clone(),
            },
        );
        Ok(())
    })?;
    assert_eq!(db.google_account("google-subject-123", EMAIL)?, account);
    assert!(
        db.google_profile("google-subject-123", EMAIL)
            .unwrap_err()
            .to_string()
            .contains("Profile selection")
    );
    let d = db.read()?;
    assert_ne!(d.player_id(EMAIL), d.player_id(second));
    assert_eq!(d.members[second].name, "Separate name");
    assert!(!d.members[second].admin);
    assert!(!d.groups.allows(second, "pond"));
    assert!(
        d.operator(second),
        "operator permission belongs to account, group admin does not"
    );
    assert_eq!(db.save_read(&d.player_id(second), "pond", "progress")?.0, 0);
    assert!(
        db.edit(|d| {
            d.groups.members.insert(second.into(), "home".into());
            Ok(())
        })
        .is_err()
    );
    db.edit(|d| {
        d.identities.accounts.get_mut(&account).unwrap().enabled = false;
        Ok(())
    })?;
    let d = db.read()?;
    assert!(!d.profile_enabled(EMAIL));
    assert!(!d.profile_enabled(second));
    assert!(db.google_profile("google-subject-123", EMAIL).is_err());
    Ok(())
}

#[test]
fn newer_identity_state_is_rejected_without_a_write() -> Result<()> {
    let root = tempfile::tempdir()?;
    let db = Database::open(root.path())?;
    let c = db.connection.lock().unwrap();
    let raw: String = c.query_row("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?;
    let mut data: Value = serde_json::from_str(&raw)?;
    data["identity_version"] = json!(2);
    c.execute(
        "UPDATE platform SET data=?1 WHERE id=1",
        params![data.to_string()],
    )?;
    drop(c);
    drop(db);
    assert!(platform::identity::check_state(root.path()).is_err());
    assert!(Database::open(root.path()).is_err());
    let c = Connection::open(root.path().join("platform.sqlite"))?;
    assert_eq!(
        c.query_row::<String, _, _>("SELECT data FROM platform WHERE id=1", [], |r| r.get(0))?,
        data.to_string()
    );
    Ok(())
}
