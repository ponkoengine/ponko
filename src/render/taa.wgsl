// Temporal antialiasing: this frame's jittered, lit HDR image is blended with
// the history (last frame's result) reprojected along each pixel's motion.
//
//  * Motion: moved models wrote their own vectors; everything else moves with
//    the camera, worked out from depth (the nearest depth of any sample in
//    3x3, so edges take the motion of the surface in front).
//  * The history is read with a Catmull-Rom filter, then clipped to the
//    colors around the pixel now (variance clipping in YCoCg), so what was
//    uncovered or changed cannot ghost.
//  * Colors are weighted by 1 / (1 + luma) when blended and clipped, as if
//    tone mapped, so a very bright pixel cannot flicker the whole edge.
//
// The image read (the "input") and the image written (the "output") may have
// different sizes: the history is as big as the output, neighbours are taken
// in the input. `fs_resolve` uses one size for both; `fs_upscale` (the
// temporal upscaler) accumulates a smaller input into an output-sized history.

struct Taa {
    // Jittered clip space back to the world (the depth was drawn with jitter).
    inv_view_proj: mat4x4<f32>,
    // The camera now and last frame, without jitter.
    view_proj: mat4x4<f32>,
    previous_view_proj: mat4x4<f32>,
    input_size: vec4<f32>,  // width, height, then their inverses
    output_size: vec4<f32>,
    // x: sharpening, y: how much of the new frame is blended in while still,
    // z: 1 to ignore the history (first frame, camera cut), w: unused
    params: vec4<f32>,
    // The temporal upscaler's: xy the frame's jitter in input pixels, z one
    // over the weight a frame's samples give an output pixel on average.
    upscale: vec4<f32>,
};

@group(0) @binding(0) var current: texture_2d<f32>;
@group(0) @binding(1) var history: texture_2d<f32>;
@group(0) @binding(2) var velocity: texture_2d<f32>;
@group(0) @binding(3) var depth: texture_depth_multisampled_2d;
@group(0) @binding(4) var linear_clamp: sampler;
@group(0) @binding(5) var<uniform> taa: Taa;

struct Full {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Full;
    out.at = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Bright pixels count for less (and come back exactly with `uncompress`).
fn compress(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + luma(c));
}

fn uncompress(c: vec3<f32>) -> vec3<f32> {
    return c / max(1.0 - luma(c), 1e-3);
}

fn to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}

fn from_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

// The color, kept finite and within a range the blend can handle.
fn safe(c: vec4<f32>) -> vec4<f32> {
    let rgb = select(vec3<f32>(0.0), c.rgb, c.rgb == c.rgb);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(60000.0)), clamp(select(0.0, c.a, c.a == c.a), 0.0, 1.0));
}

// Catmull-Rom filtered history in five bilinear taps (Jimenez, "Filmic SMAA").
fn history_at(uv: vec2<f32>) -> vec4<f32> {
    let size = taa.output_size.xy;
    let position = uv * size;
    let center = floor(position - 0.5) + 0.5;
    let f = position - center;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (center - 1.0) * taa.output_size.zw;
    let t3 = (center + 2.0) * taa.output_size.zw;
    let t12 = (center + w2 / w12) * taa.output_size.zw;
    var sum = textureSampleLevel(history, linear_clamp, vec2<f32>(t12.x, t0.y), 0.0) * (w12.x * w0.y)
        + textureSampleLevel(history, linear_clamp, vec2<f32>(t0.x, t12.y), 0.0) * (w0.x * w12.y)
        + textureSampleLevel(history, linear_clamp, t12, 0.0) * (w12.x * w12.y)
        + textureSampleLevel(history, linear_clamp, vec2<f32>(t3.x, t12.y), 0.0) * (w3.x * w12.y)
        + textureSampleLevel(history, linear_clamp, vec2<f32>(t12.x, t3.y), 0.0) * (w12.x * w3.y);
    let total = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    return max(sum / total, vec4<f32>(0.0));
}

// Clips `value` towards `center` until it is inside the box `center +- extent`.
fn clip_to_box(value: vec3<f32>, center: vec3<f32>, extent: vec3<f32>) -> vec3<f32> {
    let offset = value - center;
    let scaled = abs(offset) / max(extent, vec3<f32>(1e-5));
    let outside = max(scaled.x, max(scaled.y, scaled.z));
    return select(value, center + offset / outside, outside > 1.0);
}

// Where this pixel was last frame, in uv, and whether the history there may
// be used: from the nearest depth of the 3x3 input pixels around `home`
// (`nearest`, at `nearest_depth`) and the moved models' own vectors.
struct Earlier {
    motion: vec2<f32>,
    usable: bool,
};

fn earlier_of(uv: vec2<f32>, home: vec2<i32>, nearest: vec2<i32>, nearest_depth: f32) -> Earlier {
    var motion = vec2<f32>(0.0);
    let point = (vec2<f32>(nearest) + 0.5) * taa.input_size.zw;
    let ndc = vec2<f32>(point.x * 2.0 - 1.0, 1.0 - point.y * 2.0);
    let world4 = taa.inv_view_proj * vec4<f32>(ndc, nearest_depth, 1.0);
    let world = world4.xyz / world4.w;
    let now_clip = taa.view_proj * vec4<f32>(world, 1.0);
    let before_clip = taa.previous_view_proj * vec4<f32>(world, 1.0);
    var usable = before_clip.w > 1e-5 && now_clip.w > 1e-5;
    if usable {
        let a = now_clip.xy / now_clip.w;
        let b = before_clip.xy / before_clip.w;
        motion = vec2<f32>((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
    }
    // A model that moved says so itself.
    let object_motion = textureLoad(velocity, nearest, 0).xy;
    motion += object_motion;
    // A moved model's vector dilated onto a pixel it does not cover (the
    // background it just uncovered, sky included) would fetch the model
    // again from the history: such a pixel starts over instead.
    let own_motion = textureLoad(velocity, home, 0).xy;
    usable = usable && !(all(own_motion == vec2<f32>(0.0)) && any(object_motion != vec2<f32>(0.0)));
    let earlier = uv + motion;
    usable = usable && taa.params.z < 0.5 && all(earlier > vec2<f32>(0.0)) && all(earlier < vec2<f32>(1.0));
    return Earlier(motion, usable);
}

struct Resolved {
    // The new history...
    @location(0) history: vec4<f32>,
    // ...and the frame the rest of post processing goes on with: the same,
    // sharpened.
    @location(1) color: vec4<f32>,
};

@fragment
fn fs_resolve(in: Full) -> Resolved {
    let in_size = taa.input_size.xy;
    let last = vec2<i32>(in_size) - 1;
    let samples = i32(textureNumSamples(depth));
    // The input pixel under this output pixel, and its 3x3 neighbours.
    let home = clamp(vec2<i32>(floor(in.uv * in_size)), vec2<i32>(0), last);
    var colors: array<vec4<f32>, 9>;
    var rgb_sum = vec3<f32>(0.0);
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var lo = vec3<f32>(1e9);
    var hi = vec3<f32>(-1e9);
    var alpha_lo = 1.0;
    var alpha_hi = 0.0;
    var nearest = home;
    // Reverse-Z depth: nearer is greater (1 the near plane, 0 the far).
    var nearest_depth = -1.0;
    for (var n = 0; n < 9; n++) {
        let at = clamp(home + vec2<i32>(n % 3 - 1, n / 3 - 1), vec2<i32>(0), last);
        let c = safe(textureLoad(current, at, 0));
        colors[n] = c;
        let compressed = compress(c.rgb);
        rgb_sum += compressed;
        let y = to_ycocg(compressed);
        m1 += y;
        m2 += y * y;
        lo = min(lo, y);
        hi = max(hi, y);
        alpha_lo = min(alpha_lo, c.a);
        alpha_hi = max(alpha_hi, c.a);
        // The nearest of the pixel's samples: edges take the front surface.
        var d = -1.0;
        for (var k = 0; k < samples; k++) {
            d = max(d, textureLoad(depth, at, k));
        }
        if d > nearest_depth {
            nearest_depth = d;
            nearest = at;
        }
    }
    let now = colors[4];
    m1 /= 9.0;
    m2 /= 9.0;
    let sigma = sqrt(max(m2 - m1 * m1, vec3<f32>(0.0)));

    // Where this pixel was last frame.
    let found = earlier_of(in.uv, home, nearest, nearest_depth);
    let motion = found.motion;
    let usable = found.usable;
    let earlier = in.uv + motion;

    var result = now;
    if usable {
        let old = safe(history_at(earlier));
        // Clip the history to the colors around this pixel in YCoCg.
        let extent = max(sigma * 1.25, vec3<f32>(1e-4));
        let box_lo = max(m1 - extent, lo);
        let box_hi = min(m1 + extent, hi);
        let center = (box_lo + box_hi) * 0.5;
        let reach = max((box_hi - box_lo) * 0.5, vec3<f32>(1e-4));
        let y_old = clip_to_box(to_ycocg(compress(old.rgb)), center, reach);
        let old_rgb = uncompress(from_ycocg(y_old));
        let old_alpha = clamp(old.a, alpha_lo, alpha_hi);
        // The faster it moves, the less the (blurred by resampling) history
        // counts.
        let speed = length(motion * taa.output_size.xy);
        let fresh = mix(taa.params.y, 0.35, saturate(speed / 6.0));
        let w_now = fresh / (1.0 + luma(now.rgb));
        let w_old = (1.0 - fresh) / (1.0 + luma(old_rgb));
        result = vec4<f32>((now.rgb * w_now + old_rgb * w_old) / (w_now + w_old), mix(old_alpha, now.a, fresh));
    }

    // Sharpen against the mean of the neighbours now, not in the history.
    let mean = rgb_sum / 9.0;
    let blurred = compress(result.rgb);
    let sharp = uncompress(max(blurred + taa.params.x * (blurred - mean), vec3<f32>(0.0)));
    var out: Resolved;
    out.history = result;
    out.color = vec4<f32>(select(sharp, result.rgb, taa.params.x <= 0.0), result.a);
    return out;
}

// The temporal upscaler (`Upscaler::Temporal`): the input is the tone mapped
// frame at the render size, drawn with a jitter of `taa.upscale.xy` input
// pixels; the history is as big as the window. Each output pixel takes this
// frame's samples near it, weighted by how close each landed
// (`exp(-2.29 d²)`, d in output pixels: a Blackman-Harris window's shape),
// and blends them into its history as strongly as that weight says: a pixel
// no sample came near this frame keeps its history, so over the jitter
// sequence every output pixel collects samples at its own resolution. The
// history is reprojected and clipped as the resolve does; where there is
// none (first frame, cut, disocclusion), a smooth spatial upsample of the
// input stands in.
@fragment
fn fs_upscale(in: Full) -> Resolved {
    let in_size = taa.input_size.xy;
    let last = vec2<i32>(in_size) - 1;
    // This output pixel's center in input pixels, and the input pixel under it.
    let at_in = in.uv * in_size;
    let home = clamp(vec2<i32>(floor(at_in)), vec2<i32>(0), last);
    // Input pixel k shows the scene at k + 0.5 - jitter (unjittered).
    let jitter = taa.upscale.xy;
    let to_out = taa.output_size.xy * taa.input_size.zw;
    var near_sum = vec4<f32>(0.0);
    var near_w = 0.0;
    var soft_sum = vec4<f32>(0.0);
    var soft_w = 0.0;
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var lo = vec3<f32>(1e9);
    var hi = vec3<f32>(-1e9);
    var rgb_lo = vec3<f32>(1e9);
    var rgb_hi = vec3<f32>(-1e9);
    var alpha_lo = 1.0;
    var alpha_hi = 0.0;
    var nearest = home;
    var nearest_depth = -1.0;
    for (var n = 0; n < 9; n++) {
        let offset = vec2<i32>(n % 3 - 1, n / 3 - 1);
        let at = clamp(home + offset, vec2<i32>(0), last);
        let c = safe(textureLoad(current, at, 0));
        let compressed = compress(c.rgb);
        let y = to_ycocg(compressed);
        m1 += y;
        m2 += y * y;
        lo = min(lo, y);
        hi = max(hi, y);
        rgb_lo = min(rgb_lo, compressed);
        rgb_hi = max(rgb_hi, compressed);
        alpha_lo = min(alpha_lo, c.a);
        alpha_hi = max(alpha_hi, c.a);
        // Where this sample landed, from the output pixel's center.
        let d = vec2<f32>(home + offset) + 0.5 - jitter - at_in;
        let d_out = d * to_out;
        let w = exp(-2.29 * dot(d_out, d_out));
        near_sum += vec4<f32>(compressed, c.a) * w;
        near_w += w;
        let w_soft = exp(-2.29 * dot(d, d));
        soft_sum += vec4<f32>(compressed, c.a) * w_soft;
        soft_w += w_soft;
        // One sample a pixel: the 3x3 input pixels already reach over
        // several output pixels, and the depth loads are most of the
        // pass's cost (four samples each would be 36 an output pixel).
        let depth_here = textureLoad(depth, at, 0);
        if depth_here > nearest_depth {
            nearest_depth = depth_here;
            nearest = at;
        }
    }
    m1 /= 9.0;
    m2 /= 9.0;
    let sigma = sqrt(max(m2 - m1 * m1, vec3<f32>(0.0)));
    // The nearest samples (sharp) and a smooth upsample (no history).
    let near = near_sum / max(near_w, 1e-12);
    let soft = soft_sum / max(soft_w, 1e-12);
    let now = vec4<f32>(uncompress(near.rgb), near.a);

    let found = earlier_of(in.uv, home, nearest, nearest_depth);
    var result = vec4<f32>(uncompress(soft.rgb), soft.a);
    if found.usable {
        let old = safe(history_at(in.uv + found.motion));
        let extent = max(sigma * 1.25, vec3<f32>(1e-4));
        let box_lo = max(m1 - extent, lo);
        let box_hi = min(m1 + extent, hi);
        let center = (box_lo + box_hi) * 0.5;
        let reach = max((box_hi - box_lo) * 0.5, vec3<f32>(1e-4));
        let y_old = clip_to_box(to_ycocg(compress(old.rgb)), center, reach);
        let old_rgb = uncompress(from_ycocg(y_old));
        let old_alpha = clamp(old.a, alpha_lo, alpha_hi);
        let speed = length(found.motion * taa.output_size.xy);
        let fresh = mix(taa.params.y, 0.35, saturate(speed / 6.0));
        // This frame counts as much as its samples weigh on this pixel
        // against what they weigh on average.
        let blend = saturate(fresh * near_w * taa.upscale.z);
        let w_now = blend / (1.0 + luma(now.rgb));
        let w_old = (1.0 - blend) / (1.0 + luma(old_rgb));
        result = vec4<f32>(
            (now.rgb * w_now + old_rgb * w_old) / max(w_now + w_old, 1e-6),
            mix(old_alpha, now.a, blend),
        );
    }

    // Sharpen what the input's resolution could not show (the difference
    // from the smooth upsample), kept within the colors around the pixel.
    let blurred = compress(result.rgb);
    let sharp = clamp(blurred + taa.params.x * (blurred - soft.rgb), rgb_lo, rgb_hi);
    var out: Resolved;
    out.history = result;
    out.color = vec4<f32>(select(uncompress(sharp), result.rgb, taa.params.x <= 0.0), result.a);
    return out;
}
