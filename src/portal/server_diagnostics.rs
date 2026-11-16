use super::*;

pub(super) async fn get(
    State(p): State<Platform>,
    Path(app): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.publisher(&h, Some(&app), "publish")?;
    let data = p.db.read()?;
    let active = data.apps.get(&app).map(|a| a.build.clone());
    let queued = data.pending_promotions.get(&app).map(|a| a.build.clone());
    let snapshot = p.controller.snapshot();
    let instance = snapshot.games.get(&app);
    let runtime = instance.map(|i| json!({"phase":i.phase,"attempt":i.attempt,
        "image":i.spec.image,"started_at":i.started,"lease_count":i.leases.len(),
        "memory_mb":i.spec.memory_mb,"cpu_millis":i.spec.cpu_millis,"error":i.error,"retry_at":i.retry_at}));
    let mut live_error = None;
    let live = if let Some(i) = instance.filter(|i| i.reserved()) {
        match p
            .controller
            .engine
            .diagnostics(&app, i, &snapshot.owner)
            .await
        {
            Ok(report) => report,
            Err(error) => {
                tracing::warn!(app_id = app, attempt = %i.attempt, %error, "Live game diagnostics unavailable");
                live_error =
                    Some("Live container diagnostics unavailable; retry or ask the operator.");
                None
            }
        }
    } else {
        None
    };
    let recent = crate::docker::retained_diagnostics(&p.config().controller, &app)?;
    Ok(reply(
        json!({"version":1,"app_id":app,"active_build":active,"queued_build":queued,
        "runtime":runtime,"live":live,"live_error":live_error,"recent":recent}),
    ))
}
