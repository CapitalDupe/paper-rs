//! Async TCP listener and per-connection framing with deadlines.
//!
//! [`Connection`] reads and writes whole protocol frames. Every read is bounded:
//! the frame decoder rejects oversized lengths from the prefix alone, and callers
//! pass a deadline so an unauthenticated client cannot hold a connection open
//! indefinitely. [`serve`] caps simultaneous connections and stops on shutdown.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use paper_protocol::frame;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::Instant;

/// Largest frame accepted from a client that has not finished logging in.
pub const PRE_LOGIN_MAX_FRAME_LEN: usize = 32 * 1024;

/// Limits applied to connections.
#[derive(Debug, Clone)]
pub struct NetLimits {
    /// Total time a client has to complete handshake and login.
    pub login_timeout: Duration,
    /// Largest accepted frame payload once logged in.
    pub max_frame_len: usize,
    /// Maximum simultaneous connections.
    pub max_connections: usize,
}

impl Default for NetLimits {
    fn default() -> Self {
        Self {
            login_timeout: Duration::from_secs(10),
            max_frame_len: frame::MAX_FRAME_LEN,
            max_connections: 1024,
        }
    }
}

/// Errors produced by connection I/O.
#[derive(Debug)]
pub enum NetError {
    /// The underlying socket failed.
    Io(io::Error),
    /// The peer sent a malformed or oversized frame.
    Frame(frame::FrameError),
    /// The peer closed the connection between frames.
    Closed,
    /// The peer closed the connection in the middle of a frame.
    Truncated,
    /// The deadline passed before a full frame arrived.
    Timeout,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Frame(err) => write!(f, "bad frame: {err}"),
            Self::Closed => f.write_str("connection closed"),
            Self::Truncated => f.write_str("connection closed mid-frame"),
            Self::Timeout => f.write_str("timed out waiting for a frame"),
        }
    }
}

impl Error for NetError {}

/// A client connection that reads and writes whole frames.
#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Connection {
    /// Wraps an accepted stream.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buf: Vec::new(),
        }
    }

    /// The peer's address, if still known.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }

    /// Reads one frame payload of at most `max_len` bytes.
    ///
    /// The internal buffer never grows past `max_len` plus a small constant,
    /// because oversized lengths are rejected as soon as the prefix is read.
    pub async fn read_frame(&mut self, max_len: usize) -> Result<Vec<u8>, NetError> {
        loop {
            let decoded = frame::decode_frame(&self.buf, max_len).map_err(NetError::Frame)?;
            if let Some((payload, used)) = decoded {
                let payload = payload.to_vec();
                self.buf.drain(..used);
                return Ok(payload);
            }
            let mut chunk = [0_u8; 4096];
            let read = self.stream.read(&mut chunk).await.map_err(NetError::Io)?;
            if read == 0 {
                if self.buf.is_empty() {
                    return Err(NetError::Closed);
                }
                return Err(NetError::Truncated);
            }
            self.buf.extend_from_slice(&chunk[..read]);
        }
    }

    /// Like [`Connection::read_frame`], but fails with [`NetError::Timeout`] once
    /// `deadline` passes. Use one deadline for a whole multi-packet exchange.
    pub async fn read_frame_until(
        &mut self,
        max_len: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, NetError> {
        match tokio::time::timeout_at(deadline, self.read_frame(max_len)).await {
            Ok(result) => result,
            Err(_) => Err(NetError::Timeout),
        }
    }

    /// Writes one frame.
    pub async fn write_frame(&mut self, payload: &[u8], max_len: usize) -> Result<(), NetError> {
        let mut out = Vec::with_capacity(payload.len() + 5);
        frame::encode_frame(payload, max_len, &mut out).map_err(NetError::Frame)?;
        self.stream.write_all(&out).await.map_err(NetError::Io)
    }

    /// Flushes and closes the write side.
    pub async fn close(&mut self) -> Result<(), NetError> {
        self.stream.shutdown().await.map_err(NetError::Io)
    }
}

/// Accepts connections until `shutdown` becomes true, handing each to `handler`.
///
/// Connections beyond `limits.max_connections` are dropped immediately.
pub async fn serve<H, F>(
    listener: TcpListener,
    limits: NetLimits,
    mut shutdown: watch::Receiver<bool>,
    handler: H,
) where
    H: Fn(Connection, NetLimits) -> F + Send + Sync + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let permits = Arc::new(tokio::sync::Semaphore::new(limits.max_connections));
    let handler = Arc::new(handler);
    loop {
        if *shutdown.borrow() {
            break;
        }
        let accepted = tokio::select! {
            result = listener.accept() => result,
            _ = shutdown.changed() => break,
        };
        let (stream, addr) = match accepted {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!("accept failed: {err}");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            tracing::warn!("rejecting {addr}: connection limit reached");
            continue;
        };
        if let Err(err) = stream.set_nodelay(true) {
            tracing::debug!("cannot set TCP_NODELAY for {addr}: {err}");
        }
        let handler = Arc::clone(&handler);
        let task_limits = limits.clone();
        tokio::spawn(async move {
            let _permit = permit;
            handler(Connection::new(stream), task_limits).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    async fn pair() -> (Connection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (Connection::new(server), client)
    }

    #[tokio::test]
    async fn reads_a_frame() {
        let (mut conn, mut client) = pair().await;
        client.write_all(&[3, 1, 2, 3]).await.unwrap();
        assert_eq!(conn.read_frame(1024).await.unwrap(), [1, 2, 3]);
    }

    #[tokio::test]
    async fn reassembles_a_frame_split_across_writes() {
        let (mut conn, mut client) = pair().await;
        client.write_all(&[4, 9]).await.unwrap();
        client.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        client.write_all(&[8, 7, 6, 2, 1, 1]).await.unwrap();
        assert_eq!(conn.read_frame(1024).await.unwrap(), [9, 8, 7, 6]);
        assert_eq!(conn.read_frame(1024).await.unwrap(), [1, 1]);
    }

    #[tokio::test]
    async fn times_out_when_nothing_arrives() {
        let (mut conn, _client) = pair().await;
        let deadline = Instant::now() + Duration::from_millis(50);
        let result = conn.read_frame_until(1024, deadline).await;
        assert!(matches!(result, Err(NetError::Timeout)));
    }

    #[tokio::test]
    async fn rejects_oversized_frames_without_waiting_for_the_payload() {
        let (mut conn, mut client) = pair().await;
        client.write_all(&[0xc0, 0x84, 0x3d]).await.unwrap();
        let result = conn.read_frame(1024).await;
        assert!(matches!(result, Err(NetError::Frame(_))));
    }

    #[tokio::test]
    async fn reports_clean_and_mid_frame_closes() {
        let (mut conn, client) = pair().await;
        drop(client);
        assert!(matches!(conn.read_frame(1024).await, Err(NetError::Closed)));

        let (mut conn, mut client) = pair().await;
        client.write_all(&[5, 1]).await.unwrap();
        drop(client);
        let result = conn.read_frame(1024).await;
        assert!(matches!(result, Err(NetError::Truncated)));
    }

    #[tokio::test]
    async fn writes_a_frame() {
        let (mut conn, mut client) = pair().await;
        conn.write_frame(&[0, 7], 1024).await.unwrap();
        let mut got = [0_u8; 3];
        client.read_exact(&mut got).await.unwrap();
        assert_eq!(got, [2, 0, 7]);
    }

    #[tokio::test]
    async fn serve_limits_connections_and_stops_on_shutdown() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let limits = NetLimits {
            max_connections: 1,
            ..NetLimits::default()
        };
        let (tx, rx) = watch::channel(false);
        let handled = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&handled);
        let server = tokio::spawn(serve(listener, limits, rx, move |conn, _| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(conn);
            }
        }));

        let _first = TcpStream::connect(addr).await.unwrap();
        let mut second = TcpStream::connect(addr).await.unwrap();
        let mut byte = [0_u8; 1];
        let read = second.read(&mut byte).await;
        assert!(matches!(read, Ok(0) | Err(_)));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(handled.load(Ordering::SeqCst), 1);

        tx.send(true).unwrap();
        let stopped = tokio::time::timeout(Duration::from_secs(2), server).await;
        assert!(stopped.is_ok());
    }
}
