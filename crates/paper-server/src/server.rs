//! Server orchestration: wires networking, the tick loop, and shutdown together.
//!
//! [`run`] binds the listener and runs until the `stop` future completes. It then
//! tells every task to stop, waits (up to a grace period) for open connections to
//! close cleanly, and returns.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::Config;
use crate::net::{self, NetLimits};
use crate::session::{self, ServerInfo};
use crate::tick::{self, TickMetrics};

/// Errors that stop the server from starting.
#[derive(Debug)]
pub enum ServerError {
    /// `bind_address` is not a valid IP address.
    BadAddress,
    /// The listening socket could not be bound.
    Bind(io::Error),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadAddress => f.write_str("bind_address is not a valid IP address"),
            Self::Bind(err) => write!(f, "cannot bind the listening socket: {err}"),
        }
    }
}

impl Error for ServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BadAddress => None,
            Self::Bind(err) => Some(err),
        }
    }
}

/// Binds the configured address and runs until `stop` completes.
pub async fn run<S>(config: Config, stop: S) -> Result<(), ServerError>
where
    S: Future<Output = ()>,
{
    let Ok(ip) = config.bind_address.parse::<IpAddr>() else {
        return Err(ServerError::BadAddress);
    };
    let addr = SocketAddr::new(ip, config.port);
    let listener = TcpListener::bind(addr).await.map_err(ServerError::Bind)?;
    run_with_listener(config, listener, stop).await
}

/// Runs on an already-bound listener until `stop` completes.
pub async fn run_with_listener<S>(
    config: Config,
    listener: TcpListener,
    stop: S,
) -> Result<(), ServerError>
where
    S: Future<Output = ()>,
{
    if let Ok(addr) = listener.local_addr() {
        tracing::info!("{} listening on {addr}", crate::SERVER_NAME);
    }
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let info = ServerInfo::from_config(&config);
    let players = usize::try_from(config.max_players).unwrap_or(usize::MAX);
    let limits = NetLimits {
        max_connections: players.saturating_mul(4).clamp(64, 1 << 20),
        ..NetLimits::default()
    };
    let grace = limits.shutdown_grace;

    let conn_shutdown = shutdown_rx.clone();
    let server_task = tokio::spawn(net::serve(
        listener,
        limits,
        shutdown_rx.clone(),
        move |conn, limits| {
            session::run_connection(conn, limits, info.clone(), conn_shutdown.clone())
        },
    ));
    let metrics = Arc::new(TickMetrics::default());
    let tick_task = tokio::spawn(tick::run(Arc::clone(&metrics), shutdown_rx, |number| {
        if number.checked_rem(1200) == Some(0) {
            tracing::debug!(tick = number, "server running");
        }
    }));

    stop.await;
    tracing::info!("shutting down {}", crate::SERVER_NAME);
    let _ = shutdown_tx.send(true);
    if tokio::time::timeout(grace + Duration::from_secs(1), server_task)
        .await
        .is_err()
    {
        tracing::warn!("network shutdown timed out");
    }
    let _ = tick_task.await;
    let snapshot = metrics.snapshot();
    tracing::info!(
        "ticks run: {}, slow: {}, skipped: {}",
        snapshot.ticks,
        snapshot.slow_ticks,
        snapshot.skipped_ticks
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use paper_protocol::codec::Reader;
    use paper_protocol::handshake::{Handshake, NextState};
    use paper_protocol::status::{PingRequest, PongResponse};
    use tokio::net::TcpStream;
    use tokio::sync::oneshot;

    use super::*;
    use crate::net::{Connection, NetError};

    fn test_config() -> Config {
        Config {
            online_mode: false,
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn rejects_an_invalid_bind_address() {
        let config = Config {
            bind_address: "not-an-ip".to_owned(),
            ..Config::default()
        };
        let result = run(config, async {}).await;
        assert!(matches!(result, Err(ServerError::BadAddress)));
    }

    #[tokio::test]
    async fn serves_a_ping_then_shuts_down_cleanly() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(run_with_listener(test_config(), listener, async move {
            let _ = stop_rx.await;
        }));

        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        let mut handshake = Vec::new();
        let packet = Handshake {
            protocol_version: 1000,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state: NextState::Status,
        };
        packet.encode(&mut handshake).unwrap();
        client.write_frame(&handshake, 1024).await.unwrap();
        let mut ping = Vec::new();
        PingRequest { payload: 5 }.encode(&mut ping);
        client.write_frame(&ping, 1024).await.unwrap();
        let reply = client.read_frame(1 << 20).await.unwrap();
        let mut reader = Reader::new(&reply);
        assert_eq!(reader.read_varint(), Ok(PongResponse::ID));
        assert_eq!(PongResponse::decode_body(&mut reader).unwrap().payload, 5);

        stop_tx.send(()).unwrap();
        let done = tokio::time::timeout(Duration::from_secs(5), server).await;
        assert!(matches!(done, Ok(Ok(Ok(())))));
    }

    #[tokio::test]
    async fn shutdown_closes_idle_connections() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(run_with_listener(test_config(), listener, async move {
            let _ = stop_rx.await;
        }));

        let mut idle = Connection::new(TcpStream::connect(addr).await.unwrap());
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop_tx.send(()).unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(3), idle.read_frame(1024)).await;
        assert!(matches!(closed, Ok(Err(NetError::Closed))));
        let done = tokio::time::timeout(Duration::from_secs(5), server).await;
        assert!(matches!(done, Ok(Ok(Ok(())))));
    }
}
