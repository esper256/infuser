//! TURN over TCP for the client connection. Each framed STUN or ChannelData
//! message becomes one datagram for the relay. Linux TCP_INFO supplies the
//! client-connection RTT. Relayed traffic stays UDP (`no-tcp-relay`).

use crate::{
    controller::now,
    turn::{
        auth::peer_from_username,
        book::Book,
        metrics::SharedRelay,
        quota::PER_USER,
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
    sync::{mpsc, mpsc::error::TrySendError},
};

const PER_SOURCE: usize = PER_USER;
const TOTAL: usize = 64;
const WRITER_QUEUE: usize = 32;
const FIRST_FRAME: std::time::Duration = std::time::Duration::from_secs(5);
const AUTHENTICATE: std::time::Duration = std::time::Duration::from_secs(30);
const IDLE: std::time::Duration = std::time::Duration::from_secs(60);
const ALLOCATE_SUCCESS: u16 = 0x0103;

struct Client {
    addr: SocketAddr,
    tx: mpsc::Sender<Vec<u8>>,
    cancel: tokio::sync::watch::Sender<bool>,
    opened: std::time::Instant,
    authed: Arc<AtomicBool>,
    allocated: Arc<AtomicBool>,
}

pub enum Flow {
    Continue,
    WaitForClose,
}

/// EMFILE and ENFILE wait for a connection to close. Anything else is logged
/// and the accept loop continues, because the listener is still readable.
pub fn on_accept_error(error: &io::Error) -> Flow {
    match error.raw_os_error() {
        Some(libc::EMFILE | libc::ENFILE) => Flow::WaitForClose,
        _ => Flow::Continue,
    }
}

pub struct TcpTurn {
    local: SocketAddr,
    incoming: tokio::sync::Mutex<mpsc::Receiver<(Vec<u8>, SocketAddr)>>,
    writers: Arc<Mutex<HashMap<u64, Client>>>,
    pending: Arc<Mutex<Option<String>>>,
    book: Arc<Book>,
    finished: Mutex<Option<mpsc::UnboundedReceiver<SocketAddr>>>,
    /// Stops the accept loop. `Conn::close` does not touch this: an allocation
    /// ending must not drop the shared listener.
    stop: Mutex<Option<Stop>>,
    /// Counts TCP headers consumed. Unit tests wait on this so an outbound
    /// packet is sent while a frame body is still unread.
    #[cfg(test)]
    headers: tokio::sync::watch::Sender<u64>,
}

struct Stop {
    stop: tokio::sync::oneshot::Sender<()>,
    done: tokio::sync::oneshot::Receiver<()>,
}

impl TcpTurn {
    pub fn port(&self) -> u16 {
        self.local.port()
    }

    pub fn take_finished(&self) -> Option<mpsc::UnboundedReceiver<SocketAddr>> {
        self.finished.lock().unwrap().take()
    }

    /// The allocation for this connection closed or expired. One TCP connection
    /// has one allocation, so a repeated close does not cancel twice.
    pub fn allocation_closed(&self, addr: SocketAddr) {
        let writers = self.writers.lock().unwrap();
        let Some(client) = writers.values().find(|client| client.addr == addr) else {
            return;
        };
        if client.allocated.swap(false, Ordering::Relaxed) {
            let _ = client.cancel.send(true);
        }
    }

    /// Drops the listener. `Running::close` calls this; an allocation does not.
    pub async fn shutdown(&self) {
        let Some(Stop { stop, done }) = self.stop.lock().unwrap().take() else {
            return;
        };
        let _ = stop.send(());
        let _ = done.await;
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
        let (finished_tx, finished_rx) = mpsc::unbounded_channel();
        let writers = Arc::new(Mutex::new(HashMap::new()));
        let accept_writers = Arc::clone(&writers);
        let slots = Arc::new(tokio::sync::Notify::new());
        #[cfg(test)]
        let (headers, _) = tokio::sync::watch::channel(0u64);
        #[cfg(test)]
        let accept_headers = headers.clone();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let mut next_id = 1u64;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        if !admit_or_evict(&accept_writers, peer) {
                            drop(stream);
                            continue;
                        }
                        let id = next_id;
                        next_id += 1;
                        let (writer_tx, writer_rx) = mpsc::channel(WRITER_QUEUE);
                        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                        let authed = Arc::new(AtomicBool::new(false));
                        let allocated = Arc::new(AtomicBool::new(false));
                        accept_writers.lock().unwrap().insert(
                            id,
                            Client {
                                addr: peer,
                                tx: writer_tx,
                                cancel: cancel_tx,
                                opened: std::time::Instant::now(),
                                authed: Arc::clone(&authed),
                                allocated: Arc::clone(&allocated),
                            },
                        );
                        let incoming = incoming_tx.clone();
                        let metrics = Arc::clone(&metrics);
                        let writers = Arc::clone(&accept_writers);
                        let finished = finished_tx.clone();
                        let slots = Arc::clone(&slots);
                        #[cfg(test)]
                        let headers = accept_headers.clone();
                        tokio::spawn(async move {
                            serve_connection(Conn {
                                stream,
                                peer,
                                incoming,
                                outbound: writer_rx,
                                metrics,
                                cancel: cancel_rx,
                                authed,
                                allocated,
                                #[cfg(test)]
                                headers,
                            })
                            .await;
                            writers.lock().unwrap().remove(&id);
                            slots.notify_waiters();
                            let _ = finished.send(peer);
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "TURN TCP accept failed");
                        if matches!(on_accept_error(&error), Flow::WaitForClose) {
                            wait_for_close(&slots).await;
                        }
                    }
                }
                }
            }
            drop(listener);
            let _ = done_tx.send(());
        });
        Ok(Self {
            local,
            incoming: tokio::sync::Mutex::new(incoming_rx),
            writers,
            pending,
            book,
            finished: Mutex::new(Some(finished_rx)),
            stop: Mutex::new(Some(Stop {
                stop: stop_tx,
                done: done_rx,
            })),
            #[cfg(test)]
            headers,
        })
    }
}

/// A close that already happened does not store a permit, and the full table
/// may belong to something other than a TURN connection. Wake on either.
async fn wait_for_close(slots: &tokio::sync::Notify) {
    tokio::select! {
        _ = slots.notified() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
    }
}

fn admit_or_evict(writers: &Mutex<HashMap<u64, Client>>, peer: SocketAddr) -> bool {
    let mut writers = writers.lock().unwrap();
    let same = writers
        .values()
        .filter(|client| client.addr.ip() == peer.ip())
        .count();
    if writers.len() < TOTAL && same < PER_SOURCE {
        return true;
    }
    let victim = writers
        .iter()
        .filter(|(_, client)| !client.authed.load(Ordering::Relaxed))
        .filter(|(_, client)| same < PER_SOURCE || client.addr.ip() == peer.ip())
        .min_by_key(|(_, client)| client.opened)
        .map(|(id, _)| *id);
    let Some(id) = victim else {
        return false;
    };
    if let Some(client) = writers.remove(&id) {
        let _ = client.cancel.send(true);
    }
    true
}

struct Conn {
    stream: TcpStream,
    peer: SocketAddr,
    incoming: mpsc::Sender<(Vec<u8>, SocketAddr)>,
    outbound: mpsc::Receiver<Vec<u8>>,
    metrics: SharedRelay,
    cancel: tokio::sync::watch::Receiver<bool>,
    authed: Arc<AtomicBool>,
    allocated: Arc<AtomicBool>,
    #[cfg(test)]
    headers: tokio::sync::watch::Sender<u64>,
}

async fn serve_connection(mut link: Conn) {
    let fd = link.stream.as_raw_fd();
    let (mut read, mut write) = link.stream.into_split();
    let (frames_tx, mut frames) = mpsc::channel(8);
    // `read_exact` is not cancel-safe. This task is the only reader, so an
    // outbound packet cannot discard a half-consumed frame.
    #[cfg(test)]
    let headers = link.headers.clone();
    let reader = tokio::spawn(async move {
        while let Ok(frame) = read_frame(
            &mut read,
            #[cfg(test)]
            &headers,
        )
        .await
        {
            if frames_tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    let started = std::time::Instant::now();
    let mut last_frame = started;
    let mut saw_frame = false;
    loop {
        let frame_left = FIRST_FRAME.saturating_sub(started.elapsed());
        let auth_left = AUTHENTICATE.saturating_sub(started.elapsed());
        let idle_left = IDLE.saturating_sub(last_frame.elapsed());
        tokio::select! {
            changed = link.cancel.changed() => {
                if changed.is_err() || *link.cancel.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(frame_left), if !saw_frame => break,
            _ = tokio::time::sleep(auth_left), if !link.authed.load(Ordering::Relaxed) => break,
            _ = tokio::time::sleep(idle_left), if !link.allocated.load(Ordering::Relaxed) => break,
            frame = frames.recv() => {
                let Some(frame) = frame else { break };
                saw_frame = true;
                last_frame = std::time::Instant::now();
                if let Some(rtt_us) = tcp_rtt_us(fd)
                    && rtt_us > 0
                {
                    link.metrics.tcp_rtt.observe(u64::from(rtt_us / 1000));
                }
                if link.incoming.send((frame, link.peer)).await.is_err() {
                    break;
                }
            }
            packet = link.outbound.recv() => {
                let Some(packet) = packet else { break };
                if write.write_all(&packet).await.is_err() {
                    break;
                }
            }
        }
    }
    reader.abort();
}

fn remember(packet: &[u8], source: SocketAddr, pending: &Mutex<Option<String>>, book: &Book) {
    if stun::parse(packet).is_some_and(|message| message.class_type == BINDING_REQUEST) {
        book.note_binding(source.ip());
    }
    if stun::allocate_username(packet).is_some() {
        *pending.lock().unwrap() = stun::allocate_username(packet)
            .and_then(|username| peer_from_username(username, now()))
            .map(str::to_owned);
    }
}

fn allocate_success(packet: &[u8]) -> bool {
    stun::parse(packet).is_some_and(|message| message.class_type == ALLOCATE_SUCCESS)
}

async fn read_frame<R: AsyncRead + Unpin>(
    stream: &mut R,
    #[cfg(test)] headers: &tokio::sync::watch::Sender<u64>,
) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    // After the header is consumed, an outbound packet must not restart the read.
    #[cfg(test)]
    headers.send_modify(|count| *count += 1);
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

fn drop_slow(writers: &Mutex<HashMap<u64, Client>>, target: SocketAddr) {
    let mut writers = writers.lock().unwrap();
    let ids: Vec<_> = writers
        .iter()
        .filter(|(_, client)| client.addr == target)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        if let Some(client) = writers.remove(&id) {
            let _ = client.cancel.send(true);
        }
    }
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
        remember(&frame, addr, &self.pending, &self.book);
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
        let writer = {
            let writers = self.writers.lock().unwrap();
            let client = writers.values().find(|client| client.addr == target);
            if allocate_success(buf)
                && let Some(client) = client
            {
                client.authed.store(true, Ordering::Relaxed);
                // A retransmitted Allocate replays this success. One connection
                // has one allocation, so a second write must not count again.
                client.allocated.store(true, Ordering::Relaxed);
            }
            client
                .map(|client| client.tx.clone())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "turn tcp peer"))?
        };
        match writer.try_send(buf.to_vec()) {
            Ok(()) => Ok(buf.len()),
            Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
                drop_slow(&self.writers, target);
                Err(io::Error::new(io::ErrorKind::WouldBlock, "turn tcp reader is behind").into())
            }
        }
    }

    fn local_addr(&self) -> util::Result<SocketAddr> {
        Ok(self.local)
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }

    async fn close(&self) -> util::Result<()> {
        // Allocation::close calls this on the shared listener. Stopping here
        // would refuse every later TCP client until the process restarts.
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
    use std::{net::Ipv4Addr, os::fd::AsRawFd};
    use util::Conn;

    fn binding() -> [u8; 20] {
        let mut request = [0u8; 20];
        request[1] = 0x01;
        request[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        request[8..20].copy_from_slice(&[3; 12]);
        request
    }

    fn allocate(username: &str) -> Vec<u8> {
        let padded = username.len().div_ceil(4) * 4;
        let mut packet = vec![0u8; 20 + 4 + padded];
        packet[0..2].copy_from_slice(&0x0003u16.to_be_bytes());
        packet[2..4].copy_from_slice(&((4 + padded) as u16).to_be_bytes());
        packet[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        packet[20..22].copy_from_slice(&0x0006u16.to_be_bytes());
        packet[22..24].copy_from_slice(&(username.len() as u16).to_be_bytes());
        packet[24..24 + username.len()].copy_from_slice(username.as_bytes());
        packet
    }

    fn client(addr: SocketAddr, authed: bool) -> Client {
        let (tx, _) = mpsc::channel(1);
        let (cancel, _) = tokio::sync::watch::channel(false);
        Client {
            addr,
            tx,
            cancel,
            opened: std::time::Instant::now(),
            authed: Arc::new(AtomicBool::new(authed)),
            allocated: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn sixteen_per_address_and_an_idle_connection_yields_when_full() {
        let writers = Mutex::new(HashMap::new());
        let one = "203.0.113.8".parse().unwrap();
        for port in 1..=PER_SOURCE as u16 {
            writers
                .lock()
                .unwrap()
                .insert(u64::from(port), client(SocketAddr::new(one, port), true));
        }
        assert!(!admit_or_evict(&writers, SocketAddr::new(one, 99)));
        writers.lock().unwrap().clear();
        for port in 1..=TOTAL as u16 {
            writers.lock().unwrap().insert(
                u64::from(port),
                client(SocketAddr::new(one, port), port != 1),
            );
        }
        assert!(admit_or_evict(
            &writers,
            SocketAddr::new("198.51.100.2".parse().unwrap(), 1)
        ));
        assert!(
            !writers
                .lock()
                .unwrap()
                .values()
                .any(|client| client.addr == SocketAddr::new(one, 1))
        );
    }

    #[test]
    fn an_exhausted_file_table_waits_for_a_close() {
        assert!(matches!(
            on_accept_error(&io::Error::from_raw_os_error(libc::EMFILE)),
            Flow::WaitForClose
        ));
        assert!(matches!(
            on_accept_error(&io::Error::from_raw_os_error(libc::ENFILE)),
            Flow::WaitForClose
        ));
        assert!(matches!(
            on_accept_error(&io::Error::from_raw_os_error(libc::ECONNABORTED)),
            Flow::Continue
        ));
    }

    #[tokio::test]
    async fn an_exhausted_file_table_wakes_without_a_close() {
        let slots = tokio::sync::Notify::new();
        let started = std::time::Instant::now();
        wait_for_close(&slots).await;
        let elapsed = started.elapsed();
        assert!(elapsed >= std::time::Duration::from_millis(900));
        assert!(elapsed < std::time::Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_binding_request_round_trips_as_one_datagram() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        book.allow([Ipv4Addr::LOCALHOST.into()].into());
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            Arc::clone(&book),
            metrics,
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        let request = binding();
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

    #[tokio::test]
    async fn a_slow_reader_is_dropped_without_stalling_the_other_client() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            book,
            metrics,
        )
        .await
        .unwrap();
        let slow = TcpStream::connect(turn.local).await.unwrap();
        let mut slow_write = slow;
        let request = binding();
        slow_write.write_all(&request).await.unwrap();
        let mut buf = [0u8; 64];
        let (_, slow_addr) = turn.recv_from(&mut buf).await.unwrap();
        let fd = slow_write.as_raw_fd();
        let small: libc::c_int = 1024;
        unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of_val(&small) as libc::socklen_t,
            );
        }
        let mut fast = TcpStream::connect(turn.local).await.unwrap();
        let packet = vec![0x40u8; 8 * 1024];
        let mut rejected = false;
        for _ in 0..WRITER_QUEUE + 8 {
            if turn.send_to(&packet, slow_addr).await.is_err() {
                rejected = true;
                break;
            }
        }
        assert!(rejected);
        drop(slow_write);
        fast.write_all(&request).await.unwrap();
        let (size, _) = turn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &request);
    }

    #[tokio::test]
    async fn queued_allocates_are_charged_to_the_username_in_that_frame() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let pending = Arc::new(Mutex::new(None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::clone(&pending),
            book,
            metrics,
        )
        .await
        .unwrap();
        let expires = crate::controller::now() + 3600;
        let mut first = TcpStream::connect(turn.local).await.unwrap();
        let mut second = TcpStream::connect(turn.local).await.unwrap();
        let alpha = allocate(&format!("{expires}:alpha"));
        let beta = allocate(&format!("{expires}:beta"));
        first.write_all(&alpha).await.unwrap();
        second.write_all(&beta).await.unwrap();
        let mut buf = vec![0u8; 256];
        let mut seen = Vec::new();
        for _ in 0..2 {
            let (size, _) = turn.recv_from(&mut buf).await.unwrap();
            let username = stun::allocate_username(&buf[..size]).unwrap();
            let charged = pending.lock().unwrap().clone().unwrap();
            seen.push((username.to_owned(), charged));
        }
        seen.sort();
        let expect = vec![
            (format!("{expires}:alpha"), "alpha".into()),
            (format!("{expires}:beta"), "beta".into()),
        ];
        assert_eq!(seen, expect);
    }

    #[tokio::test]
    async fn a_split_frame_survives_an_outbound_packet() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            book,
            metrics,
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        let mut headers = turn.headers.subscribe();
        client.write_all(&binding()).await.unwrap();
        let mut buf = [0u8; 64];
        let (_, addr) = turn.recv_from(&mut buf).await.unwrap();
        let mut second = binding();
        second[8] = 9;
        let seen = *headers.borrow_and_update();
        client.write_all(&second[..10]).await.unwrap();
        while *headers.borrow() == seen {
            headers.changed().await.unwrap();
        }
        turn.send_to(&binding(), addr).await.unwrap();
        client.write_all(&second[10..]).await.unwrap();
        let (size, _) = turn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &second);
    }

    #[tokio::test]
    async fn a_forged_allocate_stays_unauthed_and_is_closed() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            book,
            metrics,
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        let username = format!("{}:attacker", crate::controller::now() + 86_400 * 365);
        let started = std::time::Instant::now();
        client.write_all(&allocate(&username)).await.unwrap();
        let mut buf = [0u8; 256];
        let (_, addr) = turn.recv_from(&mut buf).await.unwrap();
        let authed = turn
            .writers
            .lock()
            .unwrap()
            .values()
            .find(|client| client.addr == addr)
            .unwrap()
            .authed
            .load(Ordering::Relaxed);
        assert!(!authed);
        let mut byte = [0u8; 1];
        let closed =
            tokio::time::timeout(std::time::Duration::from_secs(45), client.read(&mut byte)).await;
        assert!(
            matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
            "forged Allocate left the connection open: {closed:?}"
        );
        assert!(started.elapsed() >= std::time::Duration::from_secs(25));
    }

    fn success() -> [u8; 20] {
        let mut packet = [0u8; 20];
        packet[0..2].copy_from_slice(&0x0103u16.to_be_bytes());
        packet[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
        packet
    }

    #[tokio::test]
    async fn closing_the_shared_connection_still_serves_a_new_client() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            book,
            metrics,
        )
        .await
        .unwrap();
        util::Conn::close(&turn).await.unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        let request = binding();
        client.write_all(&request).await.unwrap();
        let mut buf = [0u8; 64];
        let (size, _) = turn.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..size], &request);
    }

    #[tokio::test]
    async fn a_replayed_allocate_success_closes_with_the_allocation() {
        let metrics = Arc::new(RelayMetrics::new());
        let book = Arc::new(Book::new(Arc::clone(&metrics), None));
        let turn = TcpTurn::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Arc::new(Mutex::new(None)),
            book,
            metrics,
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(turn.local).await.unwrap();
        client.write_all(&binding()).await.unwrap();
        let mut buf = [0u8; 64];
        let (_, addr) = turn.recv_from(&mut buf).await.unwrap();
        let response = success();
        turn.send_to(&response, addr).await.unwrap();
        turn.send_to(&response, addr).await.unwrap();
        turn.allocation_closed(addr);
        let mut buf = [0u8; 64];
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(2), client.read(&mut buf))
                .await
            {
                Ok(Ok(0)) | Ok(Err(_)) => break,
                Ok(Ok(_)) => continue,
                Err(_) => panic!("a replayed Allocate success kept the connection"),
            }
        }
    }
}
