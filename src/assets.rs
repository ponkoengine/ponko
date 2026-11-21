//! [`Assets`]: asset files, as `ctx.assets`.
//!
//! **What lives here.** Where named assets are read from
//! ([`Assets::add_source`], [`AssetSource`], [`MemorySource`]), your own
//! asset types ([`Assets::load_with`]), data files that fill resources and
//! components and follow their files ([`Assets::load_resource`] and the
//! `load_*_json` calls), saving scenes and prefabs
//! ([`Assets::save_scene`], [`Assets::save_prefab`]), and what hot reload
//! changed this frame ([`Assets::reloaded_files`],
//! [`Assets::reloaded_textures`]). Textures, fonts, meshes and shaders load
//! through `ctx.gfx` and sounds through `ctx.audio`; they read the same
//! sources and are watched by the same hot reload.
//! [`Ctx::load_scene`](crate::ctx::Ctx::load_scene) and
//! [`Ctx::spawn_prefab`](crate::ctx::Ctx::spawn_prefab) stay on `Ctx`: they
//! replace or extend the world, load through every service and reset
//! commands and physics.
//!
//! **When to call it.** Add sources first, in setup, before loading what
//! they hold. Load data files in setup too; with hot reload on they are
//! applied again when their files change, at the start of a frame. Ask for
//! the reloaded files from a frame or simulation system. Save from any
//! system.
//!
//! **What it needs.** No window, no GPU and no audio device. Files come
//! from the added sources, then the `assets/` folders on desktop (the
//! files fetched from `assets/index.json` in a browser); saving writes to
//! `assets/`, and in web builds only with the `editor` feature, which keeps
//! saved files in the browser's storage. The calls that read or
//! write entities and resources take the world (`&mut ctx.world`), so
//! `ctx.assets` borrows apart from it.
//!
//! ```
//! use ponko::assets::MemorySource;
//! use ponko::prelude::*;
//!
//! #[derive(serde::Deserialize)]
//! struct Rules {
//!     speed: f32,
//! }
//!
//! Game::new("assets", 1, 1).run_headless(0, |ctx| {
//!     ctx.assets
//!         .add_source(MemorySource::new("built-in").with("rules.json", r#"{"speed": 30}"#));
//!     ctx.assets.load_resource::<Rules>(&mut ctx.world, "rules.json");
//!     assert_eq!(ctx.world.resource::<Rules>().speed, 30.0);
//! });
//! ```
//!
//! # Finding and decoding
//!
//! Loading a named asset has three steps, kept apart here and in the
//! services that load:
//! 1. **Acquire** its bytes by name: from the [`AssetSource`]s a game added,
//!    in order, then from the default source (the `assets/` folders on
//!    desktop, the files fetched from `assets/index.json` in a browser).
//! 2. **Decode** them on the CPU (`decode_image`): no GPU, any thread.
//! 3. **Create** the GPU resource on the game thread (`Graphics::create_texture`).
#![deny(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ponko_ecs::{Entity, World};
use serde_json::{Value, json};

use crate::hot_reload::Asset;
use crate::reflect::WorldReflect;
use crate::scene::{PREFAB_FORMAT, PREFAB_VERSION};

pub(crate) mod group;
mod pack;
pub use group::{
    AssetGroup, DEFAULT_GROUP_FRAME_BUDGET_MS, GroupId, GroupProgress, GroupState, GroupStats, SliceLoader,
};
pub use pack::PackSource;
#[cfg(all(target_arch = "wasm32", feature = "window"))]
pub(crate) use pack::{parse_header as parse_pack_header, set_web_pack};

/// A place named assets can be read from, other than the `assets/` folder:
/// bytes embedded in the game, a pack file, a test fixture in memory.
///
/// Add one with [`Assets::add_source`].
/// Sources are asked in the order they were added, before the default
/// `assets/` search; a source that doesn't have a name says so with an
/// error of kind [`std::io::ErrorKind::NotFound`], and the next one is
/// asked. Any other error stops the search and is reported.
///
/// [`MemorySource`] is a ready-made one.
///
/// # Names, caching and hot reload
///
/// Names are the same logical names the default source uses
/// (`"sprites/crab.png"`, `/`-separated), and scenes and prefabs refer to
/// assets by them. A loaded asset is cached by its source's
/// [`id`](Self::id) and its name, so the same name in two sources is two
/// assets, and loading a name again returns the first handle without
/// reading the source again. This holds for every kind of asset: textures,
/// fonts, data, scenes, prefabs, sounds, streamed music and (with the `3d`
/// feature) meshes, skeletons, animation clips, shaders, screen effects and
/// compute passes. Images a glTF file refers to are shared between models
/// by the source they were found in, so two sources with the same image
/// name give two images. Texture arrays, reflection probe images and
/// [`load_asset_with`](crate::assets::Assets::load_with) read their source on
/// every call (they aren't cached).
///
/// # In a browser
///
/// Added sources work the same in web builds: they are asked first, and
/// what they have is never fetched. The default source there is the files
/// listed in `assets/index.json`, fetched before setup.
///
/// # Hot reload
///
/// Only files found in the default source on desktop are watched and
/// reloaded when they change (textures, fonts, sounds, data, shaders,
/// effects, compute). Assets that came from an added source are loaded once
/// and never reloaded, whatever the source does later, so a source can keep
/// its bytes in memory without being polled. The same name found in `assets/`
/// before a source was added keeps being watched.
///
/// `read` runs on the game thread, inside the load call that needs it.
pub trait AssetSource: 'static {
    /// A short name for this source, unique among the sources added to a
    /// game: part of every cache key and error message.
    fn id(&self) -> &str;

    /// The bytes of asset `name`, or an error of kind
    /// [`NotFound`](std::io::ErrorKind::NotFound) if this source doesn't
    /// have it.
    ///
    /// # Errors
    ///
    /// `NotFound` lets the next source try; any other error is reported to
    /// the caller of the load.
    fn read(&self, name: &str) -> std::io::Result<Vec<u8>>;

    /// Whether this source has asset `name`, without handing over its
    /// bytes. Used to tell which source a file a model refers to comes
    /// from, so it is shared with what was loaded from that source before
    /// instead of read again.
    ///
    /// The default reads it and checks the error is not
    /// [`NotFound`](std::io::ErrorKind::NotFound); override it when that is
    /// cheaper to answer (an index lookup).
    fn contains(&self, name: &str) -> bool {
        !matches!(self.read(name), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    }

    /// The size of asset `name` in bytes, if this source can tell without
    /// reading it; `None` if it can't (the default) or doesn't have it.
    /// [Asset groups](crate::assets::AssetGroup) use it to show progress in
    /// bytes before the files are read.
    ///
    /// ```
    /// use ponko::assets::{AssetSource, MemorySource};
    /// let source = MemorySource::new("embedded").with("a.txt", "hello");
    /// assert_eq!(source.size("a.txt"), Some(5));
    /// assert_eq!(source.size("b.txt"), None);
    /// ```
    fn size(&self, name: &str) -> Option<u64> {
        let _ = name;
        None
    }
}

/// An [`AssetSource`] holding its files in memory: assets embedded with
/// `include_bytes!`, generated at startup, or made up in a test.
///
/// ```
/// use ponko::assets::MemorySource;
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let mut pixel = Vec::new();
///     image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
///         .write_to(&mut std::io::Cursor::new(&mut pixel), image::ImageFormat::Png)
///         .unwrap();
///     ctx.assets.add_source(MemorySource::new("embedded").with("red.png", pixel));
///     let red = ctx.gfx.load_texture("red.png");
///     assert_eq!((red.width(), red.height()), (1, 1));
/// });
/// ```
#[derive(Clone, Default)]
pub struct MemorySource {
    id: String,
    files: HashMap<String, Arc<[u8]>>,
}

impl MemorySource {
    /// An empty source called `id`.
    ///
    /// ```
    /// use ponko::assets::{AssetSource, MemorySource};
    /// let source = MemorySource::new("embedded");
    /// assert_eq!(source.id(), "embedded");
    /// assert!(!source.contains("a.txt")); // empty to begin with
    /// ```
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            files: HashMap::new(),
        }
    }

    /// Adds (or replaces) file `name`, builder style.
    ///
    /// ```
    /// use ponko::assets::{AssetSource, MemorySource};
    /// let source = MemorySource::new("embedded").with("a.txt", "hello").with("b.bin", vec![1, 2, 3]);
    /// assert_eq!(source.read("a.txt").unwrap(), b"hello");
    /// assert!(source.contains("b.bin"));
    /// ```
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.insert(name, bytes);
        self
    }

    /// Adds (or replaces) file `name`.
    ///
    /// ```
    /// use ponko::assets::{AssetSource, MemorySource};
    /// let mut source = MemorySource::new("embedded");
    /// source.insert("a.txt", "one");
    /// source.insert("a.txt", "two"); // replaces
    /// assert_eq!(source.read("a.txt").unwrap(), b"two");
    /// ```
    pub fn insert(&mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(name.into(), bytes.into().into());
    }
}

impl AssetSource for MemorySource {
    fn id(&self) -> &str {
        &self.id
    }

    fn contains(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    fn size(&self, name: &str) -> Option<u64> {
        self.files.get(name).map(|b| b.len() as u64)
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        self.files.get(name).map(|b| b.to_vec()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("\"{name}\" is not in memory source \"{}\"", self.id),
            )
        })
    }
}

/// Where a loaded asset came from; with its kind, the cache key.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Origin {
    /// The default source: the file [`resolve`] found (on the web, the
    /// fetched copy's name). The only kind hot reload watches.
    File(PathBuf),
    /// An added [`AssetSource`], by its id, and the logical name.
    Source { source: String, name: String },
}

impl Origin {
    /// `name` for error messages, with where it came from.
    pub(crate) fn describe(&self, name: &str) -> String {
        match self {
            Origin::File(_) => format!("\"{name}\""),
            Origin::Source { source, .. } => format!("\"{name}\" (from asset source \"{source}\")"),
        }
    }

    /// Replaces a listing name, retaining its capacity.
    #[cfg(feature = "3d")]
    pub(crate) fn display_into(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();
        match self {
            Origin::File(path) => {
                let _ = write!(out, "{}", path.display());
            }
            Origin::Source { source, name } => {
                let _ = write!(out, "{source}:{name}");
            }
        }
    }
}

/// What [`Sources::lookup`] found for a name.
pub(crate) enum Lookup<T> {
    /// The asset was loaded before: where it came from (3D files need it to
    /// find their skeleton and clips), and what it became.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    Cached(Origin, T),
    /// Not loaded yet.
    Found(Found),
}

/// A named asset that was found: where, and its bytes if reading it was
/// how it was found (added sources).
pub(crate) struct Found {
    pub origin: Origin,
    bytes: Option<Vec<u8>>,
}

impl Found {
    /// The asset's bytes, reading the file now if it came from the default source.
    /// A failed read is [`ErrorKind::Io`](crate::error::ErrorKind::Io).
    pub(crate) fn into_bytes(self) -> Result<Vec<u8>, crate::error::Error> {
        match (self.bytes, &self.origin) {
            (Some(bytes), _) => Ok(bytes),
            (None, Origin::File(path)) => read(path).map_err(|e| {
                crate::error::Error::new(
                    crate::error::ErrorKind::Io,
                    format!("could not read {}: {e}", path.display()),
                )
            }),
            (None, Origin::Source { .. }) => unreachable!("sources hand over their bytes when found"),
        }
    }

    /// The asset as UTF-8 text; `name` is for messages and the error's path.
    /// Bytes that aren't UTF-8 are [`ErrorKind::Decode`](crate::error::ErrorKind::Decode).
    pub(crate) fn into_text(self, name: &str) -> Result<String, crate::error::Error> {
        let origin = self.origin.clone();
        let bytes = self.into_bytes().map_err(|e| e.with_path(name))?;
        String::from_utf8(bytes).map_err(|e| {
            let message = match &origin {
                Origin::File(path) => format!("{} is not UTF-8 text: {e}", path.display()),
                Origin::Source { .. } => format!("{} is not UTF-8 text: {e}", origin.describe(name)),
            };
            crate::error::Error::decode(name, message)
        })
    }
}

/// An added source: read on the game thread only, or (added with
/// [`Assets::add_background_source`]) by worker threads too.
pub(crate) enum Slot {
    Game(Box<dyn AssetSource>),
    Background(Arc<dyn AssetSource + Send + Sync>),
}

impl Slot {
    fn get(&self) -> &dyn AssetSource {
        match self {
            Slot::Game(source) => source.as_ref(),
            Slot::Background(source) => source.as_ref(),
        }
    }
}

/// The packs read after the loose files, as a worker can hold them.
#[derive(Clone)]
pub(crate) enum Packs {
    /// `assets.pack` beside the asset folders (the page's pack on the web).
    Default(&'static [PackSource]),
    /// Packs a test put in place of the default ones.
    #[cfg(test)]
    Owned(Arc<Vec<PackSource>>),
}

impl std::ops::Deref for Packs {
    type Target = [PackSource];
    fn deref(&self) -> &[PackSource] {
        match self {
            Packs::Default(packs) => packs,
            #[cfg(test)]
            Packs::Owned(packs) => packs,
        }
    }
}

/// The asset sources a game added, asked before the default one.
#[derive(Default)]
pub(crate) struct Sources {
    list: Vec<Slot>,
    /// The packs read after the loose files, in place of the default ones
    /// (`assets.pack` beside the asset folders): for tests.
    #[cfg(test)]
    packs: Option<Arc<Vec<PackSource>>>,
}

impl Sources {
    /// The packs read after the loose files: `assets.pack` beside the
    /// asset folders (the page's pack on the web).
    fn packs(&self) -> &[PackSource] {
        #[cfg(test)]
        if let Some(packs) = &self.packs {
            return packs;
        }
        pack::default_packs()
    }

    /// [`packs`](Self::packs), to hand to a worker.
    fn worker_packs(&self) -> Packs {
        #[cfg(test)]
        if let Some(packs) = &self.packs {
            return Packs::Owned(Arc::clone(packs));
        }
        Packs::Default(pack::default_packs())
    }

    /// Reads `packs` instead of the default packs (tests).
    #[cfg(test)]
    pub(crate) fn with_packs(packs: Vec<PackSource>) -> Sources {
        Sources {
            list: Vec::new(),
            packs: Some(Arc::new(packs)),
        }
    }

    /// Whether a source with this id was added.
    pub(crate) fn has(&self, id: &str) -> bool {
        self.list.iter().any(|s| s.get().id() == id)
    }

    /// Whether no source was added (only the default one is searched).
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Adds `source` after the others.
    ///
    /// # Panics
    /// If its id is empty or another source already has it.
    pub(crate) fn add(&mut self, source: Slot) {
        let id = source.get().id();
        assert!(!id.is_empty(), "ponko: an asset source needs a non-empty id");
        assert!(
            self.list.iter().all(|s| s.get().id() != id),
            "ponko: an asset source called \"{id}\" was already added; choose a unique AssetSource::id()"
        );
        self.list.push(source);
    }

    /// The cache keys `name` would have in each added source, in order.
    pub(crate) fn origins<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Origin> + 'a {
        self.list.iter().map(move |s| Origin::Source {
            source: s.get().id().to_string(),
            name: name.to_string(),
        })
    }

    /// Finds asset `name` unless `cached` already has it from where it
    /// would be found. Sources are only read for what isn't cached, so
    /// loading a name again never reads a source again.
    ///
    /// # Errors
    /// As [`find`](Self::find).
    pub(crate) fn lookup<T>(
        &self,
        name: &str,
        cached: impl Fn(&Origin) -> Option<T>,
    ) -> Result<Lookup<T>, crate::error::Error> {
        for origin in self.origins(name) {
            if let Some(value) = cached(&origin) {
                return Ok(Lookup::Cached(origin, value));
            }
        }
        // A name from a pack (no loose file): cached without unpacking it again.
        let packs = self.packs();
        if !packs.is_empty()
            && resolve(name).is_err()
            && let Some(pack) = packs.iter().find(|p| p.contains(name))
        {
            let origin = Origin::Source {
                source: pack.id().to_string(),
                name: name.to_string(),
            };
            if let Some(value) = cached(&origin) {
                return Ok(Lookup::Cached(origin, value));
            }
        }
        let found = self.find(name)?;
        Ok(match cached(&found.origin) {
            Some(value) => Lookup::Cached(found.origin, value),
            None => Lookup::Found(found),
        })
    }

    /// The bytes of `name` from the first place that has it, for files an
    /// asset refers to (a glTF's images, say).
    ///
    /// # Errors
    /// As [`find`](Self::find), or if reading fails.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        Ok(self.find(name)?.into_bytes()?)
    }

    /// Where asset `name` would be found, without reading it: the first
    /// added source that [has](AssetSource::contains) it, else the default
    /// source's file. `None` if nothing has it.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn locate(&self, name: &str) -> Option<Origin> {
        match self.list.iter().map(Slot::get).find(|s| s.contains(name)) {
            Some(source) => Some(Origin::Source {
                source: source.id().to_string(),
                name: name.to_string(),
            }),
            None => resolve(name).ok().map(Origin::File).or_else(|| {
                self.packs()
                    .iter()
                    .find(|p| p.contains(name))
                    .map(|pack| Origin::Source {
                        source: pack.id().to_string(),
                        name: name.to_string(),
                    })
            }),
        }
    }

    /// Finds asset `name`: in the added sources in order, then the default one.
    ///
    /// # Errors
    /// If no source has it (naming every place searched), or a source
    /// failed to read it.
    /// Not finding it is [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound);
    /// a source that has it but fails to read it is
    /// [`Io`](crate::error::ErrorKind::Io). The error names `name` as its path.
    pub(crate) fn find(&self, name: &str) -> Result<Found, crate::error::Error> {
        search(name, self.list.iter().map(Slot::get), self.packs(), &|| self.ids())
    }

    /// The ids of the added sources, quoted, for messages.
    fn ids(&self) -> Vec<String> {
        self.list.iter().map(|s| format!("\"{}\"", s.get().id())).collect()
    }

    /// How to read asset `name` without blocking the game thread where it
    /// can be helped: the sources only the game thread may read, before the
    /// first background one, are read now; the rest of the search (the
    /// background sources, `assets/`, the packs) is handed back to run on a
    /// worker. With a game-thread source after a background one the whole
    /// search runs now, so the order sources are asked in never changes.
    pub(crate) fn plan(&self, name: &str) -> ReadPlan {
        let first = self
            .list
            .iter()
            .position(|s| matches!(s, Slot::Background(_)))
            .unwrap_or(self.list.len());
        if self.list[first..].iter().any(|s| matches!(s, Slot::Game(_))) {
            return ReadPlan::Now(self.find(name).and_then(|found| {
                let origin = found.origin.clone();
                Ok((origin, found.into_bytes().map_err(|e| e.with_path(name))?))
            }));
        }
        for source in self.list[..first].iter().map(Slot::get) {
            match source.read(name) {
                Ok(bytes) => {
                    let origin = Origin::Source {
                        source: source.id().to_string(),
                        name: name.to_string(),
                    };
                    return ReadPlan::Now(Ok((origin, bytes)));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    let message = format!("asset source \"{}\" could not read \"{name}\": {e}", source.id());
                    return ReadPlan::Now(Err(
                        crate::error::Error::new(crate::error::ErrorKind::Io, message).with_path(name)
                    ));
                }
            }
        }
        ReadPlan::Later(BackgroundRead {
            name: name.to_string(),
            sources: self.list[first..]
                .iter()
                .filter_map(|s| match s {
                    Slot::Background(source) => Some(Arc::clone(source)),
                    Slot::Game(_) => None,
                })
                .collect(),
            packs: self.worker_packs(),
            ids: self.ids(),
        })
    }

    /// Whether every added source may be read by a worker.
    pub(crate) fn all_background(&self) -> bool {
        self.list.iter().all(|s| matches!(s, Slot::Background(_)))
    }
}

/// Finds asset `name`: in `sources` in order, then the default source,
/// then `packs`; `ids` lists the added sources for the message.
fn search<'a>(
    name: &str,
    sources: impl Iterator<Item = &'a dyn AssetSource>,
    packs: &[PackSource],
    ids: &dyn Fn() -> Vec<String>,
) -> Result<Found, crate::error::Error> {
    for source in sources {
        match source.read(name) {
            Ok(bytes) => {
                return Ok(Found {
                    origin: Origin::Source {
                        source: source.id().to_string(),
                        name: name.to_string(),
                    },
                    bytes: Some(bytes),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                let message = format!("asset source \"{}\" could not read \"{name}\": {e}", source.id());
                return Err(crate::error::Error::new(crate::error::ErrorKind::Io, message).with_path(name));
            }
        }
    }
    let e = match resolve(name) {
        Ok(path) => {
            return Ok(Found {
                origin: Origin::File(path),
                bytes: None,
            });
        }
        Err(e) => e,
    };
    // Loose files win over the packs, so a file can be patched (and
    // hot reloaded) beside a pack.
    for pack in packs {
        if pack.contains(name) {
            return match pack.read(name) {
                Ok(bytes) => Ok(Found {
                    origin: Origin::Source {
                        source: pack.id().to_string(),
                        name: name.to_string(),
                    },
                    bytes: Some(bytes),
                }),
                Err(err) => {
                    let message = format!("asset pack {} could not read \"{name}\": {err}", pack.id());
                    Err(crate::error::Error::new(crate::error::ErrorKind::Io, message).with_path(name))
                }
            };
        }
    }
    let mut message = e;
    if !packs.is_empty() {
        let ids: Vec<&str> = packs.iter().map(|p| p.id()).collect();
        message.push_str(&format!("\nAsset packs searched after them: {}", ids.join(", ")));
    }
    let ids = ids();
    if !ids.is_empty() {
        message.push_str(&format!("\nAsset sources searched first: {}", ids.join(", ")));
    }
    Err(crate::error::Error::not_found(name, message))
}

/// What [`Sources::plan`] found: an asset read now, on the game thread, or
/// the rest of its search, for a worker.
pub(crate) enum ReadPlan {
    /// Read (or failed) on the game thread: where it came from, its bytes.
    Now(Result<(Origin, Vec<u8>), crate::error::Error>),
    /// To read on a worker.
    Later(BackgroundRead),
}

/// The rest of an asset's search, which any thread may run: the background
/// sources, the default source and the packs.
#[derive(Clone)]
pub(crate) struct BackgroundRead {
    name: String,
    sources: Vec<Arc<dyn AssetSource + Send + Sync>>,
    packs: Packs,
    /// Every added source's id, for the message when nothing has it.
    ids: Vec<String>,
}

impl BackgroundRead {
    /// Reads the asset: where it came from and its bytes. Errors name it as
    /// their path, as [`Sources::find`]'s do.
    pub(crate) fn read(&self) -> Result<(Origin, Vec<u8>), crate::error::Error> {
        let found = search(
            &self.name,
            self.sources.iter().map(|s| s.as_ref() as &dyn AssetSource),
            &self.packs,
            &|| self.ids.clone(),
        )?;
        let origin = found.origin.clone();
        Ok((origin, found.into_bytes().map_err(|e| e.with_path(&self.name))?))
    }

    /// Reads another asset the same way: a file this one refers to.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn read_other(&self, name: &str) -> Result<Vec<u8>, String> {
        let found = search(
            name,
            self.sources.iter().map(|s| s.as_ref() as &dyn AssetSource),
            &self.packs,
            &|| self.ids.clone(),
        )?;
        Ok(found.into_bytes()?)
    }

    /// The asset's size in bytes, where the place that has it can tell
    /// without reading it: [`AssetSource::size`], the file's length, the
    /// pack's index.
    pub(crate) fn size(&self) -> Option<u64> {
        let name = self.name.as_str();
        if let Some(source) = self.sources.iter().find(|s| s.contains(name)) {
            return source.size(name);
        }
        if let Ok(path) = resolve(name) {
            return file_size(&path);
        }
        self.packs.iter().find(|p| p.contains(name)).and_then(|p| p.size(name))
    }
}

/// The length of an asset file found by [`resolve`].
#[cfg(not(target_arch = "wasm32"))]
fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// The length of a fetched asset file (web builds).
#[cfg(target_arch = "wasm32")]
fn file_size(path: &Path) -> Option<u64> {
    let name = path.to_string_lossy();
    web_files::with(|files| files.get(name.as_ref()).map(|b| b.len() as u64))
}

/// The asset [`Sources`] of a game, shared by every service that loads
/// files (`ctx.assets` adds to them; `ctx.gfx` and `ctx.audio` read through
/// them). One game thread uses them, so a `RefCell` is enough.
#[derive(Clone, Default)]
pub(crate) struct SharedSources(std::rc::Rc<std::cell::RefCell<Sources>>);

impl SharedSources {
    /// As [`Sources::add`].
    pub(crate) fn add(&self, source: Slot) {
        self.0.borrow_mut().add(source);
    }

    /// As [`Sources::plan`].
    pub(crate) fn plan(&self, name: &str) -> ReadPlan {
        self.0.borrow().plan(name)
    }

    /// As [`Sources::all_background`].
    pub(crate) fn all_background(&self) -> bool {
        self.0.borrow().all_background()
    }

    /// As [`Sources::lookup`].
    pub(crate) fn lookup<T>(
        &self,
        name: &str,
        cached: impl Fn(&Origin) -> Option<T>,
    ) -> Result<Lookup<T>, crate::error::Error> {
        self.0.borrow().lookup(name, cached)
    }

    /// As [`Sources::read`].
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        self.0.borrow().read(name)
    }

    /// As [`Sources::find`].
    pub(crate) fn find(&self, name: &str) -> Result<Found, crate::error::Error> {
        self.0.borrow().find(name)
    }

    /// As [`Sources::locate`].
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn locate(&self, name: &str) -> Option<Origin> {
        self.0.borrow().locate(name)
    }

    /// As [`Sources::has`].
    pub(crate) fn has(&self, id: &str) -> bool {
        self.0.borrow().has(id)
    }

    /// As [`Sources::is_empty`].
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

/// Folders we look in for `assets/`, in order:
/// 1. next to `Cargo.toml` when started with `cargo run` (`CARGO_MANIFEST_DIR`)
/// 2. next to the executable (shipped games)
/// 3. the bundle's `Contents/Resources` when the executable is in a macOS
///    app bundle (`Name.app/Contents/MacOS/`), where `ponko package` puts them
/// 4. the current working directory
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn asset_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
        roots.push(PathBuf::from(dir).join("assets"));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        roots.push(dir.join("assets"));
        if let Some(resources) = bundle_resources(&dir) {
            roots.push(resources.join("assets"));
        }
    }
    if let Ok(dir) = std::env::current_dir() {
        roots.push(dir.join("assets"));
    }
    // The working directory is often the manifest directory too: list each
    // folder once, in search order.
    let mut seen = std::collections::HashSet::new();
    roots.retain(|root| seen.insert(root.clone()));
    roots
}

/// The `Contents/Resources` folder of the macOS app bundle whose
/// `Contents/MacOS` folder is `exe_dir`, if it is one.
#[cfg(not(target_arch = "wasm32"))]
fn bundle_resources(exe_dir: &Path) -> Option<PathBuf> {
    let contents = exe_dir.parent()?;
    let bundle = contents.parent()?;
    let is_bundle =
        exe_dir.file_name()? == "MacOS" && contents.file_name()? == "Contents" && bundle.extension()? == "app";
    is_bundle.then(|| contents.join("Resources"))
}

/// Resolves `name` (relative to `assets/`) to an existing file.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve(name: &str) -> Result<PathBuf, String> {
    let roots = asset_roots();
    for root in &roots {
        let candidate = root.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let searched: Vec<String> = roots.iter().map(|r| format!("  {}", r.display())).collect();
    Err(format!(
        "\"{name}\" not found in assets/. Looked in:\n{}",
        searched.join("\n") + &nearby_assets(name, &roots)
    ))
}

/// Suggestions are bounded to the requested folder, only on lookup failure.
#[cfg(not(target_arch = "wasm32"))]
fn nearby_assets(name: &str, roots: &[PathBuf]) -> String {
    let requested = Path::new(name);
    let filename = requested
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let mut nearby = Vec::new();
    for root in roots {
        let folder = root.join(requested.parent().unwrap_or_else(|| Path::new("")));
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.take(256).flatten() {
                let candidate = entry.file_name().to_string_lossy().into_owned();
                let lower = candidate.to_lowercase();
                let common = lower.chars().zip(filename.chars()).take_while(|(a, b)| a == b).count();
                if lower == filename || common >= 2 || Path::new(&candidate).file_stem() == requested.file_stem() {
                    nearby.push(candidate);
                }
            }
        }
    }
    nearby.sort();
    nearby.dedup();
    nearby.truncate(8);
    format!(
        "\nCheck spelling and case, or place the file under one of these assets/ folders. Nearby names: {}",
        if nearby.is_empty() {
            "(none)".into()
        } else {
            nearby.join(", ")
        }
    )
}

/// Reads an asset file found by [`resolve`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Where [`write`] puts `name` given asset folders `roots`: over the first
/// existing file, else into the first folder that exists (or the first one).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_path(roots: &[PathBuf], name: &str) -> Result<PathBuf, String> {
    if let Some(existing) = roots.iter().map(|r| r.join(name)).find(|p| p.is_file()) {
        return Ok(existing);
    }
    let root = roots
        .iter()
        .find(|r| r.is_dir())
        .or(roots.first())
        .ok_or("no assets/ folder")?;
    Ok(root.join(name))
}

/// Writes `name` (relative to `assets/`): over the file where [`resolve`]
/// finds it, else into the first `assets/` folder that exists (or the first
/// one, created). Returns where it went.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = write_path(&asset_roots(), name)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    // A file a screenshot or trace has claimed (it would remove it, and
    // what is written here, if it's given up).
    if crate::control::is_claimed_path(&path) {
        return Err(format!(
            "could not write {}: a screenshot or trace will be written there; name another file",
            path.display()
        ));
    }
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Files in folder `dir` of `assets/` ending in `.{extension}`, as asset
/// names (`"prefabs/palm.json"`), sorted, from every assets folder found.
#[cfg(all(not(target_arch = "wasm32"), feature = "inspector"))]
pub(crate) fn list(dir: &str, extension: &str) -> Vec<String> {
    let mut names = Vec::new();
    for root in asset_roots() {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path.extension().is_some_and(|e| e == extension)
                && let Some(file) = path.file_name().and_then(|f| f.to_str())
            {
                names.push(format!("{dir}/{file}"));
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Web builds list their file manifest (see [`web_manifest`]).
#[cfg(all(target_arch = "wasm32", feature = "inspector"))]
pub(crate) fn list(dir: &str, extension: &str) -> Vec<String> {
    let prefix = format!("{dir}/");
    let suffix = format!(".{extension}");
    let mut names: Vec<String> = web_manifest()
        .into_iter()
        .filter(|n| n.starts_with(&prefix) && !n[prefix.len()..].contains('/') && n.ends_with(&suffix))
        .collect();
    names.sort();
    names
}

/// The web build's file manifest: the files fetched from the names in
/// `assets/index.json` (with the files the editor saved in the browser),
/// and the files in the page's asset pack. Sorted, each once.
#[cfg(target_arch = "wasm32")]
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) fn web_manifest() -> Vec<String> {
    let mut names: Vec<String> = web_files::with(|files| files.keys().cloned().collect());
    for pack in pack::default_packs() {
        names.extend(pack.names().map(str::to_string));
    }
    names.sort();
    names.dedup();
    names
}

/// The bytes of `name` in the web build's file manifest (a fetched or
/// saved file, else the page's asset pack).
#[cfg(target_arch = "wasm32")]
#[cfg_attr(not(feature = "inspector"), allow(dead_code))]
pub(crate) fn read_web_asset(name: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = web_files::with(|files| files.get(name).map(|b| b.to_vec())) {
        return Some(bytes);
    }
    let pack = pack::default_packs().iter().find(|p| p.contains(name))?;
    pack.read(name).ok()
}

/// Web builds without the editor can't write files.
#[cfg(all(target_arch = "wasm32", not(feature = "editor")))]
pub(crate) fn write(name: &str, _: &[u8]) -> Result<PathBuf, String> {
    Err(format!("can't save \"{name}\": web builds have no files"))
}

/// With the editor, web builds keep what they save in the browser's
/// storage (see `editor::web`): it is read from there from now on, and
/// again when the page is loaded next.
#[cfg(all(target_arch = "wasm32", feature = "editor"))]
pub(crate) fn write(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    crate::editor::web::keep(name, bytes)?;
    web_files::insert(name.to_string(), bytes.to_vec());
    Ok(PathBuf::from(name))
}

/// [`write`] for the public save calls: a failure is
/// [`ErrorKind::Io`](crate::error::ErrorKind::Io) naming `name`, or
/// [`Unsupported`](crate::error::ErrorKind::Unsupported) in web builds without
/// the editor, which have no files.
pub(crate) fn write_asset(name: &str, bytes: &[u8]) -> Result<(), crate::error::Error> {
    let kind = if cfg!(all(target_arch = "wasm32", not(feature = "editor"))) {
        crate::error::ErrorKind::Unsupported
    } else {
        crate::error::ErrorKind::Io
    };
    write(name, bytes)
        .map(|_| ())
        .map_err(|e| crate::error::Error::new(kind, e).with_path(name))
}

/// Reads an asset file found by [`resolve`] as UTF-8 text.
pub(crate) fn read_to_string(path: &Path) -> Result<String, String> {
    let bytes = read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|e| format!("{} is not UTF-8 text: {e}", path.display()))
}

// In a browser there are no files: the web build fetches everything listed
// in `assets/index.json` before setup runs (see `crate::web`), and asset
// loading reads those copies.
#[cfg(target_arch = "wasm32")]
mod web_files {
    use std::collections::HashMap;

    #[cfg(not(target_feature = "atomics"))]
    pub(super) type Bytes = std::rc::Rc<[u8]>;
    #[cfg(target_feature = "atomics")]
    pub(super) type Bytes = std::sync::Arc<[u8]>;

    #[cfg(not(target_feature = "atomics"))]
    thread_local! {
        static FILES: std::cell::RefCell<HashMap<String, Bytes>> = Default::default();
    }

    /// With threads, jobs read the files on workers too (asset groups,
    /// streaming). The page's thread writes them all before setup and
    /// never waits for the lock (it may not block): see [`insert`].
    #[cfg(target_feature = "atomics")]
    static FILES: std::sync::RwLock<Option<HashMap<String, Bytes>>> = std::sync::RwLock::new(None);

    /// Runs `f` on the fetched files.
    pub(super) fn with<R>(f: impl FnOnce(&HashMap<String, Bytes>) -> R) -> R {
        #[cfg(not(target_feature = "atomics"))]
        return FILES.with(|files| f(&files.borrow()));
        #[cfg(target_feature = "atomics")]
        {
            let files = FILES.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            match files.as_ref() {
                Some(files) => f(files),
                None => f(&HashMap::new()),
            }
        }
    }

    pub(super) fn insert(name: String, bytes: Vec<u8>) {
        #[cfg(not(target_feature = "atomics"))]
        FILES.with(|files| files.borrow_mut().insert(name, bytes.into()));
        #[cfg(target_feature = "atomics")]
        {
            use std::sync::TryLockError;
            // Spins instead of waiting; a worker holds the lock only to
            // copy a file.
            let mut files = loop {
                match FILES.try_write() {
                    Ok(files) => break files,
                    Err(TryLockError::Poisoned(files)) => break files.into_inner(),
                    Err(TryLockError::WouldBlock) => std::hint::spin_loop(),
                }
            };
            files.get_or_insert_with(HashMap::new).insert(name, bytes.into());
        }
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(super) fn remove(name: &str) {
        #[cfg(not(target_feature = "atomics"))]
        FILES.with(|files| files.borrow_mut().remove(name));
        #[cfg(target_feature = "atomics")]
        {
            use std::sync::TryLockError;
            // As in `insert`: spins instead of waiting.
            let mut files = loop {
                match FILES.try_write() {
                    Ok(files) => break files,
                    Err(TryLockError::Poisoned(files)) => break files.into_inner(),
                    Err(TryLockError::WouldBlock) => std::hint::spin_loop(),
                }
            };
            if let Some(files) = files.as_mut() {
                files.remove(name);
            }
        }
    }
}

/// Stores a fetched asset for later loads (web builds).
#[cfg(target_arch = "wasm32")]
pub(crate) fn insert_web_file(name: String, bytes: Vec<u8>) {
    web_files::insert(name, bytes);
}

/// Drops a file the editor saved and then reverted, when the build has no
/// file of that name (web builds; the page's asset pack is left alone).
#[cfg(all(target_arch = "wasm32", feature = "editor"))]
pub(crate) fn remove_web_file(name: &str) {
    web_files::remove(name);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve(name: &str) -> Result<PathBuf, String> {
    if web_files::with(|files| files.contains_key(name)) {
        Ok(PathBuf::from(name))
    } else {
        Err(format!(
            "\"{name}\" not found in assets/. Web builds load the files listed in assets/index.json; \
             is it listed there?"
        ))
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    let name = path.to_string_lossy();
    web_files::with(|files| files.get(name.as_ref()).map(|b| b.to_vec()))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{name} was not fetched")))
}

/// Decoded RGBA8 image.
pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads and decodes the image at `path`; `name` is for messages.
pub(crate) fn load_image_at(path: &Path, name: &str) -> Result<Image, String> {
    let bytes = read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    decode_image(&bytes, name, &format!("\"{name}\"")).map_err(|e| e.message().to_string())
}

/// The ponko cargo feature that turns on reading `format`, and its name for
/// people; `None` for a format the engine never reads (PNG is always built in).
fn image_feature(format: image::ImageFormat) -> Option<(&'static str, &'static str)> {
    use image::ImageFormat as F;
    Some(match format {
        F::Jpeg => ("image-jpeg", "JPEG"),
        F::WebP => ("image-webp", "WebP"),
        F::Bmp => ("image-bmp", "BMP"),
        F::Tga => ("image-tga", "TGA"),
        F::Gif => ("image-gif", "GIF"),
        F::Tiff => ("image-tiff", "TIFF"),
        F::Ico => ("image-ico", "ICO"),
        F::Qoi => ("image-qoi", "QOI"),
        F::Pnm => ("image-pnm", "PNM"),
        F::Dds => ("image-dds", "DDS"),
        F::Hdr => ("image-hdr", "Radiance HDR"),
        F::OpenExr => ("image-exr", "OpenEXR"),
        _ => return None,
    })
}

/// The error for a file in `format` when the build can't read it. `by_magic`
/// says how the format was found (the file's first bytes, or its extension).
fn format_unavailable(format: image::ImageFormat, what: &str, path: &str, by_magic: bool) -> crate::error::Error {
    let found = if by_magic { "is" } else { "has the extension of" };
    let message = match image_feature(format) {
        Some((feature, name)) => format!(
            "{what} {found} a {name} image, but this build of ponko doesn't read {name}: \
             turn on its `{feature}` cargo feature (or `image-formats` for all of them)"
        ),
        None => format!(
            "{what} {found} a {format:?} image, which ponko can't read: \
             use PNG or JPEG, or any other format listed under the `image-formats` cargo feature"
        ),
    };
    crate::error::Error::new(crate::error::ErrorKind::Unsupported, message).with_path(path)
}

/// Decodes an image file's bytes into RGBA8. The format comes from the first
/// bytes, so a mislabelled extension doesn't matter; the extension of `path`
/// is only used for formats with no recognizable start (TGA). `what` names the
/// file for messages. A format the build leaves out fails with
/// [`ErrorKind::Unsupported`](crate::error::ErrorKind::Unsupported), naming the
/// cargo feature to enable; bytes that aren't a readable image with
/// [`ErrorKind::Decode`](crate::error::ErrorKind::Decode).
pub(crate) fn decode_image(bytes: &[u8], path: &str, what: &str) -> Result<Image, crate::error::Error> {
    // KTX2 and DDS of GPU blocks, decoded on the CPU for loaders that need
    // pixels (textures themselves upload as blocks, see `Graphics::texture_from_file`).
    #[cfg(feature = "compressed-textures")]
    if let Some(image) = crate::compressed::decode_rgba(bytes, path, what) {
        return image;
    }
    #[cfg(not(feature = "compressed-textures"))]
    if bytes.starts_with(&[0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB]) {
        return Err(crate::error::Error::new(
            crate::error::ErrorKind::Unsupported,
            format!(
                "{what} is a KTX2 texture, but this build of ponko doesn't read KTX2: \
                 turn on its `compressed-textures` cargo feature"
            ),
        )
        .with_path(path));
    }
    let by_magic = image::guess_format(bytes).ok();
    let format = by_magic.or_else(|| image::ImageFormat::from_path(path).ok());
    if let Some(format) = format.filter(|f| !f.reading_enabled()) {
        return Err(format_unavailable(format, what, path, by_magic.is_some()));
    }
    let decoded = match format {
        Some(format) => image::load_from_memory_with_format(bytes, format),
        None => image::load_from_memory(bytes),
    };
    let rgba = decoded
        .map_err(|e| crate::error::Error::decode(path, format!("{what} is not a valid image: {e}")))?
        .into_rgba8();
    Ok(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// How to apply a loaded data file's text to the world (kept for reloads).
pub(crate) type DataLoader = Box<dyn Fn(&mut World, &str) -> Result<(), crate::error::Error>>;

/// Asset files: `ctx.assets`.
///
/// - **What lives here:** where named assets are read from
///   ([`add_source`](Self::add_source)), your own asset types
///   ([`load_with`](Self::load_with)), data files that fill resources and
///   components and follow their files, saving scenes and prefabs, and
///   what hot reload changed this frame. Textures, fonts, meshes and
///   shaders load through `ctx.gfx` and sounds through `ctx.audio`, from
///   the same sources. [`Ctx::load_scene`](crate::ctx::Ctx::load_scene) and
///   [`Ctx::spawn_prefab`](crate::ctx::Ctx::spawn_prefab) stay on `Ctx`: they
///   load through every service.
/// - **When to call it:** add sources first, in setup; load data files in
///   setup too (with hot reload on they are applied again when their files
///   change, at the start of a frame). Read what was reloaded from any
///   system, and save from any system.
/// - **What it needs:** no window, GPU or audio device. Saving writes to
///   `assets/`; web builds save only with the `editor` feature (into the
///   browser's storage, see the guide's "Saving in a browser"). The calls that read or
///   write entities and resources take the world (`&mut ctx.world`), so
///   `ctx.assets` borrows apart from it.
#[derive(Default)]
pub struct Assets {
    /// Asset sources added with `add_source`, asked before `assets/`.
    /// Shared with the services that load files.
    pub(crate) asset_sources: SharedSources,
    /// Asset hot reload: which files loaded assets came from. Shared too.
    pub(crate) watcher: crate::hot_reload::SharedWatcher,
    /// Textures reloaded at the start of this frame.
    pub(crate) reloaded_textures: Vec<crate::texture::Texture>,
    /// Names of every file reloaded at the start of this frame.
    pub(crate) reloaded_files: Vec<String>,
    /// Scratch for the watcher's changes, kept so checks don't allocate.
    pub(crate) reload_pending: Vec<(Asset, PathBuf, String)>,
    /// How to apply each loaded data file's text (`load_resource`).
    pub(crate) data_loaders: Vec<DataLoader>,
    /// Parsed prefab files, by name (`Ctx::spawn_prefab`).
    pub(crate) prefabs: HashMap<String, std::rc::Rc<crate::scene::PrefabFile>>,
    /// Fields of reflected components that name assets, and how to load
    /// them before a scene or prefab is spawned (`scene_asset_field`).
    pub(crate) scene_asset_fields: Vec<crate::scene::SceneAssetField>,
    /// Asset groups started with `start_group`.
    pub(crate) groups: group::Groups,
}

impl Assets {
    /// Whether files loaded from `assets/` are watched and reloaded when
    /// they change (see [`Game::hot_reload`](crate::game::Game::hot_reload)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
    ///     assert!(!ctx.assets.hot_reload());
    /// });
    /// ```
    #[must_use]
    pub fn hot_reload(&self) -> bool {
        self.watcher.on()
    }

    /// Adds a place to load named assets from. Added sources are asked in
    /// the order they were added, all before the `assets/` folder.
    ///
    /// Every load by name goes through it: [`load_texture`](crate::gfx::Graphics::load_texture),
    /// [`load_font`](crate::gfx::Graphics::load_font), [`load_resource`](Self::load_resource),
    /// [`load_scene`](crate::ctx::Ctx::load_scene), [`spawn_prefab`](crate::ctx::Ctx::spawn_prefab)
    /// (and the textures their scenes name), [`load_sound`](crate::audio::Audio::load),
    /// [`load_with`](Self::load_with) and, with the `3d` feature,
    /// meshes, skeletons and animation clips (and the files a glTF refers to),
    /// shaders, screen effects and compute passes.
    ///
    /// Not covered yet: [`load_stream`](crate::audio::Audio::load_stream) (music streamed
    /// from a file), texture arrays and reflection probe images still come
    /// only from `assets/`.
    ///
    /// Assets from sources are never hot reloaded: only files found in
    /// `assets/` are watched. See [`AssetSource`] for
    /// names, caching and hot reload.
    ///
    /// Add sources in setup, before loading what they hold: assets loaded
    /// earlier stay cached where they were found.
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// # use ponko::prelude::*;
    /// #[derive(serde::Deserialize)]
    /// struct Rules {
    ///     speed: f32,
    /// }
    ///
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_source(MemorySource::new("built-in").with("rules.json", r#"{"speed": 30}"#));
    ///     ctx.assets.load_resource::<Rules>(&mut ctx.world, "rules.json");
    ///     assert_eq!(ctx.world.resource::<Rules>().speed, 30.0);
    /// });
    /// ```
    ///
    /// # Panics
    /// If the source's id is empty or a source with that id was already added.
    pub fn add_source(&mut self, source: impl crate::assets::AssetSource) {
        self.asset_sources.add(Slot::Game(Box::new(source)));
    }

    /// Adds a source worker threads may read too, as
    /// [`add_source`](Self::add_source) does otherwise (asked in the order
    /// added, all before `assets/`). [Asset groups](AssetGroup) and
    /// [`stream_texture`](crate::gfx::Graphics::stream_texture) read assets
    /// from it on a worker, so a slow source (a big pack, a network cache)
    /// never holds up a frame; [`MemorySource`] and [`PackSource`] qualify.
    /// A source from `add_source` is read on the game thread, as its trait
    /// may need; one added after a background source makes every search
    /// that reaches it run on the game thread, to keep the order sources
    /// are asked in.
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets
    ///         .add_background_source(MemorySource::new("level-1").with("rules.json", r#"{"speed": 30}"#));
    ///     assert!(ctx.assets.has_source("level-1"));
    /// });
    /// ```
    ///
    /// # Panics
    /// If the source's id is empty or a source with that id was already added.
    pub fn add_background_source(&mut self, source: impl crate::assets::AssetSource + Send + Sync) {
        self.asset_sources.add(Slot::Background(Arc::new(source)));
    }

    /// Whether a source with id `id` was added with
    /// [`add_source`](Self::add_source), for code that adds a source only
    /// once (`add_source` panics on a duplicate id).
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert!(!ctx.assets.has_source("built-in"));
    ///     ctx.assets.add_source(MemorySource::new("built-in"));
    ///     assert!(ctx.assets.has_source("built-in"));
    /// });
    /// ```
    pub fn has_source(&self, id: &str) -> bool {
        self.asset_sources.has(id)
    }

    /// Reads asset `name` (from the added sources, then `assets/`) and
    /// decodes it with `decode` on the CPU: your own asset type, with no
    /// registration. Nothing is cached and nothing is watched: it reads and
    /// decodes each call, so keep the result.
    ///
    /// `decode` gets the bytes and returns your value, or a [`crate::error::Error`]
    /// (usually [`ErrorKind::Decode`](crate::error::ErrorKind::Decode)), which is
    /// reported with the asset's name and source in front of its message.
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// # use ponko::prelude::*;
    /// /// A level in a tiny made-up format: one row of tiles per line.
    /// struct Tiles(Vec<Vec<u8>>);
    ///
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_source(MemorySource::new("built-in").with("levels/one.tiles", "##.\n#..\n"));
    ///     let level = ctx
    ///         .assets
    ///         .load_with("levels/one.tiles", |bytes| {
    ///             let text = std::str::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::Decode, e.to_string()))?;
    ///             Ok(Tiles(text.lines().map(|row| row.bytes().collect()).collect()))
    ///         })
    ///         .unwrap();
    ///     assert_eq!(level.0.len(), 2);
    ///     assert_eq!(level.0[0], b"##.");
    /// });
    /// ```
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if no source has
    /// the asset (the message says where it looked), [`Io`](crate::error::ErrorKind::Io)
    /// if reading it fails, and `decode`'s error, with its kind, if it fails.
    /// The error's path is `name`.
    pub fn load_with<T>(
        &self,
        name: &str,
        decode: impl FnOnce(&[u8]) -> Result<T, crate::error::Error>,
    ) -> Result<T, crate::error::Error> {
        let found = self.asset_sources.find(name)?;
        let origin = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(name))?;
        decode(&bytes).map_err(|e| {
            e.context(format!("{} could not be decoded", origin.describe(name)))
                .with_path(name)
        })
    }

    /// Reads asset `name` as UTF-8 text from the first source that has it.
    pub(crate) fn read_text(&self, name: &str) -> Result<(Origin, String), crate::error::Error> {
        let found = self.asset_sources.find(name)?;
        let origin = found.origin.clone();
        Ok((origin, found.into_text(name)?))
    }

    /// Loads resource `R` from a JSON file in `assets/`. With [hot
    /// reload](crate::game::Game::hot_reload) on, editing the file replaces the
    /// resource while the game runs: tune numbers without restarting. A file
    /// that doesn't parse keeps the old values and prints why.
    ///
    /// # Panics
    /// If the file is missing or isn't valid JSON for `R` (with the line and
    /// column).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// #[derive(serde::Deserialize)]
    /// struct Rules {
    ///     speed: f32,
    ///     damage: f32,
    /// }
    ///
    /// Game::new("Tuning", 800, 600).run(|ctx| {
    ///     ctx.assets.load_resource::<Rules>(&mut ctx.world, "rules.json"); // {"speed": 30, "damage": 2}
    ///     |ctx: &mut Ctx| {
    ///         let speed = ctx.world.resource::<Rules>().speed;
    ///         // ...
    ///     }
    /// });
    /// ```
    pub fn load_resource<R: Send + Sync + 'static + serde::de::DeserializeOwned>(
        &mut self,
        world: &mut World,
        path: &str,
    ) {
        self.try_load_resource::<R>(world, path)
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// Loads and replaces a deserialized resource, with the same hot reload
    /// behavior as [`Self::load_resource`], returning errors instead of panicking.
    ///
    /// # Errors
    /// Returns a source/read/JSON error with `path`. Failed loads leave the
    /// existing resource unchanged and do not register a reload callback.
    pub fn try_load_resource<R: Send + Sync + 'static + serde::de::DeserializeOwned>(
        &mut self,
        world: &mut World,
        path: &str,
    ) -> Result<(), crate::error::Error> {
        self.load_data(
            world,
            path,
            Box::new(|world: &mut World, text: &str| {
                let value: R = serde_json::from_str(text)?;
                world.insert_resource(value);
                Ok(())
            }),
        )
    }

    /// Loads a string table file (see [`crate::locale`] for the format) into
    /// the [`Strings`](crate::locale::Strings) resource, which it adds if the
    /// world has none. The first table loaded is the reference language and
    /// the current one. With [hot reload](crate::game::Game::hot_reload) on,
    /// editing the file replaces the table while the game runs, and UI nodes
    /// that hold a key show the new text; a file that doesn't parse keeps
    /// the old table and prints why.
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// # use ponko::prelude::*;
    /// use ponko::locale::Strings;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_source(MemorySource::new("built-in")
    ///         .with("lang/en.json", r#"{"language": "en", "strings": {"play": "Play"}}"#)
    ///         .with("lang/es.json", r#"{"language": "es", "strings": {"play": "Jugar"}}"#));
    ///     ctx.assets.load_strings(&mut ctx.world, "lang/en.json");
    ///     ctx.assets.load_strings(&mut ctx.world, "lang/es.json");
    ///     let strings = ctx.world.resource_mut::<Strings>();
    ///     strings.set_language("es");
    ///     assert_eq!(strings.get("play"), "Jugar");
    /// });
    /// ```
    ///
    /// # Panics
    /// If the file is missing or isn't a string table, with the engine's
    /// message (it starts with `ponko:`). [`try_load_strings`](Self::try_load_strings)
    /// returns the error instead.
    pub fn load_strings(&mut self, world: &mut World, path: &str) {
        self.try_load_strings(world, path).unwrap_or_else(|e| panic!("{e}"));
    }

    /// [`load_strings`](Self::load_strings), returning the error instead of
    /// panicking: the format and hot reload are the same, and a failed load
    /// leaves the tables as they were.
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if no source
    /// has the file, [`Decode`](crate::error::ErrorKind::Decode) if it isn't a
    /// valid string table, or [`Json`](crate::error::ErrorKind::Json) for malformed
    /// JSON (see [`StringTable::from_json`](crate::locale::StringTable::from_json)).
    pub fn try_load_strings(&mut self, world: &mut World, path: &str) -> Result<(), crate::error::Error> {
        self.load_data(
            world,
            path,
            Box::new(|world: &mut World, text: &str| {
                let table = crate::locale::StringTable::from_json(text)?;
                if world.get_resource::<crate::locale::Strings>().is_none() {
                    world.insert_resource(crate::locale::Strings::default());
                }
                world.resource_mut::<crate::locale::Strings>().insert(table);
                Ok(())
            }),
        )
    }

    /// Loads a JSON file into the reflected component `T` of `entity` (object
    /// fields merge), reloading it when the file changes. Like every
    /// reflection edit it returns its errors (a runtime operation: the
    /// entity may be gone) instead of panicking; [`load_resource`](Self::load_resource)
    /// is the typed, panicking loader that replaces a whole resource. `T` must be
    /// registered with `World::reflect`; the name the file is applied under
    /// comes from the type, so it can't drift from it.
    /// ```
    /// # use ponko::prelude::*;
    /// # #[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
    /// # struct Stats { hp: f32 }
    /// # fn f(ctx: &mut Ctx, e: Entity) -> Result<(), ponko::prelude::Error> {
    /// ctx.assets.load_reflected_component_json::<Stats>(&mut ctx.world, "stats.json", e)?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// As [`load_reflected_resource_json`](Self::load_reflected_resource_json), and
    /// [`ErrorKind::BadHandle`](crate::error::ErrorKind::BadHandle) if `entity` is dead.
    pub fn load_reflected_component_json<T: 'static>(
        &mut self,
        world: &mut World,
        path: &str,
        entity: Entity,
    ) -> Result<(), crate::error::Error> {
        self.load_reflected_component_json_named(
            world,
            path,
            entity,
            &crate::reflect::short_name(std::any::type_name::<T>()),
        )
    }

    /// Loads a JSON file into the reflected component `component` of `entity`
    /// by name: the C and C++ way, and for components registered by name.
    /// Rust code that has the type uses
    /// [`load_reflected_component_json`](Self::load_reflected_component_json).
    #[doc(hidden)]
    pub fn load_reflected_component_json_named(
        &mut self,
        world: &mut World,
        path: &str,
        entity: Entity,
        component: &str,
    ) -> Result<(), crate::error::Error> {
        let component = component.to_string();
        self.load_data(
            world,
            path,
            Box::new(move |world: &mut World, text: &str| {
                let value: serde_json::Value = serde_json::from_str(text)?;
                Ok(world.set_json(entity, &component, value)?)
            }),
        )
    }

    /// Loads a JSON file into the reflected resource `T` (object fields
    /// merge; it's added if missing), reloading it when the file changes.
    /// `T` must be registered with `World::reflect_resource`; the name comes
    /// from the type. Unlike [`load_resource`](Self::load_resource) this
    /// merges into the existing value instead of replacing it.
    /// ```
    /// # use ponko::prelude::*;
    /// # #[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
    /// # struct Rules { speed: f32 }
    /// # fn f(ctx: &mut Ctx) -> Result<(), ponko::prelude::Error> {
    /// ctx.assets.load_reflected_resource_json::<Rules>(&mut ctx.world, "rules.json")?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file is
    /// missing or `T` isn't registered, [`Io`](crate::error::ErrorKind::Io) if it
    /// can't be read, [`Decode`](crate::error::ErrorKind::Decode) if it isn't UTF-8
    /// and [`Json`](crate::error::ErrorKind::Json) if it isn't valid JSON for `T`.
    /// The error's path is `path`.
    pub fn load_reflected_resource_json<T: 'static>(
        &mut self,
        world: &mut World,
        path: &str,
    ) -> Result<(), crate::error::Error> {
        self.load_reflected_resource_json_named(world, path, &crate::reflect::short_name(std::any::type_name::<T>()))
    }

    /// Loads a JSON file into the reflected resource `name` by name: the C
    /// and C++ way. Rust code that has the type uses
    /// [`load_reflected_resource_json`](Self::load_reflected_resource_json).
    #[doc(hidden)]
    pub fn load_reflected_resource_json_named(
        &mut self,
        world: &mut World,
        path: &str,
        name: &str,
    ) -> Result<(), crate::error::Error> {
        let name = name.to_string();
        self.load_data(
            world,
            path,
            Box::new(move |world: &mut World, text: &str| {
                let value: serde_json::Value = serde_json::from_str(text)?;
                Ok(world.set_resource_json(&name, value)?)
            }),
        )
    }

    pub(crate) fn load_data(
        &mut self,
        world: &mut World,
        path: &str,
        apply: DataLoader,
    ) -> Result<(), crate::error::Error> {
        let (origin, text) = self.read_text(path)?;
        apply(world, &text).map_err(|e| e.context(path).with_path(path))?;
        self.data_loaders.push(apply);
        if let Origin::File(file) = origin {
            self.watcher.watch(path, file, Asset::Data(self.data_loaders.len() - 1));
        }
        Ok(())
    }

    /// The files the editor saved in this browser for this page, by asset
    /// name, sorted: in web builds with the `editor` feature, where
    /// [`save_scene`](Self::save_scene) and [`save_prefab`](Self::save_prefab)
    /// keep files in the page's `localStorage`. Empty in other builds.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     // Desktop builds save to `assets/`, not to a browser.
    ///     assert!(ctx.assets.browser_saves().is_empty());
    /// });
    /// ```
    pub fn browser_saves(&self) -> Vec<String> {
        #[cfg(feature = "editor")]
        return crate::editor::web::saved().into_iter().map(|s| s.name).collect();
        #[cfg(not(feature = "editor"))]
        Vec::new()
    }

    /// Whether the browser keeps the editor's saves (see
    /// [`browser_saves`](Self::browser_saves)) when it runs low on space:
    /// in web builds with the `editor` feature the page asks for persistent
    /// storage before setup. `Some(true)` when the browser granted it,
    /// `Some(false)` when it refused (the saves may be cleared; the editor
    /// says so), `None` until it answers, and always in other builds.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.assets.browser_saves_persistent(), None); // no browser
    /// });
    /// ```
    #[must_use]
    pub fn browser_saves_persistent(&self) -> Option<bool> {
        #[cfg(feature = "editor")]
        return match crate::editor::web::persistence() {
            crate::editor::web::Persistence::Unknown => None,
            crate::editor::web::Persistence::Granted => Some(true),
            crate::editor::web::Persistence::Refused => Some(false),
        };
        #[cfg(not(feature = "editor"))]
        None
    }

    /// Drops the copy of `path` saved in this browser (see
    /// [`browser_saves`](Self::browser_saves)): from now on, and after the
    /// page is loaded again, `path` is the build's own file, or no file if
    /// the build has none. Returns whether a copy was saved; always `false`
    /// outside web builds with the `editor` feature.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.assets.revert_browser_save("levels/one.json"), Ok(false));
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Io`](crate::error::ErrorKind::Io) if the browser's
    /// storage can't be reached or refuses the removal.
    pub fn revert_browser_save(&self, path: &str) -> Result<bool, crate::error::Error> {
        #[cfg(feature = "editor")]
        return crate::editor::web::forget(path)
            .map_err(|e| crate::error::Error::new(crate::error::ErrorKind::Io, e).with_path(path));
        #[cfg(not(feature = "editor"))]
        {
            let _ = path;
            Ok(false)
        }
    }

    /// Saves the world (its reflected components and resources) as a scene
    /// in `assets/<path>`, over the file if it exists. Web builds save only
    /// with the `editor` feature, into the browser's storage.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.assets.save_scene(&ctx.world, "levels/beach.json").expect("saved");
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Io`](crate::error::ErrorKind::Io) if the file can't be written
    /// ([`Unsupported`](crate::error::ErrorKind::Unsupported) in web builds
    /// without the `editor` feature).
    pub fn save_scene(&self, world: &World, path: &str) -> Result<(), crate::error::Error> {
        let text = serde_json::to_string_pretty(&world.save())?;
        write_asset(path, text.as_bytes())
    }

    /// Saves entity `e` (its reflected components) as a prefab in
    /// `assets/<path>`, for [`spawn_prefab`](crate::ctx::Ctx::spawn_prefab). Its
    /// position isn't kept: each spawn says where. Web builds save only
    /// with the `editor` feature, into the browser's storage.
    ///
    /// The prefab holds `e`'s children too: its [`PrefabParts`](crate::prelude::PrefabParts)
    /// (if a prefab spawned it) and every entity attached to one of them,
    /// by [`Attached`](crate::attach::Attached) or its 3D form, a skin or joint attachment, or a 3D
    /// joint between two of them. Their transforms are saved relative to
    /// `e`'s, and their references to each other are kept. Included prefabs
    /// are saved as their entities, not as a link to their file. An entity
    /// with no children is saved as a version-1 file, as before.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, palm: Entity) {
    /// ctx.assets.save_prefab(&ctx.world, palm, "prefabs/palm.json").expect("saved");
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::BadHandle`](crate::error::ErrorKind::BadHandle) if `e` is dead,
    /// [`Io`](crate::error::ErrorKind::Io) if the file can't be written
    /// ([`Unsupported`](crate::error::ErrorKind::Unsupported) in web builds
    /// without the `editor` feature).
    pub fn save_prefab(&mut self, world: &World, e: Entity, path: &str) -> Result<(), crate::error::Error> {
        let mut components = world.entity_json(e)?;
        if let Some(t) = components.get_mut("Transform2D") {
            t["position"] = json!([0.0, 0.0]);
        }
        // Spawning gives the root its parts again.
        if let Value::Object(map) = &mut components {
            map.remove("PrefabParts");
        }
        let members = crate::scene::prefab_members(world, e);
        if members.len() == 1 {
            let prefab = json!({ "format": PREFAB_FORMAT, "version": PREFAB_VERSION, "components": components });
            let text = serde_json::to_string_pretty(&prefab)?;
            write_asset(path, text.as_bytes())?;
            if let Value::Object(components) = components {
                let file = crate::scene::PrefabFile::single(components);
                self.prefabs.insert(path.to_string(), std::rc::Rc::new(file));
            }
            return Ok(());
        }
        let mut entities = Vec::with_capacity(members.len());
        for &m in &members {
            let mut json = if m == e {
                components.clone()
            } else {
                world.entity_json(m)?
            };
            if m != e {
                crate::scene::relative_to(world, m, e, &mut json);
            }
            // Spawning gives the root its parts again.
            if let Value::Object(map) = &mut json {
                map.remove("PrefabParts");
            }
            entities.push(json!({ "entity": m.to_bits(), "components": json }));
        }
        let prefab =
            json!({ "format": PREFAB_FORMAT, "version": crate::scene::PREFAB_TREE_VERSION, "entities": entities });
        let text = serde_json::to_string_pretty(&prefab)?;
        write_asset(path, text.as_bytes())?;
        // Read back from the file's JSON next time, like any prefab.
        self.prefabs.remove(path);
        Ok(())
    }

    /// Names of the files (as you loaded them) that changed and were
    /// reloaded at the start of this frame, textures and data alike. Use it
    /// to redo work that depends on them.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// if ctx.assets.reloaded_files().iter().any(|f| f == "map.json") {
    ///     println!("map changed: rebuilding the flow fields");
    /// }
    /// # }
    /// ```
    pub fn reloaded_files(&self) -> &[String] {
        &self.reloaded_files
    }

    /// Textures whose files changed and were reloaded at the start of this
    /// frame (with [hot reload](crate::game::Game::hot_reload) on). They keep their
    /// handles, so sprites show the new image by themselves; use this to redo
    /// anything you computed from an image, such as a level read from its pixels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, level: Texture) {
    /// if ctx.assets.reloaded_textures().contains(&level) {
    ///     println!("level image changed: rebuilding the map");
    /// }
    /// # }
    /// ```
    pub fn reloaded_textures(&self) -> &[crate::texture::Texture] {
        &self.reloaded_textures
    }

    pub(crate) fn report_reload(&mut self, name: String, applied: Result<(), String>) {
        match applied {
            Ok(()) => {
                eprintln!("ponko: reloaded {name}");
                self.reloaded_files.push(name);
            }
            Err(e) => eprintln!("ponko: could not reload {name} (keeping the old one): {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn missing_assets_suggest_case_and_extension_near_misses() {
        let root = std::env::temp_dir().join(format!("ponko-nearby-assets-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Wall.PNG"), []).unwrap();
        std::fs::write(root.join("wall.jpg"), []).unwrap();
        let suggestions = nearby_assets("wall.png", std::slice::from_ref(&root));
        assert!(suggestions.contains("Wall.PNG"), "{suggestions}");
        assert!(suggestions.contains("wall.jpg"), "{suggestions}");
        assert!(suggestions.contains("Check spelling and case"), "{suggestions}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(feature = "compressed-textures"))]
    #[test]
    fn ktx2_files_without_the_feature_name_it() {
        let header = [
            0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
        ];
        let Err(err) = decode_image(&header, "rock.ktx2", "rock.ktx2") else {
            panic!("decoded");
        };
        assert_eq!(err.kind(), crate::error::ErrorKind::Unsupported);
        assert!(err.to_string().contains("rock.ktx2"), "{err}");
        assert!(err.message().contains("compressed-textures"), "{err}");
    }

    #[test]
    fn missing_asset_error_names_file_and_folders() {
        let err = resolve("definitely-not-here.png").unwrap_err();
        assert!(err.contains("\"definitely-not-here.png\" not found in assets/"));
        assert!(err.contains("assets"));
    }

    #[test]
    fn a_macos_app_bundle_finds_assets_in_its_resources() {
        let exe_dir = Path::new("/Games/Coin Dash.app/Contents/MacOS");
        assert_eq!(
            bundle_resources(exe_dir),
            Some(PathBuf::from("/Games/Coin Dash.app/Contents/Resources"))
        );
        // A plain folder, or one that only looks partly like a bundle, is not one.
        for dir in [
            "/Games/coin-dash",
            "/Games/Coin Dash/Contents/MacOS",
            "/x.app/Other/MacOS",
            "/MacOS",
        ] {
            assert_eq!(bundle_resources(Path::new(dir)), None, "{dir}");
        }
    }

    #[test]
    fn sources_are_asked_in_order_and_say_not_found() {
        let mut sources = Sources::default();
        sources.add(Slot::Game(Box::new(MemorySource::new("a").with("x", *b"from a"))));
        sources.add(Slot::Game(Box::new(
            MemorySource::new("b").with("x", *b"from b").with("y", *b"only b"),
        )));
        let x = sources.find("x").unwrap();
        assert_eq!(
            x.origin,
            Origin::Source {
                source: "a".into(),
                name: "x".into()
            }
        );
        assert_eq!(x.into_bytes().unwrap(), b"from a");
        assert_eq!(sources.find("y").unwrap().into_text("y").unwrap(), "only b");
        let missing = MemorySource::new("a").read("z").unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        let err = sources.find("definitely-not-here.png").err().unwrap();
        assert_eq!(err.kind(), crate::error::ErrorKind::NotFound);
        assert!(
            err.message().ends_with("Asset sources searched first: \"a\", \"b\""),
            "{err}"
        );
    }

    /// A source that only implements `read`: `contains` falls back to it.
    struct ReadOnly;
    impl AssetSource for ReadOnly {
        fn id(&self) -> &str {
            "read-only"
        }
        fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
            match name {
                "here" => Ok(b"x".to_vec()),
                "broken" => Err(std::io::Error::other("disk on fire")),
                _ => Err(std::io::ErrorKind::NotFound.into()),
            }
        }
    }

    #[test]
    fn locate_names_the_first_source_that_has_a_name_without_reading_memory() {
        let mut sources = Sources::default();
        sources.add(Slot::Game(Box::new(ReadOnly)));
        sources.add(Slot::Game(Box::new(
            MemorySource::new("mem").with("here", *b"y").with("only-mem", *b"z"),
        )));
        let in_source = |source: &str, name: &str| Origin::Source {
            source: source.into(),
            name: name.into(),
        };
        assert_eq!(sources.locate("here"), Some(in_source("read-only", "here")));
        assert_eq!(sources.locate("only-mem"), Some(in_source("mem", "only-mem")));
        // A source that has it but fails to read it still has it: the
        // load reports the error.
        assert_eq!(sources.locate("broken"), Some(in_source("read-only", "broken")));
        assert_eq!(sources.locate("definitely-not-here.png"), None);
    }

    #[test]
    fn loose_files_win_over_default_packs_which_fill_in_the_rest() {
        // `crab.png` is a loose file of this crate's assets/ folder.
        let pack = pack::write_for_test(&[("crab.png", b"packed crab", false), ("only/packed.txt", b"hi", true)]);
        let mut sources = Sources::with_packs(vec![PackSource::from_bytes("assets.pack", pack).unwrap()]);
        let found = sources.find("crab.png").unwrap();
        assert!(matches!(found.origin, Origin::File(_)), "the loose file wins");
        let found = sources.find("only/packed.txt").unwrap();
        let in_pack = Origin::Source {
            source: "assets.pack".into(),
            name: "only/packed.txt".into(),
        };
        assert_eq!(found.origin, in_pack);
        assert_eq!(found.into_bytes().unwrap(), b"hi");
        assert_eq!(sources.locate("only/packed.txt"), Some(in_pack.clone()));
        // Added sources still come first.
        sources.add(Slot::Game(Box::new(
            MemorySource::new("mem").with("only/packed.txt", *b"mem"),
        )));
        assert_eq!(sources.find("only/packed.txt").unwrap().into_bytes().unwrap(), b"mem");
        // A missing name says where it looked, packs included.
        let err = sources.find("nowhere.png").err().unwrap();
        assert_eq!(err.kind(), crate::error::ErrorKind::NotFound);
        assert!(
            err.message().contains("Asset packs searched after them: assets.pack"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_name_loaded_from_a_pack_is_cached_without_unpacking_it_again() {
        let pack = pack::write_for_test(&[("packed.txt", b"hi", false)]);
        let mut bytes = pack.clone();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff; // reading it would fail its checksum
        let sources = Sources::with_packs(vec![PackSource::from_bytes("assets.pack", bytes).unwrap()]);
        assert_eq!(
            sources.find("packed.txt").err().unwrap().kind(),
            crate::error::ErrorKind::Io
        );
        let cached = sources.lookup("packed.txt", |origin| {
            matches!(origin, Origin::Source { source, .. } if source == "assets.pack").then_some(7)
        });
        assert!(matches!(cached, Ok(Lookup::Cached(_, 7))));
    }

    #[test]
    fn garbage_bytes_are_rejected() {
        assert!(decode_image(b"not a png", "a.png", "\"a.png\"").is_err());
    }

    #[test]
    fn decodes_a_jpeg_to_rgba() {
        if !image::ImageFormat::Jpeg.reading_enabled() {
            return;
        }
        let jpeg = include_bytes!("../tests/fixtures/images/red-blue.jpg");
        let img = decode_image(jpeg, "bg.jpg", "\"bg.jpg\"").unwrap();
        assert_eq!((img.width, img.height), (16, 8));
        let px = |x: usize| &img.rgba[(3 * 16 + x) * 4..][..4];
        let (left, right) = (px(2), px(13));
        assert!(left[0] > 200 && left[1] < 60 && left[2] < 60, "left is red: {left:?}");
        assert!(
            right[2] > 200 && right[0] < 60 && right[1] < 60,
            "right is blue: {right:?}"
        );
        assert_eq!(left[3], 255);
    }

    #[test]
    fn a_compiled_out_format_is_found_from_its_magic_bytes() {
        // Never decoded: the check comes first. Skip when the build has WebP.
        if image::ImageFormat::WebP.reading_enabled() {
            return;
        }
        let webp = b"RIFF\x0c\0\0\0WEBPVP8 \0\0\0\0";
        let err = decode_image(webp, "x.png", "\"x.png\"").err().unwrap();
        assert_eq!(err.kind(), crate::error::ErrorKind::Unsupported);
        assert!(err.message().contains("`image-webp`"), "{}", err.message());
        assert!(err.message().contains("is a WebP image"), "{}", err.message());
    }

    #[test]
    fn a_format_the_engine_never_reads_says_so() {
        let err = format_unavailable(image::ImageFormat::Avif, "\"a.avif\"", "a.avif", false);
        assert_eq!(err.kind(), crate::error::ErrorKind::Unsupported);
        assert!(err.message().contains("can't read"), "{}", err.message());
    }

    #[test]
    fn decodes_png() {
        // 1x1 red pixel PNG, encoded in the test so no asset file is needed.
        let mut png = Vec::new();
        image::RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255])
            .unwrap()
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let img = decode_image(&png, "red.png", "\"red.png\"").unwrap();
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.rgba, [255, 0, 0, 255]);
    }
}
