// The sky kit's cloud model, shared by its sky shader (sky_kit.wgsl: the
// flat layer and the sky texture), the cloud march (sky_kit_clouds.wgsl)
// and the cloud shadow map (sky_kit_shadow.wgsl), so all three agree on
// where the clouds are. It reads no bindings: each caller hands it the
// settings (`SkyKit::frame_params`, as an array of six vec4s).

// The cloud layer's settings (meters, world space).
struct CloudLayer {
    coverage: f32,
    kind: f32,
    drift: vec2<f32>,
    base: f32,
    thickness: f32,
    density: f32,
    self_shadow: f32,
    silver: f32,
};

fn kit_layer(k: array<vec4<f32>, 6>) -> CloudLayer {
    var layer: CloudLayer;
    layer.coverage = k[1].x;
    layer.kind = k[1].y;
    layer.drift = k[1].zw;
    layer.base = k[2].x;
    layer.thickness = k[2].y;
    layer.density = k[2].z;
    layer.self_shadow = k[2].w;
    layer.silver = k[3].x;
    return layer;
}

// Light the clouds take away per meter at density 1.
const KIT_EXTINCTION: f32 = 0.025;
// The weather's scale: the size of a cloud field, in meters.
const KIT_WEATHER_SCALE: f32 = 3200.0;
// The billows' scale: the detail the march erodes the edges with.
const KIT_DETAIL_SCALE: f32 = 420.0;
// How much of the sun's light a sunlit cloud sends toward the camera (with
// `kit_cloud_phase` 1, light scattered evenly): about what a white wall
// facing the sun does (about 1 / pi). Brighter clouds would light the ground (the
// ambient light from the sky texture) more than the sun itself does.
const KIT_CLOUD_GAIN: f32 = 0.3;

// A pseudo-random number in 0..1 for a 2D or 3D point (no sines: the same
// on every GPU).
fn kit_hash2(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.xyx) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn kit_hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

// Smooth value noise in 0..1.
fn kit_noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = kit_hash2(i);
    let b = kit_hash2(i + vec2<f32>(1.0, 0.0));
    let c = kit_hash2(i + vec2<f32>(0.0, 1.0));
    let d = kit_hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn kit_noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(kit_hash3(i), kit_hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(kit_hash3(i + vec3<f32>(0.0, 1.0, 0.0)), kit_hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(kit_hash3(i + vec3<f32>(0.0, 0.0, 1.0)), kit_hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(kit_hash3(i + vec3<f32>(0.0, 1.0, 1.0)), kit_hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

// The weather: how cloudy each point of the sky is, 0..1, spread fairly
// evenly so `coverage` is about the share of the sky clouded.
fn kit_weather(xz: vec2<f32>, layer: CloudLayer) -> f32 {
    var p = (xz + layer.drift) / KIT_WEATHER_SCALE;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i++) {
        sum += kit_noise2(p) * amp;
        p = vec2<f32>(p.x * 1.6 - p.y * 1.2, p.x * 1.2 + p.y * 1.6) + 17.0;
        amp *= 0.5;
    }
    // The sum's middle half, stretched over 0..1.
    return clamp((sum / 0.9375 - 0.25) * 2.0, 0.0, 1.0);
}

// The coarse cloud density at `xz`, height `h` through the layer (0 at the
// base, 1 at its top), 0..1, without the billows: what the shadows, the
// light toward the sun and the flat layer use.
fn kit_shape_at(w: f32, h: f32, layer: CloudLayer) -> f32 {
    if h <= 0.0 || h >= 1.0 {
        return 0.0;
    }
    // Sheets keep to the layer's lower third; heaps rise through it, the
    // strongest highest, with round tops.
    let top = mix(0.32, mix(0.45, 1.0, w), layer.kind);
    let profile = smoothstep(0.0, 0.07, h) * smoothstep(top, top * mix(0.5, 0.2, layer.kind), h);
    let cover = max(layer.coverage, 0.02);
    // Toward full coverage the gaps close: an overcast sky.
    let filled = mix(w, 1.0, smoothstep(0.7, 1.0, cover));
    return clamp((filled * profile - (1.0 - cover)) / cover, 0.0, 1.0);
}

fn kit_shape(xz: vec2<f32>, h: f32, layer: CloudLayer) -> f32 {
    return kit_shape_at(kit_weather(xz, layer), h, layer);
}

// The full density at world point `p`, with the billows eroding the edges.
fn kit_density(p: vec3<f32>, layer: CloudLayer) -> f32 {
    let h = (p.y - layer.base) / layer.thickness;
    let coarse = kit_shape(p.xz, h, layer);
    if coarse <= 0.0 {
        return 0.0;
    }
    // The billows drift a little faster than the weather, and rise.
    let q = (p + vec3<f32>(layer.drift.x * 1.3, -h * 200.0, layer.drift.y * 1.3)) / KIT_DETAIL_SCALE;
    let billows = kit_noise3(q) * 0.65 + kit_noise3(q * 2.7) * 0.35;
    let erode = mix(0.35, 0.7, layer.kind) * (1.0 - coarse);
    return clamp((coarse - billows * erode) / max(1.0 - erode, 0.1), 0.0, 1.0) * layer.density;
}

// Henyey-Greenstein phase function.
fn kit_phase(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * 3.14159265 * pow(max(1.0 + g2 - 2.0 * g * mu, 1e-4), 1.5));
}

// How light scatters off the clouds toward the camera, for the cosine `mu`
// between the view and the light: a bright forward lobe (the silver lining,
// scaled by `silver`) over a broad one.
fn kit_cloud_phase(mu: f32, silver: f32) -> f32 {
    return mix(kit_phase(mu, -0.2), kit_phase(mu, 0.8), 0.15 + 0.6 * silver) * 4.0 * 3.14159265;
}

// Light through optical depth `depth` toward the sun. Light scattered many
// times inside a cloud goes much further than a straight beam: two weaker,
// wider terms stand for it, so thick clouds stay grey-white inside and
// their undersides are dim but never black.
fn kit_light_through(depth: f32) -> f32 {
    return 0.5 * exp(-depth) + 0.25 * exp(-depth * 0.25) + 0.25 * exp(-depth * 0.05);
}

// The light around a cloud at height `h` through the layer (0 base, 1
// top): the sky's light, more of it higher up, and under the cloud a
// little sunlight thrown back up by the ground (`sun_up` is the sun's
// light times how high it stands).
fn kit_ambient_at(ambient: vec3<f32>, sun_up: vec3<f32>, h: f32) -> vec3<f32> {
    return ambient * mix(0.6, 1.15, h) + sun_up * 0.06 * (1.0 - h);
}

// Where a ray from `origin` along `dir` is inside the layer: the distances
// it enters and leaves (enter >= leave when it misses).
fn kit_slab(origin: vec3<f32>, dir: vec3<f32>, layer: CloudLayer, far: f32) -> vec2<f32> {
    let bottom = layer.base;
    let top = layer.base + layer.thickness;
    if abs(dir.y) < 1e-5 {
        if origin.y > bottom && origin.y < top {
            return vec2<f32>(0.0, far);
        }
        return vec2<f32>(1.0, 0.0);
    }
    let a = (bottom - origin.y) / dir.y;
    let b = (top - origin.y) / dir.y;
    let enter = max(min(a, b), 0.0);
    let leave = min(max(a, b), far);
    return vec2<f32>(enter, leave);
}

// The flat cloud layer: one look through the layer along `dir` from
// `origin` at a few heights, lit by `sun` (toward the sun, its light at the
// clouds `sun_light`) and `ambient`. Returns the light the clouds add and
// how much of the sky behind them still shows (rgb, a). `seen` is whether
// the camera looks at them (the background): then they shine toward the
// sun (the silver lining). Otherwise (the sky texture: the ambient light,
// fog and reflections) they scatter evenly, as a thick cloud's many
// bounces do: a bright lobe around the sun would light the ground more
// than the sun does.
fn kit_flat_clouds(
    origin: vec3<f32>,
    dir: vec3<f32>,
    layer: CloudLayer,
    sun: vec3<f32>,
    sun_light: vec3<f32>,
    ambient: vec3<f32>,
    seen: bool,
) -> vec4<f32> {
    if dir.y <= 0.0 || layer.coverage <= 0.0 || layer.density <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let span = kit_slab(origin, dir, layer, 60000.0);
    if span.x >= span.y {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let phase = select(1.0, kit_cloud_phase(dot(dir, sun), layer.silver), seen);
    let toward = sun.xz / max(sun.y, 0.1);
    var light = vec3<f32>(0.0);
    var through = 1.0;
    let steps = 4;
    let dt = (span.y - span.x) / f32(steps);
    for (var i = 0; i < steps; i++) {
        let t = span.x + dt * (f32(i) + 0.5);
        let p = origin + dir * t;
        let h = (p.y - layer.base) / layer.thickness;
        let w = kit_weather(p.xz, layer);
        let d = kit_shape_at(w, h, layer) * layer.density;
        if d <= 0.0 {
            continue;
        }
        // How much cloud lies toward the sun: two looks a little way up.
        let up = layer.thickness * 0.25;
        let shade = kit_shape(p.xz + toward * up, h + 0.25, layer) + kit_shape(p.xz + toward * up * 2.0, h + 0.5, layer);
        let sunlit = kit_light_through(shade * layer.density * up * KIT_EXTINCTION * layer.self_shadow);
        let lit = sun_light * sunlit * phase * KIT_CLOUD_GAIN + kit_ambient_at(ambient, sun_light * max(sun.y, 0.0), h);
        let sigma = d * KIT_EXTINCTION;
        let gone = exp(-sigma * dt);
        light += through * lit * (1.0 - gone);
        through *= gone;
    }
    // Far clouds fade into the haze at the horizon.
    let haze = exp(-span.x / 80000.0) * smoothstep(-0.02, 0.05, dir.y);
    return vec4<f32>(light * haze, mix(1.0, through, haze));
}
