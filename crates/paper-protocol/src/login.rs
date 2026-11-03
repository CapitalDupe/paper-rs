//! Packets for the login state.
//!
//! The field layouts here follow the protocol documentation for the 1.21 line. They have
//! not yet been confirmed against a real 26.3 client; that is the open item in
//! ADR 0003, and the byte fixtures below are hand-built, not captures.
//!
//! Typical online-mode exchange: the client sends [`LoginStart`]; the server answers
//! with [`EncryptionRequest`]; the client replies with [`EncryptionResponse`]; the
//! server enables encryption, optionally sends [`SetCompression`], then sends
//! [`LoginSuccess`]; the client confirms with [`LoginAcknowledged`]. A server can end
//! the exchange at any point with [`LoginDisconnect`].

use crate::codec::{CodecError, Reader, write_bool, write_bytes, write_string};
use crate::packet::Packet;
use crate::varint::write_varint;

/// Longest player name, in UTF-16 code units.
pub const MAX_NAME_CHARS: usize = 16;
/// Longest server id string in an encryption request.
pub const MAX_SERVER_ID_CHARS: usize = 20;
/// Largest key, token or secret byte array accepted in the encryption exchange.
///
/// A 1024-bit RSA public key is 162 bytes and an encrypted block is 128 bytes; this
/// leaves room for larger keys while keeping reads bounded.
pub const MAX_CRYPTO_BYTES: usize = 512;
/// Most profile properties a login success may carry.
pub const MAX_PROPERTIES: usize = 16;
/// Longest profile property name.
pub const MAX_PROPERTY_NAME_CHARS: usize = 64;
/// Longest profile property value.
pub const MAX_PROPERTY_VALUE_CHARS: usize = 32_767;
/// Longest profile property signature.
pub const MAX_PROPERTY_SIGNATURE_CHARS: usize = 1_024;
/// Longest disconnect reason (a JSON text component).
pub const MAX_DISCONNECT_REASON_CHARS: usize = 262_144;

/// Login start (serverbound, login state, id `0x00`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// The player's name.
    pub name: String,
    /// The player's UUID as sent by the client. Not trustworthy before authentication.
    pub uuid: [u8; 16],
}

impl Packet for LoginStart {
    const ID: i32 = 0x00;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_string(&self.name, MAX_NAME_CHARS, out)?;
        out.extend_from_slice(&self.uuid);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            name: reader.read_string(MAX_NAME_CHARS)?,
            uuid: reader.read_array()?,
        })
    }
}

/// Encryption request (clientbound, login state, id `0x01`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionRequest {
    /// Server id used in the session hash; empty for vanilla-compatible behavior.
    pub server_id: String,
    /// The server's RSA public key in DER form.
    pub public_key: Vec<u8>,
    /// Random bytes the client must return encrypted, proving it holds the key.
    pub verify_token: Vec<u8>,
    /// Whether the client should authenticate with Mojang's session servers.
    pub should_authenticate: bool,
}

impl Packet for EncryptionRequest {
    const ID: i32 = 0x01;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_string(&self.server_id, MAX_SERVER_ID_CHARS, out)?;
        write_bytes(&self.public_key, MAX_CRYPTO_BYTES, out)?;
        write_bytes(&self.verify_token, MAX_CRYPTO_BYTES, out)?;
        write_bool(self.should_authenticate, out);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            server_id: reader.read_string(MAX_SERVER_ID_CHARS)?,
            public_key: reader.read_bytes(MAX_CRYPTO_BYTES)?.to_vec(),
            verify_token: reader.read_bytes(MAX_CRYPTO_BYTES)?.to_vec(),
            should_authenticate: reader.read_bool()?,
        })
    }
}

/// Encryption response (serverbound, login state, id `0x01`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionResponse {
    /// The shared secret, encrypted with the server's public key.
    pub shared_secret: Vec<u8>,
    /// The verify token, encrypted with the server's public key.
    pub verify_token: Vec<u8>,
}

impl Packet for EncryptionResponse {
    const ID: i32 = 0x01;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_bytes(&self.shared_secret, MAX_CRYPTO_BYTES, out)?;
        write_bytes(&self.verify_token, MAX_CRYPTO_BYTES, out)
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            shared_secret: reader.read_bytes(MAX_CRYPTO_BYTES)?.to_vec(),
            verify_token: reader.read_bytes(MAX_CRYPTO_BYTES)?.to_vec(),
        })
    }
}

/// One profile property, such as a skin texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProperty {
    /// Property name.
    pub name: String,
    /// Property value.
    pub value: String,
    /// Mojang's signature over the value, when present.
    pub signature: Option<String>,
}

/// Login success (clientbound, login state, id `0x02`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    /// The player's UUID.
    pub uuid: [u8; 16],
    /// The player's name.
    pub username: String,
    /// Profile properties.
    pub properties: Vec<ProfileProperty>,
}

impl Packet for LoginSuccess {
    const ID: i32 = 0x02;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        out.extend_from_slice(&self.uuid);
        write_string(&self.username, MAX_NAME_CHARS, out)?;
        let count = self.properties.len();
        if count > MAX_PROPERTIES {
            return Err(CodecError::LengthTooLarge {
                len: count,
                max: MAX_PROPERTIES,
            });
        }
        write_varint(count as i32, out);
        for property in &self.properties {
            write_string(&property.name, MAX_PROPERTY_NAME_CHARS, out)?;
            write_string(&property.value, MAX_PROPERTY_VALUE_CHARS, out)?;
            write_bool(property.signature.is_some(), out);
            if let Some(signature) = &property.signature {
                write_string(signature, MAX_PROPERTY_SIGNATURE_CHARS, out)?;
            }
        }
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let uuid = reader.read_array()?;
        let username = reader.read_string(MAX_NAME_CHARS)?;
        let count = reader.read_len(MAX_PROPERTIES)?;
        let mut properties = Vec::with_capacity(count);
        for _ in 0..count {
            let name = reader.read_string(MAX_PROPERTY_NAME_CHARS)?;
            let value = reader.read_string(MAX_PROPERTY_VALUE_CHARS)?;
            let signature = if reader.read_bool()? {
                Some(reader.read_string(MAX_PROPERTY_SIGNATURE_CHARS)?)
            } else {
                None
            };
            properties.push(ProfileProperty {
                name,
                value,
                signature,
            });
        }
        Ok(Self {
            uuid,
            username,
            properties,
        })
    }
}

/// Set compression (clientbound, login state, id `0x03`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCompression {
    /// Packets at least this large are compressed. A negative value disables compression.
    pub threshold: i32,
}

impl Packet for SetCompression {
    const ID: i32 = 0x03;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_varint(self.threshold, out);
        Ok(())
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            threshold: reader.read_varint()?,
        })
    }
}

/// Login acknowledged (serverbound, login state, id `0x03`). Carries no data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginAcknowledged;

impl Packet for LoginAcknowledged {
    const ID: i32 = 0x03;

    fn encode_body(&self, _out: &mut Vec<u8>) -> Result<(), CodecError> {
        Ok(())
    }

    fn decode_body(_reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self)
    }
}

/// Login disconnect (clientbound, login state, id `0x00`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginDisconnect {
    /// The reason shown to the player, as a JSON text component.
    pub reason_json: String,
}

impl LoginDisconnect {
    /// Builds a disconnect whose reason is plain text, correctly escaped.
    #[must_use]
    pub fn with_text(text: &str) -> Self {
        let mut json = String::from(r#"{"text":""#);
        for ch in text.chars() {
            match ch {
                '"' => json.push_str("\\\""),
                '\\' => json.push_str("\\\\"),
                '\n' => json.push_str("\\n"),
                '\r' => json.push_str("\\r"),
                '\t' => json.push_str("\\t"),
                c if u32::from(c) < 0x20 => json.push_str(&format!("\\u{:04x}", u32::from(c))),
                c => json.push(c),
            }
        }
        json.push_str(r#""}"#);
        Self { reason_json: json }
    }
}

impl Packet for LoginDisconnect {
    const ID: i32 = 0x00;

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), CodecError> {
        write_string(&self.reason_json, MAX_DISCONNECT_REASON_CHARS, out)
    }

    fn decode_body(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            reason_json: reader.read_string(MAX_DISCONNECT_REASON_CHARS)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{PacketError, decode_packet, encode_packet};

    const UUID: [u8; 16] = [
        0x06, 0x9a, 0x79, 0xf4, 0x44, 0xe9, 0x47, 0x26, 0xa5, 0xbe, 0xfc, 0xa9, 0x0e, 0x38, 0xaa,
        0xf5,
    ];

    #[test]
    fn login_start_layout() {
        let packet = LoginStart {
            name: "Steve".to_owned(),
            uuid: UUID,
        };
        let mut expected = vec![0x00, 5];
        expected.extend_from_slice(b"Steve");
        expected.extend_from_slice(&UUID);
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(decode_packet::<LoginStart>(&bytes), Ok(packet));
    }

    #[test]
    fn login_start_rejects_long_names_and_short_uuids() {
        let long = LoginStart {
            name: "a".repeat(MAX_NAME_CHARS + 1),
            uuid: UUID,
        };
        assert!(encode_packet(&long).is_err());

        let mut bytes = vec![0x00, 5];
        bytes.extend_from_slice(b"Steve");
        bytes.extend_from_slice(&UUID[..15]);
        assert_eq!(
            decode_packet::<LoginStart>(&bytes),
            Err(PacketError::Codec(CodecError::UnexpectedEof))
        );
    }

    #[test]
    fn encryption_request_layout() {
        let packet = EncryptionRequest {
            server_id: String::new(),
            public_key: vec![0x30, 0x81, 0x9f],
            verify_token: vec![1, 2, 3, 4],
            should_authenticate: true,
        };
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(bytes, [0x01, 0, 3, 0x30, 0x81, 0x9f, 4, 1, 2, 3, 4, 1]);
        assert_eq!(decode_packet::<EncryptionRequest>(&bytes), Ok(packet));
    }

    #[test]
    fn encryption_response_round_trips_and_is_bounded() {
        let packet = EncryptionResponse {
            shared_secret: vec![9; 128],
            verify_token: vec![7; 128],
        };
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(decode_packet::<EncryptionResponse>(&bytes), Ok(packet));

        let oversized = EncryptionResponse {
            shared_secret: vec![0; MAX_CRYPTO_BYTES + 1],
            verify_token: vec![],
        };
        assert!(encode_packet(&oversized).is_err());

        // A declared 100,000-byte secret is refused before any data is read.
        let mut bytes = vec![0x01];
        write_varint(100_000, &mut bytes);
        assert!(matches!(
            decode_packet::<EncryptionResponse>(&bytes),
            Err(PacketError::Codec(CodecError::LengthTooLarge { .. }))
        ));
    }

    #[test]
    fn login_success_without_properties() {
        let packet = LoginSuccess {
            uuid: UUID,
            username: "Steve".to_owned(),
            properties: vec![],
        };
        let mut expected = vec![0x02];
        expected.extend_from_slice(&UUID);
        expected.extend_from_slice(&[5]);
        expected.extend_from_slice(b"Steve");
        expected.push(0);
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(decode_packet::<LoginSuccess>(&bytes), Ok(packet));
    }

    #[test]
    fn login_success_with_signed_and_unsigned_properties() {
        let packet = LoginSuccess {
            uuid: UUID,
            username: "Alex".to_owned(),
            properties: vec![
                ProfileProperty {
                    name: "textures".to_owned(),
                    value: "e30=".to_owned(),
                    signature: Some("sig".to_owned()),
                },
                ProfileProperty {
                    name: "other".to_owned(),
                    value: String::new(),
                    signature: None,
                },
            ],
        };
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(decode_packet::<LoginSuccess>(&bytes), Ok(packet));
    }

    #[test]
    fn login_success_property_count_is_bounded() {
        let property = ProfileProperty {
            name: "p".to_owned(),
            value: "v".to_owned(),
            signature: None,
        };
        let too_many = LoginSuccess {
            uuid: UUID,
            username: "Steve".to_owned(),
            properties: vec![property; MAX_PROPERTIES + 1],
        };
        assert_eq!(
            encode_packet(&too_many),
            Err(CodecError::LengthTooLarge {
                len: MAX_PROPERTIES + 1,
                max: MAX_PROPERTIES
            })
        );

        // A hostile count must not allocate or loop: it is rejected up front.
        let mut bytes = vec![0x02];
        bytes.extend_from_slice(&UUID);
        bytes.push(1);
        bytes.push(b'a');
        write_varint(i32::MAX, &mut bytes);
        assert!(matches!(
            decode_packet::<LoginSuccess>(&bytes),
            Err(PacketError::Codec(CodecError::LengthTooLarge { .. }))
        ));
    }

    #[test]
    fn invalid_signature_flag_is_rejected() {
        let mut bytes = vec![0x02];
        bytes.extend_from_slice(&UUID);
        bytes.extend_from_slice(&[1, b'a', 1, 1, b'n', 1, b'v', 2]);
        assert_eq!(
            decode_packet::<LoginSuccess>(&bytes),
            Err(PacketError::Codec(CodecError::InvalidValue("boolean")))
        );
    }

    #[test]
    fn set_compression_layout() {
        let bytes = encode_packet(&SetCompression { threshold: 256 }).unwrap();
        assert_eq!(bytes, [0x03, 0x80, 0x02]);
        assert_eq!(
            decode_packet::<SetCompression>(&bytes),
            Ok(SetCompression { threshold: 256 })
        );
        // A negative threshold (compression disabled) survives the round trip.
        let off = encode_packet(&SetCompression { threshold: -1 }).unwrap();
        assert_eq!(
            decode_packet::<SetCompression>(&off),
            Ok(SetCompression { threshold: -1 })
        );
    }

    #[test]
    fn login_acknowledged_is_just_the_id() {
        assert_eq!(encode_packet(&LoginAcknowledged).unwrap(), [0x03]);
        assert_eq!(
            decode_packet::<LoginAcknowledged>(&[0x03]),
            Ok(LoginAcknowledged)
        );
    }

    #[test]
    fn disconnect_text_is_escaped() {
        let packet = LoginDisconnect::with_text("Use \"26.3\"\nplease");
        assert_eq!(packet.reason_json, r#"{"text":"Use \"26.3\"\nplease"}"#);
        let bytes = encode_packet(&packet).unwrap();
        assert_eq!(decode_packet::<LoginDisconnect>(&bytes), Ok(packet));
    }

    #[test]
    fn packets_with_the_same_id_are_told_apart_by_type() {
        // LoginStart and LoginDisconnect share id 0x00 in opposite directions; each
        // decoder only accepts its own layout, and wrong ids are rejected.
        let start = encode_packet(&SetCompression { threshold: 1 }).unwrap();
        assert_eq!(
            decode_packet::<LoginStart>(&start),
            Err(PacketError::UnexpectedId {
                expected: 0,
                actual: 3
            })
        );
    }
}
