//! What a file really is, from its first bytes, for the message of a loader
//! that can't read it.
//!
//! The usual mistakes with a wrong asset are a sound passed to
//! `load_texture`, a file whose extension was changed by hand, an empty
//! file left by a failed export and a Git LFS pointer standing in for the
//! real file. A decoder's own words ("Invalid PNG signature") name none of
//! them; [`not_a`] does.

/// How many leading bytes [`Sniff`] keeps.
const HEAD: usize = 64;

/// The first bytes of a file and its length: all a message needs, so the
/// bytes themselves can be handed on to a decoder first.
#[derive(Clone)]
pub(crate) struct Sniff {
    head: [u8; HEAD],
    kept: usize,
    len: usize,
}

/// The kind of file a loader wanted, or found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Class {
    Image,
    Sound,
    Font,
    Model,
    Data,
    Archive,
}

impl Class {
    /// The noun in "is not a valid ...".
    fn noun(self) -> &'static str {
        match self {
            Class::Image => "image",
            Class::Sound => "sound",
            Class::Font => "font",
            Class::Model => "model",
            Class::Data => "data file",
            Class::Archive => "archive",
        }
    }

    /// The call that loads this kind of file, when there is a plain one.
    fn loader(self) -> Option<&'static str> {
        match self {
            Class::Image => Some("`ctx.gfx.load_texture`"),
            Class::Sound => Some("`ctx.audio.load`"),
            Class::Font => Some("`ctx.gfx.load_font`"),
            Class::Model => Some("`ctx.spawn_prefab` (with the `3d` feature)"),
            Class::Data => Some("`ctx.load_scene`, `ctx.spawn_prefab` or `Assets::load_reflected_resource_json`"),
            Class::Archive => None,
        }
    }
}

/// What the first bytes say.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Found {
    /// A format ponko knows: "a PNG image", "an Ogg sound".
    Format(Class, &'static str),
    /// A text file (not any of the formats above).
    PlainText,
    /// A Git LFS pointer: the file's place-holder in a checkout without LFS.
    LfsPointer,
    /// Nothing known.
    Unknown,
}

impl Sniff {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        let kept = bytes.len().min(HEAD);
        let mut head = [0; HEAD];
        head[..kept].copy_from_slice(&bytes[..kept]);
        Self {
            head,
            kept,
            len: bytes.len(),
        }
    }

    /// The start of the file at `path` (nothing known if it can't be read).
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub(crate) fn of_file(path: &std::path::Path) -> Self {
        use std::io::Read;
        let mut head = [0; HEAD];
        let (kept, len) = std::fs::File::open(path)
            .and_then(|mut f| {
                let len = f.metadata()?.len();
                let mut kept = 0;
                while kept < HEAD {
                    match f.read(&mut head[kept..])? {
                        0 => break,
                        n => kept += n,
                    }
                }
                Ok((kept, len as usize))
            })
            .unwrap_or((0, usize::MAX));
        Self { head, kept, len }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn bytes(&self) -> &[u8] {
        &self.head[..self.kept]
    }

    pub(crate) fn found(&self) -> Found {
        let b = self.bytes();
        let starts = |m: &[u8]| b.starts_with(m);
        let riff = |kind: &[u8]| b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == kind;
        let format = |class, what| Found::Format(class, what);
        if starts(b"\x89PNG\r\n\x1a\n") {
            format(Class::Image, "a PNG image")
        } else if starts(&[0xFF, 0xD8, 0xFF]) {
            format(Class::Image, "a JPEG image")
        } else if starts(b"GIF87a") || starts(b"GIF89a") {
            format(Class::Image, "a GIF image")
        } else if riff(b"WEBP") {
            format(Class::Image, "a WebP image")
        } else if starts(b"BM") && b.len() > 14 && !self.is_text() {
            format(Class::Image, "a BMP image")
        } else if starts(b"qoif") {
            format(Class::Image, "a QOI image")
        } else if starts(&[0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB]) {
            format(Class::Image, "a KTX2 texture")
        } else if starts(b"DDS ") {
            format(Class::Image, "a DDS texture")
        } else if starts(b"OggS") {
            format(Class::Sound, "an Ogg sound")
        } else if riff(b"WAVE") {
            format(Class::Sound, "a WAV sound")
        } else if starts(b"fLaC") {
            format(Class::Sound, "a FLAC sound")
        } else if starts(b"FORM") && b.len() >= 12 && (&b[8..12] == b"AIFF" || &b[8..12] == b"AIFC") {
            format(Class::Sound, "an AIFF sound")
        } else if starts(b"ID3") || (b.len() >= 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0) {
            format(Class::Sound, "an MP3 sound")
        } else if starts(&[0x00, 0x01, 0x00, 0x00]) || starts(b"true") {
            format(Class::Font, "a TrueType font")
        } else if starts(b"OTTO") {
            format(Class::Font, "an OpenType font")
        } else if starts(b"ttcf") {
            format(Class::Font, "a font collection")
        } else if starts(b"wOFF") || starts(b"wOF2") {
            format(Class::Font, "a web font (WOFF)")
        } else if starts(b"glTF") {
            format(Class::Model, "a glTF model (.glb)")
        } else if starts(b"PK\x03\x04") {
            format(Class::Archive, "a zip archive")
        } else if starts(b"version https://git-lfs") {
            Found::LfsPointer
        } else if self.is_text() {
            let text = String::from_utf8_lossy(b);
            if matches!(text.trim_start().chars().next(), Some('{' | '[')) {
                format(Class::Data, "a JSON file")
            } else {
                Found::PlainText
            }
        } else {
            Found::Unknown
        }
    }

    /// Whether what is kept reads as text: UTF-8 without control characters
    /// (a multi-byte character cut at the end of the kept bytes is fine).
    fn is_text(&self) -> bool {
        let b = self.bytes();
        if b.is_empty() {
            return false;
        }
        let text = match std::str::from_utf8(b) {
            Ok(text) => text,
            Err(e) if e.error_len().is_none() && self.len > self.kept => {
                std::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or("")
            }
            Err(_) => return false,
        };
        !text.is_empty() && text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
    }
}

/// The message for a file `what` (a path, or a path with its asset source)
/// that a loader wanting `wanted` couldn't read: what went wrong, what the
/// file looks like instead, and the fix. `detail` is the decoder's own
/// words, or `None`.
///
/// Every message keeps the shape "`what` is not a valid `noun`: ..." so a
/// test or a log search finds it, and tells the cases apart by what follows.
pub(crate) fn not_a(wanted: Class, what: &str, sniff: &Sniff, detail: Option<&str>) -> String {
    let noun = wanted.noun();
    let head = format!("{what} is not a valid {noun}");
    if sniff.is_empty() {
        return format!(
            "{head}: the file is empty (0 bytes); a failed export or copy leaves one, so export or copy it again"
        );
    }
    match sniff.found() {
        Found::LfsPointer => format!(
            "{head}: it is a Git LFS pointer, the small text file a checkout without the real file leaves; \
             run `git lfs pull` to fetch the file itself"
        ),
        Found::Format(class, what_it_is) if class != wanted => {
            let instead = match class.loader() {
                Some(call) => format!("load it with {call} instead"),
                None => "check which file the name should point to".to_string(),
            };
            format!(
                "{head}: its bytes are {what_it_is}, not {a_noun}; {instead}",
                a_noun = article(noun)
            )
        }
        Found::Format(_, what_it_is) => format!(
            "{head}: it starts like {what_it_is} but can't be read (cut off or damaged?); \
             export or copy it again, and open it in an editor to check{}",
            detail.map(|d| format!(": {d}")).unwrap_or_default()
        ),
        Found::PlainText => format!(
            "{head}: it is a text file, not {a_noun}; check that the file name points to the real {noun}",
            a_noun = article(noun)
        ),
        Found::Unknown => {
            let start = if sniff.bytes().is_empty() {
                String::new()
            } else {
                format!(" ({})", hex_head(sniff.bytes()))
            };
            format!(
                "{head}: its first bytes{start} aren't those of any {noun} format ponko reads{}",
                detail.map(|d| format!(": {d}")).unwrap_or_default()
            )
        }
    }
}

/// [`not_a`] for a sound, with the decoder's words `detail` (its own
/// "could not decode the sound" lead taken off) and the formats ponko reads.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub(crate) fn not_a_sound(what: &str, sniff: &Sniff, detail: &str) -> String {
    let detail = detail.strip_prefix("could not decode the sound: ").unwrap_or(detail);
    not_a(
        Class::Sound,
        what,
        sniff,
        Some(&format!("{detail}; ponko reads Ogg Vorbis, MP3, FLAC, WAV and AIFF")),
    )
}

fn article(noun: &str) -> String {
    let a = if noun.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!("{a} {noun}")
}

fn hex_head(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(wanted: Class, bytes: &[u8]) -> String {
        not_a(wanted, "\"x\"", &Sniff::of(bytes), Some("the decoder's words"))
    }

    #[test]
    fn a_sound_given_to_the_image_loader_says_what_it_is_and_what_loads_it() {
        let m = said(Class::Image, b"OggS\0\x02rest");
        assert!(
            m.starts_with("\"x\" is not a valid image: its bytes are an Ogg sound, not an image"),
            "{m}"
        );
        assert!(m.contains("`ctx.audio.load`"), "{m}");
    }

    #[test]
    fn an_empty_file_and_a_lfs_pointer_each_name_their_fix() {
        assert!(said(Class::Sound, b"").contains("is empty (0 bytes)"));
        let lfs = said(
            Class::Image,
            b"version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 12\n",
        );
        assert!(lfs.contains("`git lfs pull`"), "{lfs}");
    }

    #[test]
    fn text_json_a_cut_off_file_and_noise_are_told_apart() {
        assert!(said(Class::Font, b"hello world").contains("it is a text file"));
        assert!(said(Class::Image, b"{\"a\": 1}").contains("its bytes are a JSON file"));
        assert!(said(Class::Image, b"\x89PNG\r\n\x1a\n\0\0").contains("starts like a PNG image but can't be read"));
        let noise = said(Class::Image, &[0xde, 0xad, 0xbe, 0xef, 0, 1, 2, 3, 4]);
        assert!(noise.contains("de ad be ef 00 01 02 03"), "{noise}");
    }

    #[test]
    fn the_formats_ponko_reads_are_recognised() {
        let class = |b: &[u8]| match Sniff::of(b).found() {
            Found::Format(c, _) => Some(c),
            _ => None,
        };
        assert_eq!(class(b"RIFF\0\0\0\0WAVEfmt "), Some(Class::Sound));
        assert_eq!(class(b"RIFF\0\0\0\0WEBPVP8 "), Some(Class::Image));
        assert_eq!(class(b"ID3\x04\0\0"), Some(Class::Sound));
        assert_eq!(class(b"OTTO\0\x0b"), Some(Class::Font));
        assert_eq!(class(b"glTF\x02\0\0\0"), Some(Class::Model));
        assert_eq!(class(b"just words"), None);
    }
}
