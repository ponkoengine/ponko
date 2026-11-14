struct Camera {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    inv_view_proj: mat4x4<f32>,
    // w: 1 when this pass can read the scene depth (soft particles).
    eye: vec4<f32>,
    forward: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;
// The scene behind the particles (only the depth is read).
@group(2) @binding(1) var scene_depth_texture: texture_depth_multisampled_2d;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: how far in front of the eye, along the view; y: the softness.
    @location(2) depth_soft: vec2<f32>,
}
@vertex fn vs(
    @builtin(vertex_index) vertex: u32,
    @location(0) position_size: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rotation_sheet: vec4<f32>,
    @location(3) params: vec4<f32>,
) -> Out {
    let corners = array<vec2<f32>, 6>(vec2(-0.5,-0.5),vec2(0.5,-0.5),vec2(-0.5,0.5),vec2(-0.5,0.5),vec2(0.5,-0.5),vec2(0.5,0.5));
    let p = corners[vertex];
    let c = cos(rotation_sheet.x);
    let s = sin(rotation_sheet.x);
    let rotated = vec2(c*p.x-s*p.y,s*p.x+c*p.y)*position_size.w;
    let world = position_size.xyz + camera.right.xyz*rotated.x + camera.up.xyz*rotated.y;
    let columns = rotation_sheet.z;
    let rows = rotation_sheet.w;
    let frame = rotation_sheet.y;
    let cell = vec2(frame % columns, floor(frame/columns));
    var out: Out;
    out.position = camera.view_proj*vec4(world,1.0);
    out.uv = (cell+vec2(p.x+0.5,0.5-p.y))/vec2(columns,rows);
    out.color = color;
    out.depth_soft = vec2(dot(world - camera.eye.xyz, camera.forward.xyz), params.x);
    return out;
}
// Soft particles: how much of the particle shows, fading out over its
// softness in front of the scene behind it.
fn soft_fade(position: vec4<f32>, depth: f32, softness: f32) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    let at = clamp(vec2<i32>(position.xy), vec2<i32>(0), size - 1);
    let scene = textureLoad(scene_depth_texture, at, 0);
    if scene >= 1.0 {
        return 1.0; // the sky: nothing behind to meet
    }
    let ndc = vec2<f32>(position.x / f32(size.x) * 2.0 - 1.0, 1.0 - position.y / f32(size.y) * 2.0);
    let world = camera.inv_view_proj * vec4<f32>(ndc, scene, 1.0);
    let behind = dot(world.xyz / world.w - camera.eye.xyz, camera.forward.xyz);
    return clamp((behind - depth) / softness, 0.0, 1.0);
}
@fragment fn fs(in: Out) -> @location(0) vec4<f32> {
    var texel = textureSample(image,image_sampler,in.uv)*in.color;
    if in.depth_soft.y > 0.0 && camera.eye.w > 0.0 {
        texel = texel * soft_fade(in.position, in.depth_soft.x, in.depth_soft.y);
    }
    return vec4(texel.rgb*texel.a,texel.a);
}
