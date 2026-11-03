//! The handshake packet, the first packet a client sends.
//!
//! The legacy (pre-1.7) server list ping is not a framed packet and is not handled
//! here; a connection that starts with it should be closed.

use crate::codec::{CodecError, Reader, write_string, write_u16};
use crate::packet::Packet;
use crate::varint::write_varint;

/// Longest server address a client may send, in UTF-16 code units.
pub const MAX_SERVER_ADDRESS_CHARS: usize = 255;

/// The connection state a handshake asks the server to move into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextState {
    /// Server list ping (`1`).
    Status,
    /// Normal login (`2`).
    Login,
    /// Login after being transferred from another server (`3`).
    Transfer,
}

impl NextState {
    fn to_wire(self) -> i32 {
        match self {
            Self::Status => 1,
            Self::Login => 2,
            Self::Transfer => 3,
        }
    }

    fn from_wire(value: i32) -> Result<Self, CodecError> {
        match value {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            _ => Err(CodecError::InvalidValue("handshake next state")),
        }
    }
}

/// Handshake (serverbound, handshaking state, id `0x00`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// The protocol version the client speaks.
    pub protocol_version: i32,
    /// The address the client connected to.
    pub server_address: String,
    /// The port the client connected to.
    pub server_port: u16,
    /// The state the client wants next.
    pub next_state: NextState,
}

impl Packet for Handshake {
    const ID: i32 = 0x00;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_varint(self.protocol_version, out);
        write_string(&self.server_address, MAX_SERVER_ADDRESS_CHARS, out)?;
        write_u16(self.server_port, out);
        write_varint(self.next_state.to_wire(), out);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            protocol_version: reader.read_varint()?,
            server_address: reader.read_string(MAX_SERVER_ADDRESS_CHARS)?,
            server_port: reader.read_u16()?,
            next_state: NextState::from_wire(reader.read_varint()?)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{MAX_PACKET_LEN, decode_frame, write_frame};
    use crate::packet::{PacketError, decode_packet, encode_packet};

    fn sample() -> Handshake {
        Handshake {
            protocol_version: 777,
            server_address: "localhost".to_owned(),
            server_port: 25565,
            next_state: NextState::Status,
        }
    }

    /// Hand-built fixture, not a real client capture. Replace with a capture once the
    /// ADR 0003 verification gate is closed.
    fn sample_bytes() -> Vec<u8> {
        let mut bytes = vec![0x00, 0x89, 0x06, 0x09];
        bytes.extend_from_slice(b"localhost");
        bytes.extend_from_slice(&[0x63, 0xDD, 0x01]);
        bytes
    }

    #[test]
    fn encodes_to_the_documented_layout() {
        assert_eq!(encode_packet(&sample()).unwrap(), sample_bytes());
    }

    #[test]
    fn decodes_the_documented_layout() {
        assert_eq!(decode_packet::<Handshake>(&sample_bytes()), Ok(sample()));
    }

    #[test]
    fn survives_framing() {
        let mut wire = Vec::new();
        write_frame(&sample_bytes(), MAX_PACKET_LEN, &mut wire).unwrap();
        assert_eq!(wire[0], 16);
        let frame = decode_frame(&wire, MAX_PACKET_LEN).unwrap().unwrap();
        assert_eq!(decode_packet::<Handshake>(frame.payload), Ok(sample()));
    }

    #[test]
    fn all_next_states_round_trip() {
        for next_state in [NextState::Status, NextState::Login, NextState::Transfer] {
            let packet = Handshake {
                next_state,
                ..sample()
            };
            let payload = encode_packet(&packet).unwrap();
            assert_eq!(decode_packet::<Handshake>(&payload), Ok(packet));
        }
    }

    #[test]
    fn unknown_next_state_is_rejected() {
        for bad in [0_u8, 4, 5] {
            let mut bytes = sample_bytes();
            *bytes.last_mut().unwrap() = bad;
            assert_eq!(
                decode_packet::<Handshake>(&bytes),
                Err(PacketError::Codec(CodecError::InvalidValue(
                    "handshake next state"
                )))
            );
        }
    }

    #[test]
    fn overlong_address_is_rejected() {
        let packet = Handshake {
            server_address: "a".repeat(MAX_SERVER_ADDRESS_CHARS + 1),
            ..sample()
        };
        assert!(encode_packet(&packet).is_err());

        let mut bytes = vec![0x00, 0x89, 0x06];
        write_varint(1000, &mut bytes);
        assert!(matches!(
            decode_packet::<Handshake>(&bytes),
            Err(PacketError::Codec(CodecError::LengthTooLarge { .. }))
        ));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = sample_bytes();
        for cut in 1..bytes.len() {
            assert!(
                decode_packet::<Handshake>(&bytes[..cut]).is_err(),
                "cut {cut}"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = sample_bytes();
        bytes.push(0);
        assert_eq!(
            decode_packet::<Handshake>(&bytes),
            Err(PacketError::Codec(CodecError::TrailingBytes(1)))
        );
    }
}
