// HDR output: the frame, drawn in linear BT.709 where 1.0 is SDR (paper)
// white, encoded for the display's color space.

struct Output {
    mode: vec4<u32>,    // x: encoding (0 SDR, 1 scRGB, 2 extended sRGB, 3 PQ)
    params: vec4<f32>,  // x: scale of 1.0 (scRGB and extended sRGB), y: paper white in nits (PQ)
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> output: Output;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The sRGB curve, continued above 1 and mirrored below 0.
fn srgb_extended(c: vec3<f32>) -> vec3<f32> {
    let a = abs(c);
    let e = select(1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - 0.055, a * 12.92, a <= vec3<f32>(0.0031308));
    return sign(c) * e;
}

// SMPTE ST 2084's inverse EOTF: luminance over 10 000 nits to signal.
fn pq(l: vec3<f32>) -> vec3<f32> {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 4096.0 * 128.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 4096.0 * 32.0;
    let c3 = 2392.0 / 4096.0 * 32.0;
    let y = pow(max(l, vec3<f32>(0.0)), vec3<f32>(m1));
    return pow((c1 + c2 * y) / (1.0 + c3 * y), vec3<f32>(m2));
}

@fragment
fn fs_output(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(frame, vec2<i32>(at.xy), 0).rgb;
    switch output.mode.x {
        case 1u: {
            return vec4<f32>(c * output.params.x, 1.0);
        }
        case 2u: {
            return vec4<f32>(srgb_extended(c * output.params.x), 1.0);
        }
        case 3u: {
            // BT.709 to BT.2020 primaries (ITU-R BT.2087), then nits.
            let to2020 = mat3x3<f32>(
                vec3<f32>(0.627404, 0.069097, 0.016391),
                vec3<f32>(0.329283, 0.919540, 0.088013),
                vec3<f32>(0.043313, 0.011362, 0.895595),
            );
            let nits = max(to2020 * c, vec3<f32>(0.0)) * output.params.y;
            return vec4<f32>(pq(nits / 10000.0), 1.0);
        }
        default: {
            // SDR: the target is an sRGB view, which encodes.
            return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
        }
    }
}
