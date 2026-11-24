use super::*;
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
