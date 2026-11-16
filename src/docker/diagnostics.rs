//! Private, bounded runtime evidence. Never serialize Docker's environment/config.
use super::*;
use serde_json::json;
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt};

const LOG_BYTES: usize = 16 * 1024;
const REPORTS: usize = 3;
pub fn capabilities() -> Value {
    json!({"version":1,"retained_attempts":REPORTS,"log_bytes_per_stream":LOG_BYTES})
}

fn path(settings: &Settings, id: &str) -> Result<PathBuf> {
    ensure!(crate::config::slug(id), "invalid diagnostics app ID");
    Ok(settings
        .state_dir
        .join("runtime-diagnostics")
        .join(format!("{id}.json")))
}

pub fn retained(settings: &Settings, id: &str) -> Result<Vec<Value>> {
    let path = path(settings, id)?;
    match fs::File::open(path) {
        Ok(file) => {
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 1024 * 1024,
                "diagnostic archive exceeds its limit"
            );
            Ok(serde_json::from_slice(&bytes)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn retain(settings: &Settings, id: &str, report: Value) -> Result<()> {
    let path = path(settings, id)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path.parent().unwrap())?;
    let mut reports = retained(settings, id)?;
    reports.retain(|old| old["attempt"] != report["attempt"]);
    reports.insert(0, report);
    reports.truncate(REPORTS);
    crate::private_file::replace(&path, &serde_json::to_vec(&reports)?)
}

async fn tail(mut reader: impl AsyncRead + Unpin) -> Result<(String, bool)> {
    let mut bytes = std::collections::VecDeque::with_capacity(LOG_BYTES);
    let mut total = 0usize;
    let mut buffer = [0; 8192];
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        let discard = (bytes.len() + n).saturating_sub(LOG_BYTES);
        bytes.drain(..discard);
        bytes.extend(&buffer[..n]);
    }
    let mut text: String = String::from_utf8_lossy(bytes.make_contiguous())
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let expanded = text.len() > LOG_BYTES;
    if expanded {
        let mut start = text.len() - LOG_BYTES;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text.drain(..start);
    }
    Ok((text, total > LOG_BYTES || expanded))
}

impl Docker {
    pub(super) async fn capture_diagnostics(
        &self,
        id: &str,
        instance: &Instance,
        owner: &str,
    ) -> Result<Option<Value>> {
        let Some(object) = self.object("container", &instance.name).await? else {
            return Ok(None);
        };
        Self::owned(&object, "container", owner, id, Some(&instance.attempt))?;
        let state = &object["State"];
        let build = object["Config"]["Labels"]["org.infuser.package.job"]
            .as_str()
            .and_then(|key| key.strip_prefix(&format!("{id}/")));
        let health: Vec<Value> = state["Health"]["Log"].as_array().into_iter().flatten().rev().take(3)
            .map(|log| json!({"at":log["End"],"exit_code":log["ExitCode"],
                "output":log["Output"].as_str().unwrap_or("").chars().take(2048).collect::<String>()})).collect();
        let mut report = json!({"captured_at":crate::controller::now(),"app_id":id,"build_id":build,
            "attempt":instance.attempt,"image":instance.spec.image,"reason":instance.error,
            "container_state":state["Status"],"exit_code":state["ExitCode"],"oom_killed":state["OOMKilled"],
            "started_at":state["StartedAt"],"finished_at":state["FinishedAt"],
            "health":state["Health"]["Status"],"health_checks":health});
        let logs = async {
            let mut child = Command::new("docker")
                .arg("--host")
                .arg(format!("unix://{}", self.settings.docker_socket.display()))
                .args(["logs", "--timestamps", "--tail", "200", &instance.name])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let (out, err, status) = tokio::join!(
                tail(child.stdout.take().unwrap()),
                tail(child.stderr.take().unwrap()),
                child.wait()
            );
            ensure!(status?.success(), "Docker logs unavailable");
            Ok::<_, anyhow::Error>((out?, err?))
        };
        match tokio::time::timeout(Duration::from_secs(8), logs).await {
            Ok(Ok(((stdout, a), (stderr, b)))) => {
                report["stdout"] = stdout.into();
                report["stderr"] = stderr.into();
                report["logs_truncated"] = (a || b).into();
            }
            _ => report["logs_error"] = "Docker logs unavailable or timed out".into(),
        }
        Ok(Some(report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn keeps_recent_output_and_three_private_attempts() -> Result<()> {
        let input = format!("{}last useful error\n", "x".repeat(LOG_BYTES * 3));
        let (output, truncated) = tail(input.as_bytes()).await?;
        assert!(truncated);
        assert!(output.ends_with("last useful error\n"));
        assert!(output.len() <= LOG_BYTES);
        let invalid = vec![0xff; LOG_BYTES];
        let (output, truncated) = tail(invalid.as_slice()).await?;
        assert!(truncated);
        assert!(
            output.len() <= LOG_BYTES,
            "UTF-8 replacement must not expand past the byte cap"
        );
        let root = tempfile::tempdir()?;
        let settings = Settings {
            state_dir: root.path().into(),
            ..Default::default()
        };
        for attempt in 0..5 {
            retain(&settings, "pond", json!({"attempt":attempt}))?;
        }
        retain(&settings, "pond", json!({"attempt":4,"updated":true}))?;
        let reports = retained(&settings, "pond")?;
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0]["updated"], true);
        assert_eq!(reports[2]["attempt"], 2);
        assert!(retained(&settings, "another-game")?.is_empty());
        assert_eq!(
            fs::metadata(path(&settings, "pond")?)?.permissions().mode() & 0o777,
            0o600
        );
        assert!(retained(&settings, "../pond").is_err());
        Ok(())
    }
}
