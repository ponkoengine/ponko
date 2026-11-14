//! `ponko pack`: a folder of asset files into one asset pack, the archive
//! the engine reads as `assets.pack` (or through `PackSource`).
//!
//! The format is the engine's (`src/assets/pack.rs` documents it): a
//! 32-byte header, an index sorted by name, then the files' bytes, each
//! stored as it is or deflated, with its size and CRC-32. [`write`] makes
//! one from files in memory and gives the same bytes for the same files,
//! wherever and whenever it runs; [`read_index`] and [`unpack`] read one
//! back (`ponko pack --list`, and the tests). `ponko package` packs a game's
//! `assets/` with [`pack_files`].
//!
//! The shared test fixture `tests/fixtures/packs/sample.pack` (at the
//! repository root) is what [`write`] makes of [`sample_files`]; the
//! engine's tests read it, so both sides keep to one format.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const USAGE: &str = "\
ponko pack: pack a folder of assets into one asset pack

Usage:
  ponko pack [<folder>] [--out <file>] [--exclude <pattern>]... [--textures]
             [--no-compress]
  ponko pack --list <file>

Packs every file in <folder> (default: assets) and its subfolders, hidden
ones left out, into one file (default: target/assets.pack), named by its
path inside the folder: assets/sprites/crab.png is \"sprites/crab.png\", the
name the game loads. A game reads `assets.pack` placed where its assets/
folder would be (next to the program, in a macOS app's Resources, next to
the web page) when a file isn't there loose; `ponko package` does this for
you. The same files always give the same pack, byte for byte.

  --out <file>          Where to write the pack
  --exclude <pattern>   Leave out the files matching it (a path inside the
                        folder; * matches within a folder name, ** across
                        folders), such as `**/*.psd`. Repeat for more.
  --textures            Also convert the images per <folder>/textures.json,
                        as `ponko textures` does, into .ktx2 files in the
                        pack (beside the images; leave those out with
                        --exclude if the game loads only the .ktx2 names).
                        A .ktx2 already in the folder and newer than its
                        image and the settings is packed as it is.
  --no-compress         Store every file as it is (by default a file is
                        deflated when that makes it at least 10% smaller;
                        PNG, JPEG, Ogg and other compressed formats are
                        stored)
  --list <file>         Print what a pack holds instead";

/// The format version written.
pub const VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"PONKOPAK";
const HEADER_LEN: usize = 32;
/// The file name a game reads by default.
pub const DEFAULT_PACK: &str = "assets.pack";

/// The parsed arguments of `ponko pack`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub folder: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub exclude: Vec<String>,
    pub textures: bool,
    pub no_compress: bool,
    pub list: Option<PathBuf>,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = |flag: &str| {
                args.next()
                    .filter(|v| !v.is_empty())
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value\n\n{USAGE}"))
            };
            match arg.as_str() {
                "--out" => o.out = Some(value("--out")?.into()),
                "--exclude" => o.exclude.push(value("--exclude")?),
                "--textures" => o.textures = true,
                "--no-compress" => o.no_compress = true,
                "--list" => o.list = Some(value("--list")?.into()),
                flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
                path if o.folder.is_none() => o.folder = Some(path.into()),
                extra => return Err(format!("unexpected argument `{extra}`\n\n{USAGE}")),
            }
        }
        if o.list.is_some() && (o.folder.is_some() || o.out.is_some()) {
            return Err("--list reads a pack: give no folder or --out with it".into());
        }
        Ok(o)
    }
}

/// How an entry is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Stored = 0,
    Deflate = 1,
}

/// One entry of a pack's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    pub codec: Codec,
    pub offset: u64,
    pub stored_len: u64,
    pub size: u64,
    pub crc32: u32,
}

/// What [`write`] made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packed {
    pub bytes: Vec<u8>,
    pub files: usize,
    /// Bytes of the files before packing.
    pub unpacked: u64,
    /// Files stored deflated.
    pub deflated: usize,
}

/// Formats whose bytes are compressed already: deflating them again is a
/// waste of packing time.
const COMPRESSED: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "avif", "gif", "ogg", "mp3", "flac", "opus", "zip", "gz", "zst", "basis",
];

/// Whether `name`'s bytes are worth trying to deflate.
fn worth_deflating(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    !COMPRESSED.contains(&ext.as_str())
}

/// Checks a name: relative, `/`-separated, no empty, `.` or `..` parts.
pub fn check_name(name: &str) -> Result<(), String> {
    let bad = name.is_empty()
        || name.len() > usize::from(u16::MAX)
        || name.contains('\\')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if bad {
        Err(format!(
            "\"{name}\" can't be a name in a pack: names are relative paths with `/` between folders"
        ))
    } else {
        Ok(())
    }
}

/// Makes a pack of `files` (name, bytes). Each is deflated when `compress`
/// and that saves at least a tenth; identical files share their bytes.
/// Names are sorted, so the order of `files` doesn't matter.
pub fn write(files: Vec<(String, Vec<u8>)>, compress: bool) -> Result<Packed, String> {
    let mut sorted: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for (name, bytes) in files {
        check_name(&name)?;
        if sorted.insert(name.clone().into_bytes(), bytes).is_some() {
            return Err(format!("two files would be \"{name}\" in the pack"));
        }
    }
    let mut index = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    // Identical stored bytes are kept once: (codec, crc, stored) -> offset.
    let mut shared: BTreeMap<(u8, u32, Vec<u8>), u64> = BTreeMap::new();
    let mut packed = Packed {
        files: sorted.len(),
        ..Packed::default()
    };
    for (name, bytes) in sorted {
        let crc32 = crc32fast::hash(&bytes);
        packed.unpacked += bytes.len() as u64;
        let deflated = (compress && worth_deflating(std::str::from_utf8(&name).unwrap_or_default()))
            .then(|| deflate(&bytes))
            .filter(|d| d.len() * 10 <= bytes.len() * 9);
        let (codec, stored) = match deflated {
            Some(d) => {
                packed.deflated += 1;
                (Codec::Deflate, d)
            }
            None => (Codec::Stored, bytes.clone()),
        };
        let stored_len = stored.len() as u64;
        let offset = *shared.entry((codec as u8, crc32, stored)).or_insert_with_key(|key| {
            let offset = data.len() as u64;
            data.extend_from_slice(&key.2);
            offset
        });
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(&name);
        index.push(codec as u8);
        index.extend_from_slice(&offset.to_le_bytes());
        index.extend_from_slice(&stored_len.to_le_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&crc32.to_le_bytes());
    }
    let count = u32::try_from(packed.files).map_err(|_| "too many files for one pack")?;
    let mut out = Vec::with_capacity(HEADER_LEN + index.len() + data.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    packed.bytes = out;
    Ok(packed)
}

/// Raw deflate at the best level: the same bytes for the same input.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b.try_into().expect("4 bytes"))
}

fn le_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().expect("8 bytes"))
}

/// Reads a pack's index; returns it and where the data starts.
pub fn read_index(pack: &[u8]) -> Result<(Vec<IndexEntry>, usize), String> {
    if pack.len() < HEADER_LEN || &pack[..8] != MAGIC {
        return Err("not an asset pack".into());
    }
    let version = le_u32(&pack[8..12]);
    if version != VERSION {
        return Err(format!("a version {version} pack; this ponko reads version {VERSION}"));
    }
    let count = le_u32(&pack[12..16]);
    let index_len = usize::try_from(le_u64(&pack[16..24])).map_err(|_| "index too large")?;
    let data_len = le_u64(&pack[24..32]);
    let start = HEADER_LEN.checked_add(index_len).ok_or("index too large")?;
    if pack.len() as u64 != start as u64 + data_len {
        return Err(format!(
            "{} bytes, but its header says {}",
            pack.len(),
            start as u64 + data_len
        ));
    }
    let mut at = &pack[HEADER_LEN..start];
    let mut take = |n: usize| -> Result<&[u8], String> {
        if at.len() < n {
            return Err("the index ends in the middle of an entry".to_string());
        }
        let (head, rest) = at.split_at(n);
        at = rest;
        Ok(head)
    };
    let mut entries = Vec::new();
    for _ in 0..count {
        let name_len = usize::from(u16::from_le_bytes(take(2)?.try_into().expect("2 bytes")));
        let name = String::from_utf8(take(name_len)?.to_vec()).map_err(|_| "a name is not UTF-8")?;
        let codec = match take(1)?[0] {
            0 => Codec::Stored,
            1 => Codec::Deflate,
            other => return Err(format!("\"{name}\" uses unknown codec {other}")),
        };
        let entry = IndexEntry {
            name,
            codec,
            offset: le_u64(take(8)?),
            stored_len: le_u64(take(8)?),
            size: le_u64(take(8)?),
            crc32: le_u32(take(4)?),
        };
        if entry
            .offset
            .checked_add(entry.stored_len)
            .is_none_or(|end| end > data_len)
        {
            return Err(format!("\"{}\" lies outside the data", entry.name));
        }
        entries.push(entry);
    }
    if !at.is_empty() {
        return Err("the index is longer than its entries".into());
    }
    Ok((entries, start))
}

/// The bytes of `entry` of `pack`, unpacked and checked.
pub fn unpack(pack: &[u8], data_start: usize, entry: &IndexEntry) -> Result<Vec<u8>, String> {
    let from = data_start + entry.offset as usize;
    let stored = &pack[from..from + entry.stored_len as usize];
    let bytes = match entry.codec {
        Codec::Stored => stored.to_vec(),
        Codec::Deflate => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(stored)
                .read_to_end(&mut out)
                .map_err(|e| format!("\"{}\": {e}", entry.name))?;
            out
        }
    };
    if bytes.len() as u64 != entry.size || crc32fast::hash(&bytes) != entry.crc32 {
        return Err(format!("\"{}\" is damaged", entry.name));
    }
    Ok(bytes)
}

/// Whether `path` matches `pattern`: `*` matches within a folder name,
/// `**` across folders.
pub fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((b'*', rest)) => {
            let (across, rest) = match rest.split_first() {
                Some((b'*', rest)) => (true, rest),
                _ => (false, rest),
            };
            (0..=path.len())
                .take_while(|&i| across || i == 0 || path[i - 1] != b'/')
                .any(|i| glob(rest, &path[i..]))
        }
        Some((c, rest)) => path.first() == Some(c) && glob(rest, &path[1..]),
    }
}

/// The files under `folder` (hidden ones left out), as (name in the pack,
/// path), sorted by name; `skip` is left out (the pack being written, when
/// it is inside).
pub fn folder_files(folder: &Path, skip: Option<&Path>) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(root: &Path, dir: &Path, skip: Option<&Path>, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("can't read {}: {e}", dir.display()))?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, skip, out)?;
            } else if skip.is_none_or(|s| s != path) {
                let relative = path.strip_prefix(root).expect("below the root");
                let name = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((name, path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(folder, folder, skip, &mut out)?;
    out.sort();
    Ok(out)
}

/// What to pack and how.
#[derive(Clone, Debug, Default)]
pub struct Settings<'a> {
    pub exclude: &'a [String],
    pub textures: bool,
    pub compress: bool,
}

/// Packs `files` (name in the pack, path on disk): leaves out the excluded
/// ones, converts the images first with `textures` (the settings files are
/// `textures.json` beside them or in a folder above, up to `root`), reads
/// the rest. Returns the pack and the conversions made, for the report.
pub fn pack_files(
    files: &[(String, PathBuf)],
    root: &Path,
    settings: &Settings,
) -> Result<(Packed, Vec<String>), String> {
    let excluded = |name: &str| settings.exclude.iter().any(|p| glob(p.as_bytes(), name.as_bytes()));
    let kept: Vec<(String, PathBuf)> = files.iter().filter(|(name, _)| !excluded(name)).cloned().collect();
    let mut contents: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let converted = if settings.textures {
        convert_textures(files, root)?
            .into_iter()
            .filter(|(name, _)| !excluded(name))
            .map(|(name, bytes)| {
                contents.insert(name.clone(), bytes);
                name
            })
            .collect()
    } else {
        Vec::new()
    };
    for (name, path) in kept {
        if contents.contains_key(&name) {
            continue; // converted just now: newer than the file
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        contents.insert(name, bytes);
    }
    let packed = write(contents.into_iter().collect(), settings.compress)?;
    Ok((packed, converted))
}

/// The KTX2 textures `--textures` adds (see `textures::convert_for_pack`).
#[cfg(feature = "textures")]
fn convert_textures(files: &[(String, PathBuf)], root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    crate::textures::convert_for_pack(files, root)
}

#[cfg(not(feature = "textures"))]
fn convert_textures(_: &[(String, PathBuf)], _: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    Err(
        "this ponko was built without `ponko textures`, so it can't convert textures: install it with its default features (`cargo install ponko_cli`)"
            .into(),
    )
}

/// A size for people.
pub fn size(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

/// `ponko pack`, relative to `dir`.
pub fn pack_command(options: &Options, dir: &Path) -> Result<String, String> {
    if let Some(list) = &options.list {
        let path = dir.join(list);
        let pack = std::fs::read(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let (entries, start) = read_index(&pack).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lines = Vec::new();
        for entry in &entries {
            unpack(&pack, start, entry).map_err(|e| format!("{}: {e}", path.display()))?;
            let how = match entry.codec {
                Codec::Stored => "stored".to_string(),
                Codec::Deflate => format!("deflated to {}", size(entry.stored_len)),
            };
            lines.push(format!("{}  {} ({how})", entry.name, size(entry.size)));
        }
        lines.push(format!(
            "{} files, {} in a pack of {}; every checksum matches",
            entries.len(),
            size(entries.iter().map(|e| e.size).sum()),
            size(pack.len() as u64)
        ));
        return Ok(lines.join("\n"));
    }
    let folder = dir.join(options.folder.clone().unwrap_or_else(|| "assets".into()));
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let out = dir.join(
        options
            .out
            .clone()
            .unwrap_or_else(|| Path::new("target").join(DEFAULT_PACK)),
    );
    let files = folder_files(&folder, Some(&out))?;
    let settings = Settings {
        exclude: &options.exclude,
        textures: options.textures,
        compress: !options.no_compress,
    };
    let (packed, converted) = pack_files(&files, &folder, &settings)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("can't make {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, &packed.bytes).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    let mut report = Vec::new();
    if !converted.is_empty() {
        report.push(format!(
            "converted {} textures: {}",
            converted.len(),
            converted.join(", ")
        ));
    }
    report.push(format!(
        "packed {} files ({}, {} deflated) into {} ({})",
        packed.files,
        size(packed.unpacked),
        packed.deflated,
        out.display(),
        size(packed.bytes.len() as u64)
    ));
    Ok(report.join("\n"))
}

/// The files of the shared fixture `tests/fixtures/packs/sample.pack`.
#[cfg(test)]
pub fn sample_files() -> Vec<(String, Vec<u8>)> {
    let text = "a line of text that repeats, so deflating it pays off.\n".repeat(40);
    vec![
        ("rules.json".to_string(), br#"{"speed": 30}"#.to_vec()),
        ("notes/long.txt".to_string(), text.into_bytes()),
        ("notes/copy.txt".to_string(), b"the same bytes twice".to_vec()),
        ("notes/same.txt".to_string(), b"the same bytes twice".to_vec()),
        ("empty.bin".to_string(), Vec::new()),
        ("sprites/dot.png".to_string(), DOT_PNG.to_vec()),
    ]
}

/// A 1 x 1 opaque red PNG.
#[cfg(test)]
pub const DOT_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x56, 0xc7, 0x2f,
    0x0d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ponko-pack-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/packs/sample.pack")
    }

    #[test]
    fn options_parse_the_folder_and_flags() {
        let o = Options::parse(&args(&[
            "art",
            "--out",
            "dist/a.pack",
            "--exclude",
            "**/*.psd",
            "--exclude",
            "raw/**",
            "--textures",
            "--no-compress",
        ]))
        .unwrap();
        assert_eq!(o.folder, Some(PathBuf::from("art")));
        assert_eq!(o.out, Some(PathBuf::from("dist/a.pack")));
        assert_eq!(o.exclude, ["**/*.psd", "raw/**"]);
        assert!(o.textures && o.no_compress);
        assert!(
            Options::parse(&args(&["--bogus"]))
                .unwrap_err()
                .contains("unknown option")
        );
        assert!(Options::parse(&args(&["--out"])).unwrap_err().contains("needs a value"));
        assert!(Options::parse(&args(&["a", "--list", "x.pack"])).is_err());
    }

    #[test]
    fn a_pack_reads_back_every_file_byte_for_byte() {
        let files = sample_files();
        let packed = write(files.clone(), true).unwrap();
        let (entries, start) = read_index(&packed.bytes).unwrap();
        assert_eq!(entries.len(), files.len());
        for (name, bytes) in files {
            let entry = entries.iter().find(|e| e.name == name).unwrap();
            assert_eq!(unpack(&packed.bytes, start, entry).unwrap(), bytes, "{name}");
        }
    }

    #[test]
    fn the_index_is_sorted_by_name_whatever_the_order_given() {
        let mut files = sample_files();
        let a = write(files.clone(), true).unwrap();
        files.reverse();
        let b = write(files, true).unwrap();
        assert_eq!(a, b);
        let names: Vec<String> = read_index(&a.bytes).unwrap().0.into_iter().map(|e| e.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn text_is_deflated_and_compressed_formats_and_tiny_files_are_stored() {
        let packed = write(sample_files(), true).unwrap();
        let (entries, _) = read_index(&packed.bytes).unwrap();
        let codec = |name: &str| entries.iter().find(|e| e.name == name).unwrap().codec;
        assert_eq!(codec("notes/long.txt"), Codec::Deflate);
        assert_eq!(codec("sprites/dot.png"), Codec::Stored);
        assert_eq!(codec("empty.bin"), Codec::Stored);
        let stored = write(sample_files(), false).unwrap();
        assert!(
            read_index(&stored.bytes)
                .unwrap()
                .0
                .iter()
                .all(|e| e.codec == Codec::Stored)
        );
        assert!(packed.bytes.len() < stored.bytes.len());
    }

    #[test]
    fn identical_files_share_their_bytes() {
        let packed = write(sample_files(), true).unwrap();
        let (entries, _) = read_index(&packed.bytes).unwrap();
        let at = |name: &str| entries.iter().find(|e| e.name == name).unwrap().offset;
        assert_eq!(at("notes/copy.txt"), at("notes/same.txt"));
    }

    #[test]
    fn bad_names_and_twice_named_files_are_refused() {
        for bad in ["", "/abs.png", "a//b", "../up.png", "a/./b", "win\\dows.png", "dir/"] {
            assert!(write(vec![(bad.into(), vec![1])], true).is_err(), "{bad:?}");
        }
        let err = write(vec![("a".into(), vec![1]), ("a".into(), vec![2])], true).unwrap_err();
        assert!(err.contains("two files"), "{err}");
    }

    #[test]
    fn damage_and_truncation_are_caught() {
        let packed = write(sample_files(), true).unwrap().bytes;
        assert!(
            read_index(&packed[..packed.len() - 1])
                .unwrap_err()
                .contains("header says")
        );
        let mut damaged = packed.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 0xff;
        let (entries, start) = read_index(&damaged).unwrap();
        assert!(entries.iter().any(|e| unpack(&damaged, start, e).is_err()));
        let mut newer = packed;
        newer[8] = 2;
        assert!(read_index(&newer).unwrap_err().contains("version 2"));
    }

    /// The fixture the engine's tests read is what this writer makes; if the
    /// format changes on purpose, write it again with
    /// `PONKO_UPDATE_PACK_FIXTURE=1 cargo test -p ponko_cli pack`.
    #[test]
    fn the_shared_fixture_is_what_the_writer_makes() {
        let packed = write(sample_files(), true).unwrap();
        if std::env::var_os("PONKO_UPDATE_PACK_FIXTURE").is_some() {
            std::fs::create_dir_all(fixture().parent().unwrap()).unwrap();
            std::fs::write(fixture(), &packed.bytes).unwrap();
        }
        assert_eq!(
            std::fs::read(fixture()).expect("tests/fixtures/packs/sample.pack"),
            packed.bytes,
            "the pack format or the writer changed: see this test's doc comment"
        );
    }

    #[test]
    fn the_command_packs_a_folder_leaving_out_hidden_and_excluded_files() {
        let dir = temp("cmd");
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("sprites")).unwrap();
        std::fs::create_dir_all(assets.join(".cache")).unwrap();
        std::fs::write(assets.join("sprites/dot.png"), DOT_PNG).unwrap();
        std::fs::write(assets.join("rules.json"), "{}").unwrap();
        std::fs::write(assets.join("art.psd"), "layers").unwrap();
        std::fs::write(assets.join(".DS_Store"), "x").unwrap();
        std::fs::write(assets.join(".cache/x"), "x").unwrap();
        let options = Options::parse(&args(&["--exclude", "*.psd"])).unwrap();
        let report = pack_command(&options, &dir).unwrap();
        assert!(report.contains("packed 2 files"), "{report}");
        let pack = std::fs::read(dir.join("target/assets.pack")).unwrap();
        let names: Vec<String> = read_index(&pack).unwrap().0.into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["rules.json", "sprites/dot.png"]);
        // Packing again gives the same bytes; --list checks them.
        pack_command(&options, &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("target/assets.pack")).unwrap(), pack);
        let list = Options::parse(&args(&["--list", "target/assets.pack"])).unwrap();
        let listing = pack_command(&list, &dir).unwrap();
        assert!(
            listing.contains("sprites/dot.png") && listing.contains("every checksum matches"),
            "{listing}"
        );
        // A pack written inside the folder isn't packed into itself.
        let inside = Options::parse(&args(&["--out", "assets/assets.pack"])).unwrap();
        pack_command(&inside, &dir).unwrap();
        let again = pack_command(&inside, &dir).unwrap();
        assert!(again.contains("packed 3 files"), "{again}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(feature = "textures")]
    #[test]
    fn textures_are_converted_into_the_pack_per_their_settings() {
        let dir = temp("textures");
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("ui")).unwrap();
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(assets.join("wall.png"), &png).unwrap();
        std::fs::write(assets.join("ui/button.png"), &png).unwrap();
        std::fs::write(assets.join("raw.png"), &png).unwrap();
        std::fs::write(
            assets.join("textures.json"),
            r#"{"default": "color", "textures": {"ui/*": "ui", "raw.png": "skip"}}"#,
        )
        .unwrap();
        let options = Options::parse(&args(&["--textures", "--exclude", "wall.png"])).unwrap();
        let report = pack_command(&options, &dir).unwrap();
        assert!(report.contains("converted 2 textures"), "{report}");
        let pack = std::fs::read(dir.join("target/assets.pack")).unwrap();
        let (entries, start) = read_index(&pack).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "raw.png",
                "textures.json",
                "ui/button.ktx2",
                "ui/button.png",
                "wall.ktx2"
            ],
            "skip leaves raw.png alone; --exclude takes out the image, not its texture"
        );
        let wall = entries.iter().find(|e| e.name == "wall.ktx2").unwrap();
        assert!(unpack(&pack, start, wall).unwrap().starts_with(b"\xabKTX 20\xbb"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn globs_match_within_and_across_folders() {
        assert!(glob(b"*.psd", b"art.psd"));
        assert!(!glob(b"*.psd", b"raw/art.psd"));
        assert!(glob(b"**/*.psd", b"raw/deep/art.psd"));
        assert!(glob(b"raw/**", b"raw/a/b.png"));
    }
}
