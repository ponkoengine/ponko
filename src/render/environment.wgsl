// Render-only IBL baking. Cube faces and roughness levels are packed into
// a 2D atlas so WebGL2 needs neither compute nor cube arrays.
const PI: f32 = 3.14159265359;
// equator.w: 1 for a three-color gradient (sky, equator, ground).
struct Config { params: vec4<f32>, sky: vec4<f32>, ground: vec4<f32>, equator: vec4<f32> };
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> config: Config;
struct Out { @builtin(position) position: vec4<f32> };
@vertex fn vertex(@builtin(vertex_index) i: u32) -> Out {
    var o: Out;
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    o.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return o;
}
// The ambient light without an atmosphere: sky above, ground below, and
// with an equator color, that color on the horizon.
fn gradient(d: vec3<f32>) -> vec3<f32> {
    if config.equator.w > 0.0 {
        if d.y >= 0.0 { return mix(config.equator.rgb, config.sky.rgb, d.y); }
        return mix(config.equator.rgb, config.ground.rgb, -d.y);
    }
    return mix(config.ground.rgb, config.sky.rgb, d.y * 0.5 + 0.5);
}
fn cube_dir(face: u32, uv: vec2<f32>) -> vec3<f32> {
    let p = uv * 2.0 - 1.0;
    switch face {
        case 0u: { return normalize(vec3<f32>(1.0, -p.y, -p.x)); }
        case 1u: { return normalize(vec3<f32>(-1.0, -p.y, p.x)); }
        case 2u: { return normalize(vec3<f32>(p.x, 1.0, p.y)); }
        case 3u: { return normalize(vec3<f32>(p.x, -1.0, -p.y)); }
        case 4u: { return normalize(vec3<f32>(p.x, -p.y, 1.0)); }
        default: { return normalize(vec3<f32>(-p.x, -p.y, -1.0)); }
    }
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
fn source(d: vec3<f32>) -> vec3<f32> {
    let uv = cube_uv(d);
    let pixel = vec2<f32>(uv.z * 128.0, 0.0) + clamp(uv.xy * 128.0, vec2<f32>(0.5), vec2<f32>(127.5));
    return textureSampleLevel(image, linear_sampler, pixel / vec2<f32>(768.0, 128.0), 0.0).rgb;
}
// Hammersley sequence without bit reversal (also supported by GLSL ES).
fn sequence(i: u32) -> vec2<f32> {
    var n = i; var value = 0.0; var power = 0.5;
    for (var b = 0u; b < 6u; b++) { value += f32(n & 1u) * power; n >>= 1u; power *= 0.5; }
    return vec2<f32>((f32(i) + 0.5) / 64.0, value);
}
fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.99);
    let t = normalize(cross(up, n)); return mat3x3<f32>(t, cross(n, t), n);
}
fn ggx(xi: vec2<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let c = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let s = sqrt(max(0.0, 1.0 - c * c)); let phi = 2.0 * PI * xi.x;
    return vec3<f32>(cos(phi) * s, sin(phi) * s, c);
}
@fragment fn cube(in: Out) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    let d = cube_dir(u32(p.x / 128.0), vec2<f32>(p.x % 128.0, p.y) / 128.0);
    var light = gradient(d);
    if config.params.y > 0.0 {
        let up = asin(clamp(d.y, -1.0, 1.0)) / (0.5 * PI);
        let uv = vec2<f32>(atan2(d.z, d.x) / (2.0 * PI) + 0.5, 0.5 - 0.5 * sign(up) * sqrt(abs(up)));
        light = sample_sky(image, linear_sampler, uv) * config.params.z;
    }
    return vec4<f32>(light, 1.0);
}
// A slot's maps start at row 128 + 256 * slot of the atlas; params.w moves
// them up by that many rows (a refresh's copy starts at row 0).
fn maps_row() -> f32 {
    return 128.0 + config.params.x * 256.0 - config.params.w;
}
@fragment fn prefilter(in: Out) -> @location(0) vec4<f32> {
    let p = in.position.xy - vec2<f32>(0.0, maps_row());
    var size = 128.0; var y = p.y; var mip = 0.0;
    for (var k = 0u; k < 7u; k++) {
        if y < size { break; } y -= size; size *= 0.5; mip += 1.0;
    }
    if p.x >= size * 6.0 || p.y >= 255.0 { discard; }
    let n = cube_dir(u32(p.x / size), vec2<f32>(p.x % size, y) / size);
    if mip == 0.0 { return vec4<f32>(source(n), 1.0); }
    let tangent = basis(n); var color = vec3<f32>(0.0); var weight = 0.0;
    for (var i = 0u; i < 64u; i++) {
        let h = tangent * ggx(sequence(i), mip / 7.0);
        let l = reflect(-n, h); let w = max(dot(n, l), 0.0);
        if w > 0.0 { color += source(l) * w; weight += w; }
    }
    return vec4<f32>(color / max(weight, 0.001), 1.0);
}
// The nine order-2 spherical harmonics at unit direction `d`.
fn sh_basis(d: vec3<f32>, i: u32) -> f32 {
    switch i {
        case 0u: { return 0.282095; }
        case 1u: { return 0.488603 * d.y; }
        case 2u: { return 0.488603 * d.z; }
        case 3u: { return 0.488603 * d.x; }
        case 4u: { return 1.092548 * d.x * d.y; }
        case 5u: { return 1.092548 * d.y * d.z; }
        case 6u: { return 0.315392 * (3.0 * d.z * d.z - 1.0); }
        case 7u: { return 1.092548 * d.x * d.z; }
        default: { return 0.546274 * (d.x * d.x - d.y * d.y); }
    }
}
// Part of one spherical harmonic coefficient of the cube source (the row is
// the slot): column `x` holds coefficient `x % 9` over one band of rows of
// one face (`SH_BANDS` bands per face), so the sum runs on many pixels at
// once; `diffuse` adds the parts. Every 2 x 2 texels are read in one
// bilinear sample, weighted by the solid angle they cover.
const SH_STEPS: u32 = 64u;
const SH_BANDS: u32 = 8u;
const SH_PARTS: u32 = 6u * SH_BANDS;
@fragment fn sh(in: Out) -> @location(0) vec4<f32> {
    let column = u32(in.position.x);
    let i = column % 9u;
    let part = column / 9u;
    let face = part / SH_BANDS;
    let rows = SH_STEPS / SH_BANDS;
    let first = (part % SH_BANDS) * rows;
    var sum = vec3<f32>(0.0);
    let step = 2.0 / f32(SH_STEPS);
    for (var y = first; y < first + rows; y++) {
        for (var x = 0u; x < SH_STEPS; x++) {
            let uv = (vec2<f32>(f32(x), f32(y)) + 0.5) / f32(SH_STEPS);
            let p = uv * 2.0 - 1.0;
            let solid_angle = step * step / pow(1.0 + dot(p, p), 1.5);
            let d = cube_dir(face, uv);
            let pixel = vec2<f32>(f32(face) * 128.0, 0.0) + uv * 128.0;
            let light = textureSampleLevel(image, linear_sampler, pixel / vec2<f32>(768.0, 128.0), 0.0).rgb;
            sum += light * sh_basis(d, i) * solid_angle;
        }
    }
    return vec4<f32>(sum, 1.0);
}
// The diffuse light for each direction (irradiance over pi, as a cosine-
// weighted average of the light), from the source's spherical harmonics
// (`image` is their texture here): smooth, with no sampling noise.
@fragment fn diffuse(in: Out) -> @location(0) vec4<f32> {
    let p = in.position.xy - vec2<f32>(800.0, maps_row());
    let n = cube_dir(u32(p.x / 16.0), vec2<f32>(p.x % 16.0, p.y) / 16.0);
    // The cosine lobe's weight per band, over pi: 1, 2/3, 1/4.
    let band = array<f32, 9>(1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25);
    var color = vec3<f32>(0.0);
    for (var i = 0u; i < 9u; i++) {
        var c = vec3<f32>(0.0);
        for (var part = 0u; part < SH_PARTS; part++) {
            c += textureLoad(image, vec2<i32>(i32(part * 9u + i), i32(config.params.x)), 0).rgb;
        }
        color += c * band[i] * sh_basis(n, i);
    }
    // A bright sun can ring below zero on the far side.
    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
@fragment fn brdf(in: Out) -> @location(0) vec4<f32> {
    let uv = (in.position.xy - vec2<f32>(800.0, 0.0)) / 128.0;
    let nv = max(uv.x, 0.001); let r = uv.y;
    let v = vec3<f32>(sqrt(1.0 - nv * nv), 0.0, nv); var result = vec2<f32>(0.0);
    for (var i = 0u; i < 64u; i++) {
        let h = ggx(sequence(i), r); let l = reflect(-v, h);
        let nl = max(l.z, 0.0); let nh = max(h.z, 0.001); let vh = max(dot(v, h), 0.0);
        if nl > 0.0 {
            let k = r * r * 0.5;
            let g = nv / (nv * (1.0 - k) + k) * nl / (nl * (1.0 - k) + k);
            let vis = g * vh / max(nh * nv, 0.001); let fc = pow(1.0 - vh, 5.0);
            result += vec2<f32>((1.0 - fc) * vis, fc * vis);
        }
    }
    return vec4<f32>(result / 64.0, 0.0, 1.0);
}
// A captured face over the sky: the capture is premultiplied, with alpha 0
// where nothing was drawn (with an atmosphere the sky pass covers it all).
@fragment fn capture(in: Out) -> @location(0) vec4<f32> {
    let face = u32(in.position.x / 128.0);
    let local = vec2<f32>(in.position.x - f32(face) * 128.0, in.position.y);
    let d = cube_dir(face, local / 128.0);
    let sky = gradient(d);
    // The cube layout is left-handed: the camera's right is the face's left.
    let c = textureLoad(image, vec2<i32>(127 - i32(local.x), i32(local.y)), 0);
    return vec4<f32>(c.rgb + sky * (1.0 - c.a), 1.0);
}
// A refresh's copy of a slot's maps (`image`, starting at row 0) over the
// atlas's: the pipeline blends it in by the blend constant.
@fragment fn blend(in: Out) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy) - vec2<i32>(0, i32(128.0 + config.params.x * 256.0));
    return vec4<f32>(textureLoad(image, p, 0).rgb, 1.0);
}
