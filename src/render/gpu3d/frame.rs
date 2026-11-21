//! The frame uniform: the camera, lights, sky and screen as every material
//! shader, effect and compute pass reads them (group 0, binding 0).
//!
//! One [`FrameUniform`] is written per view of a frame: the main view's,
//! each target camera's, each probe view's and each shadow view's (the same
//! frame seen from elsewhere). The buffers belong to whoever owns the view;
//! each is written through a [`Mirror`], so only the parts that changed
//! since the last frame are uploaded.

use crate::render::traffic::{Mirror, Upload};
use crate::three::{Frame3D, MAX_CASCADES, MAX_LIGHT_SHADOW_VIEWS};

/// The frame's camera and light, as the shader reads them.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct FrameUniform {
    pub(super) view_proj: [[f32; 4]; 4],
    /// World to each shadow cascade's clip space.
    pub(super) sun_view_proj: [[[f32; 4]; 4]; MAX_CASCADES],
    /// In the shadow pass: the cascade being drawn.
    pub(super) caster_view_proj: [[f32; 4]; 4],
    pub(super) camera: [f32; 4],
    pub(super) sun_dir: [f32; 4],
    pub(super) sun_color: [f32; 4],
    pub(super) sky: [f32; 4],
    pub(super) ground: [f32; 4],
    /// x: how many cascades (0: no shadows), y: one shadow-map texel in
    /// uv, z: how far the shadow lookup moves along the normal (in
    /// texels), w: where shadows fade out (distance from the camera).
    pub(super) shadow: [f32; 4],
    /// Each cascade's texel size, in world units.
    pub(super) cascade_texel: [f32; 4],
    /// x: seconds since the game started, y: seconds since the last frame.
    pub(super) time: [f32; 4],
    /// Whole and fractional elapsed seconds (current and last drawn frame),
    /// next to `time` so a clock tick is one contiguous write.
    pub(super) time_seconds: [u32; 2],
    pub(super) time_fraction: [f32; 2],
    pub(super) inv_view_proj: [[f32; 4]; 4],
    /// Width and height in pixels, then their inverses.
    pub(super) screen: [f32; 4],
    /// x: a physical sky (1) or not, y: haze, z: cos of the sun's radius,
    /// w: how bright the stars are.
    pub(super) atmosphere: [f32; 4],
    /// rgb: the sunlight the sky is lit by (before the air), w: camera
    /// height in km.
    pub(super) sky_light: [f32; 4],
    /// x: fog density per unit, y: fog height, z: 1 / falloff.
    pub(super) fog: [f32; 4],
    pub(super) fog_color: [f32; 4],
    /// Light at night.
    pub(super) night: [f32; 4],
    /// x: distance fog start, y: its end (0: none), z: sun glow, w: sun focus.
    pub(super) fog_far: [f32; 4],
    pub(super) cluster_depth: [f32; 4],
    pub(super) cluster_view_proj: [[f32; 4]; 4],
    /// World to each point and spot light shadow view's clip space.
    pub(super) light_view_proj: [[[f32; 4]; 4]; MAX_LIGHT_SHADOW_VIEWS],
    /// Each light shadow view's tile in the atlas, in uv: x, y, side.
    pub(super) light_rect: [[f32; 4]; MAX_LIGHT_SHADOW_VIEWS],
    /// x: how many light shadow views, y: one atlas texel in uv, z: how
    /// far the lookup moves along the normal (in texels).
    pub(super) light_shadow: [f32; 4],
    pub(super) probes: [crate::three::probe::ProbeUniform; 4],
    /// The camera without the temporal antialiasing jitter, and last
    /// frame's (the same without TAA).
    pub(super) unjittered_view_proj: [[f32; 4]; 4],
    pub(super) previous_view_proj: [[f32; 4]; 4],
    /// The motion pass's camera motion: what temporal antialiasing's
    /// reprojection of this view's depth makes of a point, in last frame's
    /// clip space (`previous_view_proj`, but for the view model's held
    /// items: see `view_model.rs`).
    pub(super) reprojection: [[f32; 4]; 4],
    /// xy: this frame's jitter in pixels.
    pub(super) jitter: [f32; 4],
    /// Global illumination's probe grid (see `gi.wgsl`); all zero without.
    pub(super) gi: [[f32; 4]; 3],
    /// A sky shader's values (`Sky::shader_params`); zero without one.
    pub(super) sky_params: [[f32; 4]; 4],
    /// The sky kit's settings (see `SkyKit::frame_params`); zero without it.
    pub(super) sky_kit: [[f32; 4]; 6],
}

/// Splits elapsed time before rounding the subsecond part to shader precision.
pub(super) fn split_elapsed(current: f64, past: f64) -> ([u32; 2], [f32; 2]) {
    let elapsed = [current, past];
    (elapsed.map(|t| t as u32), elapsed.map(|t| t.fract() as f32))
}

/// Writes a frame uniform at `at` in `buffer`, whose contents there
/// `mirror` knows: only the vec4s that changed since the last frame (the
/// time, the camera when it moved), not the whole uniform for every view.
pub(super) fn sync_frame(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    at: u64,
    mirror: &mut Mirror,
    uniform: &FrameUniform,
) {
    mirror.upload(queue, Upload::Uniforms, buffer, at, bytemuck::bytes_of(uniform));
}

/// A frame's cluster mapping for the shader: the lights' near plane and
/// slice scale, then how many decals there are and where their cell table
/// starts in the light lists (in words).
pub(super) fn cluster_depth(frame: &Frame3D) -> [f32; 4] {
    let mut depth = frame.lights.depth;
    depth[2] = frame.decals.records.len() as f32;
    depth[3] = frame.lights.lists.len() as f32;
    depth
}

#[cfg(test)]
mod tests {
    use super::{FrameUniform, split_elapsed};

    /// Every field of [`FrameUniform`], in order, with its offset: a field
    /// added to the struct and not here fails
    /// `every_frame_field_is_where_each_shader_reads_it`.
    macro_rules! offsets {
        ($($field:ident),* $(,)?) => {
            [$((stringify!($field), std::mem::offset_of!(FrameUniform, $field))),*]
        };
    }

    /// The `FrameUniform` fields as this file declares them, in order.
    fn declared_fields() -> Vec<&'static str> {
        let source = include_str!("frame.rs");
        let body = source
            .split_once("pub(super) struct FrameUniform {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .expect("the struct is in this file")
            .0;
        body.lines()
            .filter_map(|line| line.trim().strip_prefix("pub(super) "))
            .filter_map(|line| line.split_once(':').map(|(name, _)| name))
            .collect()
    }

    /// The members of WGSL struct `Frame` in `source`: names, offsets and
    /// the struct's size, as naga lays them out (the GPU's layout).
    fn shader_frame(source: &str) -> (Vec<(String, usize)>, usize) {
        let module = wgpu::naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{e}"));
        let (_, ty) = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("Frame"))
            .expect("a Frame struct");
        let wgpu::naga::TypeInner::Struct { members, span } = &ty.inner else {
            panic!("Frame must be a struct");
        };
        let members = members
            .iter()
            .map(|m| (m.name.clone().unwrap_or_default(), m.offset as usize))
            .collect();
        (members, *span as usize)
    }

    /// The Rust uniform, the engine's `mesh.wgsl` (as composed) and the
    /// cook's copy of the prelude (`ponko_cli`'s `mesh_prelude.wgsl`, which
    /// checks a game's shaders before they ship) agree field by field: the
    /// same names in the same order at the same offsets, and the same size.
    /// The split clock (`time_seconds`, `time_fraction`) sits right after
    /// `time` in all three, so a clock tick is one contiguous write.
    #[test]
    fn every_frame_field_is_where_each_shader_reads_it() {
        let rust = offsets!(
            view_proj,
            sun_view_proj,
            caster_view_proj,
            camera,
            sun_dir,
            sun_color,
            sky,
            ground,
            shadow,
            cascade_texel,
            time,
            time_seconds,
            time_fraction,
            inv_view_proj,
            screen,
            atmosphere,
            sky_light,
            fog,
            fog_color,
            night,
            fog_far,
            cluster_depth,
            cluster_view_proj,
            light_view_proj,
            light_rect,
            light_shadow,
            probes,
            unjittered_view_proj,
            previous_view_proj,
            reprojection,
            jitter,
            gi,
            sky_params,
            sky_kit,
        );
        let names: Vec<&str> = rust.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, declared_fields(), "every field of FrameUniform is listed here");
        let engine = super::super::compose_shader("");
        let cook = include_str!("../../../crates/ponko_cli/src/cook/shaders/mesh_prelude.wgsl");
        for (what, source) in [("mesh.wgsl", engine.as_str()), ("the cook's mesh_prelude.wgsl", cook)] {
            let (members, span) = shader_frame(source);
            let shader: Vec<(&str, usize)> = members.iter().map(|(n, o)| (n.as_str(), *o)).collect();
            assert_eq!(shader, rust, "{what}: the Frame struct's fields and offsets");
            assert_eq!(span, size_of::<FrameUniform>(), "{what}: the Frame struct's size");
        }
        // The two clocks next to each other: one 32-byte run.
        let at = |field: &str| rust.iter().find(|(name, _)| *name == field).unwrap().1;
        assert_eq!(at("time_seconds"), at("time") + 16);
        assert_eq!(at("time_fraction"), at("time_seconds") + 8);
        assert_eq!(at("inv_view_proj"), at("time") + 32);
    }

    #[test]
    fn shader_time_preserves_submilliseconds_at_ten_hours_and_after_days() {
        for seconds in [36_000, 3 * 86_400] {
            let past = f64::from(seconds) + 0.25;
            let now = past + 0.000_1;
            assert_eq!(now as f32, past as f32, "legacy absolute time rounds both together");
            let (whole, fraction) = split_elapsed(now, past);
            assert_eq!(whole, [seconds; 2]);
            assert!((f64::from(fraction[0] - fraction[1]) - (now - past)).abs() < 3e-8);

            let (whole, fraction) = split_elapsed(f64::from(seconds), f64::from(seconds) - 0.000_1);
            assert_eq!(whole, [seconds, seconds - 1]);
            assert_eq!(fraction[0], 0.0);
            assert!((fraction[1] - 0.9999).abs() < 3e-8);
        }
    }
}
