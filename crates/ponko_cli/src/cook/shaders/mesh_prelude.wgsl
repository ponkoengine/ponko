// The 256 x 128 sky occupies the top-left of either its own texture or
// the shared IBL atlas. Filtering must never reach a neighbouring region.
// Longitude is periodic: blend the last and first columns across -X;
// clamping longitude alone would leave a one-texel discontinuity there.
fn sample_sky(image: texture_2d<f32>, linear_sampler: sampler, uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(256.0, 128.0);
    let dimensions = vec2<f32>(textureDimensions(image));
    let pixel = vec2<f32>(fract(uv.x) * size.x, clamp(uv.y * size.y, 0.5, size.y - 0.5));
    if pixel.x < 0.5 || pixel.x > size.x - 0.5 {
        let first = textureSampleLevel(image, linear_sampler, vec2<f32>(0.5, pixel.y) / dimensions, 0.0).rgb;
        let last = textureSampleLevel(image, linear_sampler, vec2<f32>(size.x - 0.5, pixel.y) / dimensions, 0.0).rgb;
        // Pixel coordinates measure from the region's edge, not its centres.
        return mix(last, first, fract(pixel.x + 0.5));
    }
    return textureSampleLevel(image, linear_sampler, pixel / dimensions, 0.0).rgb;
}

// Instanced 3D meshes: one instance = one Model, lit by a sun (Cook-Torrance
// GGX, metallic-roughness, as glTF) and sky/ground ambient light.
//
// The engine builds each material shader from three parts: this prelude,
// the game's shader (its `vertex` and `surface` functions, when it has
// them) and `mesh_main.wgsl`, which calls them.

const PI: f32 = 3.14159265;

struct ReflectionVolume { center: vec4<f32>, size: vec4<f32>, params: vec4<f32> };

struct Frame {
    view_proj: mat4x4<f32>,
    // World to each shadow cascade's clip space (see `sunlight`).
    sun_view_proj: array<mat4x4<f32>, 4>,
    // In the shadow pass: the cascade being drawn.
    caster_view_proj: mat4x4<f32>,
    camera: vec4<f32>,     // xyz: camera position; w: 0 in shadow maps, else 1
    sun_dir: vec4<f32>,    // xyz: unit vector towards the sun; w: contact-hardening shadow edge width per unit of distance (0: off)
    sun_color: vec4<f32>,  // rgb: linear color times intensity
    sky: vec4<f32>,        // rgb: linear, times ambient intensity
    ground: vec4<f32>,
    shadow: vec4<f32>,     // cascades (0: no shadows), texel size in uv, normal offset in texels, fade distance
    cascade_texel: vec4<f32>, // each cascade's texel size, in world units
    time: vec4<f32>,       // x: seconds since the game started, y: seconds since the last frame; zw: the last drawn frame's xy
    time_seconds: vec2<u32>, // whole elapsed seconds: current and last drawn frame
    time_fraction: vec2<f32>, // fractional elapsed seconds: current and last drawn frame
    inv_view_proj: mat4x4<f32>, // clip space back to the world
    screen: vec4<f32>,     // width and height in pixels, then their inverses
    atmosphere: vec4<f32>, // x: a physical sky (1) or not (0), y: haze, z: cos of the sun's radius, w: stars
    sky_light: vec4<f32>,  // rgb: the sunlight the sky is lit by, w: camera height in km
    fog: vec4<f32>,        // x: density per unit, y: fog height, z: 1 / falloff, w: light shafts
    fog_color: vec4<f32>,  // rgb: linear, without an atmosphere; w: screen-space reflections on transparent surfaces, their strength (0: off)
    night: vec4<f32>,      // rgb: light at night; w: how far those reflections look
    fog_far: vec4<f32>,    // x: distance fog start, y: its end (0: none), z: sun glow, w: sun focus
    cluster_depth: vec4<f32>,
    cluster_view_proj: mat4x4<f32>,
    // World to each point and spot light shadow view's clip space.
    light_view_proj: array<mat4x4<f32>, 24>,
    // Each view's tile in the light shadow atlas, in uv: x, y, side.
    light_rect: array<vec4<f32>, 24>,
    light_shadow: vec4<f32>, // views (0: none), atlas texel size in uv, normal offset in texels, w: sun shadow filter (1 to 4)
    probes: array<ReflectionVolume, 4>,
    // Temporal antialiasing: `view_proj` carries the sub-pixel jitter; these
    // are the camera without it, and last frame's, to tell motion from jitter.
    unjittered_view_proj: mat4x4<f32>,
    previous_view_proj: mat4x4<f32>,
    // The motion pass: where temporal antialiasing's reprojection of this
    // view's depth puts a point of this frame last frame, in clip space.
    // `previous_view_proj` for the world; the view model's held items take
    // the main camera's reprojection of the depth sliver they are drawn in.
    reprojection: mat4x4<f32>,
    jitter: vec4<f32>,     // xy: this frame's jitter in pixels (0 without TAA); z: the mip bias (see texture_mip_bias)
    gi: array<vec4<f32>, 3>, // global illumination's probe grid (see gi.wgsl)
    sky_params: array<vec4<f32>, 4>, // a sky shader's values (`Sky::shader_params`)
    sky_kit: array<vec4<f32>, 6>, // the sky kit's clouds, moon and cloud shadows (zero without it)
};

struct MaterialData {
    color: vec4<f32>,      // linear RGBA
    emissive: vec4<f32>,   // linear RGB; w: unlit (1)
    params: vec4<f32>,     // roughness, metallic, normal map strength, alpha cutoff
    maps: vec4<u32>,       // has: normal map, ORM map, emissive map; w: masked tint
    custom: array<vec4<f32>, 4>,
    // Layers over the surface (see material_layers.wgsl): which are on,
    // where their maps are, and their values. Read only where they're on.
    layer_info: vec4<u32>,
    layers: array<vec4<f32>, 9>,
    // The texture coordinates' transform (KHR_texture_transform): the
    // first two rows of its matrix, (a, b, offset) each.
    uv_transform: array<vec4<f32>, 2>,
    // EXTRA_UV_UNIFORM
};

// Texture coordinates moved by the material's texture transform.
fn material_uv(uv: vec2<f32>) -> vec2<f32> {
    let t = material.uv_transform;
    return vec2<f32>(dot(t[0].xyz, vec3<f32>(uv, 1.0)), dot(t[1].xyz, vec3<f32>(uv, 1.0)));
}

// MULTI_UV_IMPLEMENTATION
const MATERIAL_MULTI_UV: bool = false;
fn map_uv(uv: vec2<f32>, uv1: vec2<f32>, kind: u32) -> vec2<f32> {
    return uv;
}
fn same_map_uv(a: u32, b: u32) -> bool {
    return true;
}
// END_MULTI_UV_IMPLEMENTATION

// Map kinds (see TextureMap). Layer map kinds start at 5.
const UV_BASE: u32 = 0u;
const UV_EMISSIVE: u32 = 1u;
const UV_NORMAL: u32 = 2u;
const UV_METALLIC_ROUGHNESS: u32 = 3u;
const UV_OCCLUSION: u32 = 4u;
const UV_LAYER_START: u32 = 5u;

// Where a cut-out material's alpha cuts (Blend::Cutout).
fn alpha_cutoff() -> f32 {
    return material.params.w;
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
// The sky around the camera (see sky_radiance), made each frame.
@group(0) @binding(3) var sky_texture: texture_2d<f32>;
@group(0) @binding(4) var sky_sampler: sampler;
@group(0) @binding(5) var punctual_records: texture_2d<f32>;
@group(0) @binding(6) var punctual_clusters: texture_2d<u32>;
// Point and spot lights' shadow views, one tile each (see punctual_shadow).
@group(0) @binding(7) var light_shadow_map: texture_depth_2d;
@group(1) @binding(0) var<uniform> material: MaterialData;
@group(1) @binding(1) var material_sampler: sampler;
@group(1) @binding(2) var base_texture: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var orm_texture: texture_2d<f32>;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var custom_texture0: texture_2d<f32>;
@group(1) @binding(7) var custom_texture1: texture_2d<f32>;
@group(1) @binding(8) var custom_array0: texture_2d_array<f32>;
@group(1) @binding(9) var custom_array1: texture_2d_array<f32>;
@group(1) @binding(10) var custom_array2: texture_2d_array<f32>;
// The opaque scene behind a transparent surface (see scene_color).
@group(2) @binding(0) var scene_color_texture: texture_2d<f32>;
@group(2) @binding(1) var scene_depth_texture: texture_depth_multisampled_2d;

// A vertex in world space, as a shader's `vertex` function gets and returns it.
struct Vertex {
    instance_index: u32, // index into a mesh GPU buffer
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    color: vec4<f32>, // the model's tint times the vertex color
    uv1: vec2<f32>,   // the mesh's second texture coordinates (zero without)
    origin: vec3<f32>, // read-only: where the model is (its transform's position)
};

// A pixel's surface, as a shader's `surface` function gets and returns it:
// filled in from the material and its textures first.
struct Surface {
    albedo: vec3<f32>,
    alpha: f32,
    normal: vec3<f32>,   // world space, unit length
    roughness: f32,
    metallic: f32,
    occlusion: f32,      // 1: fully lit by ambient light, 0: none
    emissive: vec3<f32>,
    unlit: bool,        // true: albedo + emissive, without direct or ambient lighting
    // Read-only inputs.
    world: vec3<f32>,
    uv: vec2<f32>,
    color: vec4<f32>,    // the model's tint
    view: vec3<f32>,     // unit vector from the surface to the camera
    screen: vec2<f32>,   // the pixel, in pixels from the top left
    uv1: vec2<f32>,      // the mesh's second texture coordinates (zero without)
    origin: vec3<f32>,   // where the model is (its transform's position)
};

// Whether this draw is into a shadow map (seen from the sun), not for a camera.
fn drawing_shadows() -> bool {
    return frame.camera.w == 0.0;
}

// The mip bias to sample material textures with: while the temporal
// upscaler draws the frame smaller than the window, `log2` of the render
// scale (negative), so textures are as sharp as at the window's resolution;
// otherwise 0. For `textureSampleBias(t, material_sampler, uv, texture_mip_bias())`.
fn texture_mip_bias() -> f32 {
    return frame.jitter.z;
}

// The motion pass (temporal antialiasing, temporal AO) runs `vertex` a
// second time, as of the last drawn frame, to see where each corner was:
// these are set only then. The engine has the game's shader read
// `frame.time` and `items[i]` through `motion_frame_time` and `motion_item`,
// which give the last frame's values while they are.
var<private> motion_past_time: bool;
var<private> motion_past_items: bool;

// Whether `vertex` is running as of the last frame, for the motion pass:
// a shader that moves vertices by inputs of its own (a wind strength in
// `material.custom`) can pick the last frame's here.
fn drawing_last_frame() -> bool {
    return motion_past_time;
}

// `frame.time` as of the frame `vertex` is running for.
fn motion_frame_time() -> vec4<f32> {
    if motion_past_time {
        return frame.time.zwzw;
    }
    return frame.time;
}

// Elapsed seconds within a whole-second period. Reduce the integer part
// before converting it to f32 so small phases stay smooth for days.
fn ponko_elapsed_time_wrapped(period_seconds: u32) -> f32 {
    let i = select(0u, 1u, drawing_last_frame());
    return f32(frame.time_seconds[i] % max(period_seconds, 1u)) + frame.time_fraction[i];
}

// The unit direction from `p` towards whoever is looking: the camera, or
// the sun when drawing shadow maps. Billboards face it in their `vertex`
// function, so their shadows are cast by the side the sun sees.
fn toward_viewer(p: vec3<f32>) -> vec3<f32> {
    if drawing_shadows() {
        return frame.sun_dir.xyz;
    }
    return normalize(frame.camera.xyz - p);
}

// The opaque scene's light at a pixel, before tone mapping (in screen
// effects: the frame so far; in scene compute passes: the settled frame).
// Only transparent materials, effects and scene compute passes see the
// scene; elsewhere this is black.
fn scene_color(screen: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(scene_color_texture));
    let at = clamp(vec2<i32>(screen), vec2<i32>(0), size - 1);
    return textureLoad(scene_color_texture, at, 0).rgb;
}

// Where the opaque scene is at a pixel, in the world. Only transparent
// materials, effects and scene compute passes see the scene; elsewhere this
// reads a stand-in depth.
fn scene_position(screen: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    let at = clamp(vec2<i32>(screen), vec2<i32>(0), size - 1);
    let depth = textureLoad(scene_depth_texture, at, 0);
    let ndc = vec2<f32>(screen.x * frame.screen.z * 2.0 - 1.0, 1.0 - screen.y * frame.screen.w * 2.0);
    let world = frame.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    return world.xyz / world.w;
}

// How far the opaque scene is from the camera at a pixel.
fn scene_distance(screen: vec2<f32>) -> f32 {
    return distance(scene_position(screen), frame.camera.xyz);
}

// Where direction `dir` is in the sky texture: around the horizon in u,
// from straight up (v 0) to straight down (v 1), squeezed towards the
// horizon where the sky changes fastest.
fn sky_uv(dir: vec3<f32>) -> vec2<f32> {
    let around = atan2(dir.z, dir.x) / (2.0 * PI) + 0.5;
    let up = asin(clamp(dir.y, -1.0, 1.0)) / (0.5 * PI);
    return vec2<f32>(around, 0.5 - 0.5 * sign(up) * sqrt(abs(up)));
}

// The direction at `uv` of the sky texture (the inverse of sky_uv).
fn sky_dir(uv: vec2<f32>) -> vec3<f32> {
    let around = (uv.x - 0.5) * 2.0 * PI;
    let s = (0.5 - uv.y) * 2.0;
    let up = sign(s) * s * s * 0.5 * PI;
    return vec3<f32>(cos(up) * cos(around), sin(up), cos(up) * sin(around));
}

// The sky's light from direction `dir` (towards the sky), without the sun
// itself: the atmosphere's or the game's sky shader's. Black for a flat sky.
fn sky_radiance(dir: vec3<f32>) -> vec3<f32> {
    return sample_sky(sky_texture, sky_sampler, sky_uv(dir));
}

// `color` seen through the fog between the camera and `world`: height fog,
// thickest at the fog height and thinning out above it, and distance fog,
// none before its start and all fog past its end. Both take the sky's color
// behind them (or the fog color, without an atmosphere), and glow toward the
// sun. The sky itself takes only the height fog (`distance` false).
fn fog_along(color: vec3<f32>, world: vec3<f32>, distance: bool) -> vec3<f32> {
    let density = frame.fog.x;
    let far = select(0.0, frame.fog_far.y, distance);
    if density <= 0.0 && far <= 0.0 {
        return color;
    }
    let ray = world - frame.camera.xyz;
    let span = length(ray);
    let k = frame.fog.z;
    // The fog along the ray: density * exp(-(height - fog height) * k),
    // integrated from the camera to `world`.
    let start = density * exp(-(frame.camera.y - frame.fog.y) * k);
    let rise = ray.y * k;
    let along = select((1.0 - exp(-rise)) / rise, 1.0, abs(rise) < 1e-4);
    var seen = exp(-start * span * along);
    if far > 0.0 {
        let near = min(frame.fog_far.x, far - 1e-3);
        seen *= clamp((far - span) / (far - near), 0.0, 1.0);
    }
    let dir = ray / max(span, 1e-6);
    var fog_color = frame.fog_color.rgb;
    if frame.atmosphere.x > 0.0 {
        fog_color = sky_radiance(normalize(vec3<f32>(dir.x, max(dir.y, 0.03), dir.z)));
    }
    // Light scattered forward out of the sun (Henyey-Greenstein, 1 at focus 0).
    let g = frame.fog_far.w;
    let mu = dot(dir, frame.sun_dir.xyz);
    let phase = (1.0 - g * g) / pow(max(1.0 + g * g - 2.0 * g * mu, 1e-4), 1.5);
    // Clouds over the sun (the sky kit's) dim the fog's glow.
    var glow = frame.fog_far.z * cloud_light(frame.camera.xyz);
    // The shafts can only darken this pixel's glow, by at most what the glow
    // adds; where that is below what a display shows (looking away from the
    // sun, or through little fog), skip the march.
    let visible = max(max(frame.sun_color.r, frame.sun_color.g), frame.sun_color.b) * phase * glow * (1.0 - seen);
    if visible > SHAFT_VISIBLE && density > 0.0 && frame.fog.w > 0.0 {
        glow *= mix(1.0, fog_in_sun(world, start * span * along), frame.fog.w);
    }
    fog_color += frame.sun_color.rgb * phase * glow;
    return mix(fog_color, color, seen);
}

// Steps of the march through the fog for light shafts.
const SHAFT_STEPS: i32 = 16;
// The least glow (in linear color) the shafts are marched for: below it,
// darkening it to nothing changes the pixel by less than a display shows.
const SHAFT_VISIBLE: f32 = 0.002;

// Of the sunlight the fog between the camera and `world` scatters toward
// the camera, the share that isn't in shadow (`optical`: the fog's whole
// optical depth along the ray). Marched to the shadow distance, each step
// weighted by how thick the fog is there and how much of it gets through
// to the camera; past the shadows, the fog is all in sun. Shadows cut the
// glow into shafts.
fn fog_in_sun(world: vec3<f32>, optical: f32) -> f32 {
    let count = i32(frame.shadow.x);
    if count == 0 || frame.camera.w == 0.0 {
        return 1.0;
    }
    let ray = world - frame.camera.xyz;
    let span = max(length(ray), 1e-6);
    let reach = min(span, frame.shadow.w);
    let dt = reach / f32(SHAFT_STEPS);
    // A different offset each pixel and frame: the noise is fine grained,
    // and with temporal antialiasing it averages away.
    let clip = frame.view_proj * vec4<f32>(world, 1.0);
    let pixel = floor((clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5) * frame.screen.xy);
    let turn = f32(u32(frame.time.x * 60.0) % 64u);
    let jitter = fract(52.9829189 * fract(dot(pixel + 5.588238 * turn, vec2<f32>(0.06711056, 0.00583715))));
    // The sun's cascades are orthographic, so a point's shadow map coordinates
    // move linearly along the ray: project its two ends once per cascade and
    // interpolate, rather than projecting every step into every cascade.
    let end = frame.camera.xyz + ray * (reach / span);
    var start_at: array<vec3<f32>, 4>;
    var along: array<vec3<f32>, 4>;
    for (var c = 0; c < count; c++) {
        let a = (frame.sun_view_proj[c] * vec4<f32>(frame.camera.xyz, 1.0)).xyz;
        start_at[c] = a;
        along[c] = (frame.sun_view_proj[c] * vec4<f32>(end, 1.0)).xyz - a;
    }
    var through = 1.0;
    var lit = 0.0;
    for (var i = 0; i < SHAFT_STEPS; i++) {
        let f = (f32(i) + jitter) / f32(SHAFT_STEPS);
        let p = frame.camera.xyz + (end - frame.camera.xyz) * f;
        let thick = frame.fog.x * exp(min(-(p.y - frame.fog.y) * frame.fog.z, 20.0)) * dt;
        let scattered = through * (1.0 - exp(-thick));
        through *= exp(-thick);
        // The sharpest cascade that holds the point (all sun outside them).
        var sun = 1.0;
        for (var c = 0; c < count; c++) {
            let ndc = start_at[c] + along[c] * f;
            if max(abs(ndc.x), abs(ndc.y)) < 1.0 && ndc.z <= 1.0 {
                sun = textureSampleCompareLevel(shadow_map, shadow_sampler, vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5), c, ndc.z);
                break;
            }
        }
        lit += scattered * sun;
    }
    let all = 1.0 - exp(-optical);
    // Past the shadow distance: all in sun.
    let beyond = max(through - exp(-optical), 0.0);
    return clamp((lit + beyond) / max(all, 1e-5), 0.0, 1.0);
}

// `color` seen through all the fog between the camera and `world`.
fn fog(color: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    return fog_along(color, world, true);
}

// A pseudo-random number in 0..1 for a 2D point.
fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

// Smooth value noise in 0..1.
fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Five octaves of noise2, in 0..1.
fn fbm2(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < 5; i++) {
        sum += noise2(q) * amplitude;
        q = q * 2.03 + vec2<f32>(17.0, 31.0);
        amplitude *= 0.5;
    }
    return sum / 0.96875;
}

fn direct_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    // Materials with layers (see material_layers.wgsl); a constant, so
    // the plain build has none of their code.
    if MATERIAL_LAYERS {
        return layered_light(s, l, radiance);
    }
    let n = s.normal;
    let v = s.view;
    let h = (l + v) / max(length(l + v), 1e-6);
    let n_l = max(dot(n, l), 0.0);
    let n_v = max(dot(n, v), 1e-4);
    let n_h = max(dot(n, h), 0.0);
    // At most 1: with the light straight behind the viewer, `h` is `v` and
    // the dot product can round past 1, and `pow` of the negative
    // `1 - v_h` below is NaN where the GPU computes it as exp2(5 * log2(x))
    // (AMD's Vulkan driver; Direct3D's compiler turns it into products).
    // A probe of global illumination sees such a point behind every lamp,
    // and its NaN spread to every probe through their light.
    let v_h = clamp(dot(v, h), 0.0, 1.0);

    let a = s.roughness * s.roughness;
    let a2 = a * a;
    let d_denom = n_h * n_h * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * d_denom * d_denom);
    let k = (s.roughness + 1.0) * (s.roughness + 1.0) / 8.0;
    let g = n_v / (n_v * (1.0 - k) + k) * n_l / (n_l * (1.0 - k) + k);
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - v_h, 5.0);
    let specular = d * g * f / max(4.0 * n_v * n_l, 1e-4);
    let diffuse = (vec3<f32>(1.0) - f) * (1.0 - s.metallic) * s.albedo / PI;
    return (diffuse + specular) * radiance * n_l;
}

// The froxel (16 x 9 screen cells, 24 slices of depth) holding a world
// position, or -1 when there are no clusters this frame.
fn cluster_cell(world: vec3<f32>) -> i32 {
    let clip = frame.cluster_view_proj * vec4<f32>(world, 1.0);
    if clip.w <= 0.0 || frame.cluster_depth.x <= 0.0 { return -1; }
    let xy = vec2<u32>(clamp(floor((clip.xy / clip.w * 0.5 + 0.5) * vec2<f32>(16.0, 9.0)), vec2<f32>(0.0), vec2<f32>(15.0, 8.0)));
    let z = u32(clamp(floor(log(max(clip.w, frame.cluster_depth.x) / frame.cluster_depth.x) * frame.cluster_depth.y), 0.0, 23.0));
    return i32((z * 9u + xy.y) * 16u + xy.x);
}

// Evaluate only the lights in the world position's froxel, using the same
// GGX BRDF as sunlight. Available to custom material shaders too.
fn punctual_light(s: Surface) -> vec3<f32> {
    let found = cluster_cell(s.world);
    if found < 0 { return vec3<f32>(0.0); }
    let cell = u32(found);
    let offset = cluster_word(cell * 2u);
    let count = cluster_word(cell * 2u + 1u);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < count; i++) {
        let id = i32(cluster_word(offset + i));
        let pr = textureLoad(punctual_records, vec2<i32>(0, id), 0);
        let delta = pr.xyz - s.world;
        let d2 = dot(delta, delta);
        if d2 >= pr.w * pr.w { continue; }
        let l = delta / max(sqrt(d2), 1e-6);
        let cone = textureLoad(punctual_records, vec2<i32>(2, id), 0);
        let shadow_info = textureLoad(punctual_records, vec2<i32>(3, id), 0);
        let physical = shadow_info.w != 0.0;
        var attenuation = 1.0;
        if cone.w > -1.0 {
            let inner = shadow_info.x;
            let angle = dot(-l, cone.xyz);
            // A zero-width transition is an explicit hard cone.
            let ramp = clamp((angle - cone.w) / max(inner - cone.w, 1e-6), 0.0, 1.0);
            let falloff = select(smoothstep(cone.w, max(inner, cone.w + 1e-6), angle), ramp * ramp, physical);
            attenuation = select(falloff, select(0.0, 1.0, angle >= cone.w), inner == cone.w);
        }
        let window = max(1.0 - (d2 / (pr.w * pr.w)) * (d2 / (pr.w * pr.w)), 0.0);
        attenuation *= select(window * window / max(d2, 1.0), window / max(d2, 1e-4), physical);
        if shadow_info.y >= 0.0 && attenuation > 0.0 {
            let backlight = MATERIAL_LAYERS && (transmission.diffuse > 0.0 || transmission.scatter > 0.0)
                && dot(s.normal, l) < 0.0;
            let bias_normal = select(s.normal, -s.normal, backlight);
            attenuation *= punctual_shadow(i32(shadow_info.y), shadow_info.z, pr.xyz, s.world, bias_normal, cone.w <= -1.0);
        }
        let radiance = textureLoad(punctual_records, vec2<i32>(1, id), 0).rgb * attenuation;
        sum += direct_light(s, l, radiance);
    }
    return sum;
}
// How much of a point or spot light at `light` reaches `world` (0 in its
// shadow, 1 lit), from its shadow views starting at `first`: a spot light's
// one, or a point light's six cube faces (+X, -X, +Y, -Y, +Z, -Z). `texel`
// is a texel's world size per unit of distance from the light.
fn punctual_shadow(first: i32, texel: f32, light: vec3<f32>, world: vec3<f32>, n: vec3<f32>, point: bool) -> f32 {
    if f32(first) >= frame.light_shadow.x {
        return 1.0;
    }
    // Looking up a little off the surface keeps it from shadowing itself;
    // texels grow with distance from the light.
    let p = world + n * (frame.light_shadow.z * texel * distance(world, light));
    var view = first;
    if point {
        let v = p - light;
        let a = abs(v);
        if a.x >= a.y && a.x >= a.z {
            view += select(1, 0, v.x > 0.0);
        } else if a.y >= a.z {
            view += select(3, 2, v.y > 0.0);
        } else {
            view += select(5, 4, v.z > 0.0);
        }
    }
    let clip = frame.light_view_proj[view] * vec4<f32>(p, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z > 1.0 {
        return 1.0;
    }
    // Into the view's atlas tile; filtering stays half a texel inside it,
    // off its neighbors.
    let rect = frame.light_rect[view];
    let uv = rect.xy + vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) * rect.z;
    let step = frame.light_shadow.y;
    let lo = rect.xy + 0.5 * step;
    let hi = rect.xy + rect.z - 0.5 * step;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let at = clamp(uv + vec2<f32>(f32(x), f32(y)) * step, lo, hi);
            lit += textureSampleCompareLevel(light_shadow_map, shadow_sampler, at, ndc.z);
        }
    }
    return lit / 9.0;
}

fn cluster_word(i: u32) -> u32 {
    return textureLoad(punctual_clusters, vec2<i32>(i32(i % 1024u), i32(i / 1024u)), 0).x;
}

fn cube_uv(d: vec3<f32>) -> vec3<f32> {
    let a = abs(d); var f = 0.0; var p = vec2<f32>(0.0);
    if a.x >= a.y && a.x >= a.z {
        if d.x > 0.0 { f = 0.0; p = vec2<f32>(-d.z, -d.y) / a.x; }
        else { f = 1.0; p = vec2<f32>(d.z, -d.y) / a.x; }
    } else if a.y >= a.z {
        if d.y > 0.0 { f = 2.0; p = vec2<f32>(d.x, d.z) / a.y; }
        else { f = 3.0; p = vec2<f32>(d.x, -d.z) / a.y; }
    } else {
        if d.z > 0.0 { f = 4.0; p = vec2<f32>(d.x, -d.y) / a.z; }
        else { f = 5.0; p = vec2<f32>(-d.x, -d.y) / a.z; }
    }
    return vec3<f32>(p * 0.5 + 0.5, f);
}

fn environment_sample(slot: f32, direction: vec3<f32>, roughness: f32, diffuse: bool) -> vec3<f32> {
    let uv = cube_uv(direction);
    if diffuse {
        let pixel = vec2<f32>(800.0 + uv.z * 16.0, 128.0 + slot * 256.0) + clamp(uv.xy * 16.0, vec2<f32>(0.5), vec2<f32>(15.5));
        return textureSampleLevel(sky_texture, sky_sampler, pixel / vec2<f32>(1024.0, 1536.0), 0.0).rgb;
    }
    let level = clamp(roughness, 0.0, 1.0) * 7.0;
    let low = floor(level); let high = min(low + 1.0, 7.0);
    let lo_size = 128.0 / exp2(low); let hi_size = 128.0 / exp2(high);
    let lo = vec2<f32>(uv.z * lo_size, 128.0 + slot * 256.0 + 256.0 - lo_size * 2.0) + clamp(uv.xy * lo_size, vec2<f32>(0.5), vec2<f32>(lo_size - 0.5));
    let hi = vec2<f32>(uv.z * hi_size, 128.0 + slot * 256.0 + 256.0 - hi_size * 2.0) + clamp(uv.xy * hi_size, vec2<f32>(0.5), vec2<f32>(hi_size - 0.5));
    return mix(textureSampleLevel(sky_texture, sky_sampler, lo / vec2<f32>(1024.0, 1536.0), 0.0).rgb,
        textureSampleLevel(sky_texture, sky_sampler, hi / vec2<f32>(1024.0, 1536.0), 0.0).rgb, fract(level));
}
fn probe_weight(p: ReflectionVolume, world: vec3<f32>) -> f32 {
    if p.center.w <= 0.0 { return 0.0; }
    let edge = p.size.xyz - abs(world - p.center.xyz);
    let distance = min(edge.x, min(edge.y, edge.z));
    if distance <= 0.0 { return 0.0; }
    return clamp(distance / max(p.size.w, 0.00001), 0.0, 1.0);
}
fn probe_direction(p: ReflectionVolume, world: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    if p.params.y == 0.0 { return ray; }
    let local = world - p.center.xyz;
    let safe_ray = select(vec3<f32>(0.000001), ray, abs(ray) > vec3<f32>(0.000001));
    let times = (select(-p.size.xyz, p.size.xyz, safe_ray > vec3<f32>(0.0)) - local) / safe_ray;
    let t = min(times.x, min(times.y, times.z));
    return local + ray * t;
}
// The opaque pass's screen-space reflection at this pixel (see ssr.wgsl):
// what the reflected ray hit on screen, and how much it counts. Zero
// elsewhere, and where the ray found nothing.
var<private> screen_reflection: vec4<f32> = vec4<f32>(0.0);
// Global illumination's bounced light at this pixel (see gi.wgsl): rgb, and
// how much it replaces the ambient light in a. Zero without it.
var<private> bounced_light: vec4<f32> = vec4<f32>(0.0);

fn environment_light(s: Surface, f0: vec3<f32>) -> vec3<f32> {
    var ray = reflect(-s.view, s.normal);
    if MATERIAL_LAYERS {
        ray = layered_reflection(s, ray);
    }
    // The two smallest volumes around this pixel: a room inside a bigger one
    // comes first. Each takes its fade's share of what is left; the sky gets the rest.
    var first = 4u; var second = 4u; var size1 = 1.0e30; var size2 = 1.0e30;
    for (var i = 0u; i < 4u; i++) {
        let p = frame.probes[i];
        if probe_weight(p, s.world) > 0.0 {
            let size = p.size.x * p.size.y * p.size.z;
            if size < size1 { second = first; size2 = size1; first = i; size1 = size; }
            else if size < size2 { second = i; size2 = size; }
        }
    }
    var remaining = 1.0;
    var diffuse = vec3<f32>(0.0);
    var specular = vec3<f32>(0.0);
    for (var j = 0u; j < 2u; j++) {
        let index = select(first, second, j == 1u);
        if index < 4u {
            let p = frame.probes[index];
            let share = remaining * probe_weight(p, s.world) * p.params.x;
            remaining *= 1.0 - probe_weight(p, s.world);
            diffuse += environment_sample(p.center.w, s.normal, 0.0, true) * share;
            specular += environment_sample(p.center.w, probe_direction(p, s.world, ray), s.roughness, false) * share;
        }
    }
    diffuse += environment_sample(0.0, s.normal, 0.0, true) * remaining;
    specular += environment_sample(0.0, ray, s.roughness, false) * remaining;
    // Bounced light replaces the ambient light where the probes cover this
    // point. Where it's darker (indoors), reflections of the sky darken too.
    if bounced_light.a > 0.0 {
        let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
        let dimmer = clamp(dot(bounced_light.rgb, luma) / max(dot(diffuse, luma), 1e-4), 0.0, 1.0);
        specular *= mix(1.0, dimmer, bounced_light.a);
        diffuse = mix(diffuse, bounced_light.rgb, bounced_light.a);
    }
    // What's on screen replaces the environment where the reflected ray hit
    // it, with filtered reflections on glossy surfaces.
    specular = mix(specular, screen_reflection.rgb, screen_reflection.a * (1.0 - smoothstep(0.4, 0.7, s.roughness)));
    let nv = max(dot(s.normal, s.view), 0.0);
    let uv = (vec2<f32>(800.0, 0.0) + clamp(vec2<f32>(nv, s.roughness) * 128.0, vec2<f32>(0.5), vec2<f32>(127.5))) / vec2<f32>(1024.0, 1536.0);
    let brdf = textureSampleLevel(sky_texture, sky_sampler, uv, 0.0).rg;
    if MATERIAL_LAYERS {
        return layered_ambient(s, diffuse, specular, brdf);
    }
    return (diffuse * s.albedo * (1.0 - s.metallic) + specular * (f0 * brdf.x + brdf.y)) * s.occlusion;
}

// The environment's light from `ray` for a surface of `roughness` at
// `world`: the reflection volumes around it, then the sky, as
// `environment_light` reflects (without screen-space reflections or
// global illumination). For a second reflection, a clear coat's.
fn environment_reflection(world: vec3<f32>, ray: vec3<f32>, roughness: f32) -> vec3<f32> {
    var remaining = 1.0;
    var specular = vec3<f32>(0.0);
    for (var i = 0u; i < 4u; i++) {
        let p = frame.probes[i];
        let weight = probe_weight(p, world);
        if weight > 0.0 {
            specular += environment_sample(p.center.w, probe_direction(p, world, ray), roughness, false) * remaining * weight * p.params.x;
            remaining *= 1.0 - weight;
        }
    }
    return specular + environment_sample(0.0, ray, roughness, false) * remaining;
}

// The split-sum lookup (scale, bias of f0) for `n_v` and `roughness`.
fn environment_brdf(n_v: f32, roughness: f32) -> vec2<f32> {
    let uv = (vec2<f32>(800.0, 0.0) + clamp(vec2<f32>(n_v, roughness) * 128.0, vec2<f32>(0.5), vec2<f32>(127.5))) / vec2<f32>(1024.0, 1536.0);
    return textureSampleLevel(sky_texture, sky_sampler, uv, 0.0).rg;
}

// The sun's shadow, for material shaders, screen effects and compute passes.
// Where `world` falls in shadow cascade `i`: the shadow map uv, the depth
// to compare, and how near the edge of the cascade's box it is (0 at the
// center, 1 at the edge, more outside).
fn cascade_coords(i: i32, world: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    // Looking up a little off the surface keeps it from shadowing itself.
    let offset = n * frame.shadow.z * frame.cascade_texel[i];
    let clip = frame.sun_view_proj[i] * vec4<f32>(world + offset, 1.0);
    let ndc = clip.xyz / clip.w;
    let edge = max(max(abs(ndc.x), abs(ndc.y)), select(0.0, 2.0, ndc.z > 1.0));
    return vec4<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z, edge);
}

// How much light cascade `i` lets through at `c` (from `cascade_coords`),
// softened by `Lighting::shadow_filter`: one bilinear lookup (2x2 texels),
// four half a texel apart (3x3), or nine a texel apart (4x4).
fn cascade_light(i: i32, c: vec4<f32>) -> f32 {
    let soft = frame.light_shadow.w;
    if soft < 1.5 {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, c.xy, i, c.z);
    }
    if soft < 2.5 {
        var four = 0.0;
        for (var y = -1; y <= 1; y += 2) {
            for (var x = -1; x <= 1; x += 2) {
                let at = c.xy + vec2<f32>(f32(x), f32(y)) * 0.5 * frame.shadow.y;
                four += textureSampleCompareLevel(shadow_map, shadow_sampler, at, i, c.z);
            }
        }
        return four / 4.0;
    }
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let at = c.xy + vec2<f32>(f32(x), f32(y)) * frame.shadow.y;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, at, i, c.z);
        }
    }
    return lit / 9.0;
}

// ---- contact-hardening sun shadows: begin ----
// (`ShadowFilter::ContactHardening`.) The GL backend can't load a texel of a
// depth texture, so it replaces this part, line for line, with a
// `contact_light` that filters as `Softest` (see `without_contact_shadows`).

// Point `k` of 16 on a spiral filling the unit disc evenly (a Vogel disc),
// turned by `turn` radians.
fn contact_disc(k: i32, turn: f32) -> vec2<f32> {
    let a = f32(k) * 2.39996323 + turn;
    return vec2<f32>(cos(a), sin(a)) * sqrt((f32(k) + 0.5) / 16.0);
}

// How much light cascade `i` lets through at `c` (from `cascade_coords`),
// with an edge as wide as the sun seen from the caster makes it, in world
// units: first the casters within `contact_search(i)` of the point and
// their mean distance from it (16 reads of the depths), then 16 lookups
// across the edge's width. `n` is the surface's normal: each read is
// compared with the depth the surface's plane has there, so a sloped
// surface doesn't shadow itself. `turn` turns the pattern (per pixel).
fn contact_light(i: i32, c: vec4<f32>, n: vec3<f32>, turn: f32) -> f32 {
    let m = frame.sun_view_proj[i];
    // The cascade's axes in the world, and its scales (orthographic).
    let ax = vec3<f32>(m[0].x, m[1].x, m[2].x);
    let ay = vec3<f32>(m[0].y, m[1].y, m[2].y);
    let az = vec3<f32>(m[0].z, m[1].z, m[2].z);
    let scale = vec3<f32>(length(ax), length(ay), length(az));
    // Shadow map uv per world unit along its x and y (uv's y points down).
    let to_uv = vec2<f32>(0.5 * scale.x, -0.5 * scale.y);
    // The depth (world units along the sunlight) the surface's plane gains
    // per world unit across the map; the depth grows away from the sun.
    let facing = max(dot(n, -az / scale.z), 0.25);
    let slope = vec2<f32>(dot(n, ax / scale.x), dot(n, ay / scale.y)) / facing * scale.z;
    let texel = frame.cascade_texel[i];
    let bias = 0.5 * texel * scale.z;
    let search = contact_search(i);
    let size = vec2<i32>(textureDimensions(shadow_map));
    var blockers = 0.0;
    var gap = 0.0;
    for (var k = 0; k < 16; k++) {
        let o = contact_disc(k, turn) * search;
        let at = clamp(vec2<i32>((c.xy + o * to_uv) * vec2<f32>(size)), vec2<i32>(0), size - 1);
        let depth = textureLoad(shadow_map, at, i, 0);
        let surface = c.z + dot(slope, o) - bias;
        if depth < surface {
            blockers += 1.0;
            gap += surface - depth;
        }
    }
    if blockers == 0.0 {
        return 1.0;
    }
    if blockers == 16.0 {
        // Casters all around, at most the edge's width away: in shadow.
        return 0.0;
    }
    // The casters' mean distance in world units gives the edge's width; no
    // narrower than `Softest`'s edge in this cascade, no wider than searched.
    let distance = gap / blockers / scale.z;
    let radius = clamp(0.5 * frame.sun_dir.w * distance, 1.5 * texel, search);
    var lit = 0.0;
    for (var k = 0; k < 16; k++) {
        let o = contact_disc(k, turn) * radius;
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, c.xy + o * to_uv, i, c.z + dot(slope, o) - bias);
    }
    return lit / 16.0;
}
// ---- contact-hardening sun shadows: end ----

// How far, in world units, contact-hardening shadows look for casters
// around a point in cascade `i`: half the edge a caster a quarter of the
// shadow distance away casts, and at least `Softest`'s edge there. The same
// in every cascade, so an edge keeps its width where two cascades meet.
fn contact_search(i: i32) -> f32 {
    return max(0.125 * frame.sun_dir.w * frame.shadow.w, 1.5 * frame.cascade_texel[i]);
}

// Whether the sun's shadows are contact-hardening this frame.
fn contact_hardening() -> bool {
    return frame.light_shadow.w > 3.5 && frame.sun_dir.w > 0.0;
}

// How the contact-hardening lookups turn at `world`: by a different angle
// at each pixel (interleaved gradient noise), and also each frame with
// temporal antialiasing on (its jitter is 0 without), so the antialiasing
// averages the pattern away; without it the pattern stays still.
fn contact_turn(world: vec3<f32>) -> f32 {
    let clip = frame.view_proj * vec4<f32>(world, 1.0);
    let pixel = floor((clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5) * frame.screen.xy);
    var per_frame = 0.0;
    if any(frame.jitter.xy != vec2<f32>(0.0)) {
        per_frame = fract(sin(dot(frame.jitter.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    }
    let noise = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
    return (noise + per_frame) * 6.28318531;
}

// How much light cascade `i` lets through at `c`, with this frame's filter.
fn sun_cascade_light(i: i32, c: vec4<f32>, n: vec3<f32>, turn: f32, contact: bool) -> f32 {
    if contact {
        return contact_light(i, c, n, turn);
    }
    return cascade_light(i, c);
}

// How much sun reaches `world` (0 in shadow, 1 lit). The first cascade
// whose box holds the point gives the sharpest shadow; near that box's edge
// it fades into the next cascade, so the step between them doesn't show.
// Only the cascades used are sampled.
fn sunlight(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let count = i32(frame.shadow.x);
    // Towards the shadow distance, shadows fade out.
    let fade = smoothstep(0.85, 1.0, distance(world, frame.camera.xyz) / frame.shadow.w);
    let clouds = cloud_light(world);
    if count == 0 || fade >= 1.0 {
        return clouds;
    }
    let contact = contact_hardening();
    var turn = 0.0;
    if contact {
        turn = contact_turn(world);
    }
    var lit = 1.0;
    for (var i = 0; i < count; i++) {
        let c = cascade_coords(i, world, n);
        // The 3x3 filter reaches one texel further out than the point;
        // contact-hardening lookups as far as they search, and a texel.
        var inside = 1.0 - 2.0 * frame.shadow.y;
        if contact {
            let m = frame.sun_view_proj[i];
            inside -= contact_search(i) * length(vec3<f32>(m[0].x, m[1].x, m[2].x));
        }
        if c.w < inside {
            lit = sun_cascade_light(i, c, n, turn, contact);
            let blend = smoothstep(0.85, 1.0, c.w / inside);
            if blend > 0.0 && i + 1 < count {
                lit = mix(lit, sun_cascade_light(i + 1, cascade_coords(i + 1, world, n), n, turn, contact), blend);
            }
            break;
        }
    }
    return mix(lit, 1.0, fade) * clouds;
}

// Where the sky kit's cloud shadow map is in the IBL atlas (`sky_texture`):
// a 128 x 128 square at the start of the rows below the probes' maps.
const CLOUD_SHADOW_ORIGIN: vec2<f32> = vec2<f32>(0.0, 1408.0);
const CLOUD_SHADOW_SIZE: f32 = 128.0;

// How much sunlight the sky kit's clouds let through to `world`: 1 without
// the kit or its cloud shadows, less under a cloud. The map holds what gets
// through each point of the cloud layer's base along the sun; `world` is
// carried up to that base along the sun to look it up. Outside the map
// (kilometers away) the shadows fade out.
fn cloud_light(world: vec3<f32>) -> f32 {
    let strength = frame.sky_kit[4].w;
    let up = frame.sun_dir.y;
    if strength <= 0.0 || up <= 0.02 || frame.camera.w == 0.0 {
        return 1.0;
    }
    let base = frame.sky_kit[2].x;
    let at = world.xz + frame.sun_dir.xz * (max(base - world.y, 0.0) / up);
    let uv = (at - frame.sky_kit[4].xy) * frame.sky_kit[4].z + 0.5;
    let edge = max(abs(uv.x - 0.5), abs(uv.y - 0.5));
    let texel = clamp(uv * CLOUD_SHADOW_SIZE, vec2<f32>(0.5), vec2<f32>(CLOUD_SHADOW_SIZE - 0.5));
    let through = textureSampleLevel(sky_texture, sky_sampler, (CLOUD_SHADOW_ORIGIN + texel) / vec2<f32>(1024.0, 1536.0), 0.0).r;
    let reach = 1.0 - smoothstep(0.4, 0.5, edge);
    // Toward the horizon the sun's light comes through more air than
    // cloud: the shadows soften away as it sets.
    let low = smoothstep(0.02, 0.15, up);
    return mix(1.0, clamp(through, 0.0, 1.0), strength * reach * low);
}

// ---- Material layers ----
// Layers over the metallic-roughness surface, as glTF's material
// extensions describe them: clear coat, sheen, specular strength and color
// with the index of refraction, anisotropy and iridescence
// (`MaterialLayers`). `MATERIAL_LAYERS` is false in every shader but the
// layered build of the built-in one (see `compose_material`): a constant,
// so the others compile none of the code it guards.
const MATERIAL_LAYERS: bool = false;

// Which layers are on (`material.layer_info.x`).
const LAYER_CLEARCOAT: u32 = 1u;
const LAYER_SHEEN: u32 = 2u;
const LAYER_ANISOTROPY: u32 = 8u;
const LAYER_IRIDESCENCE: u32 = 16u;
const LAYER_TRANSMISSION: u32 = 32u;

// The layer maps, in `material.layer_info.y`'s order (two bits each: 0
// none, 1 or 2 the slot it is bound in).
const MAP_CLEARCOAT: u32 = 0u;
const MAP_CLEARCOAT_ROUGHNESS: u32 = 1u;
const MAP_CLEARCOAT_NORMAL: u32 = 2u;
const MAP_SHEEN_COLOR: u32 = 3u;
const MAP_SHEEN_ROUGHNESS: u32 = 4u;
const MAP_SPECULAR: u32 = 5u;
const MAP_SPECULAR_COLOR: u32 = 6u;
const MAP_ANISOTROPY: u32 = 7u;
const MAP_IRIDESCENCE: u32 = 8u;
const MAP_IRIDESCENCE_THICKNESS: u32 = 9u;

// A pixel's layers, from the material and its maps (see `fill_layers`).
struct Layers {
    on: u32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    // Unit, world space: the coat's own normal map, else the mesh's.
    clearcoat_normal: vec3<f32>,
    sheen: vec3<f32>,
    sheen_roughness: f32,
    // A non-metal's reflectance head-on, and how strong its reflection is.
    f0: vec3<f32>,
    specular: f32,
    // 0 to 1, along `bitangent` across `tangent` (unit, world space).
    anisotropy: f32,
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
    iridescence: f32,
    // The thin film's reflectance at this view, as metal or non-metal.
    film: vec3<f32>,
};

var<private> layers: Layers;
// The two layer images at this pixel (white where not bound).
var<private> layer_texel0: vec4<f32> = vec4<f32>(1.0);
var<private> layer_texel1: vec4<f32> = vec4<f32>(1.0);
var<private> layer_uv: vec2<f32>;
var<private> layer_uv1: vec2<f32>;

// Map `kind`'s texel, or `otherwise` without one.
fn layer_map(kind: u32, otherwise: vec4<f32>) -> vec4<f32> {
    let slot = (material.layer_info.y >> (2u * kind)) & 3u;
    if MATERIAL_MULTI_UV {
        let uv = map_uv(layer_uv, layer_uv1, UV_LAYER_START + kind);
        if slot == 1u {
            return textureSampleBias(custom_texture0, material_sampler, uv, texture_mip_bias());
        }
        if slot == 2u {
            return textureSampleBias(custom_texture1, material_sampler, uv, texture_mip_bias());
        }
        return otherwise;
    }
    if slot == 1u {
        return layer_texel0;
    }
    if slot == 2u {
        return layer_texel1;
    }
    return otherwise;
}

fn has_layer_map(kind: u32) -> bool {
    return ((material.layer_info.y >> (2u * kind)) & 3u) != 0u;
}

fn layer_max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

// The texture directions at this pixel (Schüler's cotangent frame).
fn layer_frame(uv: vec2<f32>, world: vec3<f32>, geometric: vec3<f32>) -> mat2x3<f32> {
    let dp1 = dpdx(world);
    let dp2 = -dpdy(world);
    let duv1 = dpdx(uv);
    let duv2 = -dpdy(uv);
    let dp2perp = cross(dp2, geometric);
    let dp1perp = cross(geometric, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
    return mat2x3<f32>(t * inv, b * inv);
}

// Fills `layers` in uniform control flow (screen derivatives).
fn fill_layers(s: Surface, uv: vec2<f32>, uv1: vec2<f32>, world: vec3<f32>, geometric: vec3<f32>, transformed_normal: vec3<f32>) {
    let info = material.layer_info;
    if MATERIAL_MULTI_UV {
        layer_uv = uv;
        layer_uv1 = uv1;
    } else {
        if (info.z & 1u) != 0u {
            layer_texel0 = textureSampleBias(custom_texture0, material_sampler, uv, texture_mip_bias());
        }
        if (info.z & 2u) != 0u {
            layer_texel1 = textureSampleBias(custom_texture1, material_sampler, uv, texture_mip_bias());
        }
    }
    var anisotropy_uv = uv;
    if MATERIAL_MULTI_UV {
        anisotropy_uv = select(s.uv, map_uv(uv, uv1, UV_LAYER_START + MAP_ANISOTROPY), has_layer_map(MAP_ANISOTROPY));
    }
    let basis = layer_frame(anisotropy_uv, world, geometric);
    let t = basis[0];
    let b = basis[1];
    var l: Layers;
    l.on = info.x;
    let values = material.layers;
    let n_v = max(dot(s.normal, s.view), 1e-4);

    // Clear coat: amount in red, roughness in green.
    l.clearcoat = values[0].x * layer_map(MAP_CLEARCOAT, vec4<f32>(1.0)).r;
    l.clearcoat_roughness = clamp(values[0].y * layer_map(MAP_CLEARCOAT_ROUGHNESS, vec4<f32>(1.0)).g, 0.03, 1.0);
    l.clearcoat_normal = geometric;
    if has_layer_map(MAP_CLEARCOAT_NORMAL) {
        var m = layer_map(MAP_CLEARCOAT_NORMAL, vec4<f32>(0.5, 0.5, 1.0, 1.0)).xyz * 2.0 - 1.0;
        m = vec3<f32>(m.xy * values[0].z, m.z);
        // glTF: +Y up the image, along -v (see `surface_at`).
        var coat_basis = basis;
        if MATERIAL_MULTI_UV {
            let coat_uv = map_uv(uv, uv1, UV_LAYER_START + MAP_CLEARCOAT_NORMAL);
            coat_basis = layer_frame(coat_uv, world, geometric);
        }
        l.clearcoat_normal = normalize(coat_basis[0] * m.x - coat_basis[1] * m.y + geometric * m.z);
    }

    // Sheen: color in red, green and blue (read as sRGB), roughness in alpha.
    l.sheen = values[1].rgb * layer_map(MAP_SHEEN_COLOR, vec4<f32>(1.0)).rgb;
    l.sheen_roughness = clamp(values[1].w * layer_map(MAP_SHEEN_ROUGHNESS, vec4<f32>(1.0)).a, 0.0, 1.0);

    // Specular: color in red, green and blue (sRGB), strength in alpha;
    // the reflectance head-on from the index of refraction.
    let ior = values[3].x;
    let reflectance = (ior - 1.0) / (ior + 1.0);
    l.f0 = min(reflectance * reflectance * values[2].rgb * layer_map(MAP_SPECULAR_COLOR, vec4<f32>(1.0)).rgb, vec3<f32>(1.0));
    l.specular = values[2].w * layer_map(MAP_SPECULAR, vec4<f32>(1.0)).a;

    // Anisotropy: a direction in the texture's directions (red, green:
    // -1 to 1, +Y up the image), turned by the rotation, and a strength
    // (blue). Negative strengths stretch across the direction.
    l.anisotropy = 0.0;
    l.tangent = t;
    l.bitangent = b;
    if (info.x & LAYER_ANISOTROPY) != 0u {
        let texel = layer_map(MAP_ANISOTROPY, vec4<f32>(1.0, 0.5, 1.0, 1.0));
        let d = texel.rg * 2.0 - 1.0;
        let c = values[3].z;
        let sn = values[3].w;
        let turned = vec2<f32>(c * d.x - sn * d.y, sn * d.x + c * d.y);
        var strength = values[3].y * texel.b;
        let along = t * turned.x - b * turned.y;
        var tangent = normalize(along - s.normal * dot(s.normal, along) + vec3<f32>(1e-6, 0.0, 0.0));
        var bitangent = cross(s.normal, tangent);
        if strength < 0.0 {
            let swap = tangent;
            tangent = bitangent;
            bitangent = swap;
            strength = -strength;
        }
        l.anisotropy = clamp(strength, 0.0, 1.0);
        l.tangent = tangent;
        l.bitangent = bitangent;
    }

    // Iridescence: amount in red, thickness (between the two) in green.
    l.iridescence = 0.0;
    l.film = vec3<f32>(0.0);
    if (info.x & LAYER_IRIDESCENCE) != 0u {
        l.iridescence = clamp(values[4].x * layer_map(MAP_IRIDESCENCE, vec4<f32>(1.0)).r, 0.0, 1.0);
        let thickness = mix(values[4].z, values[4].w, layer_map(MAP_IRIDESCENCE_THICKNESS, vec4<f32>(1.0)).g);
        let base = mix(l.f0, s.albedo, s.metallic);
        l.film = thin_film(1.0, values[4].y, n_v, thickness, base);
    }
    layers = l;
    // `info`, not `l`: `l` holds screen derivatives, so a browser's
    // uniformity analysis counts a branch on any of it as non-uniform, and
    // the layer maps sample inside.
    if (info.x & LAYER_TRANSMISSION) != 0u {
        fill_transmission(transformed_normal);
    }
}

// ---- Direct light ----

// GGX with the height-correlated Smith visibility, stretched to roughness
// `at` along `layers.tangent` and `ab` along `layers.bitangent`: the
// specular term D * V (V holds the 1 / (4 n.l n.v)).
fn anisotropic_ggx(l: vec3<f32>, v: vec3<f32>, n: vec3<f32>, h: vec3<f32>, at: f32, ab: f32) -> f32 {
    let t = layers.tangent;
    let b = layers.bitangent;
    let n_l = max(dot(n, l), 1e-4);
    let n_v = max(dot(n, v), 1e-4);
    let a2 = at * ab;
    let f = vec3<f32>(ab * dot(t, h), at * dot(b, h), a2 * max(dot(n, h), 0.0));
    let w2 = a2 / max(dot(f, f), 1e-12);
    let d = a2 * w2 * w2 / PI;
    let gv = n_l * length(vec3<f32>(at * dot(t, v), ab * dot(b, v), n_v));
    let gl = n_v * length(vec3<f32>(at * dot(t, l), ab * dot(b, l), n_l));
    return d * clamp(0.5 / max(gv + gl, 1e-6), 0.0, 1.0);
}

// The specular term D * G / (4 n.l n.v) of GGX at `roughness`, as
// `direct_light` has it.
fn ggx_specular(n_l: f32, n_v: f32, n_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d_denom = n_h * n_h * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * d_denom * d_denom);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let g = n_v / (n_v * (1.0 - k) + k) * n_l / (n_l * (1.0 - k) + k);
    return d * g / max(4.0 * n_v * n_l, 1e-4);
}

// The sheen's microfiber distribution (Estevez and Kulla's "Charlie").
fn sheen_distribution(roughness: f32, n_h: f32) -> f32 {
    let inv = 1.0 / max(roughness * roughness, 1e-6);
    let sin2 = max(1.0 - n_h * n_h, 0.0);
    return (2.0 + inv) * pow(sin2, inv * 0.5) / (2.0 * PI);
}

fn sheen_lambda_fit(x: f32, a: f32) -> f32 {
    let w = (1.0 - a) * (1.0 - a);
    let p = mix(21.5473, 25.3245, w);
    let q = mix(3.82987, 3.32435, w);
    let r = mix(0.19823, 0.16801, w);
    let s = mix(-1.97760, -1.27393, w);
    let u = mix(-4.32054, -4.85967, w);
    return p / (1.0 + q * pow(x, r)) + s * x + u;
}

fn sheen_lambda(c: f32, a: f32) -> f32 {
    if c < 0.5 {
        return exp(sheen_lambda_fit(c, a));
    }
    return exp(2.0 * sheen_lambda_fit(0.5, a) - sheen_lambda_fit(1.0 - c, a));
}

// The sheen's visibility (Estevez and Kulla), with the 1 / (4 n.l n.v).
fn sheen_visibility(n_l: f32, n_v: f32, roughness: f32) -> f32 {
    let a = max(roughness * roughness, 1e-6);
    return clamp(1.0 / ((1.0 + sheen_lambda(n_v, a) + sheen_lambda(n_l, a)) * (4.0 * n_v * n_l)), 0.0, 1.0);
}

// How much of the light coming from `mu` (its cosine to the normal) a
// white sheen of `roughness` reflects, over every direction: what it takes
// from the layer under it. A fit (RMS error 0.02) of the integral of the
// two functions above, made for this shader, which has no texture slot to
// spare for a table.
fn sheen_albedo(mu: f32, roughness: f32) -> f32 {
    let r = clamp(roughness, 0.05, 1.0);
    let a = 1.4924 + r * (-2.9561 + r * 2.3887);
    let b = 11.3448 + r * (-22.1403 + r * 12.57);
    let c = -0.1429 + r * (1.1099 - r * 1.0403);
    return clamp(a * exp(-b * mu) + c * (1.0 - mu), 0.0, 1.0);
}

// The light a layered surface reflects toward the viewer from a light in
// direction `l` bringing `radiance`: `direct_light` for the layered build.
fn layered_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let n = s.normal;
    let v = s.view;
    let h = (l + v) / max(length(l + v), 1e-6);
    let n_l = max(dot(n, l), 0.0);
    let n_v = max(dot(n, v), 1e-4);
    // At most 1, as in `direct_light`.
    let v_h = clamp(dot(v, h), 0.0, 1.0);
    let schlick = pow(1.0 - v_h, 5.0);
    var color = vec3<f32>(0.0);
    if transmission.diffuse > 0.0 || transmission.scatter > 0.0 {
        let exit_fresnel = pow(1.0 - clamp(n_v, 0.0, 1.0), 5.0);
        let reflected = layers.specular * layer_max3(layers.f0 + (1.0 - layers.f0) * exit_fresnel);
        color = transmitted_light(s, l, reflected);
    }
    if n_l > 0.0 {
        var specular = 0.0;
        if (layers.on & LAYER_ANISOTROPY) != 0u {
            let a = s.roughness * s.roughness;
            let at = mix(a, 1.0, layers.anisotropy * layers.anisotropy);
            specular = anisotropic_ggx(l, v, n, h, at, clamp(a, 0.001, 1.0));
        } else {
            specular = ggx_specular(n_l, n_v, max(dot(n, h), 0.0), s.roughness);
        }
        var dielectric_f = layers.f0 + (vec3<f32>(1.0) - layers.f0) * schlick;
        var metal_f = s.albedo + (vec3<f32>(1.0) - s.albedo) * schlick;
        if (layers.on & LAYER_IRIDESCENCE) != 0u {
            dielectric_f = mix(dielectric_f, layers.film, layers.iridescence);
            metal_f = mix(metal_f, layers.film, layers.iridescence);
        }
        // A non-metal: diffuse under a reflection as strong as `specular`.
        let diffuse_share = (1.0 - transmission.factor) * (1.0 - transmission.diffuse);
        let dielectric = diffuse_share * s.albedo / PI * (1.0 - layers.specular * layer_max3(dielectric_f))
            + dielectric_f * (layers.specular * specular);
        var base = mix(dielectric, metal_f * specular, s.metallic);
        if (layers.on & LAYER_SHEEN) != 0u {
            let strongest = layer_max3(layers.sheen);
            let r = layers.sheen_roughness;
            let under = min(1.0 - strongest * sheen_albedo(n_v, r), 1.0 - strongest * sheen_albedo(n_l, r));
            base = base * under + layers.sheen * (sheen_distribution(r, max(dot(n, h), 0.0)) * sheen_visibility(n_l, n_v, r));
        }
        color += base * n_l;
    }
    if (layers.on & LAYER_CLEARCOAT) != 0u {
        let c = layers.clearcoat_normal;
        let c_v = max(dot(c, v), 1e-4);
        let c_l = max(dot(c, l), 0.0);
        // What the coat reflects head-on (4%: an index of refraction of 1.5)
        // it takes from the light reaching the base.
        let coat_f = 0.04 + 0.96 * pow(1.0 - c_v, 5.0);
        color *= 1.0 - layers.clearcoat * coat_f;
        if c_l > 0.0 {
            let f = 0.04 + 0.96 * schlick;
            color += layers.clearcoat * f * ggx_specular(c_l, c_v, max(dot(c, h), 0.0), layers.clearcoat_roughness) * c_l;
        }
    }
    return color * radiance;
}

// ---- Environment light ----

// The direction a layered surface reflects the environment from: for an
// anisotropic one, the normal bent toward the stretch (as the glTF Sample
// Viewer and Filament do), so reflections smear along it.
fn layered_reflection(s: Surface, ray: vec3<f32>) -> vec3<f32> {
    if (layers.on & LAYER_ANISOTROPY) == 0u || layers.anisotropy <= 0.0 {
        return ray;
    }
    let along = layers.bitangent;
    let across = cross(along, s.view);
    let bent = cross(across, along);
    let bend = 1.0 - layers.anisotropy * (1.0 - s.roughness);
    let bend4 = bend * bend * bend * bend;
    let n = normalize(mix(bent, s.normal, bend4));
    return reflect(-s.view, n);
}

// The environment's light on a layered surface: `environment_light`'s end
// for the layered build. `diffuse` and `specular` are the environment's
// light around the normal and from the reflection, `brdf` the split-sum
// lookup at the view.
fn layered_ambient(s: Surface, diffuse: vec3<f32>, specular: vec3<f32>, brdf: vec2<f32>) -> vec3<f32> {
    let n_v = max(dot(s.normal, s.view), 1e-4);
    var f0 = layers.f0;
    var metal = s.albedo;
    if (layers.on & LAYER_IRIDESCENCE) != 0u {
        f0 = mix(f0, layers.film, layers.iridescence);
        metal = mix(metal, layers.film, layers.iridescence);
    }
    let dielectric_f = f0 * brdf.x + brdf.y;
    let through = (1.0 - transmission.factor) * transmission.diffuse;
    let diffuse_share = (1.0 - transmission.factor) * (1.0 - transmission.diffuse);
    var transmitted = vec3<f32>(0.0);
    if through > 0.0 || transmission.scatter > 0.0 {
        let behind = environment_sample(0.0, -s.normal, 0.0, true);
        let clear = volume_transmittance(transmission.thickness);
        let thin = transmission.thickness <= 0.0;
        let scattered = mix(diffuse, behind, 0.5 + select(0.0, 0.5 * transmission.anisotropy, thin));
        transmitted = through * behind * transmission.diffuse_color * clear
            + transmission.scatter * transmission.scatter_color * scatter_extinction() * scattered;
    }
    let dielectric = (diffuse_share * diffuse * s.albedo + transmitted) *
        (1.0 - layers.specular * layer_max3(dielectric_f)) + specular * dielectric_f * layers.specular;
    var base = mix(dielectric, specular * (metal * brdf.x + brdf.y), s.metallic);
    if (layers.on & LAYER_SHEEN) != 0u {
        // The sheen's lobe is wide: lit by the light around the normal.
        let e = sheen_albedo(n_v, layers.sheen_roughness);
        base = base * (1.0 - layer_max3(layers.sheen) * e) + layers.sheen * diffuse * e;
    }
    base *= s.occlusion;
    if (layers.on & LAYER_CLEARCOAT) != 0u {
        let c = layers.clearcoat_normal;
        let c_v = max(dot(c, s.view), 1e-4);
        let coat_f = 0.04 + 0.96 * pow(1.0 - c_v, 5.0);
        let lookup = environment_brdf(c_v, layers.clearcoat_roughness);
        let coat = environment_reflection(s.world, reflect(-s.view, c), layers.clearcoat_roughness) * (0.04 * lookup.x + lookup.y);
        base = base * (1.0 - layers.clearcoat * coat_f) + coat * (layers.clearcoat * s.occlusion);
    }
    return base;
}

// ---- Iridescence ----
// A thin film over the surface (Belcour and Barla, "A Practical Extension
// to Microfacet Theory for the Modeling of Varying Iridescence", 2017, as
// the glTF Sample Viewer evaluates it).

fn film_square(x: f32) -> f32 {
    return x * x;
}

fn film_f0(transmitted: f32, incident: f32) -> f32 {
    return film_square((transmitted - incident) / (transmitted + incident));
}

fn film_f0_rgb(transmitted: vec3<f32>, incident: f32) -> vec3<f32> {
    let r = (transmitted - vec3<f32>(incident)) / (transmitted + vec3<f32>(incident));
    return r * r;
}

// The XYZ color matching functions' Fourier transform at optical path
// difference `opd` (nanometers) and phase `shift`, in linear sRGB.
fn film_sensitivity(opd: f32, shift: vec3<f32>) -> vec3<f32> {
    let phase = 2.0 * PI * opd * 1.0e-9;
    let val = vec3<f32>(5.4856e-13, 4.4201e-13, 5.2481e-13);
    let pos = vec3<f32>(1.6810e+06, 1.7953e+06, 2.2084e+06);
    let spread = vec3<f32>(4.3278e+09, 9.3046e+09, 6.6121e+09);
    var xyz = val * sqrt(2.0 * PI * spread) * cos(pos * phase + shift) * exp(-phase * phase * spread);
    xyz.x += 9.7470e-14 * sqrt(2.0 * PI * 4.5282e+09) * cos(2.2399e+06 * phase + shift.x) * exp(-4.5282e+09 * phase * phase);
    xyz /= 1.0685e-7;
    let to_rgb = mat3x3<f32>(
        3.2404542, -0.9692660, 0.0556434,
        -1.5371385, 1.8760108, -0.2040259,
        -0.4985314, 0.0415560, 1.0572252,
    );
    return to_rgb * xyz;
}

// The reflectance of a film of index `film_ior` and `thickness` (nm), over
// a base of reflectance `base_f0`, seen at `cos1` from a medium of index
// `outside`.
fn thin_film(outside: f32, film_ior: f32, cos1: f32, thickness: f32, base_f0: vec3<f32>) -> vec3<f32> {
    // A film thinning to nothing turns into the medium outside.
    let ior = mix(outside, film_ior, smoothstep(0.0, 0.03, thickness));
    let sin2_sq = film_square(outside / ior) * (1.0 - cos1 * cos1);
    let cos2_sq = 1.0 - sin2_sq;
    if cos2_sq < 0.0 {
        // Total internal reflection.
        return vec3<f32>(1.0);
    }
    let cos2 = sqrt(cos2_sq);
    // The first interface.
    let r0 = film_f0(ior, outside);
    let r12 = r0 + (1.0 - r0) * pow(1.0 - cos1, 5.0);
    let t121 = 1.0 - r12;
    let phi12 = select(0.0, PI, ior < outside);
    let phi21 = PI - phi12;
    // The second interface.
    let root = sqrt(clamp(base_f0, vec3<f32>(0.0), vec3<f32>(0.9999)));
    let base_ior = (vec3<f32>(1.0) + root) / (vec3<f32>(1.0) - root);
    let r1 = film_f0_rgb(base_ior, ior);
    let r23 = r1 + (vec3<f32>(1.0) - r1) * pow(1.0 - cos2, 5.0);
    let phi23 = select(vec3<f32>(0.0), vec3<f32>(PI), base_ior < vec3<f32>(ior));
    // The phase shift.
    let opd = 2.0 * ior * thickness * cos2;
    let phi = vec3<f32>(phi21) + phi23;
    // The compound terms.
    let r123 = clamp(r12 * r23, vec3<f32>(1e-5), vec3<f32>(0.9999));
    let root123 = sqrt(r123);
    let rs = t121 * t121 * r23 / (vec3<f32>(1.0) - r123);
    // The reflectance for m = 0 (the DC term), then m = 1 and 2.
    var reflectance = vec3<f32>(r12) + rs;
    var cm = rs - vec3<f32>(t121);
    for (var m = 1; m <= 2; m++) {
        cm *= root123;
        reflectance += cm * 2.0 * film_sensitivity(f32(m) * opd, f32(m) * phi);
    }
    // Colors out of gamut can come out negative.
    return max(reflectance, vec3<f32>(0.0));
}

// Transmission is evaluated by the layered material only. These values are
// filled alongside the other material layers, before any lighting branches.
const MAP_TRANSMISSION: u32 = 10u;
const MAP_THICKNESS: u32 = 11u;
const MAP_DIFFUSE_TRANSMISSION_COLOR: u32 = 12u;
const MAP_DIFFUSE_TRANSMISSION: u32 = 13u;
const MAP_SCATTER_COLOR: u32 = 14u;
const MAP_SCATTER: u32 = 15u;

struct Transmission {
    factor: f32,
    thickness: f32,
    distance: f32,
    attenuation: vec3<f32>,
    dispersion: f32,
    diffuse: f32,
    diffuse_color: vec3<f32>,
    scatter: f32,
    scatter_color: vec3<f32>,
    anisotropy: f32,
};
var<private> transmission: Transmission;

fn fill_transmission(transformed_normal: vec3<f32>) {
    let v = material.layers;
    transmission.factor = v[5].x * layer_map(MAP_TRANSMISSION, vec4<f32>(1.0)).r;
    // Its reciprocal length estimates world units per mesh unit along
    // the normal (exact for uniform model scaling).
    let scale = 1.0 / max(length(transformed_normal), 1e-6);
    transmission.thickness = v[5].y * layer_map(MAP_THICKNESS, vec4<f32>(1.0)).g * scale;
    transmission.distance = v[5].z;
    transmission.dispersion = v[5].w;
    transmission.attenuation = v[6].rgb;
    transmission.diffuse = v[6].a * layer_map(MAP_DIFFUSE_TRANSMISSION, vec4<f32>(1.0)).a;
    transmission.diffuse_color = v[7].rgb * layer_map(MAP_DIFFUSE_TRANSMISSION_COLOR, vec4<f32>(1.0)).rgb;
    transmission.anisotropy = v[7].a;
    transmission.scatter_color = v[8].rgb * layer_map(MAP_SCATTER_COLOR, vec4<f32>(1.0)).rgb;
    transmission.scatter = transmission.factor * v[8].a * layer_map(MAP_SCATTER, vec4<f32>(1.0)).a;
}

// Linear radiance at the entrance of the medium after it traveled a world
// distance. An infinite attenuation distance is encoded as zero.
fn volume_transmittance(travel: f32) -> vec3<f32> {
    if transmission.distance <= 0.0 || travel <= 0.0 {
        return vec3<f32>(1.0);
    }
    return pow(transmission.attenuation, vec3<f32>(travel / transmission.distance));
}

// Some draft scatter materials give a thickness and color but no finite
// attenuation distance. Give their authored scatter lobe a half share of
// transmission in that case, with matching loss from the clear path.
fn scatter_without_density() -> bool {
    return transmission.thickness > 0.0 && transmission.distance <= 0.0
        && layer_max3(transmission.scatter_color) > 0.0;
}

fn scatter_extinction() -> vec3<f32> {
    if transmission.thickness <= 0.0 {
        return vec3<f32>(1.0);
    }
    if scatter_without_density() {
        return vec3<f32>(0.5);
    }
    return 1.0 - volume_transmittance(transmission.thickness);
}

fn refraction_level(uv: vec2<f32>, level: i32) -> vec3<f32> {
    let center = 0.5 / vec2<f32>(textureDimensions(scene_color_texture, level));
    return textureSampleLevel(scene_color_texture, sky_sampler, clamp(uv, center, 1.0 - center), f32(level)).rgb;
}

// Bilinear within each mip, with an explicit blend between mips (the
// environment sampler has nearest mip filtering). WebGL uses level zero.
fn refraction_color(uv: vec2<f32>, roughness: f32) -> vec3<f32> {
    let last = f32(textureNumLevels(scene_color_texture) - 1u);
    let lod = clamp(roughness * roughness * last, 0.0, last);
    let low = i32(lod);
    var color = refraction_level(uv, low);
    if fract(lod) > 0.0 {
        color = mix(color, refraction_level(uv, low + 1), fract(lod));
    }
    return color;
}

// Project the ray's estimated exit from the volume. A wall of zero
// thickness keeps the incident direction and samples at the surface.
fn refracted_radiance(s: Surface, travel: f32, ior: f32) -> vec3<f32> {
    let ray = refract(-s.view, s.normal, 1.0 / ior);
    let projection = frame.view_proj * vec4<f32>(s.world + ray * travel, 1.0);
    let uv = projection.xy / max(projection.w, 1e-6) * vec2<f32>(0.5, -0.5) + 0.5;
    // Approximate the transmission lobe narrowing as IOR approaches 1.
    let roughness = s.roughness * clamp(2.0 * (ior - 1.0), 0.0, 1.0);
    let size = vec2<f32>(textureDimensions(scene_color_texture));
    if all(size == vec2<f32>(1.0)) || projection.w <= 0.0
        || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return environment_reflection(s.world, ray, roughness);
    }
    if travel > 0.0 && scene_distance(uv * size) + 0.001 < distance(s.world, frame.camera.xyz) {
        // A foreground object is not behind the glass.
        return refraction_color(s.screen / size, roughness);
    }
    return refraction_color(uv, roughness);
}

// Direct backlighting and a broad scatter lobe. Local extinction is
// represented by the authored thickness, rather than a second light march.
fn transmitted_light(s: Surface, l: vec3<f32>, reflected: f32) -> vec3<f32> {
    let back = max(-dot(s.normal, l), 0.0);
    let scatter = transmission.scatter;
    var diffuse = transmission.diffuse * (1.0 - transmission.factor) *
        transmission.diffuse_color * back * volume_transmittance(transmission.thickness);
    if scatter > 0.0 {
        let facing = dot(s.normal, l);
        if transmission.thickness <= 0.0 {
            let through = 0.5 + 0.5 * transmission.anisotropy;
            diffuse += transmission.scatter_color * scatter * mix(max(facing, 0.0), back, through);
        } else {
            // A broad single surface lobe approximates light redistributed
            // by the medium. Keep it finite at the ends of the phase range.
            let g = transmission.anisotropy * 0.75;
            let cosine = clamp(dot(-l, s.view), -1.0, 1.0);
            let phase = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * cosine, 1.5);
            let extinguished = scatter_extinction();
            diffuse += extinguished * transmission.scatter_color * scatter * (0.25 + 0.75 * back) * phase * 0.25;
        }
    }
    return diffuse * (1.0 - reflected) * (1.0 - s.metallic) / PI;
}

// This is added to shading, whose diffuse lobe already relinquished the
// transmitted energy. Base color tints the light at the boundary, and
// Beer-Lambert absorption tints it along the ray inside.
fn specular_transmission(s: Surface) -> vec3<f32> {
    let travel = transmission.thickness;
    var scattered = select(transmission.scatter, 0.0, travel > 0.0);
    if scatter_without_density() {
        scattered = transmission.scatter * 0.5;
    }
    let amount = (transmission.factor - scattered) * (1.0 - s.metallic);
    if amount <= 0.0 {
        return vec3<f32>(0.0);
    }
    let ior = material.layers[3].x;
    var rgb = refracted_radiance(s, travel, ior);
    if travel > 0.0 && transmission.dispersion > 0.0 {
        // Two more lookups for the red and blue rays, with green at the
        // material's IOR. A zero dispersion only samples once.
        let spread = (ior - 1.0) * 0.025 * transmission.dispersion;
        let red = refracted_radiance(s, travel, max(1.0, ior - spread));
        let blue = refracted_radiance(s, travel, ior + spread);
        rgb = vec3<f32>(red.r, rgb.g, blue.b);
    }
    let n_v = clamp(dot(s.normal, s.view), 0.0, 1.0);
    let lookup = environment_brdf(n_v, s.roughness);
    var f0 = layers.f0;
    if (layers.on & LAYER_IRIDESCENCE) != 0u {
        f0 = mix(f0, layers.film, layers.iridescence);
    }
    let reflected = clamp((f0 * lookup.x + lookup.y) * layers.specular, vec3<f32>(0.0), vec3<f32>(1.0));
    let coat = 1.0 - layers.clearcoat * (0.04 + 0.96 * pow(1.0 - n_v, 5.0));
    return rgb * s.albedo * volume_transmittance(travel) * (1.0 - reflected) * amount * coat;
}
