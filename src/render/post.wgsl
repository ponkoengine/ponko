// Post-processing of the 3D frame: bloom (a mip chain of downsamples and
// upsamples, as in Jimenez's "Next generation post processing in Call of
// Duty"), then exposure, white balance, contrast, saturation, tone mapping,
// split toning, a color lookup table, vignette, chromatic aberration and
// lens distortion.

struct Post {
    grade: vec4<f32>,   // exposure multiplier, bloom, contrast, saturation
    balance: vec4<f32>, // white balance in LMS; w: vignette
    mode: vec4<u32>,    // tone map, lookup table size (0 none), bloom on, HDR output on
    bloom: vec4<f32>,   // threshold, soft knee
    output: vec4<f32>,  // scaling: one output pixel in uv, sharpness, scene size / output size
    lens: vec4<f32>,    // distortion: theta (1/theta pinching), sigma, 1/scale, 1 barrel -1 pinch 0 off
    fringe: vec4<f32>,  // x: chromatic aberration amount (0 off)
    split_shadows: vec4<f32>,    // split toning: shadows' tint, balance
    split_highlights: vec4<f32>, // highlights' tint, 1 on (0 off)
    dof: vec4<f32>,     // depth of field: focus distance, sharp range, largest blur (0 off)
    camera: vec4<f32>,  // near and far planes; z: HDR output's headroom over SDR white
    meter: vec4<f32>,   // auto exposure: min, max (stops), compensation, center weight
    adapt: vec4<f32>,   // auto exposure: most stops to brighten, to darken this frame
    auto_mode: vec4<u32>, // auto exposure: on, jump to the target this frame
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp: sampler;
@group(0) @binding(2) var<uniform> post: Post;
@group(0) @binding(3) var bloom: texture_2d<f32>;
// The lookup table; the auto exposure meter reads its mask in this slot.
@group(1) @binding(0) var lut: texture_2d<f32>;
@group(1) @binding(1) var lut_sampler: sampler;
// Auto exposure's adapted exposure, in stops (one texel, the sum of its red
// and green: see `exposure_texel`). The adaptation pass reads last frame's
// here; bloom and tone mapping this frame's.
@group(3) @binding(0) var exposure_state: texture_2d<f32>;

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

// The exposure multiplier: the fixed exposure, times the adapted one with
// auto exposure on.
fn exposure() -> f32 {
    if post.auto_mode.x == 0u {
        return post.grade.x;
    }
    return post.grade.x * exp2(adapted());
}

// The adapted exposure in `exposure_state`, in stops.
fn adapted() -> f32 {
    let s = textureLoad(exposure_state, vec2<i32>(0, 0), 0);
    return s.r + s.g;
}

// Stops as a texel of the 16-bit float image that holds them: rounded to
// 1/64 of a stop in red (exact in 16 bits up to 32 stops), what that left
// over in green (to about 1e-5 stops), so a slow adaptation's small steps
// aren't lost. Any device that draws the 3D frame renders to this format.
fn exposure_texel(stops: f32) -> vec4<f32> {
    let high = round(stops * 64.0) / 64.0;
    return vec4<f32>(high, stops - high, 0.0, 1.0);
}

// First step: only light above the threshold, with a soft knee.
@fragment
fn fs_bloom_prefilter(in: Full) -> @location(0) vec4<f32> {
    let c = downsample13(in.uv, true) * exposure();
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

// ---- Auto exposure ----
// A meter of the frame's log luminance, reduced to one value, then the
// exposure adapted toward what brings it to middle grey. Each image of the
// meter holds (weighted log luminance, weight), each texel the sum over the
// pixels it covers divided by the same constant for every texel of its
// image, so the next image's sums weigh partial edge texels rightly and the
// ratio of the last one's channels is the weighted mean.

// How much a point of the picture counts: toward the middle by the center
// weight, and by the mask's red channel (white without a mask).
fn meter_weight(uv: vec2<f32>) -> f32 {
    let d = length(uv - 0.5) * 1.41421356;
    let center = 1.0 - post.meter.w * smoothstep(0.0, 1.0, d);
    return center * textureSampleLevel(lut, lut_sampler, uv, 0.0).r;
}

// The first image: each texel covers 8x8 pixels of the frame, read as 16
// bilinear taps at the middles of 2x2 blocks. Only what the 3D frame drew
// counts (alpha 1); the background color is left as it is by the post pass.
@fragment
fn fs_meter(in: Full) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let base = floor(in.at.xy) * 8.0;
    let weight = meter_weight((base + 4.0) / size);
    var sum = vec2<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let texel = base + vec2<f32>(f32(i), f32(j)) * 2.0;
            if texel.x < size.x && texel.y < size.y {
                let c = textureSampleLevel(source, linear_clamp, (texel + 1.0) / size, 0.0);
                let l = clamp(log2(max(luma(c.rgb), 1e-6)), -16.0, 16.0);
                let w = c.a * weight;
                sum += vec2<f32>(w * l, w);
            }
        }
    }
    return vec4<f32>(sum / 16.0, 0.0, 1.0);
}

// The next images: each texel sums 4x4 of the image before.
@fragment
fn fs_meter_reduce(in: Full) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    let base = vec2<i32>(in.at.xy) * 4;
    var sum = vec2<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let p = base + vec2<i32>(i, j);
            if p.x < size.x && p.y < size.y {
                sum += textureLoad(source, p, 0).rg;
            }
        }
    }
    return vec4<f32>(sum / 16.0, 0.0, 1.0);
}

// The adapted exposure: last frame's (in `exposure_state`) moved toward
// the target by at most this frame's steps; the target at once on a jump.
// A frame with nothing metered keeps it.
@fragment
fn fs_adapt(in: Full) -> @location(0) vec4<f32> {
    let m = textureLoad(source, vec2<i32>(0, 0), 0).rg;
    let last = adapted();
    var goal = last;
    if m.y > 1e-7 {
        let metered = clamp(m.x / m.y, -16.0, 16.0);
        goal = clamp(log2(0.18) - metered + post.meter.z, post.meter.x, post.meter.y);
    } else if post.auto_mode.y != 0u {
        goal = clamp(0.0, post.meter.x, post.meter.y);
    }
    if post.auto_mode.y != 0u {
        return exposure_texel(goal);
    }
    let held = clamp(last, post.meter.x, post.meter.y);
    let step = clamp(goal - held, -post.adapt.y, post.adapt.x);
    return exposure_texel(held + step);
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

// HDR output: the tone mapped color's highlights opened up to the
// display's headroom over SDR white. Below the knee the SDR curve stays;
// above it the brightest channel rises to `top`, meeting the curve with the
// same slope at the knee, and the channels keep their ratios (the SDR
// picture's hue and saturation). The CPU twin is `hdr.rs`'s.
fn expand_highlights(c: vec3<f32>, top: f32) -> vec3<f32> {
    let knee = 0.8;
    let m = max(c.r, max(c.g, c.b));
    if m <= knee || top <= 1.0 {
        return c;
    }
    let t = (min(m, 1.0) - knee) / (1.0 - knee);
    let r = (1.0 - knee) / (top - knee);
    let g = r * t + (1.0 - r) * t * t;
    return c * ((knee + (top - knee) * g) / m);
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
    var c = texel.rgb * exposure();
    if post.mode.z != 0u {
        c += textureSampleLevel(bloom, linear_clamp, bloom_uv, 0.0).rgb * post.grade.y;
    }
    c = max(white_balance(c), vec3<f32>(0.0));
    // Contrast around middle grey, in stops.
    c = 0.18 * pow(c / 0.18 + 1e-6, vec3<f32>(post.grade.z));
    c = max(mix(vec3<f32>(luma(c)), c, post.grade.w), vec3<f32>(0.0));
    // With HDR output and no curve, light is kept up to the headroom, and
    // graded at SDR scale (split toning and the lookup table see 0..1).
    let hdr = post.mode.w != 0u;
    var over = 1.0;
    if hdr && post.mode.x == 3u {
        c = clamp(c, vec3<f32>(0.0), vec3<f32>(post.camera.z));
        over = max(1.0, max(c.r, max(c.g, c.b)));
        c /= over;
    } else {
        c = tonemap(c);
    }
    if post.split_highlights.w != 0.0 {
        c = split_tone(c);
    }
    if post.mode.y != 0u {
        c = apply_lut(c);
    }
    if hdr {
        if post.mode.x == 3u {
            c *= over;
        } else {
            c = expand_highlights(c, post.camera.z);
        }
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
    // HDR output's highlights go above white: clamp to its headroom.
    let top = select(1.0, post.camera.z, post.mode.w != 0u);
    let c = clamp(((b + d + f + h) * w + e) / (vec3<f32>(1.0) + 4.0 * w), vec3<f32>(0.0), vec3<f32>(top));
    return vec4<f32>(c, 1.0);
}
