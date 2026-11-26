
@vertex
fn vs_effect(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

// The game's effect at a pixel of the screen (named so no game effect
// function clashes with it).
fn engine_effect_at(at: vec2<f32>) -> vec3<f32> {
    var p: Pixel;
    p.screen = at;
    p.uv = at * frame.screen.zw;
    p.color = scene_color(at);
    p.world = scene_position(at);
    p.distance = distance(p.world, frame.camera.xyz);
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    p.sky = textureLoad(scene_depth_texture, clamp(vec2<i32>(at), vec2<i32>(0), size - 1), 0) <= 0.0; // reverse-Z: the sky is at 0
    return effect(p);
}

@fragment
fn fs_effect(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(engine_effect_at(at.xy), 1.0);
}

// A target camera's effects: the same, keeping the picture's alpha (a
// see-through background stays see-through).
@fragment
fn fs_effect_view(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene_color_texture));
    let alpha = textureLoad(scene_color_texture, clamp(vec2<i32>(at.xy), vec2<i32>(0), size - 1), 0).a;
    return vec4<f32>(engine_effect_at(at.xy), alpha);
}

// Target effects: the same `effect`, written into a render target of any
// size. Each texel reads the frame at the matching point (`uv` across the
// target, times the frame's size), so a smaller target is a coarse copy.
struct TargetOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_effect_target(@builtin(vertex_index) i: u32) -> TargetOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: TargetOut;
    out.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_effect_target(in: TargetOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    let texel = clamp(vec2<i32>(in.uv * frame.screen.xy), vec2<i32>(0), size - 1);
    let at = vec2<f32>(texel) + 0.5;
    var p: Pixel;
    p.screen = at;
    p.uv = at * frame.screen.zw;
    p.color = scene_color(at);
    p.world = scene_position(at);
    p.distance = distance(p.world, frame.camera.xyz);
    p.sky = textureLoad(scene_depth_texture, texel, 0) <= 0.0; // reverse-Z: the sky is at 0
    return vec4<f32>(effect(p), 1.0);
}
