// The sky kit's cloud shadow map (see `render/clouds.rs`): how much
// sunlight gets through the cloud layer, for each point of its base, along
// the sun. Drawn into a 128 x 128 square of the IBL atlas (rows 1408 on,
// `CLOUD_SHADOW_ORIGIN` in mesh.wgsl), where every lit surface reads it
// (`cloud_light`). Composed after the cloud model (sky_kit_common.wgsl).

struct CloudShadow {
    // `SkyKit::frame_params`: [4] is the map's center, 1 / extent, strength.
    kit: array<vec4<f32>, 6>,
    // xyz: toward the sun.
    sun: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cloud_shadow: CloudShadow;

const SHADOW_ORIGIN: vec2<f32> = vec2<f32>(0.0, 1408.0);
const SHADOW_SIZE: f32 = 128.0;
const SHADOW_STEPS: i32 = 6;

@vertex
fn vs_cloud_shadow(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_cloud_shadow(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let k = cloud_shadow.kit;
    let layer = kit_layer(k);
    let uv = (at.xy - SHADOW_ORIGIN) / SHADOW_SIZE;
    let xz = k[4].xy + (uv - 0.5) / k[4].z;
    let sun = cloud_shadow.sun.xyz;
    let up = max(sun.y, 0.05);
    let toward = sun.xz / up;
    // Through the layer along the sun, from its base.
    var depth = 0.0;
    for (var i = 0; i < SHADOW_STEPS; i++) {
        let h = (f32(i) + 0.5) / f32(SHADOW_STEPS);
        depth += kit_shape(xz + toward * (h * layer.thickness), h, layer);
    }
    let path = layer.thickness / up / f32(SHADOW_STEPS);
    // Softer than the clouds' own extinction: light scattered through a
    // cloud still reaches the ground, and thin clouds leave a light shade.
    let through = exp(-depth * path * layer.density * KIT_EXTINCTION * 0.3);
    return vec4<f32>(through, through, through, 1.0);
}
