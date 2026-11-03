//! Packets for the status (server list ping) state.
//!
//! The usual exchange is: the client sends [`StatusRequest`], the server answers with a
//! [`StatusResponse`], the client sends a [`PingRequest`], and the server echoes its
//! payload in a [`PongResponse`] and closes the connection.

use crate::codec::{CodecError, Reader, write_i64, write_string};
use crate::packet::Packet;

/// Longest status JSON the protocol allows, in UTF-16 code units.
pub const MAX_STATUS_JSON_CHARS: usize = 32_767;

/// Status request (serverbound, status state, id `0x00`). Carries no data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequest;

impl Packet for StatusRequest {
    const ID: i32 = 0x00;

    fn encode_body(&self, _out: &mut Vec<u8>) -> Result<(), CodecError> {
        Ok(())
    }

    fn decode_body(_reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self)
    }
}

/// Status response (clientbound, status state, id `0x00`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    /// The status as a JSON document; see [`StatusInfo::to_json`].
    pub json: String,
}

impl Packet for StatusResponse {
    const ID: i32 = 0x00;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_string(&self.json, MAX_STATUS_JSON_CHARS, out)
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            json: reader.read_string(MAX_STATUS_JSON_CHARS)?,
        })
    }
}

/// Ping request (serverbound, status state, id `0x01`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingRequest {
    /// An opaque value the client wants echoed back.
    pub payload: i64,
}

impl Packet for PingRequest {
    const ID: i32 = 0x01;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_i64(self.payload, out);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            payload: reader.read_i64()?,
        })
    }
}

/// Pong response (clientbound, status state, id `0x01`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    /// The payload from the matching [`PingRequest`].
    pub payload: i64,
}

impl Packet for PongResponse {
    const ID: i32 = 0x01;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_i64(self.payload, out);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            payload: reader.read_i64()?,
        })
    }
}

/// The server list information shown to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo<'a> {
    /// Version name shown in the server list, such as `26.3`.
    pub version_name: &'a str,
    /// The protocol number the server speaks.
    pub protocol: i32,
    /// Player cap.
    pub max_players: u32,
    /// Players currently online.
    pub online_players: u32,
    /// Message of the day, as plain text.
    pub description: &'a str,
}

impl StatusInfo<'_> {
    /// Renders the status JSON document.
    ///
    /// All text is escaped, so a message of the day containing quotes or control
    /// characters cannot break the document.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut json = String::from(r#"{"version":{"name":""#);
        push_json_escaped(self.version_name, &mut json);
        json.push_str(&format!(
            r#"","protocol":{}}},"players":{{"max":{},"online":{}}},"description":{{"text":""#,
            self.protocol, self.max_players, self.online_players
        ));
        push_json_escaped(self.description, &mut json);
        json.push_str(r#""}}"#);
        json
    }
}

fn push_json_escaped(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{PacketError, decode_packet, encode_packet};

    #[test]
    fn status_request_is_just_the_id() {
        assert_eq!(encode_packet(&StatusRequest).unwrap(), [0x00]);
        assert_eq!(decode_packet::<StatusRequest>(&[0x00]), Ok(StatusRequest));
        assert_eq!(
            decode_packet::<StatusRequest>(&[0x00, 0x00]),
            Err(PacketError::Codec(CodecError::TrailingBytes(1)))
        );
    }

    #[test]
    fn ping_and_pong_use_eight_big_endian_bytes() {
        let ping = PingRequest {
            payload: 0x0102_0304_0506_0708,
        };
        let bytes = encode_packet(&ping).unwrap();
        assert_eq!(bytes, [0x01, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(decode_packet::<PingRequest>(&bytes), Ok(ping));

        let pong = PongResponse { payload: -1 };
        let bytes = encode_packet(&pong).unwrap();
        assert_eq!(
            bytes,
            [0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(decode_packet::<PongResponse>(&bytes), Ok(pong));
    }

    #[test]
    fn truncated_ping_is_rejected() {
        for cut in 1..9 {
            assert!(
                decode_packet::<PingRequest>(&[0x01, 1, 2, 3, 4, 5, 6, 7, 8][..cut]).is_err(),
                "cut {cut}"
            );
        }
    }

    #[test]
    fn status_response_round_trips() {
        let response = StatusResponse {
            json: r#"{"ok":true}"#.to_owned(),
        };
        let bytes = encode_packet(&response).unwrap();
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[1], 11);
        assert_eq!(decode_packet::<StatusResponse>(&bytes), Ok(response));
    }

    #[test]
    fn oversized_status_json_is_refused() {
        let response = StatusResponse {
            json: "x".repeat(MAX_STATUS_JSON_CHARS + 1),
        };
        assert!(encode_packet(&response).is_err());
    }

    #[test]
    fn status_json_has_the_expected_shape() {
        let info = StatusInfo {
            version_name: "26.3",
            protocol: 777,
            max_players: 20,
            online_players: 0,
            description: "A paper-rs server",
        };
        assert_eq!(
            info.to_json(),
            r#"{"version":{"name":"26.3","protocol":777},"players":{"max":20,"online":0},"description":{"text":"A paper-rs server"}}"#
        );
    }

    #[test]
    fn status_json_escapes_text() {
        let info = StatusInfo {
            version_name: "a\"b",
            protocol: 1,
            max_players: 1,
            online_players: 0,
            description: "He said \"hi\"\n\\ \u{1}",
        };
        let json = info.to_json();
        assert!(json.contains(r#""name":"a\"b""#));
        assert!(json.contains(r#""text":"He said \"hi\"\n\\ \u0001""#));
        // No raw control characters may survive.
        assert!(!json.chars().any(|c| u32::from(c) < 0x20));
    }
}
