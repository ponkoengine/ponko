// Post-processing of the 3D frame: bloom (a mip chain of downsamples and
// upsamples, as in Jimenez's "Next generation post processing in Call of
// Duty"), then exposure, white balance, contrast, saturation, tone mapping,
// split toning, a color lookup table, vignette, chromatic aberration and
// lens distortion.

struct Post {
    grade: vec4<f32>,   // exposure multiplier, bloom, contrast, saturation
    balance: vec4<f32>, // white balance in LMS; w: vignette
    mode: vec4<u32>,    // tone map, lookup table size (0 none), bloom on
    bloom: vec4<f32>,   // threshold, soft knee
    output: vec4<f32>,  // scaling: one output pixel in uv, sharpness, scene size / output size
    lens: vec4<f32>,    // distortion: theta (1/theta pinching), sigma, 1/scale, 1 barrel -1 pinch 0 off
    fringe: vec4<f32>,  // x: chromatic aberration amount (0 off)
    split_shadows: vec4<f32>,    // split toning: shadows' tint, balance
    split_highlights: vec4<f32>, // highlights' tint, 1 on (0 off)
    dof: vec4<f32>,     // depth of field: focus distance, sharp range, largest blur (0 off)
    camera: vec4<f32>,  // near and far planes
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp: sampler;
@group(0) @binding(2) var<uniform> post: Post;
@group(0) @binding(3) var bloom: texture_2d<f32>;
@group(1) @binding(0) var lut: texture_2d<f32>;
@group(1) @binding(1) var lut_sampler: sampler;

struct Full {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Full;
    out.at = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

// ---- Bloom ----

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// 13 taps in 5 overlapping boxes: a smooth half-size image without flicker.
fn downsample13(uv: vec2<f32>, karis: bool) -> vec3<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let a = textureSample(source, linear_clamp, uv + t * vec2<f32>(-2.0, -2.0)).rgb;
    let b = textureSample(source, linear_clamp, uv + t * vec2<f32>(0.0, -2.0)).rgb;
    let c = textureSample(source, linear_clamp, uv + t * vec2<f32>(2.0, -2.0)).rgb;
    let d = textureSample(source, linear_clamp, uv + t * vec2<f32>(-2.0, 0.0)).rgb;
    let e = textureSample(source, linear_clamp, uv).rgb;
    let f = textureSample(source, linear_clamp, uv + t * vec2<f32>(2.0, 0.0)).rgb;
    let g = textureSample(source, linear_clamp, uv + t * vec2<f32>(-2.0, 2.0)).rgb;
    let h = textureSample(source, linear_clamp, uv + t * vec2<f32>(0.0, 2.0)).rgb;
    let i = textureSample(source, linear_clamp, uv + t * vec2<f32>(2.0, 2.0)).rgb;
    let j = textureSample(source, linear_clamp, uv + t * vec2<f32>(-1.0, -1.0)).rgb;
    let k = textureSample(source, linear_clamp, uv + t * vec2<f32>(1.0, -1.0)).rgb;
    let l = textureSample(source, linear_clamp, uv + t * vec2<f32>(-1.0, 1.0)).rgb;
    let m = textureSample(source, linear_clamp, uv + t * vec2<f32>(1.0, 1.0)).rgb;
    let boxes = array<vec3<f32>, 5>(
        (j + k + l + m) * 0.25,
        (a + b + d + e) * 0.25,
        (b + c + e + f) * 0.25,
        (d + e + g + h) * 0.25,
        (e + f + h + i) * 0.25,
    );
    let weights = array<f32, 5>(0.5, 0.125, 0.125, 0.125, 0.125);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var n = 0; n < 5; n++) {
        // Karis average on the first step: one very bright pixel can't
        // become a blinking blob.
        let w = select(weights[n], weights[n] / (1.0 + luma(boxes[n])), karis);
        sum += boxes[n] * w;
        total += w;
    }
    return sum / total;
}

// First step: only light above the threshold, with a soft knee.
@fragment
fn fs_bloom_prefilter(in: Full) -> @location(0) vec4<f32> {
    let c = downsample13(in.uv, true) * post.grade.x;
    let threshold = post.bloom.x;
    let knee = max(threshold * post.bloom.y, 1e-4);
    let brightness = max(c.r, max(c.g, c.b));
    var soft = clamp(brightness - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 1e-4);
    let contribution = max(soft, brightness - threshold) / max(brightness, 1e-4);
    return vec4<f32>(c * contribution, 1.0);
}

@fragment
fn fs_bloom_down(in: Full) -> @location(0) vec4<f32> {
    return vec4<f32>(downsample13(in.uv, false), 1.0);
}

// A 3x3 tent filter; blended over the bigger mip by the pass.
@fragment
fn fs_bloom_up(in: Full) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    var sum = textureSample(source, linear_clamp, in.uv).rgb * 4.0;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(-1.0, 0.0)).rgb * 2.0;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(1.0, 0.0)).rgb * 2.0;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(0.0, -1.0)).rgb * 2.0;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(0.0, 1.0)).rgb * 2.0;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(-1.0, -1.0)).rgb;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(1.0, -1.0)).rgb;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(-1.0, 1.0)).rgb;
    sum += textureSample(source, linear_clamp, in.uv + t * vec2<f32>(1.0, 1.0)).rgb;
    return vec4<f32>(sum / 16.0, 1.0);
}

// ---- Tone mapping ----

// Narkowicz's fit of the ACES filmic curve.
fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// AgX (Troy Sobotka), with Benjamin Wrensch's polynomial fit of its curve.
fn agx(c: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var x = inset * max(c, vec3<f32>(1e-10));
    x = clamp((log2(x) - min_ev) / (max_ev - min_ev), vec3<f32>(0.0), vec3<f32>(1.0));
    let x2 = x * x;
    let x4 = x2 * x2;
    x = 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
    // The curve's output is display-encoded (gamma 2.2); back to linear.
    x = pow(max(outset * x, vec3<f32>(0.0)), vec3<f32>(2.2));
    return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Khronos PBR Neutral.
fn neutral(c_in: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    var c = c_in;
    let x = min(c.r, min(c.g, c.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    c -= offset;
    let peak = max(c.r, max(c.g, c.b));
    if peak < start {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    switch post.mode.x {
        case 0u: { return aces(c); }
        case 1u: { return agx(c); }
        case 2u: { return clamp(neutral(c), vec3<f32>(0.0), vec3<f32>(1.0)); }
        default: { return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }
    }
}

// ---- Grading ----

fn white_balance(c: vec3<f32>) -> vec3<f32> {
    // Linear sRGB to LMS and back (the matrices Unity uses).
    let lms = vec3<f32>(
        dot(c, vec3<f32>(3.90405e-1, 5.49941e-1, 8.92632e-3)),
        dot(c, vec3<f32>(7.08416e-2, 9.63172e-1, 1.35775e-3)),
        dot(c, vec3<f32>(2.31082e-2, 1.28021e-1, 9.36245e-1)),
    ) * post.balance.xyz;
    return vec3<f32>(
        dot(lms, vec3<f32>(2.85847e+0, -1.62879e+0, -2.48910e-2)),
        dot(lms, vec3<f32>(-2.10182e-1, 1.15820e+0, 3.24281e-4)),
        dot(lms, vec3<f32>(-4.18120e-2, -1.18169e-1, 1.06867e+0)),
    );
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn from_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// Soft light (the W3C formula): a blend of 0.5 changes nothing, above
// brightens and tints, below darkens.
fn soft_light(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let d = select(sqrt(a), ((16.0 * a - 12.0) * a + 4.0) * a, a <= vec3<f32>(0.25));
    let dark = a - (1.0 - 2.0 * b) * a * (1.0 - a);
    let light = a + (2.0 * b - 1.0) * (d - a);
    return select(light, dark, b <= vec3<f32>(0.5));
}

// Split toning on the tone mapped color, in sRGB (as the tints are picked):
// the shadows' tint weighs most where the picture is dark, the highlights'
// where it is bright, the balance moving the split.
fn split_tone(c: vec3<f32>) -> vec3<f32> {
    var s = to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    let light = clamp(luma(s) + post.split_shadows.w * 0.5, 0.0, 1.0);
    s = soft_light(s, mix(vec3<f32>(0.5), post.split_shadows.rgb, 1.0 - light));
    s = soft_light(s, mix(vec3<f32>(0.5), post.split_highlights.rgb, light));
    return from_srgb(s);
}

// The lookup table is read with sRGB-encoded coordinates and returns
// linear color (its texture is sRGB): a neutral table changes nothing.
fn apply_lut(c: vec3<f32>) -> vec3<f32> {
    let n = f32(post.mode.y);
    let s = clamp(to_srgb(c), vec3<f32>(0.0), vec3<f32>(1.0)) * (n - 1.0);
    let blue = floor(s.b);
    let next = min(blue + 1.0, n - 1.0);
    let size = vec2<f32>(n * n, n);
    let uv0 = (vec2<f32>(blue * n + s.r, s.g) + 0.5) / size;
    let uv1 = (vec2<f32>(next * n + s.r, s.g) + 0.5) / size;
    let a = textureSampleLevel(lut, lut_sampler, uv0, 0.0).rgb;
    let b = textureSampleLevel(lut, lut_sampler, uv1, 0.0).rgb;
    return mix(a, b, s.b - blue);
}

// ---- Lens ----

// The usual "lens distortion" mapping (as in Unity's post stack): scale
// about the middle, then bend the radius with tan (barrel) or atan
// (pincushion).
fn distort(uv_in: vec2<f32>) -> vec2<f32> {
    if post.lens.w == 0.0 {
        return uv_in;
    }
    let uv = (uv_in - 0.5) * post.lens.z + 0.5;
    let ruv = uv - 0.5;
    let r = max(length(ruv), 1e-5);
    var bent: f32;
    if post.lens.w > 0.0 {
        bent = tan(r * post.lens.x) / (r * post.lens.y);
    } else {
        bent = post.lens.x * atan(r * post.lens.y) / r;
    }
    return uv + ruv * (bent - 1.0);
}

// The scene through the lens: distorted, and with red, green and blue
// taken a little apart toward the edges when there is a fringe.
fn lens_sample(uv: vec2<f32>) -> vec4<f32> {
    let amount = post.fringe.x;
    let center = textureSampleLevel(source, linear_clamp, distort(uv), 0.0);
    if amount <= 0.0 {
        return center;
    }
    let coords = 2.0 * uv - 1.0;
    let end = uv - coords * dot(coords, coords) * amount;
    let delta = (end - uv) / 3.0;
    let g = textureSampleLevel(source, linear_clamp, distort(uv + delta), 0.0).g;
    let b = textureSampleLevel(source, linear_clamp, distort(uv + delta * 2.0), 0.0).b;
    return vec4<f32>(center.r, g, b, center.a);
}

@fragment
fn fs_tonemap(in: Full) -> @location(0) vec4<f32> {
    var texel: vec4<f32>;
    var bloom_uv = in.uv;
    if post.lens.w == 0.0 && post.fringe.x <= 0.0 {
        texel = textureLoad(source, vec2<i32>(in.at.xy), 0);
    } else {
        texel = lens_sample(in.uv);
        bloom_uv = distort(in.uv);
    }
    // Alpha is 1 where models were drawn and 0 on the background, which
    // keeps the game's clear color as it is (averaged at the edges).
    var c = texel.rgb * post.grade.x;
    if post.mode.z != 0u {
        c += textureSampleLevel(bloom, linear_clamp, bloom_uv, 0.0).rgb * post.grade.y;
    }
    c = max(white_balance(c), vec3<f32>(0.0));
    // Contrast around middle grey, in stops.
    c = 0.18 * pow(c / 0.18 + 1e-6, vec3<f32>(post.grade.z));
    c = max(mix(vec3<f32>(luma(c)), c, post.grade.w), vec3<f32>(0.0));
    c = tonemap(c);
    if post.split_highlights.w != 0.0 {
        c = split_tone(c);
    }
    if post.mode.y != 0u {
        c = apply_lut(c);
    }
    let from_center = (in.uv - 0.5) * 2.0;
    c *= 1.0 - post.balance.w * smoothstep(0.4, 1.4, dot(from_center, from_center));
    return vec4<f32>(mix(texel.rgb, c, texel.a), 1.0);
}

// FXAA directional luma filter. Reads the tone-mapped LDR intermediate.
@fragment
fn fs_fxaa(in: Full) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    let nw = textureSampleLevel(source, linear_clamp, uv + t * vec2<f32>(-1.0, -1.0), 0.0).rgb;
    let ne = textureSampleLevel(source, linear_clamp, uv + t * vec2<f32>(1.0, -1.0), 0.0).rgb;
    let sw = textureSampleLevel(source, linear_clamp, uv + t * vec2<f32>(-1.0, 1.0), 0.0).rgb;
    let se = textureSampleLevel(source, linear_clamp, uv + t * vec2<f32>(1.0, 1.0), 0.0).rgb;
    let c = textureSampleLevel(source, linear_clamp, uv, 0.0).rgb;
    let lnw = luma(nw); let lne = luma(ne); let lsw = luma(sw); let lse = luma(se); let lc = luma(c);
    let lo = min(lc, min(min(lnw, lne), min(lsw, lse)));
    let hi = max(lc, max(max(lnw, lne), max(lsw, lse)));
    if hi - lo < max(0.0312, hi * 0.125) { return vec4<f32>(c, 1.0); }
    var dir = vec2<f32>(-((lnw + lne) - (lsw + lse)), (lnw + lsw) - (lne + lse));
    let reduce = max((lnw + lne + lsw + lse) * (0.25 * 0.125), 1.0 / 128.0);
    dir = clamp(dir / (min(abs(dir.x), abs(dir.y)) + reduce), vec2<f32>(-8.0), vec2<f32>(8.0)) * t;
    let a = 0.5 * (textureSampleLevel(source, linear_clamp, uv + dir * (-1.0/6.0), 0.0).rgb + textureSampleLevel(source, linear_clamp, uv + dir * (1.0/6.0), 0.0).rgb);
    let b = a * 0.5 + 0.25 * (textureSampleLevel(source, linear_clamp, uv - dir * 0.5, 0.0).rgb + textureSampleLevel(source, linear_clamp, uv + dir * 0.5, 0.0).rgb);
    return vec4<f32>(select(b, a, luma(b) < lo || luma(b) > hi), 1.0);
}

// ---- Scaling ----
// Draws the scene-sized frame onto the window: bilinear when it is smaller
// (render scale below 1); a small box filter when it is larger (above 1).
// The adaptive sharpening is AMD's contrast adaptive sharpening idea (a
// cross of neighbours, weighted by how much room the local contrast has
// before clipping), applied to the scaled image.

fn scaled_tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear_clamp, uv, 0.0).rgb;
}

@fragment
fn fs_upscale(in: Full) -> @location(0) vec4<f32> {
    let px = post.output.xy;
    let uv = in.uv;
    var e: vec3<f32>;
    if post.output.w > 1.0 {
        // Several source texels per output pixel: four taps cover them.
        let o = px * 0.25;
        e = 0.25 * (scaled_tap(uv + vec2<f32>(-o.x, -o.y)) + scaled_tap(uv + vec2<f32>(o.x, -o.y))
            + scaled_tap(uv + vec2<f32>(-o.x, o.y)) + scaled_tap(uv + vec2<f32>(o.x, o.y)));
    } else {
        e = scaled_tap(uv);
    }
    let sharpness = post.output.z;
    if sharpness <= 0.0 {
        return vec4<f32>(e, 1.0);
    }
    let b = scaled_tap(uv + vec2<f32>(0.0, -px.y));
    let d = scaled_tap(uv + vec2<f32>(-px.x, 0.0));
    let f = scaled_tap(uv + vec2<f32>(px.x, 0.0));
    let h = scaled_tap(uv + vec2<f32>(0.0, px.y));
    let mn = min(min(min(d, e), min(f, b)), h);
    let mx = max(max(max(d, e), max(f, b)), h);
    let amp = sqrt(clamp(min(mn, vec3<f32>(1.0) - mx) / max(mx, vec3<f32>(1e-4)), vec3<f32>(0.0), vec3<f32>(1.0)));
    let w = amp * (-1.0 / mix(8.0, 5.0, sharpness));
    let c = clamp(((b + d + f + h) * w + e) / (vec3<f32>(1.0) + 4.0 * w), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(c, 1.0);
}
