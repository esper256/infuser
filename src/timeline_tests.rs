use super::*;

#[tokio::test]
async fn profile_filter_keeps_unattributed_events_in_time_order() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let store = Timeline::new(&crate::config::Settings {
        state_dir: temp.path().join("state"),
        diagnostics_dir: Some(temp.path().join("logs")),
        ..Default::default()
    });
    let start = ms() - 1000;
    let event = |offset: i64, kind: &str, profile: &str, app: &str| {
        let mut e = Event::server(
            kind,
            ContextFields {
                profile: profile.into(),
                app: app.into(),
                ..Default::default()
            },
            json!({}),
            false,
        );
        e.at = start + offset;
        e
    };
    assert!(
        store
            .submit(vec![
                event(3, "runtime.transition", "", "pond"),
                event(2, "launch.open", "other-player", "pond"),
                event(1, "launch.open", "player", "pond"),
                event(0, "controller.start", "", ""),
            ])
            .await
    );
    let kinds = |app| -> Result<Vec<Value>> {
        let result = read(store.path(), &Filter::default(), app, Some("player"), false)?;
        Ok(result["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].clone())
            .collect())
    };
    assert_eq!(
        kinds(None)?,
        ["controller.start", "launch.open", "runtime.transition"]
    );
    assert_eq!(kinds(Some("pond"))?, ["launch.open", "runtime.transition"]);
    Ok(())
}

#[tokio::test]
async fn queue_orders_retries_and_keeps_visit_details_private() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let settings = crate::config::Settings {
        diagnostics_dir: Some(temp.path().join("logs")),
        ..Default::default()
    };
    let store = Timeline::new(&settings);
    let start = ms() - 500;
    let event = |seq: u32, at: i64| {
        let mut e = Event::server(
            "js.error",
            ContextFields {
                profile: "profile-a".into(),
                visit: "visit-a".into(),
                app: "pond".into(),
                ..Default::default()
            },
            json!({"message":"private visit","line":seq}),
            true,
        );
        e.src = "game".into();
        e.at = at;
        e.page = "page".into();
        e.seq = Some(seq);
        e
    };
    assert!(
        store
            .submit(vec![
                event(2, start + 5),
                event(1, start),
                event(2, start + 5),
                event(3, start - 2 * DAY)
            ])
            .await
    );
    let q = Filter {
        limit: Some(1),
        ..Default::default()
    };
    let first = read(store.path(), &q, Some("pond"), None, true)?;
    assert_eq!(first["events"][0]["data"], json!({"line":1}));
    assert!(first["events"][0].get("profile").is_none());
    let q = Filter {
        cursor: Some(first["next"].as_str().unwrap().into()),
        ..q
    };
    let next = read(store.path(), &q, Some("pond"), None, true)?;
    assert_eq!(next["events"][0]["at"], start + 5);
    assert!(next["next"].is_null());
    assert_eq!(
        read(store.path(), &Filter::default(), Some("other"), None, true)?["events"],
        json!([])
    );
    let c = Connection::open(store.path())?;
    maintain(&c)?;
    assert_eq!(
        c.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, u32>(0))?,
        2
    );
    Ok(())
}
