//@@ponko-mesh-prelude@@
// Screen effects (Graphics::create_effect): a game's `fn effect(p: Pixel) ->
// vec3<f32>` run over the lit 3D frame, before bloom and tone mapping.
// The engine builds each effect from the mesh prelude (frame, material and
// scene bindings and helpers), this file's first part, the game's
// shader and this file's second part (`effect_main.wgsl`).

// A pixel of the lit frame, as an effect gets it.
struct Pixel {
    color: vec3<f32>,   // the light here so far: HDR, before bloom and tone mapping
    world: vec3<f32>,   // the nearest solid surface here, in the world
    distance: f32,      // from the camera to `world`
    sky: bool,          // nothing solid here, only sky or background
    screen: vec2<f32>,  // the pixel's center, in pixels from the top left
    uv: vec2<f32>,      // 0 to 1 across the screen, from the top left
};

// ---- the game's effect ----
//@@ponko-game@@
// ---- the engine's entry points ----

@vertex
fn vs_effect(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fs_effect(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    var p: Pixel;
    p.screen = at.xy;
    p.uv = at.xy * frame.screen.zw;
    p.color = scene_color(at.xy);
    p.world = scene_position(at.xy);
    p.distance = distance(p.world, frame.camera.xyz);
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    p.sky = textureLoad(scene_depth_texture, clamp(vec2<i32>(at.xy), vec2<i32>(0), size - 1), 0) <= 0.0; // reverse-Z: the sky is at 0
    return vec4<f32>(effect(p), 1.0);
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
