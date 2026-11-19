//! Recently shows the latest publication of each visible deployment. A profile
//! needs at most one stored vote per deployment, even after years of updates.
use super::*;
use rusqlite::{TransactionBehavior, params};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vote {
    emoji: String,
}

pub(super) async fn set(
    State(p): State<Platform>,
    Path((app, publication)): Path<(String, String)>,
    h: HeaderMap,
    Json(vote): Json<Vote>,
) -> Answer {
    let profile = p.member(&h, true).await?;
    ensure!(
        valid_emoji(&vote.emoji),
        fault(400, "Choose a reaction emoji.")
    );
    let mut c = p.db.connection.lock().unwrap();
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    let data = p.db.cached_catalog(&tx)?;
    let (group, _) = groups::view(&data, &h, &profile)?;
    ensure!(
        data.profile_enabled(&profile)
            && groups::visible(&data, &profile, &app, &group, false, now()),
        fault(404, "Game not found.")
    );
    let runtime = data
        .deployment_for(&app, &group)
        .ok_or_else(|| fault(404, "Game not found."))?;
    let active = data
        .deployed(runtime)
        .ok_or_else(|| fault(404, "Game not found."))?;
    ensure!(
        active.update_id(runtime) == publication,
        fault(
            409,
            "A newer update is available. Choose a reaction on that update."
        )
    );
    tx.execute("INSERT INTO update_reactions(deployment,profile,publication,emoji) VALUES(?1,?2,?3,?4)
        ON CONFLICT(deployment,profile) DO UPDATE SET publication=excluded.publication,emoji=excluded.emoji",
        params![runtime,profile,publication,vote.emoji]).map_err(anyhow::Error::from)?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(reply(json!({"ok":true})))
}

pub(super) fn add_summaries(
    db: &Database,
    data: &platform::Data,
    viewer: &str,
    apps: &mut [Value],
) -> Result<()> {
    let c = db.connection.lock().unwrap();
    let mut query = c.prepare_cached(
        "SELECT profile,emoji FROM update_reactions WHERE deployment=?1 AND publication=?2",
    )?;
    for app in apps {
        let mut counts = BTreeMap::<String, usize>::new();
        let mut mine = None;
        let mut rows = query.query(params![
            app["deployment_id"].as_str(),
            app["update_id"].as_str()
        ])?;
        while let Some(row) = rows.next()? {
            let profile: String = row.get(0)?;
            if data.profile_enabled(&profile) {
                let emoji: String = row.get(1)?;
                if viewer == profile {
                    mine = Some(emoji.clone());
                }
                *counts.entry(emoji).or_default() += 1;
            }
        }
        app["update_reactions"] = json!({"counts":counts,"mine":mine});
    }
    Ok(())
}
