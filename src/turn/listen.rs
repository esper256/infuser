//! UDP listener wrapper. STUN Binding requests are recorded by source address,
//! and an Allocate username is remembered for the quota check that follows on
//! this listener's read loop.

use crate::{
    controller::now,
    turn::{
        auth::peer_from_username,
        book::Book,
        bucket::Bucket,
        stun::{self, BINDING_REQUEST},
    },
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::net::UdpSocket;

/// Integrity-bearing requests may use the full rate. A request without
/// integrity costs five tokens, so those stay near 20 per second.
const REQUESTS_PER_SECOND: f64 = 100.0;
const UNAUTH_COST: usize = 5;
const AUTH_COST: usize = 1;
const SOURCE_CAP: usize = 4096;
const INTEGRITY: u16 = 0x0008;

pub struct UdpListener {
    inner: UdpSocket,
    pending: Arc<Mutex<Option<String>>>,
    book: Arc<Book>,
    sources: SourceLimit,
}

impl UdpListener {
    pub fn new(inner: UdpSocket, pending: Arc<Mutex<Option<String>>>, book: Arc<Book>) -> Self {
        Self {
            inner,
            pending,
            book,
            sources: SourceLimit::new(SOURCE_CAP),
        }
    }

    fn observe(&self, packet: &[u8], source: SocketAddr) {
        if stun::parse(packet).is_some_and(|message| message.class_type == BINDING_REQUEST) {
            self.book.note_binding(source.ip());
        }
        if stun::allocate_username(packet).is_some() {
            *self.pending.lock().unwrap() = stun::allocate_username(packet)
                .and_then(|username| peer_from_username(username, now()))
                .map(str::to_owned);
        }
    }
}

#[async_trait]
impl util::Conn for UdpListener {
    async fn connect(&self, addr: SocketAddr) -> util::Result<()> {
        Ok(self.inner.connect(addr).await?)
    }

    async fn recv(&self, buf: &mut [u8]) -> util::Result<usize> {
        let (size, _) = self.recv_from(buf).await?;
        Ok(size)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> util::Result<(usize, SocketAddr)> {
        loop {
            let (size, addr) = self.inner.recv_from(buf).await?;
            if stun_request(&buf[..size])
                && !self.sources.allow(addr.ip(), has_integrity(&buf[..size]))
            {
                continue;
            }
            self.observe(&buf[..size], addr);
            return Ok((size, addr));
        }
    }

    async fn send(&self, buf: &[u8]) -> util::Result<usize> {
        Ok(self.inner.send(buf).await?)
    }

    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> util::Result<usize> {
        Ok(self.inner.send_to(buf, target).await?)
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

/// STUN requests only. Indications, responses, and ChannelData are not limited.
fn stun_request(packet: &[u8]) -> bool {
    stun::parse(packet).is_some_and(|message| message.class_type & 0x0110 == 0)
}

fn has_integrity(packet: &[u8]) -> bool {
    if stun::parse(packet).is_none() {
        return false;
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let mut attributes = &packet[20..20 + length];
    while attributes.len() >= 4 {
        let kind = u16::from_be_bytes([attributes[0], attributes[1]]);
        let size = u16::from_be_bytes([attributes[2], attributes[3]]) as usize;
        let padded = size.saturating_add(3) & !3;
        if attributes.len() < 4 + padded {
            return false;
        }
        if kind == INTEGRITY {
            return true;
        }
        attributes = &attributes[4 + padded..];
    }
    false
}

struct SourceLimit {
    cap: usize,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl SourceLimit {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn allow(&self, ip: IpAddr, authenticated: bool) -> bool {
        let ip = source_key(ip);
        let cost = if authenticated {
            AUTH_COST
        } else {
            UNAUTH_COST
        };
        let mut buckets = self.buckets.lock().unwrap();
        if let Some(bucket) = buckets.get(&ip) {
            return bucket.allow(cost);
        }
        if buckets.len() >= self.cap {
            let idle = buckets
                .iter()
                .find(|(_, bucket)| bucket.refilled())
                .map(|(ip, _)| *ip);
            match idle {
                Some(idle) => {
                    buckets.remove(&idle);
                }
                // A full table of active sources must not lock out a new player.
                None => return true,
            }
        }
        let bucket = Bucket::new(REQUESTS_PER_SECOND, REQUESTS_PER_SECOND);
        let allowed = bucket.allow(cost);
        buckets.insert(ip, bucket);
        allowed
    }
}

/// IPv4 stays one address. IPv6 is one bucket per /64, so a prefix cannot
/// fill the table with a fresh address per packet.
fn source_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(address) => {
            let mut prefix = address.octets();
            prefix[8..].fill(0);
            IpAddr::V6(prefix.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_table_still_admits_a_new_source() {
        let limit = SourceLimit::new(4096);
        for n in 0..5000u32 {
            let ip = IpAddr::V4(std::net::Ipv4Addr::from(n.wrapping_add(1)));
            assert!(limit.allow(ip, false), "{ip}");
        }
        assert!(limit.buckets.lock().unwrap().len() <= 4096);
        assert!(limit.allow(IpAddr::from([203, 0, 113, 50]), false));
    }

    #[test]
    fn an_idle_source_is_evicted_for_a_new_one() {
        let limit = SourceLimit::new(2);
        let a = IpAddr::from([203, 0, 113, 1]);
        let b = IpAddr::from([203, 0, 113, 2]);
        let c = IpAddr::from([203, 0, 113, 3]);
        assert!(limit.allow(a, false));
        assert!(limit.allow(b, false));
        for bucket in limit.buckets.lock().unwrap().values() {
            bucket.restore();
        }
        assert!(limit.allow(c, false));
        let left: Vec<_> = limit.buckets.lock().unwrap().keys().copied().collect();
        assert_eq!(left.len(), 2);
        assert!(left.contains(&c));
    }

    #[test]
    fn addresses_in_one_ipv6_prefix_share_a_bucket() {
        let limit = SourceLimit::new(2);
        let first: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let second: IpAddr = "2001:db8:1:2::ffff".parse().unwrap();
        let other: IpAddr = "2001:db8:9:9::1".parse().unwrap();
        assert!(limit.allow(first, false));
        assert!(limit.allow(second, false));
        assert_eq!(limit.buckets.lock().unwrap().len(), 1);
        assert!(limit.allow(other, false));
        assert_eq!(limit.buckets.lock().unwrap().len(), 2);
    }
}
