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
            let set = if ip.is_ipv4() { "members4" } else { "members6" };
            // Serialize updates. A short, fail-closed gap is preferable to accidentally
            // extending a stale entry forever; nft's kernel timeout is authoritative.
            let _ = tokio::process::Command::new("/usr/sbin/nft")
                .args([
                    "delete",
                    "element",
                    "inet",
                    "infuser_ingress",
                    set,
                    &format!("{{ {ip} }}"),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?;
            let status = tokio::process::Command::new("/usr/sbin/nft")
                .args([
                    "add",
                    "element",
                    "inet",
                    "infuser_ingress",
                    set,
                    &format!("{{ {ip} timeout 30m }}"),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?;
            ensure!(status.success(), "nft grant failed");
            Ok(())
        }
        .await;
        let _ = stream
            .write_all(if result.is_ok() { b"ok\n" } else { b"error\n" })
            .await;
    }
}
