//! Fixed-width packed integer arrays, as used for chunk section data.
//!
//! Entries never span two 64-bit words: each word holds `64 / bits` entries and
//! any leftover high bits are unused. This matches the layout used on the wire by
//! modern protocol versions.

use std::error::Error;
use std::fmt;

/// Errors produced by [`PackedArray`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedError {
    /// The index is past the end of the array.
    IndexOutOfRange,
    /// The value does not fit in the entry width.
    ValueTooLarge,
    /// The width or word count is inconsistent with the array length.
    BadLength,
}

impl fmt::Display for PackedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexOutOfRange => f.write_str("index out of range"),
            Self::ValueTooLarge => f.write_str("value does not fit in the entry width"),
            Self::BadLength => f.write_str("inconsistent packed array length"),
        }
    }
}

impl Error for PackedError {}

/// A fixed-length array of `bits`-wide unsigned integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedArray {
    bits: u8,
    len: usize,
    data: Vec<u64>,
}

impl PackedArray {
    /// Widest supported entry, in bits.
    pub const MAX_BITS: u8 = 32;

    fn words_needed(bits: u8, len: usize) -> usize {
        len.div_ceil(64 / usize::from(bits))
    }

    /// Creates a zeroed array. `bits` is clamped to `1..=32`.
    pub fn new(bits: u8, len: usize) -> Self {
        let bits = bits.clamp(1, Self::MAX_BITS);
        Self {
            bits,
            len,
            data: vec![0; Self::words_needed(bits, len)],
        }
    }

    /// Wraps existing words, checking that the count matches `bits` and `len`.
    pub fn from_raw(bits: u8, len: usize, data: Vec<u64>) -> Result<Self, PackedError> {
        if !(1..=Self::MAX_BITS).contains(&bits) {
            return Err(PackedError::BadLength);
        }
        if data.len() != Self::words_needed(bits, len) {
            return Err(PackedError::BadLength);
        }
        Ok(Self { bits, len, data })
    }

    /// Entry width in bits.
    pub fn bits(&self) -> u8 {
        self.bits
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the array has no entries.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The underlying words.
    pub fn as_longs(&self) -> &[u64] {
        &self.data
    }

    fn mask(&self) -> u64 {
        (1_u64 << self.bits) - 1
    }

    fn locate(&self, index: usize) -> Option<(usize, u32)> {
        if index >= self.len {
            return None;
        }
        let per_word = 64 / usize::from(self.bits);
        let shift = (index % per_word) * usize::from(self.bits);
        Some((index / per_word, shift as u32))
    }

    /// Reads entry `index`.
    pub fn get(&self, index: usize) -> Option<u32> {
        let (word, shift) = self.locate(index)?;
        Some(((self.data[word] >> shift) & self.mask()) as u32)
    }

    /// Writes entry `index` and returns the previous value.
    pub fn set(&mut self, index: usize, value: u32) -> Result<u32, PackedError> {
        let Some((word, shift)) = self.locate(index) else {
            return Err(PackedError::IndexOutOfRange);
        };
        let mask = self.mask();
        if u64::from(value) > mask {
            return Err(PackedError::ValueTooLarge);
        }
        let old = (self.data[word] >> shift) & mask;
        self.data[word] = (self.data[word] & !(mask << shift)) | (u64::from(value) << shift);
        Ok(old as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(index: usize, bits: u8) -> u32 {
        let mask = (1_u64 << bits) - 1;
        ((index as u64).wrapping_mul(2_654_435_761) & mask) as u32
    }

    #[test]
    fn round_trips_at_many_widths() {
        for bits in [1, 4, 5, 7, 8, 12, 16, 32] {
            let mut array = PackedArray::new(bits, 4096);
            for index in 0..4096 {
                array.set(index, pattern(index, bits)).unwrap();
            }
            for index in 0..4096 {
                assert_eq!(array.get(index), Some(pattern(index, bits)), "bits {bits}");
            }
        }
    }

    #[test]
    fn writes_do_not_disturb_neighbours() {
        let mut array = PackedArray::new(5, 100);
        for index in 0..100 {
            array.set(index, 31).unwrap();
        }
        assert_eq!(array.set(40, 0), Ok(31));
        for index in 0..100 {
            let expected = if index == 40 { 0 } else { 31 };
            assert_eq!(array.get(index), Some(expected));
        }
    }

    #[test]
    fn rejects_bad_access() {
        let mut array = PackedArray::new(4, 16);
        assert_eq!(array.get(16), None);
        assert_eq!(array.set(16, 1), Err(PackedError::IndexOutOfRange));
        assert_eq!(array.set(0, 16), Err(PackedError::ValueTooLarge));
    }

    #[test]
    fn from_raw_checks_the_word_count() {
        assert!(PackedArray::from_raw(4, 4096, vec![0; 256]).is_ok());
        assert_eq!(
            PackedArray::from_raw(4, 4096, vec![0; 255]),
            Err(PackedError::BadLength)
        );
        assert_eq!(
            PackedArray::from_raw(0, 16, vec![0]),
            Err(PackedError::BadLength)
        );
    }

    #[test]
    fn entries_do_not_span_words() {
        let array = PackedArray::new(7, 100);
        assert_eq!(array.as_longs().len(), 100_usize.div_ceil(9));
    }
}
