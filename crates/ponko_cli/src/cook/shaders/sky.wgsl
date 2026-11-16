//@@ponko-mesh-prelude@@

// The sky: a physical atmosphere (Rayleigh and Mie scattering, ozone),
// single scattering, as in sky.rs. Each frame `fs_lut` works it out into
// the sky texture (every direction around the camera); `fs_sky` draws it
// behind everything, with the sun and the stars. Built after mesh.wgsl's
// prelude, for `frame` and the sky texture.

// In kilometers.
const GROUND: f32 = 6360.0;
const TOP: f32 = 6460.0;
const RAYLEIGH: vec3<f32> = vec3<f32>(5.802e-3, 13.558e-3, 33.1e-3);
const RAYLEIGH_HEIGHT: f32 = 8.0;
const MIE_SCATTER: f32 = 3.996e-3;
const MIE_EXTINCTION: f32 = 4.44e-3;
const MIE_HEIGHT: f32 = 1.2;
const OZONE: vec3<f32> = vec3<f32>(0.650e-3, 1.881e-3, 0.085e-3);
// Light scattered more than once, which the model leaves out (SKY_GAIN in sky.rs).
const SKY_GAIN: f32 = 4.0;
const GROUND_ALBEDO: f32 = 0.25;

struct FullScreen {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle over the whole screen, at the far plane.
@vertex
fn vs_far(@builtin(vertex_index) i: u32) -> FullScreen {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FullScreen;
    // Depth 0: the far plane of the reverse-Z scene depth.
    out.clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn extinction(h: f32) -> vec3<f32> {
    let haze = frame.atmosphere.y;
    let rayleigh = RAYLEIGH * exp(-h / RAYLEIGH_HEIGHT);
    let mie = MIE_EXTINCTION * haze * exp(-h / MIE_HEIGHT);
    let ozone = OZONE * max(1.0 - abs(h - 25.0) / 15.0, 0.0);
    return rayleigh + vec3<f32>(mie) + ozone;
}

fn exit_distance(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> f32 {
    let b = dot(origin, dir);
    let c = dot(origin, origin) - radius * radius;
    return -b + sqrt(max(b * b - c, 0.0));
}

fn hits_ground(origin: vec3<f32>, dir: vec3<f32>) -> bool {
    let b = dot(origin, dir);
    let c = dot(origin, origin) - GROUND * GROUND;
    return b < 0.0 && b * b - c >= 0.0;
}

// The share of sunlight reaching `origin` from `sun`.
fn transmittance(origin: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    if hits_ground(origin, sun) {
        return vec3<f32>(0.0);
    }
    let steps = 8;
    let step = exit_distance(origin, sun, TOP) / f32(steps);
    var depth = vec3<f32>(0.0);
    for (var i = 0; i < steps; i++) {
        let p = origin + sun * (step * (f32(i) + 0.5));
        depth += extinction(length(p) - GROUND) * step;
    }
    return exp(-depth);
}

// Sky light from `dir` for a sun of brightness 1 towards `sun`.
fn scattered(dir: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let haze = frame.atmosphere.y;
    let origin = vec3<f32>(0.0, GROUND + max(frame.sky_light.w, 0.002), 0.0);
    var span = exit_distance(origin, dir, TOP);
    if hits_ground(origin, dir) {
        let b = dot(origin, dir);
        span = -b - sqrt(max(b * b - dot(origin, origin) + GROUND * GROUND, 0.0));
    }
    let steps = 24;
    let step = span / f32(steps);
    let mu = dot(dir, sun);
    let rayleigh_phase = 3.0 / (16.0 * PI) * (1.0 + mu * mu);
    let g = 0.8;
    let mie_phase = 3.0 / (8.0 * PI) * ((1.0 - g * g) * (1.0 + mu * mu))
        / ((2.0 + g * g) * pow(1.0 + g * g - 2.0 * g * mu, 1.5));
    var depth = vec3<f32>(0.0);
    var light = vec3<f32>(0.0);
    for (var i = 0; i < steps; i++) {
        let p = origin + dir * (step * (f32(i) + 0.5));
        let h = length(p) - GROUND;
        let ext = extinction(h);
        depth += ext * step * 0.5;
        let scatter = RAYLEIGH * exp(-h / RAYLEIGH_HEIGHT) * rayleigh_phase
            + vec3<f32>(MIE_SCATTER * haze * exp(-h / MIE_HEIGHT) * mie_phase);
        light += scatter * exp(-depth) * transmittance(p, sun) * step;
        depth += ext * step * 0.5;
    }
    if hits_ground(origin, dir) {
        // The ground far away, lit by the sun, seen through the air (and
        // not brightened by SKY_GAIN, which is for the air's light).
        let at = origin + dir * span;
        let up = normalize(at);
        let lit = transmittance(at, sun) * max(dot(up, sun), 0.0) * GROUND_ALBEDO / PI;
        light += lit * exp(-depth) / SKY_GAIN;
    }
    return light;
}

// The sky texture: every direction's light.
@fragment
fn fs_lut(in: FullScreen) -> @location(0) vec4<f32> {
    let dir = sky_dir(in.uv);
    let light = scattered(dir, frame.sun_dir.xyz) * frame.sky_light.rgb * SKY_GAIN;
    return vec4<f32>(light + frame.night.rgb * 0.5, 1.0);
}

// Stars: a few bright points on a grid of directions, fading in as the sun
// goes down.
fn stars(dir: vec3<f32>) -> vec3<f32> {
    let dark = smoothstep(0.05, -0.15, frame.sun_dir.y) * frame.atmosphere.w;
    if dark <= 0.0 || dir.y < 0.0 {
        return vec3<f32>(0.0);
    }
    let p = dir * 300.0;
    let cell = floor(p);
    let h = hash2(cell.xy + cell.z * 17.31);
    if h < 0.996 {
        return vec3<f32>(0.0);
    }
    let center = cell + 0.5 + (vec3<f32>(hash2(cell.yz), hash2(cell.zx), hash2(cell.xy + 3.1)) - 0.5) * 0.6;
    let glow = smoothstep(0.35, 0.0, length(p - center));
    let twinkle = 0.75 + 0.25 * sin(frame.time.x * (2.0 + h * 40.0) + h * 1000.0);
    let tint = mix(vec3<f32>(0.8, 0.85, 1.0), vec3<f32>(1.0, 0.9, 0.75), fract(h * 731.0));
    return tint * glow * twinkle * (h - 0.996) * 1500.0 * dark * smoothstep(0.0, 0.15, dir.y);
}

// The direction from the camera through the far plane at `uv` (depth 0,
// reverse-Z).
fn far_dir(uv: vec2<f32>) -> vec3<f32> {
    let far = frame.inv_view_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return normalize(far.xyz / far.w - frame.camera.xyz);
}

// The sky behind everything: where nothing was drawn.
@fragment
fn fs_sky(in: FullScreen) -> @location(0) vec4<f32> {
    let dir = far_dir(in.uv);
    return background(dir, sky_radiance(dir));
}

// The background in direction `dir`, where the sky gives `sky_light`: with the
// stars, the sun's disk and the height fog.
fn background(dir: vec3<f32>, sky_light: vec3<f32>) -> vec4<f32> {
    var light = sky_light + stars(dir);
    // The sun's disk, with a soft edge; bright enough for bloom to catch.
    let sun = dot(dir, frame.sun_dir.xyz);
    let edge = 1.0 - frame.atmosphere.z;
    let disk = smoothstep(frame.atmosphere.z - edge * 0.2, frame.atmosphere.z + edge * 0.2, sun);
    light += frame.sun_color.rgb * disk * 40.0 * step(-0.02, dir.y);
    // Height fog reaches the sky too: all the way to the horizon, and below
    // it. Distance fog doesn't: the sky has no distance.
    return vec4<f32>(fog_along(light, frame.camera.xyz + dir * 20000.0, false), 1.0);
}

// ---- the game's sky shader ----
//@@ponko-game@@
// ---- the engine's entry points ----
// The entry points of a game's sky shader (see `SkyShader`): composed after
// mesh.wgsl's prelude, sky.wgsl and the game's `sky` function.

// The sky texture: every direction's light, from the game's sky.
@fragment
fn fs_lut_shader(in: FullScreen) -> @location(0) vec4<f32> {
    return vec4<f32>(max(sky(sky_dir(in.uv)), vec3<f32>(0.0)), 1.0);
}

// The background: the game's sky at every pixel, sharper than the texture.
@fragment
fn fs_sky_shader(in: FullScreen) -> @location(0) vec4<f32> {
    let dir = far_dir(in.uv);
    return background(dir, max(sky(dir), vec3<f32>(0.0)));
}
