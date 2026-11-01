//! Per-connection protocol state machine.
//!
//! [`Session`] is pure: it turns one incoming packet payload into the replies to
//! send and the next state, with no I/O, so every transition is unit-testable.
//! [`run_connection`] drives a session over a [`Connection`] with the login
//! deadline applied.
//!
//! The `Play` state is a placeholder: logging in succeeds, but no play packets are
//! handled yet, and any further packet closes the connection. Online-mode login
//! (encryption and session verification) is not implemented, so online-mode
//! servers refuse logins with a clear message instead of accepting unverified
//! players.

use std::error::Error;
use std::fmt;

use paper_protocol::codec::Reader;
use paper_protocol::error::{DecodeError, EncodeError};
use paper_protocol::handshake::{Handshake, NextState};
use paper_protocol::login::{LoginDisconnect, LoginStart, LoginSuccess};
use paper_protocol::status::{PingRequest, PongResponse, StatusRequest, StatusResponse};
use tokio::time::Instant;

use crate::config::Config;
use crate::net::{Connection, NetError, NetLimits, PRE_LOGIN_MAX_FRAME_LEN};

/// Protocol state of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Waiting for the Handshake packet.
    Handshaking,
    /// Server list ping.
    Status,
    /// Logging in.
    Login,
    /// Logged in (placeholder; no play packets are handled yet).
    Play,
    /// Finished; the connection should be closed.
    Closed,
}

/// What the status response reports about the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// Message of the day.
    pub motd: String,
    /// Maximum players.
    pub max_players: u32,
    /// Players currently online.
    pub online_players: u32,
    /// Version name shown to clients.
    pub version_name: String,
    /// Protocol number; -1 means "no compatible version" until a target is chosen.
    pub protocol_version: i32,
    /// Whether logins must be verified against Mojang.
    pub online_mode: bool,
}

impl ServerInfo {
    /// Builds server info from configuration.
    pub fn from_config(config: &Config) -> Self {
        Self {
            motd: config.motd.clone(),
            max_players: config.max_players,
            online_players: 0,
            version_name: "paper-rs (unreleased)".to_owned(),
            protocol_version: -1,
            online_mode: config.online_mode,
        }
    }

    /// The JSON body of the status response.
    pub fn status_json(&self) -> String {
        serde_json::json!({
            "version": { "name": self.version_name, "protocol": self.protocol_version },
            "players": { "max": self.max_players, "online": self.online_players },
            "description": { "text": self.motd },
        })
        .to_string()
    }
}

/// The result of handling one packet.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Packet payloads (id and body) to send, in order.
    pub replies: Vec<Vec<u8>>,
    /// Whether to close the connection after sending the replies.
    pub close: bool,
}

/// Errors produced while handling a packet.
#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    /// The packet body was malformed.
    Decode(DecodeError),
    /// A reply could not be encoded.
    Encode(EncodeError),
    /// The packet id is not valid in the current state: `(state, id)`.
    UnexpectedPacket(State, i32),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(err) => write!(f, "malformed packet: {err}"),
            Self::Encode(err) => write!(f, "cannot encode reply: {err}"),
            Self::UnexpectedPacket(state, id) => {
                write!(f, "unexpected packet {id:#04x} in state {state:?}")
            }
        }
    }
}

impl Error for SessionError {}

impl From<DecodeError> for SessionError {
    fn from(err: DecodeError) -> Self {
        Self::Decode(err)
    }
}

impl From<EncodeError> for SessionError {
    fn from(err: EncodeError) -> Self {
        Self::Encode(err)
    }
}

fn is_valid_name(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '_';
    !name.is_empty() && name.len() <= 16 && name.chars().all(allowed)
}

/// Protocol state for one connection.
#[derive(Debug)]
pub struct Session {
    state: State,
    info: ServerInfo,
}

impl Session {
    /// Starts a session in the handshaking state.
    pub fn new(info: ServerInfo) -> Self {
        Self {
            state: State::Handshaking,
            info,
        }
    }

    /// The current state.
    pub fn state(&self) -> State {
        self.state
    }

    /// Handles one packet payload (packet id and body).
    pub fn handle_packet(&mut self, payload: &[u8]) -> Result<Outcome, SessionError> {
        let mut reader = Reader::new(payload);
        let id = reader.read_varint()?;
        match self.state {
            State::Handshaking => self.on_handshake(id, reader),
            State::Status => self.on_status(id, reader),
            State::Login => self.on_login(id, reader),
            State::Play | State::Closed => Err(SessionError::UnexpectedPacket(self.state, id)),
        }
    }

    fn on_handshake(&mut self, id: i32, mut reader: Reader<'_>) -> Result<Outcome, SessionError> {
        if id != Handshake::ID {
            return Err(SessionError::UnexpectedPacket(self.state, id));
        }
        let packet = Handshake::decode_body(&mut reader)?;
        reader.finish()?;
        self.state = match packet.next_state {
            NextState::Status => State::Status,
            NextState::Login | NextState::Transfer => State::Login,
        };
        Ok(Outcome::default())
    }

    fn on_status(&mut self, id: i32, mut reader: Reader<'_>) -> Result<Outcome, SessionError> {
        let mut out = Vec::new();
        match id {
            StatusRequest::ID => {
                StatusRequest::decode_body(&mut reader)?;
                reader.finish()?;
                let json = self.info.status_json();
                StatusResponse { json }.encode(&mut out)?;
                Ok(Outcome {
                    replies: vec![out],
                    close: false,
                })
            }
            PingRequest::ID => {
                let ping = PingRequest::decode_body(&mut reader)?;
                reader.finish()?;
                PongResponse {
                    payload: ping.payload,
                }
                .encode(&mut out);
                self.state = State::Closed;
                Ok(Outcome {
                    replies: vec![out],
                    close: true,
                })
            }
            other => Err(SessionError::UnexpectedPacket(self.state, other)),
        }
    }

    fn on_login(&mut self, id: i32, mut reader: Reader<'_>) -> Result<Outcome, SessionError> {
        if id != LoginStart::ID {
            return Err(SessionError::UnexpectedPacket(self.state, id));
        }
        let start = LoginStart::decode_body(&mut reader)?;
        reader.finish()?;
        if !is_valid_name(&start.name) {
            return self.disconnect("Invalid player name.");
        }
        if self.info.online_mode {
            return self.disconnect("Online mode is not supported yet.");
        }
        let success = LoginSuccess {
            uuid: start.uuid,
            username: start.name,
            properties: Vec::new(),
        };
        let mut out = Vec::new();
        success.encode(&mut out)?;
        self.state = State::Play;
        Ok(Outcome {
            replies: vec![out],
            close: false,
        })
    }

    fn disconnect(&mut self, message: &str) -> Result<Outcome, SessionError> {
        let reason_json = serde_json::json!({ "text": message }).to_string();
        let mut out = Vec::new();
        LoginDisconnect { reason_json }.encode(&mut out)?;
        self.state = State::Closed;
        Ok(Outcome {
            replies: vec![out],
            close: true,
        })
    }
}

/// Drives a session over `conn` until it closes, errors, or times out.
///
/// Until login completes, the whole exchange must finish within
/// `limits.login_timeout`, and frames are capped at
/// [`PRE_LOGIN_MAX_FRAME_LEN`].
pub async fn run_connection(mut conn: Connection, limits: NetLimits, info: ServerInfo) {
    let peer = conn.peer_addr();
    let deadline = Instant::now() + limits.login_timeout;
    let mut session = Session::new(info);
    'conn: loop {
        let read = if session.state() == State::Play {
            conn.read_frame(limits.max_frame_len).await
        } else {
            conn.read_frame_until(PRE_LOGIN_MAX_FRAME_LEN, deadline)
                .await
        };
        let payload = match read {
            Ok(payload) => payload,
            Err(NetError::Closed) => break,
            Err(err) => {
                tracing::debug!(?peer, "closing connection: {err}");
                break;
            }
        };
        let outcome = match session.handle_packet(&payload) {
            Ok(outcome) => outcome,
            Err(err) => {
                tracing::debug!(?peer, "closing connection: {err}");
                break;
            }
        };
        for reply in &outcome.replies {
            if let Err(err) = conn.write_frame(reply, limits.max_frame_len).await {
                tracing::debug!(?peer, "write failed: {err}");
                break 'conn;
            }
        }
        if outcome.close {
            break;
        }
    }
    if let Err(err) = conn.close().await {
        tracing::debug!(?peer, "close failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use super::*;
    use crate::net::serve;

    fn info(online_mode: bool) -> ServerInfo {
        ServerInfo {
            motd: "hello".to_owned(),
            max_players: 20,
            online_players: 0,
            version_name: "test".to_owned(),
            protocol_version: -1,
            online_mode,
        }
    }

    fn handshake(next_state: NextState) -> Vec<u8> {
        let packet = Handshake {
            protocol_version: 1000,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state,
        };
        let mut out = Vec::new();
        packet.encode(&mut out).unwrap();
        out
    }

    fn login_start(name: &str) -> Vec<u8> {
        let packet = LoginStart {
            name: name.to_owned(),
            uuid: [5; 16],
        };
        let mut out = Vec::new();
        packet.encode(&mut out).unwrap();
        out
    }

    fn open(bytes: &[u8]) -> (i32, Reader<'_>) {
        let mut reader = Reader::new(bytes);
        let id = reader.read_varint().unwrap();
        (id, reader)
    }

    #[test]
    fn status_flow_reports_server_info_and_echoes_ping() {
        let mut session = Session::new(info(false));
        session
            .handle_packet(&handshake(NextState::Status))
            .unwrap();
        assert_eq!(session.state(), State::Status);

        let mut request = Vec::new();
        StatusRequest.encode(&mut request);
        let outcome = session.handle_packet(&request).unwrap();
        assert!(!outcome.close);
        let (id, mut reader) = open(&outcome.replies[0]);
        assert_eq!(id, StatusResponse::ID);
        let response = StatusResponse::decode_body(&mut reader).unwrap();
        let json: serde_json::Value = serde_json::from_str(&response.json).unwrap();
        assert_eq!(json["description"]["text"], "hello");
        assert_eq!(json["players"]["max"], 20);

        let mut ping = Vec::new();
        PingRequest { payload: 99 }.encode(&mut ping);
        let outcome = session.handle_packet(&ping).unwrap();
        assert!(outcome.close);
        assert_eq!(session.state(), State::Closed);
        let (id, mut reader) = open(&outcome.replies[0]);
        assert_eq!(id, PongResponse::ID);
        assert_eq!(PongResponse::decode_body(&mut reader).unwrap().payload, 99);
    }

    #[test]
    fn offline_login_succeeds() {
        let mut session = Session::new(info(false));
        session.handle_packet(&handshake(NextState::Login)).unwrap();
        assert_eq!(session.state(), State::Login);
        let outcome = session.handle_packet(&login_start("Steve")).unwrap();
        assert!(!outcome.close);
        assert_eq!(session.state(), State::Play);
        let (id, mut reader) = open(&outcome.replies[0]);
        assert_eq!(id, LoginSuccess::ID);
        let success = LoginSuccess::decode_body(&mut reader).unwrap();
        assert_eq!(success.username, "Steve");
        assert_eq!(success.uuid, [5; 16]);
    }

    #[test]
    fn online_mode_refuses_login_for_now() {
        let mut session = Session::new(info(true));
        session.handle_packet(&handshake(NextState::Login)).unwrap();
        let outcome = session.handle_packet(&login_start("Steve")).unwrap();
        assert!(outcome.close);
        assert_eq!(session.state(), State::Closed);
        let (id, _) = open(&outcome.replies[0]);
        assert_eq!(id, LoginDisconnect::ID);
    }

    #[test]
    fn invalid_names_are_refused() {
        let cases = [
            ("bad name", true),
            ("x", false),
            ("a-b", true),
            ("", true),
            ("Steve_01", false),
        ];
        for (name, refused) in cases {
            let mut session = Session::new(info(false));
            session.handle_packet(&handshake(NextState::Login)).unwrap();
            let outcome = session.handle_packet(&login_start(name)).unwrap();
            assert_eq!(outcome.close, refused, "name {name:?}");
        }
    }

    #[test]
    fn packets_out_of_order_are_rejected() {
        let mut session = Session::new(info(false));
        let mut ping = Vec::new();
        PingRequest { payload: 1 }.encode(&mut ping);
        assert_eq!(
            session.handle_packet(&ping),
            Err(SessionError::UnexpectedPacket(State::Handshaking, 1))
        );
        assert_eq!(
            session.handle_packet(&login_start("Steve")),
            Err(SessionError::Decode(DecodeError::UnexpectedEof))
        );
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut session = Session::new(info(false));
        let mut bytes = handshake(NextState::Status);
        bytes.push(0);
        assert_eq!(
            session.handle_packet(&bytes),
            Err(SessionError::Decode(DecodeError::TrailingBytes(1)))
        );
    }

    #[test]
    fn play_state_rejects_every_packet_for_now() {
        let mut session = Session::new(info(false));
        session.handle_packet(&handshake(NextState::Login)).unwrap();
        session.handle_packet(&login_start("Steve")).unwrap();
        assert_eq!(
            session.handle_packet(&[0x10]),
            Err(SessionError::UnexpectedPacket(State::Play, 0x10))
        );
    }

    async fn start_server(login_timeout: Duration) -> (std::net::SocketAddr, watch::Sender<bool>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let limits = NetLimits {
            login_timeout,
            ..NetLimits::default()
        };
        let (tx, rx) = watch::channel(false);
        let server_info = info(false);
        tokio::spawn(serve(listener, limits, rx, move |conn, limits| {
            run_connection(conn, limits, server_info.clone())
        }));
        (addr, tx)
    }

    #[tokio::test]
    async fn end_to_end_status_ping() {
        let (addr, _shutdown) = start_server(Duration::from_secs(5)).await;
        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        client
            .write_frame(&handshake(NextState::Status), 1024)
            .await
            .unwrap();
        let mut ping = Vec::new();
        PingRequest { payload: 7 }.encode(&mut ping);
        client.write_frame(&ping, 1024).await.unwrap();
        let reply = client.read_frame(1 << 20).await.unwrap();
        let (id, mut reader) = open(&reply);
        assert_eq!(id, PongResponse::ID);
        assert_eq!(PongResponse::decode_body(&mut reader).unwrap().payload, 7);
        assert!(matches!(
            client.read_frame(1024).await,
            Err(NetError::Closed)
        ));
    }

    #[tokio::test]
    async fn silent_clients_are_dropped_after_the_login_timeout() {
        let (addr, _shutdown) = start_server(Duration::from_millis(100)).await;
        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        client
            .write_frame(&handshake(NextState::Login), 1024)
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), client.read_frame(1024)).await;
        assert!(matches!(result, Ok(Err(NetError::Closed))));
    }
}
