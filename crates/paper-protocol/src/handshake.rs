//! Handshake packet (serverbound, first packet of every connection).

use crate::codec::{self, Reader};
use crate::error::{DecodeError, EncodeError};
use crate::varint;

/// Maximum characters in the server address field.
pub const MAX_ADDRESS_CHARS: usize = 255;

/// Connection state the client asks to enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextState {
    /// Server list ping.
    Status,
    /// Log in and play.
    Login,
    /// Login after a server transfer; only valid for versions that define it.
    Transfer,
}

impl NextState {
    fn from_id(id: i32) -> Result<Self, DecodeError> {
        match id {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            other => Err(DecodeError::InvalidValue("next_state", i64::from(other))),
        }
    }

    fn id(self) -> i32 {
        match self {
            Self::Status => 1,
            Self::Login => 2,
            Self::Transfer => 3,
        }
    }
}

/// The Handshake packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// Protocol version the client speaks.
    pub protocol_version: i32,
    /// Address the client connected to.
    pub server_address: String,
    /// Port the client connected to.
    pub server_port: u16,
    /// State the client wants next.
    pub next_state: NextState,
}

impl Handshake {
    /// Packet id.
    pub const ID: i32 = 0x00;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let protocol_version = reader.read_varint()?;
        let server_address = reader.read_string(MAX_ADDRESS_CHARS)?;
        let server_port = reader.read_u16()?;
        let next_state = NextState::from_id(reader.read_varint()?)?;
        Ok(Self {
            protocol_version,
            server_address,
            server_port,
            next_state,
        })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        varint::write_varint(self.protocol_version, out);
        codec::write_string(out, &self.server_address, MAX_ADDRESS_CHARS)?;
        out.extend_from_slice(&self.server_port.to_be_bytes());
        varint::write_varint(self.next_state.id(), out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(next_state: NextState) -> Handshake {
        Handshake {
            protocol_version: 1000,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state,
        }
    }

    #[test]
    fn round_trips() {
        for state in [NextState::Status, NextState::Login, NextState::Transfer] {
            let packet = sample(state);
            let mut bytes = Vec::new();
            packet.encode(&mut bytes).unwrap();
            let mut reader = Reader::new(&bytes);
            assert_eq!(reader.read_varint(), Ok(Handshake::ID));
            assert_eq!(Handshake::decode_body(&mut reader), Ok(packet));
            assert_eq!(reader.finish(), Ok(()));
        }
    }

    #[test]
    fn rejects_unknown_next_state() {
        let mut bytes = Vec::new();
        sample(NextState::Login).encode(&mut bytes).unwrap();
        let last = bytes.len() - 1;
        bytes[last] = 4;
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(Handshake::ID));
        let result = Handshake::decode_body(&mut reader);
        assert_eq!(result, Err(DecodeError::InvalidValue("next_state", 4)));
    }

    #[test]
    fn rejects_oversized_address() {
        let mut packet = sample(NextState::Status);
        packet.server_address = "a".repeat(MAX_ADDRESS_CHARS + 1);
        let mut bytes = Vec::new();
        let result = packet.encode(&mut bytes);
        assert_eq!(result, Err(EncodeError::StringTooLong(MAX_ADDRESS_CHARS)));
    }

    #[test]
    fn rejects_truncated_body() {
        let mut bytes = Vec::new();
        sample(NextState::Login).encode(&mut bytes).unwrap();
        bytes.truncate(bytes.len() - 2);
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.read_varint(), Ok(Handshake::ID));
        let result = Handshake::decode_body(&mut reader);
        assert_eq!(result, Err(DecodeError::UnexpectedEof));
    }
}
