//! Managed server leases are separate from the browser launch and its saves.
use super::*;

pub(super) fn capabilities() -> Value {
    json!({"version":1,"experimental":true,"start":"on_demand","session_policy":"one_per_app"})
}

impl Platform {
    // Callers hold publishing, preventing acquisition/release from racing a
    // build promotion or a heartbeat carrying an out-of-date launch snapshot.
    pub(super) fn runtime_view(&self, launch: &Launch, renew: bool) -> Result<Value> {
        let (Some(lease), Some(token)) = (&launch.lease_id, &launch.lease_token) else {
            return Ok(json!({"state":"idle","session_id":null,"endpoint":null,"error":null}));
        };
        // Managed-session demand is renewed by authenticated live sockets, not
        // title-screen timers. An acquisition has its initial bounded startup lease.
        let renew = renew && !self.db.read()?.builds[&build_key(&launch.app,&launch.build)].manifest.dedicated_sessions();
        let state = match self.controller.lease(
            launch.runtime_id(),
            lease,
            token,
            if renew { "renew" } else { "get" },
            now(),
        ) {
            Ok(state) => state,
            Err(error)
                if error
                    .downcast_ref::<Fault>()
                    .is_some_and(|f| matches!(f.status, 404 | 409 | 410)) =>
            {
                // Losing a backend must not expire the browser/cloud-save session.
                return Ok(json!({"state":"ended","session_id":lease,"endpoint":null,
                    "error":"The server session ended. Choose a game mode to reconnect."}));
            }
            Err(error) => return Err(error),
        };
        let endpoint = if state["state"] == "ready"
            && self
                .controller
                .snapshot()
                .games
                .get(launch.runtime_id())
                .is_some_and(|i| i.spec.web)
        {
            json!({"url":format!("https://{}/_infuser/server",launch.host)})
        } else {
            state["endpoint"].clone()
        };
        Ok(
            json!({"state":state["state"],"session_id":lease,"endpoint":endpoint,
            "error":state["error"],"retry_at":state["retry_at"]}),
        )
    }

    fn runtime_launch(&self, h: &HeaderMap) -> Result<(String, Launch)> {
        let (id, launch) = self.game(h)?;
        let data = self.db.read()?;
        let build = &data.builds[&build_key(&launch.app, &launch.build)];
        ensure!(
            build.manifest.server_start.is_some() && build.server.is_some(),
            fault(
                422,
                "this build does not declare an on-demand managed server"
            )
        );
        Ok((id, launch))
    }
}

pub(super) async fn status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let _guard = p.publishing.lock().unwrap();
    let (_, launch) = p.runtime_launch(&h)?;
    Ok(reply(p.runtime_view(&launch, false)?))
}

pub(super) async fn acquire(State(p): State<Platform>, h: HeaderMap) -> Answer {
    p.origin(&h, host(&h))?;
    p.game(&h)?;
    p.admit(&h).await;
    let _guard = p.publishing.lock().unwrap();
    let (id, mut launch) = p.runtime_launch(&h)?;
    let data = p.db.read()?;
    if data.builds[&build_key(&launch.app,&launch.build)].manifest.dedicated_sessions()
        && data.pending_promotions.contains_key(launch.runtime_id()) {
        ensure!(p.runtime_view(&launch,false)?["state"] == "ready", crate::dedicated::fault(409,"server_draining","An update is waiting; existing runs are finishing."));
    }
    ensure!(
        p.db.read()?
            .deployed(launch.runtime_id())
            .is_some_and(|a| a.build == launch.build),
        fault(409, "a newer build is available; reopen the game")
    );
    let lease = p.controller.acquire(launch.runtime_id(), &id, now())?;
    launch.lease_id = lease["lease_id"].as_str().map(str::to_owned);
    launch.lease_token = lease["lease_token"].as_str().map(str::to_owned);
    // Controller acquisition is idempotent by launch ID. If the catalog write
    // fails, a retry adopts the same lease; an abandoned reservation expires.
    p.db.edit(|d| {
        let saved = d
            .launches
            .get_mut(&id)
            .ok_or_else(|| fault(410, "game session expired"))?;
        saved.lease_id = launch.lease_id.clone();
        saved.lease_token = launch.lease_token.clone();
        Ok(())
    })?;
    let value = p.runtime_view(&launch, false)?;
    let starting = value["state"] == "starting";
    let mut response = reply(value);
    if starting {
        *response.status_mut() = StatusCode::ACCEPTED;
    }
    Ok(response)
}

pub(super) async fn release(
    State(p): State<Platform>,
    Path(session): Path<String>,
    h: HeaderMap,
) -> Answer {
    p.origin(&h, host(&h))?;
    let _guard = p.publishing.lock().unwrap();
    let (id, mut launch) = p.runtime_launch(&h)?;
    // A delayed retry for a previous lease cannot release a later acquisition.
    // Another player's lease ID is never authority to release their server use.
    if launch.lease_id.as_deref() == Some(&session) {
        if let Some(token) = &launch.lease_token {
            match p
                .controller
                .lease(launch.runtime_id(), &session, token, "delete", now())
            {
                Ok(_) => {}
                Err(e)
                    if e.downcast_ref::<Fault>()
                        .is_some_and(|f| matches!(f.status, 404 | 410)) => {}
                Err(e) => return Err(e.into()),
            }
        }
        p.db.edit(|d| {
            let saved = d
                .launches
                .get_mut(&id)
                .ok_or_else(|| fault(410, "game session expired"))?;
            saved.lease_id = None;
            saved.lease_token = None;
            Ok(())
        })?;
        launch.lease_id = None;
        launch.lease_token = None;
    }
    Ok(reply(p.runtime_view(&launch, false)?))
}
