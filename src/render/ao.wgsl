// GTAO: horizon search in view-aligned slices and analytic cosine-weighted
// integration (Jimenez et al., Practical Realtime Strategies for Accurate
// Indirect Occlusion, 2016). World positions/normals reconstructed from depth.
struct Settings {
    inverse: mat4x4<f32>,
    projection: mat4x4<f32>,
    previous: mat4x4<f32>,
    temporal: vec4<f32>, // weight, rotation, half resolution, reserved
    camera: vec4<f32>,
    params: vec4<f32>, // radius, intensity (0: no AO), slices, steps
    sun: vec4<f32>,    // xyz: toward the sun; w: contact shadow length (0: none)
};
@group(0) @binding(0) var depth: texture_depth_multisampled_2d;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var<uniform> settings: Settings;
@group(0) @binding(3) var history: texture_2d<f32>;
@group(0) @binding(4) var velocity: texture_2d<f32>;
struct Full { @builtin(position) at: vec4<f32> };
@vertex fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    var out: Full;
    out.at = vec4<f32>(vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
    return out;
}
fn coordinate(p: vec2<i32>) -> vec2<i32> {
    return clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(depth)) - 1);
}
fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    return select(fallback, v / max(length(v), 0.00001), dot(v, v) > 0.00000001);
}
fn z_at(p: vec2<i32>) -> f32 { return textureLoad(depth, coordinate(p), 0); }
// The depth is reverse-Z: 1 at the near plane, 0 at the far plane and the
// sky. Next to the far plane counts as sky too (as far as 1 - 1e-6 did
// with standard depth).
fn is_sky(z: f32) -> bool { return z <= 0.000001; }
// The nearest of a pixel's samples (the greatest reverse-Z depth). Only
// velocity dilation needs all MSAA samples; keep horizon tracing cheap.
fn motion_depth(p: vec2<i32>) -> f32 {
    var z = 0.0;
    for (var sample = 0; sample < i32(textureNumSamples(depth)); sample++) {
        z = max(z, textureLoad(depth, coordinate(p), sample));
    }
    return z;
}
fn position_at_depth(p: vec2<i32>, z: f32) -> vec3<f32> {
    let uv = (vec2<f32>(coordinate(p)) + 0.5) / vec2<f32>(textureDimensions(depth));
    let h = settings.inverse * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
    return h.xyz / h.w;
}
fn position(p: vec2<i32>) -> vec3<f32> { return position_at_depth(p, z_at(p)); }
// How far in front of the camera (clip w) pixel `p` is.
fn view_distance(p: vec2<i32>) -> f32 {
    let uv = (vec2<f32>(coordinate(p)) + 0.5) / vec2<f32>(textureDimensions(depth));
    let h = settings.inverse * vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z_at(p), 1.0);
    return 1.0 / max(abs(h.w), 1e-20);
}
// How much the distance in front of the camera changes over one pixel at
// `p`, on the side of each axis that changes least (so a silhouette next to
// the pixel doesn't count). Temporal antialiasing moves each frame's pixels
// by a fraction of one: on a surface seen at a grazing angle far away, the
// distance under a pixel changes by this much from frame to frame.
fn distance_slope(p: vec2<i32>) -> f32 {
    let w = view_distance(p);
    let x = min(abs(view_distance(p + vec2<i32>(1, 0)) - w), abs(view_distance(p - vec2<i32>(1, 0)) - w));
    let y = min(abs(view_distance(p + vec2<i32>(0, 1)) - w), abs(view_distance(p - vec2<i32>(0, 1)) - w));
    return max(x, y);
}
// Steps of the march toward the sun for contact shadows.
const CONTACT_STEPS: i32 = 12;

// 1 where something on screen blocks the sun within `settings.sun.w` of
// `center` (pixel `p`), else 0: a march toward the sun, each step tested
// against the depth there. Only what's in front of a step by no more than
// the march length counts, so a wall far behind doesn't shade a point.
fn contact_shadow(p: vec2<i32>, center: vec3<f32>) -> f32 {
    let length = settings.sun.w;
    let size = vec2<f32>(textureDimensions(depth));
    let eye = distance(center, settings.camera.xyz);
    // The depth buffer's precision falls with distance: so does trust.
    let bias = 0.02 + eye * 0.002;
    let jitter = fract(52.9829189 * fract(dot(vec2<f32>(p), vec2<f32>(0.06711056, 0.00583715))));
    for (var i = 0; i < CONTACT_STEPS; i++) {
        let at = center + settings.sun.xyz * length * (f32(i) + jitter + 0.5) / f32(CONTACT_STEPS);
        let clip = settings.projection * vec4<f32>(at, 1.0);
        if clip.w <= 0.0 { break; }
        let uv = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
        if any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) { break; }
        let q = vec2<i32>(uv * size);
        if is_sky(z_at(q)) { continue; }
        let gap = distance(at, settings.camera.xyz) - distance(position(q), settings.camera.xyz);
        if gap > bias && gap < length {
            return 1.0;
        }
    }
    return 0.0;
}

@fragment fn fs_ao(in: Full) -> @location(0) vec4<f32> {
    let p = coordinate(vec2<i32>(in.at.xy * select(1.0, 2.0, settings.temporal.z > 0.0)));
    if is_sky(z_at(p)) { return vec4<f32>(0.0); }
    let center = position(p);
    var contact = 0.0;
    if settings.sun.w > 0.0 && settings.temporal.z == 0.0 {
        contact = contact_shadow(p, center);
    }
    if settings.params.y <= 0.0 {
        return vec4<f32>(0.0, contact, 0.0, 1.0);
    }
    let view = normalize(settings.camera.xyz - center);
    // Use the less discontinuous neighbor on either side of each axis.
    let left = center - position(p - vec2<i32>(1, 0));
    let right = position(p + vec2<i32>(1, 0)) - center;
    let up = center - position(p - vec2<i32>(0, 1));
    let down = position(p + vec2<i32>(0, 1)) - center;
    let dx = select(left, right, dot(right, right) < dot(left, left));
    let dy = select(up, down, dot(down, down) < dot(up, up));
    var normal = safe_normalize(cross(dx, dy), view);
    normal *= select(-1.0, 1.0, dot(normal, view) >= 0.0);
    let radius = settings.params.x;
    // Projection of a world-space radius at this pixel (perspective).
    let clip = settings.projection * vec4<f32>(center, 1.0);
    let pixel_radius = clamp(radius * length(vec3<f32>(settings.projection[0].y, settings.projection[1].y, settings.projection[2].y)) * f32(textureDimensions(depth).y) / max(abs(clip.w), 0.01) * 0.5, 1.0, 256.0);
    var visibility = 0.0;
    for (var slice = 0u; slice < u32(settings.params.z); slice++) {
        let phi = (f32(slice) + 0.5 + settings.temporal.y) * 3.14159265 / settings.params.z;
        let direction = vec2<f32>(cos(phi), sin(phi));
        let neighbor = position(p + vec2<i32>(round(direction * 2.0)));
        let fallback = safe_normalize(cross(view, vec3<f32>(0.0, 1.0, 0.0)), vec3<f32>(1.0, 0.0, 0.0));
        let tangent = safe_normalize(neighbor - center - view * dot(neighbor - center, view), fallback);
        let axis = normalize(cross(tangent, view));
        let projected = normal - axis * dot(normal, axis);
        let projected_length = max(length(projected), 0.0001);
        let cos_n = clamp(dot(projected / projected_length, view), -1.0, 1.0);
        let angle_n = acos(cos_n) * sign(dot(projected, tangent));
        var horizons = vec2<f32>(-1.0);
        for (var side = 0u; side < 2u; side++) {
            let sign_side = select(-1.0, 1.0, side == 1u);
            // A local max, not horizons[side]: FXC (D3D) can't write a vector
            // through a dynamic index without unrolling the dynamic loop below.
            var best = -1.0;
            for (var step = 1u; step <= u32(settings.params.w); step++) {
                let offset = direction * sign_side * max(1.0, pixel_radius * pow(f32(step) / settings.params.w, 2.0));
                let at = p + vec2<i32>(round(offset));
                if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(textureDimensions(depth))) || is_sky(z_at(at)) { continue; }
                let delta = position(at) - center;
                let dist = length(delta);
                if dist < 0.001 || dist > radius { continue; }
                // Fade distant occluders back to the open hemisphere.
                let horizon = dot(delta / dist, view) - 0.02 * radius / max(dist, 0.01);
                let falloff = 1.0 - smoothstep(radius * 0.5, radius, dist);
                best = max(best, mix(-1.0, horizon, falloff));
            }
            if side == 0u { horizons.x = best; } else { horizons.y = best; }
        }
        let h1 = angle_n + clamp(-acos(clamp(horizons.x, -1.0, 1.0)) - angle_n, -1.5707963, 1.5707963);
        let h2 = angle_n + clamp(acos(clamp(horizons.y, -1.0, 1.0)) - angle_n, -1.5707963, 1.5707963);
        visibility += projected_length * 0.25 * (2.0 * cos_n + 2.0 * (h1 + h2) * sin(angle_n) - cos(2.0 * h1 - angle_n) - cos(2.0 * h2 - angle_n));
    }
    let ao = pow(clamp(visibility / settings.params.z, 0.0, 1.0), settings.params.y);
    return vec4<f32>(1.0 - ao, contact, 0.0, 1.0);
}
// Blue 1 marks screen-space reflections in the target's second level (an RG8
// target drops it; see ssr.rs).
struct Resolved { @location(0) color: vec4<f32>, @location(1) history: vec4<f32> };
fn resolve_ao(in: Full) -> Resolved {
    let p = vec2<i32>(in.at.xy);
    if is_sky(z_at(p)) { return Resolved(vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0)); }
    let center = position(p);
    var sum = vec2<f32>(0.0);
    var total = 0.0;
    let scale = select(1, 2, settings.temporal.z > 0.0);
    let begin = select(-2, 0, scale == 2);
    let end = select(2, 1, scale == 2);
    let base = select(p, vec2<i32>(floor((vec2<f32>(p) + 0.5) / 2.0 - 0.5)), scale == 2);
    for (var y = begin; y <= end; y++) {
        for (var x = begin; x <= end; x++) {
            let q = clamp(base + vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(textureDimensions(image))-1);
            let at = coordinate(q * scale + vec2<i32>(scale / 2));
            if is_sky(z_at(at)) { continue; }
            let delta = position(at) - center;
            let w = exp(-dot(vec2<f32>(at-p) / f32(scale), vec2<f32>(at-p) / f32(scale)) * 0.25) * exp(-dot(delta, delta) / max(settings.params.x * settings.params.x * 0.01, 0.000001));
            sum += textureLoad(image, q, 0).rg * w;
            total += w;
        }
    }
    var value = sum / max(total, 0.00001);
    // Half-resolution tracing never softens the contact-shadow channel.
    if scale == 2 && settings.sun.w > 0.0 { value.y = contact_shadow(p, center); }
    if settings.temporal.x > 0.0 {
        let clip = settings.previous * vec4<f32>(center, 1.0);
        var nearest = p;
        var nearest_z = motion_depth(p);
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let q = coordinate(p + vec2<i32>(x,y));
                let z = motion_depth(q);
                if z > nearest_z { nearest_z = z; nearest = q; }
            }
        }
        let near_clip = settings.previous * vec4<f32>(position_at_depth(nearest, nearest_z), 1.0);
        let near_uv = near_clip.xy / near_clip.w * vec2<f32>(0.5, -0.5) + 0.5;
        let size = vec2<f32>(textureDimensions(depth));
        let camera_motion = near_uv - (vec2<f32>(nearest) + 0.5) / size;
        let uv = (vec2<f32>(p) + 0.5) / size + camera_motion + textureLoad(velocity, nearest, 0).xy;
        if near_clip.w > 0.0 && clip.w > 0.0 && all(uv >= vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0)) {
            let old = textureLoad(history, vec2<i32>(uv * vec2<f32>(textureDimensions(history))), 0);
            // The previous frame stored its distance along the view (clip w,
            // linear, unlike the reverse-Z depth); reject newly exposed
            // surfaces and sky. The allowance grows with the surface's
            // slope across a pixel, or the jitter of temporal antialiasing
            // rejects the history of distant, grazing ground every frame and
            // the rotating sample pattern shows as a pulse.
            let allowance = max(max(0.001, abs(clip.w) * 0.002), distance_slope(p) * 1.5);
            if old.w > 0.0 && abs(old.z - clip.w) < allowance {
                value.x = mix(value.x, clamp(old.x, value.x - 0.15, value.x + 0.15), settings.temporal.x);
            }
        }
    }
    let depth_value = (settings.projection * vec4<f32>(center, 1.0)).w;
    return Resolved(vec4<f32>(value, 1.0, 1.0), vec4<f32>(value, depth_value, 1.0));
}

@fragment fn fs_denoise(in: Full) -> Resolved { return resolve_ao(in); }
@fragment fn fs_spatial(in: Full) -> @location(0) vec4<f32> { return resolve_ao(in).color; }
