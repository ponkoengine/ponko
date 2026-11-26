//! Archives and icons written by the command itself, the same bytes on
//! every OS: zip, tar.gz and macOS's icns. Entries are sorted, owned by
//! root (uid and gid 0) and dated 1980 (zip) or 1970 (tar), so packing the
//! same folder twice gives the same archive whatever the files' dates.

use flate2::Compression;
use flate2::GzBuilder;
use flate2::write::DeflateEncoder;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One file or folder of an archive, below the folder being packed.
#[derive(Debug)]
struct Item {
    /// `/`-separated, starting with the packed folder's name.
    name: String,
    path: PathBuf,
    dir: bool,
    mode: u32,
}

/// Every file and folder of `parent/folder`, sorted. A file is executable
/// (0755) if it is in `executables` (paths below `parent`) or, on Unix, has
/// an execute bit; others are 0644, folders 0755. Symbolic links are refused:
/// a zip made on Windows can't hold them, so packages never contain any.
fn items(parent: &Path, folder: &Path, executables: &BTreeSet<PathBuf>) -> Result<Vec<Item>, String> {
    let mut out = Vec::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(rel) = stack.pop() {
        let path = parent.join(&rel);
        let meta = fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if meta.file_type().is_symlink() {
            return Err(format!(
                "{} is a symbolic link; packages hold only files and folders",
                path.display()
            ));
        }
        if meta.is_dir() {
            out.push(Item {
                name: format!("{name}/"),
                path: path.clone(),
                dir: true,
                mode: 0o755,
            });
            let mut children: Vec<PathBuf> = fs::read_dir(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .map(|e| e.map(|e| rel.join(e.file_name())))
                .collect::<Result<_, _>>()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            children.sort();
            stack.extend(children.into_iter().rev());
        } else {
            let mode = if executables.contains(&rel) || unix_executable(&meta) {
                0o755
            } else {
                0o644
            };
            out.push(Item {
                name,
                path,
                dir: false,
                mode,
            });
        }
    }
    Ok(out)
}

#[cfg(unix)]
fn unix_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn unix_executable(_: &fs::Metadata) -> bool {
    false
}

fn write_file(out: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(out, bytes).map_err(|e| format!("can't write {}: {e}", out.display()))
}

// ---- zip ----------------------------------------------------------------

/// 1980-01-01, the earliest date a zip can hold.
const ZIP_DATE: u16 = (1 << 5) | 1;

/// Packs `parent/folder` into the zip file `out`, the folder at its top.
pub fn zip(parent: &Path, folder: &Path, executables: &BTreeSet<PathBuf>, out: &Path) -> Result<(), String> {
    let bytes = zip_bytes(&items(parent, folder, executables)?)?;
    write_file(out, &bytes)
}

fn zip_bytes(items: &[Item]) -> Result<Vec<u8>, String> {
    let too_big = || "the package is too big for a zip without zip64 (4 GB or 65535 files)".to_string();
    let mut out = Vec::new();
    let mut central = Vec::new();
    for item in items {
        let (data, crc, size, method) = if item.dir {
            (Vec::new(), 0, 0, 0u16)
        } else {
            let raw = fs::read(&item.path).map_err(|e| format!("{}: {e}", item.path.display()))?;
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
            encoder.write_all(&raw).map_err(|e| e.to_string())?;
            let packed = encoder.finish().map_err(|e| e.to_string())?;
            let size = u32::try_from(raw.len()).map_err(|_| too_big())?;
            (packed, crc32fast::hash(&raw), size, 8)
        };
        let offset = u32::try_from(out.len()).map_err(|_| too_big())?;
        let packed = u32::try_from(data.len()).map_err(|_| too_big())?;
        let name = item.name.as_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| format!("{}: name too long", item.name))?;
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
            v.extend_from_slice(&0x0800u16.to_le_bytes()); // names are UTF-8
            v.extend_from_slice(&method.to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // time 00:00
            v.extend_from_slice(&ZIP_DATE.to_le_bytes());
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&packed.to_le_bytes());
            v.extend_from_slice(&size.to_le_bytes());
            v.extend_from_slice(&name_len.to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // no extra field
        };
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name);
        out.extend_from_slice(&data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made by: Unix, 2.0
        common(&mut central);
        central.extend_from_slice(&0u16.to_le_bytes()); // no comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk 0
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        let kind = if item.dir { 0o040000 } else { 0o100000 };
        let dos = if item.dir { 0x10 } else { 0 };
        central.extend_from_slice(&(((kind | item.mode) << 16) | dos).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let count = u16::try_from(items.len()).map_err(|_| too_big())?;
    let start = u32::try_from(out.len()).map_err(|_| too_big())?;
    let length = u32::try_from(central.len()).map_err(|_| too_big())?;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // this disk, the central directory's disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // no comment
    Ok(out)
}

// ---- tar.gz ---------------------------------------------------------------

/// Packs `parent/folder` into the gzip-compressed tar file `out`.
pub fn tar_gz(parent: &Path, folder: &Path, executables: &BTreeSet<PathBuf>, out: &Path) -> Result<(), String> {
    let tar = tar_bytes(&items(parent, folder, executables)?)?;
    // No file name and time 0 in the gzip header either.
    let mut gz = GzBuilder::new().mtime(0).write(Vec::new(), Compression::best());
    gz.write_all(&tar).map_err(|e| e.to_string())?;
    write_file(out, &gz.finish().map_err(|e| e.to_string())?)
}

fn octal(field: &mut [u8], value: u64) -> Result<(), String> {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    if text.len() >= field.len() {
        return Err(format!("{value} does not fit a tar header field"));
    }
    field[..text.len()].copy_from_slice(text.as_bytes());
    field[text.len()] = 0;
    Ok(())
}

/// Splits a name into ustar's `prefix` (155 bytes) and `name` (100 bytes).
fn split_name(name: &str) -> Result<(&str, &str), String> {
    if name.len() <= 100 {
        return Ok(("", name));
    }
    let trimmed = name.trim_end_matches('/');
    for (i, _) in trimmed.match_indices('/') {
        let (prefix, rest) = (&name[..i], &name[i + 1..]);
        if prefix.len() <= 155 && rest.len() <= 100 && !rest.is_empty() {
            return Ok((prefix, rest));
        }
    }
    Err(format!("`{name}` is too long a path for a tar archive"))
}

fn tar_header(name: &str, size: u64, mode: u32, dir: bool) -> Result<[u8; 512], String> {
    let mut h = [0u8; 512];
    let (prefix, short) = split_name(name)?;
    h[..short.len()].copy_from_slice(short.as_bytes());
    octal(&mut h[100..108], mode.into())?;
    octal(&mut h[108..116], 0)?; // uid
    octal(&mut h[116..124], 0)?; // gid
    octal(&mut h[124..136], size)?;
    octal(&mut h[136..148], 0)?; // mtime
    h[156] = if dir { b'5' } else { b'0' };
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[265..269].copy_from_slice(b"root");
    h[297..301].copy_from_slice(b"root");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    // The checksum is taken with its own field as spaces.
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let text = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(text.as_bytes());
    Ok(h)
}

fn tar_bytes(items: &[Item]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for item in items {
        if item.dir {
            out.extend_from_slice(&tar_header(&item.name, 0, item.mode, true)?);
            continue;
        }
        let data = fs::read(&item.path).map_err(|e| format!("{}: {e}", item.path.display()))?;
        out.extend_from_slice(&tar_header(&item.name, data.len() as u64, item.mode, false)?);
        out.extend_from_slice(&data);
        out.resize(out.len().next_multiple_of(512), 0);
    }
    out.extend_from_slice(&[0; 1024]);
    Ok(out)
}

// ---- icons ----------------------------------------------------------------

/// The width and height of a PNG, from its header.
pub fn png_size(png: &[u8]) -> Result<(u32, u32), String> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
        return Err("the icon is not a PNG file".into());
    }
    let word = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    Ok((word(16), word(20)))
}

/// Checks the icon is a square PNG of a size every platform's icon takes.
pub fn check_icon(png: &[u8]) -> Result<u32, String> {
    let (w, h) = png_size(png)?;
    if w != h || ![16, 32, 64, 128, 256, 512, 1024].contains(&w) {
        return Err(format!(
            "the icon is {w} x {h}: use a square PNG of 16, 32, 64, 128, 256, 512 or 1024 pixels (512 or 1024 looks best)"
        ));
    }
    Ok(w)
}

/// A macOS `.icns` holding the PNG as it is (icns takes PNG data directly).
pub fn icns(png: &[u8]) -> Result<Vec<u8>, String> {
    let kind: &[u8; 4] = match check_icon(png)? {
        16 => b"icp4",
        32 => b"icp5",
        64 => b"icp6",
        128 => b"ic07",
        256 => b"ic08",
        512 => b"ic09",
        _ => b"ic10",
    };
    let entry = 8 + png.len() as u32;
    let mut out = Vec::with_capacity(16 + png.len());
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(8 + entry).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(&entry.to_be_bytes());
    out.extend_from_slice(png);
    Ok(out)
}

/// The icon scaled to each of [`IOS_ICONS`](super::layout::IOS_ICONS)'
/// sizes, as PNGs, by their file names. iOS shows icons without an alpha
/// channel (it would show transparent pixels black), so they are made
/// opaque over white.
pub fn ios_icons(png: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    use image::imageops::FilterType;
    check_icon(png)?;
    let source = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| format!("can't read the icon: {e}"))?
        .to_rgba8();
    let mut out = Vec::new();
    for &(name, size) in super::layout::IOS_ICONS {
        let scaled = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
        let opaque = image::RgbImage::from_fn(size, size, |x, y| {
            let [r, g, b, a] = scaled.get_pixel(x, y).0;
            let over = |c: u8| ((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a)) + 127) / 255) as u8;
            image::Rgb([over(r), over(g), over(b)])
        });
        let mut bytes = Vec::new();
        opaque
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| format!("can't write the {size}-pixel icon: {e}"))?;
        out.push((name, bytes));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::{DeflateDecoder, GzDecoder};
    use std::io::Read;

    /// A folder `pkg` with a program, a nested asset and a long path.
    fn fixture(name: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!("ponko-archive-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let long = format!("pkg/assets/{}/{}.png", "d".repeat(90), "f".repeat(60));
        fs::create_dir_all(root.join(&long).parent().unwrap()).unwrap();
        fs::write(root.join("pkg/game"), b"program bytes").unwrap();
        fs::write(root.join("pkg/assets/ü.txt"), "text ".repeat(100)).unwrap();
        fs::write(root.join(&long), b"deep").unwrap();
        (root, long)
    }

    fn le16(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }
    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    /// (name, mode, contents) of every entry, read through the central directory.
    fn read_zip(bytes: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
        let end = bytes.len() - 22;
        assert_eq!(le32(bytes, end), 0x06054b50);
        let (count, mut at) = (le16(bytes, end + 10), le32(bytes, end + 16) as usize);
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(le32(bytes, at), 0x02014b50);
            let (method, packed) = (le16(bytes, at + 10), le32(bytes, at + 20) as usize);
            let (name_len, extra, comment) = (le16(bytes, at + 28), le16(bytes, at + 30), le16(bytes, at + 32));
            let mode = le32(bytes, at + 38) >> 16;
            let local = le32(bytes, at + 42) as usize;
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            let data_at = local + 30 + le16(bytes, local + 26) + le16(bytes, local + 28);
            let raw = &bytes[data_at..data_at + packed];
            let mut data = Vec::new();
            if method == 8 {
                DeflateDecoder::new(raw).read_to_end(&mut data).unwrap();
            }
            assert_eq!(crc32fast::hash(&data), le32(bytes, at + 16), "{name}");
            out.push((name, mode, data));
            at += 46 + name_len + extra + comment;
        }
        out
    }

    #[test]
    fn a_zip_holds_the_folder_with_modes_and_utf8_names() {
        let (root, long) = fixture("zip");
        let exes = BTreeSet::from([PathBuf::from("pkg/game")]);
        let out = root.join("pkg.zip");
        zip(&root, Path::new("pkg"), &exes, &out).unwrap();
        let entries = read_zip(&fs::read(&out).unwrap());
        let get = |n: &str| {
            entries
                .iter()
                .find(|e| e.0 == n)
                .unwrap_or_else(|| panic!("{n} in {entries:?}"))
        };
        assert_eq!(entries[0].0, "pkg/");
        assert_eq!(get("pkg/").1, 0o040755);
        assert_eq!(get("pkg/game").1, 0o100755);
        assert_eq!(get("pkg/game").2, b"program bytes");
        assert_eq!(get("pkg/assets/ü.txt").1, 0o100644);
        assert_eq!(get("pkg/assets/ü.txt").2.len(), 500);
        assert_eq!(get(&long).2, b"deep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn packing_twice_gives_the_same_bytes_whatever_the_file_dates() {
        let (root, _) = fixture("same");
        let exes = BTreeSet::new();
        let (a, b, c, d) = (
            root.join("a.zip"),
            root.join("b.zip"),
            root.join("a.tgz"),
            root.join("b.tgz"),
        );
        zip(&root, Path::new("pkg"), &exes, &a).unwrap();
        tar_gz(&root, Path::new("pkg"), &exes, &c).unwrap();
        // Touch a file: its date must not reach the archive.
        let f = fs::File::options().write(true).open(root.join("pkg/game")).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567_890))
            .unwrap();
        drop(f);
        zip(&root, Path::new("pkg"), &exes, &b).unwrap();
        tar_gz(&root, Path::new("pkg"), &exes, &d).unwrap();
        assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
        assert_eq!(fs::read(&c).unwrap(), fs::read(&d).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_tar_gz_is_owned_by_root_dated_zero_and_keeps_long_paths() {
        let (root, long) = fixture("tar");
        let exes = BTreeSet::from([PathBuf::from("pkg/game")]);
        let out = root.join("pkg.tar.gz");
        tar_gz(&root, Path::new("pkg"), &exes, &out).unwrap();
        let gz = fs::read(&out).unwrap();
        assert_eq!(&gz[4..8], &[0; 4], "gzip time is zero");
        assert_eq!(gz[3] & 0x08, 0, "no file name in the gzip header");
        let mut tar = Vec::new();
        GzDecoder::new(&gz[..]).read_to_end(&mut tar).unwrap();
        let field = |h: &[u8], a: usize, b: usize| {
            let s = String::from_utf8_lossy(&h[a..b]).trim_matches(['\0', ' ']).to_string();
            u64::from_str_radix(&s, 8).unwrap()
        };
        let text = |h: &[u8], a: usize, b: usize| String::from_utf8_lossy(&h[a..b]).trim_end_matches('\0').to_string();
        let mut at = 0;
        let mut seen = Vec::new();
        while tar[at..at + 512].iter().any(|&b| b != 0) {
            let h = &tar[at..at + 512];
            let mut sum: u64 = h.iter().map(|&b| u64::from(b)).sum();
            sum = sum - h[148..156].iter().map(|&b| u64::from(b)).sum::<u64>() + 8 * u64::from(b' ');
            assert_eq!(field(h, 148, 154), sum, "header checksum");
            assert_eq!((field(h, 108, 116), field(h, 116, 124), field(h, 136, 148)), (0, 0, 0));
            assert_eq!((text(h, 265, 297), text(h, 297, 329)), ("root".into(), "root".into()));
            let (prefix, name) = (text(h, 345, 500), text(h, 0, 100));
            let full = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let size = field(h, 124, 136) as usize;
            seen.push((full, field(h, 100, 108), tar[at + 512..at + 512 + size].to_vec()));
            at += 512 + size.next_multiple_of(512);
        }
        let get = |n: &str| {
            seen.iter()
                .find(|e| e.0 == n)
                .unwrap_or_else(|| panic!("{n} in {seen:?}"))
        };
        assert_eq!(get("pkg/game").1, 0o755);
        assert_eq!(get("pkg/game").2, b"program bytes");
        assert_eq!(get("pkg/assets/ü.txt").1, 0o644);
        assert_eq!(get(&long).2, b"deep");
        // The system's tar reads it too, where there is one.
        if let Ok(listed) = std::process::Command::new("tar").arg("-tzf").arg(&out).output()
            && listed.status.success()
        {
            assert!(String::from_utf8_lossy(&listed.stdout).contains(&long));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn symbolic_links_are_refused() {
        #[cfg(unix)]
        {
            let (root, _) = fixture("link");
            std::os::unix::fs::symlink("game", root.join("pkg/link")).unwrap();
            let err = zip(&root, Path::new("pkg"), &BTreeSet::new(), &root.join("x.zip")).unwrap_err();
            assert!(err.contains("symbolic link"), "{err}");
            let _ = fs::remove_dir_all(root);
        }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        p.extend_from_slice(&w.to_be_bytes());
        p.extend_from_slice(&h.to_be_bytes());
        p.extend_from_slice(&[8, 6, 0, 0, 0]);
        p
    }

    #[test]
    fn ios_icons_are_opaque_pngs_of_each_size_ios_looks_for() {
        // A half-transparent red icon.
        let mut source = Vec::new();
        image::RgbaImage::from_pixel(256, 256, image::Rgba([255, 0, 0, 128]))
            .write_to(&mut std::io::Cursor::new(&mut source), image::ImageFormat::Png)
            .unwrap();
        let icons = ios_icons(&source).unwrap();
        let names: Vec<&str> = icons.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "AppIcon60x60@2x.png",
                "AppIcon60x60@3x.png",
                "AppIcon76x76@2x~ipad.png",
                "AppIcon83.5x83.5@2x~ipad.png"
            ]
        );
        for ((_, bytes), size) in icons.iter().zip([120, 180, 152, 167]) {
            let icon = image::load_from_memory(bytes).unwrap();
            assert_eq!((icon.width(), icon.height()), (size, size));
            assert!(!icon.color().has_alpha(), "iOS shows transparency as black");
            // Red at half strength over white.
            let [r, g, b] = icon.to_rgb8().get_pixel(size / 2, size / 2).0;
            assert_eq!(r, 255);
            assert!((126..=128).contains(&g) && g == b, "{g} {b}");
        }
        assert!(ios_icons(&png(100, 100)).is_err(), "not an icon size");
    }

    #[test]
    fn an_icns_wraps_the_png_under_its_size() {
        let p = png(512, 512);
        let icns = icns(&p).unwrap();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize, icns.len());
        assert_eq!(&icns[8..12], b"ic09");
        assert_eq!(&icns[16..], &p[..]);
        assert_eq!(&super::icns(&png(1024, 1024)).unwrap()[8..12], b"ic10");
    }

    #[test]
    fn an_icon_must_be_a_square_png_of_an_icon_size() {
        assert!(check_icon(&png(300, 300)).unwrap_err().contains("300 x 300"));
        assert!(check_icon(&png(256, 128)).is_err());
        assert!(
            check_icon(b"GIF89a not a png at all")
                .unwrap_err()
                .contains("not a PNG")
        );
        assert_eq!(check_icon(&png(256, 256)), Ok(256));
    }
}
