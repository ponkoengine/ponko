//! Sound cooking: uncompressed PCM WAV files to FLAC, lossless, at about
//! half the size, in pure Rust.
//!
//! [`wav_to_flac`] reads a WAV file's integer PCM samples (8, 16 or 24 bits,
//! one to eight channels) and writes a FLAC stream of the same samples. The
//! engine finds a sound's format from its first bytes, so the FLAC file is
//! loaded through the same `audio.load("jump.wav")` call. Float and 32-bit
//! WAV files, and anything else, are left as they are (`None`).
//!
//! The encoder uses FLAC's fixed predictors (orders 0 to 4) with partitioned
//! Rice coding of the residual, and picks per block the best of the
//! independent, left/side, right/side and mid/side channel layouts. It is
//! simpler than a full LPC encoder, a few percent larger, and fast. The same
//! samples always give the same bytes. The stream's MD5 field is zero
//! ("not computed"), which the format allows.

#![deny(missing_docs)]

/// Samples per FLAC block (the reference encoder's default).
const BLOCK: usize = 4096;
/// The highest Rice partition order tried.
const MAX_PARTITION_ORDER: u32 = 8;

/// Integer PCM samples read from a WAV file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    /// Samples per second.
    pub sample_rate: u32,
    /// Interleaved channels.
    pub channels: usize,
    /// Bits per sample (8, 16 or 24).
    pub bits: u32,
    /// The samples, interleaved, signed.
    pub samples: Vec<i32>,
}

/// The integer PCM samples of a WAV file, or `None` when it is not one the
/// encoder takes (float, 32-bit, compressed, more than eight channels).
/// Errors are for files that claim to be WAV and are broken.
pub fn read_wav(bytes: &[u8]) -> Result<Option<Pcm>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Ok(None);
    }
    let mut at = 12;
    let mut format: Option<(u16, usize, u32, u32)> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
        let body = at + 8;
        let end = body.checked_add(len).ok_or("a WAV chunk is too long")?;
        if id == b"fmt " {
            if len < 16 || end > bytes.len() {
                return Err("its fmt chunk is too short".into());
            }
            let b = &bytes[body..end];
            let mut tag = u16::from_le_bytes([b[0], b[1]]);
            let channels = usize::from(u16::from_le_bytes([b[2], b[3]]));
            let rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            let bits = u32::from(u16::from_le_bytes([b[14], b[15]]));
            // WAVE_FORMAT_EXTENSIBLE: the real tag starts the sub-format GUID.
            if tag == 0xFFFE && len >= 26 {
                tag = u16::from_le_bytes([b[24], b[25]]);
            }
            format = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let Some((tag, channels, sample_rate, bits)) = format else {
                return Err("its data chunk comes before its fmt chunk".into());
            };
            if tag != 1 || !(1..=8).contains(&channels) || !matches!(bits, 8 | 16 | 24) || sample_rate == 0 {
                return Ok(None);
            }
            if sample_rate >= 1 << 20 {
                return Ok(None);
            }
            let data = &bytes[body..end.min(bytes.len())];
            let width = (bits / 8) as usize;
            let frames = data.len() / (width * channels);
            let samples = data[..frames * width * channels]
                .chunks_exact(width)
                .map(|s| match bits {
                    8 => i32::from(s[0]) - 128,
                    16 => i32::from(i16::from_le_bytes([s[0], s[1]])),
                    _ => (i32::from_le_bytes([0, s[0], s[1], s[2]])) >> 8,
                })
                .collect();
            return Ok(Some(Pcm {
                sample_rate,
                channels,
                bits,
                samples,
            }));
        }
        // Chunks are padded to an even length.
        at = end + (len & 1);
    }
    Err("it has no data chunk".into())
}

/// A FLAC file of a WAV file's samples, or `None` when [`read_wav`] doesn't
/// take the file.
pub fn wav_to_flac(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    Ok(read_wav(bytes)?.map(|pcm| encode(&pcm)))
}

/// Writes bits most significant first.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }

    /// Writes the low `count` bits of `value` (`count` ≤ 32).
    fn put(&mut self, value: u64, count: u32) {
        debug_assert!(count <= 32);
        if count == 0 {
            return;
        }
        self.acc = (self.acc << count) | (value & ((1u64 << count) - 1));
        self.n += count;
        while self.n >= 8 {
            self.n -= 8;
            self.out.push((self.acc >> self.n) as u8);
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    /// Writes `value` as a signed number in `count` bits.
    fn put_signed(&mut self, value: i64, count: u32) {
        self.put(value as u64, count);
    }

    /// `count` zero bits then a one: a Rice quotient.
    fn unary(&mut self, mut count: u64) {
        while count >= 32 {
            self.put(0, 32);
            count -= 32;
        }
        self.put(1, count as u32 + 1);
    }

    /// Pads to a whole byte with zeros.
    fn align(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put(0, pad);
        }
    }
}

/// CRC-8 (polynomial 0x07), FLAC's frame header check.
fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// CRC-16 (polynomial 0x8005), FLAC's frame check.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in bytes {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The residual of fixed predictor `order` (0 to 4) over `x`, from sample
/// `order` on.
fn residual(x: &[i64], order: usize, out: &mut Vec<i64>) {
    out.clear();
    for i in order..x.len() {
        let p = match order {
            0 => 0,
            1 => x[i - 1],
            2 => 2 * x[i - 1] - x[i - 2],
            3 => 3 * x[i - 1] - 3 * x[i - 2] + x[i - 3],
            _ => 4 * x[i - 1] - 6 * x[i - 2] + 4 * x[i - 3] - x[i - 4],
        };
        out.push(x[i] - p);
    }
}

fn zigzag(r: i64) -> u64 {
    ((r << 1) ^ (r >> 63)) as u64
}

/// The best Rice parameter for one partition and its size in bits
/// (parameter field excluded).
fn rice(part: &[i64]) -> (u32, u64) {
    if part.is_empty() {
        return (0, 0);
    }
    let sum: u64 = part.iter().map(|&r| zigzag(r)).sum();
    let mean = sum / part.len() as u64;
    let guess = if mean == 0 { 0 } else { 63 - mean.leading_zeros() };
    let cost =
        |k: u32| -> u64 { part.len() as u64 * u64::from(k + 1) + part.iter().map(|&r| zigzag(r) >> k).sum::<u64>() };
    let mut best = (guess.min(30), cost(guess.min(30)));
    for k in [guess.saturating_sub(1), guess + 1] {
        let k = k.min(30);
        let c = cost(k);
        if c < best.1 {
            best = (k, c);
        }
    }
    best
}

/// The partition order and Rice parameters for a residual of a block of
/// `block` samples whose first `order` samples are warm-up, and the bits.
fn partitions(res: &[i64], block: usize, order: usize) -> (u32, Vec<u32>, u64) {
    let mut best: Option<(u32, Vec<u32>, u64)> = None;
    for p in 0..=MAX_PARTITION_ORDER {
        let parts = 1usize << p;
        if !block.is_multiple_of(parts) || block / parts <= order {
            break;
        }
        let size = block / parts;
        let mut params = Vec::with_capacity(parts);
        let mut bits = 0u64;
        let mut start = 0;
        for i in 0..parts {
            let len = if i == 0 { size - order } else { size };
            let (k, b) = rice(&res[start..start + len]);
            params.push(k);
            bits += b;
            start += len;
        }
        let wide = params.iter().any(|&k| k > 14);
        bits += parts as u64 * if wide { 5 } else { 4 };
        if best.as_ref().is_none_or(|b| bits < b.2) {
            best = Some((p, params, bits));
        }
    }
    best.unwrap_or((0, vec![0], 4))
}

/// One channel's subframe, encoded into a new bit writer's state: returns
/// the encoder's choice and its size, to write later.
enum Subframe {
    Constant(i64),
    Verbatim,
    Fixed {
        order: usize,
        partition_order: u32,
        params: Vec<u32>,
        residual: Vec<i64>,
    },
}

fn choose(x: &[i64], bps: u32) -> (Subframe, u64) {
    if x.iter().all(|&s| s == x[0]) {
        return (Subframe::Constant(x[0]), 8 + u64::from(bps));
    }
    let mut best = (Subframe::Verbatim, 8 + x.len() as u64 * u64::from(bps));
    let mut res = Vec::with_capacity(x.len());
    for order in 0..=4usize.min(x.len().saturating_sub(1)) {
        residual(x, order, &mut res);
        let (partition_order, params, bits) = partitions(&res, x.len(), order);
        let total = 8 + order as u64 * u64::from(bps) + 6 + bits;
        if total < best.1 {
            best = (
                Subframe::Fixed {
                    order,
                    partition_order,
                    params,
                    residual: res.clone(),
                },
                total,
            );
        }
    }
    best
}

fn write_subframe(w: &mut Bits, x: &[i64], bps: u32, sub: &Subframe) {
    match sub {
        Subframe::Constant(v) => {
            w.put(0b0000_0000, 8);
            w.put_signed(*v, bps);
        }
        Subframe::Verbatim => {
            w.put(0b0000_0010, 8);
            for &s in x {
                w.put_signed(s, bps);
            }
        }
        Subframe::Fixed {
            order,
            partition_order,
            params,
            residual,
        } => {
            w.put(0b0001_0000 | ((*order as u64) << 1), 8);
            for &s in &x[..*order] {
                w.put_signed(s, bps);
            }
            let wide = params.iter().any(|&k| k > 14);
            w.put(u64::from(wide), 2);
            w.put(u64::from(*partition_order), 4);
            let size = x.len() >> partition_order;
            let mut start = 0;
            for (i, &k) in params.iter().enumerate() {
                let len = if i == 0 { size - order } else { size };
                w.put(u64::from(k), if wide { 5 } else { 4 });
                for &r in &residual[start..start + len] {
                    let u = zigzag(r);
                    w.unary(u >> k);
                    w.put(u & ((1u64 << k) - 1), k);
                }
                start += len;
            }
        }
    }
}

/// FLAC's UTF-8-like coding of a frame number.
fn coded_number(n: u64) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let mut bytes = Vec::new();
    let mut n = n;
    let mut payload_bits = 6; // room in the first byte shrinks as bytes are added
    while n >= (1 << payload_bits) {
        bytes.push(0x80 | (n & 0x3F) as u8);
        n >>= 6;
        payload_bits -= 1;
    }
    let count = bytes.len() + 1;
    let lead = !(0xFFu8 >> count);
    bytes.push(lead | n as u8);
    bytes.reverse();
    bytes
}

/// Encodes `pcm` as a FLAC file.
pub fn encode(pcm: &Pcm) -> Vec<u8> {
    let channels = pcm.channels;
    let frames = pcm.samples.len() / channels;
    let bps = pcm.bits;
    let mut body = Vec::new();
    let (mut min_frame, mut max_frame) = (u32::MAX, 0u32);
    let mut chans: Vec<Vec<i64>> = vec![Vec::with_capacity(BLOCK); channels];
    for (number, start) in (0..frames).step_by(BLOCK).enumerate() {
        let len = BLOCK.min(frames - start);
        for (c, chan) in chans.iter_mut().enumerate() {
            chan.clear();
            chan.extend((start..start + len).map(|f| i64::from(pcm.samples[f * channels + c])));
        }
        // Stereo: pick the cheapest of the four channel layouts.
        let (assignment, subframes): (u64, Vec<(Vec<i64>, u32, Subframe)>) = if channels == 2 {
            let (l, r) = (&chans[0], &chans[1]);
            let side: Vec<i64> = l.iter().zip(r).map(|(a, b)| a - b).collect();
            let mid: Vec<i64> = l.iter().zip(r).map(|(a, b)| (a + b) >> 1).collect();
            let (cl, bl) = choose(l, bps);
            let (cr, br) = choose(r, bps);
            let (cs, bs) = choose(&side, bps + 1);
            let (cm, bm) = choose(&mid, bps);
            let options = [bl + br, bl + bs, bs + br, bm + bs];
            let best = (0..4).min_by_key(|&i| options[i]).expect("four options");
            match best {
                0 => (1, vec![(l.clone(), bps, cl), (r.clone(), bps, cr)]),
                1 => (8, vec![(l.clone(), bps, cl), (side, bps + 1, cs)]),
                2 => (9, vec![(side, bps + 1, cs), (r.clone(), bps, cr)]),
                _ => (10, vec![(mid, bps, cm), (side, bps + 1, cs)]),
            }
        } else {
            (
                channels as u64 - 1,
                chans
                    .iter()
                    .map(|x| {
                        let (sub, _) = choose(x, bps);
                        (x.clone(), bps, sub)
                    })
                    .collect(),
            )
        };
        let mut w = Bits::new();
        w.put(0xFFF8, 16); // sync, reserved, fixed block size
        w.put(0b0111, 4); // block size: 16 bits at the end of the header
        w.put(0b0000, 4); // sample rate: from STREAMINFO
        w.put(assignment, 4);
        w.put(0b000, 3); // sample size: from STREAMINFO
        w.put(0, 1);
        for b in coded_number(number as u64) {
            w.put(u64::from(b), 8);
        }
        w.put(len as u64 - 1, 16);
        let crc = crc8(&w.out);
        w.put(u64::from(crc), 8);
        for (x, bits, sub) in &subframes {
            write_subframe(&mut w, x, *bits, sub);
        }
        w.align();
        let crc = crc16(&w.out);
        w.put(u64::from(crc), 16);
        let size = w.out.len() as u32;
        min_frame = min_frame.min(size);
        max_frame = max_frame.max(size);
        body.extend_from_slice(&w.out);
    }
    if frames == 0 {
        min_frame = 0;
    }
    let mut out = Vec::with_capacity(body.len() + 42);
    out.extend_from_slice(b"fLaC");
    // STREAMINFO, the last metadata block.
    out.push(0x80);
    out.extend_from_slice(&[0, 0, 34]);
    let block = BLOCK.min(frames.max(16)) as u16;
    let mut info = Bits::new();
    info.put(u64::from(block), 16);
    info.put(u64::from(block), 16);
    info.put(u64::from(min_frame), 24);
    info.put(u64::from(max_frame), 24);
    info.put(u64::from(pcm.sample_rate), 20);
    info.put(channels as u64 - 1, 3);
    info.put(u64::from(bps - 1), 5);
    info.put((frames as u64) >> 32, 4);
    info.put(frames as u64 & 0xFFFF_FFFF, 32);
    for _ in 0..4 {
        info.put(0, 32); // MD5: not computed
    }
    out.extend_from_slice(&info.out);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file of `samples` (interleaved) at `bits`.
    pub fn wav(rate: u32, channels: u16, bits: u16, samples: &[i32]) -> Vec<u8> {
        let width = usize::from(bits / 8);
        let mut data = Vec::new();
        for &s in samples {
            match bits {
                8 => data.push((s + 128) as u8),
                16 => data.extend_from_slice(&(s as i16).to_le_bytes()),
                _ => data.extend_from_slice(&s.to_le_bytes()[..3]),
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * width as u32).to_le_bytes());
        out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    /// A tone with some noise, `frames` long, scaled to `bits`.
    pub fn tone(frames: usize, channels: usize, bits: u32) -> Vec<i32> {
        let peak = f64::from((1 << (bits - 1)) - 1) * 0.6;
        let mut seed = 12345u32;
        (0..frames * channels)
            .map(|i| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = f64::from((seed >> 16) & 0xFF) / 255.0 - 0.5;
                let t = (i / channels) as f64 / 44_100.0;
                let c = (i % channels) as f64;
                ((t * (440.0 + c * 110.0) * std::f64::consts::TAU).sin() * peak + noise * peak * 0.02) as i32
            })
            .collect()
    }

    /// Decodes a FLAC file with symphonia, the decoder the engine plays
    /// FLAC with, to interleaved samples at their own bit depth.
    pub fn decode(bytes: &[u8], bits: u32) -> (u32, usize, Vec<i32>) {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::formats::{TrackType, probe::Hint};
        use symphonia::core::io::MediaSourceStream;
        let source = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
        let mut reader = symphonia::default::get_probe()
            .probe(&Hint::default(), source, Default::default(), Default::default())
            .expect("a FLAC stream");
        let track = reader.default_track(TrackType::Audio).expect("an audio track");
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            panic!("not audio")
        };
        let id = track.id;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &Default::default())
            .expect("a FLAC decoder");
        let mut out: Vec<i32> = Vec::new();
        let mut channels = 0;
        while let Ok(Some(packet)) = reader.next_packet() {
            if packet.track_id != id {
                continue;
            }
            let buffer = decoder.decode(&packet).expect("a good packet");
            channels = buffer.num_planes();
            let mut samples: Vec<i32> = Vec::new();
            buffer.copy_to_vec_interleaved(&mut samples);
            out.extend(samples);
        }
        // i32 samples are scaled to the full 32 bits.
        let shift = 32 - bits;
        (
            params.sample_rate.unwrap_or(0),
            channels,
            out.into_iter().map(|s| s >> shift).collect(),
        )
    }

    #[test]
    fn coded_numbers_follow_the_utf8_like_scheme() {
        assert_eq!(coded_number(0), [0]);
        assert_eq!(coded_number(0x7F), [0x7F]);
        assert_eq!(coded_number(0x80), [0xC2, 0x80]);
        assert_eq!(coded_number(0x7FF), [0xDF, 0xBF]);
        assert_eq!(coded_number(0x800), [0xE0, 0xA0, 0x80]);
    }

    #[test]
    fn crcs_match_flacs_check_values() {
        // The standard check value of each CRC over "123456789".
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
    }

    #[test]
    fn flac_decodes_to_the_same_samples_for_every_depth_and_layout() {
        for (channels, bits, frames) in [
            (1, 16, 10_000),
            (2, 16, 9_000),
            (2, 24, 5_000),
            (1, 8, 3_000),
            (3, 16, 4_100),
        ] {
            let samples = tone(frames, channels, bits);
            let file = wav(44_100, channels as u16, bits as u16, &samples);
            let flac = wav_to_flac(&file).unwrap().expect("a PCM WAV");
            assert!(flac.starts_with(b"fLaC"));
            let (rate, got_channels, decoded) = decode(&flac, bits);
            assert_eq!(rate, 44_100);
            assert_eq!(got_channels, channels);
            assert_eq!(decoded, samples, "{channels} channels, {bits} bits");
        }
    }

    #[test]
    fn flac_is_smaller_than_the_wav_and_the_same_bytes_every_time() {
        let samples = tone(44_100, 2, 16);
        let file = wav(44_100, 2, 16, &samples);
        let a = wav_to_flac(&file).unwrap().unwrap();
        let b = wav_to_flac(&file).unwrap().unwrap();
        assert_eq!(a, b);
        assert!(a.len() < file.len() * 3 / 4, "{} of {}", a.len(), file.len());
    }

    #[test]
    fn silence_and_tiny_files_encode() {
        for frames in [0, 1, 5, 4096, 4097] {
            let samples = vec![0; frames];
            let flac = wav_to_flac(&wav(22_050, 1, 16, &samples)).unwrap().unwrap();
            if frames > 0 {
                assert_eq!(decode(&flac, 16).2, samples, "{frames} frames");
            }
        }
    }

    #[test]
    fn float_and_other_files_are_left_alone() {
        let mut float = wav(44_100, 1, 16, &[0, 1, 2]);
        float[20] = 3; // WAVE_FORMAT_IEEE_FLOAT
        assert_eq!(wav_to_flac(&float).unwrap(), None);
        assert_eq!(wav_to_flac(b"OggS....").unwrap(), None);
        assert!(wav_to_flac(b"RIFF\0\0\0\0WAVE").is_err());
    }
}
