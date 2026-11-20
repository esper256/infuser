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

const UNAUTH_PER_SECOND: f64 = 20.0;
const UNAUTH_SOURCES: usize = 4096;
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
            sources: SourceLimit::new(UNAUTH_SOURCES),
        }
    }

    fn observe(&self, packet: &[u8], source: SocketAddr) {
        if stun::parse(packet).is_some_and(|message| message.class_type == BINDING_REQUEST) {
            self.book.note_binding(source.ip());
        }
        if let Some(username) = stun::allocate_username(packet)
            && let Some(peer) = peer_from_username(username, now())
        {
            *self.pending.lock().unwrap() = Some(peer.to_owned());
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
            if unauthenticated(&buf[..size]) && !self.sources.allow(addr.ip()) {
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

fn unauthenticated(packet: &[u8]) -> bool {
    stun::parse(packet).is_some() && !has_integrity(packet)
}

fn has_integrity(packet: &[u8]) -> bool {
    let Some(stun) = stun::parse(packet) else {
        return false;
    };
    let _ = stun;
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

    fn allow(&self, ip: IpAddr) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        if let Some(bucket) = buckets.get(&ip) {
            return bucket.allow(1);
        }
        if buckets.len() >= self.cap {
            return false;
        }
        let bucket = Bucket::new(UNAUTH_PER_SECOND, UNAUTH_PER_SECOND);
        let allowed = bucket.allow(1);
        buckets.insert(ip, bucket);
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthenticated_sources_stop_at_the_cap() {
        let limit = SourceLimit::new(2);
        let a = IpAddr::from([203, 0, 113, 1]);
        let b = IpAddr::from([203, 0, 113, 2]);
        let c = IpAddr::from([203, 0, 113, 3]);
        assert!(limit.allow(a));
        assert!(limit.allow(b));
        assert!(!limit.allow(c));
        assert_eq!(limit.buckets.lock().unwrap().len(), 2);
    }
}
