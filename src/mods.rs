//! Mods: packs of asset files that players add to a game, mounted in a
//! declared load order, checked before anything of them is used; with the
//! feature `mods-wasm`, a mod may also bring code (see [Code mods](#code-mods)).
//!
//! **Maturity: tested**, from the guide chapter `mods.md`. Game: a sample mod for Ember Hop, run by
//! `tests/mod_sample.rs`. Tests: `tests/mods.rs`, `tests/mod_code.rs`, `tests/mod_code_host.rs`.
//! Platforms: Linux, macOS and Windows, headless (the workspace tests in the list of checks).
//! Graphics: not involved. Missing: a shipped game that loads players' mods.
//!
//! A mod is an [asset pack](crate::assets::PackSource) (or files in memory)
//! with a `mod.json` manifest at its root. It can add files (a new enemy's
//! prefab and sprite sheet) and replace the game's own by using the same
//! path (`prefabs/slime.json`): the game loads assets by name as always and
//! gets the mod's. A data-only mod runs no code; what it can change is what
//! the game reads from files: [prefabs and scenes](crate::assets::Assets::save_prefab)
//! made of [reflected](crate::reflect::WorldReflect) components, data files,
//! art and sound.
//!
//! ```
//! use ponko::mods::{ModPack, ModSet};
//! use ponko::prelude::*;
//!
//! #[derive(Component, serde::Serialize, serde::Deserialize)]
//! struct Slime {
//!     speed: f32,
//! }
//!
//! let manifest = r#"{ "format": "ponko-mod", "version": 1, "id": "fast-slimes" }"#;
//! let prefab = r#"{ "format": "ponko-prefab", "version": 1,
//!                   "components": { "Slime": { "speed": 60.0 } } }"#;
//! let fast = ModPack::from_files(
//!     "fast-slimes",
//!     [("mod.json", manifest), ("prefabs/slime.json", prefab)],
//! )
//! .unwrap();
//!
//! Game::new("modded", 1, 1).run_headless(0, |ctx| {
//!     ctx.world.reflect::<Slime>(); // reflect first: mods are checked against it
//!     ModSet::new().with(fast).mount(ctx).expect("the mods are fine");
//!     let slime = ctx.spawn_prefab("prefabs/slime.json", vec2(0.0, 0.0)).unwrap();
//!     assert_eq!(ctx.world.get::<Slime>(slime).unwrap().speed, 60.0);
//! });
//! ```
//!
//! # The manifest
//!
//! ```json
//! {
//!   "format": "ponko-mod",
//!   "version": 1,
//!   "id": "frost-drips",
//!   "name": "Frost drips",
//!   "mod_version": "1.2.0",
//!   "requires": ["deep-caves"],
//!   "after": ["hd-sprites"]
//! }
//! ```
//!
//! `id` is lowercase letters, digits, `-` and `_` (at most 64). `requires`
//! names mods that must be mounted too and load before this one; `after`
//! names mods that load before this one if they are there. `name`,
//! `mod_version`, `requires` and `after` may be left out.
//!
//! # Load order
//!
//! Mods load in the order they were [added](ModSet::add), moved only as far
//! as `requires` and `after` need: a mod comes after every mod it names.
//! Where two mods have a file of the same name, the later one's is used; the
//! game's own files (`assets/`, its `assets.pack`) come before every mod.
//!
//! # What is checked
//!
//! [`ModSet::mount`] checks every mod before it mounts any of them, so a bad
//! mod is refused whole and nothing of it (or of the mods with it) is used.
//! The [`ModError`] names the mod, the file and the reason:
//! - the pack: damaged or not a pack, too many files, a file larger than
//!   [`ModLimits`] allow, or a name that isn't a plain relative path
//!   (`../`, `/etc`, `C:`, `\`);
//! - the manifest: missing, not JSON, an id twice, a required mod missing,
//!   mods that must load after each other in a cycle;
//! - every `.json` file: it parses, and the prefabs and scenes in it only
//!   use components and resources the game reflects, with fields that fit;
//! - every asset those prefabs and scenes name (textures, meshes, skeletons,
//!   included prefabs, the game's own [asset
//!   fields](crate::assets::Assets::scene_asset_field)) is in a mod of the set or
//!   in the game's assets: one error lists every missing one, with the file
//!   that names it ([`ModProblem::MissingAssets`]);
//! - its code, if it has some: the module compiles and imports only host
//!   functions, and starts within its limits.
//!
//! The game must [reflect](crate::reflect::WorldReflect::reflect) its
//! components before it mounts mods. What is only known when the mod is
//! used (an image that doesn't decode) fails then, with that file's error,
//! as for the game's own files.
//!
//! # Code mods
//!
//! With the feature `mods-wasm`, a manifest may name a WebAssembly module
//! (`"code": "code/frost.wasm"`). Mounting compiles and starts it in a
//! sandbox, an interpreter with fuel and memory limits ([`CodeLimits`]) that
//! runs on every platform the engine builds for, the web included. The game
//! calls the functions it exports with [`with_code`]; a module reaches only
//! the host functions listed there. Without the feature, a mod with code is
//! refused with [`ModProblem::Code`].
//!
//! # Determinism
//!
//! A mod is data, so a modded game is as deterministic as the game: the
//! same mods in the same order give the same files. Lockstep peers and
//! replays should agree on [`Mods::fingerprint`].
#![deny(missing_docs)]

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use ponko_ecs::World;
use serde_json::Value;

use crate::assets::{AssetSource, PackSource};
use crate::ctx::Ctx;
use crate::reflect::ReflectError;

#[cfg(feature = "mods-wasm")]
mod code;
#[cfg(feature = "mods-wasm")]
pub use code::{CodeLimits, ModCode, ModFn, ModValue, REFUSED_ENTITY, REFUSED_FIELD, REFUSED_VALUE, with_code};

/// What checking a mod's code needs: the engine it compiles with.
#[cfg(feature = "mods-wasm")]
type CodeEngine = wasmi::Engine;
#[cfg(not(feature = "mods-wasm"))]
struct CodeEngine;

/// The manifest's file name, at the root of a mod.
pub const MANIFEST: &str = "mod.json";
/// The `"format"` of a manifest.
const FORMAT: &str = "ponko-mod";
/// The manifest version this engine reads.
const VERSION: u64 = 1;
/// The [`AssetSource::id`] the mounted mods have.
pub const SOURCE_ID: &str = "mods";
/// The longest file name a mod may have, in bytes.
const MAX_NAME: usize = 512;

/// How much a mod may hold. A pack over a limit is refused before any of
/// its files is read.
///
/// ```
/// use ponko::mods::ModLimits;
/// let strict = ModLimits { max_files: 100, ..ModLimits::default() };
/// assert_eq!(strict.max_file_bytes, 64 << 20);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModLimits {
    /// Files in one mod. Default 10 000.
    pub max_files: usize,
    /// Bytes of one file once unpacked. Default 64 MiB.
    pub max_file_bytes: u64,
    /// Bytes of all of one mod's files once unpacked. Default 1 GiB.
    pub max_total_bytes: u64,
}

impl Default for ModLimits {
    fn default() -> Self {
        Self {
            max_files: 10_000,
            max_file_bytes: 64 << 20,
            max_total_bytes: 1 << 30,
        }
    }
}

/// What a mod's `mod.json` says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ModManifest {
    /// Always `"ponko-mod"`.
    format: String,
    /// The manifest version, 1.
    version: u64,
    /// The mod's id: lowercase letters, digits, `-` and `_`.
    pub id: String,
    /// A name for people; the id if left out.
    #[serde(default)]
    pub name: String,
    /// The mod's own version, as its author numbers it; empty if left out.
    #[serde(default)]
    pub mod_version: String,
    /// Mods that must be mounted too, and load before this one.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Mods that load before this one when they are mounted.
    #[serde(default)]
    pub after: Vec<String>,
    /// The mod's WebAssembly module, a file of the mod (feature
    /// `mods-wasm`); `None` for a data-only mod.
    #[serde(default)]
    pub code: Option<String>,
}

/// Where a mod's files are.
enum Files {
    Pack(PackSource),
    Memory(HashMap<String, Arc<[u8]>>),
}

impl Files {
    fn names(&self) -> Vec<&str> {
        match self {
            Files::Pack(p) => p.names().collect(),
            Files::Memory(m) => m.keys().map(String::as_str).collect(),
        }
    }

    fn size(&self, name: &str) -> Option<u64> {
        match self {
            Files::Pack(p) => p.size(name),
            Files::Memory(m) => m.get(name).map(|b| b.len() as u64),
        }
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        match self {
            Files::Pack(p) => p.read(name),
            Files::Memory(m) => m.get(name).map(|b| b.to_vec()).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, format!("\"{name}\" is not in the mod"))
            }),
        }
    }
}

/// One mod, opened: its manifest is read, its files not yet checked
/// (that is [`ModSet::mount`]'s job).
pub struct ModPack {
    /// Where it came from: the pack's file name, or the name given.
    origin: String,
    manifest: ModManifest,
    files: Files,
}

impl fmt::Debug for ModPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModPack")
            .field("origin", &self.origin)
            .field("id", &self.manifest.id)
            .finish_non_exhaustive()
    }
}

impl ModPack {
    /// Opens the mod pack at `path` (made with `ponko pack` from a folder
    /// with `mod.json` at its root) and reads its manifest.
    ///
    /// # Errors
    /// The pack can't be read or isn't one, or its manifest is missing or
    /// wrong; the error names the file.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<ModPack, ModError> {
        let path = path.as_ref();
        let origin = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let pack = PackSource::open(path)
            .map_err(|e| ModError::new(&origin, None, ModProblem::Unreadable(e.message().into())))?;
        Self::with_files(origin, Files::Pack(pack))
    }

    /// A mod pack held in memory: downloaded, or (in a browser) fetched.
    /// `origin` names it in errors until its manifest is read.
    ///
    /// # Errors
    /// As [`open`](Self::open).
    pub fn from_pack_bytes(origin: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Result<ModPack, ModError> {
        let origin = origin.into();
        let pack = PackSource::from_bytes(origin.clone(), bytes)
            .map_err(|e| ModError::new(&origin, None, ModProblem::Unreadable(e.message().into())))?;
        Self::with_files(origin, Files::Pack(pack))
    }

    /// A mod made of loose files: a mod folder read by the game, one
    /// built into it, or one made up in a test. `origin` names it in errors
    /// until its manifest is read. A name given twice keeps the last bytes.
    ///
    /// ```
    /// use ponko::mods::ModPack;
    /// let m = ModPack::from_files("demo", [("mod.json", r#"{"format":"ponko-mod","version":1,"id":"demo"}"#)]).unwrap();
    /// assert_eq!(m.id(), "demo");
    /// ```
    ///
    /// # Errors
    /// Its manifest is missing or wrong.
    pub fn from_files<N: Into<String>, B: Into<Vec<u8>>>(
        origin: impl Into<String>,
        files: impl IntoIterator<Item = (N, B)>,
    ) -> Result<ModPack, ModError> {
        let files = files
            .into_iter()
            .map(|(n, b)| (n.into(), Arc::<[u8]>::from(b.into())))
            .collect();
        Self::with_files(origin.into(), Files::Memory(files))
    }

    fn with_files(origin: String, files: Files) -> Result<ModPack, ModError> {
        let bad = |problem| ModError::new(&origin, Some(MANIFEST), problem);
        let size = files
            .size(MANIFEST)
            .ok_or_else(|| bad(ModProblem::Manifest(format!("a mod needs a {MANIFEST} at its root"))))?;
        if size > 1 << 20 {
            return Err(bad(ModProblem::TooLarge { size, limit: 1 << 20 }));
        }
        let bytes = files
            .read(MANIFEST)
            .map_err(|e| bad(ModProblem::Unreadable(e.to_string())))?;
        let mut manifest: ModManifest =
            serde_json::from_slice(&bytes).map_err(|e| bad(ModProblem::Manifest(e.to_string())))?;
        if manifest.format != FORMAT || manifest.version != VERSION {
            return Err(bad(ModProblem::Manifest(format!(
                "expected \"format\": \"{FORMAT}\" and \"version\": {VERSION} (got \"{}\" and {})",
                manifest.format, manifest.version
            ))));
        }
        for id in std::iter::once(&manifest.id)
            .chain(&manifest.requires)
            .chain(&manifest.after)
        {
            if !valid_id(id) {
                return Err(bad(ModProblem::Manifest(format!(
                    "\"{id}\" is not a mod id: 1 to 64 of a-z, 0-9, '-' and '_'"
                ))));
            }
        }
        if manifest.name.is_empty() {
            manifest.name = manifest.id.clone();
        }
        Ok(ModPack {
            origin,
            manifest,
            files,
        })
    }

    /// The mod's id, from its manifest.
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Its manifest.
    pub fn manifest(&self) -> &ModManifest {
        &self.manifest
    }

    /// Checks the files of this mod against `limits` and `world`'s
    /// reflection, and compiles its code; returns the bytes of the JSON and
    /// code files it read, for the fingerprint and the asset references.
    fn check(&self, world: &World, limits: &ModLimits, engine: &CodeEngine) -> Result<Checked, ModError> {
        let id = self.id();
        let mut names = self.files.names();
        names.sort_unstable();
        if names.len() > limits.max_files {
            return Err(ModError::new(
                id,
                None,
                ModProblem::TooManyFiles {
                    count: names.len(),
                    limit: limits.max_files,
                },
            ));
        }
        let mut total = 0u64;
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            if let Err(why) = check_name(name) {
                return Err(ModError::new(id, Some(name), ModProblem::BadName(why)));
            }
            let size = self.files.size(name).unwrap_or(0);
            if size > limits.max_file_bytes {
                return Err(ModError::new(
                    id,
                    Some(name),
                    ModProblem::TooLarge {
                        size,
                        limit: limits.max_file_bytes,
                    },
                ));
            }
            total = total.saturating_add(size);
            if total > limits.max_total_bytes {
                return Err(ModError::new(
                    id,
                    Some(name),
                    ModProblem::TooLarge {
                        size: total,
                        limit: limits.max_total_bytes,
                    },
                ));
            }
            let is_code = self.manifest.code.as_deref() == Some(name);
            let json = if (name.ends_with(".json") && name != MANIFEST) || is_code {
                let bytes = self
                    .files
                    .read(name)
                    .map_err(|e| ModError::new(id, Some(name), ModProblem::Unreadable(e.to_string())))?;
                if !is_code {
                    check_json(world, &bytes).map_err(|problem| ModError::new(id, Some(name), problem))?;
                }
                Some(bytes)
            } else {
                None
            };
            out.push((name.to_string(), size, json));
        }
        let module = self.check_code(&out, engine)?;
        Ok(Checked { files: out, module })
    }

    /// Compiles the mod's code, if it has some.
    #[cfg(feature = "mods-wasm")]
    fn check_code(&self, files: &[CheckedFile], engine: &CodeEngine) -> Result<Option<wasmi::Module>, ModError> {
        let Some(name) = self.manifest.code.as_deref() else {
            return Ok(None);
        };
        let bad = |problem| ModError::new(self.id(), Some(name), problem);
        let Some((_, _, Some(bytes))) = files.iter().find(|(n, _, _)| n == name) else {
            return Err(bad(ModProblem::Code(
                "the manifest names this module, and the mod has no such file".into(),
            )));
        };
        code::compile(engine, bytes).map(Some).map_err(bad)
    }

    /// Without the feature `mods-wasm`, a mod with code is refused.
    #[cfg(not(feature = "mods-wasm"))]
    fn check_code(&self, _: &[CheckedFile], _: &CodeEngine) -> Result<Option<()>, ModError> {
        match self.manifest.code.as_deref() {
            None => Ok(None),
            Some(name) => Err(ModError::new(
                self.id(),
                Some(name),
                ModProblem::Code(
                    "this game was built without code mods: turn on the `mods-wasm` cargo feature, or remove the code from the mod's manifest"
                        .into(),
                ),
            )),
        }
    }
}

/// A checked file of a mod: its name, its size, and its bytes if it is JSON
/// or the mod's code.
type CheckedFile = (String, u64, Option<Vec<u8>>);

/// What [`ModPack::check`] found fine: the files, and the compiled code.
struct Checked {
    files: Vec<CheckedFile>,
    #[cfg(feature = "mods-wasm")]
    module: Option<wasmi::Module>,
    #[cfg(not(feature = "mods-wasm"))]
    #[allow(dead_code)]
    module: Option<()>,
}

/// 1 to 64 of `a-z`, `0-9`, `-`, `_`.
fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Whether `name` is a plain relative path: `/`-separated parts that are
/// not empty, `.` or `..`, with no `\`, `:` or control characters, so no
/// name can point outside the game's assets wherever it ends up (a save,
/// an export, a cache on disk).
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(format!("a file name must be 1 to {MAX_NAME} bytes"));
    }
    if name.starts_with('/') {
        return Err("an absolute path; names are relative to the game's assets".into());
    }
    if let Some(c) = name.chars().find(|&c| c == '\\' || c == ':' || c.is_control()) {
        return Err(format!("{c:?} is not allowed in a file name"));
    }
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("an empty, \".\" or \"..\" part; names are plain relative paths".into());
    }
    Ok(())
}

/// Checks a `.json` file of a mod: it parses, and if it is a prefab or a
/// scene, what it spawns is reflected and fits.
fn check_json(world: &World, bytes: &[u8]) -> Result<(), ModProblem> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| ModProblem::BadJson(e.to_string()))?;
    if value["format"] == crate::scene::PREFAB_FORMAT {
        let file = crate::scene::PrefabFile::parse(&value).map_err(ModProblem::BadJson)?;
        for entry in &file.entries {
            // An entry that includes a prefab changes some fields of it: only
            // the names can be checked; any other entry is whole components.
            let whole = entry.prefab.is_none();
            for (name, value) in &entry.components {
                check_component(world, name, value, whole)?;
            }
            for name in &entry.remove {
                check_component(world, name, &Value::Null, false)?;
            }
            for part in &entry.parts {
                for (name, value) in &part.components {
                    check_component(world, name, value, false)?;
                }
                for name in &part.remove {
                    check_component(world, name, &Value::Null, false)?;
                }
            }
        }
    } else if crate::reflect::is_world_save(&value) {
        crate::reflect::check_world_save(world, &value).map_err(ModProblem::from)?;
    }
    Ok(())
}

/// The assets the JSON files of a mod name that are neither in `all` (the
/// files of the mods being mounted) nor in the game's assets, as `(file,
/// name)`, sorted.
fn missing_assets(ctx: &Ctx, all: &std::collections::HashSet<&str>, files: &[CheckedFile]) -> Vec<(String, String)> {
    let mut missing = Vec::new();
    let mut names = Vec::new();
    for (file, _, bytes) in files {
        let Some(bytes) = bytes.as_deref().filter(|_| file.ends_with(".json") && file != MANIFEST) else {
            continue;
        };
        // It parsed when it was checked.
        let Ok(json) = serde_json::from_slice::<Value>(bytes) else {
            continue;
        };
        names.clear();
        asset_references(ctx, &json, &mut names);
        names.sort_unstable();
        names.dedup();
        for name in &names {
            if !all.contains(name.as_str()) && ctx.assets.asset_sources.locate(name).is_none() {
                missing.push((file.clone(), name.clone()));
            }
        }
    }
    missing
}

/// Every asset name `json` (a prefab or a scene) refers to: what spawning
/// it loads first (textures, skeletons, meshes, collision meshes, the
/// game's registered asset fields) and the prefabs it includes.
fn asset_references(ctx: &Ctx, json: &Value, out: &mut Vec<String>) {
    use crate::reflect::asset_names;
    for (component, field) in [
        ("Sprite", "texture"),
        ("ParticleEmitter2D", "texture"),
        ("Animation", "texture"),
        ("Skeleton2D", "skeleton"),
        ("Model", "mesh"),
        ("ShadowMesh", "mesh"),
        ("ModelMaterial", "texture"),
    ] {
        asset_names(json, component, field, out);
    }
    #[cfg(feature = "3d")]
    crate::reflect::lod_mesh_names(json, out);
    #[cfg(feature = "physics3d")]
    {
        let mut files = Vec::new();
        crate::physics3d::collider_files(json, &mut files);
        out.extend(
            files
                .into_iter()
                .filter_map(|f| crate::reflect::collider_file_parts(f).0.map(String::from)),
        );
    }
    crate::scene::scene_asset_field_names(&ctx.assets.scene_asset_fields, json, out);
    included_prefabs(json, out);
}

/// The `"prefab"` names of a prefab file's entities.
fn included_prefabs(json: &Value, out: &mut Vec<String>) {
    if json["format"] != crate::scene::PREFAB_FORMAT {
        return;
    }
    if let Ok(file) = crate::scene::PrefabFile::parse(json) {
        out.extend(file.entries.into_iter().filter_map(|e| e.prefab));
    }
}

fn check_component(world: &World, name: &str, value: &Value, whole: bool) -> Result<(), ModProblem> {
    let checked = if whole {
        crate::reflect::check_component(world, name, value)
    } else {
        crate::reflect::is_reflected(world, name)
    };
    checked.map_err(ModProblem::from)
}

/// Why a mod was refused. [`ModError`] says which mod and file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModProblem {
    /// The pack or a file in it can't be read: not a pack, cut short,
    /// damaged.
    Unreadable(String),
    /// `mod.json` is missing or says something this engine doesn't read.
    Manifest(String),
    /// Two mods have this id.
    DuplicateId(String),
    /// A mod in `requires` isn't mounted.
    MissingDependency(String),
    /// Mods that must each load after the next: `a -> b -> a`.
    Cycle(Vec<String>),
    /// A file name that isn't a plain relative path, and why.
    BadName(String),
    /// More bytes than [`ModLimits`] allow (one file, or the whole mod).
    TooLarge {
        /// Its size.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// More files than [`ModLimits::max_files`].
    TooManyFiles {
        /// How many.
        count: usize,
        /// The limit.
        limit: usize,
    },
    /// A `.json` file that doesn't parse, or isn't a prefab it says it is.
    BadJson(String),
    /// A component the game doesn't reflect.
    UnknownComponent(String),
    /// A resource the game doesn't reflect.
    UnknownResource(String),
    /// A component or resource whose fields don't fit its type.
    BadValue {
        /// The component or resource.
        component: String,
        /// What didn't fit.
        message: String,
    },
    /// Mods were already mounted in this game.
    AlreadyMounted,
    /// Assets the mod's prefabs and scenes name that neither a mod of the
    /// set nor the game has: each as `(file, name)`, the file that names it
    /// and the name, sorted. Every missing one is listed, not the first.
    MissingAssets(Vec<(String, String)>),
    /// The mod's code can't be used: the module the manifest names is
    /// missing or not WebAssembly, imports something that isn't a host
    /// function, can't start within its limits, or the game was built
    /// without code mods (the feature `mods-wasm`).
    Code(String),
    /// A call into the mod's code failed: arguments that don't fit, out of
    /// fuel, out of memory, a WebAssembly trap.
    Trap(String),
}

impl From<ReflectError> for ModProblem {
    fn from(e: ReflectError) -> Self {
        match e {
            ReflectError::UnknownComponent(name) => ModProblem::UnknownComponent(name),
            ReflectError::UnknownResource(name) => ModProblem::UnknownResource(name),
            ReflectError::BadValue { component, message } => ModProblem::BadValue { component, message },
            other => ModProblem::BadJson(other.to_string()),
        }
    }
}

impl fmt::Display for ModProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModProblem::Unreadable(why) => write!(f, "can't be read: {why}"),
            ModProblem::Manifest(why) => write!(f, "bad manifest: {why}"),
            ModProblem::DuplicateId(id) => write!(f, "another mod has the id \"{id}\""),
            ModProblem::MissingDependency(id) => write!(f, "requires mod \"{id}\", which isn't mounted"),
            ModProblem::Cycle(chain) => write!(f, "mods must load after each other in a cycle: {}", chain.join(" -> ")),
            ModProblem::BadName(why) => write!(f, "bad file name: {why}"),
            ModProblem::TooLarge { size, limit } => write!(f, "{size} bytes, more than the {limit} allowed"),
            ModProblem::TooManyFiles { count, limit } => write!(f, "{count} files, more than the {limit} allowed"),
            ModProblem::BadJson(why) => write!(f, "bad JSON: {why}"),
            ModProblem::UnknownComponent(name) => {
                write!(f, "component \"{name}\" isn't one this game has (not reflected)")
            }
            ModProblem::UnknownResource(name) => {
                write!(f, "resource \"{name}\" isn't one this game has (not reflected)")
            }
            ModProblem::BadValue { component, message } => write!(f, "\"{component}\" doesn't fit: {message}"),
            ModProblem::AlreadyMounted => write!(f, "mods were already mounted in this game"),
            ModProblem::MissingAssets(missing) => {
                write!(f, "{} asset(s) named that no mod and not the game has:", missing.len())?;
                for (i, (file, name)) in missing.iter().enumerate() {
                    let sep = if i == 0 { " " } else { ", " };
                    write!(f, "{sep}\"{name}\" (in \"{file}\")")?;
                }
                Ok(())
            }
            ModProblem::Code(why) => write!(f, "bad code: {why}"),
            ModProblem::Trap(why) => write!(f, "its code failed: {why}"),
        }
    }
}

/// A mod that was refused: which one ([`mod_id`](Self::mod_id), or the
/// pack's name before its manifest was read), which of its files, and why.
///
/// ```
/// use ponko::mods::{ModPack, ModProblem};
/// let err = ModPack::from_files("empty.pack", [("readme.txt", "hi")]).unwrap_err();
/// assert_eq!(err.mod_id(), "empty.pack");
/// assert_eq!(err.file(), Some("mod.json"));
/// assert!(matches!(err.problem(), ModProblem::Manifest(_)));
/// assert!(err.to_string().starts_with("mod \"empty.pack\", file \"mod.json\": bad manifest"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModError {
    mod_id: String,
    file: Option<String>,
    problem: ModProblem,
}

impl ModError {
    fn new(mod_id: &str, file: Option<&str>, problem: ModProblem) -> Self {
        Self {
            mod_id: mod_id.to_string(),
            file: file.map(str::to_string),
            problem,
        }
    }

    /// The mod's id, or its pack's name if its manifest wasn't read.
    pub fn mod_id(&self) -> &str {
        &self.mod_id
    }

    /// The file in the mod, when the problem is in one.
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// What is wrong.
    pub fn problem(&self) -> &ModProblem {
        &self.problem
    }
}

impl fmt::Display for ModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mod \"{}\"", self.mod_id)?;
        if let Some(file) = &self.file {
            write!(f, ", file \"{file}\"")?;
        }
        write!(f, ": {}", self.problem)
    }
}

impl std::error::Error for ModError {}

impl From<ModError> for crate::error::Error {
    fn from(e: ModError) -> Self {
        use crate::error::ErrorKind;
        let kind = match &e.problem {
            ModProblem::Unreadable(_) => ErrorKind::Decode,
            ModProblem::MissingDependency(_)
            | ModProblem::UnknownComponent(_)
            | ModProblem::UnknownResource(_)
            | ModProblem::MissingAssets(_) => ErrorKind::NotFound,
            ModProblem::TooLarge { .. } | ModProblem::TooManyFiles { .. } => ErrorKind::Limit,
            ModProblem::BadJson(_) | ModProblem::BadValue { .. } | ModProblem::Manifest(_) => ErrorKind::Json,
            _ => ErrorKind::Invalid,
        };
        let err = crate::error::Error::new(kind, e.to_string());
        match e.file {
            Some(file) => err.with_path(file),
            None => err,
        }
    }
}

/// The mods a game mounts, in their declared load order.
///
/// ```
/// use ponko::mods::{ModPack, ModSet};
/// let m = |id: &str, requires: &str| {
///     let manifest = format!(r#"{{"format":"ponko-mod","version":1,"id":"{id}","requires":[{requires}]}}"#);
///     ModPack::from_files(id, [("mod.json", manifest)]).unwrap()
/// };
/// // "frost" requires "caves", so "caves" loads first though it was added last.
/// let set = ModSet::new().with(m("frost", r#""caves""#)).with(m("hats", "")).with(m("caves", ""));
/// assert_eq!(set.order().unwrap(), ["hats", "caves", "frost"]);
/// ```
#[derive(Debug, Default)]
pub struct ModSet {
    packs: Vec<ModPack>,
    limits: ModLimits,
    #[cfg(feature = "mods-wasm")]
    code_limits: CodeLimits,
}

impl ModSet {
    /// No mods yet, with the default [`ModLimits`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `pack` after the mods added before it.
    pub fn add(&mut self, pack: ModPack) -> &mut Self {
        self.packs.push(pack);
        self
    }

    /// Adds `pack`, builder style.
    #[must_use]
    pub fn with(mut self, pack: ModPack) -> Self {
        self.packs.push(pack);
        self
    }

    /// Uses `limits` instead of the default ones.
    #[must_use]
    pub fn limits(mut self, limits: ModLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Uses `limits` for the mods' code instead of the default ones.
    #[cfg(feature = "mods-wasm")]
    #[must_use]
    pub fn code_limits(mut self, limits: CodeLimits) -> Self {
        self.code_limits = limits;
        self
    }

    /// The engine mods' code compiles with.
    fn code_engine() -> CodeEngine {
        #[cfg(feature = "mods-wasm")]
        return code::engine();
        #[cfg(not(feature = "mods-wasm"))]
        CodeEngine
    }

    /// The mods' ids in the order they load: the order they were added,
    /// each moved after the mods it `requires` or loads `after`.
    ///
    /// # Errors
    /// Two mods with one id, a required mod missing, or a cycle; the error
    /// names the mod.
    pub fn order(&self) -> Result<Vec<&str>, ModError> {
        self.order_indices()
            .map(|order| order.into_iter().map(|i| self.packs[i].id()).collect())
    }

    fn order_indices(&self) -> Result<Vec<usize>, ModError> {
        let mut index: HashMap<&str, usize> = HashMap::with_capacity(self.packs.len());
        for (i, pack) in self.packs.iter().enumerate() {
            if index.insert(pack.id(), i).is_some() {
                return Err(ModError::new(
                    pack.id(),
                    Some(MANIFEST),
                    ModProblem::DuplicateId(pack.id().into()),
                ));
            }
        }
        // before[i]: the mods that load before mod i.
        let mut before: Vec<Vec<usize>> = Vec::with_capacity(self.packs.len());
        for pack in &self.packs {
            let mut list = Vec::new();
            for id in &pack.manifest.requires {
                let Some(&j) = index.get(id.as_str()) else {
                    return Err(ModError::new(
                        pack.id(),
                        Some(MANIFEST),
                        ModProblem::MissingDependency(id.clone()),
                    ));
                };
                list.push(j);
            }
            list.extend(
                pack.manifest
                    .after
                    .iter()
                    .filter_map(|id| index.get(id.as_str()).copied()),
            );
            before.push(list);
        }
        // Each step places the first-added mod whose mods-before are placed.
        let mut placed = vec![false; self.packs.len()];
        let mut order = Vec::with_capacity(self.packs.len());
        while order.len() < self.packs.len() {
            let next = (0..self.packs.len()).find(|&i| !placed[i] && before[i].iter().all(|&j| placed[j]));
            let Some(i) = next else {
                let chain = self.cycle(&before, &placed);
                let first = chain[0];
                return Err(ModError::new(
                    self.packs[first].id(),
                    Some(MANIFEST),
                    ModProblem::Cycle(chain.iter().map(|&i| self.packs[i].id().to_string()).collect()),
                ));
            };
            placed[i] = true;
            order.push(i);
        }
        Ok(order)
    }

    /// A cycle among the mods not placed: each waits for the next.
    fn cycle(&self, before: &[Vec<usize>], placed: &[bool]) -> Vec<usize> {
        // Every unplaced mod waits for an unplaced one; walk until one repeats.
        let mut at = (0..placed.len()).find(|&i| !placed[i]).unwrap_or(0);
        let mut seen: Vec<usize> = Vec::new();
        loop {
            if let Some(start) = seen.iter().position(|&i| i == at) {
                let mut chain = seen[start..].to_vec();
                chain.push(at);
                return chain;
            }
            seen.push(at);
            match before[at].iter().copied().find(|&j| !placed[j]) {
                Some(j) => at = j,
                None => return seen,
            }
        }
    }

    /// Checks every mod as [`mount`](Self::mount) does, without mounting
    /// anything: for a mods menu that shows which mods can be turned on.
    /// Returns every problem found, at most one per mod, ordering problems
    /// first; empty if the set would mount. Missing assets are checked by
    /// [`mount`](Self::mount) only (it needs the game's assets), and code
    /// is compiled but not started.
    pub fn problems(&self, world: &World) -> Vec<ModError> {
        let mut out = Vec::new();
        if let Err(e) = self.order_indices() {
            out.push(e);
        }
        let engine = Self::code_engine();
        for pack in &self.packs {
            if let Err(e) = pack.check(world, &self.limits, &engine) {
                out.push(e);
            }
        }
        out
    }

    /// Checks every mod, then mounts them all in load order: from now on
    /// assets load from the mods first (the last mod with a name wins),
    /// then from the game's own files. [`Mods`] is inserted as a resource:
    /// which mods are on, which mod a file comes from, and the files under
    /// a folder (to find what mods added).
    ///
    /// Mount in setup, after reflecting the game's components and before
    /// loading assets the mods may replace: an asset loaded before is
    /// cached and stays as it was. Mods are mounted once per game.
    ///
    /// With code mods (feature `mods-wasm`), every mod's module is compiled
    /// and started before anything is mounted, and [`ModCode`] is inserted
    /// as a resource when at least one mod has code.
    ///
    /// # Errors
    /// The first problem found (see [`problems`](Self::problems)), or the
    /// assets a mod names that are missing (all of that mod's, in one
    /// [`ModProblem::MissingAssets`]); nothing is mounted then, and the game
    /// runs as without mods.
    pub fn mount(self, ctx: &mut Ctx) -> Result<(), ModError> {
        if ctx.world.get_resource::<Mods>().is_some() || ctx.assets.has_source(SOURCE_ID) {
            return Err(ModError::new(SOURCE_ID, None, ModProblem::AlreadyMounted));
        }
        let order = self.order_indices()?;
        let engine = Self::code_engine();
        let mut checked = Vec::with_capacity(self.packs.len());
        for &i in &order {
            checked.push(self.packs[i].check(&ctx.world, &self.limits, &engine)?);
        }
        // Every asset the mods' files name: in a mod of the set, or the game's.
        let all: std::collections::HashSet<&str> = checked
            .iter()
            .flat_map(|c| c.files.iter().map(|(name, _, _)| name.as_str()))
            .collect();
        for (&i, files) in order.iter().zip(&checked) {
            let missing = missing_assets(ctx, &all, &files.files);
            if !missing.is_empty() {
                let first = missing[0].0.clone();
                return Err(ModError::new(
                    self.packs[i].id(),
                    Some(&first),
                    ModProblem::MissingAssets(missing),
                ));
            }
        }
        #[cfg(feature = "mods-wasm")]
        let code = {
            let modules: Vec<(String, wasmi::Module)> = order
                .iter()
                .zip(&mut checked)
                .filter_map(|(&i, c)| Some((self.packs[i].id().to_string(), c.module.take()?)))
                .collect();
            if modules.is_empty() {
                None
            } else {
                Some(ModCode::start(&engine, modules, self.code_limits)?)
            }
        };
        let checked: Vec<Vec<CheckedFile>> = checked.into_iter().map(|c| c.files).collect();
        // Everything is fine: build the source and the resource.
        // The packs in load order: each sorted by its place in `order`.
        let mut rank = vec![0; order.len()];
        for (place, &i) in order.iter().enumerate() {
            rank[i] = place;
        }
        let mut packs: Vec<(usize, ModPack)> = self.packs.into_iter().enumerate().map(|(i, p)| (rank[i], p)).collect();
        packs.sort_by_key(|&(place, _)| place);
        let mut source = ModSource {
            files: BTreeMap::new(),
            mods: Vec::with_capacity(order.len()),
        };
        let mut info = Vec::with_capacity(order.len());
        let mut hash = Fnv::new();
        for (slot, ((_, pack), files)) in packs.into_iter().zip(checked).enumerate() {
            hash.str(pack.id());
            hash.str(&pack.manifest.mod_version);
            for (name, size, json) in &files {
                hash.str(name);
                hash.bytes(&size.to_le_bytes());
                if let Some(bytes) = json {
                    hash.bytes(bytes);
                }
                // The manifest describes the mod; it isn't one of its assets.
                if name != MANIFEST {
                    source.files.insert(name.clone(), slot);
                }
            }
            info.push(ModInfo {
                id: pack.manifest.id.clone(),
                name: pack.manifest.name.clone(),
                mod_version: pack.manifest.mod_version.clone(),
                origin: pack.origin.clone(),
                files: files.len(),
            });
            source.mods.push(pack);
        }
        let mods = Mods {
            files: source.files.iter().map(|(n, &s)| (n.clone(), s)).collect(),
            list: info,
            fingerprint: hash.0,
        };
        ctx.assets.add_background_source(source);
        ctx.world.insert_resource(mods);
        #[cfg(feature = "mods-wasm")]
        if let Some(code) = code {
            ctx.world.insert_resource(code);
        }
        Ok(())
    }
}

/// The mounted mods' files as one asset source: each name from the last
/// mod in load order that has it.
struct ModSource {
    files: BTreeMap<String, usize>,
    mods: Vec<ModPack>,
}

impl AssetSource for ModSource {
    fn id(&self) -> &str {
        SOURCE_ID
    }

    fn contains(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    fn size(&self, name: &str) -> Option<u64> {
        let &slot = self.files.get(name)?;
        self.mods[slot].files.size(name)
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        match self.files.get(name) {
            Some(&slot) => self.mods[slot].files.read(name),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("\"{name}\" is in no mod"),
            )),
        }
    }
}

/// One mounted mod, as [`Mods::list`] shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModInfo {
    /// Its id.
    pub id: String,
    /// Its name for people.
    pub name: String,
    /// Its own version (empty if its manifest gives none).
    pub mod_version: String,
    /// Where it came from: the pack's file name, or the name it was given.
    pub origin: String,
    /// How many files it has, its manifest included.
    pub files: usize,
}

/// The mounted mods: a resource [`ModSet::mount`] inserts.
///
/// ```
/// use ponko::mods::{ModPack, ModSet, Mods};
/// use ponko::prelude::*;
/// let manifest = r#"{"format":"ponko-mod","version":1,"id":"more-enemies"}"#;
/// let pack = ModPack::from_files("more", [("mod.json", manifest), ("enemies/bat.json", "{}"), ("enemies/rat.json", "{}")]).unwrap();
/// Game::new("modded", 1, 1).run_headless(0, |ctx| {
///     ModSet::new().with(pack).mount(ctx).unwrap();
///     let mods = ctx.world.resource::<Mods>();
///     assert_eq!(mods.list()[0].id, "more-enemies");
///     assert_eq!(mods.files_under("enemies/").collect::<Vec<_>>(), ["enemies/bat.json", "enemies/rat.json"]);
///     assert_eq!(mods.provider("enemies/bat.json"), Some("more-enemies"));
/// });
/// ```
#[derive(Debug, Clone)]
pub struct Mods {
    list: Vec<ModInfo>,
    /// Every file name, with the place in `list` of the mod it comes from.
    files: BTreeMap<String, usize>,
    fingerprint: u64,
}

impl Mods {
    /// The mods, in load order.
    pub fn list(&self) -> &[ModInfo] {
        &self.list
    }

    /// The id of the mod file `name` loads from (the last in load order
    /// that has it), or `None` if no mod has it.
    pub fn provider(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(|&i| self.list[i].id.as_str())
    }

    /// The names of the mods' files that start with `prefix`, sorted, each
    /// once: how a game finds what mods added (`"enemies/"`).
    pub fn files_under<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.files
            .range::<str, _>((std::ops::Bound::Included(prefix), std::ops::Bound::Unbounded))
            .map(|(n, _)| n.as_str())
            .take_while(move |n| n.starts_with(prefix))
    }

    /// A 64-bit hash of the mods, their order, their versions, their files'
    /// names and sizes and the bytes of their `.json` files. Two games with
    /// the same fingerprint read the same data from their mods; lockstep
    /// peers and replays compare it. Art and sound are covered by name and
    /// size only.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// FNV-1a, 64 bits: small, stable across machines and versions.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn str(&mut self, s: &str) {
        self.bytes(&(s.len() as u64).to_le_bytes());
        self.bytes(s.as_bytes());
    }
}
