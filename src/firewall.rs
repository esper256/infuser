//! Privileged helper accepts only typed IP addresses over an infuser-owned Unix socket.
use anyhow::{Result, ensure};
use std::{net::IpAddr, os::unix::fs::PermissionsExt, path::Path, process::Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub async fn grant(socket: &Path, ip: IpAddr) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut stream = tokio::net::UnixStream::connect(socket).await?;
        stream.write_all(format!("{ip}\n").as_bytes()).await?;
        stream.shutdown().await?;
        let mut response = String::new();
        stream.take(32).read_to_string(&mut response).await?;
        ensure!(response == "ok\n", "firewall helper rejected grant");
        Ok(())
    })
    .await?
}
pub async fn serve(socket: &Path) -> Result<()> {
    if socket.exists() {
        std::fs::remove_file(socket)?;
    }
    let listener = tokio::net::UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let result: Result<()> = async {
            let mut raw = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                (&mut stream).take(64).read_to_string(&mut raw),
            )
            .await??;
            let ip: IpAddr = raw.trim().parse()?;
            let mut command = tokio::process::Command::new("/usr/sbin/nft")
                .args(["-f", "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
                .kill_on_drop(true).spawn()?;
            command.stdin.take().unwrap().write_all(renewal(ip, "30m").as_bytes()).await?;
            let status = tokio::time::timeout(std::time::Duration::from_secs(2), command.wait()).await??;
            ensure!(status.success(), "nft grant failed");
            Ok(())
        }
        .await;
        let _ = stream
            .write_all(if result.is_ok() { b"ok\n" } else { b"error\n" })
            .await;
    }
}

/// nft commits the entire batch atomically. The first add also handles an absent
/// or expired entry, so this works without a version-specific destroy command.
pub fn renewal(ip: IpAddr, timeout: &str) -> String {
    let set = if ip.is_ipv4() {"members4"} else {"members6"};
    format!("add element inet infuser_ingress {set} {{ {ip} }}\ndelete element inet infuser_ingress {set} {{ {ip} }}\nadd element inet infuser_ingress {set} {{ {ip} timeout {timeout} }}\n")
}
