//! Two families on the same real HTTP server, including hostile known-ID requests.
use anyhow::{Result, ensure};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use infuser::{config::Config, controller::Controller, docker::Docker, platform, portal};
use reqwest::{Client, Method, RequestBuilder};
use serde_json::{Value, json};
use std::sync::Arc;

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
async fn ok(r: RequestBuilder) -> Result<Value> {
    let r = r.send().await?;
    let status = r.status();
    let body: Value = r.json().await?;
    ensure!(status.is_success(), "{status}: {body}");
    Ok(body)
}
async fn denied(r: RequestBuilder) -> Result<()> {
    let r = r.send().await?;
    let status = r.status();
    let body = r.text().await?;
    ensure!(
        [401, 403, 404, 409].contains(&status.as_u16()),
        "expected denied, got {status}: {body}"
    );
    Ok(())
}
struct ClientFixture {
    url: String,
    http: Client,
}
impl ClientFixture {
    fn member(&self, who: &str, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("host", "games.example.com")
            .header("cookie", format!("member={who}"))
            .header("origin", "https://games.example.com")
    }
    fn key(&self, key: &str, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .header("host", "games.example.com")
            .bearer_auth(key)
    }
    async fn publish(&self, who: &str, tag: &str) -> Result<(String, String)> {
        let setup = ok(self
            .member(who, Method::POST, "/api/my-games")
            .json(&json!({"title":"Moon Race"})))
        .await?;
        let app = setup["app_id"].as_str().unwrap().to_owned();
        let code = setup["setup_code"]
            .as_str()
            .unwrap()
            .strip_prefix("infuser_setup_")
            .unwrap();
        let claim = ok(self
            .key("", Method::POST, "/api/publisher/claim")
            .json(&json!({"token":code,"app_id":app})))
        .await?;
        let key = claim["token"].as_str().unwrap().to_owned();
        let base = format!("/api/publisher/apps/{app}/builds/first");
        ok(self.key(&key,Method::PUT,&base).json(&json!({"manifest":{"schema_version":1,"app_id":app,"title":"Moon Race","play_modes":["solo"],"solo_runtime":"client_only"}}))).await?;
        ok(self
            .key(&key, Method::PUT, &format!("{base}/files/index.html"))
            .body("<script type=module src='/_infuser/sdk.js'></script>"))
        .await?;
        let mut art = Vec::new();
        image_webp::WebPEncoder::new(&mut art).encode(
            &vec![60; 1024 * 1024 * 3],
            1024,
            1024,
            image_webp::ColorType::Rgb8,
        )?;
        ok(self
            .key(
                &key,
                Method::PUT,
                &format!("{base}/files/infuser-art/card-hero.webp"),
            )
            .body(art))
        .await?;
        ok(self
            .key(&key, Method::POST, &format!("{base}/publish"))
            .json(&json!({"listed":true})))
        .await?;
        ok(self
            .key(
                &key,
                Method::PUT,
                &format!("/api/publisher/apps/{app}/tags"),
            )
            .json(&json!({"tags":[tag]})))
        .await?;
        Ok((app, key))
    }
}

#[tokio::test]
async fn families_are_isolated_and_operator_inspects_without_joining() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (auth, _auth) = serve(Router::new().route(
        "/auth",
        get(|h: HeaderMap| async move {
            let email = match h.get("cookie").and_then(|v| v.to_str().ok()) {
                Some("member=frizz") => "frizz@example.com",
                Some("member=admin") => "admin@example.com",
                Some("member=user") => "user@example.com",
                Some("member=artist") => "artist@example.com",
                Some("member=visitor") => "visitor@example.com",
                _ => "",
            };
            (
                if email.is_empty() {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::ACCEPTED
                },
                [
                    ("X-Auth-Request-Email", email),
                    ("X-Auth-Request-User", email),
                ],
            )
        }),
    ))
    .await?;
    let cfg: Config = toml::from_str(&format!(
        "[controller]\nstate_dir={:?}\nauth_url={:?}\ndisk_reserve_mb=64\nprivate_groups=true",
        temp.path().display().to_string(),
        format!("{auth}/auth")
    ))?;
    platform::member(&cfg, "frizz@example.com", "Frizz", true, true)?;
    platform::member(&cfg, "visitor@example.com", "Home friend", false, true)?;
    let p = portal::Platform::open(Arc::new(Controller::open(
        cfg.clone(),
        Arc::new(Docker {
            settings: cfg.controller.clone(),
        }),
    )?))?;
    let (url, _server) =
        serve(portal::router(p.clone()).merge(infuser::operations::routes(p.clone(), "control")))
            .await?;
    let c = ClientFixture {
        url,
        http: Client::new(),
    };
    // Existing admins do not automatically gain installation-wide authority.
    denied(c.member("frizz", Method::GET, "/api/operator/groups")).await?;
    denied(c.member("frizz", Method::GET, "/api/operator/health")).await?;
    denied(c.member("frizz", Method::GET, "/api/operator/update")).await?;
    denied(
        c.member("frizz", Method::POST, "/api/operator/update")
            .json(&json!({"request_id":"1".repeat(32),"current_build":infuser::BUILD_SHA})),
    )
    .await?;
    p.db.edit(|d| {
        d.groups.operators.insert("frizz@example.com".into());
        Ok(())
    })?;
    assert_eq!(p.db.read()?.groups.member("frizz@example.com"), "home");
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/operator/update")).await?["configured"],
        false
    );
    let report_url = format!("{}/v1/deployment", c.url);
    let report = json!({"release":"b".repeat(40),"phase":"waiting","maintenance_blocked":false,"built_at":123});
    assert_eq!(
        c.http
            .post(&report_url)
            .json(&report)
            .send()
            .await?
            .status(),
        401
    );
    c.http
        .post(&report_url)
        .bearer_auth("control")
        .json(&report)
        .send()
        .await?
        .error_for_status()?;
    let first = ok(c.member("frizz", Method::GET, "/api/operator/update")).await?;
    assert_eq!(first["available"], true);
    assert_eq!(first["release_built_at"], 123);
    assert_eq!(first["current_built_at"], infuser::build_timestamp());
    assert_eq!(first["publishing"], false);
    assert!(p.controller.reserve_build("update-warning-test", 1, 1)?);
    let publishing = ok(c.member("frizz", Method::GET, "/api/operator/update")).await?;
    assert_eq!(publishing["publishing"], true);
    assert!(
        publishing["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .to_ascii_lowercase()
                .contains("publishing"))
    );
    assert!(
        publishing["activity"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["holds"] == true
                    && item["kind"] == "packaging"
                    && item["summary"].as_str().unwrap().contains("server image")
                    && item["effect"]
                        .as_str()
                        .unwrap()
                        .contains("stops that build")
            })
    );
    p.controller.release_build("update-warning-test")?;

    let authorization = json!({"request_id":"1".repeat(32),"current_build":infuser::BUILD_SHA});
    ok(c.member("frizz", Method::POST, "/api/operator/update")
        .json(&authorization))
    .await?;
    // A newer staged release inherits the one-time approval, but not a fresh wait counter.
    c.http
        .post(&report_url)
        .bearer_auth("control")
        .json(&json!({"release":"c".repeat(40),"phase":"waiting"}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/operator/update")).await?["available_since"],
        first["available_since"]
    );
    let maintenance = p
        .controller
        .maintenance
        .begin("backup", || Ok(true))?
        .unwrap();
    assert_eq!(
        c.http
            .post(format!("{}/v1/deployment/admit", c.url))
            .bearer_auth("control")
            .json(&json!({"release":"c".repeat(40)}))
            .send()
            .await?
            .status(),
        409
    );
    assert!(
        p.controller.deployment.read().authorized(),
        "another maintenance lease must not consume approval"
    );
    p.controller.maintenance.end(&maintenance.id);
    let work = p.controller.maintenance.enter()?;
    let admitted: Value = c
        .http
        .post(format!("{}/v1/deployment/admit", c.url))
        .bearer_auth("control")
        .json(&json!({"release":"c".repeat(40)}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        admitted["forced"], true,
        "approval may interrupt an active request"
    );
    assert!(p.controller.deployment.read().grant.is_none());
    p.controller
        .maintenance
        .end(admitted["lease"]["id"].as_str().unwrap());
    // Retrying the same browser POST must not create a second permission.
    ok(c.member("frizz", Method::POST, "/api/operator/update")
        .json(&authorization))
    .await?;
    assert!(p.controller.deployment.read().grant.is_none());
    // The spent approval does not force another activation. An ordinary
    // in-flight request, such as a game check-in, does not block it either.
    let again: Value = c
        .http
        .post(format!("{}/v1/deployment/admit", c.url))
        .bearer_auth("control")
        .json(&json!({"release":"c".repeat(40)}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(again["forced"], false);
    p.controller
        .maintenance
        .end(again["lease"]["id"].as_str().unwrap());
    drop(work);
    p.controller
        .deployment
        .report(infuser::deployment::Report {
            release: Some("c".repeat(40)),
            built_at: None,
            phase: infuser::deployment::Phase::Current,
            maintenance_blocked: false,
        })?;
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/operator/health")).await?["configured"],
        false
    );
    std::fs::write(
        temp.path().join("host-health.json"),
        serde_json::to_vec(&infuser::host_health::Report::default())?,
    )?;
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/operator/health")).await?["stale"],
        true
    );
    let create = |name: &str| {
        c.member("frizz", Method::POST, "/api/operator/groups")
            .json(
                &json!({"name":name,"admin_email":"admin@example.com","admin_name":"Other admin"}),
            )
    };
    let group = ok(create("Second group")).await?["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rename = |actor: &str, id: &str, name: &str| {
        c.member(actor, Method::PUT, &format!("/api/operator/groups/{id}"))
            .json(&json!({"name":name}))
    };
    assert_eq!(
        rename("admin", &group, "Forbidden").send().await?.status(),
        403
    );
    let rename_request = |origin: &str| {
        c.http
            .put(format!("{}/api/operator/groups/{group}", c.url))
            .header("host", "games.example.com")
            .header("origin", origin)
            .json(&json!({"name":"Forbidden"}))
    };
    assert_eq!(
        rename_request("https://games.example.com")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        rename_request("https://evil.example")
            .header("cookie", "member=frizz")
            .send()
            .await?
            .status(),
        403
    );
    denied(rename("frizz", &group, "Forbidden").header("x-infuser-inspect-group", &group)).await?;
    assert_eq!(
        rename("frizz", "missing", "New").send().await?.status(),
        404
    );
    for invalid in [
        "  ",
        "Control\ncharacter",
        &"x".repeat(61),
        "\u{200b}",
        "\u{3164}",
        "\u{2800}",
        "A\u{202e}B",
    ] {
        for request in [create(invalid), rename("frizz", &group, invalid)] {
            let response = request.send().await?;
            assert_eq!(response.status(), 422);
            assert_eq!(
                response.json::<Value>().await?["error"],
                "Use a group name of 1–60 characters."
            );
        }
    }
    for valid in [
        &"x".repeat(60),
        &"🎮".repeat(60),
        "Familia Pérez",
        "日本のクラブ",
        "🎮",
        "Straße",
    ] {
        let response = rename("frizz", &group, valid).send().await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().await?["name"], valid);
    }
    for request in [rename("frizz", "home", " STRASSE "), create(" STRASSE ")] {
        let response = request.send().await?;
        assert_eq!(response.status(), 409);
        assert_eq!(
            response.json::<Value>().await?["error"],
            "Another group already uses that name."
        );
    }
    for own_name in ["Home", "home"] {
        let response = rename("frizz", "home", own_name).send().await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().await?["name"], own_name);
    }
    assert_eq!(
        ok(rename("frizz", "home", "Family Arcade")).await?,
        json!({"id":"home","name":"Family Arcade"})
    );
    assert_eq!(
        ok(rename("frizz", &group, " Other family ")).await?,
        json!({"id":group,"name":"Other family"})
    );
    assert_eq!(
        ok(c.member("admin", Method::GET, "/api/me")).await?["group"],
        json!({"id":group,"name":"Other family"})
    );
    ok(c.member("admin", Method::POST, "/api/admin/members").json(
        &json!({"email":"user@example.com","name":"Other user","limited_communication":true}),
    ))
    .await?;
    assert_eq!(
        rename("user", &group, "Forbidden").send().await?.status(),
        403
    );
    denied(c.member("admin", Method::GET, "/api/operator/groups")).await?;
    denied(c.member("admin", Method::GET, "/api/operator/health")).await?;
    denied(
        c.member("admin", Method::GET, "/api/library")
            .header("x-infuser-inspect-group", "home"),
    )
    .await?;
    // Cross-group invitations now create independent profiles; that flow is
    // exercised in profile_chooser_journey. Renaming another group's profile
    // remains forbidden here.
    denied(
        c.member("admin", Method::PUT, "/api/admin/members/handle")
            .json(&json!({"email":"frizz@example.com","name":"Renamed"})),
    )
    .await?;
    assert_eq!(
        c.member("admin", Method::POST, "/api/admin/members")
            .json(&json!({"email":"new@example.com","name":"Escalated","operator":true}))
            .send()
            .await?
            .status(),
        422
    );
    let (home, home_key) = c.publish("frizz", "home-special").await?;
    let (away, away_key) = c.publish("user", "away-special").await?;
    // The invited profile can sign in, publish a full game and see emoji feedback.
    let communication = "/api/admin/members/communication";
    let access =
        |email: &str, limited: bool| json!({"email":email,"limited_communication":limited});
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/me")).await?["limited_communication"],
        true
    );
    denied(
        c.member("user", Method::PUT, communication)
            .json(&access("user@example.com", false)),
    )
    .await?;
    denied(
        c.member("admin", Method::PUT, communication)
            .json(&access("frizz@example.com", true)),
    )
    .await?;
    denied(c.member("user", Method::GET, "/api/chat")).await?;
    denied(
        c.member("user", Method::POST, "/api/chat")
            .json(&json!({"client_id":"limited-message-test","text":"Hi"})),
    )
    .await?;
    denied(
        c.member("user", Method::POST, "/api/voice")
            .json(&json!({"id":"valid-voice-lease-request"})),
    )
    .await?;
    let reaction_path = format!("/api/apps/{away}/reaction");
    ok(c.member("admin", Method::PUT, &reaction_path)
        .json(&json!({"emoji":"⭐","text":"A private reaction note"})))
    .await?;
    let filtered = ok(c.member("user", Method::GET, "/api/library")).await?;
    assert_eq!(filtered["apps"][0]["reactions"][0]["emoji"], "⭐");
    assert_eq!(filtered["apps"][0]["reactions"][0]["text"], "");
    assert!(!filtered.to_string().contains("A private reaction note"));
    denied(
        c.member("user", Method::PUT, &reaction_path)
            .json(&json!({"emoji":"⭐","text":"A written note"})),
    )
    .await?;
    ok(c.member("user", Method::PUT, &reaction_path)
        .json(&json!({"emoji":"🚀","text":""})))
    .await?;
    ok(c.member("user", Method::DELETE, &reaction_path)).await?;
    ok(c.member("admin", Method::PUT, communication)
        .json(&access("user@example.com", false)))
    .await?;
    ok(c.member("user", Method::GET, "/api/chat")).await?;
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/library")).await?["apps"][0]["reactions"][0]["text"],
        "A private reaction note"
    );
    ok(c.member("admin", Method::DELETE, &reaction_path)).await?;
    // Ordered display credits are admin-only and never transfer publishing rights.
    ok(c.member("admin", Method::POST, "/api/admin/members")
        .json(&json!({"email":"artist@example.com","name":"Collaborator"})))
    .await?;
    let artist_id = p.db.read()?.player_id("artist@example.com");
    let author_path = format!("/api/admin/apps/{away}/authors");
    let user_id = p.db.read()?.player_id("user@example.com");
    let admin_id = p.db.read()?.player_id("admin@example.com");
    let foreign_id = p.db.read()?.player_id("frizz@example.com");
    let initial = ok(c.member("user", Method::GET, "/api/library")).await?;
    assert_eq!(initial["apps"][0]["authors"][0]["id"], user_id);
    let credentials_before = serde_json::to_value(&p.db.read()?.credentials)?;
    let inventory = ok(c.member("admin", Method::GET, "/api/admin/game-authors")).await?;
    assert_eq!(inventory["games"].as_array().unwrap().len(), 1);
    assert_eq!(inventory["games"][0]["id"], away);
    assert!(!inventory.to_string().contains(&foreign_id));
    denied(c.member("user", Method::GET, "/api/admin/game-authors")).await?;
    for who in ["user", "artist", "frizz", ""] {
        denied(
            c.member(who, Method::PUT, &author_path)
                .json(&json!({"authors":[user_id]})),
        )
        .await?;
    }
    denied(
        c.member("admin", Method::PUT, &author_path)
            .header("origin", "https://elsewhere.example")
            .json(&json!({"authors":[user_id]})),
    )
    .await?;
    denied(
        c.member("admin", Method::PUT, &author_path)
            .header("x-infuser-inspect-group", "home")
            .json(&json!({"authors":[user_id]})),
    )
    .await?;
    for invalid in [
        json!([]),
        json!([user_id, user_id]),
        json!([foreign_id]),
        json!(["missing"]),
    ] {
        assert_eq!(
            c.member("admin", Method::PUT, &author_path)
                .json(&json!({"authors":invalid}))
                .send()
                .await?
                .status(),
            400
        );
    }
    for ids in [
        json!([admin_id, user_id]),
        json!([user_id, admin_id]),
        json!([artist_id]),
    ] {
        let saved = ok(c
            .member("admin", Method::PUT, &author_path)
            .json(&json!({"authors":ids})))
        .await?;
        assert_eq!(
            saved["authors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["id"].clone())
                .collect::<Vec<_>>(),
            *ids.as_array().unwrap()
        );
        let public = ok(c.member("user", Method::GET, "/api/library")).await?;
        assert_eq!(public["apps"][0]["authors"], saved["authors"]);
        assert_eq!(public["apps"][0]["author"], saved["authors"][0]);
        assert_eq!(
            ok(c.member("artist", Method::GET, "/api/library")).await?["apps"][0]["can_edit_tags"],
            false,
            "credit does not grant tag-editing rights"
        );
        assert_eq!(
            public["apps"][0]["can_edit_tags"], true,
            "owner keeps authoring permissions after removal from credits"
        );
    }
    ok(c.member("admin", Method::PUT, &author_path)
        .json(&json!({"authors":[admin_id,user_id]})))
    .await?;
    assert_eq!(p.db.read()?.owners[&away], "user@example.com");
    assert_eq!(
        serde_json::to_value(&p.db.read()?.credentials)?,
        credentials_before
    );
    ok(c.key(&away_key, Method::GET, "/api/publisher/capabilities")).await?;
    // Publishing another build must retain the explicit credits and their order.
    let next_build = format!("/api/publisher/apps/{away}/builds/second");
    ok(c.key(&away_key, Method::PUT, &next_build).json(&json!({"manifest":{"schema_version":1,"app_id":away,"title":"Moon Race","play_modes":["solo"],"solo_runtime":"client_only"}}))).await?;
    ok(c.key(
        &away_key,
        Method::PUT,
        &format!("{next_build}/files/index.html"),
    )
    .body("<script type=module src='/_infuser/sdk.js'></script>"))
    .await?;
    let mut updated_art = Vec::new();
    image_webp::WebPEncoder::new(&mut updated_art).encode(
        &vec![70; 1024 * 1024 * 3],
        1024,
        1024,
        image_webp::ColorType::Rgb8,
    )?;
    ok(c.key(
        &away_key,
        Method::PUT,
        &format!("{next_build}/files/infuser-art/card-hero.webp"),
    )
    .body(updated_art))
    .await?;
    ok(
        c.key(&away_key, Method::POST, &format!("{next_build}/publish"))
            .json(&json!({"listed":true})),
    )
    .await?;
    assert_eq!(
        p.db.read()?.author_credits[&away],
        vec!["admin@example.com", "user@example.com"]
    );
    // Same title/slug is independent; publisher codes automatically inherit their family.
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/games/moon-race")).await?["app_id"],
        home
    );
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/games/moon-race")).await?["app_id"],
        away
    );
    assert_eq!(p.db.read()?.groups.app(&away), group);
    ok(c.member("frizz", Method::POST, "/api/chat")
        .json(&json!({"client_id":"home-message-00001","text":"Home only"})))
    .await?;
    let message = ok(c
        .member("user", Method::POST, "/api/chat")
        .json(&json!({"client_id":"away-message-00001","text":"Other family only"})))
    .await?;
    ok(c.member("user", Method::PUT, "/api/profile")
        .json(&json!({"avatar":"circuit-fox"})))
    .await?;
    let user_identity = ok(c.member("user", Method::GET, "/api/me")).await?;
    // The shipped UI can load every illustrated reaction from the real server.
    for id in [
        "atmosphere",
        "cozy",
        "gem",
        "clever",
        "wild",
        "surprises",
        "art",
        "music",
        "smile",
        "together",
        "fire",
        "spooky",
        "trophy",
        "celebrate",
    ] {
        let response = c
            .member("user", Method::GET, &format!("/assets/reactions/{id}.webp"))
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/webp");
        let bytes = response.bytes().await?;
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }
    assert_eq!(
        c.member("user", Method::GET, "/assets/reactions/unknown.webp")
            .send()
            .await?
            .status(),
        404
    );
    // Every expressive reaction round-trips through the real group-scoped API.
    for emoji in [
        "😅",
        "⚡",
        "🧠",
        "🎯",
        "🧭",
        "🧲",
        "🐦‍⬛",
        "🍵",
        "💎",
        "🧩",
        "🎢",
        "🪄",
        "🎨",
        "🎵",
        "😄",
        "🤝",
        "🔥",
        "👻",
        "🏆",
        "🎉",
    ] {
        ok(
            c.member("user", Method::PUT, &format!("/api/apps/{away}/reaction"))
                .json(&json!({"emoji":emoji,"text":"A new challenge!"})),
        )
        .await?;
        let library = ok(c.member("user", Method::GET, "/api/library")).await?;
        assert_eq!(library["apps"][0]["reactions"][0]["emoji"], emoji);
        assert_eq!(
            library["apps"][0]["reactions"][0]["text"],
            "A new challenge!"
        );
    }
    assert_eq!(
        c.member("user", Method::PUT, &format!("/api/apps/{away}/reaction"))
            .json(&json!({"emoji":"😡","text":""}))
            .send()
            .await?
            .status(),
        400
    );

    ok(
        c.member("user", Method::PUT, &format!("/api/apps/{away}/reaction"))
            .json(&json!({"emoji":"🚀","text":"Our game"})),
    )
    .await?;
    // Per-user starred games stay private to the user, respect group boundaries, and toggle cleanly.
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/library")).await?["apps"][0]["starred"],
        false
    );
    assert_eq!(
        ok(c.member("user", Method::PUT, &format!("/api/apps/{away}/star"))).await?["starred"],
        true
    );
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/library")).await?["apps"][0]["starred"],
        true
    );
    // Another member in the same group (or another group) does not see user's star, nor can user star a foreign game.
    assert_eq!(
        ok(c.member("frizz", Method::GET, "/api/library")).await?["apps"][0]["starred"],
        false
    );
    assert_eq!(
        c.member("user", Method::PUT, &format!("/api/apps/{home}/star"))
            .send()
            .await?
            .status(),
        404
    );
    assert_eq!(
        ok(c.member("user", Method::DELETE, &format!("/api/apps/{away}/star"))).await?["starred"],
        false
    );
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/library")).await?["apps"][0]["starred"],
        false
    );
    for (who, own, foreign, own_tag, foreign_tag) in [
        ("frizz", &home, &away, "home-special", "away-special"),
        ("user", &away, &home, "away-special", "home-special"),
    ] {
        let library = ok(c.member(who, Method::GET, "/api/library")).await?;
        assert_eq!(library["apps"].as_array().unwrap().len(), 1);
        assert_eq!(library["apps"][0]["id"], *own);
        let reactions = library["apps"][0]["reactions"].as_array().unwrap();
        if who == "user" {
            assert_eq!(reactions.len(), 1);
            assert_eq!(reactions[0]["id"], user_identity["player_id"]);
            assert_eq!(reactions[0]["avatar"], "circuit-fox");
            assert!(reactions[0].get("email").is_none());
        } else {
            assert!(reactions.is_empty(), "Other groups' reactions stay private");
        }
        let serialized = library.to_string();
        assert!(serialized.contains(own_tag));
        assert!(!serialized.contains(foreign_tag));
        assert!(!serialized.contains(foreign));
        for (method, path, body) in [
            (
                Method::POST,
                format!("/api/apps/{foreign}/launch"),
                json!({"guest":true}),
            ),
            (
                Method::POST,
                format!("/api/native/v1/play/{foreign}"),
                json!({"platform":"linux-x86_64","guest":true}),
            ),
            (
                Method::PUT,
                format!("/api/apps/{foreign}/reaction"),
                json!({"emoji":"🚀","text":"Intrusion"}),
            ),
            (
                Method::DELETE,
                format!("/api/apps/{foreign}/reaction"),
                json!({}),
            ),
            (
                Method::PUT,
                format!("/api/apps/{foreign}/tags"),
                json!({"tags":["stolen"]}),
            ),
            (
                Method::PUT,
                format!("/api/apps/{foreign}/slug"),
                json!({"slug":"stolen"}),
            ),
            (
                Method::POST,
                format!("/api/my-games/{foreign}/setup"),
                json!({}),
            ),
            (
                Method::GET,
                format!("/api/apps/{foreign}/card-art/first"),
                json!({}),
            ),
        ] {
            denied(c.member(who, method, &path).json(&body)).await?;
        }
    }
    // Only a home-group admin may moderate by the opaque reaction author ID.
    let target = user_identity["player_id"].as_str().unwrap();
    let moderation = format!("/api/admin/apps/{away}/reactions/{target}");
    for who in ["user", "frizz", ""] {
        denied(c.member(who, Method::DELETE, &moderation)).await?;
    }
    denied(
        c.member("admin", Method::DELETE, &moderation)
            .header("origin", "https://elsewhere.example"),
    )
    .await?;
    let admin_library = ok(c.member("admin", Method::GET, "/api/library")).await?;
    assert_eq!(admin_library["apps"][0]["can_moderate_reactions"], true);
    assert_eq!(admin_library["apps"][0]["can_manage_visibility"], true);
    assert_eq!(
        ok(c.member("user", Method::GET, "/api/library")).await?["apps"][0]["can_moderate_reactions"],
        false
    );
    ok(
        c.member("admin", Method::PUT, &format!("/api/apps/{away}/reaction"))
            .json(&json!({"emoji":"⭐","text":"Keep this reaction"})),
    )
    .await?;
    p.db.edit(|d| {
        d.apps.get_mut(&away).unwrap().listed = false;
        Ok(())
    })?;
    ok(c.member("admin", Method::DELETE, &moderation)).await?;
    ok(c.member("admin", Method::DELETE, &moderation)).await?; // safe repeated removal
    let remaining = ok(c.member("admin", Method::GET, "/api/library")).await?;
    assert_eq!(
        remaining["apps"][0]["reactions"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        remaining["apps"][0]["reactions"][0]["text"],
        "Keep this reaction"
    );
    p.db.edit(|d| {
        d.apps.get_mut(&away).unwrap().listed = true;
        Ok(())
    })?;
    let members = ok(c.member("admin", Method::GET, "/api/admin/members"))
        .await?
        .to_string();
    assert!(members.contains("Other user"));
    assert!(!members.contains("Frizz"));
    for (method, path, body) in [
        (
            Method::POST,
            "/api/admin/credentials".to_owned(),
            json!({"app":home,"label":"Steal"}),
        ),
        (
            Method::PUT,
            format!("/api/admin/apps/{home}/visibility"),
            json!({"listed":false}),
        ),
    ] {
        denied(c.member("admin", method, &path).json(&body)).await?;
    }
    let home_chat = ok(c.member("frizz", Method::GET, "/api/chat"))
        .await?
        .to_string();
    assert!(home_chat.contains("Home only"));
    assert!(!home_chat.contains("Other family only"));
    let away_chat = ok(c.member("user", Method::GET, "/api/chat"))
        .await?
        .to_string();
    assert!(away_chat.contains("Other family only"));
    assert!(!away_chat.contains("Home only"));
    denied(c.member(
        "frizz",
        Method::DELETE,
        &format!("/api/chat/{}", message["id"]),
    ))
    .await?;
    // Scoped service keys cannot enumerate or edit another group's games.
    let (scoped, global, global_id) = p.db.edit(|d| {
        let scoped = platform::credential(d, &group, "metadata-group", "Other art")?.1;
        let (id, global) = platform::credential(d, "all-games", "metadata", "Installation art")?;
        Ok((scoped, global, id))
    })?;
    assert_eq!(
        ok(c.key(&scoped, Method::GET, "/api/metadata/apps")).await?["apps"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(c.key(&global, Method::GET, "/api/metadata/apps")).await?["apps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for key in [&scoped, &away_key] {
        denied(
            c.key(key, Method::PATCH, &format!("/api/metadata/apps/{home}"))
                .json(&json!({"title":"Intrusion"})),
        )
        .await?;
    }
    denied(
        c.key(
            &home_key,
            Method::PUT,
            &format!("/api/publisher/apps/{away}/tags"),
        )
        .json(&json!({"tags":["intrusion"]})),
    )
    .await?;
    denied(c.member(
        "admin",
        Method::DELETE,
        &format!("/api/admin/credentials/{global_id}"),
    ))
    .await?;
    assert!(
        !ok(c.member("admin", Method::GET, "/api/admin/credentials"))
            .await?
            .to_string()
            .contains(&global_id)
    );
    // Browser launch exchange and cloud saves remain bound to the group/game/player.
    let mut sessions = Vec::new();
    for (who, app) in [("frizz", &home), ("user", &away)] {
        let launch = ok(c
            .member(who, Method::POST, &format!("/api/apps/{app}/launch"))
            .json(&json!({})))
        .await?;
        let link = reqwest::Url::parse(launch["url"].as_str().unwrap())?;
        let host = link.host_str().unwrap().to_owned();
        let ticket = link
            .fragment()
            .unwrap()
            .strip_prefix("infuser=")
            .unwrap()
            .to_owned();
        ok(c.http
            .post(format!("{}/_infuser/session", c.url))
            .header("host", &host)
            .header("origin", format!("https://{host}"))
            .json(&json!({"token":ticket})))
        .await?;
        sessions.push((host, ticket));
    }
    let game = |n: usize, method, path: &str| {
        c.http
            .request(method, format!("{}{path}", c.url))
            .header("host", &sessions[n].0)
            .header("origin", format!("https://{}", sessions[n].0))
            .bearer_auth(&sessions[n].1)
    };
    ok(game(0, Method::PUT, "/_infuser/saves/progress")
        .json(&json!({"version":0,"data":{"stars":7}})))
    .await?;
    assert_eq!(
        ok(game(1, Method::GET, "/_infuser/saves/progress")).await?["version"],
        0
    );
    for path in [
        "/_infuser/context",
        "/_infuser/presence/v1",
        "/_infuser/saves/progress",
        "/index.html",
    ] {
        denied(
            c.http
                .get(format!("{}{path}", c.url))
                .header("host", &sessions[1].0)
                .bearer_auth(&sessions[0].1),
        )
        .await?;
    }
    let home_presence = ok(game(0, Method::GET, "/_infuser/presence/v1"))
        .await?
        .to_string();
    let away_presence = ok(game(1, Method::GET, "/_infuser/presence/v1"))
        .await?
        .to_string();
    assert!(!home_presence.contains("Other user"));
    assert!(!away_presence.contains("Frizz"));
    // Even an old exchanged session is fenced if an operator changes membership offline.
    p.db.edit(|d| {
        d.groups
            .members
            .insert("user@example.com".into(), "home".into());
        Ok(())
    })?;
    denied(game(1, Method::GET, "/_infuser/saves/progress")).await?;
    denied(c.key(&away_key, Method::GET, "/api/publisher/capabilities")).await?;
    p.db.edit(|d| {
        d.groups
            .members
            .insert("user@example.com".into(), group.clone());
        Ok(())
    })?;
    // Inspector can see target content but cannot interact or create presence there.
    let before = p.db.read()?;
    let inspect = |method, path: &str| {
        c.member("frizz", method, path)
            .header("x-infuser-inspect-group", &group)
    };
    let me = ok(inspect(Method::GET, "/api/me")).await?;
    assert_eq!(me["name"], "Frizz");
    assert_eq!(me["inspecting"], true);
    assert_eq!(me["admin"], false);
    let library = ok(inspect(Method::GET, "/api/library")).await?;
    assert_eq!(library["apps"][0]["id"], away);
    assert!(!library["friends"].to_string().contains("Frizz"));
    assert_eq!(library["apps"][0]["can_edit_tags"], false);
    assert!(
        c.member(
            "frizz",
            Method::GET,
            library["apps"][0]["card_art_url"].as_str().unwrap()
        )
        .send()
        .await?
        .status()
        .is_success()
    );
    denied(c.member(
        "visitor",
        Method::GET,
        library["apps"][0]["card_art_url"].as_str().unwrap(),
    ))
    .await?;
    let chat = ok(inspect(Method::GET, "/api/chat")).await?;
    assert_eq!(chat["messages"][0]["can_delete"], false);
    assert!(chat.to_string().contains("Other family only"));
    assert_eq!(library["apps"][0]["can_moderate_reactions"], false);
    assert_eq!(library["apps"][0]["can_manage_visibility"], false);
    for (method, path, body) in [
        (Method::DELETE, moderation.clone(), json!({})),
        (Method::POST, "/api/my-games".to_owned(), json!({})),
        (
            Method::POST,
            "/api/chat".to_owned(),
            json!({"client_id":"inspection-000001","text":"Should not post"}),
        ),
        (
            Method::PUT,
            "/api/profile".to_owned(),
            json!({"avatar":"circuit-fox"}),
        ),
        (Method::POST, format!("/api/apps/{away}/launch"), json!({})),
        (
            Method::PUT,
            format!("/api/apps/{away}/reaction"),
            json!({"emoji":"🚀","text":"Oops"}),
        ),
        (Method::GET, "/api/admin/credentials".to_owned(), json!({})),
        (
            Method::GET,
            "/api/launches/nonexistent".to_owned(),
            json!({}),
        ),
    ] {
        denied(inspect(method, &path).json(&body)).await?;
    }
    let after = p.db.read()?;
    assert_eq!(before.presence, after.presence);
    assert_eq!(before.launches.len(), after.launches.len());
    assert_eq!(after.members["frizz@example.com"].name, "Frizz");
    assert_eq!(after.groups.member("frizz@example.com"), "home");
    // Persistence, legacy catalog updates, and fail-closed downgrade configuration.
    platform::member(
        &cfg,
        "visitor@example.com",
        "Home friend updated",
        false,
        true,
    )?;
    let reopened = platform::Database::open(temp.path())?;
    assert_eq!(reopened.read()?.groups.member("user@example.com"), group);
    assert!(reopened.read()?.operator("frizz@example.com"));
    assert_eq!(
        reopened.read()?.author_credits[&away],
        vec!["admin@example.com", "user@example.com"]
    );
    let mut unsafe_cfg = cfg.clone();
    unsafe_cfg.controller.private_groups = false;
    let disabled_state = tempfile::tempdir()?;
    unsafe_cfg.controller.state_dir = disabled_state.path().into();
    platform::Database::open(disabled_state.path())?.edit(|d| {
        d.groups.names.insert("other".into(), "Other".into());
        Ok(())
    })?;
    assert!(
        portal::Platform::open(Arc::new(Controller::open(
            unsafe_cfg.clone(),
            Arc::new(Docker {
                settings: unsafe_cfg.controller
            })
        )?))
        .is_err()
    );
    // Run the real Groups dialog against this same HTTP fixture when requested.
    if let Ok(node) = std::env::var("INFUSER_GROUP_BROWSER_NODE") {
        let status = tokio::process::Command::new(node)
            .args(["tests/groups_journey.mjs", &c.url, &group])
            .status()
            .await?;
        ensure!(status.success(), "private-group browser journey failed");
    }
    Ok(())
}
