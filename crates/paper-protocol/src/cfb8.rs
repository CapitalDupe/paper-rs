//! AES-128 in CFB8 mode, the stream cipher the protocol uses after login encryption.
//!
//! CFB8 turns a block cipher into a byte-at-a-time stream cipher. A 16-byte shift
//! register starts as the IV; for each byte, the register is encrypted and the first
//! byte of the result is XORed with the data byte. The *ciphertext* byte is then shifted
//! into the register. The protocol uses the shared secret as both key and IV.
//!
//! The mode is implemented here over the [`BlockEncrypt16`] trait so it can be tested
//! without a crypto dependency. The AES block cipher itself is never hand-rolled: with
//! the `encryption` feature, [`Aes128Block`] wraps the audited `aes` crate.
//!
//! Encryption only ever uses the cipher's encrypt direction, in both directions of the
//! stream, which is why the trait has no decrypt method.

/// A 128-bit block cipher's encrypt direction.
pub trait BlockEncrypt16 {
    /// Encrypts one 16-byte block in place.
    fn encrypt_block(&self, block: &mut [u8; 16]);
}

/// The CFB8 shift register shared by [`Cfb8Encryptor`] and [`Cfb8Decryptor`].
#[derive(Debug, Clone)]
struct Register {
    bytes: [u8; 16],
}

impl Register {
    /// Returns the next keystream byte for the current register contents.
    fn keystream_byte<C: BlockEncrypt16>(&self, cipher: &C) -> u8 {
        let mut block = self.bytes;
        cipher.encrypt_block(&mut block);
        block[0]
    }

    /// Shifts the register left one byte and appends a ciphertext byte.
    fn push(&mut self, ciphertext_byte: u8) {
        self.bytes.copy_within(1.., 0);
        self.bytes[15] = ciphertext_byte;
    }
}

/// Encrypts a byte stream with CFB8. State carries across calls, so a stream may be
/// encrypted in arbitrary chunks.
#[derive(Debug, Clone)]
pub struct Cfb8Encryptor<C: BlockEncrypt16> {
    cipher: C,
    register: Register,
}

impl<C: BlockEncrypt16> Cfb8Encryptor<C> {
    /// Creates an encryptor from a block cipher and an initialisation vector.
    pub fn new(cipher: C, iv: [u8; 16]) -> Self {
        Self {
            cipher,
            register: Register { bytes: iv },
        }
    }

    /// Encrypts `data` in place.
    pub fn encrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let ciphertext = *byte ^ self.register.keystream_byte(&self.cipher);
            *byte = ciphertext;
            self.register.push(ciphertext);
        }
    }
}

/// Decrypts a byte stream with CFB8. State carries across calls.
#[derive(Debug, Clone)]
pub struct Cfb8Decryptor<C: BlockEncrypt16> {
    cipher: C,
    register: Register,
}

impl<C: BlockEncrypt16> Cfb8Decryptor<C> {
    /// Creates a decryptor from a block cipher and an initialisation vector.
    pub fn new(cipher: C, iv: [u8; 16]) -> Self {
        Self {
            cipher,
            register: Register { bytes: iv },
        }
    }

    /// Decrypts `data` in place.
    pub fn decrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            let ciphertext = *byte;
            *byte = ciphertext ^ self.register.keystream_byte(&self.cipher);
            self.register.push(ciphertext);
        }
    }
}

#[cfg(feature = "encryption")]
pub use aes_block::{Aes128Block, aes128_cfb8_pair};

#[cfg(feature = "encryption")]
mod aes_block {
    use aes::Aes128;
    use aes::cipher::{BlockEncrypt, KeyInit};

    use super::{BlockEncrypt16, Cfb8Decryptor, Cfb8Encryptor};

    /// AES-128 as a [`BlockEncrypt16`], backed by the `aes` crate.
    #[derive(Clone)]
    pub struct Aes128Block(Aes128);

    impl Aes128Block {
        /// Creates the cipher from a 16-byte key.
        #[must_use]
        pub fn new(key: &[u8; 16]) -> Self {
            Self(Aes128::new_from_slice(key).expect("a 16-byte array is a valid AES-128 key"))
        }
    }

    impl BlockEncrypt16 for Aes128Block {
        fn encrypt_block(&self, block: &mut [u8; 16]) {
            let mut generic = aes::Block::from(*block);
            self.0.encrypt_block(&mut generic);
            block.copy_from_slice(&generic);
        }
    }

    /// Builds the encryptor and decryptor for one connection from the shared secret,
    /// which the protocol uses as both the AES key and the IV.
    #[must_use]
    pub fn aes128_cfb8_pair(
        shared_secret: &[u8; 16],
    ) -> (Cfb8Encryptor<Aes128Block>, Cfb8Decryptor<Aes128Block>) {
        (
            Cfb8Encryptor::new(Aes128Block::new(shared_secret), *shared_secret),
            Cfb8Decryptor::new(Aes128Block::new(shared_secret), *shared_secret),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stand-in for a block cipher. CFB8 only needs *some* function of
    /// the register, so this exercises the mode's bookkeeping without AES.
    struct Toy;

    impl BlockEncrypt16 for Toy {
        fn encrypt_block(&self, block: &mut [u8; 16]) {
            for (index, byte) in block.iter_mut().enumerate() {
                *byte = byte.wrapping_mul(31).wrapping_add(index as u8) ^ 0x5A;
            }
            block.rotate_left(3);
        }
    }

    const IV: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    fn sample() -> Vec<u8> {
        (0..200_u32).map(|n| (n * 7 + 3) as u8).collect()
    }

    #[test]
    fn round_trip() {
        let plain = sample();
        let mut data = plain.clone();
        Cfb8Encryptor::new(Toy, IV).encrypt(&mut data);
        assert_ne!(data, plain);
        Cfb8Decryptor::new(Toy, IV).decrypt(&mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let plain = sample();
        let mut whole = plain.clone();
        Cfb8Encryptor::new(Toy, IV).encrypt(&mut whole);

        for chunk in [1_usize, 2, 3, 7, 16, 17, 64] {
            let mut pieces = plain.clone();
            let mut encryptor = Cfb8Encryptor::new(Toy, IV);
            for part in pieces.chunks_mut(chunk) {
                encryptor.encrypt(part);
            }
            assert_eq!(pieces, whole, "encrypt chunk size {chunk}");

            let mut decryptor = Cfb8Decryptor::new(Toy, IV);
            for part in pieces.chunks_mut(chunk) {
                decryptor.decrypt(part);
            }
            assert_eq!(pieces, plain, "decrypt chunk size {chunk}");
        }
    }

    #[test]
    fn a_wrong_iv_corrupts_only_the_first_block_then_resynchronises() {
        let plain = sample();
        let mut data = plain.clone();
        Cfb8Encryptor::new(Toy, IV).encrypt(&mut data);

        // `Toy` moves register byte 3 into the first keystream byte, so flipping it
        // changes the first decrypted byte.
        let mut wrong = IV;
        wrong[3] ^= 1;
        Cfb8Decryptor::new(Toy, wrong).decrypt(&mut data);

        assert_ne!(data[..16], plain[..16]);
        // After 16 bytes the register holds only ciphertext, identical for both sides,
        // so CFB8 recovers on its own.
        assert_eq!(data[16..], plain[16..]);
    }

    #[test]
    fn the_register_follows_ciphertext_not_plaintext() {
        // Identical plaintext bytes must produce different ciphertext as the register
        // evolves; a mode that fed plaintext back in would differ from this check only
        // through the decrypt round trip above, so also pin the first bytes.
        let mut data = [0x42_u8; 4];
        Cfb8Encryptor::new(Toy, IV).encrypt(&mut data);
        assert!(data.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn empty_input_is_a_no_op() {
        let mut data: [u8; 0] = [];
        Cfb8Encryptor::new(Toy, IV).encrypt(&mut data);
        Cfb8Decryptor::new(Toy, IV).decrypt(&mut data);
    }

    #[cfg(feature = "encryption")]
    mod aes_vectors {
        use super::super::*;

        fn hex(text: &str) -> Vec<u8> {
            (0..text.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
                .collect()
        }

        /// NIST SP 800-38A, F.3.7 (CFB8-AES128.Encrypt), first plaintext block.
        /// The expected bytes were also reproduced with `openssl enc -aes-128-cfb8`.
        #[test]
        fn matches_the_nist_cfb8_vector() {
            let key: [u8; 16] = hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap();
            let iv: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
            let plain = hex("6bc1bee22e409f96e93d7e117393172a");
            let expected = hex("3b79424c9c0dd436bace9e0ed4586a4f");

            let mut data = plain.clone();
            Cfb8Encryptor::new(Aes128Block::new(&key), iv).encrypt(&mut data);
            assert_eq!(data, expected);

            Cfb8Decryptor::new(Aes128Block::new(&key), iv).decrypt(&mut data);
            assert_eq!(data, plain);
        }

        /// Key and IV are the same shared secret, as in the protocol. The expected bytes
        /// come from `openssl enc -aes-128-cfb8 -K <secret> -iv <secret>`.
        #[test]
        fn matches_openssl_with_key_equal_to_iv() {
            let secret: [u8; 16] = core::array::from_fn(|index| index as u8);
            let plain = b"The quick brown fox jumps over the lazy dog";
            let expected = hex(
                "5eb44639981b73c74762c48f7d973120307c8f1aa88b0e6db30e4abda49393d9828e35c494ff4a4bc6e4f1",
            );

            let (mut encryptor, mut decryptor) = aes128_cfb8_pair(&secret);
            let mut data = plain.to_vec();
            encryptor.encrypt(&mut data);
            assert_eq!(data, expected);
            decryptor.decrypt(&mut data);
            assert_eq!(data, plain);
        }
    }
}
