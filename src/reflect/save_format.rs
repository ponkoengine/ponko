//! How a world save is kept as bytes: JSON text, or the same save in a
//! compact binary encoding ([`SaveFormat`]).

use std::io::{Read, Write};

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

use crate::error::{Error, ErrorKind};

/// The first bytes of a binary save. `0x89` is never the first byte of JSON
/// text (nor of UTF-8 text), so a file is told apart by its start.
const MAGIC: &[u8; 8] = b"\x89PNKSAV\n";
/// The layout after [`MAGIC`]: 1 is an LZ4 frame of MessagePack.
const LAYOUT: u8 = 1;

/// How a world save ([`WorldReflect::save`](super::WorldReflect::save)) is
/// kept as bytes. Both hold the same save: the same entities, components,
/// resources, versions and [`SaveMigrations`](super::SaveMigrations), so a
/// save converts between them without loss, and every load
/// ([`decode`](Self::decode), [`Ctx::load_world`](crate::ctx::Ctx::load_world))
/// reads either, telling them apart by their first bytes.
///
/// [`Storage::set_save_format`](crate::storage::Storage::set_save_format)
/// picks the one save slots are written in.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::reflect::SaveFormat;
///
/// let mut world = World::new();
/// world.spawn((ponko::d2::Transform::at(vec2(1.0, 2.0)),));
/// let save = world.save();
/// let bytes = SaveFormat::Binary.encode(&save);
/// assert_eq!(SaveFormat::detect(&bytes), SaveFormat::Binary);
/// assert_eq!(SaveFormat::decode(&bytes).unwrap(), save);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SaveFormat {
    /// Pretty-printed JSON text, to read and edit by hand and to diff.
    /// The default.
    #[default]
    Json,
    /// [MessagePack](https://msgpack.org) of the same save in an
    /// [LZ4](https://lz4.org) frame (with its content checksum), after an
    /// 8-byte mark and a layout byte. Numbers that fit an `f32` exactly
    /// (every `f32` field) take 4 bytes. Much smaller and quicker to read
    /// and write than JSON; the format to pick for large worlds.
    Binary,
}

impl SaveFormat {
    /// The extension of a save slot file in this format, without the dot:
    /// `"json"` or `"save"`.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Binary => "save",
        }
    }

    /// The format `bytes` are in, by their first bytes: [`Binary`](Self::Binary)
    /// if they start with its mark, [`Json`](Self::Json) otherwise.
    #[must_use]
    pub fn detect(bytes: &[u8]) -> Self {
        if bytes.starts_with(MAGIC) {
            Self::Binary
        } else {
            Self::Json
        }
    }

    /// `save` in this format. Never fails: any JSON value can be written.
    #[must_use]
    pub fn encode(self, save: &Value) -> Vec<u8> {
        match self {
            Self::Json => serde_json::to_vec_pretty(save).expect("a JSON value always serializes"),
            Self::Binary => {
                let mut packed = Vec::new();
                Narrow(save)
                    .serialize(&mut rmp_serde::Serializer::new(&mut packed))
                    .expect("writing MessagePack to memory can't fail");
                let mut out = Vec::with_capacity(packed.len() / 4 + MAGIC.len() + 1);
                out.extend_from_slice(MAGIC);
                out.push(LAYOUT);
                let info = lz4_flex::frame::FrameInfo::new()
                    .content_checksum(true)
                    .block_size(lz4_flex::frame::BlockSize::Max4MB);
                let mut lz4 = lz4_flex::frame::FrameEncoder::with_frame_info(info, out);
                lz4.write_all(&packed).expect("compressing to memory can't fail");
                lz4.finish().expect("compressing to memory can't fail")
            }
        }
    }

    /// Reads a save written in either format ([`detect`](Self::detect)
    /// says which). It isn't checked or migrated here: loading it
    /// ([`WorldReflect::load`](super::WorldReflect::load)) does that.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Json`] for JSON text that doesn't parse;
    /// [`ErrorKind::Decode`] for a binary save that is cut short, damaged
    /// (its checksum doesn't match) or of a layout this engine doesn't know.
    pub fn decode(bytes: &[u8]) -> Result<Value, Error> {
        let Some(rest) = bytes.strip_prefix(MAGIC) else {
            return serde_json::from_slice(bytes).map_err(Error::from);
        };
        let bad = |why: String| Error::new(ErrorKind::Decode, format!("binary save: {why}"));
        match rest.split_first() {
            Some((&LAYOUT, frame)) => {
                let mut packed = Vec::new();
                lz4_flex::frame::FrameDecoder::new(frame)
                    .read_to_end(&mut packed)
                    .map_err(|e| bad(format!("damaged or cut short ({e})")))?;
                rmp_serde::from_slice(&packed).map_err(|e| bad(format!("not a save ({e})")))
            }
            Some((layout, _)) => Err(bad(format!(
                "layout {layout} is newer than this engine reads ({LAYOUT})"
            ))),
            None => Err(bad("cut short".into())),
        }
    }
}

/// A JSON value written as MessagePack, with each number in its smallest
/// exact form: integers as the compact integer types, and floats that an
/// `f32` holds exactly (every value of an `f32` field) as `f32`. Reading
/// one back gives the same value: an `f32` widens to the same `f64`.
struct Narrow<'a>(&'a Value);

impl Serialize for Narrow<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    s.serialize_u64(u)
                } else if let Some(i) = n.as_i64() {
                    s.serialize_i64(i)
                } else {
                    let f = n.as_f64().unwrap_or(f64::NAN);
                    #[allow(clippy::cast_possible_truncation)] // checked: only when exact
                    let narrow = f as f32;
                    if f64::from(narrow).to_bits() == f.to_bits() {
                        s.serialize_f32(narrow)
                    } else {
                        s.serialize_f64(f)
                    }
                }
            }
            Value::String(text) => s.serialize_str(text),
            Value::Array(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&Narrow(item))?;
                }
                seq.end()
            }
            Value::Object(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, &Narrow(value))?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_kind_of_value_survives_the_binary_format() {
        let save = json!({
            "format": "ponko-world",
            "version": 1,
            "game_version": 3,
            "entities": [{ "entity": 4_294_967_296_u64, "components": {
                "A": { "f32": 0.1_f32, "f64": 0.1, "neg": -7, "big": u64::MAX, "small": i64::MIN,
                       "zero": -0.0, "huge": 1e300, "tiny": 1e-300, "pi": std::f64::consts::PI,
                       "text": "ünïcode ✓", "empty": "", "none": null, "yes": true, "no": false,
                       "list": [1, 2.5, [], {}], "nested": { "deep": [[[1]]] } }
            }}],
            "resources": {},
            "not_saved": {}
        });
        let bytes = SaveFormat::Binary.encode(&save);
        let back = SaveFormat::decode(&bytes).unwrap();
        assert_eq!(back, save);
        // Bit for bit, not just equal: -0.0 stays negative, 0.1 stays 0.1.
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            serde_json::to_string(&save).unwrap()
        );
    }

    #[test]
    fn json_saves_are_detected_and_read_as_before() {
        let save = json!({ "format": "ponko-world", "version": 1, "entities": [] });
        for bytes in [
            serde_json::to_vec(&save).unwrap(),
            serde_json::to_vec_pretty(&save).unwrap(),
            SaveFormat::Json.encode(&save),
        ] {
            assert_eq!(SaveFormat::detect(&bytes), SaveFormat::Json);
            assert_eq!(SaveFormat::decode(&bytes).unwrap(), save);
        }
    }

    #[test]
    fn damaged_binary_saves_are_refused_not_misread() {
        let save = json!({ "format": "ponko-world", "version": 1, "entities": [{ "entity": 1, "components": { "A": { "x": 1.5 } } }] });
        let bytes = SaveFormat::Binary.encode(&save);
        let kind = |b: &[u8]| SaveFormat::decode(b).unwrap_err().kind();
        // Cut short anywhere after the mark.
        for len in [MAGIC.len(), MAGIC.len() + 1, MAGIC.len() + 5, bytes.len() - 1] {
            assert_eq!(kind(&bytes[..len]), ErrorKind::Decode, "cut at {len}");
        }
        // A flipped byte in the compressed data: the checksum catches it.
        // (The frame's header is 7 bytes and a block's size 4: byte 15 of
        // the frame is data.)
        let mut flipped = bytes.clone();
        flipped[MAGIC.len() + 1 + 15] ^= 0x40;
        assert_eq!(kind(&flipped), ErrorKind::Decode);
        // A layout from a newer engine.
        let mut newer = bytes.clone();
        newer[MAGIC.len()] = LAYOUT + 1;
        let err = SaveFormat::decode(&newer).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(err.to_string().contains("newer"), "{err}");
        // Not JSON either.
        assert_eq!(kind(b"\x89PNG"), ErrorKind::Json);
    }

    #[test]
    fn binary_saves_are_smaller_than_json() {
        let entities: Vec<Value> = (0..1000_u64)
            .map(|i| {
                json!({ "entity": i, "components": { "Transform2D": {
                "position": [i as f32 * 0.37, i as f32 * 1.13], "rotation": 0.0, "scale": [1.0, 1.0] } } })
            })
            .collect();
        let save = json!({ "format": "ponko-world", "version": 1, "entities": entities });
        let json = SaveFormat::Json.encode(&save).len();
        let binary = SaveFormat::Binary.encode(&save).len();
        assert!(binary * 4 < json, "binary {binary} bytes, JSON {json} bytes");
    }
}
