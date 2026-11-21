//! Baked lighting from C (Rust `ponko::render::bake_lighting` and its
//! types): lightmap coordinates for a mesh, a scene of models and lamps to
//! bake, the bake itself, its result kept as bytes and set for drawing,
//! and the `Lightmapped` component that draws a model with its lightmap.
use std::ffi::c_void;
use std::mem::{offset_of, size_of};

use ponko::prelude::{Mat4, Vec3};
use ponko::render::{BakeModel, BakedLighting, LightBakeScene, LightBakeSettings, Lightmapped};

use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::three::{PonkoMeshData, PonkoPointLight3D, PonkoSpotLight3D, PonkoTransform3D, PonkoVec3, transform_from_c};
use crate::types::{PonkoColor, SameLayout, same_layout, to_c};
use crate::{Failure, Kind, PonkoCtx, PonkoStatus, PonkoWorld, boundary, builtin, engine_error, invalid};

/// Gives `data` lightmap coordinates (its second UV set; Rust
/// `MeshData::generate_lightmap_uvs`): every triangle unfolded onto a
/// square without overlaps, `texels_per_unit` lightmap texels per world
/// unit (a value that isn't positive is 1). Writes the square's side in
/// texels to `out_side` (may be null). Bake and draw the model with the
/// same coordinates: make its GPU mesh from this data afterwards.
///
/// Ownership: `data` stays the caller's; `out_side` is written during the
/// call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_mesh_data_generate_lightmap_uvs(std::ptr::null_mut(), 4.0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `data` null or live; `out_side` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_generate_lightmap_uvs(
    data: *mut PonkoMeshData,
    texels_per_unit: f32,
    out_side: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: the caller's contract.
        let mesh = unsafe { data.as_mut() }.ok_or_else(|| invalid("mesh data is null"))?;
        let side = mesh.0.generate_lightmap_uvs(texels_per_unit);
        if !out_side.is_null() {
            // SAFETY: the caller's contract.
            unsafe { out(out_side, side)? };
        }
        Ok(())
    })
}

/// The version of `PonkoLightBakeSettings` this header describes.
pub const PONKO_LIGHT_BAKE_SETTINGS_VERSION: u32 = 1;

/// How `ponko_bake_lighting` bakes (Rust `LightBakeSettings`). Fill it with
/// `ponko_light_bake_settings_defaults`, then change what you need.
///
/// Version 1: every field below.
/// ```
/// let s: ponko_ffi::PonkoLightBakeSettings = unsafe { std::mem::zeroed() };
/// assert_eq!(s.samples, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoLightBakeSettings {
    /// `sizeof(PonkoLightBakeSettings)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_LIGHT_BAKE_SETTINGS_VERSION` as the caller compiled it.
    pub version: u32,
    /// Lightmap texels per world unit along a surface (default 4).
    pub texels_per_unit: f32,
    /// Rays per texel and per probe: more is less noisy and slower
    /// (default 128).
    pub samples: u32,
    /// Times light bounces off surfaces on its way to a texel (default 3).
    pub bounces: u32,
    /// The pages' largest side in texels, 64 to 2048 (default 1024).
    pub page_size: u32,
    /// Also bake the direction light comes from, so normal maps shape baked
    /// light (twice the memory; default true).
    pub directional: bool,
    /// Smooth each lightmap's noise, keeping edges (default true).
    pub denoise: bool,
    /// The probe volume's spacing in world units; 0: no probes (default 2).
    pub probe_spacing: f32,
    /// Where the random numbers start: the same seed bakes the same result
    /// on any machine (default 0).
    pub seed: u64,
}

impl Versioned for PonkoLightBakeSettings {
    const NAME: &'static str = "PonkoLightBakeSettings";
    const VERSION: u32 = PONKO_LIGHT_BAKE_SETTINGS_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, seed) + size_of::<u64>()),
            _ => None,
        }
    }
}

fn settings_to_c(s: &LightBakeSettings) -> PonkoLightBakeSettings {
    PonkoLightBakeSettings {
        struct_size: size_of::<PonkoLightBakeSettings>() as u32,
        version: PONKO_LIGHT_BAKE_SETTINGS_VERSION,
        texels_per_unit: s.texels_per_unit,
        samples: s.samples,
        bounces: s.bounces,
        page_size: s.page_size,
        directional: s.directional,
        denoise: s.denoise,
        probe_spacing: s.probe_spacing,
        seed: s.seed,
    }
}

fn settings_from_c(s: &PonkoLightBakeSettings) -> LightBakeSettings {
    LightBakeSettings::default()
        .with_texels_per_unit(s.texels_per_unit)
        .with_samples(s.samples)
        .with_bounces(s.bounces)
        .with_page_size(s.page_size)
        .with_directional(s.directional)
        .with_denoise(s.denoise)
        .with_probe_spacing(s.probe_spacing)
        .with_seed(s.seed)
}

/// Fills the fields of `settings`' version with the defaults (Rust
/// `LightBakeSettings::default`).
///
/// Ownership: `settings` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_light_bake_settings_defaults(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `settings` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_light_bake_settings_defaults(settings: *mut PonkoLightBakeSettings) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(settings, &settings_to_c(&LightBakeSettings::default())) })
}

/// The version of `PonkoBakeModel` this header describes.
pub const PONKO_BAKE_MODEL_VERSION: u32 = 1;

/// One model to bake (Rust `BakeModel`): its triangles placed in the world,
/// and its surface. Fill it with `ponko_bake_model_defaults`, then set
/// `mesh` and `transform`.
///
/// Version 1: every field below.
/// ```
/// let m: ponko_ffi::PonkoBakeModel = unsafe { std::mem::zeroed() };
/// assert!(m.mesh.is_null());
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoBakeModel {
    /// `sizeof(PonkoBakeModel)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_BAKE_MODEL_VERSION` as the caller compiled it.
    pub version: u32,
    /// Its triangles. A lightmapped model's mesh needs lightmap coordinates
    /// (from its file, or `ponko_mesh_data_generate_lightmap_uvs`), the
    /// same ones its drawn mesh has.
    pub mesh: *const PonkoMeshData,
    /// Where it stands (default: the origin, unrotated, scale 1).
    pub transform: PonkoTransform3D,
    /// Its surface's color, sRGB (default white).
    pub albedo: PonkoColor,
    /// The light it gives off, sRGB (default black).
    pub emissive: PonkoColor,
    /// Both sides of its triangles take and bounce light (default false).
    pub double_sided: bool,
    /// Gets a lightmap (default true); false: it only shades and bounces
    /// light for the others.
    pub lightmapped: bool,
}

impl Versioned for PonkoBakeModel {
    const NAME: &'static str = "PonkoBakeModel";
    const VERSION: u32 = PONKO_BAKE_MODEL_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, lightmapped) + size_of::<bool>()),
            _ => None,
        }
    }
}

fn model_defaults() -> PonkoBakeModel {
    PonkoBakeModel {
        struct_size: size_of::<PonkoBakeModel>() as u32,
        version: PONKO_BAKE_MODEL_VERSION,
        mesh: std::ptr::null(),
        transform: crate::three::transform_to_c(ponko::d3::Transform::default()),
        albedo: to_c(ponko::prelude::Color::WHITE),
        emissive: to_c(ponko::prelude::Color::rgb(0.0, 0.0, 0.0)),
        double_sided: false,
        lightmapped: true,
    }
}

/// Fills the fields of `model`'s version with the defaults: no mesh, at
/// the origin, white, not glowing, one-sided, lightmapped.
///
/// Ownership: `model` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_bake_model_defaults(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `model` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_bake_model_defaults(model: *mut PonkoBakeModel) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(model, &model_defaults()) })
}

/// The models and lamps to bake (Rust `LightBakeScene`): an owned handle
/// from `ponko_ctx_light_bake_scene_new`, freed with
/// `ponko_light_bake_scene_free`. It holds copies, not the ctx: bake it on
/// any thread.
pub struct PonkoLightBakeScene(LightBakeScene);

/// The result of `ponko_bake_lighting` (Rust `BakedLighting`): lightmap
/// pages, where each model's lightmap is, the probe volume. An owned
/// handle, freed with `ponko_baked_lighting_free`; draw with it through
/// `ponko_ctx_set_baked_lighting`, keep it with
/// `ponko_baked_lighting_to_bytes`.
pub struct PonkoBakedLighting(BakedLighting);

fn scene_mut<'a>(scene: *mut PonkoLightBakeScene) -> Result<&'a mut LightBakeScene, Failure> {
    // SAFETY: the caller's contract: null or live.
    unsafe { scene.as_mut() }
        .map(|s| &mut s.0)
        .ok_or_else(|| invalid("bake scene is null"))
}

fn baked_ref<'a>(baked: *const PonkoBakedLighting) -> Result<&'a BakedLighting, Failure> {
    // SAFETY: the caller's contract: null or live.
    unsafe { baked.as_ref() }
        .map(|b| &b.0)
        .ok_or_else(|| invalid("baked lighting is null"))
}

/// A new bake scene lit as the game is now (Rust `LightBakeScene::new`
/// with the ctx's lighting and sky): the sun, and the sky's ambient light.
/// Add models with `ponko_light_bake_scene_add` and lamps with
/// `ponko_ctx_light_bake_scene_add_lights` or the `_add_*_light` calls.
///
/// Ownership: the caller owns the scene written to `out_scene` and frees
/// it with `ponko_light_bake_scene_free`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_light_bake_scene_new(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_scene` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_light_bake_scene_new(
    ctx: *mut PonkoCtx,
    out_scene: *mut *mut PonkoLightBakeScene,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_scene.is_null() {
                return Err(invalid("output is null"));
            }
            let scene = LightBakeScene::new(&c.scene3d.lighting, &c.scene3d.sky);
            out(out_scene, Box::into_raw(Box::new(PonkoLightBakeScene(scene))))
        })
    })
}

/// Frees a bake scene. Null is ignored.
///
/// Ownership: takes `scene` back; don't use it afterwards.
/// ```
/// unsafe { ponko_ffi::ponko_light_bake_scene_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `scene` null or live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_light_bake_scene_free(scene: *mut PonkoLightBakeScene) {
    if !scene.is_null() {
        // SAFETY: the caller's contract: an owned scene, once.
        drop(unsafe { Box::from_raw(scene) });
    }
}

/// Adds a model to bake (Rust `LightBakeScene::add`) and writes its index
/// to `out_index` (may be null): what `ponko_baked_lighting_lightmap` and
/// `PonkoLightmapped` take. A model whose mesh is lightmapped but has no
/// lightmap coordinates is `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `model` and its mesh are read during the call only (the
/// scene keeps a copy of the placed triangles).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_light_bake_scene_add(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `scene` null or live; `model` null or a readable descriptor with its
/// header set, whose `mesh` is null or live; `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_light_bake_scene_add(
    scene: *mut PonkoLightBakeScene,
    model: *const PonkoBakeModel,
    out_index: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let scene = scene_mut(scene)?;
        // SAFETY: forwarded caller contract.
        let m = unsafe { read_desc(model, model_defaults()) }?;
        // SAFETY: the caller's contract: `mesh` null or live.
        let mesh = &unsafe { m.mesh.as_ref() }
            .ok_or_else(|| invalid("PonkoBakeModel: mesh is null"))?
            .0;
        if m.lightmapped && mesh.uvs1.len() != mesh.positions.len() {
            return Err(invalid(
                "PonkoBakeModel: a lightmapped mesh needs lightmap coordinates (ponko_mesh_data_generate_lightmap_uvs)",
            ));
        }
        let matrix: Mat4 = transform_from_c(m.transform).matrix();
        let model = BakeModel::new(mesh, matrix)
            .with_albedo(to_c(m.albedo))
            .with_emissive(to_c(m.emissive))
            .with_double_sided(m.double_sided)
            .with_lightmapped(m.lightmapped);
        let index = scene.add(model);
        if !out_index.is_null() {
            // SAFETY: the caller's contract.
            unsafe { out(out_index, index)? };
        }
        Ok(())
    })
}

/// Adds a point light at `position` (Rust
/// `LightBakeScene::add_point_light`), as the renderer lights with it.
///
/// Ownership: `light` is read during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_light_bake_scene_add_point_light(std::ptr::null_mut(), Default::default(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `scene` null or live; `light` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_light_bake_scene_add_point_light(
    scene: *mut PonkoLightBakeScene,
    position: PonkoVec3,
    light: *const PonkoPointLight3D,
) -> PonkoStatus {
    boundary(|| {
        let scene = scene_mut(scene)?;
        // SAFETY: the caller's contract.
        let light = *unsafe { light.as_ref() }.ok_or_else(|| invalid("light is null"))?;
        scene.add_point_light(to_c(position), &to_c(light));
        Ok(())
    })
}

/// Adds a spot light at `position` shining along `forward` (Rust
/// `LightBakeScene::add_spot_light`).
///
/// Ownership: `light` is read during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_light_bake_scene_add_spot_light(std::ptr::null_mut(), Default::default(), Default::default(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `scene` null or live; `light` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_light_bake_scene_add_spot_light(
    scene: *mut PonkoLightBakeScene,
    position: PonkoVec3,
    forward: PonkoVec3,
    light: *const PonkoSpotLight3D,
) -> PonkoStatus {
    boundary(|| {
        let scene = scene_mut(scene)?;
        // SAFETY: the caller's contract.
        let light = *unsafe { light.as_ref() }.ok_or_else(|| invalid("light is null"))?;
        let forward: Vec3 = to_c(forward);
        scene.add_spot_light(to_c(position), forward, &to_c(light));
        Ok(())
    })
}

/// Adds every point and spot light of the game's world where it is now
/// (Rust `LightBakeScene::add_lights_from`).
///
/// Ownership: nothing is kept but copies.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_light_bake_scene_add_lights(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `scene` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_light_bake_scene_add_lights(
    ctx: *mut PonkoCtx,
    scene: *mut PonkoLightBakeScene,
) -> PonkoStatus {
    boundary(|| {
        let scene = scene_mut(scene)?;
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                scene.add_lights_from(&mut c.world);
                Ok(())
            })
        }
    })
}

/// Bakes the light bouncing around `scene` (Rust `bake_lighting`): on the
/// CPU, on every core, so it takes seconds to minutes; it reads only the
/// scene, so a game can run it on a thread of its own. `settings` null:
/// the defaults. The same scene, settings and seed bake the same result.
/// Settings out of range are `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `scene` and `settings` are read during the call only. The
/// caller owns the result written to `out_baked` and frees it with
/// `ponko_baked_lighting_free`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_bake_lighting(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `scene` null or live (not changed during the call); `settings` null or a
/// readable descriptor with its header set; `out_baked` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_bake_lighting(
    scene: *const PonkoLightBakeScene,
    settings: *const PonkoLightBakeSettings,
    out_baked: *mut *mut PonkoBakedLighting,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: the caller's contract.
        let scene = &unsafe { scene.as_ref() }
            .ok_or_else(|| invalid("bake scene is null"))?
            .0;
        if out_baked.is_null() {
            return Err(invalid("output is null"));
        }
        let settings = if settings.is_null() {
            LightBakeSettings::default()
        } else {
            // SAFETY: forwarded caller contract.
            settings_from_c(&unsafe { read_desc(settings, settings_to_c(&LightBakeSettings::default())) }?)
        };
        let baked = ponko::render::bake_lighting(scene, &settings).map_err(engine_error)?;
        // SAFETY: the caller's contract.
        unsafe { out(out_baked, Box::into_raw(Box::new(PonkoBakedLighting(baked)))) }
    })
}

/// Frees baked lighting. Null is ignored. Lighting set for drawing stays
/// set: the renderer keeps its own copy.
///
/// Ownership: takes `baked` back; don't use it afterwards.
/// ```
/// unsafe { ponko_ffi::ponko_baked_lighting_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `baked` null or live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_free(baked: *mut PonkoBakedLighting) {
    if !baked.is_null() {
        // SAFETY: the caller's contract: an owned result, once.
        drop(unsafe { Box::from_raw(baked) });
    }
}

/// Writes baked lighting as bytes for a file (Rust
/// `BakedLighting::to_bytes`): `out_len` gets their length, and if
/// `capacity` holds them they are written to `buffer`. Call with capacity
/// 0 first to learn the length.
///
/// Ownership: `buffer` and `out_len` are the caller's, written during the
/// call only.
/// ```
/// let mut len = 0usize;
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_to_bytes(std::ptr::null(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `baked` null or live; `buffer` writable for `capacity` bytes (or null
/// with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_to_bytes(
    baked: *const PonkoBakedLighting,
    buffer: *mut u8,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let baked = baked_ref(baked)?;
        if out_len.is_null() || (buffer.is_null() && capacity != 0) {
            return Err(invalid("output is null"));
        }
        let bytes = baked.to_bytes();
        // SAFETY: the caller's contract: checked above.
        unsafe {
            out_len.write(bytes.len());
            if bytes.len() <= capacity {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
            }
        }
        Ok(())
    })
}

/// Reads baked lighting back from `ponko_baked_lighting_to_bytes`' bytes
/// (Rust `BakedLighting::from_bytes`). Bytes that aren't baked lighting, or
/// of a newer version, are `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `data` is read during the call only. The caller owns the
/// result written to `out_baked` and frees it with
/// `ponko_baked_lighting_free`.
/// ```
/// let mut b = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_from_bytes(c"no".as_ptr().cast(), 2, &mut b) } as u32, 1);
/// ```
/// # Safety
/// `data` readable for `size` bytes; `out_baked` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_from_bytes(
    data: *const c_void,
    size: usize,
    out_baked: *mut *mut PonkoBakedLighting,
) -> PonkoStatus {
    boundary(|| {
        if out_baked.is_null() || (data.is_null() && size > 0) {
            return Err(invalid("data or output is null"));
        }
        let bytes = if size == 0 {
            &[][..]
        } else {
            // SAFETY: the caller's contract.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }
        };
        let baked = BakedLighting::from_bytes(bytes).map_err(engine_error)?;
        // SAFETY: the caller's contract.
        unsafe { out(out_baked, Box::into_raw(Box::new(PonkoBakedLighting(baked)))) }
    })
}

/// Loads baked lighting saved with `ponko_baked_lighting_to_bytes` from an
/// added asset source or the `assets/` folder (Rust
/// `Graphics::try_load_baked_lighting`). A missing file is
/// `PONKO_NOT_FOUND`; one that isn't baked lighting
/// `PONKO_INVALID_ARGUMENT` (`ponko_last_error` names it).
///
/// Ownership: `path` is read during the call only. The caller owns the
/// result written to `out_baked` and frees it with
/// `ponko_baked_lighting_free`.
/// ```
/// let mut b = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_baked_lighting(std::ptr::null_mut(), c"room.lit".as_ptr(), &mut b) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a C string; `out_baked` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_baked_lighting(
    ctx: *mut PonkoCtx,
    path: *const std::ffi::c_char,
    out_baked: *mut *mut PonkoBakedLighting,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = crate::ctx::text(path)?;
            if out_baked.is_null() {
                return Err(invalid("output is null"));
            }
            let baked = c.gfx.try_load_baked_lighting(path).map_err(engine_error)?;
            out(out_baked, Box::into_raw(Box::new(PonkoBakedLighting(baked))))
        })
    })
}

/// Draws the world with `baked` (Rust `Graphics::set_baked_lighting`):
/// models with a `PonkoLightmapped` take their lightmap, the others the
/// probe volume's light. Null turns baked lighting off. More pages than
/// this GPU's textures take is `PONKO_INVALID_ARGUMENT` (bake with a
/// smaller `page_size`).
///
/// Ownership: the renderer copies what it needs; `baked` stays the
/// caller's.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_baked_lighting(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `baked` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_baked_lighting(
    ctx: *mut PonkoCtx,
    baked: *const PonkoBakedLighting,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let baked = baked.as_ref().map(|b| &b.0);
            c.gfx.set_baked_lighting(baked).map_err(engine_error)
        })
    })
}

/// The version of `PonkoBakedLightingInfo` this header describes.
pub const PONKO_BAKED_LIGHTING_INFO_VERSION: u32 = 1;

/// What a bake made and how it went (`ponko_baked_lighting_info`; Rust
/// `BakedLighting::page_size`, `pages`, `directional`, `gpu_bytes` and
/// `stats`).
///
/// Version 1: every field below.
/// ```
/// let i: ponko_ffi::PonkoBakedLightingInfo = unsafe { std::mem::zeroed() };
/// assert_eq!(i.pages, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoBakedLightingInfo {
    /// `sizeof(PonkoBakedLightingInfo)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_BAKED_LIGHTING_INFO_VERSION` as the caller compiled it.
    pub version: u32,
    /// The pages' width and height in texels.
    pub page_size: u32,
    /// How many lightmap pages.
    pub pages: u32,
    /// Whether it has the light's direction per texel.
    pub directional: bool,
    /// The GPU memory it takes when set, in bytes.
    pub gpu_bytes: u64,
    /// Wall-clock seconds the bake took.
    pub seconds: f64,
    /// Lightmap texels on surfaces.
    pub texels: u64,
    /// Of those, the ones inside other geometry, filled from neighbours.
    pub inside_texels: u64,
    /// Probes in the volume.
    pub probes: u64,
    /// Rays traced.
    pub rays: u64,
}

impl Versioned for PonkoBakedLightingInfo {
    const NAME: &'static str = "PonkoBakedLightingInfo";
    const VERSION: u32 = PONKO_BAKED_LIGHTING_INFO_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, rays) + size_of::<u64>()),
            _ => None,
        }
    }
}

/// Writes what `baked` holds and how its bake went into the fields of
/// `out_info`'s version. How the bake went (seconds, texels, probes, rays)
/// isn't kept in its bytes: zero for lighting read back with
/// `ponko_baked_lighting_from_bytes`.
///
/// Ownership: `out_info` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_info(std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `baked` null or live; `out_info` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_info(
    baked: *const PonkoBakedLighting,
    out_info: *mut PonkoBakedLightingInfo,
) -> PonkoStatus {
    boundary(|| {
        let b = baked_ref(baked)?;
        let stats = b.stats();
        let info = PonkoBakedLightingInfo {
            struct_size: size_of::<PonkoBakedLightingInfo>() as u32,
            version: PONKO_BAKED_LIGHTING_INFO_VERSION,
            page_size: b.page_size(),
            pages: b.pages(),
            directional: b.directional(),
            gpu_bytes: b.gpu_bytes(),
            seconds: stats.seconds,
            texels: stats.texels,
            inside_texels: stats.inside_texels,
            probes: stats.probes,
            rays: stats.rays,
        };
        // SAFETY: the caller's contract.
        unsafe { write_desc(out_info, &info) }
    })
}

/// Where a model's lightmap is: a rectangle of a page, in texels from its
/// top left corner (Rust `LightmapRect`).
/// ```
/// assert_eq!(ponko_ffi::PonkoLightmapRect::default().page, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoLightmapRect {
    /// The page.
    pub page: u32,
    /// Its left column.
    pub x: u32,
    /// Its top row.
    pub y: u32,
    /// Its width in texels.
    pub width: u32,
    /// Its height in texels.
    pub height: u32,
}

/// Where model `index` of the bake (its index from
/// `ponko_light_bake_scene_add`) has its lightmap (Rust
/// `BakedLighting::rect`), and the `PonkoLightmapped` that draws it with it
/// (Rust `BakedLighting::lightmap`). `PONKO_NOT_FOUND` for a model that
/// isn't lightmapped or isn't in the bake. Either output may be null.
///
/// Ownership: the outputs are the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_lightmap(std::ptr::null(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `baked` null or live; the outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_lightmap(
    baked: *const PonkoBakedLighting,
    index: usize,
    out_rect: *mut PonkoLightmapRect,
    out_lightmapped: *mut PonkoLightmapped,
) -> PonkoStatus {
    boundary(|| {
        let b = baked_ref(baked)?;
        let (Some(rect), Some(component)) = (b.rect(index), b.lightmap(index)) else {
            return Err(crate::replication::not_found("that model has no lightmap"));
        };
        // SAFETY: the caller's contract.
        unsafe {
            if !out_rect.is_null() {
                out(
                    out_rect,
                    PonkoLightmapRect {
                        page: rect.page,
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    },
                )?;
            }
            if !out_lightmapped.is_null() {
                out(out_lightmapped, to_c(component))?;
            }
        }
        Ok(())
    })
}

/// The baked light at texel (`x`, `y`) of `page` (Rust
/// `BakedLighting::texel`): irradiance over pi, what a white surface there
/// shows, and whether the texel was baked. `PONKO_NOT_FOUND` outside the
/// pages.
///
/// Ownership: the outputs are the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_texel(std::ptr::null(), 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `baked` null or live; `out_light` writable; `out_baked` null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_texel(
    baked: *const PonkoBakedLighting,
    page: u32,
    x: u32,
    y: u32,
    out_light: *mut PonkoVec3,
    out_baked: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let b = baked_ref(baked)?;
        if out_light.is_null() {
            return Err(invalid("output is null"));
        }
        if x >= b.page_size() || y >= b.page_size() {
            return Err(crate::replication::not_found("no such texel"));
        }
        let (light, was_baked) = b
            .texel(page, x, y)
            .ok_or_else(|| crate::replication::not_found("no such texel"))?;
        // SAFETY: the caller's contract.
        unsafe {
            out(out_light, to_c(light))?;
            if !out_baked.is_null() {
                out(out_baked, was_baked)?;
            }
        }
        Ok(())
    })
}

/// The light the probe volume gives a surface at `position` facing
/// `normal` (Rust `BakedLighting::probe_light`): irradiance over pi.
/// `PONKO_NOT_FOUND` outside the volume, or without one.
///
/// Ownership: `out_light` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_baked_lighting_probe_light(std::ptr::null(), Default::default(), Default::default(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `baked` null or live; `out_light` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_baked_lighting_probe_light(
    baked: *const PonkoBakedLighting,
    position: PonkoVec3,
    normal: PonkoVec3,
    out_light: *mut PonkoVec3,
) -> PonkoStatus {
    boundary(|| {
        let b = baked_ref(baked)?;
        if out_light.is_null() {
            return Err(invalid("output is null"));
        }
        let light = b
            .probe_light(to_c(position), to_c(normal))
            .ok_or_else(|| crate::replication::not_found("outside the probe volume"))?;
        // SAFETY: the caller's contract.
        unsafe { out(out_light, to_c(light)) }
    })
}

/// Draws a model with its lightmap from the baked lighting set with
/// `ponko_ctx_set_baked_lighting` (Rust `Lightmapped`): the model's index in
/// the bake, from `ponko_baked_lighting_lightmap`. Its mesh must have the
/// lightmap coordinates it was baked with.
/// ```
/// assert_eq!(std::mem::size_of::<ponko_ffi::PonkoLightmapped>(), 4);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoLightmapped {
    /// The model's index in the bake.
    pub index: u32,
}

same_layout!(PonkoLightmapped, Lightmapped);

/// Registers the built-in `Lightmapped` component (`PonkoLightmapped`) for
/// insertion and queries.
///
/// Ownership: `out_id` is the caller's, written during the call only.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut id = 0; assert_eq!(ponko_lightmapped_component(w, &mut id), PonkoStatus::PONKO_OK); ponko_world_free(w); }
/// ```
/// # Safety
/// Live world and writable output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lightmapped_component(world: *mut PonkoWorld, out_id: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { builtin::<Lightmapped>(world, out_id) }
}

/// How C sees the bake's built-in components.
pub(crate) fn bake_kind(t: std::any::TypeId) -> Option<Kind> {
    // repr(transparent) over a u32, layout checked against
    // PonkoLightmapped; an index the bake doesn't have draws the model as
    // without it.
    (t == std::any::TypeId::of::<Lightmapped>()).then_some(Kind::Plain)
}
