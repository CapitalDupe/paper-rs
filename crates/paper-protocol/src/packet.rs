//! The [`Packet`] trait and helpers for encoding and decoding whole packets.
//!
//! A packet payload (the contents of one frame) is a `VarInt` packet id followed by the
//! packet body. Packet ids are only unique within a connection state and direction, so
//! each packet type documents which it belongs to.

use std::error::Error;
use std::fmt;

use crate::codec::{CodecError, Reader};
use crate::varint::write_varint;

/// A protocol packet that can be encoded and decoded.
///
/// Both directions are implemented for every packet so that tests and a future test
/// client can round-trip them; the type's documentation says which side sends it.
pub trait Packet: Sized {
    /// The packet id, valid within the connection state and direction the type documents.
    const ID: i32;

    /// Appends the packet body (without the id) to `out`.
    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError>;

    /// Reads the packet body (without the id). The caller checks for trailing bytes.
    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError>;
}

/// Errors produced while decoding a whole packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    /// The body was malformed.
    Codec(CodecError),
    /// The packet id did not match the expected packet.
    UnexpectedId {
        /// The id the decoder was asked for.
        expected: i32,
        /// The id found in the payload.
        actual: i32,
    },
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(err) => write!(f, "malformed packet: {err}"),
            Self::UnexpectedId { expected, actual } => {
                write!(f, "expected packet id {expected:#04x}, got {actual:#04x}")
            }
        }
    }
}

impl Error for PacketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Codec(err) => Some(err),
            Self::UnexpectedId { .. } => None,
        }
    }
}

impl From<CodecError> for PacketError {
    fn from(err: CodecError) -> Self {
        Self::Codec(err)
    }
}

/// Reads the packet id from the front of a payload without consuming it.
pub fn peek_packet_id(payload: &[u8]) -> Result<i32, CodecError> {
    Reader::new(payload).read_varint()
}

/// Encodes `packet` as a payload: the id followed by the body.
pub fn encode_packet<P: Packet>(packet: &P) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    write_varint(P::ID, &mut out);
    packet.encode_body(&mut out)?;
    Ok(out)
}

/// Decodes a payload as packet `P`, requiring the right id and no trailing bytes.
pub fn decode_packet<P: Packet>(payload: &[u8]) -> Result<P, PacketError> {
    let mut reader = Reader::new(payload);
    let actual = reader.read_varint()?;
    if actual != P::ID {
        return Err(PacketError::UnexpectedId {
            expected: P::ID,
            actual,
        });
    }
    let packet = P::decode_body(&mut reader)?;
    reader.finish()?;
    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct Probe(u16);

    impl Packet for Probe {
        const ID: i32 = 0x2A;

        fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
            crate::codec::write_u16(self.0, out);
            Ok(())
        }

        fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
            Ok(Self(reader.read_u16()?))
        }
    }

    #[test]
    fn round_trip() {
        let payload = encode_packet(&Probe(513)).unwrap();
        assert_eq!(payload, [0x2A, 0x02, 0x01]);
        assert_eq!(peek_packet_id(&payload), Ok(0x2A));
        assert_eq!(decode_packet::<Probe>(&payload), Ok(Probe(513)));
    }

    #[test]
    fn wrong_id_is_reported() {
        assert_eq!(
            decode_packet::<Probe>(&[0x01, 0, 0]),
            Err(PacketError::UnexpectedId {
                expected: 0x2A,
                actual: 1
            })
        );
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        assert_eq!(
            decode_packet::<Probe>(&[0x2A, 0, 1, 9]),
            Err(PacketError::Codec(CodecError::TrailingBytes(1)))
        );
    }

    #[test]
    fn truncated_body_is_rejected() {
        assert_eq!(
            decode_packet::<Probe>(&[0x2A, 0]),
            Err(PacketError::Codec(CodecError::UnexpectedEof))
        );
    }
}
