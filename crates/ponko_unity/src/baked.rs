//! The runtime data Unity bakes into animation clips: streamed, dense and
//! constant curves bound to targets by hashed paths, and the bit-packed
//! vectors of compressed rotation curves.
//!
//! These decoders are written from the formats' public descriptions.

use crate::{Error, Value};

/// One key of a single float channel: Hermite value and tangents.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SKey {
    pub time: f32,
    pub value: f32,
    pub incoming: f32,
    pub outgoing: f32,
}

/// Standard CRC-32 (the one Unity hashes transform paths and property
/// names with).
pub(crate) fn crc32(text: &str) -> u32 {
    !crc32_extend(!0, text.as_bytes())
}

/// The CRC-32 state after `bytes`, from the state `crc` (`!0` to start;
/// the checksum is `!` the final state).
pub(crate) fn crc32_extend(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

// --- Reading numeric lists out of the value tree ---

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// A list of numbers, however the file stored it: a binary array, a YAML
/// list or (for bytes) a hex string.
fn numbers(v: &Value) -> Vec<f64> {
    match v {
        Value::Numbers(n) => n.clone(),
        Value::Seq(s) => s.iter().filter_map(|x| x.as_str()?.trim().parse().ok()).collect(),
        Value::Bytes(b) => b.iter().map(|&x| f64::from(x)).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn bytes_of(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.clone(),
        Value::Scalar(s) => hex_bytes(s).unwrap_or_default(),
        other => numbers(other).into_iter().map(|x| x as u8).collect(),
    }
}

fn u32s_of(v: &Value) -> Vec<u32> {
    if let Value::Scalar(s) = v {
        return hex_bytes(s)
            .unwrap_or_default()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
    }
    numbers(v).into_iter().map(|x| x as u32).collect()
}

fn f32s_of(v: &Value) -> Vec<f32> {
    if let Value::Scalar(s) = v {
        return hex_bytes(s)
            .unwrap_or_default()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
    }
    numbers(v).into_iter().map(|x| x as f32).collect()
}

fn count(v: &Value) -> usize {
    v.as_i64().unwrap_or(0).max(0) as usize
}

// --- Bit-packed vectors ---

/// Reads little-endian bit fields: each value fills a byte from its low
/// bits first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u32, Error> {
        let mut value = 0u64;
        let mut got = 0;
        while got < n {
            let byte = *self
                .data
                .get(self.at / 8)
                .ok_or_else(|| Error::decode("packed animation data ends early"))?;
            let offset = (self.at % 8) as u32;
            let take = (n - got).min(8 - offset);
            value |= u64::from((byte >> offset) & ((1u16 << take) - 1) as u8) << got;
            got += take;
            self.at += take as usize;
        }
        Ok(value as u32)
    }
}

/// `m_NumItems` unsigned integers of `m_BitSize` bits.
/// The most items a packed vector with no bits per item (so no data to
/// check its count against) may unpack to.
const MAX_UNPACKED: usize = 1 << 20;

pub(crate) fn packed_ints(v: &Value) -> Result<Vec<u32>, Error> {
    let items = count(&v["m_NumItems"]);
    let size = v["m_BitSize"].as_i64().unwrap_or(0).clamp(0, 32) as u32;
    if size == 0 && items > MAX_UNPACKED {
        return Err(Error::decode("packed animation data: too many items"));
    }
    let data = bytes_of(&v["m_Data"]);
    if (items as u64 * u64::from(size)).div_ceil(8) > data.len() as u64 {
        return Err(Error::decode("packed animation data ends early"));
    }
    let mut bits = Bits { data: &data, at: 0 };
    (0..items).map(|_| bits.read(size)).collect()
}

/// `m_NumItems` floats quantized to `m_BitSize` bits over `m_Range`
/// starting at `m_Start`.
pub(crate) fn packed_floats(v: &Value) -> Result<Vec<f32>, Error> {
    let items = count(&v["m_NumItems"]);
    let size = v["m_BitSize"].as_i64().unwrap_or(0).clamp(0, 32) as u32;
    let range = v["m_Range"].as_f32().unwrap_or(0.0);
    let start = v["m_Start"].as_f32().unwrap_or(0.0);
    if size == 0 {
        if items > MAX_UNPACKED {
            return Err(Error::decode("packed animation data: too many items"));
        }
        return Ok(vec![start; items]);
    }
    let data = bytes_of(&v["m_Data"]);
    if (items as u64 * u64::from(size)).div_ceil(8) > data.len() as u64 {
        return Err(Error::decode("packed animation data ends early"));
    }
    let max = ((1u64 << size) - 1) as f32;
    let mut bits = Bits { data: &data, at: 0 };
    (0..items)
        .map(|_| Ok(bits.read(size)? as f32 / max * range + start))
        .collect()
}

/// `m_NumItems` unit quaternions in 32 bits each: three flag bits (the
/// dropped largest component's index and sign), then the other three
/// components in 9 or 10 bits.
pub(crate) fn packed_quats(v: &Value) -> Result<Vec<[f32; 4]>, Error> {
    let items = count(&v["m_NumItems"]);
    let data = bytes_of(&v["m_Data"]);
    if items as u64 * 4 > data.len() as u64 {
        return Err(Error::decode("packed animation data ends early"));
    }
    let mut bits = Bits { data: &data, at: 0 };
    let mut out = Vec::with_capacity(items);
    for _ in 0..items {
        let flags = bits.read(3)?;
        let dropped = (flags & 3) as usize;
        let mut q = [0f32; 4];
        let mut sum = 0.0;
        for (j, slot) in q.iter_mut().enumerate() {
            if j == dropped {
                continue;
            }
            let size = if (dropped + 1) % 4 == j { 9 } else { 10 };
            let x = bits.read(size)? as f32;
            *slot = x / (0.5 * ((1u32 << size) - 1) as f32) - 1.0;
            sum += *slot * *slot;
        }
        q[dropped] = (1.0 - sum).max(0.0).sqrt();
        if flags & 4 != 0 {
            q[dropped] = -q[dropped];
        }
        out.push(q);
    }
    Ok(out)
}

/// A decoded `m_CompressedRotationCurves` entry: key times and values with
/// in and out tangents of the four components.
pub(crate) struct CompressedKeys {
    pub time: f32,
    pub value: [f32; 4],
    pub incoming: [f32; 4],
    pub outgoing: [f32; 4],
}

/// Decodes one compressed rotation curve. Times are stored as differences
/// in hundredths of a second.
pub(crate) fn compressed_rotation(entry: &Value) -> Result<Vec<CompressedKeys>, Error> {
    // The values' count is checked against their data; the others' counts
    // are checked against it before anything is unpacked (with no bits per
    // item, they have no data to check).
    let values = packed_quats(&entry["m_Values"])?;
    let n = values.len();
    if count(&entry["m_Times"]["m_NumItems"]) != n {
        return Err(Error::decode(
            "compressed rotation curve: times and values differ in count",
        ));
    }
    if ![0, n * 4, n * 8].contains(&count(&entry["m_Slopes"]["m_NumItems"])) {
        return Err(Error::decode("compressed rotation curve: unexpected slope count"));
    }
    let times = packed_ints(&entry["m_Times"])?;
    let slopes = packed_floats(&entry["m_Slopes"])?;
    // Per key either in then out tangents (eight floats), one set used for
    // both (four), or none.
    let stride = match slopes.len() {
        0 => 0,
        l if l == n * 8 => 8,
        l if l == n * 4 => 4,
        _ => return Err(Error::decode("compressed rotation curve: unexpected slope count")),
    };
    let mut keys = Vec::with_capacity(n);
    let mut ticks = 0u64;
    for (i, (&delta, &value)) in times.iter().zip(&values).enumerate() {
        ticks += u64::from(delta);
        let tangent = |at: usize| -> [f32; 4] {
            if stride == 0 {
                [0.0; 4]
            } else {
                std::array::from_fn(|c| slopes[i * stride + at + c])
            }
        };
        keys.push(CompressedKeys {
            time: ticks as f32 * 0.01,
            value,
            incoming: tangent(0),
            outgoing: tangent(stride / 2),
        });
    }
    Ok(keys)
}

// --- Streamed, dense and constant clips ---

/// Every float channel of a baked clip, in the order bindings consume them:
/// streamed channels, then dense, then constant.
pub(crate) fn channels(clip: &Value) -> Result<Vec<Vec<SKey>>, Error> {
    let mut out = streamed(&clip["m_StreamedClip"])?;
    out.extend(dense(&clip["m_DenseClip"])?);
    out.extend(f32s_of(&clip["m_ConstantClip"]["data"]).into_iter().map(|v| {
        vec![SKey {
            time: 0.0,
            value: v,
            incoming: 0.0,
            outgoing: 0.0,
        }]
    }));
    Ok(out)
}

/// One streamed key: the cubic `a s^3 + b s^2 + c s + d` over the time `s`
/// since the key, up to the curve's next key.
struct Cubic {
    time: f32,
    coef: [f32; 4],
}

impl Cubic {
    /// Infinite coefficients, or a constant, hold the value until the next key.
    fn is_step(&self) -> bool {
        let [a, b, c, _] = self.coef;
        !(a.is_finite() && b.is_finite() && c.is_finite()) || (a == 0.0 && b == 0.0 && c == 0.0)
    }
}

fn streamed(clip: &Value) -> Result<Vec<Vec<SKey>>, Error> {
    let curves = count(&clip["curveCount"]);
    let words = u32s_of(&clip["data"]);
    // Each curve has keys in the data (5 words each), so a count past the
    // data's size is corrupt.
    if curves > words.len() {
        return Err(Error::decode("streamed clip: too many curves"));
    }
    let mut keys: Vec<Vec<Cubic>> = (0..curves).map(|_| Vec::new()).collect();
    let mut initial: Vec<Option<f32>> = vec![None; curves];
    let mut at = 0;
    let short = || Error::decode("streamed clip: data ends early");
    while at < words.len() {
        let time = f32::from_bits(words[at]);
        let n = *words.get(at + 1).ok_or_else(short)? as usize;
        at += 2;
        let body = words
            .get(at..at.saturating_add(n.saturating_mul(5)))
            .ok_or_else(short)?;
        at += n * 5;
        // The first frame (about -f32::MAX) holds each curve's value before
        // its first key; the last (+inf) closes the stream.
        let sentinel = !time.is_finite() || time.abs() >= 1e30;
        for key in body.as_chunks::<5>().0 {
            let curve = key[0] as usize;
            if curve >= curves {
                return Err(Error::decode("streamed clip: key for a curve past the curve count"));
            }
            let coef = [1, 2, 3, 4].map(|i| f32::from_bits(key[i]));
            if sentinel {
                initial[curve].get_or_insert(coef[3]);
            } else {
                keys[curve].push(Cubic { time, coef });
            }
        }
    }
    Ok(keys
        .into_iter()
        .zip(initial)
        .map(|(cubics, initial)| cubic_keys(cubics, initial))
        .collect())
}

fn cubic_keys(mut cubics: Vec<Cubic>, initial: Option<f32>) -> Vec<SKey> {
    cubics.sort_by(|a, b| a.time.total_cmp(&b.time));
    // A later key at the same time replaces an earlier one (the sort is
    // stable): backwards, the first of each time is kept.
    cubics.reverse();
    cubics.dedup_by(|later, kept| later.time == kept.time);
    cubics.reverse();
    if cubics.is_empty() {
        return initial
            .map(|value| SKey {
                time: 0.0,
                value,
                incoming: 0.0,
                outgoing: 0.0,
            })
            .into_iter()
            .collect();
    }
    let mut out: Vec<SKey> = Vec::with_capacity(cubics.len());
    for (i, key) in cubics.iter().enumerate() {
        let [_, _, c, d] = key.coef;
        let incoming = match i.checked_sub(1).map(|p| &cubics[p]) {
            None => 0.0,
            Some(prev) if prev.is_step() => f32::INFINITY,
            Some(prev) => {
                // The slope the previous cubic arrives with, from its start
                // slope, its curvature and where it ends.
                let dt = (key.time - prev.time).max(0.0001);
                (3.0 * (d - prev.coef[3]) - 2.0 * prev.coef[2] * dt - prev.coef[1] * dt * dt) / dt
            }
        };
        out.push(SKey {
            time: key.time,
            value: d,
            incoming,
            outgoing: if key.is_step() { f32::INFINITY } else { c },
        });
    }
    out
}

fn dense(clip: &Value) -> Result<Vec<Vec<SKey>>, Error> {
    let curves = count(&clip["m_CurveCount"]);
    let frames = count(&clip["m_FrameCount"]);
    let rate = clip["m_SampleRate"].as_f32().unwrap_or(0.0);
    let begin = clip["m_BeginTime"].as_f32().unwrap_or(0.0);
    let samples = f32s_of(&clip["m_SampleArray"]);
    if curves == 0 || frames == 0 {
        // Curves with no frames: empty, but each still takes its binding.
        if curves > MAX_UNPACKED {
            return Err(Error::decode("dense clip: too many curves"));
        }
        return Ok(vec![Vec::new(); curves]);
    }
    if !(rate.is_finite() && rate > 0.0) || !begin.is_finite() {
        return Err(Error::invalid("dense clip: invalid sample rate"));
    }
    if (frames as u64) * (curves as u64) > samples.len() as u64 {
        return Err(Error::invalid("dense clip: fewer samples than frames times curves"));
    }
    let dt = 1.0 / rate;
    Ok((0..curves)
        .map(|c| {
            let at = |f: usize| samples[f * curves + c];
            (0..frames)
                .map(|f| {
                    // Dense samples are interpolated linearly: each tangent
                    // is the slope of the segment on its side.
                    let before = if f > 0 { (at(f) - at(f - 1)) / dt } else { 0.0 };
                    let after = if f + 1 < frames { (at(f + 1) - at(f)) / dt } else { 0.0 };
                    SKey {
                        time: begin + f as f32 * dt,
                        value: at(f),
                        incoming: if f > 0 { before } else { after },
                        outgoing: if f + 1 < frames { after } else { before },
                    }
                })
                .collect()
        })
        .collect())
}

// --- Bindings ---

/// What a binding drives, with the first channel it reads.
pub(crate) enum Target {
    /// A transform property: 1 position, 2 rotation, 3 scale, 4 Euler.
    Transform { path: u32, attribute: u32 },
    /// A blend shape of a skinned mesh, by the hash of `blendShape.<name>`.
    BlendShape { path: u32, hash: u32 },
    /// A muscle or root curve of a humanoid clip.
    Humanoid,
    /// Anything else: its class id.
    Other(i64),
}

pub(crate) struct Binding {
    pub target: Target,
    /// First channel, and how many it reads.
    pub first: usize,
    pub width: usize,
}

/// The bindings' channel ranges, and the total channels they read.
pub(crate) fn bindings(constant: &Value) -> Vec<Binding> {
    let mut out = Vec::new();
    let mut next = 0;
    for b in constant["genericBindings"].as_seq().unwrap_or_default() {
        let path = b["path"].as_i64().unwrap_or(0) as u32;
        let attribute = b["attribute"].as_i64().unwrap_or(0) as u32;
        let type_id = b["typeID"].as_i64().or_else(|| b["classID"].as_i64()).unwrap_or(0);
        let custom = b["customType"].as_i64().unwrap_or(0);
        // Object reference curves live outside the float channels.
        if b["isPPtrCurve"].as_bool().unwrap_or(false) {
            out.push(Binding {
                target: Target::Other(-1),
                first: next,
                width: 0,
            });
            continue;
        }
        let (target, width) = match type_id {
            4 => (
                Target::Transform { path, attribute },
                match attribute {
                    2 => 4,
                    1 | 3 | 4 => 3,
                    _ => 1,
                },
            ),
            95 => (Target::Humanoid, 1),
            137 if custom == 20 => (Target::BlendShape { path, hash: attribute }, 1),
            other => (Target::Other(other), 1),
        };
        out.push(Binding {
            target,
            first: next,
            width,
        });
        next += width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32("123456789"), 0xCBF4_3926);
        assert_eq!(crc32(""), 0);
    }

    #[test]
    fn bits_read_low_bits_first_across_bytes() {
        let mut b = Bits {
            data: &[0b1010_1101, 0b0000_0011],
            at: 0,
        };
        assert_eq!(b.read(3).unwrap(), 0b101);
        assert_eq!(b.read(7).unwrap(), 0b111_0101);
        assert!(b.read(8).is_err());
    }

    fn map(fields: &[(&str, Value)]) -> Value {
        Value::Map(fields.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect())
    }
    fn scalar(x: impl ToString) -> Value {
        Value::Scalar(x.to_string())
    }

    #[test]
    fn packed_ints_and_floats_unpack_their_bit_fields() {
        // Three 3-bit values 1, 2, 3 laid low bits first: 011_010_001.
        let packed = map(&[
            ("m_NumItems", scalar(3)),
            ("m_BitSize", scalar(3)),
            (
                "m_Data",
                Value::Numbers(vec![f64::from(1 | 2 << 3 | (3 & 3) << 6), f64::from(3 >> 2)]),
            ),
        ]);
        assert_eq!(packed_ints(&packed).unwrap(), [1, 2, 3]);
        // 4-bit floats over [-1, 2]: 0, 5 and 15 map to -1, 0 and 2.
        let floats = map(&[
            ("m_NumItems", scalar(3)),
            ("m_BitSize", scalar(4)),
            ("m_Range", scalar(3)),
            ("m_Start", scalar(-1)),
            ("m_Data", Value::Numbers(vec![f64::from(5 << 4), f64::from(15)])),
        ]);
        let out = packed_floats(&floats).unwrap();
        assert_eq!(out, [-1.0, 0.0, 2.0]);
        // No bits: every value is the start.
        let flat = map(&[
            ("m_NumItems", scalar(2)),
            ("m_BitSize", scalar(0)),
            ("m_Start", scalar(0.5)),
        ]);
        assert_eq!(packed_floats(&flat).unwrap(), [0.5, 0.5]);
    }

    #[test]
    fn streamed_keys_carry_value_slopes_and_steps() {
        let cubics = vec![
            Cubic {
                time: 0.0,
                coef: [0.0, 0.0, 2.0, 0.0],
            },
            Cubic {
                time: 1.0,
                coef: [-2.0, 3.0, 0.0, 2.0],
            },
            Cubic {
                time: 2.0,
                coef: [0.0, 0.0, 0.0, 3.0],
            },
            Cubic {
                time: 3.0,
                coef: [0.0, 0.0, 0.0, 3.0],
            },
        ];
        let keys = cubic_keys(cubics, None);
        // Linear start: both slopes 2. The eased segment arrives flat. A
        // constant segment is a step, which the key after it sees too.
        assert_eq!((keys[0].value, keys[0].outgoing), (0.0, 2.0));
        assert_eq!((keys[1].value, keys[1].incoming, keys[1].outgoing), (2.0, 2.0, 0.0));
        assert_eq!(keys[2].incoming, 0.0);
        assert!(keys[2].outgoing.is_infinite() && keys[3].incoming.is_infinite());
        // A curve with only its initial value is a constant.
        let only = cubic_keys(Vec::new(), Some(4.0));
        assert_eq!((only.len(), only[0].value), (1, 4.0));
    }

    #[test]
    fn dense_samples_are_linear_between_frames() {
        let clip = map(&[
            ("m_FrameCount", scalar(3)),
            ("m_CurveCount", scalar(1)),
            ("m_SampleRate", scalar(2)),
            ("m_BeginTime", scalar(1)),
            ("m_SampleArray", Value::Numbers(vec![0.0, 1.0, 1.0])),
        ]);
        let keys = &dense(&clip).unwrap()[0];
        assert_eq!(keys.iter().map(|k| k.time).collect::<Vec<_>>(), [1.0, 1.5, 2.0]);
        assert_eq!((keys[0].incoming, keys[0].outgoing), (2.0, 2.0));
        assert_eq!((keys[1].incoming, keys[1].outgoing), (2.0, 0.0));
        assert_eq!((keys[2].incoming, keys[2].outgoing), (0.0, 0.0));
    }
}
