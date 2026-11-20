//! UDP listener wrapper. STUN Binding requests are recorded by source address,
//! and an Allocate username is remembered for the quota check that follows on
//! this listener's read loop.

use crate::{
    controller::now,
    turn::{
        auth::peer_from_username,
        book::Book,
        stun::{self, BINDING_REQUEST},
    },
};
use async_trait::async_trait;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::UdpSocket;

pub struct UdpListener {
    inner: UdpSocket,
    pending: Arc<Mutex<Option<String>>>,
    book: Arc<Book>,
}

impl UdpListener {
    pub fn new(inner: UdpSocket, pending: Arc<Mutex<Option<String>>>, book: Arc<Book>) -> Self {
        Self {
            inner,
            pending,
            book,
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
        let size = self.inner.recv(buf).await?;
        self.observe(&buf[..size], self.inner.local_addr()?);
        Ok(size)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> util::Result<(usize, SocketAddr)> {
        let (size, addr) = self.inner.recv_from(buf).await?;
        self.observe(&buf[..size], addr);
        Ok((size, addr))
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
