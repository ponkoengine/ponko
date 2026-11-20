//! Asset groups: a set of assets declared together, loaded in the
//! background with progress, for loading screens ([`AssetGroup`],
//! [`Assets::start_group`]).
//!
//! A group's assets are read and decoded on [`Job`] threads (in a browser,
//! which has no threads here, a few at a time within each frame's budget);
//! the game thread only takes what is ready, once per frame before the
//! game's systems, within [`Assets::set_group_frame_budget`] and the
//! per-frame [upload budget](crate::gfx::Graphics::set_upload_budget). So
//! the frames keep coming while it loads, and a loading screen can draw
//! its progress.
//!
//! Without workers, a large PNG or sound is decoded a slice at a time
//! ([`sliced`]), so one file is spread over several frames. A failed or
//! cancelled group can be [restarted](Assets::restart_group): only what had
//! not loaded is read again.

mod sliced;

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::assets::{Assets, BackgroundRead, Origin, ReadPlan};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::streaming::Job;

/// How one declared asset is loaded, and where it ends up. Kept for a
/// restart, which loads an asset that failed again.
#[derive(Clone)]
enum Kind {
    /// As `load_texture`: kept until freed; `load_texture` then returns it.
    Texture,
    /// As `stream_texture`: under the texture residency budget.
    StreamedTexture,
    /// As `stream_mesh`: under the mesh residency budget.
    #[cfg(feature = "3d")]
    Mesh,
    /// As `Audio::load`.
    Sound,
    /// The game's own loader; its value is taken with `take_group_value`.
    Value(Decoder),
}

type Decoder = Arc<dyn Fn(&[u8]) -> Result<Box<dyn Any + Send>, Error> + Send + Sync>;

/// A set of assets to load together in the background: a level's textures,
/// sounds, meshes and data files, behind a loading screen.
///
/// Declare it with the builder calls, then start it with
/// [`Assets::start_group`] (or let a [loading state](crate::game::Game::loading_state)
/// start it). Each asset is named as every load names it, so it comes from
/// the added [asset sources](Assets::add_source), `assets/` or the asset
/// packs, in that order. Once the group is [`GroupState::Ready`], get each
/// asset the usual way: `ctx.gfx.load_texture(path)`, `ctx.audio.load(path)`
/// and `ctx.gfx.load_mesh(path)` return the loaded handles without reading
/// anything again, `stream_texture` and `resident_texture` the streamed
/// textures, and [`Assets::take_group_value`] the values of your own
/// loaders.
///
/// ```
/// use ponko::assets::{AssetGroup, GroupState, MemorySource};
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     ctx.assets.add_background_source(MemorySource::new("level").with("level.txt", "##..#"));
///     let group = AssetGroup::new("level-1").with("level.txt", |bytes| Ok(bytes.iter().filter(|&&b| b == b'#').count()));
///     let id = ctx.assets.start_group(group);
///     while ctx.assets.group_state(id) == GroupState::Loading {
///         ctx.pump_asset_groups(); // the engine does this once per frame
///     }
///     assert_eq!(ctx.assets.take_group_value::<usize>(id, "level.txt"), Some(3));
/// });
/// ```
pub struct AssetGroup {
    name: String,
    assets: Vec<(String, Kind)>,
}

impl std::fmt::Debug for AssetGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetGroup")
            .field("name", &self.name)
            .field("assets", &self.assets.iter().map(|(path, _)| path).collect::<Vec<_>>())
            .finish()
    }
}

impl AssetGroup {
    /// An empty group called `name` (for [`Assets::group`] and messages).
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("title");
    /// assert_eq!(group.name(), "title");
    /// assert!(group.is_empty());
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            assets: Vec::new(),
        }
    }

    /// The group's name.
    ///
    /// ```
    /// assert_eq!(ponko::assets::AssetGroup::new("level-2").name(), "level-2");
    /// ```
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many assets it declares.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("hud").texture("hud/heart.png").sound("hud/beep.wav");
    /// assert_eq!(group.len(), 2);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether it declares no assets (it is ready as soon as it starts).
    ///
    /// ```
    /// assert!(ponko::assets::AssetGroup::new("nothing").is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// The paths it declares, in order.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("hud").texture("a.png").sound("b.ogg");
    /// assert_eq!(group.paths().collect::<Vec<_>>(), ["a.png", "b.ogg"]);
    /// ```
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.assets.iter().map(|(path, _)| path.as_str())
    }

    /// Adds texture `path`, loaded as [`load_texture`](crate::gfx::Graphics::load_texture)
    /// loads it (kept until freed, hot reloaded from `assets/`), decoded on a
    /// worker and made on the GPU within the upload budget. Once loaded,
    /// `ctx.gfx.load_texture(path)` returns it.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("level").texture("tiles.png");
    /// assert_eq!(group.len(), 1);
    /// ```
    #[must_use]
    pub fn texture(mut self, path: &str) -> Self {
        self.assets.push((path.to_string(), Kind::Texture));
        self
    }

    /// Adds texture `path`, streamed as [`stream_texture`](crate::gfx::Graphics::stream_texture)
    /// streams it: under the texture [residency budget](crate::gfx::Graphics::set_residency_budget),
    /// which may hold it at a lower resolution or evict it later. It counts
    /// as loaded once it is as sharp as the budget lets it be.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("level").streamed_texture("far/mountains.png");
    /// assert_eq!(group.len(), 1);
    /// ```
    #[must_use]
    pub fn streamed_texture(mut self, path: &str) -> Self {
        self.assets.push((path.to_string(), Kind::StreamedTexture));
        self
    }

    /// Adds mesh file `path`, streamed as [`stream_mesh`](crate::gfx::Graphics::stream_mesh)
    /// streams it, under the mesh residency budget. Once loaded,
    /// `load_mesh`, `resident_mesh` and `stream_mesh` return it.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("level").mesh("models/house.glb");
    /// assert_eq!(group.len(), 1);
    /// ```
    #[cfg(feature = "3d")]
    #[must_use]
    pub fn mesh(mut self, path: &str) -> Self {
        self.assets.push((path.to_string(), Kind::Mesh));
        self
    }

    /// Adds sound `path`, decoded on a worker, as [`Audio::load`](crate::audio::Audio::load)
    /// would decode it on the game thread. Once loaded, `ctx.audio.load(path)`
    /// returns it.
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("level").sound("music/level.ogg");
    /// assert_eq!(group.len(), 1);
    /// ```
    #[must_use]
    pub fn sound(mut self, path: &str) -> Self {
        self.assets.push((path.to_string(), Kind::Sound));
        self
    }

    /// Adds asset `path` with your own loader, the same shape
    /// [`Assets::load_with`] takes: `decode` gets the file's bytes on a
    /// worker and returns your value, or an [`Error`] (usually
    /// [`ErrorKind::Decode`]). Take the value with
    /// [`Assets::take_group_value`] once it is loaded. Any loader that
    /// decodes bytes plugs in here: a level format, a tile map, a sprite
    /// sheet's data. It may run more than once: again for each
    /// [restart](Assets::restart_group) after it failed.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// #[derive(serde::Deserialize)]
    /// struct Rules {
    ///     speed: f32,
    /// }
    /// let group = ponko::assets::AssetGroup::new("level").with("rules.json", |bytes| {
    ///     serde_json::from_slice::<Rules>(bytes).map_err(|e| Error::new(ErrorKind::Json, e.to_string()))
    /// });
    /// assert_eq!(group.len(), 1);
    /// ```
    #[must_use]
    pub fn with<T: Send + 'static>(
        mut self,
        path: &str,
        decode: impl Fn(&[u8]) -> Result<T, Error> + Send + Sync + 'static,
    ) -> Self {
        let decode: Decoder = Arc::new(move |bytes| decode(bytes).map(|value| Box::new(value) as Box<dyn Any + Send>));
        self.assets.push((path.to_string(), Kind::Value(decode)));
        self
    }

    /// Adds asset `path` as its bytes, a `Vec<u8>` to take with
    /// [`Assets::take_group_value`]: for files decoded on the game thread
    /// later (a C game's own formats).
    ///
    /// ```
    /// let group = ponko::assets::AssetGroup::new("level").bytes("level.bin");
    /// assert_eq!(group.len(), 1);
    /// ```
    #[must_use]
    pub fn bytes(self, path: &str) -> Self {
        self.with(path, |bytes| Ok(bytes.to_vec()))
    }
}

/// Where an [`AssetGroup`] is ([`Assets::group_state`]).
///
/// ```
/// use ponko::assets::GroupState;
/// assert_eq!(GroupState::Ready.name(), "ready");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GroupState {
    /// Its assets are being read, decoded and uploaded.
    Loading,
    /// Every asset is loaded.
    Ready,
    /// An asset failed to load: [`Assets::group_error`] says which and why.
    /// Nothing more of the group starts; what had loaded stays loaded, and
    /// the assets already being read finish on their workers and wait, so
    /// [`Assets::restart_group`] reads again only what failed and what never
    /// started. [`Assets::cancel_group`] drops what waits.
    Failed,
    /// Cancelled with [`Assets::cancel_group`] before it was ready.
    /// [`Assets::restart_group`] loads the rest.
    Cancelled,
}

impl GroupState {
    /// Its name in reports, such as `"loading"`.
    ///
    /// ```
    /// use ponko::assets::GroupState;
    /// assert_eq!(GroupState::Cancelled.name(), "cancelled");
    /// ```
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            GroupState::Loading => "loading",
            GroupState::Ready => "ready",
            GroupState::Failed => "failed",
            GroupState::Cancelled => "cancelled",
        }
    }
}

/// A started [`AssetGroup`], from [`Assets::start_group`]. Valid for the
/// whole game it was started in.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("empty"));
///     assert_eq!(ctx.assets.group("empty"), Some(id));
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(u32);

impl GroupId {
    /// Its number, for C and logs: groups are numbered from 1 in the order
    /// they were started.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.assets.start_group(ponko::assets::AssetGroup::new("a")).index(), 1);
    /// });
    /// ```
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// How far a group got ([`Assets::group_progress`]).
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("empty"));
///     let progress = ctx.assets.group_progress(id);
///     assert_eq!((progress.loaded, progress.total), (0, 0));
///     assert_eq!(progress.fraction(), 1.0);
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GroupProgress {
    /// Where the group is.
    pub state: GroupState,
    /// Assets loaded (read, decoded and uploaded).
    pub loaded: usize,
    /// Assets the group declares.
    pub total: usize,
    /// Bytes of the assets' files read so far.
    pub bytes_read: u64,
    /// Bytes of all the assets' files, as far as they are known: see
    /// [`bytes_known`](Self::bytes_known).
    pub bytes_total: u64,
    /// Whether `bytes_total` counts every asset. Sizes are known before
    /// reading for files in `assets/`, in packs and in sources that tell
    /// ([`AssetSource::size`](crate::assets::AssetSource::size)), when every
    /// added source is a [background](Assets::add_background_source) one;
    /// otherwise each is known once read.
    pub bytes_known: bool,
    /// Bytes of the assets loaded (each asset's file size), out of
    /// `bytes_total`.
    pub bytes_loaded: u64,
    /// Milliseconds the game thread spent uploading this group's textures:
    /// making each on the GPU and handing it its pixels. Measured, not
    /// estimated; 0 in a [headless](crate::game::Game::run_headless) game,
    /// which has no GPU. [Streamed](AssetGroup::streamed_texture) textures
    /// and meshes upload in the streaming step and count in
    /// [`upload_stats`](crate::gfx::Graphics::upload_stats) instead.
    pub upload_ms: f32,
    /// Bytes of this group's textures uploaded to the GPU (their size on the
    /// GPU, as the upload budget counts them).
    pub bytes_uploaded: u64,
}

impl GroupProgress {
    /// How much is done, from 0 to 1, for a progress bar: the share of the
    /// bytes loaded when every size is known, else of the assets; 1 once
    /// ready. It never goes back.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("empty"));
    ///     assert_eq!(ctx.assets.group_progress(id).fraction(), 1.0);
    /// });
    /// ```
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.state == GroupState::Ready || self.total == 0 {
            return 1.0;
        }
        if self.bytes_known && self.bytes_total > 0 {
            return (self.bytes_loaded as f64 / self.bytes_total as f64) as f32;
        }
        self.loaded as f32 / self.total as f32
    }
}

/// The game thread's share of loading asset groups
/// ([`Assets::group_stats`]): what the per-frame step cost.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let stats = ctx.assets.group_stats();
///     assert_eq!(stats.frame_budget_ms, ponko::assets::DEFAULT_GROUP_FRAME_BUDGET_MS);
///     assert_eq!(stats.steps, 0);
/// });
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct GroupStats {
    /// The time budget of each frame's step, in milliseconds.
    pub frame_budget_ms: f32,
    /// Frames that ran the step (frames with a group loading).
    pub steps: u64,
    /// Milliseconds the last step took on the game thread.
    pub step_ms: f32,
    /// The longest step so far, in milliseconds.
    pub max_step_ms: f32,
    /// Steps that took longer than the budget: one piece of work (a large
    /// texture made on the GPU, or in a browser a decode) can't be split.
    pub over_budget: u64,
    /// Bytes the last step uploaded (counted against the upload budget).
    pub frame_bytes: u64,
    /// Milliseconds the last step spent uploading: making textures on the
    /// GPU and handing them their pixels (part of `step_ms`). The GPU copies
    /// the pixels in its own time, while it draws the frame.
    pub upload_ms: f32,
    /// The most milliseconds one step spent uploading.
    pub max_upload_ms: f32,
    /// Milliseconds every step spent uploading, since the game started.
    pub total_upload_ms: f64,
    /// Bytes every step uploaded, since the game started.
    pub total_upload_bytes: u64,
}

/// The default time budget of the game thread's per-frame asset group
/// step, in milliseconds.
pub const DEFAULT_GROUP_FRAME_BUDGET_MS: f32 = 4.0;

/// Shared with the workers of one group: cancel, and bytes read per asset.
struct Shared {
    cancelled: AtomicBool,
    /// Each asset's file size once read; `u64::MAX` before.
    read: Vec<AtomicU64>,
}

impl Shared {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// What a worker made of an asset, for the game thread.
enum Done {
    Texture(Origin, DecodedTexture),
    Sound(Origin, crate::audio::DecodedSound),
    Value(Box<dyn Any + Send>),
}

/// A texture file, decoded: RGBA8 or GPU blocks.
enum DecodedTexture {
    Rgba(crate::assets::Image),
    #[cfg(feature = "compressed-textures")]
    Blocks(crate::compressed::CompressedImage),
}

impl DecodedTexture {
    fn bytes(&self) -> u64 {
        match self {
            DecodedTexture::Rgba(image) => crate::inventory::rgba8_bytes(image.width, image.height),
            #[cfg(feature = "compressed-textures")]
            DecodedTexture::Blocks(image) => image.bytes(),
        }
    }
}

/// The block formats the GPU samples (nothing to choose without the feature).
#[cfg(feature = "compressed-textures")]
type Formats = crate::compressed::GpuFormats;
#[cfg(not(feature = "compressed-textures"))]
#[derive(Clone, Copy)]
struct Formats;

/// Decodes a texture file as `load_texture` does, on any thread.
fn decode_texture(bytes: &[u8], path: &str, what: &str, gpu: Formats) -> Result<DecodedTexture, Error> {
    #[cfg(feature = "compressed-textures")]
    if let Some(decoded) = crate::compressed::decode(bytes, path, what, gpu) {
        return Ok(match decoded? {
            crate::compressed::Decoded::Rgba(image) => DecodedTexture::Rgba(image),
            crate::compressed::Decoded::Blocks(image) => DecodedTexture::Blocks(image),
        });
    }
    #[cfg(not(feature = "compressed-textures"))]
    let Formats = gpu;
    crate::assets::decode_image(bytes, path, what).map(DecodedTexture::Rgba)
}

/// Where one asset of a running group is.
enum Step {
    /// Not started (or failed, or dropped by a cancel: started again by a
    /// restart).
    Waiting,
    /// Read and decoded on a worker.
    Working(Job<Result<Done, Error>>),
    /// Read, and being decoded a slice at a time on the game thread (where
    /// there are no workers).
    Sliced(Box<Sliced>),
    /// Decoded, waiting for the game thread (and room in the upload budget).
    Decoded(Done),
    /// Handed to the streaming system: a texture or a mesh.
    Streaming { mesh: bool },
    /// Loaded.
    Finished,
}

/// A decode done a slice at a time ([`sliced`]).
struct Sliced {
    origin: Origin,
    work: SliceWork,
}

enum SliceWork {
    Texture(sliced::PngSlices),
    #[cfg(feature = "audio")]
    Sound(sliced::SoundSlices),
}

impl Sliced {
    /// Decodes the next slice; `Ok(true)` once it is all decoded.
    fn step(&mut self, path: &str) -> Result<bool, Error> {
        match &mut self.work {
            SliceWork::Texture(png) => png.step().map_err(|e| {
                Error::decode(
                    path,
                    format!("{} is not a valid image: {e}", self.origin.describe(path)),
                )
            }),
            #[cfg(feature = "audio")]
            SliceWork::Sound(sound) => sound
                .step()
                .map_err(|e| Error::decode(path, format!("{}: {e}", self.origin.describe(path)))),
        }
    }

    fn finish(self) -> Done {
        match self.work {
            SliceWork::Texture(png) => Done::Texture(self.origin, DecodedTexture::Rgba(png.finish())),
            #[cfg(feature = "audio")]
            SliceWork::Sound(sound) => Done::Sound(self.origin, sound.finish()),
        }
    }
}

/// One asset of a running group.
struct Item {
    path: String,
    kind: Kind,
    step: Step,
    /// Its size from the sizing job, if known before it is read.
    size: Option<u64>,
    /// Whether it loaded.
    loaded: bool,
}

/// A started group.
struct Running {
    name: String,
    items: Vec<Item>,
    /// The first item not started.
    next: usize,
    /// Items started and not finished.
    in_flight: usize,
    loaded: usize,
    shared: Arc<Shared>,
    state: GroupState,
    error: Option<Error>,
    values: HashMap<String, Box<dyn Any + Send>>,
    /// The sizes of the assets, worked out on a worker.
    sizes: Option<Job<Vec<Option<u64>>>>,
    /// Cancelled, and what it left loading stopped.
    stopped: bool,
    /// Restarted: the next step loads what had not loaded.
    restart: bool,
    /// Seconds the game thread spent uploading its textures.
    upload_secs: f64,
    /// Bytes of its textures uploaded.
    uploaded_bytes: u64,
}

impl Running {
    fn progress(&self) -> GroupProgress {
        let (mut bytes_read, mut bytes_total, mut bytes_loaded, mut known) = (0, 0, 0, true);
        for (item, read) in self.items.iter().zip(&self.shared.read) {
            let read = read.load(Ordering::Relaxed);
            let size = if read == u64::MAX {
                item.size
            } else {
                bytes_read += read;
                Some(read)
            };
            match size {
                Some(size) => {
                    bytes_total += size;
                    if item.loaded {
                        bytes_loaded += size;
                    }
                }
                None => known = false,
            }
        }
        GroupProgress {
            state: self.state,
            loaded: self.loaded,
            total: self.items.len(),
            bytes_read,
            bytes_total,
            bytes_known: known,
            bytes_loaded,
            upload_ms: (self.upload_secs * 1000.0) as f32,
            bytes_uploaded: self.uploaded_bytes,
        }
    }
}

/// Every asset group a game started (an [`Assets`] field).
pub(crate) struct Groups {
    list: Vec<Running>,
    /// Groups loading now.
    loading: usize,
    /// A group failed or was cancelled since the last step.
    stopping: bool,
    budget_ms: f32,
    stats: GroupStats,
    /// Tests: decode on the game thread a slice at a time (as where there
    /// are no workers) or not, instead of what the platform has.
    sliced: Option<bool>,
}

impl Default for Groups {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            loading: 0,
            stopping: false,
            sliced: None,
            budget_ms: DEFAULT_GROUP_FRAME_BUDGET_MS,
            stats: GroupStats {
                frame_budget_ms: DEFAULT_GROUP_FRAME_BUDGET_MS,
                ..GroupStats::default()
            },
        }
    }
}

/// Assets started and not finished per group: enough to keep the job
/// threads busy without decoding far ahead of the uploads.
fn max_in_flight() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map_or(4, |n| n.get().max(4))
    }
    #[cfg(target_arch = "wasm32")]
    {
        usize::MAX
    }
}

/// Whether [`Job`]s run where they are spawned, on the game thread: a
/// browser build without threads, or one whose page didn't start them.
fn jobs_run_here() -> bool {
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    {
        crate::web_threads::frame_threads() == 0
    }
    #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
    {
        true
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

impl Groups {
    /// Whether large decodes run a slice at a time on the game thread.
    fn sliced(&self) -> bool {
        self.sliced.unwrap_or_else(jobs_run_here)
    }

    fn get(&self, id: GroupId) -> &Running {
        let index = (id.0 as usize).wrapping_sub(1);
        self.list.get(index).unwrap_or_else(|| {
            panic!(
                "ponko: asset group {} was not started in this game; use the GroupId start_group returned",
                id.0
            )
        })
    }

    fn get_mut(&mut self, id: GroupId) -> &mut Running {
        let n = self.list.len();
        let index = (id.0 as usize).wrapping_sub(1);
        self.list.get_mut(index).unwrap_or_else(|| {
            panic!(
                "ponko: asset group {} was not started in this game (it has {n}); use the GroupId start_group returned",
                id.0
            )
        })
    }
}

impl Assets {
    /// Starts loading `group` in the background and returns its id. Nothing
    /// is read here: from the start of the next frame (before the game's
    /// systems), its files are read and decoded on worker threads and made
    /// ready on the game thread a few at a time, within
    /// [`set_group_frame_budget`](Self::set_group_frame_budget) and the
    /// upload budget. Watch it with [`group_progress`](Self::group_progress)
    /// or [`group_state`](Self::group_state).
    ///
    /// Assets already loaded the same way count as loaded at once. An empty
    /// group is ready at once.
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, GroupState, MemorySource};
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_background_source(MemorySource::new("mem").with("a.txt", "a").with("b.txt", "bb"));
    ///     let id = ctx.assets.start_group(AssetGroup::new("texts").bytes("a.txt").bytes("b.txt"));
    ///     assert_eq!(ctx.assets.group_state(id), GroupState::Loading);
    ///     while ctx.assets.group_state(id) == GroupState::Loading {
    ///         ctx.pump_asset_groups();
    ///     }
    ///     let progress = ctx.assets.group_progress(id);
    ///     assert_eq!((progress.loaded, progress.bytes_loaded), (2, 3));
    /// });
    /// ```
    pub fn start_group(&mut self, group: AssetGroup) -> GroupId {
        let groups = &mut self.groups;
        let id = GroupId(groups.list.len() as u32 + 1);
        let n = group.assets.len();
        let shared = Arc::new(Shared {
            cancelled: AtomicBool::new(false),
            read: (0..n).map(|_| AtomicU64::new(u64::MAX)).collect(),
        });
        // With every source readable by workers, plans read nothing: the
        // sizes can be found on a worker before any file is read.
        let sizes = (n > 0 && self.asset_sources.all_background()).then(|| {
            let plans: Vec<Option<BackgroundRead>> = group
                .assets
                .iter()
                .map(|(path, _)| match self.asset_sources.plan(path) {
                    ReadPlan::Later(read) => Some(read),
                    ReadPlan::Now(_) => None,
                })
                .collect();
            Job::spawn(move || {
                plans
                    .iter()
                    .map(|p| p.as_ref().and_then(BackgroundRead::size))
                    .collect()
            })
        });
        let state = if n == 0 { GroupState::Ready } else { GroupState::Loading };
        if state == GroupState::Loading {
            groups.loading += 1;
        }
        groups.list.push(Running {
            name: group.name,
            items: group
                .assets
                .into_iter()
                .map(|(path, kind)| Item {
                    path,
                    kind,
                    step: Step::Waiting,
                    size: None,
                    loaded: false,
                })
                .collect(),
            next: 0,
            in_flight: 0,
            loaded: 0,
            shared,
            state,
            error: None,
            values: HashMap::new(),
            sizes,
            stopped: false,
            restart: false,
            upload_secs: 0.0,
            uploaded_bytes: 0,
        });
        id
    }

    /// Every group started in this game, oldest first.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let a = ctx.assets.start_group(ponko::assets::AssetGroup::new("a"));
    ///     let b = ctx.assets.start_group(ponko::assets::AssetGroup::new("b"));
    ///     assert_eq!(ctx.assets.groups().collect::<Vec<_>>(), [a, b]);
    /// });
    /// ```
    pub fn groups(&self) -> impl ExactSizeIterator<Item = GroupId> + '_ {
        (0..self.groups.list.len()).map(|i| GroupId(i as u32 + 1))
    }

    /// The last group started with this name, if any.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.assets.group("level-1"), None);
    ///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("level-1"));
    ///     assert_eq!(ctx.assets.group("level-1"), Some(id));
    /// });
    /// ```
    #[must_use]
    pub fn group(&self, name: &str) -> Option<GroupId> {
        self.groups
            .list
            .iter()
            .rposition(|g| g.name == name)
            .map(|i| GroupId(i as u32 + 1))
    }

    /// Where group `id` is.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("empty"));
    ///     assert_eq!(ctx.assets.group_state(id), ponko::assets::GroupState::Ready);
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    #[must_use]
    pub fn group_state(&self, id: GroupId) -> GroupState {
        self.groups.get(id).state
    }

    /// How far group `id` got: assets and bytes loaded, for a progress bar.
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, MemorySource};
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_background_source(MemorySource::new("mem").with("a.bin", vec![0; 100]));
    ///     let id = ctx.assets.start_group(AssetGroup::new("one").bytes("a.bin"));
    ///     assert_eq!(ctx.assets.group_progress(id).total, 1);
    ///     ctx.finish_uploads();
    ///     let done = ctx.assets.group_progress(id);
    ///     assert_eq!((done.loaded, done.bytes_total, done.fraction()), (1, 100, 1.0));
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    #[must_use]
    pub fn group_progress(&self, id: GroupId) -> GroupProgress {
        self.groups.get(id).progress()
    }

    /// Why group `id` failed: the first error, whose
    /// [`path`](Error::path) names the asset. `None` unless it is
    /// [`GroupState::Failed`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let id = ctx.assets.start_group(ponko::assets::AssetGroup::new("broken").bytes("no/such.bin"));
    ///     ctx.finish_uploads();
    ///     let error = ctx.assets.group_error(id).unwrap();
    ///     assert_eq!((error.kind(), error.path()), (ErrorKind::NotFound, Some("no/such.bin")));
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    #[must_use]
    pub fn group_error(&self, id: GroupId) -> Option<&Error> {
        self.groups.get(id).error.as_ref()
    }

    /// Stops loading group `id`: nothing more of it is read, decoded or
    /// uploaded, and it becomes [`GroupState::Cancelled`]. What had already
    /// loaded stays loaded (free it as usual); a streamed texture already
    /// showing its small stand-in streams on. A [failed](GroupState::Failed)
    /// group is cancelled too, dropping what it read for a restart. Does
    /// nothing to a group that is ready or already cancelled.
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, GroupState, MemorySource};
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_background_source(MemorySource::new("mem").with("a.bin", "a"));
    ///     let id = ctx.assets.start_group(AssetGroup::new("one").bytes("a.bin"));
    ///     ctx.assets.cancel_group(id);
    ///     assert_eq!(ctx.assets.group_state(id), GroupState::Cancelled);
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    pub fn cancel_group(&mut self, id: GroupId) {
        let group = self.groups.get_mut(id);
        let was = group.state;
        if !matches!(was, GroupState::Loading | GroupState::Failed) {
            return;
        }
        group.state = GroupState::Cancelled;
        group.error = None;
        group.shared.cancelled.store(true, Ordering::Relaxed);
        if was == GroupState::Loading {
            self.groups.loading -= 1;
        }
        // Streams it started are stopped by the next step (it needs `gfx`).
        self.groups.stopping = true;
    }

    /// Loads the rest of a [failed](GroupState::Failed) or
    /// [cancelled](GroupState::Cancelled) group `id`: it is
    /// [`GroupState::Loading`] again, and from the next step it reads only
    /// the assets that had not loaded. What loaded stays loaded and is not
    /// read again, values not yet taken stay, and a failed group's assets
    /// that were already being read when it failed are taken as they are.
    /// For a loading screen's "try again" after a network error, or after
    /// fixing a broken file. Does nothing to a group that is loading or
    /// ready.
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, GroupState, MemorySource};
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_background_source(MemorySource::new("mem").with("a.bin", "a"));
    ///     let id = ctx.assets.start_group(AssetGroup::new("two").bytes("a.bin").bytes("b.bin"));
    ///     ctx.finish_uploads();
    ///     assert_eq!(ctx.assets.group_state(id), GroupState::Failed, "b.bin is missing");
    ///     // The missing file turns up (a download finished, say).
    ///     ctx.assets.add_background_source(MemorySource::new("late").with("b.bin", "b"));
    ///     ctx.assets.restart_group(id);
    ///     assert_eq!(ctx.assets.group_state(id), GroupState::Loading);
    ///     ctx.finish_uploads();
    ///     assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
    ///     assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "a.bin"), Some(b"a".to_vec()));
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    pub fn restart_group(&mut self, id: GroupId) {
        let group = self.groups.get_mut(id);
        if !matches!(group.state, GroupState::Failed | GroupState::Cancelled) {
            return;
        }
        group.state = GroupState::Loading;
        group.error = None;
        group.restart = true;
        self.groups.loading += 1;
    }

    /// Takes the value your loader (from [`AssetGroup::with`] or
    /// [`AssetGroup::bytes`]) made of asset `path` in group `id`: `None`
    /// before it is loaded, after it was taken, or if it isn't a `T`.
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, MemorySource};
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.add_background_source(MemorySource::new("mem").with("n.txt", "42"));
    ///     let group = AssetGroup::new("n").with("n.txt", |b| {
    ///         std::str::from_utf8(b).unwrap().parse::<u32>().map_err(|e| Error::new(ErrorKind::Decode, e.to_string()))
    ///     });
    ///     let id = ctx.assets.start_group(group);
    ///     ctx.finish_uploads();
    ///     assert_eq!(ctx.assets.take_group_value::<u32>(id, "n.txt"), Some(42));
    ///     assert_eq!(ctx.assets.take_group_value::<u32>(id, "n.txt"), None, "taken");
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` wasn't started in this game.
    pub fn take_group_value<T: 'static>(&mut self, id: GroupId, path: &str) -> Option<T> {
        let values = &mut self.groups.get_mut(id).values;
        if !values.get(path).is_some_and(|v| v.is::<T>()) {
            return None;
        }
        values.remove(path).and_then(|v| v.downcast::<T>().ok()).map(|v| *v)
    }

    /// Sets how long the game thread may spend on asset groups each frame,
    /// in milliseconds (default [`DEFAULT_GROUP_FRAME_BUDGET_MS`]). The step
    /// stops taking new work once it is spent; at least one piece of work
    /// runs each frame, so a group always moves on. Uploads are also held to
    /// the [upload budget](crate::gfx::Graphics::set_upload_budget) in bytes.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.assets.set_group_frame_budget(2.0);
    ///     assert_eq!(ctx.assets.group_stats().frame_budget_ms, 2.0);
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If `milliseconds` is negative or not finite.
    pub fn set_group_frame_budget(&mut self, milliseconds: f32) {
        assert!(
            milliseconds.is_finite() && milliseconds >= 0.0,
            "ponko: set_group_frame_budget: the budget must be 0 or more milliseconds (got {milliseconds})"
        );
        self.groups.budget_ms = milliseconds;
        self.groups.stats.frame_budget_ms = milliseconds;
    }

    /// What the game thread's asset group step cost: the last and longest
    /// step, and how many went over the budget.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.assets.group_stats().max_step_ms, 0.0);
    /// });
    /// ```
    #[must_use]
    pub fn group_stats(&self) -> GroupStats {
        self.groups.stats
    }
}

impl Ctx {
    /// The engine's asset group step, at the start of each frame before the
    /// game's systems (it runs after setup too): takes what the workers
    /// finished, uploads within the budgets and starts more. Call it
    /// yourself only to drive groups outside the game loop, as tests and
    /// the examples here do.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.pump_asset_groups(); // nothing loading: costs a comparison
    ///     assert_eq!(ctx.assets.group_stats().steps, 0);
    /// });
    /// ```
    #[doc(hidden)]
    pub fn pump_asset_groups(&mut self) {
        if self.assets.groups.stopping {
            self.stop_cancelled_streams();
        }
        if self.assets.groups.loading == 0 {
            return;
        }
        let start = crate::perf::Instant::now();
        let budget = f64::from(self.assets.groups.budget_ms) / 1000.0;
        let upload_budget = self.gfx.streaming.budget().unwrap_or(u64::MAX);
        let mut frame = Frame {
            start,
            budget,
            worked: false,
            upload_budget,
            spent: 0,
            uploads: 0,
            upload_secs: 0.0,
        };
        for index in 0..self.assets.groups.list.len() {
            if self.assets.groups.list[index].state == GroupState::Loading {
                self.pump_group(index, &mut frame);
            }
        }
        let (bytes, uploads) = &mut self.gfx.streaming.group_uploads;
        *bytes += frame.spent;
        *uploads += frame.uploads;
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        let stats = &mut self.assets.groups.stats;
        stats.steps += 1;
        stats.step_ms = ms;
        stats.max_step_ms = stats.max_step_ms.max(ms);
        stats.frame_bytes = frame.spent;
        let upload_ms = (frame.upload_secs * 1000.0) as f32;
        stats.upload_ms = upload_ms;
        stats.max_upload_ms = stats.max_upload_ms.max(upload_ms);
        stats.total_upload_ms += frame.upload_secs * 1000.0;
        stats.total_upload_bytes += frame.spent;
        if ms > self.assets.groups.budget_ms {
            stats.over_budget += 1;
        }
    }

    /// Runs the asset group and upload steps without their budgets until
    /// no group is loading: the group half of [`Ctx::finish_uploads`].
    pub(crate) fn finish_groups(&mut self) {
        let budget_ms = self.assets.groups.budget_ms;
        self.assets.groups.budget_ms = f32::INFINITY;
        while self.assets.groups.loading > 0 {
            self.pump_asset_groups();
            self.gfx.finish_uploads(&mut self.world);
            self.gfx.streaming.group_uploads = (0, 0);
            if self.assets.groups.loading > 0 {
                std::thread::yield_now();
            }
        }
        self.assets.groups.budget_ms = budget_ms;
    }

    /// One frame's step of group `index`.
    fn pump_group(&mut self, index: usize, frame: &mut Frame) {
        if self.assets.groups.list[index].restart {
            self.requeue_group(index);
        }
        let group = &mut self.assets.groups.list[index];
        if let Some(sizes) = group.sizes.as_mut()
            && let Some(sizes) = sizes.take()
        {
            for (item, size) in group.items.iter_mut().zip(sizes) {
                item.size = size;
            }
            group.sizes = None;
        }
        // Take what is ready, in order.
        for i in 0..self.assets.groups.list[index].items.len() {
            if let Err(error) = self.advance_item(index, i, frame) {
                self.item_failed(index, i, error);
                return;
            }
        }
        // Start more.
        let limit = max_in_flight();
        loop {
            let group = &mut self.assets.groups.list[index];
            while group
                .items
                .get(group.next)
                .is_some_and(|item| !matches!(item.step, Step::Waiting))
            {
                group.next += 1;
            }
            if group.next >= group.items.len() || group.in_flight >= limit || !frame.may_work() {
                break;
            }
            let i = group.next;
            group.next += 1;
            group.in_flight += 1;
            let started = self.start_item(index, i);
            frame.worked = true;
            // A browser decodes as it starts: take it at once.
            if let Err(error) = started.and_then(|()| self.advance_item(index, i, frame)) {
                self.item_failed(index, i, error);
                return;
            }
        }
        let group = &mut self.assets.groups.list[index];
        if group.loaded == group.items.len() {
            group.state = GroupState::Ready;
            self.assets.groups.loading -= 1;
        }
    }

    /// Group `index` was restarted: what had not loaded starts again from
    /// this step. A cancelled group's work is stopped first (if the step
    /// that stops it hasn't run yet), and its workers get a fresh cancel
    /// flag, so the work dropped by the cancel is never done.
    fn requeue_group(&mut self, index: usize) {
        if !self.assets.groups.list[index].stopped && self.assets.groups.list[index].shared.cancelled() {
            self.stop_group(index);
        }
        let group = &mut self.assets.groups.list[index];
        group.restart = false;
        group.stopped = false;
        group.next = 0;
        if group.shared.cancelled() {
            let read = group
                .items
                .iter()
                .zip(&group.shared.read)
                .map(|(item, read)| {
                    let size = if item.loaded {
                        read.load(Ordering::Relaxed)
                    } else {
                        u64::MAX
                    };
                    AtomicU64::new(size)
                })
                .collect();
            group.shared = Arc::new(Shared {
                cancelled: AtomicBool::new(false),
                read,
            });
        }
    }

    /// Starts item `i` of group `index`.
    fn start_item(&mut self, index: usize, i: usize) -> Result<(), Error> {
        let sliced = self.assets.groups.sliced();
        let group = &mut self.assets.groups.list[index];
        let item = &mut group.items[i];
        if !matches!(item.step, Step::Waiting) {
            return Ok(());
        }
        let kind = item.kind.clone();
        let path = item.path.clone();
        let shared = Arc::clone(&group.shared);
        shared.read[i].store(u64::MAX, Ordering::Relaxed);
        let plan = self.assets.asset_sources.plan(&path);
        // A mesh is parsed on the worker when the files it refers to can
        // be read there too.
        #[cfg(feature = "3d")]
        let others = match (&plan, self.assets.asset_sources.all_background()) {
            (ReadPlan::Later(read), true) => Some(read.clone()),
            _ => None,
        };
        let read = reader(plan, shared, i, path.clone());
        let step = match kind {
            Kind::Texture if sliced => {
                let (origin, bytes) = read()?;
                let bytes = Arc::new(bytes);
                match sliced::PngSlices::new(&bytes) {
                    Some(png) => Step::Sliced(Box::new(Sliced {
                        work: SliceWork::Texture(png),
                        origin,
                    })),
                    None => {
                        // Not a PNG it can split: in one piece, as before.
                        let gpu = texture_formats(&self.gfx);
                        let what = origin.describe(&path);
                        Step::Decoded(Done::Texture(origin, decode_texture(&bytes, &path, &what, gpu)?))
                    }
                }
            }
            Kind::Texture => {
                let gpu = texture_formats(&self.gfx);
                Step::Working(Job::spawn(move || {
                    let (origin, bytes) = read()?;
                    let what = origin.describe(&path);
                    Ok(Done::Texture(origin, decode_texture(&bytes, &path, &what, gpu)?))
                }))
            }
            Kind::StreamedTexture => {
                self.gfx.stream_texture_from(&path, read);
                Step::Streaming { mesh: false }
            }
            #[cfg(feature = "3d")]
            Kind::Mesh => {
                self.gfx
                    .stream_mesh_from(&path, move || read().map(|(origin, bytes)| (origin, bytes, others)));
                Step::Streaming { mesh: true }
            }
            #[cfg(feature = "audio")]
            Kind::Sound if sliced => {
                let (origin, bytes) = read()?;
                let sound = sliced::SoundSlices::new(bytes)
                    .map_err(|e| Error::decode(&path, format!("{}: {e}", origin.describe(&path))))?;
                Step::Sliced(Box::new(Sliced {
                    work: SliceWork::Sound(sound),
                    origin,
                }))
            }
            Kind::Sound => Step::Working(Job::spawn(move || {
                let (origin, bytes) = read()?;
                let sound = crate::audio::decode_sound(bytes)
                    .map_err(|e| Error::decode(&path, format!("{}: {e}", origin.describe(&path))))?;
                Ok(Done::Sound(origin, sound))
            })),
            Kind::Value(decode) => Step::Working(Job::spawn(move || {
                let (origin, bytes) = read()?;
                decode(&bytes).map(Done::Value).map_err(|e| {
                    e.context(format!("{} could not be decoded", origin.describe(&path)))
                        .with_path(&path)
                })
            })),
        };
        self.assets.groups.list[index].items[i].step = step;
        Ok(())
    }

    /// Moves item `i` of group `index` on as far as it can go this frame.
    fn advance_item(&mut self, index: usize, i: usize, frame: &mut Frame) -> Result<(), Error> {
        let item = &mut self.assets.groups.list[index].items[i];
        if let Step::Working(job) = &mut item.step {
            match job.take() {
                None => return Ok(()),
                Some(done) => item.step = Step::Decoded(done.map_err(|e| path_of(e, &item.path))?),
            }
        }
        if let Step::Sliced(work) = &mut item.step {
            // A slice at a time while the frame's budget lasts.
            loop {
                if !frame.may_work() {
                    return Ok(());
                }
                frame.worked = true;
                if work.step(&item.path)? {
                    break;
                }
            }
            let Step::Sliced(work) = std::mem::replace(&mut item.step, Step::Finished) else {
                unreachable!("matched above")
            };
            item.step = Step::Decoded(work.finish());
        }
        let finished = match &item.step {
            Step::Decoded(done) => {
                let bytes = match done {
                    Done::Texture(_, texture) => texture.bytes(),
                    _ => 0,
                };
                if !frame.may_upload(bytes) {
                    return Ok(());
                }
                let Step::Decoded(done) = std::mem::replace(&mut item.step, Step::Finished) else {
                    unreachable!("matched above")
                };
                let path = item.path.clone();
                match done {
                    Done::Texture(origin, texture) => {
                        let started = crate::perf::Instant::now();
                        adopt_texture(&mut self.gfx, &path, origin, texture);
                        let secs = started.elapsed().as_secs_f64();
                        frame.spent += bytes;
                        frame.uploads += 1;
                        frame.upload_secs += secs;
                        let group = &mut self.assets.groups.list[index];
                        group.upload_secs += secs;
                        group.uploaded_bytes += bytes;
                    }
                    Done::Sound(origin, sound) => {
                        self.audio.adopt_decoded(&path, origin, sound);
                    }
                    Done::Value(value) => {
                        self.assets.groups.list[index].values.insert(path, value);
                    }
                }
                frame.worked = true;
                true
            }
            Step::Streaming { mesh } => {
                let settled = if *mesh {
                    #[cfg(feature = "3d")]
                    {
                        self.gfx.streamed_mesh_settled(&item.path)
                    }
                    #[cfg(not(feature = "3d"))]
                    {
                        Some(Ok(()))
                    }
                } else {
                    self.gfx.streamed_texture_settled(&item.path)
                };
                let item = &mut self.assets.groups.list[index].items[i];
                match settled {
                    None => false,
                    Some(result) => {
                        result.map_err(|e| path_of(e, &item.path))?;
                        item.step = Step::Finished;
                        true
                    }
                }
            }
            Step::Waiting | Step::Working(_) | Step::Sliced(_) | Step::Finished => false,
        };
        if finished {
            let group = &mut self.assets.groups.list[index];
            group.items[i].loaded = true;
            group.loaded += 1;
            group.in_flight -= 1;
        }
        Ok(())
    }

    /// Item `i` of group `index` failed with `error`: it waits to be started
    /// again by a restart, and the group fails.
    fn item_failed(&mut self, index: usize, i: usize, error: Error) {
        let group = &mut self.assets.groups.list[index];
        group.items[i].step = Step::Waiting;
        group.in_flight -= 1;
        self.fail_group(index, error);
    }

    /// Group `index` failed with `error`: nothing more of it starts. What
    /// is on workers or streaming finishes and waits for a restart.
    fn fail_group(&mut self, index: usize, error: Error) {
        let group = &mut self.assets.groups.list[index];
        group.state = GroupState::Failed;
        group.error = Some(error);
        self.assets.groups.loading -= 1;
    }

    /// Stops what cancelled groups left loading: their jobs' results are
    /// dropped and their streams that show nothing yet end.
    fn stop_cancelled_streams(&mut self) {
        self.assets.groups.stopping = false;
        for index in 0..self.assets.groups.list.len() {
            let group = &self.assets.groups.list[index];
            if group.stopped || group.state != GroupState::Cancelled {
                continue;
            }
            self.stop_group(index);
        }
    }

    /// Stops what group `index` left loading: every asset that had not
    /// loaded waits again (for a restart), its job dropped (the workers see
    /// the cancel and skip it) and its stream ended if it shows nothing.
    fn stop_group(&mut self, index: usize) {
        let group = &mut self.assets.groups.list[index];
        group.stopped = true;
        group.sizes = None;
        group.in_flight = 0;
        for i in 0..group.items.len() {
            let item = &mut self.assets.groups.list[index].items[i];
            if item.loaded {
                continue;
            }
            let step = std::mem::replace(&mut item.step, Step::Waiting);
            if let Step::Streaming { mesh } = step {
                let path = item.path.clone();
                if mesh {
                    #[cfg(feature = "3d")]
                    self.gfx.cancel_mesh_stream(&path);
                } else {
                    self.gfx.cancel_texture_stream(&path);
                }
            }
        }
    }
}

/// The time and bytes one frame's step may still spend.
struct Frame {
    start: crate::perf::Instant,
    /// Seconds.
    budget: f64,
    /// Whether any work ran yet (the first piece always may).
    worked: bool,
    upload_budget: u64,
    spent: u64,
    uploads: u32,
    /// Seconds spent uploading textures.
    upload_secs: f64,
}

impl Frame {
    fn may_work(&self) -> bool {
        !self.worked || self.start.elapsed().as_secs_f64() < self.budget
    }

    /// Whether `bytes` more may be uploaded now: within the time and the
    /// upload budget, or the frame's first upload (which may be larger).
    fn may_upload(&self, bytes: u64) -> bool {
        if !self.may_work() {
            return false;
        }
        bytes == 0 || self.uploads == 0 || self.spent + bytes <= self.upload_budget
    }
}

/// An error about item `path`, naming it if it names nothing.
fn path_of(error: Error, path: &str) -> Error {
    if error.path().is_some() {
        error
    } else {
        error.with_path(path)
    }
}

/// Reads an asset for a group on a worker: nothing if the group was
/// cancelled, and its size counted once read.
fn reader(
    plan: ReadPlan,
    shared: Arc<Shared>,
    i: usize,
    path: String,
) -> impl FnOnce() -> Result<(Origin, Vec<u8>), Error> + Send + 'static {
    move || {
        if shared.cancelled() {
            return Err(Error::new(ErrorKind::Invalid, format!("loading \"{path}\" was cancelled")).with_path(path));
        }
        let read = match plan {
            ReadPlan::Now(read) => read,
            ReadPlan::Later(read) => read.read(),
        };
        if let Ok((_, bytes)) = &read {
            shared.read[i].store(bytes.len() as u64, Ordering::Relaxed);
        }
        read
    }
}

#[cfg(feature = "compressed-textures")]
fn texture_formats(gfx: &crate::gfx::Graphics) -> Formats {
    gfx.compressed_formats()
}

#[cfg(not(feature = "compressed-textures"))]
fn texture_formats(_: &crate::gfx::Graphics) -> Formats {
    Formats
}

/// Makes a texture a group decoded, as `load_texture` would have: cached by
/// where it came from (one loaded meanwhile is kept instead), named, and
/// watched if it came from `assets/`.
fn adopt_texture(gfx: &mut crate::gfx::Graphics, path: &str, origin: Origin, decoded: DecodedTexture) {
    use crate::asset_cache::AssetKind;
    let texture = match gfx.loaded_textures.get(&origin, AssetKind::Texture) {
        Some(texture) => texture,
        None => {
            let texture = match decoded {
                DecodedTexture::Rgba(image) => gfx.create_texture(image.width, image.height, &image.rgba),
                #[cfg(feature = "compressed-textures")]
                DecodedTexture::Blocks(image) => {
                    let texture = gfx.make_compressed_texture(&image);
                    gfx.textures_live.insert(texture.id);
                    texture
                }
            };
            gfx.asset_ledger
                .own(crate::inventory::AssetClass::Texture, texture.id, path);
            gfx.loaded_textures.insert(origin.clone(), AssetKind::Texture, texture);
            if let Origin::File(file) = origin {
                gfx.watcher
                    .watch(path, file, crate::hot_reload::Asset::Texture(texture));
            }
            texture
        }
    };
    gfx.texture_names.add(texture.id, path, texture);
}

impl crate::game::Game {
    /// Makes state `loading` a loading screen: entering it starts the
    /// [`AssetGroup`] that `group` declares, and once that group is
    /// [ready](GroupState::Ready) the game moves to state `then`, at the
    /// end of the tick that sees it (a simulation system of the engine's,
    /// in [`Phase::Input`](crate::game::Phase::Input), run only in
    /// `loading`, asks for the change). Leaving `loading` before the group
    /// is ready (a back button) [cancels](Assets::cancel_group) it. A group
    /// that fails keeps the game in `loading`: read
    /// [`group_error`](Assets::group_error) to show what went wrong.
    ///
    /// The loading screen's own systems run in `loading` as any state's do,
    /// and find the group by its name with [`Assets::group`] to draw its
    /// [progress](Assets::group_progress). The group loads at the start of
    /// every frame, also while the simulation is paused; the state changes
    /// only on a tick, as every state change does. In lockstep games,
    /// where every peer must change state on the same tick, decide the
    /// change with the session instead (peers load at different speeds).
    ///
    /// ```
    /// use ponko::assets::{AssetGroup, MemorySource};
    /// use ponko::prelude::*;
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    /// enum Screen { Loading, Playing }
    ///
    /// let world = Game::new("t", 1, 1)
    ///     .state(Screen::Loading)
    ///     .loading_state(Screen::Loading, |_ctx| AssetGroup::new("level").bytes("level.bin"), Screen::Playing)
    ///     .on_enter(Screen::Playing, |ctx: &mut Ctx| {
    ///         let id = ctx.assets.group("level").unwrap();
    ///         let level: Vec<u8> = ctx.assets.take_group_value(id, "level.bin").unwrap();
    ///         assert_eq!(level, [1, 2, 3]);
    ///     })
    ///     // Headless ticks take no time; real frames give the worker time to read.
    ///     .sim_system(|_: &mut Ctx| std::thread::sleep(std::time::Duration::from_millis(2)))
    ///     .run_headless(200, |ctx| {
    ///         ctx.assets.add_background_source(MemorySource::new("mem").with("level.bin", vec![1, 2, 3]));
    ///     });
    /// assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Playing);
    /// ```
    pub fn loading_state<S: crate::state::States>(
        self,
        loading: S,
        mut group: impl FnMut(&mut Ctx) -> AssetGroup + 'static,
        then: S,
    ) -> Self {
        use crate::game::IntoSystem;
        let started = std::rc::Rc::new(std::cell::Cell::new(None::<GroupId>));
        let (on_exit, check) = (started.clone(), started.clone());
        self.on_enter(loading.clone(), move |ctx: &mut Ctx| {
            let declared = group(ctx);
            started.set(Some(ctx.assets.start_group(declared)));
        })
        .on_exit(loading.clone(), move |ctx: &mut Ctx| {
            if let Some(id) = on_exit.take() {
                ctx.assets.cancel_group(id);
            }
        })
        .add(
            crate::game::Phase::Input,
            (move |ctx: &mut Ctx| {
                if let Some(id) = check.get()
                    && ctx.assets.group_state(id) == GroupState::Ready
                {
                    ctx.set_state(then.clone());
                }
            })
            .run_if(crate::state::in_state(loading)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::MemorySource;
    use crate::prelude::*;

    /// A PNG of `width` x `height` noise, which decodes as slowly as a
    /// photo does.
    fn noise_png(width: u32, height: u32) -> Vec<u8> {
        let mut s = 0x2545_F491u32;
        let image = image::RgbaImage::from_fn(width, height, |_, _| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            image::Rgba([s as u8, (s >> 8) as u8, (s >> 16) as u8, 255])
        });
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// Runs the group step until `id` stops loading; each step's time.
    fn steps_until_done(ctx: &mut Ctx, id: GroupId) -> Vec<f32> {
        let mut steps = Vec::new();
        while ctx.assets.group_state(id) == GroupState::Loading {
            ctx.pump_asset_groups();
            steps.push(ctx.assets.group_stats().step_ms);
            assert!(steps.len() < 100_000, "the group never finished");
        }
        steps
    }

    #[test]
    fn without_workers_a_large_png_is_decoded_a_slice_per_frame_at_a_zero_budget() {
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.assets.groups.sliced = Some(true);
            ctx.assets
                .add_background_source(MemorySource::new("mem").with("big.png", noise_png(1024, 1024)));
            // No time at all: each step runs exactly one piece of work.
            ctx.assets.set_group_frame_budget(0.0);
            let id = ctx.assets.start_group(AssetGroup::new("big").texture("big.png"));
            let steps = steps_until_done(ctx, id);
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
            let texture = ctx.gfx.load_texture("big.png");
            assert_eq!((texture.width(), texture.height()), (1024, 1024));
            // Reading the file is the first step's one piece of work; then
            // 16 rows of 1024 pixels a slice, 64 slices; then the upload,
            // which waits for a step of its own.
            assert_eq!(steps.len(), 1 + 64 + 1, "one slice a step");
        });
    }

    #[test]
    fn without_workers_a_long_png_decode_keeps_each_step_near_the_frame_budget() {
        let png = noise_png(2048, 2048);
        // The decode in one piece, as a browser without threads did it.
        let started = std::time::Instant::now();
        let whole = crate::assets::decode_image(&png, "big.png", "big.png").unwrap();
        let whole_ms = started.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(whole.width, 2048);
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.assets.groups.sliced = Some(true);
            ctx.assets
                .add_background_source(MemorySource::new("mem").with("big.png", png));
            // Below the decode's few milliseconds here (a browser's is
            // slower), so it must be spread.
            let budget = 0.5;
            ctx.assets.set_group_frame_budget(budget);
            let id = ctx.assets.start_group(AssetGroup::new("big").texture("big.png"));
            let mut steps = steps_until_done(ctx, id);
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
            let total: f32 = steps.iter().sum();
            steps.sort_by(f32::total_cmp);
            let median = steps[steps.len() / 2];
            eprintln!(
                "one piece: {whole_ms:.1} ms; sliced at a {budget} ms budget: {} steps, median {median:.2} ms, \
                 longest {:.2} ms, {total:.1} ms in all",
                steps.len(),
                steps[steps.len() - 1]
            );
            // Spread over frames, each about the budget long. Coarse bounds
            // only (a busy machine pauses threads): the decode takes far
            // more than one step, and a typical step is near the budget.
            assert!(
                steps.len() as f32 >= (whole_ms / (budget + 3.0)).floor(),
                "{} steps",
                steps.len()
            );
            assert!(steps.len() >= 3, "{} steps", steps.len());
            assert!(median < budget + 3.0, "median step {median} ms");
        });
    }

    #[cfg(feature = "audio")]
    #[test]
    fn without_workers_a_long_sound_is_decoded_over_several_frames() {
        // Twenty seconds of mono 16-bit sound at 44.1 kHz.
        let frames = 20 * 44_100u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for field in [
            16u32.to_le_bytes(),
            [1, 0, 1, 0],
            44_100u32.to_le_bytes(),
            88_200u32.to_le_bytes(),
            [2, 0, 16, 0],
        ] {
            wav.extend_from_slice(&field);
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(frames * 2).to_le_bytes());
        for i in 0..frames {
            wav.extend_from_slice(&((i % 200) as i16 * 100 - 10_000).to_le_bytes());
        }
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.assets.groups.sliced = Some(true);
            ctx.assets
                .add_background_source(MemorySource::new("mem").with("music.wav", wav));
            ctx.assets.set_group_frame_budget(0.0);
            let id = ctx.assets.start_group(AssetGroup::new("music").sound("music.wav"));
            let steps = steps_until_done(ctx, id);
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
            // About 8192 frames a slice (whole packets, so a little more):
            // around a hundred steps for 882 000 frames.
            assert!(steps.len() > 50, "{} steps", steps.len());
            let sound = ctx.audio.load("music.wav");
            assert!((sound.seconds() - 20.0).abs() < 0.01, "{} s", sound.seconds());
        });
    }

    #[test]
    fn without_workers_other_assets_load_as_before() {
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.assets.groups.sliced = Some(true);
            let gif = b"GIF89a".to_vec();
            ctx.assets.add_background_source(
                MemorySource::new("mem")
                    .with("n.txt", "7")
                    .with("small.png", noise_png(4, 4))
                    .with("not-png.png", gif),
            );
            let group = AssetGroup::new("n").with("n.txt", |b| Ok(b.len())).texture("small.png");
            let id = ctx.assets.start_group(group);
            steps_until_done(ctx, id);
            assert_eq!(ctx.assets.take_group_value::<usize>(id, "n.txt"), Some(1));
            assert_eq!(ctx.gfx.load_texture("small.png").width(), 4);
            // A texture it can't split decodes in one piece, failing as before.
            let id = ctx.assets.start_group(AssetGroup::new("bad").texture("not-png.png"));
            steps_until_done(ctx, id);
            assert_eq!(
                ctx.assets.group_error(id).unwrap().kind(),
                ErrorKind::Unsupported,
                "GIF is not built in"
            );
        });
    }

    #[test]
    fn without_workers_a_broken_png_fails_the_group_with_a_decode_error() {
        let mut png = noise_png(256, 256);
        png.truncate(png.len() / 2);
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.assets.groups.sliced = Some(true);
            ctx.assets
                .add_background_source(MemorySource::new("mem").with("cut.png", png));
            let id = ctx.assets.start_group(AssetGroup::new("cut").texture("cut.png"));
            steps_until_done(ctx, id);
            let error = ctx.assets.group_error(id).unwrap();
            assert_eq!((error.kind(), error.path()), (ErrorKind::Decode, Some("cut.png")));
            assert!(error.message().contains("is not a valid image"), "{error}");
        });
    }
}
