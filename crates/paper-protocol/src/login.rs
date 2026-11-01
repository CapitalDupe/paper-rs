//! Login-state packets.
//!
//! Layouts follow the 1.20.2-and-later form (Login Start carries a UUID, Login
//! Success carries a property list). Older versions differ; the final target
//! version is decided in `docs/adr/0003-target-minecraft-version.md`.

use crate::codec::{self, Reader};
use crate::error::{DecodeError, EncodeError};
use crate::varint;

/// Maximum characters in a player name.
pub const MAX_NAME_CHARS: usize = 16;
/// Maximum characters in the encryption request server id.
pub const MAX_SERVER_ID_CHARS: usize = 20;
/// Maximum size of the public key in an encryption request.
pub const MAX_PUBLIC_KEY_BYTES: usize = 512;
/// Maximum size of an encrypted secret or verify token.
pub const MAX_TOKEN_BYTES: usize = 256;
/// Maximum number of profile properties in Login Success.
pub const MAX_PROPERTIES: usize = 64;
/// Maximum characters in a property name.
pub const MAX_PROPERTY_NAME_CHARS: usize = 64;
/// Maximum characters in a property value.
pub const MAX_PROPERTY_VALUE_CHARS: usize = 32_767;
/// Maximum characters in a property signature.
pub const MAX_SIGNATURE_CHARS: usize = 1_024;
/// Maximum characters in a disconnect reason JSON.
pub const MAX_REASON_CHARS: usize = 262_144;

fn read_uuid(reader: &mut Reader<'_>) -> Result<[u8; 16], DecodeError> {
    let bytes = reader.read_bytes(16)?;
    bytes.try_into().map_err(|_| DecodeError::UnexpectedEof)
}

/// Serverbound Login Start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    /// Player name.
    pub name: String,
    /// Player UUID as 16 big-endian bytes.
    pub uuid: [u8; 16],
}

impl LoginStart {
    /// Packet id.
    pub const ID: i32 = 0x00;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let name = reader.read_string(MAX_NAME_CHARS)?;
        let uuid = read_uuid(reader)?;
        Ok(Self { name, uuid })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        codec::write_string(out, &self.name, MAX_NAME_CHARS)?;
        out.extend_from_slice(&self.uuid);
        Ok(())
    }
}

/// Clientbound Login Disconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginDisconnect {
    /// Reason as JSON text.
    pub reason_json: String,
}

impl LoginDisconnect {
    /// Packet id.
    pub const ID: i32 = 0x00;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let reason_json = reader.read_string(MAX_REASON_CHARS)?;
        Ok(Self { reason_json })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        codec::write_string(out, &self.reason_json, MAX_REASON_CHARS)
    }
}

/// Clientbound Encryption Request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionRequest {
    /// Server id string (empty in modern versions).
    pub server_id: String,
    /// DER-encoded RSA public key.
    pub public_key: Vec<u8>,
    /// Random token the client must echo back, encrypted.
    pub verify_token: Vec<u8>,
}

impl EncryptionRequest {
    /// Packet id.
    pub const ID: i32 = 0x01;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let server_id = reader.read_string(MAX_SERVER_ID_CHARS)?;
        let public_key = reader.read_byte_array(MAX_PUBLIC_KEY_BYTES)?;
        let verify_token = reader.read_byte_array(MAX_TOKEN_BYTES)?;
        Ok(Self {
            server_id,
            public_key,
            verify_token,
        })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        codec::write_string(out, &self.server_id, MAX_SERVER_ID_CHARS)?;
        codec::write_byte_array(out, &self.public_key, MAX_PUBLIC_KEY_BYTES)?;
        codec::write_byte_array(out, &self.verify_token, MAX_TOKEN_BYTES)?;
        Ok(())
    }
}

/// Serverbound Encryption Response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionResponse {
    /// Shared secret, RSA-encrypted with the server's public key.
    pub shared_secret: Vec<u8>,
    /// Verify token, RSA-encrypted with the server's public key.
    pub verify_token: Vec<u8>,
}

impl EncryptionResponse {
    /// Packet id.
    pub const ID: i32 = 0x01;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let shared_secret = reader.read_byte_array(MAX_TOKEN_BYTES)?;
        let verify_token = reader.read_byte_array(MAX_TOKEN_BYTES)?;
        Ok(Self {
            shared_secret,
            verify_token,
        })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        varint::write_varint(Self::ID, out);
        codec::write_byte_array(out, &self.shared_secret, MAX_TOKEN_BYTES)?;
        codec::write_byte_array(out, &self.verify_token, MAX_TOKEN_BYTES)?;
        Ok(())
    }
}

/// A profile property (for example a skin texture) in Login Success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Property name.
    pub name: String,
    /// Property value.
    pub value: String,
    /// Optional signature.
    pub signature: Option<String>,
}

impl Property {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let name = reader.read_string(MAX_PROPERTY_NAME_CHARS)?;
        let value = reader.read_string(MAX_PROPERTY_VALUE_CHARS)?;
        let signature = if reader.read_bool()? {
            Some(reader.read_string(MAX_SIGNATURE_CHARS)?)
        } else {
            None
        };
        Ok(Self {
            name,
            value,
            signature,
        })
    }

    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        codec::write_string(out, &self.name, MAX_PROPERTY_NAME_CHARS)?;
        codec::write_string(out, &self.value, MAX_PROPERTY_VALUE_CHARS)?;
        match &self.signature {
            Some(signature) => {
                out.push(1);
                codec::write_string(out, signature, MAX_SIGNATURE_CHARS)?;
            }
            None => out.push(0),
        }
        Ok(())
    }
}

/// Clientbound Login Success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    /// Player UUID as 16 big-endian bytes.
    pub uuid: [u8; 16],
    /// Player name.
    pub username: String,
    /// Profile properties.
    pub properties: Vec<Property>,
}

impl LoginSuccess {
    /// Packet id.
    pub const ID: i32 = 0x02;

    /// Decodes the body (everything after the packet id).
    pub fn decode_body(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let uuid = read_uuid(reader)?;
        let username = reader.read_string(MAX_NAME_CHARS)?;
        let count = reader.read_length()?;
        if count > MAX_PROPERTIES {
            return Err(DecodeError::TooManyItems(MAX_PROPERTIES));
        }
        let mut properties = Vec::with_capacity(count);
        for _ in 0..count {
            properties.push(Property::decode(reader)?);
        }
        Ok(Self {
            uuid,
            username,
            properties,
        })
    }

    /// Encodes the packet id and body.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        if self.properties.len() > MAX_PROPERTIES {
            return Err(EncodeError::TooManyItems(MAX_PROPERTIES));
        }
        varint::write_varint(Self::ID, out);
        out.extend_from_slice(&self.uuid);
        codec::write_string(out, &self.username, MAX_NAME_CHARS)?;
        codec::write_length(out, self.properties.len())?;
        for property in &self.properties {
            property.encode(out)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(bytes: &[u8]) -> Reader<'_> {
        let mut reader = Reader::new(bytes);
        assert!(reader.read_varint().is_ok());
        reader
    }

    #[test]
    fn login_start_round_trips() {
        let packet = LoginStart {
            name: "Steve".to_owned(),
            uuid: [9; 16],
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = body(&bytes);
        assert_eq!(LoginStart::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn login_start_rejects_long_name() {
        let mut bytes = Vec::new();
        varint::write_varint(17, &mut bytes);
        bytes.extend_from_slice(&[b'a'; 17]);
        bytes.extend_from_slice(&[0; 16]);
        let result = LoginStart::decode_body(&mut Reader::new(&bytes));
        assert_eq!(result, Err(DecodeError::StringTooLong(MAX_NAME_CHARS)));
    }

    #[test]
    fn login_disconnect_round_trips() {
        let packet = LoginDisconnect {
            reason_json: "{\"text\":\"bye\"}".to_owned(),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = body(&bytes);
        assert_eq!(LoginDisconnect::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn encryption_request_round_trips() {
        let packet = EncryptionRequest {
            server_id: String::new(),
            public_key: vec![1, 2, 3, 4],
            verify_token: vec![5, 6, 7, 8],
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = body(&bytes);
        assert_eq!(EncryptionRequest::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn encryption_response_round_trips_and_bounds_tokens() {
        let packet = EncryptionResponse {
            shared_secret: vec![1; 128],
            verify_token: vec![2; 128],
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = body(&bytes);
        assert_eq!(EncryptionResponse::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));

        let mut oversized = Vec::new();
        codec::write_byte_array(&mut oversized, &[1; 4], 8).unwrap();
        varint::write_varint(257, &mut oversized);
        let result = EncryptionResponse::decode_body(&mut Reader::new(&oversized));
        assert_eq!(result, Err(DecodeError::ByteArrayTooLong(MAX_TOKEN_BYTES)));
    }

    #[test]
    fn login_success_round_trips() {
        let packet = LoginSuccess {
            uuid: [7; 16],
            username: "Steve".to_owned(),
            properties: vec![
                Property {
                    name: "textures".to_owned(),
                    value: "abc".to_owned(),
                    signature: Some("sig".to_owned()),
                },
                Property {
                    name: "extra".to_owned(),
                    value: "x".to_owned(),
                    signature: None,
                },
            ],
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let mut reader = body(&bytes);
        assert_eq!(LoginSuccess::decode_body(&mut reader), Ok(packet));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn login_success_rejects_too_many_properties() {
        let mut bytes = vec![0; 16];
        bytes.extend_from_slice(&[1, b'a']);
        varint::write_varint(65, &mut bytes);
        let result = LoginSuccess::decode_body(&mut Reader::new(&bytes));
        assert_eq!(result, Err(DecodeError::TooManyItems(MAX_PROPERTIES)));
    }

    #[test]
    fn login_success_rejects_bad_signature_flag() {
        let mut bytes = vec![0; 16];
        bytes.extend_from_slice(&[1, b'a', 1, 1, b'n', 1, b'v', 2]);
        let result = LoginSuccess::decode_body(&mut Reader::new(&bytes));
        assert_eq!(result, Err(DecodeError::InvalidValue("bool", 2)));
    }
}
