//! Bit-packed reading and writing for replication packets: values of any
//! width from 0 to 32 bits back to back, plus variable-length integers.
//! The writer reuses its buffer, so a steady stream of packets doesn't
//! allocate.

/// Writes values bit by bit, least significant bits first.
#[derive(Default, Debug)]
pub(crate) struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    used: u32,
}

impl BitWriter {
    /// Empties the writer, keeping its buffer.
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.acc = 0;
        self.used = 0;
    }

    /// Writes the low `bits` bits of `value` (0 to 32 bits).
    pub(crate) fn write(&mut self, value: u32, bits: u32) {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return;
        }
        let masked = u64::from(value) & mask64(bits);
        self.acc |= masked << self.used;
        self.used += bits;
        while self.used >= 8 {
            self.bytes.push(self.acc as u8);
            self.acc >>= 8;
            self.used -= 8;
        }
    }

    pub(crate) fn write_bool(&mut self, value: bool) {
        self.write(u32::from(value), 1);
    }

    /// A variable-length integer: 7 bits at a time, each group followed by
    /// a bit saying whether another one follows. Small numbers (ids, counts)
    /// take 8 bits.
    pub(crate) fn write_varint(&mut self, mut value: u64) {
        loop {
            let group = (value & 0x7F) as u32;
            value >>= 7;
            self.write(group, 7);
            self.write_bool(value != 0);
            if value == 0 {
                break;
            }
        }
    }

    /// Bits written so far.
    pub(crate) fn bit_len(&self) -> usize {
        self.bytes.len() * 8 + self.used as usize
    }

    /// Bytes the packet takes once finished.
    pub(crate) fn byte_len(&self) -> usize {
        self.bit_len().div_ceil(8)
    }

    /// Cuts the stream back to `bits` bits (an entity that didn't fit).
    pub(crate) fn truncate(&mut self, bits: usize) {
        if bits >= self.bit_len() {
            return;
        }
        let whole = bits / 8;
        let rest = (bits % 8) as u32;
        let last = if whole < self.bytes.len() {
            self.bytes[whole]
        } else {
            self.acc as u8
        };
        self.bytes.truncate(whole);
        self.acc = u64::from(last) & mask64(rest);
        self.used = rest;
    }

    /// The packet: every byte written, the last one padded with zeros.
    pub(crate) fn finish(&mut self) -> &[u8] {
        if self.used > 0 {
            self.bytes.push(self.acc as u8);
            self.acc = 0;
            self.used = 0;
        }
        &self.bytes
    }
}

/// Reads what a [`BitWriter`] wrote. Every read past the end returns `None`.
pub(crate) struct BitReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Reads `bits` bits (0 to 32).
    pub(crate) fn read(&mut self, bits: u32) -> Option<u32> {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return Some(0);
        }
        if self.pos + bits as usize > self.bytes.len() * 8 {
            return None;
        }
        let mut value: u64 = 0;
        let mut got = 0;
        while got < bits {
            let byte = self.bytes[self.pos / 8];
            let offset = (self.pos % 8) as u32;
            let take = (8 - offset).min(bits - got);
            let chunk = (u64::from(byte) >> offset) & mask64(take);
            value |= chunk << got;
            got += take;
            self.pos += take as usize;
        }
        Some(value as u32)
    }

    pub(crate) fn read_bool(&mut self) -> Option<bool> {
        self.read(1).map(|b| b != 0)
    }

    /// Reads a [`BitWriter::write_varint`] number; `None` past the end or
    /// for one longer than 64 bits.
    pub(crate) fn read_varint(&mut self) -> Option<u64> {
        let mut value: u64 = 0;
        for shift in (0..64).step_by(7) {
            let group = u64::from(self.read(7)?);
            if shift == 63 && group > 1 {
                return None;
            }
            value |= group << shift;
            if !self.read_bool()? {
                return Some(value);
            }
        }
        None
    }

    /// Bits left to read (the padding of the last byte included).
    #[cfg(test)]
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() * 8 - self.pos
    }
}

/// The low `bits` bits set (0 to 64).
pub(crate) const fn mask64(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// The low `bits` bits set, as a word (0 to 32).
pub(crate) const fn mask32(bits: u32) -> u32 {
    if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 }
}

/// How many bits hold every number up to `max`.
pub(crate) const fn bits_for(max: u32) -> u32 {
    32 - max.leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_of_every_width_round_trip_back_to_back() {
        let mut w = BitWriter::default();
        let values: Vec<(u32, u32)> = (0..=32u32)
            .map(|bits| (0xDEAD_BEEFu32.rotate_left(bits) & mask32(bits), bits))
            .collect();
        for &(v, bits) in &values {
            w.write(v, bits);
        }
        w.write_varint(0);
        w.write_varint(300);
        w.write_varint(u64::MAX);
        let bytes = w.finish().to_vec();
        let mut r = BitReader::new(&bytes);
        for &(v, bits) in &values {
            assert_eq!(r.read(bits), Some(v), "{bits} bits");
        }
        assert_eq!(r.read_varint(), Some(0));
        assert_eq!(r.read_varint(), Some(300));
        assert_eq!(r.read_varint(), Some(u64::MAX));
        assert!(r.remaining() < 8);
    }

    #[test]
    fn reading_past_the_end_fails_instead_of_inventing_bits() {
        let mut w = BitWriter::default();
        w.write(5, 3);
        let bytes = w.finish().to_vec();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read(8), Some(5));
        assert_eq!(r.read(1), None);
        assert_eq!(BitReader::new(&[0xFF; 10]).read_varint(), None, "too long");
    }

    #[test]
    fn truncate_drops_what_was_written_after_a_mark() {
        let mut w = BitWriter::default();
        w.write(0b101, 3);
        let mark = w.bit_len();
        w.write(0xFFFF_FFFF, 32);
        w.write(1, 1);
        w.truncate(mark);
        w.write(0b11, 2);
        let bytes = w.finish().to_vec();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read(3), Some(0b101));
        assert_eq!(r.read(2), Some(0b11));
        assert_eq!(bytes.len(), 1);
    }

    #[test]
    fn bits_for_counts_the_width_of_the_largest_value() {
        assert_eq!(bits_for(0), 0);
        assert_eq!(bits_for(1), 1);
        assert_eq!(bits_for(255), 8);
        assert_eq!(bits_for(256), 9);
        assert_eq!(bits_for(u32::MAX), 32);
    }
}
