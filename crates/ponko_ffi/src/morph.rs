//! Morph targets (blend shapes) from C: shapes on mesh data, weights on
//! entities (Rust `MeshData::morph_targets`, `MorphWeights`,
//! `Graphics::morph_target`).

use ponko::anim::MorphTarget;
use ponko::{
    anim::MorphWeights,
    prelude::{Ctx, Entity},
};

use crate::ctx::{ctx_access, out, text};
use crate::three::{PonkoMesh, PonkoMeshData, PonkoVec3};
use crate::types::to_c;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid, invalid_owned};

/// The most targets one model can have weights for.
const MAX_TARGETS: usize = 1 << 16;

fn entity(c: &Ctx, bits: u64) -> Result<Entity, Failure> {
    let e = Entity::from_bits(bits);
    if c.world.is_alive(e) {
        Ok(e)
    } else {
        Err(invalid("no such entity"))
    }
}

/// Adds a morph target to mesh data (Rust `MeshData::morph_targets.push`):
/// a shape the mesh can blend toward, as one offset per corner (`positions`,
/// `count` of them, which must be the number of corners) and, optionally, one
/// normal offset per corner (`normals`, or null when normals don't change).
/// Targets are numbered in the order they're added; the name is copied.
/// # Safety
/// `data` null or live; `name` a C string; `positions` and `normals` hold
/// `count` values (`normals` may be null).
/// ```
/// use ponko_ffi::{PonkoStatus::*, *};
/// let d = ponko_mesh_data_plane(1.0);
/// let up = [PonkoVec3 { x: 0.0, y: 0.1, z: 0.0 }; 4];
/// assert_eq!(unsafe { ponko_mesh_data_add_morph_target(d, c"lift".as_ptr(), up.as_ptr(), std::ptr::null(), 4) }, PONKO_OK);
/// assert_eq!(unsafe { ponko_mesh_data_add_morph_target(d, c"short".as_ptr(), up.as_ptr(), std::ptr::null(), 3) }, PONKO_INVALID_ARGUMENT);
/// unsafe { ponko_mesh_data_free(d) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_add_morph_target(
    data: *mut PonkoMeshData,
    name: *const std::ffi::c_char,
    positions: *const PonkoVec3,
    normals: *const PonkoVec3,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let Some(data) = data.as_mut() else {
            return Err(invalid("mesh data is null"));
        };
        let name = text(name)?;
        if positions.is_null() {
            return Err(invalid("positions is null"));
        }
        let corners = data.0.positions.len();
        if count != corners {
            return Err(invalid_owned(format!("{count} offsets for {corners} corners")));
        }
        let read =
            |p: *const PonkoVec3| -> Vec<_> { std::slice::from_raw_parts(p, count).iter().map(|&v| to_c(v)).collect() };
        let target = MorphTarget::new(name.to_string(), read(positions)).with_normals(if normals.is_null() {
            Vec::new()
        } else {
            read(normals)
        });
        if target.positions.iter().chain(&target.normals).any(|v| !v.is_finite()) {
            return Err(invalid("an offset isn't a finite number"));
        }
        data.0.morph_targets.push(target);
        Ok(())
    })
}

/// Removes every morph target from mesh data.
/// # Safety
/// `data` null or live.
/// ```
/// use ponko_ffi::{PonkoStatus::*, *};
/// let d = ponko_mesh_data_plane(1.0);
/// assert_eq!(unsafe { ponko_mesh_data_clear_morph_targets(d) }, PONKO_OK);
/// assert_eq!(unsafe { ponko_mesh_data_clear_morph_targets(std::ptr::null_mut()) }, PONKO_INVALID_ARGUMENT);
/// unsafe { ponko_mesh_data_free(d) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_clear_morph_targets(data: *mut PonkoMeshData) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller contract: null or live.
        let Some(data) = (unsafe { data.as_mut() }) else {
            return Err(invalid("mesh data is null"));
        };
        data.0.morph_targets.clear();
        data.0.morph_frames.clear();
        Ok(())
    })
}

/// How many morph targets mesh data has (0 for null).
/// # Safety
/// `data` null or live.
/// ```
/// let d = ponko_ffi::ponko_mesh_data_plane(1.0);
/// assert_eq!(unsafe { ponko_ffi::ponko_mesh_data_morph_target_count(d) }, 0);
/// unsafe { ponko_ffi::ponko_mesh_data_free(d) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_morph_target_count(data: *const PonkoMeshData) -> usize {
    // SAFETY: caller contract: null or live.
    unsafe { data.as_ref() }.map_or(0, |d| d.0.morph_targets.len())
}

/// The index of `mesh`'s morph target called `name`, for the weight calls
/// (Rust `Graphics::morph_target`). `PONKO_NOT_FOUND` when the mesh has no such
/// target or was freed.
/// # Safety
/// `ctx` null or live; `name` a C string; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_morph_target(std::ptr::null_mut(), Default::default(), c"lift".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_morph_target(
    ctx: *mut PonkoCtx,
    mesh: PonkoMesh,
    name: *const std::ffi::c_char,
    out_target: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_target.is_null() {
                return Err(invalid("output is null"));
            }
            let target = c.gfx.morph_target(to_c(mesh), name).ok_or_else(|| {
                error(
                    PonkoStatus::PONKO_NOT_FOUND,
                    "that mesh has no morph target of that name",
                )
            })?;
            out(out_target, target)
        })
    })
}

/// Sets how much of morph target `target` `entity`'s model shows: 0 none, 1
/// all of it; values outside that range exaggerate (Rust
/// `MorphWeights::set`). Gives the entity weights (all 0 but this one) if it
/// had none, so it stops showing its mesh's default weights.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_morph_weight(std::ptr::null_mut(), 0, 0, 1.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_morph_weight(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    target: usize,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            if target >= MAX_TARGETS {
                return Err(invalid_owned(format!("morph target {target} is past {MAX_TARGETS}")));
            }
            match c.world.get_mut::<MorphWeights>(e) {
                Some(w) => w.set(target, weight),
                None => {
                    let mut w = MorphWeights::default();
                    w.set(target, weight);
                    c.world.insert(e, w);
                }
            }
            Ok(())
        })
    })
}

/// How much of morph target `target` `entity`'s model shows (Rust
/// `MorphWeights::get`; 0 for targets never set). `PONKO_NOT_FOUND` when the
/// entity has no weights, in which case its model shows its mesh's default
/// weights.
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_morph_weight(std::ptr::null_mut(), 0, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_morph_weight(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    target: usize,
    out_weight: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let w = c
                .world
                .get::<MorphWeights>(e)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no morph weights"))?;
            out(out_weight, w.get(target))
        })
    })
}

/// Takes `entity`'s morph weights away, so its model shows its mesh's
/// default weights again (from glTF, else 0). Does nothing if it has none.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_clear_morph_weights(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clear_morph_weights(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.remove::<MorphWeights>(e);
            Ok(())
        })
    })
}

/// Links existing mesh targets into a piecewise linear channel (Rust MorphFrame).
/// Frame weights must be positive and distinct, and targets cannot be shared.
/// # Safety
/// `data` null or live.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_mesh_data_add_morph_frame(std::ptr::null_mut(), 0, 0, 0.5) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_add_morph_frame(
    data: *mut PonkoMeshData,
    channel: usize,
    target: usize,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let data = data.as_mut().ok_or_else(|| invalid("mesh data is null"))?;
        data.0.morph_frames.push(ponko::anim::MorphFrame {
            channel,
            target,
            weight,
        });
        if let Err(e) = data.0.check() {
            data.0.morph_frames.pop();
            return Err(invalid_owned(e));
        }
        Ok(())
    })
}

/// Creates an empty animation clip, ready for named morph tracks.
/// # Safety
/// `name` is a C string; `output` writable.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_clip_new(c"face".as_ptr(), std::ptr::null_mut()) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_clip_new(
    name: *const std::ffi::c_char,
    output: *mut *mut crate::PonkoClip,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let name = text(name)?;
        if output.is_null() {
            return Err(invalid("output is null"));
        }
        out(
            output,
            Box::into_raw(Box::new(crate::PonkoClip(ponko::anim::AnimationClip::new(
                name,
                &ponko::anim::Skeleton::new(vec![]),
                vec![],
            )))),
        )?;
        Ok(())
    })
}

/// Adds/replaces one named scalar morph track. `interpolation`: 0 STEP,
/// 1 LINEAR, 2 CUBICSPLINE. Cubic values are in/value/out triples per key;
/// other modes have one value per key. Weights use 1 for full weight.
/// # Safety
/// `clip` null or live; `name` C string; `times` holds `count` floats;
/// `values` holds `count` floats (three times as many for cubic).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_clip_set_morph_track(std::ptr::null_mut(), c"smile".as_ptr(), std::ptr::null(), std::ptr::null(), 0, 1) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_clip_set_morph_track(
    clip: *mut crate::PonkoClip,
    name: *const std::ffi::c_char,
    times: *const f32,
    values: *const f32,
    count: usize,
    interpolation: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let clip = clip.as_mut().ok_or_else(|| invalid("clip is null"))?;
        let name = text(name)?;
        if count == 0 || count > 1_000_000 || times.is_null() || values.is_null() {
            return Err(invalid("invalid morph keys"));
        }
        let interpolation = match interpolation {
            0 => ponko::anim::Interpolation::Step,
            1 => ponko::anim::Interpolation::Linear,
            2 => ponko::anim::Interpolation::CubicSpline,
            _ => return Err(invalid("invalid interpolation")),
        };
        let times = std::slice::from_raw_parts(times, count).to_vec();
        let n = count
            * if interpolation == ponko::anim::Interpolation::CubicSpline {
                3
            } else {
                1
            };
        let values = std::slice::from_raw_parts(values, n);
        let track = ponko::anim::AnimationTrack::try_new(
            times,
            values
                .iter()
                .map(|v| ponko::prelude::Vec4::new(*v, 0., 0., 0.))
                .collect(),
            interpolation,
        )
        .map_err(invalid_owned)?;
        let mut tracks = clip.0.morph_tracks().to_vec();
        tracks.retain(|t| t.name != name);
        tracks.push(ponko::anim::MorphAnimation::new(name, track));
        clip.0 = clip.0.clone().try_with_morph_tracks(tracks).map_err(invalid_owned)?;
        Ok(())
    })
}

/// Writes manually sampled skeleton morph channels to model components.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_write_animated_morphs(std::ptr::null_mut()) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_write_animated_morphs(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.gfx.write_animated_morphs(&mut c.world);
            Ok(())
        })
    })
}
