//! TURN over TCP for the client connection. Each framed STUN or ChannelData
//! message becomes one datagram for the relay. Linux TCP_INFO supplies the
//! client-connection RTT. Relayed traffic stays UDP (`no-tcp-relay`).

use crate::{
    controller::now,
    turn::{
        auth::peer_from_username,
        book::Book,
        metrics::SharedRelay,
        stun::{self, BINDING_REQUEST},
    },
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    os::fd::AsRawFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

pub struct TcpTurn {
    local: SocketAddr,
    incoming: tokio::sync::Mutex<mpsc::Receiver<(Vec<u8>, SocketAddr)>>,
    writers: Arc<Mutex<HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>>>,
    closed: Arc<AtomicBool>,
}

impl TcpTurn {
    pub fn port(&self) -> u16 {
        self.local.port()
    }

    pub async fn bind(
        addr: SocketAddr,
        pending: Arc<Mutex<Option<String>>>,
        book: Arc<Book>,
        metrics: SharedRelay,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let (incoming_tx, incoming_rx) = mpsc::channel(256);
        let writers = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let accept_writers = Arc::clone(&writers);
        let accept_pending = Arc::clone(&pending);
        let accept_book = Arc::clone(&book);
        let accept_closed = Arc::clone(&closed);
        tokio::spawn(async move {
            loop {
                if accept_closed.load(Ordering::Relaxed) {
                    break;
                }
                let accepted = listener.accept().await;
                let Ok((stream, peer)) = accepted else {
                    break;
                };
                let (writer_tx, writer_rx) = mpsc::channel(32);
                accept_writers.lock().unwrap().insert(peer, writer_tx);
                let incoming = incoming_tx.clone();
                let pending = Arc::clone(&accept_pending);
                let book = Arc::clone(&accept_book);
                let metrics = Arc::clone(&metrics);
                let writers = Arc::clone(&accept_writers);
                tokio::spawn(async move {
                    serve_connection(stream, peer, incoming, writer_rx, pending, book, metrics)
                        .await;
                    writers.lock().unwrap().remove(&peer);
                });
            }
        });
        Ok(Self {
            local,
            incoming: tokio::sync::Mutex::new(incoming_rx),
            writers,
            closed,
        })
    }
}

async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    incoming: mpsc::Sender<(Vec<u8>, SocketAddr)>,
    mut outbound: mpsc::Receiver<Vec<u8>>,
    pending: Arc<Mutex<Option<String>>>,
    book: Arc<Book>,
    metrics: SharedRelay,
) {
    let fd = stream.as_raw_fd();
    let (mut read, mut write) = stream.into_split();
    loop {
        tokio::select! {
            frame = read_frame(&mut read) => {
                let Ok(frame) = frame else { break };
                if let Some(rtt_us) = tcp_rtt_us(fd)
                    && rtt_us > 0
                {
                    metrics.tcp_rtt.observe(u64::from(rtt_us / 1000));
                }
                let datagram = datagram(frame);
                observe(&datagram, peer, &pending, &book);
                if incoming.send((datagram, peer)).await.is_err() {
                    break;
                }
            }
            packet = outbound.recv() => {
                let Some(packet) = packet else { break };
                if write.write_all(&packet).await.is_err() {
                    break;
                }
            }
        }
    }
}

fn observe(packet: &[u8], source: SocketAddr, pending: &Mutex<Option<String>>, book: &Book) {
    if stun::parse(packet).is_some_and(|message| message.class_type == BINDING_REQUEST) {
        book.note_binding(source.ip());
    }
    if let Some(username) = stun::allocate_username(packet)
        && let Some(peer) = peer_from_username(username, now())
    {
        *pending.lock().unwrap() = Some(peer.to_owned());
    }
}

async fn read_frame<R: AsyncRead + Unpin>(stream: &mut R) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let total = stun::tcp_frame_len(&header)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "turn frame"))?;
    if total < 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "turn frame"));
    }
    let mut frame = vec![0u8; total];
    frame[..4].copy_from_slice(&header);
    if total > 4 {
        stream.read_exact(&mut frame[4..]).await?;
    }
    Ok(frame)
}

fn datagram(frame: Vec<u8>) -> Vec<u8> {
    if frame[0] & 0xc0 == 0x00 || frame[0] & 0xc0 == 0x40 {
        frame
    } else {
        frame[2..].to_vec()
    }
}

fn tcp_rtt_us(fd: std::os::fd::RawFd) -> Option<u32> {
    let mut info = [0u8; 128];
    let mut len = info.len() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_INFO,
            info.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if result != 0 || len < 72 {
        return None;
    }
    Some(u32::from_ne_bytes(info[68..72].try_into().ok()?))
}

#[async_trait]
impl util::Conn for TcpTurn {
    async fn connect(&self, _addr: SocketAddr) -> util::Result<()> {
        Ok(())
    }

    async fn recv(&self, buf: &mut [u8]) -> util::Result<usize> {
        let (size, _) = self.recv_from(buf).await?;
        Ok(size)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> util::Result<(usize, SocketAddr)> {
        let mut incoming = self.incoming.lock().await;
        let (frame, addr) = incoming
            .recv()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "turn tcp closed"))?;
        drop(incoming);
        if frame.len() > buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "turn frame too large").into());
        }
        buf[..frame.len()].copy_from_slice(&frame);
        Ok((frame.len(), addr))
    }

    async fn send(&self, buf: &[u8]) -> util::Result<usize> {
        self.send_to(buf, self.local).await
    }

    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> util::Result<usize> {
        let writer = self
            .writers
            .lock()
            .unwrap()
            .get(&target)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "turn tcp peer"))?;
        writer
            .send(buf.to_vec())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "turn tcp closed"))?;
        Ok(buf.len())
    }

    fn local_addr(&self) -> util::Result<SocketAddr> {
        Ok(self.local)
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }

    async fn close(&self) -> util::Result<()> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn as_any(&self) -> &(dyn std::any::Any + Send + Sync) {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::metrics::RelayMetrics;
    use std::net::Ipv4Addr;
    use util::Conn;

    #[tokio::test]
    async fn a_binding_request_round_trips_as_one_datagram() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            Arc::clone(&book),
            metrics,
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        let mut request = [0u8; 20];
        request[1] = 0x01;
        request[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        request[8..20].copy_from_slice(&[3; 12]);
        client.write_all(&request).await.unwrap();
        let mut buf = [0u8; 64];
        let (size, addr) = turn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &request);
        assert_eq!(addr.ip(), client.local_addr().unwrap().ip());
        assert!(book.snapshot().bindings.contains(&addr.ip()));
        turn.send_to(&request, addr).await.unwrap();
        client.read_exact(&mut buf[..20]).await.unwrap();
        assert_eq!(&buf[..20], &request);
    }
}
