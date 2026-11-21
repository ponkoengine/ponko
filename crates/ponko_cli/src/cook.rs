//! `ponko cook`: a folder of source assets to runtime-ready ones, under the
//! same names, so a game loads them through the same calls.
//!
//! Each file is cooked by its kind (see [`cook_files`]):
//! - images (PNG, JPEG): GPU-compressed KTX2 with mipmaps (BC7, BC5 or BC4
//!   for desktop and the web, Basis Universal UASTC for mobile), through the
//!   encoder of `ponko textures`; those glTF models read for what their
//!   materials use them as, a metallic-roughness image with its occlusion
//!   merged in ([`model_images`]);
//! - glTF and GLB models: meshes reordered for the vertex cache, overdraw
//!   and vertex fetch, normals, texture coordinates and positions quantized
//!   (`KHR_mesh_quantization`), the images they hold as KTX2
//!   (`KHR_texture_basisu`), written as one GLB ([`gltf`]); a `.gltf`'s
//!   `.bin` buffers are left out;
//! - PCM WAV sounds: FLAC, lossless ([`flac`]), short ones too (see
//!   "Cooking assets" in the book for the measurement);
//! - WGSL shaders: checked with the engine's rules for their kind and
//!   validated as the engine composes them, written as they are
//!   ([`shaders`]);
//! - Aseprite files: the JSON Array sheet and PNG Aseprite's export makes,
//!   beside them (`hero.aseprite` -> `hero.json`, `hero.png`; [`aseprite`]),
//!   the PNG then cooked as any image;
//! - anything else: copied.
//!
//! The engine finds a texture's, model's and sound's format from its first
//! bytes, so `bricks.png` holding KTX2 blocks loads with
//! `load_texture("bricks.png")`. Results are cached by content: the key is a
//! SHA-256 of the cooker's version, the platform, the asset's settings and
//! its bytes, so a second cook of unchanged files reads the cache, and the
//! same input always gives the same bytes.

#![deny(missing_docs)]

#[cfg(feature = "aseprite")]
pub mod aseprite;
pub mod flac;
#[cfg(feature = "meshes")]
pub mod gltf;
pub mod model_images;
#[cfg(feature = "shaders")]
pub mod shaders;

use crate::pack::glob;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// `ponko cook --help`.
pub const USAGE: &str = "\
ponko cook: turn source assets into runtime-ready ones

Usage:
  ponko cook [<folder>] [--out <folder> | --pack <file>] [--platform <name>]
             [--cache <folder> | --no-cache]

Cooks every file in <folder> (default: assets) into --out (default:
target/cooked/<platform>), under the same names, so the game loads the
cooked files with the same calls (`load_texture(\"bricks.png\")`):
  images (PNG, JPEG)  GPU-compressed KTX2 with mipmaps (needs the engine's
                      `compressed-textures` feature to load); a glTF
                      model's image files for their use, its occlusion
                      merged into its metallic-roughness file
  glTF, GLB           meshes optimized (vertex cache, overdraw, vertex
                      fetch), normals, UVs and positions (within 0.1 mm)
                      quantized, the images it holds as KTX2, one GLB
                      file (a .gltf's .bin buffers are left out)
  WAV (PCM)           FLAC: lossless, about half the size
  WGSL                checked with the engine's rules for its kind and
                      validated as the engine compiles it; kept as it is
  Aseprite            hero.aseprite -> hero.json and hero.png, the JSON
                      Array sheet load_aseprite reads (no export step)
  anything else       copied
Results are cached by content (--cache, default target/cook-cache): a second
cook of unchanged files reads them back. The same input always gives the
same bytes. Games run their source assets while you work; `ponko package`
cooks into the game's asset pack.

  --out <folder>     Write the cooked files here (emptied first; it must be
                     empty or a folder ponko cook made)
  --pack <file>      Write one asset pack of them instead (as `ponko pack`)
  --platform <name>  desktop (default) or web: BC7/BC5/BC4 textures;
                     mobile: Basis Universal UASTC, transcoded at load to
                     ASTC or ETC2 (needs the engine's `basis-universal`)
  --cache <folder>   Where cooked results are kept
  --no-cache         Cook everything again and keep nothing

Settings: <folder>/cook.json (optional) gives assets their own settings,
by path or pattern (* within a folder name, ** across folders; an exact
path wins, then the longest pattern):
  {
    \"assets\": {
      \"ui/**\": \"keep\",
      \"raw/**\": \"skip\",
      \"*_normal.png\": { \"texture\": \"normal\" },
      \"heights.png\": { \"texture\": \"keep\" },
      \"music/*.wav\": { \"audio\": \"keep\" },
      \"props/*.glb\": { \"quantize\": false },
      \"sprites/hero.aseprite\": { \"layers\": true },
      \"fx/glow.wgsl\": { \"shader\": \"effect\" }
    }
  }
  \"keep\"     copy the file as it is;  \"skip\": leave it out
  texture    color (default), linear, normal, mask or keep; with
             \"basis\": true|false and \"mipmaps\": true|false
  audio      flac (default) or keep
  optimize   reorder meshes (default true);  quantize: (default true)
  layers     an Aseprite file's frames split by layer (default false)
  shader     material, sprite, sky, effect, compute or module (default:
             found from the hooks it defines), or keep (not validated)
Image files a glTF file uses are compressed for that use (color, normal
map, or metallic-roughness with the occlusion merged in); 16-bit images and
images whose sides aren't multiples of 4 are kept: the engine reads those
as RGBA8 anyway. A textures.json (see `ponko textures`) gives image kinds
too.";

/// The cooker's version, part of every cache key: bump it whenever a cooked
/// file's bytes would change for the same input and settings.
pub const VERSION: u32 = 4;

/// The settings file's name.
pub const CONFIG: &str = "cook.json";

/// Where a cooked file runs, which decides its texture format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    /// Desktop GPUs: BC7, BC5 and BC4 textures.
    #[default]
    Desktop,
    /// Browsers: BC blocks as on the desktop (a GPU without them gets RGBA8,
    /// decoded at load).
    Web,
    /// Phones and tablets: Basis Universal UASTC, transcoded at load to
    /// ASTC 4x4 or ETC2.
    Mobile,
}

impl Platform {
    /// Parses `desktop`, `web` or `mobile`.
    pub fn parse(name: &str) -> Result<Platform, String> {
        match name {
            "desktop" => Ok(Platform::Desktop),
            "web" => Ok(Platform::Web),
            "mobile" => Ok(Platform::Mobile),
            other => Err(format!("unknown platform `{other}` (use desktop, web or mobile)")),
        }
    }

    /// Its name, as [`parse`](Self::parse) reads it.
    pub fn name(self) -> &'static str {
        match self {
            Platform::Desktop => "desktop",
            Platform::Web => "web",
            Platform::Mobile => "mobile",
        }
    }
}

/// What the game that loads the cooked files can read, from the engine
/// features it is built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reads {
    /// KTX2 textures (`compressed-textures`): without it images are kept.
    pub ktx2: bool,
    /// Basis Universal UASTC (`basis-universal`, not on the web): without it
    /// mobile textures are BC blocks too.
    pub uastc: bool,
}

impl Reads {
    /// Everything: what `ponko cook` assumes (the game's features are its
    /// author's to match).
    pub const ALL: Reads = Reads {
        ktx2: true,
        uastc: true,
    };

    /// What a game built with the engine features `features` reads.
    pub fn from_features<S: AsRef<str>>(features: &[S]) -> Reads {
        let has = |name: &str| features.iter().any(|f| f.as_ref() == name);
        Reads {
            ktx2: has("compressed-textures") || has("basis-universal"),
            uastc: has("basis-universal"),
        }
    }
}

/// How one cook runs.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The platform the files are for.
    pub platform: Platform,
    /// What the game reads.
    pub reads: Reads,
    /// The cache folder, or `None` to cook everything and keep nothing.
    pub cache: Option<PathBuf>,
}

/// The parsed arguments of `ponko cook`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The source folder (default `assets`).
    pub folder: Option<PathBuf>,
    /// The output folder.
    pub out: Option<PathBuf>,
    /// An asset pack to write instead of a folder.
    pub pack: Option<PathBuf>,
    /// The platform.
    pub platform: Platform,
    /// The cache folder.
    pub cache: Option<PathBuf>,
    /// Cook without the cache.
    pub no_cache: bool,
}

impl Options {
    /// Parses `ponko cook`'s arguments.
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
                "--pack" => o.pack = Some(value("--pack")?.into()),
                "--platform" => o.platform = Platform::parse(&value("--platform")?)?,
                "--cache" => o.cache = Some(value("--cache")?.into()),
                "--no-cache" => o.no_cache = true,
                flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
                path if o.folder.is_none() => o.folder = Some(path.into()),
                extra => return Err(format!("unexpected argument `{extra}`\n\n{USAGE}")),
            }
        }
        if o.out.is_some() && o.pack.is_some() {
            return Err("give --out or --pack, not both".into());
        }
        if o.cache.is_some() && o.no_cache {
            return Err("give --cache or --no-cache, not both".into());
        }
        Ok(o)
    }
}

/// A texture's setting in `cook.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKind {
    /// Colors (sRGB).
    Color,
    /// Data that isn't a color.
    Linear,
    /// A normal map.
    Normal,
    /// One channel.
    Mask,
    /// Not converted.
    Keep,
}

/// A shader's kind: which engine prelude it is compiled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShaderKind {
    /// A material (`vertex` and `surface` hooks).
    Material,
    /// A sprite shader (`fragment` hook).
    Sprite,
    /// A sky shader (`sky` hook).
    Sky,
    /// A screen effect (`effect` hook).
    Effect,
    /// A compute pass (`compute` or `update` hook).
    Compute,
    /// A whole module with its own entry points.
    Module,
    /// Not validated.
    Keep,
}

impl ShaderKind {
    fn parse(name: &str) -> Result<ShaderKind, String> {
        Ok(match name {
            "material" => ShaderKind::Material,
            "sprite" => ShaderKind::Sprite,
            "sky" => ShaderKind::Sky,
            "effect" => ShaderKind::Effect,
            "compute" => ShaderKind::Compute,
            "module" => ShaderKind::Module,
            "keep" => ShaderKind::Keep,
            other => {
                return Err(format!(
                    "unknown shader kind \"{other}\" (use material, sprite, sky, effect, compute, module or keep)"
                ));
            }
        })
    }

    /// Its name in `cook.json` and in messages.
    #[cfg_attr(not(feature = "shaders"), allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            ShaderKind::Material => "material",
            ShaderKind::Sprite => "sprite",
            ShaderKind::Sky => "sky",
            ShaderKind::Effect => "effect",
            ShaderKind::Compute => "compute",
            ShaderKind::Module => "module",
            ShaderKind::Keep => "keep",
        }
    }
}

/// One asset's settings: what `cook.json` says for it, `None` where it
/// says nothing (the defaults apply).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// `"keep"`: copy as it is.
    pub keep: bool,
    /// `"skip"`: leave it out.
    pub skip: bool,
    /// A texture's kind.
    pub texture: Option<TextureKind>,
    /// Basis Universal UASTC instead of BC blocks (default: on mobile).
    pub basis: Option<bool>,
    /// A full mip chain (default true).
    pub mipmaps: Option<bool>,
    /// `Some(false)`: a sound is kept as it is.
    pub flac: Option<bool>,
    /// Reorder meshes (default true).
    pub optimize: Option<bool>,
    /// Quantize normals and texture coordinates (default true).
    pub quantize: Option<bool>,
    /// A shader's kind.
    pub shader: Option<ShaderKind>,
    /// An Aseprite file's frames split by layer.
    pub layers: Option<bool>,
}

impl Rule {
    fn parse(value: &serde_json::Value) -> Result<Rule, String> {
        if let Some(word) = value.as_str() {
            return match word {
                "keep" => Ok(Rule {
                    keep: true,
                    ..Rule::default()
                }),
                "skip" => Ok(Rule {
                    skip: true,
                    ..Rule::default()
                }),
                other => Err(format!("expected \"keep\", \"skip\" or an object, got \"{other}\"")),
            };
        }
        let object = value.as_object().ok_or("expected \"keep\", \"skip\" or an object")?;
        let mut rule = Rule::default();
        let flag = |v: &serde_json::Value, key: &str| v.as_bool().ok_or(format!("\"{key}\" is true or false"));
        for (key, v) in object {
            match key.as_str() {
                "texture" => {
                    rule.texture = Some(match v.as_str().unwrap_or_default() {
                        "color" => TextureKind::Color,
                        "linear" => TextureKind::Linear,
                        "normal" => TextureKind::Normal,
                        "mask" => TextureKind::Mask,
                        "keep" => TextureKind::Keep,
                        _ => return Err("\"texture\" is color, linear, normal, mask or keep".into()),
                    });
                }
                "basis" => rule.basis = Some(flag(v, key)?),
                "mipmaps" => rule.mipmaps = Some(flag(v, key)?),
                "audio" => {
                    rule.flac = Some(match v.as_str().unwrap_or_default() {
                        "flac" => true,
                        "keep" => false,
                        _ => return Err("\"audio\" is flac or keep".into()),
                    });
                }
                "optimize" => rule.optimize = Some(flag(v, key)?),
                "quantize" => rule.quantize = Some(flag(v, key)?),
                "shader" => rule.shader = Some(ShaderKind::parse(v.as_str().unwrap_or_default())?),
                "layers" => rule.layers = Some(flag(v, key)?),
                other => {
                    return Err(format!(
                        "unknown setting \"{other}\" (the settings are texture, basis, mipmaps, audio, optimize, quantize, shader and layers)"
                    ));
                }
            }
        }
        Ok(rule)
    }
}

/// `cook.json`: the rules by path or pattern.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// (path or pattern, rule), in the file's order.
    pub rules: Vec<(String, Rule)>,
}

impl Config {
    /// Parses a `cook.json`.
    pub fn parse(text: &str) -> Result<Config, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{CONFIG}: {e}"))?;
        let object = json.as_object().ok_or(format!("{CONFIG}: expected an object"))?;
        let mut config = Config::default();
        for (key, value) in object {
            match key.as_str() {
                "assets" => {
                    let rules = value
                        .as_object()
                        .ok_or(format!("{CONFIG}: \"assets\" must be an object"))?;
                    for (path, value) in rules {
                        let rule = Rule::parse(value).map_err(|e| format!("{CONFIG}: \"{path}\": {e}"))?;
                        config.rules.push((path.clone(), rule));
                    }
                }
                other => return Err(format!("{CONFIG}: unknown key \"{other}\" (the key is \"assets\")")),
            }
        }
        Ok(config)
    }

    /// The rule for `path` (relative, `/`-separated): an exact path, else
    /// the longest matching pattern, else the defaults.
    pub fn rule_for(&self, path: &str) -> Rule {
        if let Some((_, rule)) = self.rules.iter().find(|(p, _)| p == path) {
            return *rule;
        }
        self.rules
            .iter()
            .filter(|(pattern, _)| pattern.contains('*') && glob(pattern.as_bytes(), path.as_bytes()))
            .max_by_key(|(pattern, _)| pattern.len())
            .map_or_else(Rule::default, |(_, rule)| *rule)
    }
}

/// How a file was cooked, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// An image.
    Texture,
    /// A glTF or GLB model.
    Mesh,
    /// A sound.
    Sound,
    /// A shader.
    Shader,
    /// Anything else.
    Other,
}

impl Kind {
    fn of(name: &str) -> Kind {
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" => Kind::Texture,
            "gltf" | "glb" => Kind::Mesh,
            "wav" => Kind::Sound,
            "wgsl" => Kind::Shader,
            _ => Kind::Other,
        }
    }
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Cooked now.
    Cooked,
    /// Read from the cache.
    Cached,
    /// Copied as it is; the reason, when it is a cookable kind.
    Kept(Option<String>),
    /// Left out (`"skip"`).
    Skipped,
    /// Left out: a `.gltf` file's buffer, whose bytes are inside the GLB
    /// every model that reads it was cooked into; the first such model.
    Embedded(String),
    /// Made into other files (an Aseprite file's sheet and JSON), which
    /// have entries of their own.
    #[cfg_attr(not(feature = "aseprite"), allow(dead_code))]
    Converted(Vec<String>),
}

/// One file of a cook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name.
    pub name: String,
    /// Its kind.
    pub kind: Kind,
    /// What happened.
    pub outcome: Outcome,
    /// Bytes before.
    pub source_bytes: u64,
    /// Bytes after (0 when skipped).
    pub cooked_bytes: u64,
    /// For a texture: video memory before (RGBA8 with mipmaps) and after.
    pub gpu: Option<(u64, u64)>,
    /// What was done, for the report.
    pub note: String,
}

/// What [`cook_files`] made.
#[derive(Clone, Debug, Default)]
pub struct Cooked {
    /// The output files: (name, bytes), sorted by name.
    pub files: Vec<(String, Vec<u8>)>,
    /// One entry per source file, sorted by name.
    pub entries: Vec<Entry>,
    /// Seconds the cook took.
    pub seconds: f64,
}

impl Cooked {
    /// Files cooked now and read from the cache.
    pub fn counts(&self) -> (usize, usize) {
        let count = |o: &Outcome| self.entries.iter().filter(|e| e.outcome == *o).count();
        let converted = self
            .entries
            .iter()
            .filter(|e| matches!(e.outcome, Outcome::Converted(_)))
            .count();
        (count(&Outcome::Cooked) + converted, count(&Outcome::Cached))
    }

    /// One line on what the cook did.
    pub fn summary(&self) -> String {
        let (cooked, cached) = self.counts();
        let kept = self
            .entries
            .iter()
            .filter(|e| matches!(e.outcome, Outcome::Kept(_)))
            .count();
        let skipped = self
            .entries
            .iter()
            .filter(|e| matches!(e.outcome, Outcome::Skipped | Outcome::Embedded(_)))
            .count();
        let before: u64 = self.entries.iter().map(|e| e.source_bytes).sum();
        let after: u64 = self.entries.iter().map(|e| e.cooked_bytes).sum();
        let mut line = format!(
            "cooked {cooked} files, {cached} from the cache, {kept} copied, {skipped} left out: {} -> {} in {:.2} s",
            crate::pack::size(before),
            crate::pack::size(after),
            self.seconds
        );
        let gpu: Vec<(u64, u64)> = self.entries.iter().filter_map(|e| e.gpu).collect();
        if !gpu.is_empty() {
            line.push_str(&format!(
                "; textures: {} of video memory instead of {}",
                crate::pack::size(gpu.iter().map(|g| g.1).sum()),
                crate::pack::size(gpu.iter().map(|g| g.0).sum())
            ));
        }
        line
    }

    /// One line per file (kept files only when there is a reason), then the
    /// summary.
    pub fn report(&self) -> String {
        let mut lines: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| {
                let how = match &e.outcome {
                    Outcome::Cooked => "cooked",
                    Outcome::Cached => "cached",
                    Outcome::Kept(Some(why)) => return Some(format!("{}: kept, {why}", e.name)),
                    Outcome::Embedded(model) => return Some(format!("{}: left out, inside {model}", e.name)),
                    Outcome::Converted(files) => {
                        return Some(format!("{}: made into {}, {}", e.name, files.join(" and "), e.note));
                    }
                    Outcome::Kept(None) | Outcome::Skipped => return None,
                };
                Some(format!(
                    "{}: {how}, {} -> {}{}{}",
                    e.name,
                    crate::pack::size(e.source_bytes),
                    crate::pack::size(e.cooked_bytes),
                    if e.note.is_empty() { "" } else { ", " },
                    e.note
                ))
            })
            .collect();
        lines.push(self.summary());
        lines.join("\n")
    }
}

/// The content-addressed cache: one file per key.
struct Cache {
    dir: PathBuf,
}

/// A cached result: the cooked bytes, and the note for the report.
fn encode_entry(bytes: &[u8], note: &str, gpu: Option<(u64, u64)>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + note.len() + 28);
    out.extend_from_slice(b"PKCOOK1\0");
    let (a, b) = gpu.unwrap_or((u64::MAX, u64::MAX));
    out.extend_from_slice(&a.to_le_bytes());
    out.extend_from_slice(&b.to_le_bytes());
    out.extend_from_slice(&(note.len() as u32).to_le_bytes());
    out.extend_from_slice(note.as_bytes());
    out.extend_from_slice(bytes);
    out
}

/// A cooked file's bytes, its report note and its texture memory.
type Result3 = (Vec<u8>, String, Option<(u64, u64)>);

fn decode_entry(data: &[u8]) -> Option<Result3> {
    let rest = data.strip_prefix(b"PKCOOK1\0")?;
    let a = u64::from_le_bytes(rest.get(0..8)?.try_into().ok()?);
    let b = u64::from_le_bytes(rest.get(8..16)?.try_into().ok()?);
    let len = u32::from_le_bytes(rest.get(16..20)?.try_into().ok()?) as usize;
    let note = String::from_utf8(rest.get(20..20 + len)?.to_vec()).ok()?;
    let gpu = (a != u64::MAX).then_some((a, b));
    Some((rest[20 + len..].to_vec(), note, gpu))
}

impl Cache {
    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(&key[..2]).join(key)
    }

    fn get(&self, key: &str) -> Option<Result3> {
        decode_entry(&std::fs::read(self.path(key)).ok()?)
    }

    /// Stores an entry; a cache that can't be written is only slower.
    fn put(&self, key: &str, entry: &Result3) {
        let path = self.path(key);
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        // Written beside, then renamed: a reader never sees half a file.
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{key}.{}.{id}", std::process::id()));
        if std::fs::write(&temp, encode_entry(&entry.0, &entry.1, entry.2)).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
        let _ = std::fs::remove_file(&temp);
    }
}

/// The cache key of an asset: the cooker version, how it is cooked, its
/// bytes and the bytes it depends on.
fn key(kind: Kind, how: &str, bytes: &[u8], deps: &[(String, Vec<u8>)]) -> String {
    let mut h = Sha256::new();
    h.update(b"ponko cook\0");
    h.update(VERSION.to_le_bytes());
    h.update(format!("{kind:?}\0{how}\0").as_bytes());
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
    for (name, dep) in deps {
        h.update(name.as_bytes());
        h.update([0]);
        h.update((dep.len() as u64).to_le_bytes());
        h.update(dep);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The files (relative to the cooked folder) glTF file `name` reads its
/// buffers from, by URI.
fn buffer_files(name: &str, bytes: &[u8]) -> Vec<String> {
    let Some(json) = gltf_json(bytes) else {
        return Vec::new();
    };
    let folder = name.rfind('/').map_or("", |i| &name[..=i]);
    json["buffers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["uri"].as_str().filter(|u| !u.starts_with("data:")))
        .map(|uri| normalize(&format!("{folder}{}", percent_decode(uri))))
        .collect()
}

/// The buffer files no output needs: those every model that reads them
/// (and isn't left out) was cooked into one self-contained GLB, unless a
/// rule keeps them. Each with the first such model.
fn embedded_buffers(jobs: &[Job], slots: &[Slot]) -> BTreeMap<String, String> {
    let mut readers: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for (job, slot) in jobs.iter().zip(slots) {
        if Kind::of(job.name) != Kind::Mesh {
            continue;
        }
        let still_external = match slot {
            Some(Ok((Outcome::Cooked | Outcome::Cached, Some((out, _, _))))) => Some(buffer_files(job.name, out)),
            Some(Ok((Outcome::Skipped, _))) => continue,
            _ => None,
        };
        for buffer in buffer_files(job.name, job.bytes) {
            let cooked = still_external.as_ref().is_some_and(|paths| !paths.contains(&buffer));
            let entry = readers.entry(buffer).or_insert_with(|| (job.name.to_string(), true));
            entry.1 &= cooked;
        }
    }
    let kept_by_rule: BTreeSet<&str> = jobs.iter().filter(|j| j.rule.keep).map(|j| j.name).collect();
    readers
        .into_iter()
        .filter(|(buffer, (_, all_cooked))| *all_cooked && !kept_by_rule.contains(buffer.as_str()))
        .map(|(buffer, (model, _))| (buffer, model))
        .collect()
}

/// A glTF or GLB file's JSON.
pub(crate) fn gltf_json(bytes: &[u8]) -> Option<serde_json::Value> {
    let json = if bytes.starts_with(b"glTF") {
        let len = u32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?) as usize;
        if bytes.get(16..20)? != b"JSON" {
            return None;
        }
        bytes.get(20..20 + len)?
    } else {
        bytes
    };
    serde_json::from_slice(json).ok()
}

/// `a/./b/../c` to `a/c`.
pub(crate) fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// `%20` and friends in URIs.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One asset's job: its name, bytes, rule and texture kind from a
/// `textures.json`.
struct Job<'a> {
    name: &'a str,
    bytes: &'a [u8],
    rule: Rule,
    texture_json: Option<TextureKind>,
}

/// One job's result, once it ran.
type Slot = Option<Result<(Outcome, Option<Result3>), String>>;

/// The work for one file: `Ok(None)` keeps it, with the reason.
type Work = Result<Result<Result3, Option<String>>, String>;

/// Cooks one file, or says why it stays as it is.
fn cook_one(
    job: &Job,
    all: &BTreeMap<String, Vec<u8>>,
    model_plans: &BTreeMap<String, model_images::Plan>,
    settings: &Settings,
) -> Work {
    let (name, bytes, rule) = (job.name, job.bytes, job.rule);
    match Kind::of(name) {
        Kind::Texture => {
            let kind = rule.texture.or(job.texture_json).unwrap_or(TextureKind::Color);
            if kind == TextureKind::Keep {
                return Ok(Err(None));
            }
            let plan = model_plans.get(name);
            if let Some(model_images::Plan::Keep(why)) = plan {
                return Ok(Err(Some(why.clone())));
            }
            if !settings.reads.ktx2 {
                return Ok(Err(Some(
                    "the game reads no KTX2 (`compressed-textures` is off)".into(),
                )));
            }
            match plan {
                // A model's image: compressed for what the model uses it as.
                Some(model_images::Plan::Encode(kind)) => texture(bytes, *kind, rule, settings),
                Some(model_images::Plan::Orm { metal_rough, occlusion }) => orm_texture(
                    name,
                    bytes,
                    (metal_rough.as_deref(), occlusion.as_deref()),
                    all,
                    rule,
                    settings,
                ),
                _ => texture(bytes, kind, rule, settings),
            }
            .map(|r| r.map_err(Some))
        }
        Kind::Mesh => {
            #[cfg(feature = "meshes")]
            {
                let deps = gltf::dependencies(name, bytes, all)?;
                let options = gltf::Options {
                    optimize: rule.optimize.unwrap_or(true),
                    quantize: rule.quantize.unwrap_or(true),
                    images: model_images(rule, settings),
                };
                if !options.optimize && !options.quantize && options.images.is_none() {
                    return Ok(Err(None));
                }
                gltf::cook(name, bytes, &deps, options).map(|r| r.map_err(Some))
            }
            #[cfg(not(feature = "meshes"))]
            {
                let _ = all;
                Ok(Err(Some("this ponko was built without the `meshes` feature".into())))
            }
        }
        Kind::Sound => {
            if rule.flac == Some(false) {
                return Ok(Err(None));
            }
            match flac::wav_to_flac(bytes).map_err(|e| format!("{name}: {e}"))? {
                Some(out) => Ok(Ok((out, "FLAC".into(), None))),
                None => Ok(Err(Some("not 8, 16 or 24-bit PCM (FLAC takes only those)".into()))),
            }
        }
        Kind::Shader => {
            if rule.shader == Some(ShaderKind::Keep) {
                return Ok(Err(None));
            }
            #[cfg(feature = "shaders")]
            {
                let text = std::str::from_utf8(bytes).map_err(|_| format!("{name}: a shader is not UTF-8 text"))?;
                let kind = shaders::validate(name, text, rule.shader)?;
                Ok(Ok((bytes.to_vec(), format!("valid {} shader", kind.name()), None)))
            }
            #[cfg(not(feature = "shaders"))]
            Ok(Err(Some("this ponko was built without the `shaders` feature".into())))
        }
        Kind::Other => Ok(Err(None)),
    }
}

/// How a model's own images are compressed: as images are (`"texture":
/// "keep"` on the model keeps them, `"basis"` picks UASTC), when the game
/// reads KTX2.
#[cfg(feature = "meshes")]
fn model_images(rule: Rule, settings: &Settings) -> Option<gltf::Images> {
    (settings.reads.ktx2 && rule.texture != Some(TextureKind::Keep)).then(|| gltf::Images {
        basis: rule.basis.unwrap_or(settings.platform == Platform::Mobile)
            && settings.reads.uastc
            && settings.platform != Platform::Web,
    })
}

/// An image to a KTX2 texture per `kind` and `rule`.
#[cfg(feature = "textures")]
fn texture(
    bytes: &[u8],
    kind: TextureKind,
    rule: Rule,
    settings: &Settings,
) -> Result<Result<Result3, String>, String> {
    let (rgba, width, height) = match rgba8(bytes)? {
        Ok(image) => image,
        Err(why) => return Ok(Err(why)),
    };
    encode_texture(&rgba, (width, height), kind, rule, settings, &[]).map(Ok)
}

/// An image file as RGBA8 with its size; `Ok(Err(reason))` for one that
/// stays as it is (16 bits a channel, sides not multiples of 4).
/// RGBA8 pixels, width and height.
#[cfg(feature = "textures")]
type Rgba8 = (Vec<u8>, u32, u32);

#[cfg(feature = "textures")]
fn rgba8(bytes: &[u8]) -> Result<Result<Rgba8, String>, String> {
    let image = image::load_from_memory(bytes).map_err(|e| format!("not a readable image: {e}"))?;
    use image::ColorType as C;
    if matches!(
        image.color(),
        C::L16 | C::La16 | C::Rgb16 | C::Rgba16 | C::Rgb32F | C::Rgba32F
    ) {
        return Ok(Err(
            "16 bits or more per channel (compression would lose precision)".into()
        ));
    }
    let image = image.into_rgba8();
    let (width, height) = image.dimensions();
    if !(width.is_multiple_of(4) && height.is_multiple_of(4)) {
        return Ok(Err(format!(
            "{width} x {height} is not a multiple of 4 (GPUs sample it as RGBA8)"
        )));
    }
    Ok(Ok((image.into_raw(), width, height)))
}

/// RGBA8 pixels to a KTX2 texture per `kind` and `rule`, with
/// `key_values` in the file.
#[cfg(feature = "textures")]
fn encode_texture(
    rgba: &[u8],
    (width, height): (u32, u32),
    kind: TextureKind,
    rule: Rule,
    settings: &Settings,
    key_values: &[(&str, &str)],
) -> Result<Result3, String> {
    let basis = rule.basis.unwrap_or(settings.platform == Platform::Mobile)
        && settings.reads.uastc
        && settings.platform != Platform::Web;
    let mipmaps = rule.mipmaps.unwrap_or(true);
    let texture_rule = crate::textures::Rule {
        kind: match kind {
            TextureKind::Color => crate::textures::Kind::Color,
            TextureKind::Linear => crate::textures::Kind::Linear,
            TextureKind::Normal => crate::textures::Kind::Normal,
            TextureKind::Mask => crate::textures::Kind::Mask,
            TextureKind::Keep => unreachable!("kept before"),
        },
        basis,
        mipmaps,
    };
    let encoded = crate::textures::encode_with(rgba, width, height, texture_rule, key_values)?;
    let before = rgba8_with_mips(width, height, true);
    Ok((
        encoded.file,
        format!("{} {width} x {height}, {} levels", encoded.format, encoded.levels),
        Some((before, encoded.gpu_bytes)),
    ))
}

/// The ORM image a model's material makes of metallic-roughness image
/// `metal_rough` and occlusion image `occlusion` (asset names, either
/// `None`), merged as the engine merges them, into file `name` (one of the
/// two), compressed as linear data with the KTX2 key that tells the engine
/// it holds them ([`model_images`]). Kept as it is when the other image
/// isn't among the assets or can't be read.
#[cfg(feature = "textures")]
fn orm_texture(
    name: &str,
    bytes: &[u8],
    (metal_rough, occlusion): (Option<&str>, Option<&str>),
    all: &BTreeMap<String, Vec<u8>>,
    rule: Rule,
    settings: &Settings,
) -> Result<Result<Result3, String>, String> {
    let (mut rgba, width, height) = match rgba8(bytes)? {
        Ok(image) => image,
        Err(why) => return Ok(Err(why)),
    };
    let pixels = rgba.as_chunks_mut::<4>().0;
    match (metal_rough, occlusion) {
        // One image for both: as it is.
        (Some(m), Some(o)) if m == o => {}
        (Some(_), Some(o)) => {
            let Some(ao) = all.get(o).and_then(|b| image::load_from_memory(b).ok()) else {
                return Ok(Err(format!(
                    "its occlusion image {o} isn't an image the cook reads, so the engine merges them"
                )));
            };
            let ao = ao.into_rgba8();
            if ao.dimensions() == (width, height) {
                for (p, a) in pixels.iter_mut().zip(ao.as_raw().as_chunks::<4>().0) {
                    p[0] = a[0];
                }
            } else {
                // The engine's merge: no occlusion where it doesn't line up.
                pixels.iter_mut().for_each(|p| p[0] = 255);
            }
        }
        // No occlusion: red 255.
        (Some(_), None) => pixels.iter_mut().for_each(|p| p[0] = 255),
        // Occlusion only: roughness, metalness and alpha 255.
        (None, Some(_)) => pixels.iter_mut().for_each(|p| *p = [p[0], 255, 255, 255]),
        (None, None) => {}
    }
    let folder = name.rfind('/').map_or("", |i| &name[..=i]);
    let rel = |file: Option<&str>| file.map_or(String::new(), |f| model_images::relative(folder, f));
    let note = format!("{}|{}", rel(metal_rough), rel(occlusion));
    let mut made = encode_texture(
        &rgba,
        (width, height),
        TextureKind::Linear,
        rule,
        settings,
        &[("ponkoOrm", &note)],
    )?;
    made.1 = format!("{}, a model's ORM image ({note})", made.1);
    Ok(Ok(made))
}

#[cfg(not(feature = "textures"))]
fn orm_texture(
    _: &str,
    _: &[u8],
    _: (Option<&str>, Option<&str>),
    _: &BTreeMap<String, Vec<u8>>,
    _: Rule,
    _: &Settings,
) -> Result<Result<Result3, String>, String> {
    Ok(Err("this ponko was built without the `textures` feature".into()))
}

#[cfg(not(feature = "textures"))]
fn texture(_: &[u8], _: TextureKind, _: Rule, _: &Settings) -> Result<Result<Result3, String>, String> {
    Ok(Err("this ponko was built without the `textures` feature".into()))
}

/// RGBA8's video memory for a `width` x `height` image with its mipmaps:
/// what the engine gives a PNG drawn by a 3D material.
#[cfg(feature = "textures")]
pub(crate) fn rgba8_with_mips(width: u32, height: u32, mipmaps: bool) -> u64 {
    let (mut w, mut h, mut total) = (u64::from(width), u64::from(height), 0);
    loop {
        total += w * h * 4;
        if !mipmaps || (w == 1 && h == 1) {
            return total;
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
}

/// How a file is cooked, as text for its cache key.
fn how(rule: &Rule, texture_json: Option<TextureKind>, settings: &Settings, kind: Kind) -> String {
    match kind {
        Kind::Texture => format!(
            "{:?} {:?} {:?} {:?} {:?}",
            rule.texture.or(texture_json),
            rule.basis,
            rule.mipmaps,
            settings.platform,
            settings.reads
        ),
        Kind::Mesh => format!(
            "{:?} {:?} {:?} {:?} {:?} {:?}",
            rule.optimize, rule.quantize, rule.texture, rule.basis, settings.platform, settings.reads
        ),
        Kind::Sound => String::new(),
        Kind::Shader => format!("{:?}", rule.shader),
        Kind::Other => String::new(),
    }
}

/// The texture kind a `textures.json` above `name` gives it, if any.
#[cfg(feature = "textures")]
fn texture_json_kinds(files: &BTreeMap<String, Vec<u8>>) -> Result<BTreeMap<String, TextureKind>, String> {
    let mut configs: Vec<(String, crate::textures::Settings)> = Vec::new();
    for (name, bytes) in files {
        if name == "textures.json" || name.ends_with("/textures.json") {
            let folder = &name[..name.len() - "textures.json".len()];
            let text = std::str::from_utf8(bytes).map_err(|_| format!("{name} is not UTF-8"))?;
            let settings = crate::textures::Settings::parse(text).map_err(|e| format!("{name}: {e}"))?;
            configs.push((folder.to_string(), settings));
        }
    }
    let mut out = BTreeMap::new();
    for name in files.keys().filter(|n| Kind::of(n) == Kind::Texture) {
        // The nearest one: the longest folder that holds the image.
        let Some((folder, settings)) = configs
            .iter()
            .filter(|(f, _)| name.starts_with(f.as_str()))
            .max_by_key(|(f, _)| f.len())
        else {
            continue;
        };
        use crate::textures::Kind as K;
        let kind = match settings.rule_for(&name[folder.len()..]).kind {
            K::Color => TextureKind::Color,
            K::Linear => TextureKind::Linear,
            K::Normal => TextureKind::Normal,
            K::Mask => TextureKind::Mask,
            K::Ui | K::Skip => TextureKind::Keep,
        };
        out.insert(name.clone(), kind);
    }
    Ok(out)
}

#[cfg(not(feature = "textures"))]
fn texture_json_kinds(_: &BTreeMap<String, Vec<u8>>) -> Result<BTreeMap<String, TextureKind>, String> {
    Ok(BTreeMap::new())
}

/// Cooks `files` ((name, bytes): every asset, named by its path inside the
/// asset folder) with `config`'s rules. The config file itself
/// ([`CONFIG`]) is left out of the output. Files are cooked on every core;
/// the output is sorted by name and the same for the same input. A shader
/// that doesn't validate, a broken model or sound, or an unreadable image
/// fails the cook, naming every such file.
pub fn cook_files(files: Vec<(String, Vec<u8>)>, config: &Config, settings: &Settings) -> Result<Cooked, String> {
    let start = Instant::now();
    let mut all: BTreeMap<String, Vec<u8>> = files.into_iter().collect();
    let cache = settings.cache.as_ref().map(|dir| Cache {
        dir: dir.join(format!("v{VERSION}")),
    });
    // From here on the in-memory sheet and JSON are cooked as exports.
    let (converted, made_from) = convert_aseprite(&mut all, config, cache.as_ref())?;
    let model_plans = model_images::plans(&all);
    let texture_json = texture_json_kinds(&all)?;
    let jobs: Vec<Job> = all
        .iter()
        .filter(|(name, _)| name.as_str() != CONFIG)
        .map(|(name, bytes)| Job {
            name,
            bytes,
            rule: config.rule_for(name),
            texture_json: texture_json.get(name.as_str()).copied(),
        })
        .collect();

    // Every core takes the next file; results land in their own slot, so
    // the output's order doesn't depend on who finished first.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(jobs.len().max(1));
    let mut slots: Vec<Slot> = vec![None; jobs.len()];
    let results = std::sync::Mutex::new(&mut slots);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let result = run_job(job, &all, &model_plans, settings, cache.as_ref());
                    results.lock().expect("no panics while holding it")[i] = Some(result);
                }
            });
        }
    });

    // A `.gltf` file's `.bin` buffers are inside the GLB it was cooked into.
    let mut embedded = embedded_buffers(&jobs, &slots);
    let mut cooked = Cooked::default();
    let mut errors = Vec::new();
    for (job, slot) in jobs.iter().zip(slots) {
        let (mut outcome, made) = match slot.expect("every job ran") {
            Ok(r) => r,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if outcome == Outcome::Kept(None)
            && let Some(model) = embedded.remove(job.name)
        {
            outcome = Outcome::Embedded(model);
        }
        // A file an Aseprite file was made into wasn't among the sources.
        let from = made_from.get(job.name);
        let source_bytes = if from.is_some() { 0 } else { job.bytes.len() as u64 };
        let (bytes, note, gpu) = match (&outcome, made) {
            (Outcome::Skipped | Outcome::Embedded(_), _) => {
                cooked.entries.push(Entry {
                    name: job.name.to_string(),
                    kind: Kind::of(job.name),
                    outcome,
                    source_bytes,
                    cooked_bytes: 0,
                    gpu: None,
                    note: String::new(),
                });
                continue;
            }
            (_, Some(made)) => made,
            (_, None) => (job.bytes.to_vec(), String::new(), None),
        };
        let note = match from {
            Some(from) if note.is_empty() => format!("made from {from}"),
            Some(from) => format!("made from {from}, {note}"),
            None => note,
        };
        cooked.entries.push(Entry {
            name: job.name.to_string(),
            kind: Kind::of(job.name),
            outcome,
            source_bytes,
            cooked_bytes: bytes.len() as u64,
            gpu,
            note,
        });
        cooked.files.push((job.name.to_string(), bytes));
    }
    cooked.entries.extend(converted);
    cooked.entries.sort_by(|a, b| a.name.cmp(&b.name));
    if !errors.is_empty() {
        return Err(format!(
            "{} asset{} can't be cooked:\n{}",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" },
            errors.join("\n")
        ));
    }
    cooked.seconds = start.elapsed().as_secs_f64();
    Ok(cooked)
}

/// Makes each Aseprite file of `all` (unless its rule keeps or skips it)
/// into its sheet and JSON ([`aseprite::export`]): they take its place in
/// `all`. Returns its entries and, for each file made, the Aseprite file it
/// was made from. An export already beside an Aseprite file (from
/// Aseprite, for hot reload while developing) is replaced, so the cooked
/// files are never older than their source. A file that can't be read is
/// an error.
#[cfg(feature = "aseprite")]
fn convert_aseprite(
    all: &mut BTreeMap<String, Vec<u8>>,
    config: &Config,
    cache: Option<&Cache>,
) -> Result<(Vec<Entry>, BTreeMap<String, String>), String> {
    let names: Vec<String> = all.keys().filter(|n| aseprite::is_aseprite(n)).cloned().collect();
    let (mut entries, mut made, mut errors) = (Vec::new(), BTreeMap::new(), Vec::new());
    for name in names {
        let rule = config.rule_for(&name);
        if rule.keep || rule.skip {
            continue;
        }
        let (json, png) = aseprite::outputs(&name);
        let beside: Vec<&str> = [&json, &png]
            .into_iter()
            .filter(|o| all.contains_key(*o))
            .map(String::as_str)
            .collect();
        let bytes = all.remove(&name).unwrap_or_default();
        let split = rule.layers.unwrap_or(false);
        // The JSON embeds the sheet's basename. Moving the pair to a
        // different directory reuses the conversion.
        let basename = png.rsplit('/').next().unwrap_or(&png);
        let key = cache.map(|_| key(Kind::Other, &format!("aseprite:{split}:{basename}"), &bytes, &[]));
        let cached = cache.zip(key.as_deref()).and_then(|(cache, key)| {
            let (bytes, note, _) = cache.get(key)?;
            let (json, png) = aseprite::cached_pair(&bytes)?;
            Some(aseprite::Sheet {
                json: json.to_vec(),
                png: png.to_vec(),
                note,
            })
        });
        let (converted, outcome) = match cached {
            Some(sheet) => (Ok(sheet), Outcome::Cached),
            None => {
                let converted = aseprite::export(&name, &bytes, split);
                if let (Ok(sheet), Some((cache, key))) = (&converted, cache.zip(key.as_deref())) {
                    cache.put(key, &(aseprite::cache_pair(sheet), sheet.note.clone(), None));
                }
                (converted, Outcome::Converted(vec![json.clone(), png.clone()]))
            }
        };
        match converted {
            Ok(mut sheet) => {
                if !beside.is_empty() {
                    sheet.note = format!("{}, replacing the {} beside it", sheet.note, beside.join(" and "));
                }
                entries.push(Entry {
                    name: name.clone(),
                    kind: Kind::Other,
                    outcome,
                    source_bytes: bytes.len() as u64,
                    cooked_bytes: 0,
                    gpu: None,
                    note: sheet.note,
                });
                all.insert(json.clone(), sheet.json);
                all.insert(png.clone(), sheet.png);
                made.insert(json, name.clone());
                made.insert(png, name);
            }
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "{} asset{} can't be cooked:\n{}",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" },
            errors.join("\n")
        ));
    }
    Ok((entries, made))
}

/// Without the `aseprite` feature, Aseprite files are copied.
#[cfg(not(feature = "aseprite"))]
fn convert_aseprite(
    _: &mut BTreeMap<String, Vec<u8>>,
    _: &Config,
    _: Option<&Cache>,
) -> Result<(Vec<Entry>, BTreeMap<String, String>), String> {
    Ok((Vec::new(), BTreeMap::new()))
}

/// Runs one job, through the cache.
fn run_job(
    job: &Job,
    all: &BTreeMap<String, Vec<u8>>,
    model_plans: &BTreeMap<String, model_images::Plan>,
    settings: &Settings,
    cache: Option<&Cache>,
) -> Result<(Outcome, Option<Result3>), String> {
    if job.rule.skip {
        return Ok((Outcome::Skipped, None));
    }
    if job.rule.keep {
        return Ok((Outcome::Kept(None), None));
    }
    let kind = Kind::of(job.name);
    if kind == Kind::Other {
        return Ok((Outcome::Kept(None), None));
    }
    // A model's key covers the buffers and images it reads; an image's,
    // whether a model uses it.
    let deps: Vec<(String, Vec<u8>)> = match kind {
        #[cfg(feature = "meshes")]
        Kind::Mesh => gltf::dependencies(job.name, job.bytes, all).unwrap_or_default(),
        Kind::Texture => {
            let plan = model_plans.get(job.name);
            let mut deps = vec![(format!("a model's image: {plan:?}"), Vec::new())];
            if let Some(model_images::Plan::Orm { occlusion: Some(o), .. }) = plan
                && o != job.name
            {
                deps.push((o.clone(), all.get(o).cloned().unwrap_or_default()));
            }
            deps
        }
        _ => Vec::new(),
    };
    let key = key(
        kind,
        &how(&job.rule, job.texture_json, settings, kind),
        job.bytes,
        &deps,
    );
    if let Some(hit) = cache.and_then(|c| c.get(&key)) {
        return Ok((Outcome::Cached, Some(hit)));
    }
    match cook_one(job, all, model_plans, settings).map_err(|e| {
        if e.starts_with(job.name) {
            e
        } else {
            format!("{}: {e}", job.name)
        }
    })? {
        Ok(made) => {
            if let Some(cache) = cache {
                cache.put(&key, &made);
            }
            Ok((Outcome::Cooked, Some(made)))
        }
        Err(why) => Ok((Outcome::Kept(why), None)),
    }
}

/// The marker file in a folder `ponko cook` wrote.
const MARKER: &str = ".ponko-cooked";

/// Reads the files of `folder` ((name, bytes)), hidden ones and `skip`
/// (the output, when it is inside) left out.
pub fn read_folder(folder: &Path, skip: Option<&Path>) -> Result<Vec<(String, Vec<u8>)>, String> {
    crate::pack::folder_files(folder, skip)?
        .into_iter()
        .map(|(name, path)| {
            std::fs::read(&path)
                .map(|bytes| (name, bytes))
                .map_err(|e| format!("can't read {}: {e}", path.display()))
        })
        .collect()
}

/// The `cook.json` of `files`, or the defaults.
pub fn config_of(files: &[(String, Vec<u8>)]) -> Result<Config, String> {
    match files.iter().find(|(name, _)| name == CONFIG) {
        Some((_, bytes)) => Config::parse(std::str::from_utf8(bytes).map_err(|_| format!("{CONFIG} is not UTF-8"))?),
        None => Ok(Config::default()),
    }
}

/// Runs `ponko cook` with `options`, relative to `dir`.
pub fn cook_command(options: &Options, dir: &Path) -> Result<String, String> {
    let folder = dir.join(options.folder.clone().unwrap_or_else(|| "assets".into()));
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let out = options
        .out
        .as_ref()
        .map(|o| dir.join(o))
        .unwrap_or_else(|| dir.join("target").join("cooked").join(options.platform.name()));
    let cache = if options.no_cache {
        None
    } else {
        Some(
            options
                .cache
                .as_ref()
                .map_or_else(|| dir.join("target").join("cook-cache"), |c| dir.join(c)),
        )
    };
    let skip = options
        .pack
        .as_ref()
        .map(|p| dir.join(p))
        .unwrap_or_else(|| out.clone());
    let files = read_folder(&folder, Some(&skip))?;
    let config = config_of(&files)?;
    let settings = Settings {
        platform: options.platform,
        reads: Reads::ALL,
        cache,
    };
    let cooked = cook_files(files, &config, &settings)?;
    let mut report = cooked.report();
    if let Some(pack) = &options.pack {
        let path = dir.join(pack);
        let packed = crate::pack::write(cooked.files, true)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("can't make {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &packed.bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        report.push_str(&format!(
            "\npacked into {} ({})",
            path.display(),
            crate::pack::size(packed.bytes.len() as u64)
        ));
    } else {
        write_folder(&out, &cooked.files)?;
        report.push_str(&format!("\nwrote {}", out.display()));
    }
    Ok(report)
}

/// Writes `files` into `out`, emptied first when `ponko cook` made it.
pub(crate) fn write_folder(out: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    if out.exists() {
        let empty = std::fs::read_dir(out)
            .map_err(|e| format!("can't read {}: {e}", out.display()))?
            .next()
            .is_none();
        if !empty && !out.join(MARKER).is_file() {
            return Err(format!(
                "{} holds files ponko cook didn't write: give an empty or new folder with --out",
                out.display()
            ));
        }
        std::fs::remove_dir_all(out).map_err(|e| format!("can't empty {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(out).map_err(|e| format!("can't make {}: {e}", out.display()))?;
    std::fs::write(out.join(MARKER), "made by ponko cook; emptied by the next cook\n")
        .map_err(|e| format!("can't write in {}: {e}", out.display()))?;
    for (name, bytes) in files {
        crate::pack::check_name(name)?;
        let path = out.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("can't make {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
