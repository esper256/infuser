//! Fixed-destination public feedback, authenticated by the ordinary member session.
use super::*;
use serde::Serialize;

const DESTINATION: &str = "https://api.github.com/repos/esper256/infuser/issues";
const UNAVAILABLE: &str = "Feedback is not set up. Ask your adult admin to enable it.";
const UNCERTAIN: &str = "We could not confirm whether GitHub received your report. Keep this preview and ask your adult admin to check the issues before sending it again.";

pub(super) fn routes() -> Router<Platform> {
    Router::new()
        .route(
            "/api/feedback",
            get(status).post(send).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/assets/feedback.js",
            get(|| async {
                (
                    [
                        ("content-type", "text/javascript"),
                        ("cache-control", "no-cache"),
                    ],
                    include_str!("../../web/feedback.js"),
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
    p.member(&h, false).await?;
    Ok(reply(json!({"available":token(&p).is_ok()})))
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Feature,
    Bug,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    online: bool,
    secure_context: bool,
    webgl: bool,
    webrtc: bool,
    webassembly: bool,
    viewport: Viewport,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Viewport {
    Small,
    Medium,
    Large,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    kind: Kind,
    title: String,
    description: String,
    diagnostics: Option<Diagnostics>,
    public_consent: bool,
}
impl Report {
    fn issue(&self) -> Result<Value> {
        ensure!(
            self.public_consent,
            fault(422, "Review and agree to post this publicly first.")
        );
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
#[derive(Deserialize, Serialize)]
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
                .any(|e| e.member == member && time.saturating_sub(e.at) < 3600),
        fault(
            429,
            "Feedback limit reached. Please try tomorrow, or ask your adult admin for help."
        )
    );
    entries.push(Attempt { at: time, member });
    crate::private_file::replace(path, &serde_json::to_vec(&entries)?)?;
    Ok(())
}
async fn send(State(p): State<Platform>, h: HeaderMap, Json(report): Json<Report>) -> Answer {
    let email = p.member(&h, true).await?;
    let issue = report.issue()?;
    let token = token(&p)?;
    {
        let _lock = p.feedback_lock.lock().unwrap();
        reserve(
            &p.config()
                .controller
                .state_dir
                .join("feedback-attempts.json"),
            &email,
            now(),
        )?;
    }
    // Never forward session headers, identity, host, or upstream error bodies.
    submit(&p.http, &token, &issue, DESTINATION).await?;
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
        let value = json!({"kind":"feature","title":"Idea","description":"Please add this","public_consent":true});
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
            ("public_consent", json!(false)),
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
                    for key in ["cookie", "origin", "x-infuser-verified-email"] {
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
