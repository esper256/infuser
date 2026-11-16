//! Explicit opt-in. All rules live in a fresh user/network namespace.
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};
fn nft(text: &str) -> Result<()> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    let result = child.wait_with_output()?;
    ensure!(
        result.status.success(),
        "nft failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
#[test]
#[ignore = "requires unprivileged user/network namespaces, ip, and nft"]
fn atomic_renewal_keeps_udp_flow_and_expiry() -> Result<()> {
    if std::env::var_os("INFUSER_FIREWALL_NAMESPACE").is_none() {
        let status = Command::new("unshare")
            .args(["--user", "--map-root-user", "--net"])
            .arg(std::env::current_exe()?)
            .args([
                "--exact",
                "atomic_renewal_keeps_udp_flow_and_expiry",
                "--ignored",
                "--nocapture",
            ])
            .env("INFUSER_FIREWALL_NAMESPACE", "1")
            .status()?;
        ensure!(status.success(), "isolated firewall journey failed");
        return Ok(());
    }
    ensure!(
        Command::new("ip")
            .args(["link", "set", "lo", "up"])
            .status()?
            .success(),
        "could not enable namespace loopback"
    );
    nft(
        "table inet infuser_ingress {\nset members4 { type ipv4_addr; flags timeout; timeout 1s; }\nset members6 { type ipv6_addr; flags timeout; timeout 1s; }\nchain ingress { type filter hook input priority -150; policy accept; udp dport 18917 ip saddr != @members4 drop; }\n}\n",
    )?;
    let ip = "127.0.0.1".parse()?;
    let renew = infuser::firewall::renewal(ip, "2s");
    nft(&renew)?;
    nft(&infuser::firewall::renewal("::1".parse()?, "2s"))?;
    let receiver = std::net::UdpSocket::bind("127.0.0.1:18917")?;
    receiver.set_read_timeout(Some(Duration::from_millis(100)))?;
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let active = running.clone();
    let server = std::thread::spawn(move || {
        let mut bytes = [0u8; 8];
        while active.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok((n, source)) = receiver.recv_from(&mut bytes) {
                let _ = receiver.send_to(&bytes[..n], source);
            }
        }
    });
    let sender = std::net::UdpSocket::bind("127.0.0.1:0")?;
    sender.connect("127.0.0.1:18917")?;
    sender.set_read_timeout(Some(Duration::from_millis(500)))?;
    let traffic = std::thread::spawn(move || -> Result<()> {
        let mut bytes = [0u8; 8];
        for n in 0u64..250 {
            sender.send(&n.to_be_bytes())?;
            sender
                .recv(&mut bytes)
                .context("packet lost during renewal")?;
            ensure!(bytes == n.to_be_bytes(), "unexpected UDP sequence");
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    });
    for _ in 0..50 {
        nft(&renew)?;
        std::thread::sleep(Duration::from_millis(3));
    }
    let result = traffic.join().unwrap();
    running.store(false, std::sync::atomic::Ordering::Relaxed);
    server.join().unwrap();
    result?;
    std::thread::sleep(Duration::from_millis(2100));
    // Expired grants disappear; first grant after expiry must also work.
    let expired = Command::new("nft")
        .args([
            "get",
            "element",
            "inet",
            "infuser_ingress",
            "members4",
            "{ 127.0.0.1 }",
        ])
        .output()?;
    ensure!(!expired.status.success(), "grant did not expire");
    nft(&renew)?;
    Ok(())
}
