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
    /// xy: this frame's jitter in pixels.
    pub(super) jitter: [f32; 4],
    /// Global illumination's probe grid (see `gi.wgsl`); all zero without.
    pub(super) gi: [[f32; 4]; 3],
    /// A sky shader's values (`Sky::shader_params`); zero without one.
    pub(super) sky_params: [[f32; 4]; 4],
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
