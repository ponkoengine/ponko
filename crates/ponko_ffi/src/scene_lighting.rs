//! glTF light units and HDR environments from C (Rust
//! `SceneSettings::light_gltf`, `ModelLight::place_exposed` and
//! `EnvironmentMap`): place a file's lights with an EV100 exposure, place one
//! light of your own the same way, and light the whole scene with a linear
//! HDR or EXR panorama, six cube faces or the procedural studio.
//!
//! The Rust `EnvironmentMap` is not a handle here: the loaders set the
//! scene's environment (`ctx.scene3d.environment`) directly, so there is
//! nothing to free.
use std::ffi::c_char;

use ponko::render::{EnvironmentMap, ModelLight, ModelLightKind};

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::three::{PonkoPointLight3D, PonkoSpotLight3D, PonkoTransform3D, transform_from_c};
use crate::types::{PonkoColor, to_c};
use crate::{PonkoStatus, boundary, engine_error, invalid};

/// Which kind of light a `PonkoModelLight` is (Rust `ModelLightKind`).
/// ```
/// assert_eq!(ponko_ffi::PonkoModelLightKind::PONKO_MODEL_LIGHT_SUN as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoModelLightKind {
    /// A point light: `point` is used.
    PONKO_MODEL_LIGHT_POINT = 0,
    /// A spot light shining along its -Z: `spot` is used.
    PONKO_MODEL_LIGHT_SPOT = 1,
    /// The directional light that is the scene's sun: `sun_color` and `sun_intensity` are used.
    PONKO_MODEL_LIGHT_SUN = 2,
}

/// A light as a glTF file places it (Rust `ModelLight`), for
/// `ponko_ctx_place_light_exposed`. Only the fields of its `kind` are read.
/// Intensities are the file's: candela for point and spot lights, lux for the sun.
/// ```
/// let l = ponko_ffi::PonkoModelLight::default();
/// assert_eq!(l.kind as u32, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoModelLight {
    /// Which of `point`, `spot` or the sun fields is used.
    pub kind: PonkoModelLightKind,
    /// Where it is relative to the model; it points along its -Z.
    pub transform: PonkoTransform3D,
    /// The point light (`PONKO_MODEL_LIGHT_POINT`).
    pub point: PonkoPointLight3D,
    /// The spot light (`PONKO_MODEL_LIGHT_SPOT`).
    pub spot: PonkoSpotLight3D,
    /// The sun's sRGB color (`PONKO_MODEL_LIGHT_SUN`).
    pub sun_color: PonkoColor,
    /// The sun's illuminance in lux, before exposure.
    pub sun_intensity: f32,
}

impl Default for PonkoModelLight {
    /// A white point light of intensity 1 at the model's origin.
    fn default() -> Self {
        let point = ponko::d3::PointLight::default();
        let spot = ponko::d3::SpotLight::default();
        Self {
            kind: PonkoModelLightKind::PONKO_MODEL_LIGHT_POINT,
            transform: crate::three::ponko_transform3d_at(crate::three::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 }),
            point: to_c(point),
            spot: to_c(spot),
            sun_color: to_c(ponko::color::Color::WHITE),
            sun_intensity: 1.0,
        }
    }
}

/// The default `PonkoModelLight`: a white point light at the model's origin.
/// ```
/// assert_eq!(ponko_ffi::ponko_model_light_default().sun_intensity, 1.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_model_light_default() -> PonkoModelLight {
    PonkoModelLight::default()
}

fn model_light_from_c(light: &PonkoModelLight) -> ModelLight {
    let transform = transform_from_c(light.transform);
    let kind = match light.kind {
        PonkoModelLightKind::PONKO_MODEL_LIGHT_POINT => ModelLightKind::Point(to_c(light.point)),
        PonkoModelLightKind::PONKO_MODEL_LIGHT_SPOT => ModelLightKind::Spot(to_c(light.spot)),
        PonkoModelLightKind::PONKO_MODEL_LIGHT_SUN => ModelLightKind::Sun {
            color: to_c(light.sun_color),
            intensity: light.sun_intensity,
        },
    };
    ModelLight {
        name: String::new(),
        transform,
        kind,
    }
}

/// How many point and spot lights the glTF file at `path` places, which is how
/// many entities `ponko_ctx_light_gltf` spawns for it (its directional light
/// is the scene's sun and spawns none). Loads the file if it isn't loaded yet.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_gltf_local_light_count(std::ptr::null_mut(), c"a.glb".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gltf_local_light_count(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        ctx_access(ctx, |c| {
            let lights = c.gfx.try_load_lights(path).map_err(engine_error)?;
            let local = lights
                .iter()
                .filter(|l| !matches!(l.kind, ModelLightKind::Sun { .. }))
                .count();
            out(out_count, local)
        })
    })
}

/// Sets up the lighting of a glTF preview from the file at `path` (Rust
/// `SceneSettings::light_gltf`, with the file's lights from
/// `Graphics::try_load_lights`): its punctual lights placed at `at` with
/// exposure `ev100` (lux and candela are multiplied by `1 / (1.2 * 2^ev100)`,
/// a non-finite `ev100` is 0, the rest limited to ±32), the hue-preserving
/// tone curve, and the neutral studio environment if the scene has none. The
/// file's directional light replaces the scene's sun. Call once per loaded
/// scene.
///
/// The spawned point and spot lights' entities go to `out_entities`, so a
/// viewer can despawn them when changing models; `capacity` must hold all of
/// them (`ponko_ctx_gltf_local_light_count`), else nothing changes and the
/// status is `PONKO_INVALID_ARGUMENT`. Pass null with capacity 0 to not get
/// them. The number of entities is written to `out_count`. Errors name the
/// file.
/// ```
/// let at = ponko_ffi::ponko_transform3d_at(ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
/// let status = unsafe { ponko_ffi::ponko_ctx_light_gltf(std::ptr::null_mut(), c"a.glb".as_ptr(), at, 8.0, std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated; `out_entities` writable for `capacity`
/// entities (or null with capacity 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_light_gltf(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoTransform3D,
    ev100: f32,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        if out_count.is_null() {
            return Err(invalid("out_count is null"));
        }
        if out_entities.is_null() && capacity > 0 {
            return Err(invalid("out_entities is null"));
        }
        ctx_access(ctx, |c| {
            let lights = c.gfx.try_load_lights(path).map_err(engine_error)?;
            let local = lights
                .iter()
                .filter(|l| !matches!(l.kind, ModelLightKind::Sun { .. }))
                .count();
            if !out_entities.is_null() && capacity < local {
                return Err(invalid("capacity is below the file's local light count"));
            }
            let spawned = c
                .scene3d
                .light_gltf(&mut c.world, &lights, &transform_from_c(at), ev100);
            if !out_entities.is_null() {
                for (i, entity) in spawned.iter().enumerate() {
                    out_entities.add(i).write(entity.to_bits());
                }
            }
            out(out_count, spawned.len())
        })
    })
}

/// Places one light as a glTF file places it, with a camera exposure (Rust
/// `ModelLight::place_exposed`): lux and candela are multiplied by
/// `1 / (1.2 * 2^ev100)` (a non-finite `ev100` is 0, the rest limited to ±32).
/// A point or spot light spawns an entity (written to `out_entity`, with
/// `out_spawned` true); a point or spot light with `range` equal to
/// the loader's derived reach grows it to where the exposed light fades. The
/// sun sets the scene's sun direction, color and intensity and spawns nothing
/// (`out_spawned` false, `out_entity` 0). Use one `ev100` for all the lights
/// of a scene. `light` is read during the call only.
/// ```
/// let at = ponko_ffi::ponko_transform3d_at(ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
/// let l = ponko_ffi::ponko_model_light_default();
/// let status = unsafe { ponko_ffi::ponko_ctx_place_light_exposed(std::ptr::null_mut(), &l, at, 8.0, std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// Live ctx; `light` readable; `out_entity` and `out_spawned` writable (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_place_light_exposed(
    ctx: *mut PonkoCtx,
    light: *const PonkoModelLight,
    at: PonkoTransform3D,
    ev100: f32,
    out_entity: *mut u64,
    out_spawned: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let light = light.as_ref().ok_or_else(|| invalid("light is null"))?;
        let light = model_light_from_c(light);
        if !(light.transform.scale.is_finite() && light.transform.position.is_finite()) {
            return Err(invalid("the light's transform isn't finite"));
        }
        ctx_access(ctx, |c| {
            let placed = light.place_exposed(&mut c.world, &mut c.scene3d.lighting, &transform_from_c(at), ev100);
            if !out_entity.is_null() {
                out_entity.write(placed.map_or(0, |e| e.to_bits()));
            }
            if !out_spawned.is_null() {
                out_spawned.write(placed.is_some());
            }
            Ok(())
        })
    })
}

/// Lights the scene with a linear HDR or EXR panorama (Rust
/// `Graphics::try_load_environment`, set as `ctx.scene3d.environment`): a
/// 2:1 latitude-longitude image with +Y at the top and +X at the center, from
/// the asset sources or `assets/`. The renderer filters it into the same
/// lighting atlas as the sky and local probes the first frame it is drawn.
/// The visible sky and fog stay `PonkoSky`'s. On failure (not found, not an
/// image, non-finite radiance, not 2:1) the old environment stays.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_environment(std::ptr::null_mut(), c"a.hdr".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_environment(ctx: *mut PonkoCtx, path: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        ctx_access(ctx, |c| {
            c.scene3d.environment = Some(c.gfx.try_load_environment(path).map_err(engine_error)?);
            Ok(())
        })
    })
}

/// Lights the scene with six linear HDR or EXR images, a cubemap (Rust
/// `Graphics::try_load_environment_cube`): `paths` points to six
/// NUL-terminated paths in the order +X, -X, +Y, -Y, +Z, -Z, all equal-size
/// squares. At each face center the directions to the right and up are
/// (-Z, +Y), (+Z, +Y), (+X, -Z), (+X, +Z), (+X, +Y) and (-X, +Y). On failure
/// the old environment stays.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_environment_cube(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `paths` readable for six NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_environment_cube(
    ctx: *mut PonkoCtx,
    paths: *const *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if paths.is_null() {
            return Err(invalid("paths is null"));
        }
        let mut faces = [""; 6];
        for (i, face) in faces.iter_mut().enumerate() {
            *face = text(paths.add(i).read())?;
        }
        ctx_access(ctx, |c| {
            c.scene3d.environment = Some(c.gfx.try_load_environment_cube(faces).map_err(engine_error)?);
            Ok(())
        })
    })
}

/// Lights the scene with the neutral procedural studio (Rust
/// `EnvironmentMap::studio`): a broad ceiling light, two softboxes and dim
/// fill, with no image to load.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_studio_environment(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_studio_environment(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.scene3d.environment = Some(EnvironmentMap::studio());
            Ok(())
        })
    })
}

/// Goes back to lighting the scene from the sky (Rust: setting
/// `ctx.scene3d.environment` to `None`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_clear_environment(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clear_environment(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.scene3d.environment = None;
            Ok(())
        })
    })
}

/// Whether the scene has an environment map set (Rust
/// `ctx.scene3d.environment.is_some()`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_has_environment(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `out_has` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_has_environment(ctx: *mut PonkoCtx, out_has: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_has, c.scene3d.environment.is_some())) })
}
