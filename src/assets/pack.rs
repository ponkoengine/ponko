//! Asset packs: many asset files in one archive, read through
//! [`PackSource`], and the `assets.pack` a shipped game reads instead of an
//! `assets/` folder.
//!
//! # The format (version 1)
//!
//! Little-endian throughout. `ponko pack` and `ponko package` write it
//! (`crates/ponko_cli/src/pack.rs`); this module reads it.
//!
//! ```text
//! header, 32 bytes:
//!   0  magic           b"PONKOPAK"
//!   8  u32 version     1
//!   12 u32 count       entries in the index
//!   16 u64 index_len   bytes of the index, which follows the header
//!   24 u64 data_len    bytes of data, which follow the index
//! index: `count` entries, sorted by name (byte order), no name twice:
//!   u16 name_len, name (UTF-8, `/`-separated, relative: "sprites/crab.png")
//!   u8  codec          0 stored, 1 deflate (raw, RFC 1951)
//!   u64 offset         from the start of the data
//!   u64 stored_len     bytes in the data
//!   u64 size           bytes once unpacked
//!   u32 crc32          CRC-32 (IEEE) of the unpacked bytes
//! data: the entries' stored bytes; identical files may share them.
//! ```
//!
//! The index comes first so a reader learns what a pack holds from its
//! first bytes: a browser fetches the head of `assets.pack` with an HTTP
//! range request and checks it before the rest arrives. Nothing in a pack
//! depends on when or where it was made (no timestamps, sorted names), so
//! packing the same files twice gives the same bytes.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::assets::AssetSource;
use crate::error::{Error, ErrorKind};

/// The first bytes of every pack.
pub(crate) const MAGIC: &[u8; 8] = b"PONKOPAK";
/// The format version this engine reads.
pub(crate) const VERSION: u32 = 1;
/// Bytes of the header.
pub(crate) const HEADER_LEN: usize = 32;
/// The file name of the pack a game reads by default, where its `assets/`
/// folder would be (next to the program, in a macOS app's `Resources`, or
/// next to the web page).
pub(crate) const DEFAULT_PACK: &str = "assets.pack";

/// How an entry's bytes are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Codec {
    Stored,
    Deflate,
}

/// One file of a pack, as its index describes it.
#[derive(Clone, Debug)]
struct Entry {
    name: Box<str>,
    codec: Codec,
    offset: u64,
    stored_len: u64,
    size: u64,
    crc32: u32,
}

/// What the header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    count: u32,
    index_len: u64,
    data_len: u64,
}

impl Header {
    /// Where the data starts: right after the index.
    fn data_start(&self) -> u64 {
        HEADER_LEN as u64 + self.index_len
    }

    /// The pack's whole length in bytes.
    pub(crate) fn total_len(&self) -> u64 {
        self.data_start() + self.data_len
    }
}

/// Reads the header from a pack's first [`HEADER_LEN`] bytes.
///
/// `Ok(None)` when they aren't a pack at all (another file, such as a web
/// host's "not found" page); an error when they are one this engine can't
/// read.
pub(crate) fn parse_header(bytes: &[u8]) -> Result<Option<Header>, String> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return Ok(None);
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
    if version != VERSION {
        return Err(format!(
            "it is a version {version} pack; this engine reads version {VERSION} (pack it again with this engine's `ponko pack`)"
        ));
    }
    let header = Header {
        count: u32::from_le_bytes(bytes[12..16].try_into().expect("4 bytes")),
        index_len: u64::from_le_bytes(bytes[16..24].try_into().expect("8 bytes")),
        data_len: u64::from_le_bytes(bytes[24..32].try_into().expect("8 bytes")),
    };
    // So `data_start` and `total_len` can't overflow.
    if (HEADER_LEN as u64)
        .checked_add(header.index_len)
        .and_then(|n| n.checked_add(header.data_len))
        .is_none()
    {
        return Err("its header gives sizes past 2^64 bytes".into());
    }
    Ok(Some(header))
}

/// A cursor over the index's bytes.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.0.len() < n {
            return Err("the index ends in the middle of an entry".into());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
}

/// Reads and checks the index (the `header.index_len` bytes after the header).
fn parse_index(header: &Header, index: &[u8]) -> Result<Vec<Entry>, String> {
    let mut cursor = Cursor(index);
    // Each entry takes at least 31 bytes: no count can ask for more memory than the index has.
    let mut entries: Vec<Entry> = Vec::with_capacity((header.count as usize).min(index.len() / 31));
    for _ in 0..header.count {
        let name_len = usize::from(cursor.u16()?);
        let name = std::str::from_utf8(cursor.take(name_len)?).map_err(|_| "a name is not UTF-8".to_string())?;
        let codec = match cursor.take(1)?[0] {
            0 => Codec::Stored,
            1 => Codec::Deflate,
            other => return Err(format!("\"{name}\" uses codec {other}, which this engine doesn't know")),
        };
        let entry = Entry {
            name: name.into(),
            codec,
            offset: cursor.u64()?,
            stored_len: cursor.u64()?,
            size: cursor.u64()?,
            crc32: cursor.u32()?,
        };
        let end = entry.offset.checked_add(entry.stored_len);
        if end.is_none_or(|end| end > header.data_len) {
            return Err(format!("\"{name}\" lies outside the pack's data"));
        }
        if codec == Codec::Stored && entry.stored_len != entry.size {
            return Err(format!("\"{name}\" is stored, but its two sizes differ"));
        }
        if let Some(last) = entries.last()
            && last.name.as_bytes() >= name.as_bytes()
        {
            return Err(format!("the index is not sorted by name at \"{name}\""));
        }
        entries.push(entry);
    }
    if !cursor.0.is_empty() {
        return Err("the index is longer than its entries".into());
    }
    Ok(entries)
}

/// The most deflate expands its input: 258 bytes from each 2-bit code.
const MAX_DEFLATE_RATIO: u64 = 1032;

/// Unpacks an entry's stored bytes and checks them.
fn unpack(entry: &Entry, stored: Vec<u8>) -> std::io::Result<Vec<u8>> {
    let bytes = match entry.codec {
        Codec::Stored => stored,
        Codec::Deflate => {
            // Reserve what the stored bytes can inflate to at most (deflate
            // shrinks 1032:1 at best), not what a damaged index claims.
            let most = (stored.len() as u64).saturating_mul(MAX_DEFLATE_RATIO);
            let mut out = Vec::with_capacity(usize::try_from(entry.size.min(most)).unwrap_or(0).min(1 << 30));
            flate2::read::DeflateDecoder::new(stored.as_slice())
                .take(entry.size.saturating_add(1))
                .read_to_end(&mut out)
                .map_err(|e| invalid(format!("\"{}\" does not inflate: {e}", entry.name)))?;
            out
        }
    };
    if bytes.len() as u64 != entry.size {
        return Err(invalid(format!(
            "\"{}\" unpacks to {} bytes, not the {} its index says",
            entry.name,
            bytes.len(),
            entry.size
        )));
    }
    let mut crc = flate2::Crc::new();
    crc.update(&bytes);
    if crc.sum() != entry.crc32 {
        return Err(invalid(format!(
            "\"{}\" is damaged (its checksum doesn't match)",
            entry.name
        )));
    }
    Ok(bytes)
}

fn invalid(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

/// Where a pack's data is.
enum Data {
    /// A file, read an entry at a time (desktop).
    File { file: Mutex<std::fs::File>, start: u64 },
    /// The whole pack, in memory (the web, embedded packs, tests).
    Memory { bytes: Arc<[u8]>, start: usize },
}

/// An [`AssetSource`](crate::assets::AssetSource) reading a pack file:
/// many asset files in one archive with an index, each stored as it is or
/// compressed, written by `ponko pack` (or `ponko package`).
///
/// A shipped game needn't add one: the engine reads `assets.pack` where the
/// `assets/` folder would be (next to the program, in a macOS app's
/// `Resources`, next to the web page) by itself, after any loose files, so
/// `ponko package` ships a game's assets as one pack and development keeps
/// its loose files and hot reload. Add a `PackSource` for packs of your own,
/// such as downloadable content, or one embedded in the program:
///
/// ```no_run
/// use ponko::assets::PackSource;
/// # use ponko::prelude::*;
/// # fn setup(ctx: &mut Ctx) -> Result<(), ponko::prelude::Error> {
/// ctx.assets.add_source(PackSource::open("dlc/forest.pack")?);
/// let tree = ctx.gfx.load_texture("forest/tree.png"); // from the pack
/// # Ok(()) }
/// ```
///
/// Reading a name reads only that file's bytes (from the file on desktop),
/// unpacks them and checks their CRC-32; a damaged file is an error of kind
/// [`InvalidData`](std::io::ErrorKind::InvalidData), which stops the search
/// for that name. As with every added source, what came from a pack is
/// never hot reloaded.
pub struct PackSource {
    id: String,
    entries: Vec<Entry>,
    data: Data,
}

impl PackSource {
    /// Opens the pack file at `path`, reading its index; its files are read
    /// when they are loaded. Its [`id`](AssetSource::id) is the file's name
    /// (`"forest.pack"`).
    ///
    /// # Errors
    /// [`Io`](ErrorKind::Io) if the file can't be read (including in web
    /// builds, which have no files: use [`from_bytes`](Self::from_bytes)),
    /// [`Decode`](ErrorKind::Decode) if it isn't a pack this engine reads.
    pub fn open(path: impl AsRef<Path>) -> Result<PackSource, Error> {
        let path = path.as_ref();
        let id = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        Self::open_as(path, id)
    }

    fn open_as(path: &Path, id: String) -> Result<PackSource, Error> {
        let shown = path.display().to_string();
        let io =
            |e: std::io::Error| Error::new(ErrorKind::Io, format!("could not read {shown}: {e}")).with_path(&shown);
        let mut file = std::fs::File::open(path).map_err(io)?;
        let mut head = [0u8; HEADER_LEN];
        let read = read_up_to(&mut file, &mut head).map_err(io)?;
        let header = match parse_header(&head[..read]) {
            Ok(Some(header)) => header,
            Ok(None) => return Err(Error::decode(&shown, format!("{shown} is not an asset pack"))),
            Err(e) => return Err(Error::decode(&shown, format!("{shown}: {e}"))),
        };
        let len = file.metadata().map_err(io)?.len();
        if len < header.total_len() {
            return Err(Error::decode(
                &shown,
                format!(
                    "{shown} is cut short: {len} bytes, its header says {}",
                    header.total_len()
                ),
            ));
        }
        let index_len = usize::try_from(header.index_len).map_err(|_| Error::decode(&shown, "index too large"))?;
        let mut index = vec![0u8; index_len];
        file.read_exact(&mut index).map_err(io)?;
        let entries = parse_index(&header, &index).map_err(|e| Error::decode(&shown, format!("{shown}: {e}")))?;
        Ok(PackSource {
            id,
            entries,
            data: Data::File {
                file: Mutex::new(file),
                start: header.data_start(),
            },
        })
    }

    /// A pack held in memory: one embedded with `include_bytes!`, or
    /// downloaded. Its [`id`](AssetSource::id) is `id`.
    ///
    /// # Errors
    /// [`Decode`](ErrorKind::Decode) if `bytes` aren't a whole pack this
    /// engine reads.
    pub fn from_bytes(id: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Result<PackSource, Error> {
        let id = id.into();
        let bytes: Arc<[u8]> = bytes.into();
        let fail = |e: String| Error::decode(&id, format!("asset pack \"{id}\": {e}"));
        let header = match parse_header(&bytes) {
            Ok(Some(header)) => header,
            Ok(None) => return Err(fail("not an asset pack".into())),
            Err(e) => return Err(fail(e)),
        };
        if (bytes.len() as u64) < header.total_len() {
            return Err(fail(format!(
                "cut short: {} bytes, its header says {}",
                bytes.len(),
                header.total_len()
            )));
        }
        let start = usize::try_from(header.data_start()).map_err(|_| fail("too large".into()))?;
        let entries = parse_index(&header, &bytes[HEADER_LEN..start]).map_err(fail)?;
        Ok(PackSource {
            id,
            entries,
            data: Data::Memory { bytes, start },
        })
    }

    /// The names of the files in the pack, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| &*e.name)
    }

    /// How many files the pack holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the pack holds no files.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|e| e.name.as_bytes().cmp(name.as_bytes()))
            .ok()
            .map(|i| &self.entries[i])
    }

    fn stored(&self, entry: &Entry) -> std::io::Result<Vec<u8>> {
        let len = usize::try_from(entry.stored_len).map_err(|_| invalid(format!("\"{}\" is too large", entry.name)))?;
        match &self.data {
            Data::Memory { bytes, start } => {
                let from = start + entry.offset as usize;
                Ok(bytes[from..from + len].to_vec())
            }
            Data::File { file, start } => {
                let mut file = file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                file.seek(SeekFrom::Start(start + entry.offset))?;
                let mut out = vec![0u8; len];
                file.read_exact(&mut out)?;
                Ok(out)
            }
        }
    }
}

impl AssetSource for PackSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn contains(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        let Some(entry) = self.entry(name) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("\"{name}\" is not in asset pack \"{}\"", self.id),
            ));
        };
        unpack(entry, self.stored(entry)?)
    }
}

/// Fills `buf` as far as the reader goes; returns how much it read.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// The packs read after the loose files: the `assets.pack` files found
/// beside the asset folders on desktop, the page's pack on the web.
static DEFAULT_PACKS: OnceLock<Vec<PackSource>> = OnceLock::new();

/// The default packs, found on first use.
pub(crate) fn default_packs() -> &'static [PackSource] {
    DEFAULT_PACKS.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            discover(&crate::assets::asset_roots())
        }
        #[cfg(target_arch = "wasm32")]
        Vec::new()
    })
}

/// Sets the page's pack (web builds), before anything is loaded.
#[cfg(all(target_arch = "wasm32", feature = "window"))]
pub(crate) fn set_web_pack(pack: PackSource) {
    if DEFAULT_PACKS.set(vec![pack]).is_err() {
        crate::web::log_error("ponko: the asset pack arrived after assets were loaded; it is not read");
    }
}

/// Where the default pack of each asset folder would be: `assets.pack`
/// beside it.
pub(crate) fn default_pack_paths(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| root.parent().map(|dir| dir.join(DEFAULT_PACK)))
        .collect();
    paths.dedup();
    paths
}

/// Opens the default packs beside `roots` that exist. One that can't be
/// read is reported and skipped (the game then reports the files it
/// misses).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn discover(roots: &[PathBuf]) -> Vec<PackSource> {
    let mut packs: Vec<PackSource> = Vec::new();
    let mut seen = Vec::new();
    for path in default_pack_paths(roots) {
        if !path.is_file() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.contains(&canonical) {
            continue;
        }
        seen.push(canonical);
        match PackSource::open_as(&path, path.display().to_string()) {
            Ok(pack) => packs.push(pack),
            Err(e) => eprintln!("ponko: skipping asset pack: {e}"),
        }
    }
    packs
}

/// A pack writer for tests: the format above, as `ponko pack` writes it.
#[cfg(test)]
pub(crate) fn write_for_test(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    use std::io::Write;
    let mut files: Vec<_> = files.to_vec();
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut index = Vec::new();
    let mut data = Vec::new();
    for (name, bytes, deflate) in &files {
        let stored = if *deflate {
            let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(bytes).unwrap();
            encoder.finish().unwrap()
        } else {
            bytes.to_vec()
        };
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(name.as_bytes());
        index.push(u8::from(*deflate));
        index.extend_from_slice(&(data.len() as u64).to_le_bytes());
        index.extend_from_slice(&(stored.len() as u64).to_le_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&crc.sum().to_le_bytes());
        data.extend_from_slice(&stored);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        write_for_test(&[
            ("rules.json", br#"{"speed": 30}"#, false),
            ("notes/long.txt", "abc ".repeat(100).as_bytes(), true),
            ("empty.bin", b"", false),
        ])
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ponko-packs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stored_and_deflated_files_read_back_from_memory_and_from_a_file() {
        let bytes = sample();
        let dir = temp("open");
        let path = dir.join("game.pack");
        std::fs::write(&path, &bytes).unwrap();
        let from_file = PackSource::open(&path).unwrap();
        let from_memory = PackSource::from_bytes("mem", bytes).unwrap();
        assert_eq!(from_file.id(), "game.pack");
        for pack in [&from_file, &from_memory] {
            assert_eq!(pack.read("rules.json").unwrap(), br#"{"speed": 30}"#);
            assert_eq!(pack.read("notes/long.txt").unwrap(), "abc ".repeat(100).as_bytes());
            assert_eq!(pack.read("empty.bin").unwrap(), b"");
            assert_eq!(
                pack.names().collect::<Vec<_>>(),
                ["empty.bin", "notes/long.txt", "rules.json"]
            );
            assert_eq!(pack.len(), 3);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_name_the_pack_lacks_is_not_found_so_the_next_source_is_asked() {
        let pack = PackSource::from_bytes("mem", sample()).unwrap();
        let err = pack.read("notes").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            err.to_string().contains("\"notes\" is not in asset pack \"mem\""),
            "{err}"
        );
        assert!(!pack.contains("Rules.json") && pack.contains("rules.json"));
    }

    #[test]
    fn a_damaged_file_fails_its_checksum_as_invalid_data() {
        let mut bytes = sample();
        // The last data byte belongs to the last entry in the data: rules.json.
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let pack = PackSource::from_bytes("mem", bytes).unwrap();
        let err = pack.read("rules.json").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("damaged"), "{err}");
        assert!(pack.read("empty.bin").is_ok());
    }

    #[test]
    fn packs_this_engine_cant_read_are_refused_with_the_reason() {
        let good = sample();
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (b"<!doctype html>".to_vec(), "not an asset pack"),
            (good[..good.len() - 2].to_vec(), "cut short"),
            (
                {
                    let mut b = good.clone();
                    b[8] = 9;
                    b
                },
                "version 9 pack; this engine reads version 1",
            ),
            (
                {
                    let mut b = good.clone();
                    b[12] += 1; // one entry more than the index holds
                    b
                },
                "ends in the middle",
            ),
        ];
        for (bytes, needle) in cases {
            let err = PackSource::from_bytes("bad", bytes).err().expect("refused");
            assert_eq!(err.kind(), ErrorKind::Decode);
            assert!(err.message().contains(needle), "{needle}: {}", err.message());
        }
    }

    #[test]
    fn an_index_out_of_order_or_pointing_outside_the_data_is_refused() {
        // Two entries written in the wrong order.
        let mut bytes = write_for_test(&[("a", b"1", false), ("b", b"2", false)]);
        let first = HEADER_LEN + 2; // the first name's byte
        let second = first + 1 + 1 + 8 + 8 + 8 + 4 + 2;
        bytes.swap(first, second);
        let err = PackSource::from_bytes("x", bytes).err().unwrap();
        assert!(err.message().contains("not sorted"), "{}", err.message());

        let mut bytes = write_for_test(&[("a", b"1", false)]);
        let offset = HEADER_LEN + 2 + 1 + 1;
        bytes[offset] = 5;
        let err = PackSource::from_bytes("x", bytes).err().unwrap();
        assert!(err.message().contains("outside the pack's data"), "{}", err.message());
    }

    #[test]
    fn default_packs_are_found_beside_asset_folders_and_bad_ones_skipped() {
        let dir = temp("discover");
        let (game, other, broken) = (dir.join("game"), dir.join("other"), dir.join("broken"));
        for d in [&game, &other, &broken] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(game.join(DEFAULT_PACK), sample()).unwrap();
        std::fs::write(broken.join(DEFAULT_PACK), b"not a pack").unwrap();
        let roots = vec![
            game.join("assets"),
            other.join("assets"),
            broken.join("assets"),
            game.join("assets"),
        ];
        assert_eq!(
            default_pack_paths(&roots[..2]),
            [game.join(DEFAULT_PACK), other.join(DEFAULT_PACK)]
        );
        let packs = discover(&roots);
        assert_eq!(packs.len(), 1, "the game's pack, once; the broken one skipped");
        assert_eq!(packs[0].id(), game.join(DEFAULT_PACK).display().to_string());
        let _ = std::fs::remove_dir_all(dir);
    }
}
