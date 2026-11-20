// The sky kit's sky shader (see `SkyKit`): the physical atmosphere, the
// sun's disk, the moon with its phase, the stars and the flat cloud layer,
// as the `sky` function of a sky shader. Composed after the sky helpers
// (sky.wgsl) and the cloud model (sky_kit_common.wgsl); it reads
// `frame.sky_kit` (see `SkyKit::frame_params`).
//
// The sky texture (fog, reflections, ambient light) gets the atmosphere,
// the moon's glow and the flat clouds. The background adds the sharp
// parts: the sun's and moon's disks and the stars; its clouds are the flat
// layer, or come from the cloud passes drawn over it (`frame.sky_kit[5].x`).

const KIT_MOON_TINT: vec3<f32> = vec3<f32>(1.0, 0.95, 0.86);

// The share of the moon's face lit at its phase.
fn kit_moon_lit(phase: f32) -> f32 {
    return 0.5 - 0.5 * cos(phase * 2.0 * PI);
}

// The moon's light reaching the clouds and the ground when it is up.
fn kit_moonlight() -> vec3<f32> {
    let k = frame.sky_kit;
    let up = smoothstep(-0.02, 0.1, k[0].y);
    return KIT_MOON_TINT * 0.04 * kit_moon_lit(k[0].w) * k[3].w * up;
}

// The light the clouds are lit by and where it comes from: the sun while it
// is near or above the horizon, the moon at night.
fn kit_key_light(at_height: f32) -> vec4<f32> {
    let k = frame.sky_kit;
    let sun = frame.sun_dir.xyz;
    let night = smoothstep(-0.04, -0.12, sun.y);
    // The sun's light at the clouds, through the air above them.
    let origin = vec3<f32>(0.0, GROUND + max(at_height, 0.0) / 1000.0, 0.0);
    let sunlight = transmittance(origin, sun) * frame.sky_light.rgb * (1.0 - night);
    let light = sunlight + kit_moonlight() * night;
    // The direction goes in w's sign: positive the sun, negative the moon.
    return vec4<f32>(light, select(1.0, -1.0, night > 0.5));
}

fn kit_key_dir(key: vec4<f32>) -> vec3<f32> {
    return select(frame.sky_kit[0].xyz, frame.sun_dir.xyz, key.w > 0.0);
}

// Light from the sky around the clouds (the ambient light).
fn kit_cloud_ambient() -> vec3<f32> {
    return max(frame.sky.rgb, frame.night.rgb) + kit_moonlight() * 0.5;
}

// The moon in direction `dir`: its glow everywhere, its face in the
// background (rgb), and how much of what lies behind it (the stars) it
// hides (a).
fn kit_moon(dir: vec3<f32>) -> vec4<f32> {
    let k = frame.sky_kit;
    let to_moon = k[0].xyz;
    let brightness = k[3].w;
    if brightness <= 0.0 {
        return vec4<f32>(0.0);
    }
    let lit = kit_moon_lit(k[0].w);
    let c = dot(dir, to_moon);
    let above = smoothstep(-0.03, 0.02, dir.y);
    let glow = KIT_MOON_TINT * brightness * lit * (0.02 * exp((c - 1.0) * 400.0) + 0.004 * exp((c - 1.0) * 40.0));
    let cos_r = k[3].z;
    if !drawing_background() || c < cos_r - 0.001 {
        return vec4<f32>(glow * above, 0.0);
    }
    // Where on its face: right and up across the disk, from -1 to 1.
    var right = cross(to_moon, vec3<f32>(0.0, 1.0, 0.0));
    if dot(right, right) < 1e-6 {
        right = vec3<f32>(1.0, 0.0, 0.0);
    }
    right = normalize(right);
    let up = cross(right, to_moon);
    let sin_r = sqrt(max(1.0 - cos_r * cos_r, 1e-8));
    let uv = vec2<f32>(dot(dir, right), dot(dir, up)) / sin_r;
    let r2 = dot(uv, uv);
    if r2 >= 1.0 {
        return vec4<f32>(glow * above, 0.0);
    }
    // The sphere's surface facing the camera, lit from the side its phase
    // says (from behind when new, from the front when full).
    let toward_camera = -to_moon;
    let n = uv.x * right + uv.y * up + sqrt(1.0 - r2) * toward_camera;
    let angle = k[0].w * 2.0 * PI;
    let light = sin(angle) * right - cos(angle) * toward_camera;
    // Dark seas and bright highlands.
    let seas = kit_noise2(uv * 2.2 + 5.0) * 0.6 + kit_noise2(uv * 5.1 + 1.0) * 0.4;
    let albedo = mix(0.55, 1.0, smoothstep(0.35, 0.65, seas));
    // Earthshine: the dark side shows faintly.
    let shade = max(dot(n, light), 0.0) + 0.004;
    let edge = smoothstep(1.0, 0.92, r2);
    return vec4<f32>((KIT_MOON_TINT * brightness * 2.5 * albedo * shade * edge + glow) * above, edge * above);
}

// The stars, turning around the sky's pole with the time of day, with a
// faint band of the galaxy, in the background only.
fn kit_stars(dir: vec3<f32>) -> vec3<f32> {
    let k = frame.sky_kit;
    let dark = smoothstep(0.02, -0.12, frame.sun_dir.y) * k[5].z;
    if dark <= 0.0 || dir.y < -0.05 || !drawing_background() {
        return vec3<f32>(0.0);
    }
    // Into the stars' own frame: turned back by the sun's hour angle around
    // the pole (30 degrees up toward +Z, as `Sky::sun_direction_at`).
    let pole = vec3<f32>(0.0, 0.5, 0.8660254);
    let a = -k[3].y;
    let d = dir * cos(a) + cross(pole, dir) * sin(a) + pole * dot(pole, dir) * (1.0 - cos(a));
    var light = vec3<f32>(0.0);
    // Two layers of cells: a few bright stars, many faint ones.
    for (var layer = 0; layer < 2; layer++) {
        let scale = select(160.0, 380.0, layer == 1);
        let p = d * scale;
        let cell = floor(p);
        let h = kit_hash3(cell + f32(layer) * 91.7);
        let threshold = select(0.985, 0.97, layer == 1);
        if h > threshold {
            let jitter = vec3<f32>(kit_hash3(cell + 3.1), kit_hash3(cell + 7.7), kit_hash3(cell + 11.3)) - 0.5;
            let center = cell + 0.5 + jitter * 0.6;
            let size = select(0.32, 0.4, layer == 1);
            let glow = smoothstep(size, 0.0, length(p - center));
            let strength = pow((h - threshold) / (1.0 - threshold), 2.5) * select(5.0, 1.6, layer == 1);
            let tint = mix(vec3<f32>(0.75, 0.82, 1.0), vec3<f32>(1.0, 0.86, 0.7), fract(h * 731.0));
            let twinkle = 0.8 + 0.2 * sin(frame.time.x * (1.5 + h * 30.0) + h * 1000.0);
            light += tint * glow * strength * twinkle;
        }
    }
    // The galaxy: a soft, uneven band across the sky.
    let band = dot(d, normalize(vec3<f32>(0.35, 0.82, -0.45)));
    let clouds = kit_noise2(vec2<f32>(atan2(d.z, d.x), band) * vec2<f32>(6.0, 18.0)) * 0.7 + 0.3;
    light += vec3<f32>(0.6, 0.65, 0.8) * 0.012 * exp(-band * band / 0.02) * clouds;
    return light * dark * smoothstep(-0.05, 0.12, dir.y);
}

// The sun's disk (the background only: the engine leaves it to the kit so
// the clouds can cover it).
fn kit_sun(dir: vec3<f32>) -> vec3<f32> {
    if !drawing_background() {
        return vec3<f32>(0.0);
    }
    let sun = dot(dir, frame.sun_dir.xyz);
    let edge = 1.0 - frame.atmosphere.z;
    let disk = smoothstep(frame.atmosphere.z - edge * 0.2, frame.atmosphere.z + edge * 0.2, sun);
    return frame.sun_color.rgb * disk * 40.0 * step(-0.02, dir.y);
}

// How much an overcast layer shades the air under it from the sun, 0
// (broken clouds: none) to most of it (a closed, dense layer).
fn kit_overcast() -> f32 {
    let k = frame.sky_kit;
    return smoothstep(0.6, 1.0, k[1].x) * clamp(k[2].z, 0.0, 1.0) * 0.7;
}

// The light of the air alone in direction `dir`: the atmosphere (shaded by
// an overcast) and the night's glow.
fn kit_air(dir: vec3<f32>) -> vec3<f32> {
    let air = scattered(dir, frame.sun_dir.xyz) * frame.sky_light.rgb * SKY_GAIN * (1.0 - kit_overcast());
    return air + frame.night.rgb * 0.5;
}

// Where the air's lookup texture is in the IBL atlas (`sky_texture`): 256 x
// 128 texels laid out as the sky texture, beside the cloud shadow map.
// `fs_kit_air` (sky_kit_air.wgsl) draws it (into the sky texture, copied
// here) whenever the sky texture is drawn; the background reads it instead of marching the
// atmosphere at every pixel.
const KIT_AIR_ORIGIN: vec2<f32> = vec2<f32>(128.0, 1408.0);
const KIT_AIR_SIZE: vec2<f32> = vec2<f32>(256.0, 128.0);

// The air's light in direction `dir`, from the lookup texture (as
// `sample_sky`, across the seam where the directions wrap around).
fn kit_air_lookup(dir: vec3<f32>) -> vec3<f32> {
    let uv = sky_uv(dir);
    let atlas = vec2<f32>(textureDimensions(sky_texture));
    let pixel = vec2<f32>(fract(uv.x) * KIT_AIR_SIZE.x, clamp(uv.y * KIT_AIR_SIZE.y, 0.5, KIT_AIR_SIZE.y - 0.5));
    if pixel.x < 0.5 || pixel.x > KIT_AIR_SIZE.x - 0.5 {
        let first = textureSampleLevel(sky_texture, sky_sampler, (KIT_AIR_ORIGIN + vec2<f32>(0.5, pixel.y)) / atlas, 0.0).rgb;
        let last = textureSampleLevel(sky_texture, sky_sampler, (KIT_AIR_ORIGIN + vec2<f32>(KIT_AIR_SIZE.x - 0.5, pixel.y)) / atlas, 0.0).rgb;
        return mix(last, first, fract(pixel.x + 0.5));
    }
    return textureSampleLevel(sky_texture, sky_sampler, (KIT_AIR_ORIGIN + pixel) / atlas, 0.0).rgb;
}

fn sky(dir: vec3<f32>) -> vec3<f32> {
    let k = frame.sky_kit;
    // The sky texture marches the air (256 x 128 directions); the
    // background reads that march back from its own lookup.
    var light: vec3<f32>;
    if drawing_background() {
        light = kit_air_lookup(dir);
    } else {
        light = kit_air(dir);
    }
    let moon = kit_moon(dir);
    light += kit_stars(dir) * (1.0 - moon.a) + moon.rgb + kit_sun(dir);
    // The flat clouds: in the sky texture always, in the background unless
    // the cloud passes draw them there.
    if !(drawing_background() && k[5].x > 0.5) {
        let layer = kit_layer(k);
        let key = kit_key_light(layer.base + layer.thickness * 0.5);
        let clouds = kit_flat_clouds(frame.camera.xyz, dir, layer, kit_key_dir(key), key.rgb, kit_cloud_ambient(), drawing_background());
        light = light * clouds.a + clouds.rgb;
    }
    return light;
}
