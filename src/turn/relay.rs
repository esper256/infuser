//! Real UDP relay ports, public-address mapping, quotas, and per-allocation timing.

use crate::turn::{
    book::Book,
    bucket::Bucket,
    deny::denied,
    metrics::SharedRelay,
    quota::Quotas,
    stun::{self, BINDING_REQUEST, BINDING_SUCCESS},
};
use async_trait::async_trait;
use std::{
    collections::{BTreeSet, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::net::UdpSocket;
use turn::{Error, relay::RelayAddressGenerator};

const STALL_MS: f64 = 200.0;

pub struct Pool {
    bind_ip: Ipv4Addr,
    public_ip: IpAddr,
    min_port: u16,
    max_port: u16,
    used: Mutex<BTreeSet<u16>>,
    quotas: Quotas,
    book: Arc<Book>,
    metrics: SharedRelay,
    global: Arc<Bucket>,
}

impl Pool {
    pub fn new(
        bind_ip: Ipv4Addr,
        public_ip: Ipv4Addr,
        min_port: u16,
        max_port: u16,
        book: Arc<Book>,
        metrics: SharedRelay,
    ) -> Self {
        Self {
            bind_ip,
            public_ip: IpAddr::V4(public_ip),
            min_port,
            max_port,
            used: Mutex::new(BTreeSet::new()),
            quotas: Quotas::new(),
            book,
            metrics,
            global: Arc::new(Bucket::global()),
        }
    }
}

pub struct Generator {
    pool: Arc<Pool>,
    pending: Arc<Mutex<Option<String>>>,
}

impl Generator {
    pub fn new(pool: Arc<Pool>, pending: Arc<Mutex<Option<String>>>) -> Self {
        Self { pool, pending }
    }
}

struct Hold {
    pool: Arc<Pool>,
    port: u16,
    user: String,
    released: AtomicBool,
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::Relaxed) {
            return;
        }
        self.pool.used.lock().unwrap().remove(&self.port);
        self.pool.quotas.release(&self.user);
    }
}

struct Direction {
    last: Mutex<Option<Instant>>,
    gap_ms: Mutex<Option<f64>>,
    jitter_ms: Mutex<f64>,
    samples: AtomicU64,
    pending: Mutex<VecDeque<([u8; 12], Instant)>>,
}

impl Direction {
    fn new() -> Self {
        Self {
            last: Mutex::new(None),
            gap_ms: Mutex::new(None),
            jitter_ms: Mutex::new(0.0),
            samples: AtomicU64::new(0),
            pending: Mutex::new(VecDeque::new()),
        }
    }

    fn remember(&self, transaction: &[u8]) {
        let Ok(id) = transaction.try_into() else {
            return;
        };
        let mut pending = self.pending.lock().unwrap();
        if pending.len() == 32 {
            pending.pop_front();
        }
        pending.push_back((id, Instant::now()));
    }

    fn take(&self, transaction: &[u8]) -> Option<u64> {
        let Ok(id) = <[u8; 12]>::try_from(transaction) else {
            return None;
        };
        let mut pending = self.pending.lock().unwrap();
        let index = pending.iter().position(|(stored, _)| *stored == id)?;
        let (_, sent) = pending.remove(index)?;
        Some(sent.elapsed().as_millis() as u64)
    }

    /// A gap over 200ms between application packets. Turn-based games produce
    /// these while players think; the counter is a gap count, not a fault.
    fn arrive(&self, metrics: &SharedRelay, stalls: &AtomicU64) {
        let now = Instant::now();
        let mut last = self.last.lock().unwrap();
        if let Some(previous) = *last {
            let gap = now.saturating_duration_since(previous).as_secs_f64() * 1000.0;
            if gap > STALL_MS && self.samples.load(Ordering::Relaxed) > 0 {
                stalls.fetch_add(1, Ordering::Relaxed);
                metrics.stalls.fetch_add(1, Ordering::Relaxed);
            }
            let mut prior = self.gap_ms.lock().unwrap();
            if let Some(previous_gap) = *prior {
                let mut jitter = self.jitter_ms.lock().unwrap();
                let delta = (gap - previous_gap).abs();
                *jitter += (delta - *jitter) / 16.0;
                let seen = self.samples.fetch_add(1, Ordering::Relaxed) + 1;
                if seen.is_multiple_of(16) {
                    metrics.jitter.observe(*jitter as u64);
                }
            }
            *prior = Some(gap);
        }
        *last = Some(now);
    }
}

pub struct Meter {
    inner: UdpSocket,
    public_ip: IpAddr,
    relay_start: u16,
    relay_end: u16,
    hold: Hold,
    id: u64,
    book: Arc<Book>,
    bucket: Bucket,
    stun: Bucket,
    global: Arc<Bucket>,
    metrics: SharedRelay,
    outbound: Direction,
    inbound: Direction,
    stalls: AtomicU64,
}

impl Meter {
    fn observe(&self, packet: &[u8], outbound: bool, remote: Option<SocketAddr>) {
        let metrics = &self.metrics;
        if let Some(message) = stun::parse(packet) {
            if outbound && message.class_type == BINDING_REQUEST {
                self.outbound.remember(message.transaction);
            } else if outbound && message.class_type == BINDING_SUCCESS {
                if let Some(rtt) = self.inbound.take(message.transaction) {
                    metrics.client_leg.observe(rtt);
                }
            } else if !outbound && message.class_type == BINDING_REQUEST {
                self.inbound.remember(message.transaction);
            } else if !outbound
                && message.class_type == BINDING_SUCCESS
                && let Some(rtt) = self.outbound.take(message.transaction)
            {
                metrics.peer_leg.observe(rtt);
            }
        }
        // RFC 7983: a first byte of 0-3 is STUN. Relayed and stall counts are
        // application traffic, so a consent check does not light the label.
        if packet.first().is_some_and(|byte| *byte < 4) {
            return;
        }
        let direction = if outbound {
            &self.outbound
        } else {
            &self.inbound
        };
        direction.arrive(metrics, &self.stalls);
        self.book.packet(self.id, packet.len() as u64, !outbound);
        if let Some(remote) = remote {
            self.book.touch(self.id, remote);
        }
    }

    fn allowed(&self, addr: SocketAddr) -> bool {
        !denied(addr, self.public_ip, self.relay_start, self.relay_end)
    }

    fn budget(&self, bytes: usize) -> bool {
        self.spend(&self.bucket, bytes)
    }

    /// Real STUN, at most 576 bytes, uses its own bucket and still spends the
    /// global one. Everything else, including a `0x00` prefix, uses the game bucket.
    fn charge(&self, packet: &[u8]) -> bool {
        if packet.len() <= 576 && stun::parse(packet).is_some() {
            self.spend(&self.stun, packet.len())
        } else {
            self.budget(packet.len())
        }
    }

    fn spend(&self, local: &Bucket, bytes: usize) -> bool {
        if !self.global.allow(bytes) {
            return false;
        }
        if local.allow(bytes) {
            return true;
        }
        self.global.refund(bytes);
        false
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        let _hold = &self.hold;
        self.book.close(self.id);
    }
}

#[async_trait]
impl RelayAddressGenerator for Generator {
    fn validate(&self) -> Result<(), Error> {
        if self.pool.min_port == 0 || self.pool.max_port < self.pool.min_port {
            return Err(Error::ErrMinPortNotZero);
        }
        Ok(())
    }

    fn supports_family(&self, use_ipv4: bool) -> bool {
        use_ipv4
    }

    async fn allocate_conn(
        &self,
        use_ipv4: bool,
        requested_port: u16,
    ) -> Result<(Arc<dyn util::Conn + Send + Sync>, SocketAddr), Error> {
        if !use_ipv4 {
            return Err(Error::ErrInvalidRequestedFamilyValue);
        }
        let user = self.pending.lock().unwrap().clone().unwrap_or_default();
        if !self.pool.quotas.try_acquire(&user) {
            return Err(Error::Other("allocation quota exceeded".into()));
        }
        let bound = bind_port(&self.pool, requested_port).await;
        let (socket, port) = match bound {
            Ok(bound) => bound,
            Err(error) => {
                self.pool.quotas.release(&user);
                return Err(error);
            }
        };
        let id = self.pool.book.open(user.clone());
        self.pool.book.bind_port(id, port);
        self.pool.book.set_public_ip(self.pool.public_ip);
        let relay_addr = SocketAddr::new(self.pool.public_ip, port);
        let meter = Meter {
            inner: socket,
            public_ip: self.pool.public_ip,
            relay_start: self.pool.min_port,
            relay_end: self.pool.max_port,
            hold: Hold {
                pool: Arc::clone(&self.pool),
                port,
                user,
                released: AtomicBool::new(false),
            },
            id,
            book: Arc::clone(&self.pool.book),
            bucket: Bucket::game(),
            stun: Bucket::stun(),
            global: Arc::clone(&self.pool.global),
            metrics: Arc::clone(&self.pool.metrics),
            outbound: Direction::new(),
            inbound: Direction::new(),
            stalls: AtomicU64::new(0),
        };
        Ok((Arc::new(meter), relay_addr))
    }
}

async fn bind_port(pool: &Pool, requested: u16) -> Result<(UdpSocket, u16), Error> {
    let candidates: Vec<u16> = {
        let used = pool.used.lock().unwrap();
        if requested != 0 {
            if !(pool.min_port..=pool.max_port).contains(&requested) || used.contains(&requested) {
                return Err(Error::Other("relay port unavailable".into()));
            }
            vec![requested]
        } else {
            (pool.min_port..=pool.max_port)
                .filter(|port| !used.contains(port))
                .collect()
        }
    };
    if candidates.is_empty() {
        return Err(Error::Other("relay ports exhausted".into()));
    }
    for port in candidates {
        let addr = SocketAddr::from((pool.bind_ip, port));
        match UdpSocket::bind(addr).await {
            Ok(socket) => {
                let mut used = pool.used.lock().unwrap();
                if !used.insert(port) {
                    continue;
                }
                return Ok((socket, port));
            }
            Err(_) if requested == 0 => continue,
            Err(_) => return Err(Error::Other("relay port unavailable".into())),
        }
    }
    Err(Error::Other("relay ports exhausted".into()))
}

#[async_trait]
impl util::Conn for Meter {
    async fn connect(&self, _addr: SocketAddr) -> util::Result<()> {
        Ok(())
    }

    async fn recv(&self, buf: &mut [u8]) -> util::Result<usize> {
        let (size, _) = self.recv_from(buf).await?;
        Ok(size)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> util::Result<(usize, SocketAddr)> {
        loop {
            let (size, addr) = self.inner.recv_from(buf).await?;
            if !self.allowed(addr) {
                self.metrics.denied.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // Consent STUN is matched here. Application bytes are counted only
            // after the TURN server accepts the peer, so a port scan cannot
            // spend the bandwidth bucket or light Relayed.
            if buf.first().is_some_and(|byte| *byte < 4) {
                self.observe(&buf[..size], false, Some(addr));
            }
            return Ok((size, addr));
        }
    }

    async fn send(&self, buf: &[u8]) -> util::Result<usize> {
        self.send_to(buf, self.inner.local_addr()?).await
    }

    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> util::Result<usize> {
        if !self.allowed(target) {
            self.metrics.denied.fetch_add(1, Ordering::Relaxed);
            return Err(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied peer").into(),
            );
        }
        if !self.charge(buf) {
            return Ok(buf.len());
        }
        let sent = self.inner.send_to(buf, target).await?;
        self.observe(&buf[..sent], true, Some(target));
        Ok(sent)
    }

    fn local_addr(&self) -> util::Result<SocketAddr> {
        Ok(self.inner.local_addr()?)
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }

    async fn close(&self) -> util::Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &(dyn std::any::Any + Send + Sync) {
        self
    }
}

/// Called by the TURN server after it knows whether the peer is permitted.
/// `forward` is false for an unpermitted packet, which only increments denied.
pub fn account_relay(
    conn: &(dyn util::Conn + Send + Sync),
    packet: &[u8],
    src: SocketAddr,
    forward: bool,
) -> bool {
    let Some(meter) = conn.as_any().downcast_ref::<Meter>() else {
        return forward;
    };
    if !forward {
        meter.metrics.denied.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    if !meter.charge(packet) {
        return false;
    }
    meter.observe(packet, false, Some(src));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::metrics::RelayMetrics;
    use std::net::Ipv4Addr;

    fn binding(kind: u16, transaction: &[u8; 12]) -> [u8; 20] {
        let mut packet = [0u8; 20];
        packet[0..2].copy_from_slice(&kind.to_be_bytes());
        packet[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        packet[8..20].copy_from_slice(transaction);
        packet
    }

    #[tokio::test]
    async fn consent_rtt_is_matched_and_private_peers_are_refused() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let pool = Arc::new(Pool::new(
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::LOCALHOST,
            33_100,
            33_108,
            Arc::clone(&book),
            Arc::clone(&metrics),
        ));
        let generator = Generator::new(pool, Arc::new(Mutex::new(Some("peer-a".into()))));
        let (conn, relay_addr) = generator.allocate_conn(true, 0).await.unwrap();
        assert_eq!(relay_addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        let peer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 33_108))
            .await
            .unwrap();
        let transaction = [7u8; 12];
        conn.send_to(
            &binding(BINDING_REQUEST, &transaction),
            peer.local_addr().unwrap(),
        )
        .await
        .unwrap();
        let mut buf = [0u8; 64];
        let (size, from) = peer.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &binding(BINDING_REQUEST, &transaction));
        peer.send_to(&binding(BINDING_SUCCESS, &transaction), from)
            .await
            .unwrap();
        let (size, _) = conn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &binding(BINDING_SUCCESS, &transaction));
        assert!(
            metrics
                .text()
                .contains("infuser_turn_leg_rtt_ms_count{leg=\"peer\"} 1")
        );
        assert!(
            conn.send_to(b"no", "10.1.2.3:9".parse().unwrap())
                .await
                .is_err()
        );
        assert!(metrics.denied.load(Ordering::Relaxed) >= 1);
        drop(conn);
        assert_eq!(book.snapshot().peers.len(), 0);
        assert!(generator.allocate_conn(true, 0).await.is_ok());
    }

    #[tokio::test]
    async fn unpermitted_inbound_does_not_spend_the_bucket_or_the_label() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let pool = Arc::new(Pool::new(
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::LOCALHOST,
            33_200,
            33_208,
            Arc::clone(&book),
            Arc::clone(&metrics),
        ));
        let junk = UdpSocket::bind((Ipv4Addr::LOCALHOST, 33_208))
            .await
            .unwrap();
        let generator = Generator::new(pool.clone(), Arc::new(Mutex::new(Some("peer-a".into()))));
        let (conn, _) = generator.allocate_conn(true, 0).await.unwrap();
        junk.send_to(b"scan", conn.local_addr().unwrap())
            .await
            .unwrap();
        let mut buf = [0u8; 32];
        let (size, from) = conn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], b"scan");
        assert_eq!(book.snapshot().peers[0].packets, 0);
        assert!(pool.global.refilled());
        assert!(account_relay(conn.as_ref(), &[0x40; 1024], from, true));
        assert_eq!(book.snapshot().peers[0].packets, 1);
        assert!(!pool.global.allow(8_388_608));
    }

    #[tokio::test]
    async fn zero_prefixed_packets_are_dropped_when_the_bucket_is_empty() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let pool = Arc::new(Pool::new(
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::LOCALHOST,
            33_300,
            33_308,
            Arc::clone(&book),
            Arc::clone(&metrics),
        ));
        let sink = UdpSocket::bind((Ipv4Addr::LOCALHOST, 33_308))
            .await
            .unwrap();
        let generator = Generator::new(pool.clone(), Arc::new(Mutex::new(Some("peer-a".into()))));
        let (conn, _) = generator.allocate_conn(true, 0).await.unwrap();
        assert!(pool.global.allow(8_388_608));
        let target = sink.local_addr().unwrap();
        let mut prefixed = [0u8; 1200];
        prefixed[0] = 0x00;
        for _ in 0..50 {
            conn.send_to(&prefixed, target).await.unwrap();
        }
        let mut buf = [0u8; 2000];
        let arrived = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            sink.recv_from(&mut buf),
        )
        .await;
        assert!(
            arrived.is_err(),
            "0x00-prefixed packets were relayed with an empty bucket"
        );
    }
}
