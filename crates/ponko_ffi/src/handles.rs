//! One handle lifecycle (Rust `Handle`, `Ctx::is_live`, `Ctx::free`,
//! `Ctx::live_counts`): every handle struct starts with its `uint32_t id`,
//! so one pair of calls takes a kind and an id for all of them.
//!
//! Rust gives each kind its own type, so a wrong kind doesn't compile. C
//! keeps integer ids and checks the kind at run time: the kind argument
//! says which table the id is looked up in, an unknown kind is an invalid
//! argument, and so is freeing a texture that isn't a render target as
//! `PONKO_HANDLE_RENDER_TARGET`. Ids of different kinds can be equal, so
//! an id passed with another kind's value names nothing or something else
//! of that kind.

use std::ffi::c_char;

use ponko::__ffi;
use ponko::handle::LiveCounts;
use ponko::inventory::{AssetClass, AssetOwner, AssetOwnerRef, AssetRecordRef};
use ponko::prelude::{Font, Sound, Texture};
use ponko::render::Compute;
use ponko::render::Effect;
use ponko::render::GpuBuffer;
use ponko::render::InstanceBatch;
use ponko::render::Material;
use ponko::render::Mesh;
use ponko::render::RenderTarget;
use ponko::render::Shader;
use ponko::render::{SpriteMaterial, SpriteShader};
use ponko::terrain::TextureArray;

use crate::ctx::{ctx_access, out, text};
use crate::{PonkoCtx, PonkoStatus, boundary, invalid};

/// The id no handle of any kind ever has (Rust `Handle::NONE`): never
/// live, and freeing it does nothing. `PONKO_MESH_NONE` is the same value.
/// ```
/// assert_eq!(ponko_ffi::PONKO_HANDLE_NONE, u32::MAX);
/// ```
pub const PONKO_HANDLE_NONE: u32 = u32::MAX;

/// Which kind of handle `ponko_ctx_handle_is_live` and
/// `ponko_ctx_handle_free` take (pass it as a `uint32_t`). A render target
/// is a texture (a `PonkoTexture`): `PONKO_HANDLE_TEXTURE` covers it too,
/// and `PONKO_HANDLE_RENDER_TARGET` also checks that it is one.
/// ```
/// assert_eq!(ponko_ffi::PonkoHandleKind::PONKO_HANDLE_TEXTURE_ARRAY as u32, 10);
/// assert_eq!(ponko_ffi::PonkoHandleKind::PONKO_HANDLE_RENDER_TARGET as u32, 13);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoHandleKind {
    /// `PonkoTexture`, render targets included.
    PONKO_HANDLE_TEXTURE = 0,
    /// `PonkoSound`.
    PONKO_HANDLE_SOUND = 1,
    /// `PonkoFont` (the built-in font is never freed).
    PONKO_HANDLE_FONT = 2,
    /// `PonkoMesh`.
    PONKO_HANDLE_MESH = 3,
    /// `PonkoMaterial`.
    PONKO_HANDLE_MATERIAL = 4,
    /// `PonkoShader`.
    PONKO_HANDLE_SHADER = 5,
    /// `PonkoCompute`.
    PONKO_HANDLE_COMPUTE = 6,
    /// `PonkoEffect`.
    PONKO_HANDLE_EFFECT = 7,
    /// `PonkoGpuBuffer`.
    PONKO_HANDLE_GPU_BUFFER = 8,
    /// `PonkoInstanceBatch`.
    PONKO_HANDLE_INSTANCE_BATCH = 9,
    /// `PonkoTextureArray`.
    PONKO_HANDLE_TEXTURE_ARRAY = 10,
    /// `PonkoSpriteShader`.
    PONKO_HANDLE_SPRITE_SHADER = 11,
    /// `PonkoSpriteMaterial`.
    PONKO_HANDLE_SPRITE_MATERIAL = 12,
    /// A `PonkoTexture` from `ponko_ctx_create_render_target` (Rust
    /// `RenderTarget`): live only while it is a live render target, and
    /// freeing a texture that isn't one is an invalid argument.
    PONKO_HANDLE_RENDER_TARGET = 13,
}

/// The value of `PonkoHandleKind::PONKO_HANDLE_RENDER_TARGET`, for matching on a `uint32_t`.
const RENDER_TARGET: u32 = PonkoHandleKind::PONKO_HANDLE_RENDER_TARGET as u32;

/// How many handles of each kind are live (Rust `LiveCounts`), from
/// `ponko_ctx_live_counts`.
/// ```
/// assert_eq!(ponko_ffi::PonkoLiveCounts::default().textures, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoLiveCounts {
    /// Textures the game made or loaded, render targets included.
    pub textures: usize,
    /// Of `textures`, the render targets.
    pub render_targets: usize,
    /// Meshes.
    pub meshes: usize,
    /// Materials.
    pub materials: usize,
    /// Material shaders.
    pub shaders: usize,
    /// Compute passes.
    pub computes: usize,
    /// Screen effects.
    pub effects: usize,
    /// GPU buffers.
    pub gpu_buffers: usize,
    /// Instance batches.
    pub instance_batches: usize,
    /// Texture arrays.
    pub texture_arrays: usize,
    /// Sounds.
    pub sounds: usize,
    /// Fonts the game loaded (not the built-in one).
    pub fonts: usize,
}

impl From<LiveCounts> for PonkoLiveCounts {
    fn from(c: LiveCounts) -> Self {
        Self {
            textures: c.textures,
            render_targets: c.render_targets,
            meshes: c.meshes,
            materials: c.materials,
            shaders: c.shaders,
            computes: c.computes,
            effects: c.effects,
            gpu_buffers: c.gpu_buffers,
            instance_batches: c.instance_batches,
            texture_arrays: c.texture_arrays,
            sounds: c.sounds,
            fonts: c.fonts,
        }
    }
}

/// Calls `f` with the handle of kind `kind` and id `id`.
macro_rules! with_handle {
    ($kind:expr, $id:expr, |$h:ident| $body:expr) => {
        match $kind {
            0 => {
                let $h = __ffi::from_id::<Texture>($id);
                Ok($body)
            }
            1 => {
                let $h = __ffi::from_id::<Sound>($id);
                Ok($body)
            }
            2 => {
                let $h = __ffi::from_id::<Font>($id);
                Ok($body)
            }
            3 => {
                let $h = __ffi::from_id::<Mesh>($id);
                Ok($body)
            }
            4 => {
                let $h = __ffi::from_id::<Material>($id);
                Ok($body)
            }
            5 => {
                let $h = __ffi::from_id::<Shader>($id);
                Ok($body)
            }
            6 => {
                let $h = __ffi::from_id::<Compute>($id);
                Ok($body)
            }
            7 => {
                let $h = __ffi::from_id::<Effect>($id);
                Ok($body)
            }
            8 => {
                let $h = __ffi::from_id::<GpuBuffer>($id);
                Ok($body)
            }
            9 => {
                let $h = __ffi::from_id::<InstanceBatch>($id);
                Ok($body)
            }
            10 => {
                let $h = __ffi::from_id::<TextureArray>($id);
                Ok($body)
            }
            11 => {
                let $h = __ffi::from_id::<SpriteShader>($id);
                Ok($body)
            }
            12 => {
                let $h = __ffi::from_id::<SpriteMaterial>($id);
                Ok($body)
            }
            RENDER_TARGET => {
                let $h = __ffi::from_id::<RenderTarget>($id);
                Ok($body)
            }
            _ => Err(invalid("unknown handle kind")),
        }
    };
}

/// Whether the handle of `kind` (a `PonkoHandleKind`) with `id` names
/// something this game made and hasn't freed (Rust `Ctx::is_live`). False
/// for `PONKO_HANDLE_NONE`.
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// let mut live = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_handle_is_live(std::ptr::null_mut(), 0, 0, &mut live) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_handle_is_live(
    ctx: *mut PonkoCtx,
    kind: u32,
    id: u32,
    out_live: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let live = with_handle!(kind, id, |h| c.is_live(h))?;
            out(out_live, live)
        })
    })
}

/// Frees the handle of `kind` (a `PonkoHandleKind`) with `id` (Rust
/// `Ctx::free`): its memory goes, and its id is never reused (except for
/// meshes and GPU buffers, whose next one made takes it). Whatever still uses it draws the default: nothing for a
/// mesh, white for a texture, the default material, the built-in shader,
/// sprite look or font; effects and compute passes are skipped and sounds stop. Freeing it
/// again, or `PONKO_HANDLE_NONE`, does nothing. A live texture that isn't a
/// render target, freed as `PONKO_HANDLE_RENDER_TARGET`, is an invalid
/// argument and stays live.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_handle_free(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_handle_free(ctx: *mut PonkoCtx, kind: u32, id: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if kind == RENDER_TARGET {
                let texture = __ffi::from_id::<Texture>(id);
                if c.is_live(texture) && !c.gfx.is_render_target(texture) {
                    return Err(invalid(
                        "that texture isn't a render target: free it as PONKO_HANDLE_TEXTURE",
                    ));
                }
            }
            with_handle!(kind, id, |h| c.free(h))
        })
    })
}

/// How many handles of each kind are live now (Rust `Ctx::live_counts`).
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// let mut counts = ponko_ffi::PonkoLiveCounts::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_live_counts(std::ptr::null_mut(), &mut counts) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_live_counts(ctx: *mut PonkoCtx, out_counts: *mut PonkoLiveCounts) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_counts, c.live_counts().into())) })
}

/// A class of asset handle in the asset inventory (Rust `AssetClass`), for
/// `ponko_ctx_asset_count` and `ponko_ctx_asset_handles` (pass it as a
/// `uint32_t`). Unlike `PonkoHandleKind`, textures here count the engine's
/// own too, and render targets are a class of their own.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetClass::PONKO_ASSET_CLASS_SOUND as u32, 9);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAssetClass {
    /// Images, the engine's own (glyph atlases, UI images) included.
    PONKO_ASSET_CLASS_TEXTURE = 0,
    /// Textures cameras draw into.
    PONKO_ASSET_CLASS_RENDER_TARGET = 1,
    /// Texture arrays.
    PONKO_ASSET_CLASS_TEXTURE_ARRAY = 2,
    /// Materials.
    PONKO_ASSET_CLASS_MATERIAL = 3,
    /// Material shaders.
    PONKO_ASSET_CLASS_SHADER = 4,
    /// Meshes.
    PONKO_ASSET_CLASS_MESH = 5,
    /// GPU buffers.
    PONKO_ASSET_CLASS_GPU_BUFFER = 6,
    /// Screen effects.
    PONKO_ASSET_CLASS_EFFECT = 7,
    /// Compute passes.
    PONKO_ASSET_CLASS_COMPUTE = 8,
    /// Sounds.
    PONKO_ASSET_CLASS_SOUND = 9,
}

/// Who made a handle (Rust `AssetOwner`), in `PonkoAssetRecord::owner`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetOwner::PONKO_ASSET_OWNER_ENGINE as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAssetOwner {
    /// Game code (`ponko_ctx_create_texture`, ...).
    PONKO_ASSET_OWNER_CODE = 0,
    /// Loading a named asset file; `ponko_ctx_asset_owner` gives its name.
    PONKO_ASSET_OWNER_FILE = 1,
    /// The engine itself; the game can't free it.
    PONKO_ASSET_OWNER_ENGINE = 2,
}

/// How many handles of one class are live and their estimated bytes (Rust
/// `AssetCount`), from `ponko_ctx_asset_count`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetCount::default().bytes, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoAssetCount {
    /// Handles made and not freed.
    pub live: usize,
    /// Estimated bytes they hold.
    pub bytes: u64,
}

/// One live handle with its bytes, age and owner (Rust `AssetRecord`), from
/// `ponko_ctx_asset_handles`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetRecord::default().made_frame, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoAssetRecord {
    /// Its id.
    pub id: u32,
    /// Who made it (a `PonkoAssetOwner`).
    pub owner: u32,
    /// Estimated bytes it holds.
    pub bytes: u64,
    /// The frame it was made on.
    pub made_frame: u64,
    /// Frames since then.
    pub age_frames: u64,
}

impl From<AssetRecordRef<'_>> for PonkoAssetRecord {
    fn from(record: AssetRecordRef<'_>) -> Self {
        Self {
            id: record.id,
            owner: match record.owner {
                AssetOwnerRef::File(_) => PonkoAssetOwner::PONKO_ASSET_OWNER_FILE,
                AssetOwnerRef::Engine => PonkoAssetOwner::PONKO_ASSET_OWNER_ENGINE,
                _ => PonkoAssetOwner::PONKO_ASSET_OWNER_CODE,
            } as u32,
            bytes: record.bytes,
            made_frame: record.made_frame,
            age_frames: record.age_frames,
        }
    }
}

fn asset_order(record: &PonkoAssetRecord) -> (u64, u32) {
    (record.made_frame, record.id)
}

// Replace the newest record in a full max heap, then restore its order.
fn keep_older(heap: &mut [PonkoAssetRecord], record: PonkoAssetRecord) {
    if asset_order(&record) >= asset_order(&heap[0]) {
        return;
    }
    let mut at = 0;
    while at < heap.len() / 2 {
        let left = 2 * at + 1;
        let right = left + 1;
        let next = if right < heap.len() && asset_order(&heap[right]) > asset_order(&heap[left]) {
            right
        } else {
            left
        };
        if asset_order(&record) >= asset_order(&heap[next]) {
            break;
        }
        heap[at] = heap[next];
        at = next;
    }
    heap[at] = record;
}

fn asset_class(class: u32) -> Result<AssetClass, crate::Failure> {
    Ok(match class {
        0 => AssetClass::Texture,
        1 => AssetClass::RenderTarget,
        2 => AssetClass::TextureArray,
        3 => AssetClass::Material,
        4 => AssetClass::Shader,
        5 => AssetClass::Mesh,
        6 => AssetClass::GpuBuffer,
        7 => AssetClass::Effect,
        8 => AssetClass::Compute,
        9 => AssetClass::Sound,
        _ => return Err(invalid("unknown asset class")),
    })
}

/// The records of `class`, from the service that holds it.
fn records_of(c: &ponko::prelude::Ctx, class: AssetClass) -> Vec<ponko::inventory::AssetRecord> {
    match class {
        AssetClass::Sound => c.audio.asset_handles(),
        _ => c.gfx.asset_handles(class),
    }
}

/// Live handles of class `asset_class_id` (a `PonkoAssetClass`) and their estimated bytes
/// (Rust `Ctx::asset_inventory`): compare two to see which class grows.
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// let mut count = ponko_ffi::PonkoAssetCount::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_count(std::ptr::null_mut(), 0, &mut count) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_count(
    ctx: *mut PonkoCtx,
    asset_class_id: u32,
    out_count: *mut PonkoAssetCount,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let count = c.asset_inventory().get(asset_class(asset_class_id)?);
            out(
                out_count,
                PonkoAssetCount {
                    live: count.live,
                    bytes: count.bytes,
                },
            )
        })
    })
}

/// Every live handle of class `asset_class_id` (a `PonkoAssetClass`), oldest first, with
/// its bytes, age and owner (Rust `Graphics::asset_handles` and
/// `Audio::asset_handles`): up to `capacity`
/// copied into `records` without allocating; `*out_len` is how many there are.
/// Call with `capacity` 0 to size the array.
/// # Safety
/// `ctx` null or live; `records` null (with `capacity` 0) or `capacity`
/// writable records; `out_len` writable and disjoint from `records`.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_handles(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_handles(
    ctx: *mut PonkoCtx,
    asset_class_id: u32,
    records: *mut PonkoAssetRecord,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if records.is_null() && capacity > 0 {
                return Err(invalid("records is null"));
            }
            if out_len.is_null() {
                return Err(invalid("out_len is null"));
            }
            let mut len = 0;
            let visit = |record: AssetRecordRef<'_>| {
                if len < capacity {
                    records.add(len).write(record.into());
                } else if capacity > 0 {
                    // The prefix is initialized. Its descending sort is a max
                    // heap, so later records can replace the latest kept one.
                    let heap = std::slice::from_raw_parts_mut(records, capacity);
                    if len == capacity {
                        heap.sort_unstable_by_key(|r| std::cmp::Reverse(asset_order(r)));
                    }
                    keep_older(heap, record.into());
                }
                len += 1;
            };
            match asset_class(asset_class_id)? {
                AssetClass::Sound => c.audio.visit_asset_handles(visit),
                class => c.gfx.visit_asset_handles(class, visit),
            }
            let written = len.min(capacity);
            if written > 0 {
                std::slice::from_raw_parts_mut(records, written).sort_unstable_by_key(asset_order);
            }
            out(out_len, len)
        })
    })
}

/// The asset name that loaded handle `id` of class `asset_class_id` (a `PonkoAssetClass`),
/// copied NUL-terminated into `buffer` (`capacity` bytes, cut short if
/// needed); `*out_len` is the full length without the NUL, 0 for a handle
/// made in code, by the engine, or not live.
/// # Safety
/// `ctx` null or live; `buffer` null (with `capacity` 0) or `capacity`
/// writable bytes; output writable.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_owner(std::ptr::null_mut(), 0, 0, std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_owner(
    ctx: *mut PonkoCtx,
    asset_class_id: u32,
    id: u32,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if buffer.is_null() && capacity > 0 {
                return Err(invalid("buffer is null"));
            }
            let name = records_of(c, asset_class(asset_class_id)?)
                .into_iter()
                .find(|r| r.id == id)
                .and_then(|r| match r.owner {
                    AssetOwner::File(name) => Some(name),
                    _ => None,
                })
                .unwrap_or_default();
            if capacity > 0 {
                let n = name.len().min(capacity - 1);
                std::ptr::copy_nonoverlapping(name.as_ptr(), buffer.cast::<u8>(), n);
                *buffer.add(n) = 0;
            }
            out(out_len, name.len())
        })
    })
}

/// Frees everything loading glTF file `path` made: its mesh, materials and
/// textures, except images another loaded file shares (Rust
/// `Graphics::unload_mesh`). `*out_unloaded` is false if it wasn't loaded.
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string; output writable.
/// ```
/// let mut done = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_unload_mesh(std::ptr::null_mut(), std::ptr::null(), &mut done) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_unload_mesh(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    out_unloaded: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            out(out_unloaded, c.gfx.unload_mesh(path))
        })
    })
}

/// The cost of the last drawn frame for one mesh (Rust `MeshCost`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoMeshCost {
    /// The mesh that drew.
    pub mesh: crate::three::PonkoMesh,
    /// Models drawn by the camera.
    pub models: usize,
    /// Triangles drawn by the camera.
    pub triangles: usize,
    /// Models drawn into the shadow maps, summed across maps.
    pub shadow_models: usize,
    /// Triangles drawn into the shadow maps, summed across maps.
    pub shadow_triangles: usize,
    /// Byte offset of the name in the caller's names buffer.
    pub name_offset: usize,
    /// UTF-8 bytes in the name, excluding the NUL. Zero for unnamed meshes.
    pub name_len: usize,
}

/// One streamed or resident asset (Rust `StreamedAsset`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoStreamedAsset {
    /// A `PonkoResidencyKind`.
    pub kind: u32,
    /// A `PonkoAssetState`.
    pub state: u32,
    /// Whether the asset has a handle.
    pub has_handle: bool,
    /// Its mesh or texture id, zero when `has_handle` is false.
    pub id: u32,
    /// GPU width, or zero for a mesh.
    pub width: u32,
    /// GPU height, or zero for a mesh.
    pub height: u32,
    /// Byte offset of the name in the caller's names buffer.
    pub name_offset: usize,
    /// UTF-8 bytes in the name, excluding the NUL.
    pub name_len: usize,
}

/// Validated outputs for a listing with a packed names buffer.
struct Listing<T> {
    records: *mut T,
    capacity: usize,
    names: *mut c_char,
    name_capacity: usize,
    len: usize,
    name_len: usize,
}

impl<T> Listing<T> {
    fn new(
        records: *mut T,
        capacity: usize,
        names: *mut c_char,
        name_capacity: usize,
        out_len: *mut usize,
        out_names_len: *mut usize,
    ) -> Result<Self, crate::Failure> {
        if records.is_null() && capacity != 0 {
            return Err(invalid("records is null"));
        }
        if names.is_null() && name_capacity != 0 {
            return Err(invalid("names is null"));
        }
        if out_len.is_null() || out_names_len.is_null() {
            return Err(invalid("listing length outputs must both be writable"));
        }
        Ok(Self {
            records,
            capacity,
            names,
            name_capacity,
            len: 0,
            name_len: 0,
        })
    }

    /// Counts the record, writing it and any complete name that fits.
    /// The caller supplies disjoint writable arrays of the stated sizes.
    unsafe fn push(&mut self, name: Option<&str>, record: impl FnOnce(usize, usize) -> T) {
        if self.len < self.capacity {
            let offset = self.name_len;
            let mut len = 0;
            if let Some(name) = name {
                len = name.len();
                self.name_len += len + 1;
                if self.name_len <= self.name_capacity {
                    // SAFETY: this complete NUL-terminated name fits, and the buffers are disjoint.
                    unsafe {
                        let start = self.names.add(offset);
                        std::ptr::copy_nonoverlapping(name.as_ptr(), start.cast(), len);
                        start.add(len).write(0);
                    }
                }
            }
            // SAFETY: this index is in the caller's writable array.
            unsafe { self.records.add(self.len).write(record(offset, len)) };
        }
        self.len += 1;
    }
}

thread_local! {
    // Keep the aggregation buffers used by the Rust collector between native calls.
    static MESH_COSTS: std::cell::RefCell<Vec<ponko::render::MeshCost>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Copies up to `capacity` mesh costs, largest total triangles first,
/// mesh id breaking ties. Empty headless and before a draw. Reuses
/// per-thread aggregation storage, which can grow as meshes or names are added.
///
/// `*out_len` is the total record count. `*out_names_len` is the bytes
/// needed for the NUL-terminated names of the copied prefix. A name is
/// written only if `name_offset + name_len + 1 <= name_capacity`; a mesh
/// with `name_len == 0` is unnamed. To size both buffers, first query the
/// count with zero capacities, then pass the record array and zero name
/// capacity to size the name buffer.
///
/// # Safety
/// `ctx` null or live. The four outputs must be disjoint and writable for
/// their stated lengths. `records` and `names` may be null at zero capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mesh_costs(
    ctx: *mut PonkoCtx,
    records: *mut PonkoMeshCost,
    capacity: usize,
    names: *mut c_char,
    name_capacity: usize,
    out_len: *mut usize,
    out_names_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let mut listing = Listing::new(records, capacity, names, name_capacity, out_len, out_names_len)?;
            MESH_COSTS.with_borrow_mut(|costs| {
                c.gfx.mesh_costs_into(costs);
                for cost in costs {
                    listing.push(cost.name.as_deref(), |name_offset, name_len| PonkoMeshCost {
                        mesh: crate::types::to_c(cost.mesh),
                        models: cost.models,
                        triangles: cost.triangles,
                        shadow_models: cost.shadow_models,
                        shadow_triangles: cost.shadow_triangles,
                        name_offset,
                        name_len,
                    });
                }
            });
            out(out_len, listing.len)?;
            out(out_names_len, listing.name_len)
        })
    })
}

/// Copies up to `capacity` streamed and resident assets in kind/name order,
/// without allocating. Count and names buffers work as in `ponko_ctx_mesh_costs`.
///
/// # Safety
/// `ctx` null or live. The four outputs must be disjoint and writable for
/// their stated lengths. `records` and `names` may be null at zero capacity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_streamed_assets(
    ctx: *mut PonkoCtx,
    records: *mut PonkoStreamedAsset,
    capacity: usize,
    names: *mut c_char,
    name_capacity: usize,
    out_len: *mut usize,
    out_names_len: *mut usize,
) -> PonkoStatus {
    use crate::residency::{PonkoResidencyKind, state_to_c};
    use ponko::streaming::ResidencyKind;
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let mut listing = Listing::new(records, capacity, names, name_capacity, out_len, out_names_len)?;
            c.gfx.visit_streamed_assets(|kind, name, state, id, [width, height]| {
                listing.push(Some(name), |name_offset, name_len| PonkoStreamedAsset {
                    kind: match kind {
                        ResidencyKind::Mesh => PonkoResidencyKind::PONKO_RESIDENCY_MESH as u32,
                        ResidencyKind::Texture => PonkoResidencyKind::PONKO_RESIDENCY_TEXTURE as u32,
                        _ => unreachable!("new residency kind needs a C representation"),
                    },
                    state: state_to_c(state),
                    has_handle: id.is_some(),
                    id: id.unwrap_or_default(),
                    width,
                    height,
                    name_offset,
                    name_len,
                });
            });
            out(out_len, listing.len)?;
            out(out_names_len, listing.name_len)
        })
    })
}
