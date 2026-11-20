//! The replication layer's packets.
//!
//! On the internal reliable lane, byte messages: `HELLO` (server to client:
//! schema hash, the client's peer id, tick rate), `READY` (client to
//! server: its schema hash), `REJECT` (server to client: why), and `EVENT`
//! (either way: event type, server tick, payload).
//!
//! On the internal unreliable lane, bit-packed packets:
//!
//! - `STATE`, server to client: kind, packet sequence (16 bits), server tick,
//!   the last input applied for this client, the entities to destroy, then
//!   one record per entity. A record is `CREATE` (spawn, every field),
//!   `FULL` (every field) or `DELTA` (only what changed since a baseline
//!   the client acknowledged, which the record names by its distance in
//!   packets). A changed field is sent as a short difference when it moved
//!   a little, in full otherwise.
//! - `CLIENT`, client to server: the newest state packet received and a
//!   64-bit mask of the ones before it, then the inputs the server hasn't
//!   applied yet.

use super::bits::{BitReader, BitWriter, mask32};
use super::kinds::Kind;
use crate::net::PeerId;

/// Reliable messages.
pub(crate) const HELLO: u8 = 1;
pub(crate) const READY: u8 = 2;
pub(crate) const REJECT: u8 = 3;
pub(crate) const EVENT: u8 = 4;
/// Unreliable packets.
pub(crate) const STATE: u32 = 5;
pub(crate) const CLIENT: u32 = 6;
/// Width of the packet kind.
pub(crate) const KIND_BITS: u32 = 8;

/// Records of one entity a client keeps to decode deltas against; the
/// server only names a baseline among the newest this many it sent.
pub(crate) const HISTORY: usize = 16;
/// Largest state packet: one datagram with the transport's headers.
pub(crate) const MAX_PACKET: usize = 1100;
/// State packets a client acknowledges at once (and the server remembers).
pub(crate) const ACK_WINDOW: usize = 64;
/// Inputs a client repeats in every packet until the server applied them.
pub(crate) const INPUT_REDUNDANCY: usize = 16;
/// Kinds one replication can declare (an entity's kinds are a bit mask).
pub(crate) const MAX_KINDS: usize = 64;
/// Net ids a client accepts: a bound on what a bad packet can make it
/// allocate. A million replicated entities is far past any budget.
pub(crate) const MAX_IDS: u32 = 1 << 20;

pub(crate) const CREATE: u32 = 0;
pub(crate) const FULL: u32 = 1;
pub(crate) const DELTA: u32 = 2;

/// Who owns an entity and under which authority epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Meta {
    pub(crate) owner: Option<PeerId>,
    pub(crate) epoch: u16,
}

/// One entity's replicated state: which kinds it has, its meta, its words.
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub(crate) present: u64,
    pub(crate) meta: Meta,
    pub(crate) words: &'a [u32],
}

/// Owner and epoch, as variable-length numbers: 16 bits for a
/// server-owned entity that never changed hands.
pub(crate) fn write_meta(w: &mut BitWriter, meta: Meta) {
    w.write_varint(meta.owner.map_or(0, |p| p.to_bits().saturating_add(1)));
    w.write_varint(u64::from(meta.epoch));
}

pub(crate) fn read_meta(r: &mut BitReader<'_>) -> Option<Meta> {
    let owner = r.read_varint()?;
    let epoch = u16::try_from(r.read_varint()?).ok()?;
    Some(Meta {
        owner: owner.checked_sub(1).map(PeerId::from_bits),
        epoch,
    })
}

/// A field that changed from `base` to `value` (`bits` wide): a small
/// difference in 4, 8 or 16 bits, or the new value in full; 2 bits say
/// which. Narrow fields (6 bits or less) always go in full.
pub(crate) fn write_change(w: &mut BitWriter, base: u32, value: u32, bits: u32) {
    if bits <= 6 {
        w.write(value, bits);
        return;
    }
    let diff = value.wrapping_sub(base) & mask32(bits);
    // The difference as a signed `bits`-wide number, zigzag-encoded.
    let signed = if bits < 32 && diff >= 1 << (bits - 1) {
        i64::from(diff) - (1i64 << bits)
    } else if bits == 32 {
        i64::from(diff as i32)
    } else {
        i64::from(diff)
    };
    let zigzag = ((signed << 1) ^ (signed >> 63)) as u64;
    for (selector, width) in [(0, 4u32), (1, 8), (2, 16)] {
        if width < bits && zigzag < 1 << width {
            w.write(selector, 2);
            w.write(zigzag as u32, width);
            return;
        }
    }
    w.write(3, 2);
    w.write(value, bits);
}

pub(crate) fn read_change(r: &mut BitReader<'_>, base: u32, bits: u32) -> Option<u32> {
    if bits <= 6 {
        return r.read(bits);
    }
    let selector = r.read(2)?;
    let width = match selector {
        0 => 4,
        1 => 8,
        2 => 16,
        _ => return r.read(bits),
    };
    if width >= bits {
        return None;
    }
    let zigzag = u64::from(r.read(width)?);
    let signed = (zigzag >> 1) as i64 ^ -((zigzag & 1) as i64);
    Some((i64::from(base).wrapping_add(signed) as u32) & mask32(bits))
}

/// Writes one entity record: `CREATE` or `FULL` without a baseline,
/// `DELTA` against `base` sent `distance` packets ago.
pub(crate) fn write_record(
    w: &mut BitWriter,
    kinds: &[Kind],
    id: u32,
    create: bool,
    current: View<'_>,
    base: Option<(u16, View<'_>)>,
) {
    w.write_varint(u64::from(id));
    let Some((distance, base)) = base else {
        w.write(if create { CREATE } else { FULL }, 2);
        write_meta(w, current.meta);
        w.write(current.present as u32, 32.min(kinds.len() as u32));
        if kinds.len() > 32 {
            w.write((current.present >> 32) as u32, kinds.len() as u32 - 32);
        }
        for (i, kind) in kinds.iter().enumerate() {
            if current.present & (1 << i) != 0 {
                for (j, spec) in kind.specs.iter().enumerate() {
                    w.write(current.words[kind.offset + j], u32::from(spec.bits));
                }
            }
        }
        return;
    };
    w.write(DELTA, 2);
    w.write_varint(u64::from(distance));
    let meta_changed = current.meta != base.meta;
    w.write_bool(meta_changed);
    if meta_changed {
        write_meta(w, current.meta);
    }
    let present_changed = current.present != base.present;
    w.write_bool(present_changed);
    if present_changed {
        w.write(current.present as u32, 32.min(kinds.len() as u32));
        if kinds.len() > 32 {
            w.write((current.present >> 32) as u32, kinds.len() as u32 - 32);
        }
    }
    for (i, kind) in kinds.iter().enumerate() {
        if current.present & (1 << i) == 0 {
            continue;
        }
        let range = kind.offset..kind.offset + kind.words();
        if base.present & (1 << i) == 0 {
            // New on this entity: every field.
            for (j, spec) in kind.specs.iter().enumerate() {
                w.write(current.words[kind.offset + j], u32::from(spec.bits));
            }
            continue;
        }
        let changed = current.words[range.clone()] != base.words[range];
        w.write_bool(changed);
        if !changed {
            continue;
        }
        for (j, spec) in kind.specs.iter().enumerate() {
            let (now, then) = (current.words[kind.offset + j], base.words[kind.offset + j]);
            w.write_bool(now != then);
            if now != then {
                write_change(w, then, now, u32::from(spec.bits));
            }
        }
    }
}

/// What a record says besides its words (which go into the caller's).
pub(crate) struct Decoded {
    pub(crate) present: u64,
    pub(crate) meta: Meta,
}

/// Reads the header of a record: its id, its type and (for a delta) the
/// distance to its baseline.
pub(crate) fn read_record_head(r: &mut BitReader<'_>) -> Option<(u32, u32, u16)> {
    let id = u32::try_from(r.read_varint()?).ok()?;
    let record = r.read(2)?;
    let distance = if record == DELTA {
        u16::try_from(r.read_varint()?).ok().filter(|&d| d > 0)?
    } else if record > DELTA {
        return None;
    } else {
        0
    };
    Some((id, record, distance))
}

/// Reads the body of a record whose head said `record`: into `out` (the
/// entity's words, every kind at its offset), against `base` for a delta.
pub(crate) fn read_record_body(
    r: &mut BitReader<'_>,
    kinds: &[Kind],
    record: u32,
    base: Option<View<'_>>,
    out: &mut [u32],
) -> Option<Decoded> {
    let read_present = |r: &mut BitReader<'_>| -> Option<u64> {
        let low = u64::from(r.read(32.min(kinds.len() as u32))?);
        let high = if kinds.len() > 32 {
            u64::from(r.read(kinds.len() as u32 - 32)?)
        } else {
            0
        };
        Some(low | high << 32)
    };
    let (meta, present) = if record == DELTA {
        let base = base?;
        let meta = if r.read_bool()? { read_meta(r)? } else { base.meta };
        let present = if r.read_bool()? { read_present(r)? } else { base.present };
        out.copy_from_slice(base.words);
        (meta, present)
    } else {
        let meta = read_meta(r)?;
        let present = read_present(r)?;
        out.fill(0);
        (meta, present)
    };
    for (i, kind) in kinds.iter().enumerate() {
        if present & (1 << i) == 0 {
            continue;
        }
        let fresh = record != DELTA || base.is_none_or(|b| b.present & (1 << i) == 0);
        if fresh {
            for (j, spec) in kind.specs.iter().enumerate() {
                out[kind.offset + j] = r.read(u32::from(spec.bits))?;
            }
            continue;
        }
        if !r.read_bool()? {
            continue;
        }
        for (j, spec) in kind.specs.iter().enumerate() {
            if r.read_bool()? {
                let at = kind.offset + j;
                out[at] = read_change(r, out[at], u32::from(spec.bits))?;
            }
        }
    }
    Some(Decoded { present, meta })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_round_trip_at_every_width_and_distance() {
        for bits in [1u32, 5, 6, 7, 8, 12, 16, 20, 31, 32] {
            let m = mask32(bits);
            let cases = [
                (0u32, 1u32),
                (5, 3),
                (m, 0),
                (0, m),
                (100 & m, 90 & m),
                (m / 2, m / 2 + 7),
                (1, m / 3),
            ];
            for (base, value) in cases {
                let (base, value) = (base & m, value & m);
                let mut w = BitWriter::default();
                write_change(&mut w, base, value, bits);
                let bytes = w.finish().to_vec();
                let got = read_change(&mut BitReader::new(&bytes), base, bits);
                assert_eq!(got, Some(value), "{bits} bits: {base} -> {value}");
            }
        }
    }

    #[test]
    fn small_moves_cost_a_few_bits() {
        let mut w = BitWriter::default();
        write_change(&mut w, 10_000, 10_003, 18);
        assert_eq!(w.bit_len(), 6, "2 + 4 bits for a move of 3 steps");
        let mut w = BitWriter::default();
        write_change(&mut w, 10_000, 9_900, 18);
        assert_eq!(w.bit_len(), 10);
        let mut w = BitWriter::default();
        write_change(&mut w, 0, 200_000, 18);
        assert_eq!(w.bit_len(), 20, "a jump goes in full");
    }

    #[test]
    fn meta_round_trips_with_and_without_an_owner() {
        for meta in [
            Meta::default(),
            Meta {
                owner: Some(PeerId::HOST),
                epoch: 7,
            },
            Meta {
                owner: Some(PeerId::from_bits(900)),
                epoch: u16::MAX,
            },
        ] {
            let mut w = BitWriter::default();
            write_meta(&mut w, meta);
            let bytes = w.finish().to_vec();
            assert_eq!(read_meta(&mut BitReader::new(&bytes)), Some(meta));
        }
    }
}
