//! Private, group-scoped feedback. Only an admin can approve a public issue.
use super::*;
use serde::Serialize;

const DESTINATION: &str = "https://api.github.com/repos/esper256/infuser/issues";
const UNAVAILABLE: &str = "Feedback is not set up. Ask your admin to enable it.";
const UNCERTAIN: &str = "We could not confirm whether GitHub received your report. Keep this preview and ask your admin to check the issues before sending it again.";

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route(
            "/api/feedback",
            get(status)
                .post(enqueue)
                .layer(DefaultBodyLimit::max(16384)),
        )
        .route("/api/feedback/review", get(list))
        .route("/api/feedback/review/{id}", delete(dismiss))
        .route(
            "/api/feedback/review/{id}/send",
            post(send).layer(DefaultBodyLimit::max(16384)),
        )
        .route(
            "/assets/feedback.js",
            get(|h: HeaderMap| async move {
                conditional::static_asset(
                    &h,
                    "text/javascript",
                    include_bytes!("../../web/feedback.js"),
                )
            }),
        )
}
fn token(p: &Platform) -> Result<String> {
    let value = std::fs::read_to_string(
        p.config()
            .controller
            .state_dir
            .join("feedback.github-token"),
    )
    .map_err(|_| fault(503, UNAVAILABLE))?;
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_whitespace),
        fault(503, UNAVAILABLE)
    );
    Ok(value.into())
}
async fn status(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let email = p.member(&h, false).await?;
    let admin = !h.contains_key(groups::INSPECT) && p.db.catalog()?.members[&email].admin;
    Ok(reply(json!({"admin":admin})))
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Feature,
    Bug,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    online: bool,
    secure_context: bool,
    webgl: bool,
    webrtc: bool,
    webassembly: bool,
    viewport: Viewport,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Viewport {
    Small,
    Medium,
    Large,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    kind: Kind,
    title: String,
    description: String,
    diagnostics: Option<Diagnostics>,
}
impl Report {
    fn issue(&self) -> Result<Value> {
        ensure!(
            !self.title.trim().is_empty()
                && self.title.chars().count() <= 80
                && !self.title.chars().any(char::is_control),
            fault(422, "Use a title of 1–80 characters.")
        );
        ensure!(
            !self.description.trim().is_empty() && self.description.chars().count() <= 2000,
            fault(422, "Describe your feedback in 1–2000 characters.")
        );
        ensure!(
            !matches!(self.kind, Kind::Feature) || self.diagnostics.is_none(),
            fault(422, "Feature requests do not include diagnostics.")
        );
        let kind = match self.kind {
            Kind::Feature => "Feature request",
            Kind::Bug => "Bug",
        };
        let mut body = format!("{}\n\n{}", kind, self.description.trim());
        if let Some(d) = &self.diagnostics {
            body.push_str(&format!(
                "\n\nBrowser diagnostics (reviewed by sender):\n```json\n{}\n```",
                serde_json::to_string_pretty(d)?
            ));
        }
        Ok(json!({"title":format!("[{kind}] {}", self.title.trim()), "body":body}))
    }
}
#[derive(Clone, Deserialize, Serialize)]
struct Attempt {
    at: u64,
    member: String,
}
// Reserve BEFORE the request, including failures: retries cannot defeat the budget.
// Persist across restarts; a corrupt ledger fails closed. Never store report text.
fn reserve(path: &std::path::Path, member: &str, time: u64) -> Result<()> {
    let mut entries: Vec<Attempt> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(e.into()),
    };
    entries.retain(|e| time.saturating_sub(e.at) < 86400);
    let member = hash(member);
    ensure!(
        entries.len() < 10
            && !entries
                .iter()
                .any(|e| e.member == member && time.saturating_sub(e.at) < 60),
        fault(
            429,
            "Posting limit reached. Wait a minute and try again; the installation allows ten attempts per day."
        )
    );
    entries.push(Attempt { at: time, member });
    crate::private_file::replace(path, &serde_json::to_vec(&entries)?)?;
    Ok(())
}
const RETENTION: u64 = 30 * 86400;
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ReviewState {
    Pending,
    Sending,
    Sent,
    Uncertain,
    Dismissed,
}
#[derive(Clone, Deserialize, Serialize)]
struct Entry {
    id: String,
    keys: Vec<String>,
    fingerprint: String,
    approved_fingerprint: Option<String>,
    member: String,
    group: String,
    created: u64,
    attempted: Option<u64>,
    state: ReviewState,
    report: Option<Report>,
}
#[derive(Default, Deserialize, Serialize)]
struct Queue {
    entries: Vec<Entry>,
}
impl Queue {
    fn prune(&mut self, time: u64) -> bool {
        let count = self.entries.len();
        self.entries
            .retain(|e| time.saturating_sub(e.created) < RETENTION);
        count != self.entries.len()
    }
    fn enqueue(&mut self, body: Intake, email: &str, group: &str, time: u64) -> Result<String> {
        body.report.issue()?;
        ensure!(
            (16..=64).contains(&body.request_id.len())
                && body
                    .request_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            fault(422, "Invalid feedback request ID.")
        );
        let member = hash(email);
        let key = hash(&format!("{group}:{email}:{}", body.request_id));
        let fingerprint = hash(&serde_json::to_string(&body.report)?);
        if let Some(entry) = self.entries.iter().find(|e| e.keys.contains(&key)) {
            ensure!(
                entry.fingerprint == fingerprint,
                fault(
                    409,
                    "This request ID was already used for different feedback."
                )
            );
            return Ok(entry.id.clone());
        }
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.member == member && e.group == group && e.fingerprint == fingerprint)
        {
            ensure!(
                entry.keys.len() < 32,
                fault(
                    429,
                    "This identical report has already been received. Please ask an admin to check the queue."
                )
            );
            entry.keys.push(key);
            return Ok(entry.id.clone());
        }
        let recent: Vec<_> = self
            .entries
            .iter()
            .filter(|e| time.saturating_sub(e.created) < 86400)
            .collect();
        ensure!(
            self.entries.len() < 500
                && recent.len() < 50
                && recent.iter().filter(|e| e.member == member).count() < 3,
            fault(
                429,
                "The private feedback queue is full or your daily limit was reached. Please try tomorrow."
            )
        );
        let id = secret();
        self.entries.push(Entry {
            id: id.clone(),
            keys: vec![key],
            fingerprint,
            approved_fingerprint: None,
            member,
            group: group.into(),
            created: time,
            attempted: None,
            state: ReviewState::Pending,
            report: Some(body.report),
        });
        Ok(id)
    }
    fn entry(&mut self, id: &str, group: &str) -> Result<&mut Entry> {
        self.entries
            .iter_mut()
            .find(|e| e.id == id && e.group == group)
            .ok_or_else(|| fault(404, "Feedback not found."))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intake {
    request_id: String,
    report: Report,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    report: Report,
    public_consent: bool,
}
fn queue_path(p: &Platform) -> PathBuf {
    p.config().controller.state_dir.join("feedback-queue.json")
}
fn load_queue(p: &Platform) -> Result<Queue> {
    let path = queue_path(p);
    let mut queue: Queue = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Queue::default(),
        Err(e) => return Err(e.into()),
    };
    if queue.prune(now()) {
        crate::private_file::replace(&path, &serde_json::to_vec(&queue)?)?;
    }
    Ok(queue)
}
fn change<T>(p: &Platform, edit: impl FnOnce(&mut Queue) -> Result<T>) -> Result<T> {
    let _lock = p.feedback_lock.lock().unwrap();
    let mut queue = load_queue(p)?;
    let result = edit(&mut queue)?;
    crate::private_file::replace(&queue_path(p), &serde_json::to_vec(&queue)?)?;
    Ok(result)
}
impl Platform {
    pub fn cleanup_feedback(&self) -> Result<()> {
        let _lock = self.feedback_lock.lock().unwrap();
        load_queue(self)?;
        Ok(())
    }
}
async fn admin_group(p: &Platform, h: &HeaderMap, write: bool) -> Result<String> {
    ensure!(
        !h.contains_key(groups::INSPECT),
        fault(
            403,
            "Feedback review is unavailable in inspection mode. Return home first."
        )
    );
    let email = p.admin(h, write).await?;
    Ok(p.db.catalog()?.groups.member(&email).into())
}
async fn enqueue(State(p): State<Platform>, h: HeaderMap, Json(body): Json<Intake>) -> Answer {
    let email = p.member(&h, true).await?;
    let group = p.db.catalog()?.groups.member(&email).to_string();
    let id = change(&p, |q| q.enqueue(body, &email, &group, now()))?;
    Ok(reply(json!({"queued":true,"id":id})))
}
async fn list(State(p): State<Platform>, h: HeaderMap) -> Answer {
    let group = admin_group(&p, &h, false).await?;
    let _lock = p.feedback_lock.lock().unwrap();
    let queue = load_queue(&p)?;
    let data = p.db.catalog()?;
    let submitters: std::collections::BTreeMap<_, _> = data
        .members
        .iter()
        .filter(|(id, _)| data.groups.member(id) == group)
        .map(|(id, member)| (hash(id), &member.name))
        .collect();
    let entries: Vec<_> = queue
        .entries
        .iter()
        .filter(|e| {
            e.group == group && !matches!(e.state, ReviewState::Sent | ReviewState::Dismissed)
        })
        // Attribution is private review metadata, never part of Report::issue().
        .map(|e| json!({"id":e.id,"created":e.created,"state":e.state,"report":e.report,"submitter":submitters.get(&e.member)}))
        .collect();
    Ok(reply(
        json!({"available":token(&p).is_ok(),"entries":entries}),
    ))
}
async fn dismiss(State(p): State<Platform>, h: HeaderMap, Path(id): Path<String>) -> Answer {
    let group = admin_group(&p, &h, true).await?;
    change(&p, |q| {
        let entry = q.entry(&id, &group)?;
        ensure!(
            entry.state != ReviewState::Sending
                || now().saturating_sub(entry.attempted.unwrap_or(now())) > 60,
            fault(409, "Posting is still in progress.")
        );
        entry.state = ReviewState::Dismissed;
        entry.report = None;
        Ok(())
    })?;
    Ok(reply(json!({"dismissed":true})))
}
async fn send(
    State(p): State<Platform>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Approval>,
) -> Answer {
    send_to(p, h, id, body, DESTINATION).await
}
async fn send_to(
    p: Platform,
    h: HeaderMap,
    id: String,
    body: Approval,
    destination: &str,
) -> Answer {
    let group = admin_group(&p, &h, true).await?;
    ensure!(
        body.public_consent,
        fault(422, "Review and agree to post this publicly first.")
    );
    let issue = body.report.issue()?;
    let approved_fingerprint =
        hash(&serde_json::to_string(&body.report).map_err(anyhow::Error::from)?);
    let mut credential = None;
    let claimed = change(&p, |q| {
        let entry = q.entry(&id, &group)?;
        if entry.state == ReviewState::Sent {
            ensure!(
                entry.approved_fingerprint.as_ref() == Some(&approved_fingerprint),
                fault(
                    409,
                    "Another approved version was already posted. Refresh the queue."
                )
            );
            return Ok(false);
        }
        ensure!(
            entry.state == ReviewState::Pending,
            fault(
                409,
                "This report was already attempted or dismissed. Check GitHub before creating another report."
            )
        );
        credential = Some(token(&p)?);
        // A global daily posting budget. The group is the rate-limit principal,
        // so one admin cannot accidentally flood the repository with a queue.
        reserve(
            &p.config()
                .controller
                .state_dir
                .join("feedback-attempts.json"),
            &group,
            now(),
        )?;
        entry.state = ReviewState::Sending;
        entry.attempted = Some(now());
        entry.approved_fingerprint = Some(approved_fingerprint);
        entry.report = Some(body.report);
        Ok(true)
    })?;
    if !claimed {
        return Ok(reply(json!({"submitted":true})));
    }
    // Claim is durable BEFORE the external side effect. Even a process crash
    // cannot cause a second automatic GitHub POST for this queue entry.
    let result = submit(&p.http, &credential.unwrap(), &issue, destination).await;
    change(&p, |q| {
        let entry = q.entry(&id, &group)?;
        if entry.state != ReviewState::Sending {
            return Ok(());
        }
        if result.is_ok() {
            entry.state = ReviewState::Sent;
            entry.report = None;
        } else {
            entry.state = ReviewState::Uncertain;
        }
        Ok(())
    })?;
    result?;
    Ok(reply(json!({"submitted":true})))
}
async fn submit(
    http: &reqwest::Client,
    token: &str,
    issue: &Value,
    destination: &str,
) -> Result<()> {
    let response = http
        .post(destination)
        .bearer_auth(token)
        .header("User-Agent", "Infuser-feedback")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(issue)
        .send()
        .await
        .map_err(|_| fault(502, UNCERTAIN))?;
    ensure!(
        response.status() == StatusCode::CREATED,
        fault(502, UNCERTAIN)
    );
    // GitHub's 201 is the confirmation. Do not parse or expose unbounded response data.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_are_bounded_and_diagnostics_are_an_allowlist() {
        let value = json!({"kind":"feature","title":"Idea","description":"Please add this"});
        let report: Report = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            report.issue().unwrap()["body"],
            "Feature request\n\nPlease add this"
        );
        let diagnostics = json!({"online":true,"secure_context":true,"webgl":true,"webrtc":false,"webassembly":true,"viewport":"small"});
        let mut bug = value.clone();
        bug["kind"] = json!("bug");
        bug["diagnostics"] = diagnostics.clone();
        assert!(
            serde_json::from_value::<Report>(bug.clone())
                .unwrap()
                .issue()
                .unwrap()["body"]
                .as_str()
                .unwrap()
                .contains("Browser diagnostics")
        );
        bug["diagnostics"]["url"] = json!("private");
        assert!(serde_json::from_value::<Report>(bug).is_err());
        let mut feature = value.clone();
        feature["diagnostics"] = diagnostics;
        assert!(
            serde_json::from_value::<Report>(feature)
                .unwrap()
                .issue()
                .is_err()
        );
        for extra in [json!({"url":"secret"}), json!({"online":true})] {
            let mut v = value.clone();
            v["diagnostics"] = extra;
            assert!(serde_json::from_value::<Report>(v).is_err());
        }
        for (key, val) in [
            ("title", json!("x".repeat(81))),
            ("description", json!("x".repeat(2001))),
        ] {
            let mut v = value.clone();
            v[key] = val;
            assert!(
                serde_json::from_value::<Report>(v)
                    .unwrap()
                    .issue()
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn upstream_confirmation_and_errors_do_not_expose_secrets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/ok",
                post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                    assert_eq!(headers["authorization"], "Bearer test-token");
                    for key in ["cookie", "origin", "x-infuser-verified-profile"] {
                        assert!(!headers.contains_key(key));
                    }
                    assert_eq!(body, json!({"title":"test","body":"reviewed"}));
                    StatusCode::CREATED
                }),
            )
            .route(
                "/fail",
                post(|| async { (StatusCode::FORBIDDEN, "private upstream details") }),
            )
            .route(
                "/slow",
                post(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    StatusCode::CREATED
                }),
            );
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(100))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let issue = json!({"title":"test","body":"reviewed"});
        submit(&http, "test-token", &issue, &format!("{base}/ok"))
            .await
            .unwrap();
        for path in ["fail", "slow"] {
            let error = submit(&http, "test-token", &issue, &format!("{base}/{path}"))
                .await
                .unwrap_err();
            let fault = error.downcast_ref::<Fault>().unwrap();
            assert_eq!(fault.status, 502);
            assert_eq!(fault.message, UNCERTAIN);
        }
        task.abort();
    }
    #[tokio::test]
    async fn admin_approval_is_durable_at_most_once_and_only_posts_edited_text() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for status in [StatusCode::CREATED, StatusCode::FORBIDDEN] {
            let dir = tempfile::tempdir().unwrap();
            let config: Config = toml::from_str(&format!(
                "[controller]\nstate_dir={:?}",
                dir.path().to_string_lossy()
            ))
            .unwrap();
            platform::member(&config, "admin@example.com", "Admin", true, true).unwrap();
            let controller = Arc::new(
                Controller::open(
                    config.clone(),
                    Arc::new(crate::docker::Docker {
                        settings: config.controller.clone(),
                    }),
                )
                .unwrap(),
            );
            let p = Platform::open(controller.clone()).unwrap();
            // Synthetic fixture only. Never reads or contacts production.
            std::fs::write(
                dir.path().join("feedback.github-token"),
                "synthetic-test-token",
            )
            .unwrap();
            let original = json!({"kind":"feature","title":"Original private title","description":"Private original text","diagnostics":null});
            let id = change(&p, |q| {
                q.enqueue(
                    serde_json::from_value(
                        json!({"request_id":"request-1234567890","report":original}),
                    )
                    .unwrap(),
                    "user@example.com",
                    "home",
                    now(),
                )
            })
            .unwrap();
            let count = Arc::new(AtomicUsize::new(0));
            let calls = count.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let destination = format!("http://{}/issues", listener.local_addr().unwrap());
            let app=Router::new().route("/issues",post(move |h:HeaderMap,Json(body):Json<Value>|{let calls=calls.clone();async move {
                calls.fetch_add(1,Ordering::SeqCst);
                assert_eq!(h["authorization"],"Bearer synthetic-test-token");assert!(!h.contains_key("cookie"));
                assert_eq!(body,json!({"title":"[Feature request] Edited","body":"Feature request\n\nSafe public description"}));
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                status
            }}));
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut h = HeaderMap::new();
            h.insert("host", "games.example.com".parse().unwrap());
            h.insert("origin", "https://games.example.com".parse().unwrap());
            h.insert(
                "x-infuser-verified-profile",
                "admin@example.com".parse().unwrap(),
            );
            let approval = || {
                serde_json::from_value::<Approval>(json!({"report":{"kind":"feature","title":"Edited","description":"Safe public description","diagnostics":null},"public_consent":true})).unwrap()
            };
            let (first, second) = tokio::join!(
                send_to(p.clone(), h.clone(), id.clone(), approval(), &destination),
                send_to(p.clone(), h.clone(), id.clone(), approval(), &destination)
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert!(second.is_err());
            assert_eq!(first.is_ok(), status == StatusCode::CREATED);
            let restarted = Platform::open(controller).unwrap();
            let retry = send_to(
                restarted.clone(),
                h.clone(),
                id.clone(),
                approval(),
                &destination,
            )
            .await;
            assert_eq!(retry.is_ok(), status == StatusCode::CREATED);
            assert_eq!(count.load(Ordering::SeqCst), 1);
            let queue = load_queue(&restarted).unwrap();
            if status == StatusCode::CREATED {
                assert!(queue.entries[0].report.is_none());
            } else {
                assert!(queue.entries[0].state == ReviewState::Uncertain);
            }
            // Simulate a crash after the durable claim, before recording a result.
            change(&restarted, |q| {
                q.entries[0].state = ReviewState::Sending;
                Ok(())
            })
            .unwrap();
            assert!(
                send_to(restarted, h, id, approval(), &destination)
                    .await
                    .is_err()
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            task.abort();
        }
    }
    #[test]
    fn budget_survives_reloads_and_limits_all_members() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("attempts.json");
        reserve(&path, "one", 100).unwrap();
        assert!(reserve(&path, "one", 101).is_err());
        for i in 0..9 {
            reserve(&path, &format!("other{i}"), 102).unwrap();
        }
        assert!(reserve(&path, "new", 4000).is_err());
        reserve(&path, "one", 86503).unwrap();
        std::fs::write(&path, "broken").unwrap();
        assert!(reserve(&path, "one", 90000).is_err());
    }
}
