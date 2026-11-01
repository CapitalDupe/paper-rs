//! Status packets (server list ping).

use crate::codec::{self, Reader};
use crate::error::{DecodeError, EncodeError};
use crate::varint;

/// Maximum characters in the status response JSON.
pub const MAX_JSON_CHARS: usize = 32_767;

/// Serverbound Status Request (empty body).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusRequest;

impl StatusRequest {
    /// Packet id.
    pub const ID: i32 = 0x00;

    /// Decodes the body (there are no fields).
    pub fn decode_body(_reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self)
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) {
        varint::write_varint(Self::ID, out);
    }
}

/// Clientbound Status Response carrying the server list JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    /// JSON text describing the server.
    pub json: String,
}

impl StatusResponse {
    /// Packet id.
    pub const ID: i32 = 0x00;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let json = reader.read_string(MAX_JSON_CHARS)?;
        Ok(Self { json })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        codec::write_string(out, &self.json, MAX_JSON_CHARS)
    }
}

/// Serverbound Ping Request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingRequest {
    /// Opaque value the server echoes back.
    pub payload: i64,
}

impl PingRequest {
    /// Packet id.
    pub const ID: i32 = 0x01;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let payload = reader.read_i64()?;
        Ok(Self { payload })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) {
        varint::write_varint(Self::ID, out);
        out.extend_from_slice(&self.payload.to_be_bytes());
    }
}

/// Clientbound Pong Response echoing the ping payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PongResponse {
    /// The value from the matching [`PingRequest`].
    pub payload: i64,
}

impl PongResponse {
    /// Packet id.
    pub const ID: i32 = 0x01;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let payload = reader.read_i64()?;
        Ok(Self { payload })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) {
        varint::write_varint(Self::ID, out);
        out.extend_from_slice(&self.payload.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_request_round_trips() {
        let mut bytes = Vec::new();
        StatusRequest.encode(&mut bytes);
        assert_eq!(bytes, [0x00]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(StatusRequest::ID));
        assert_eq!(StatusRequest::decode_body(&mut reader), Ok(StatusRequest));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn status_response_round_trips() {
        let packet = StatusResponse {
            json: "{\"description\":\"hi\"}".to_owned(),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(StatusResponse::ID));
        assert_eq!(StatusResponse::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn status_response_rejects_oversized_json() {
        let packet = StatusResponse {
            json: "a".repeat(MAX_JSON_CHARS + 1),
        };
        let mut bytes = Vec::new();
        let result = packet.encode(&mut bytes);
        assert_eq!(result, Err(EncodeError::StringTooLong(MAX_JSON_CHARS)));
    }

    #[test]
    fn ping_and_pong_round_trip() {
        let ping = PingRequest { payload: -42 };
        let mut bytes = Vec::new();
        ping.encode(&mut bytes);
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(PingRequest::ID));
        assert_eq!(PingRequest::decode_body(&mut reader), Ok(ping));
        assert_eq!(reader.finish(), Ok(()));

        let pong = PongResponse { payload: ping.payload };
        let mut bytes = Vec::new();
        pong.encode(&mut bytes);
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(PongResponse::ID));
        assert_eq!(PongResponse::decode_body(&mut reader), Ok(pong));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn ping_rejects_truncated_payload() {
        let mut reader = Reader::new(&[0, 0, 0]);
        let result = PingRequest::decode_body(&mut reader);
        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }
}
