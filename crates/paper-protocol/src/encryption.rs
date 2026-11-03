//! AES-128/CFB8 stream encryption used after the login encryption handshake.
//!
//! The shared secret is used as both the key and the IV. Each direction keeps its
//! own cipher state and must process bytes strictly in order.

use std::error::Error;
use std::fmt;

use aes::Aes128;
use cfb8::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};

/// Required length of the shared secret, in bytes.
pub const SECRET_LEN: usize = 16;

/// The shared secret had the wrong length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretLengthError;

impl fmt::Display for SecretLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("shared secret must be 16 bytes")
    }
}

impl Error for SecretLengthError {}

/// Encrypts the outgoing byte stream.
pub struct Encryptor {
    cipher: cfb8::Encryptor<Aes128>,
}

impl Encryptor {
    /// Creates an encryptor from the shared secret.
    pub fn new(secret: &[u8]) -> Result<Self, SecretLengthError> {
        match cfb8::Encryptor::<Aes128>::new_from_slices(secret, secret) {
            Ok(cipher) => Ok(Self { cipher }),
            Err(_) => Err(SecretLengthError),
        }
    }

    /// Encrypts `data` in place, continuing the stream from earlier calls.
    pub fn encrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let slot = std::slice::from_mut(byte);
            let block = cfb8::cipher::generic_array::GenericArray::from_mut_slice(slot);
            self.cipher.encrypt_block_mut(block);
        }
    }
}

/// Decrypts the incoming byte stream.
pub struct Decryptor {
    cipher: cfb8::Decryptor<Aes128>,
}

impl Decryptor {
    /// Creates a decryptor from the shared secret.
    pub fn new(secret: &[u8]) -> Result<Self, SecretLengthError> {
        match cfb8::Decryptor::<Aes128>::new_from_slices(secret, secret) {
            Ok(cipher) => Ok(Self { cipher }),
            Err(_) => Err(SecretLengthError),
        }
    }

    /// Decrypts `data` in place, continuing the stream from earlier calls.
    pub fn decrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let slot = std::slice::from_mut(byte);
            let block = cfb8::cipher::generic_array::GenericArray::from_mut_slice(slot);
            self.cipher.decrypt_block_mut(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; SECRET_LEN] = *b"0123456789abcdef";

    #[test]
    fn round_trips_across_different_chunking() {
        let plain: Vec<u8> = (0..=255).collect();
        let mut data = plain.clone();
        Encryptor::new(&SECRET).unwrap().encrypt(&mut data);
        assert_ne!(data, plain);

        let mut decryptor = Decryptor::new(&SECRET).unwrap();
        let (first, second) = data.split_at_mut(100);
        decryptor.decrypt(first);
        decryptor.decrypt(second);
        assert_eq!(data, plain);
    }

    #[test]
    fn stream_state_carries_across_calls() {
        let plain: Vec<u8> = (0..64).collect();
        let mut whole = plain.clone();
        Encryptor::new(&SECRET).unwrap().encrypt(&mut whole);

        let mut split = plain.clone();
        let mut encryptor = Encryptor::new(&SECRET).unwrap();
        let (head, tail) = split.split_at_mut(7);
        encryptor.encrypt(head);
        encryptor.encrypt(tail);
        assert_eq!(whole, split);
    }

    #[test]
    fn rejects_wrong_secret_length() {
        assert!(Encryptor::new(&[0; 15]).is_err());
        assert!(Decryptor::new(&[0; 17]).is_err());
    }
}
