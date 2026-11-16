// Screen-space reflections, at half size. Each pixel follows its reflected
// view ray across the depth buffer; where the ray first goes behind
// something on screen, the color there is what the pixel reflects. The color comes from the last
// frame's opaque image, reprojected (this frame's isn't lit yet: the opaque
// pass mixes the result into its reflection of the environment, by the
// surface's roughness and with its Fresnel, see environment_light in
// mesh.wgsl). Normals come from the depth, as there is no normal buffer: flat
// faces, so normal maps don't bend these reflections.
//
// How blurred a reflection is depends on the surface's roughness and on
// how far the ray went: the reflected light comes from a cone, which is
// narrow where the ray starts. The trace writes, beside the color, the
// cone's spread: the footprint, in full-size pixels, of a cone of tangent 1
// around the ray where it ends, seen through the surface (a mirror image
// of the hit, as far behind the surface as the hit is in front of it). The
// opaque pass multiplies it by its roughness's cone (see `reflection_at`
// in mesh_main.wgsl). A ray that finds nothing has none (0); `fs_fill`
// gives it its nearest neighbors', whose light the filtered levels spread
// over it. It reaches the opaque pass through the alpha of the first level
// of the screen image (GTAO's denoise, or `fs_mark` without it, copies it
// there).
struct Settings {
    inverse: mat4x4<f32>,  // this frame's clip space to the world
    projection: mat4x4<f32>, // the world to this frame's clip space
    previous: mat4x4<f32>, // the world to the clip space of `image` (the last frame)
    camera: vec4<f32>,
    params: vec4<f32>,     // strength (0: none), longest ray, frame number
};
@group(0) @binding(0) var depth: texture_depth_multisampled_2d;
// fs_trace: the last frame's opaque image. fs_copy: this frame's.
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var image_sampler: sampler;
@group(0) @binding(3) var<uniform> settings: Settings;
struct Full { @builtin(position) at: vec4<f32> };
@vertex fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    var out: Full;
    out.at = vec4<f32>(vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

// Linear steps along the ray, then halvings of the step that crossed.
const STEPS: i32 = 32;
const REFINE: i32 = 6;

fn coordinate(p: vec2<i32>) -> vec2<i32> {
    return clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(depth)) - 1);
}
fn z_at(p: vec2<i32>) -> f32 { return textureLoad(depth, coordinate(p), 0); }
// The depth is reverse-Z: 1 at the near plane, 0 at the far plane and the
// sky, so farther is smaller. Next to the far plane counts as sky.
fn is_sky(z: f32) -> bool { return z <= 0.000001; }
fn ndc(p: vec2<i32>) -> vec2<f32> {
    let uv = (vec2<f32>(coordinate(p)) + 0.5) / vec2<f32>(textureDimensions(depth));
    return uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
}
fn position(p: vec2<i32>) -> vec3<f32> {
    let h = settings.inverse * vec4<f32>(ndc(p), z_at(p), 1.0);
    return h.xyz / h.w;
}
// How far in front of the camera (clip w) the depth `z` at pixel `p` is.
fn scene_w(p: vec2<i32>, z: f32) -> f32 {
    let h = settings.inverse * vec4<f32>(ndc(p), z, 1.0);
    return 1.0 / h.w;
}
fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    return select(fallback, v / max(length(v), 0.00001), dot(v, v) > 0.00000001);
}
fn to_uv(clip: vec4<f32>) -> vec2<f32> {
    return clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
}
fn on_screen(uv: vec2<f32>) -> bool {
    return all(uv >= vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
}
// 0 at the screen's edges, 1 a tenth of the way in: a ray that leaves the
// screen there fades instead of cutting off.
fn edge_fade(uv: vec2<f32>) -> f32 {
    let d = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    return clamp(d * 10.0, 0.0, 1.0);
}
// Whether the ray at clip position `c` is behind the depth buffer at its pixel.
fn behind(c: vec4<f32>) -> bool {
    let q = vec2<i32>(to_uv(c) * vec2<f32>(textureDimensions(depth)));
    let z = z_at(q);
    return !is_sky(z) && c.z / c.w < z;
}

// The footprint, in full-size pixels, of a cone of tangent 1 around a ray
// from `center` (seen along `view`, toward the camera) that ends after
// `travel`: its diameter there, 2 * travel, seen at the mirror image of the
// ray's end, `travel` behind the surface along the view ray.
fn cone_spread(center: vec3<f32>, view: vec3<f32>, travel: f32) -> f32 {
    let mirrored = settings.projection * vec4<f32>(center - view * travel, 1.0);
    if mirrored.w <= 0.0 { return 0.0; }
    // How a step across the view ray moves the image on screen (the
    // derivative of the projection, which is linear in clip space).
    let across = safe_normalize(cross(view, vec3<f32>(0.0, 1.0, 0.0)), vec3<f32>(1.0, 0.0, 0.0));
    let step = settings.projection * vec4<f32>(across, 0.0);
    let moved = (step.xy * mirrored.w - mirrored.xy * step.w) / (mirrored.w * mirrored.w);
    let pixels = length(moved * 0.5 * vec2<f32>(textureDimensions(depth)));
    return 2.0 * travel * pixels;
}

// The trace's two targets: what the pixel reflects (rgb the color times
// alpha, alpha how much it counts; 0: the ray found nothing on screen, the
// environment stands in) and, where the ray found something, the cone's
// spread (see the top) and 1.
struct Traced { @location(0) color: vec4<f32>, @location(1) hit: vec2<f32> };

@fragment fn fs_trace(in: Full) -> Traced {
    // The top left of the 2x2 full-size pixels this one stands for.
    let p = vec2<i32>(in.at.xy) * 2;
    let strength = settings.params.x;
    let missed = Traced(vec4<f32>(0.0), vec2<f32>(0.0));
    if strength <= 0.0 || is_sky(z_at(p)) { return missed; }
    let center = position(p);
    let view = normalize(settings.camera.xyz - center);
    // The less discontinuous neighbor on either side of each axis.
    let left = center - position(p - vec2<i32>(1, 0));
    let right = position(p + vec2<i32>(1, 0)) - center;
    let up = center - position(p - vec2<i32>(0, 1));
    let down = position(p + vec2<i32>(0, 1)) - center;
    let dx = select(left, right, dot(right, right) < dot(left, left));
    let dy = select(up, down, dot(down, down) < dot(up, up));
    var normal = safe_normalize(cross(dx, dy), view);
    normal *= select(-1.0, 1.0, dot(normal, view) >= 0.0);
    let ray = reflect(-view, normal);
    // Off the surface a little, so the ray doesn't find where it starts; the
    // depth buffer's precision falls with distance, and so does the offset.
    let eye = distance(center, settings.camera.xyz);
    let origin = center + normal * (0.01 + eye * 0.002);
    let longest = settings.params.y;
    let start = settings.projection * vec4<f32>(origin, 1.0);
    var end = settings.projection * vec4<f32>(origin + ray * longest, 1.0);
    // A ray toward the camera stops before it: clip space is linear along
    // the ray, so stopping is a mix.
    var reach = 1.0;
    let nearest = start.w * 0.1;
    if end.w < nearest {
        reach = (start.w - nearest) / (start.w - end.w);
        end = mix(start, end, reach);
    }
    // Interleaved gradient noise, moving each frame: TAA averages the steps
    // of several frames into a finer march.
    let noise = vec2<f32>(p) + 5.588238 * f32(u32(settings.params.z) % 64u);
    let jitter = fract(52.9829189 * fract(dot(noise, vec2<f32>(0.06711056, 0.00583715))));
    let size = vec2<f32>(textureDimensions(depth));
    var before = 0.0;
    var before_w = start.w;
    var hit = -1.0;
    for (var i = 0; i < STEPS; i++) {
        // Squared: short steps near the surface, where contact matters most.
        let k = (f32(i) + jitter) / f32(STEPS);
        let f = k * k;
        let c = mix(start, end, f);
        let uv = to_uv(c);
        if !on_screen(uv) { break; }
        let q = vec2<i32>(uv * size);
        let z = z_at(q);
        if !is_sky(z) && c.z / c.w < z {
            // Behind it, by less than its assumed thickness (or the step):
            // a hit. Further behind, the ray passed behind it; go on.
            let gap = c.w - scene_w(q, z);
            if gap < 0.1 + 0.02 * c.w + 2.0 * abs(c.w - before_w) {
                var lo = before;
                var hi = f;
                for (var j = 0; j < REFINE; j++) {
                    let mid = (lo + hi) * 0.5;
                    if behind(mix(start, end, mid)) { hi = mid; } else { lo = mid; }
                }
                hit = hi;
                break;
            }
        }
        before = f;
        before_w = c.w;
    }
    if hit < 0.0 { return missed; }
    let uv = to_uv(mix(start, end, hit));
    // Where the hit was in the last frame's image.
    let found = position(vec2<i32>(uv * size));
    let last = settings.previous * vec4<f32>(found, 1.0);
    if last.w <= 0.0 { return missed; }
    let last_uv = to_uv(last);
    if !on_screen(last_uv) { return missed; }
    let color = textureSampleLevel(image, image_sampler, last_uv, 0.0).rgb;
    let far = 1.0 - smoothstep(0.6, 1.0, hit * reach);
    let confidence = strength * far * edge_fade(uv) * edge_fade(last_uv);
    let spread = cone_spread(center, view, distance(origin, found));
    return Traced(vec4<f32>(color * confidence, confidence), vec2<f32>(spread, 1.0));
}

// The cone's spread where the ray found nothing: its nearest neighbors'
// (bound as `image`, the trace's: spread and 1 where there was a hit), at
// half size. The light the filtered levels spread over a miss is what those
// neighbors found, so it blurs as theirs does: a miss beside a contact
// reflection stays sharp, one beside a distant reflection takes its wide
// glow. Rings of eight taps 1, 2, 4, 8 and 16 pixels out (to a coarsest
// level's footprint), each tap filtered over four pixels; the first ring
// with hits gives their mean. None: 0, and the miss shows the environment
// (its filtered levels would only spread faraway light over it). Most
// misses have no hit anywhere near (a floor that reflects the sky): one
// look at a coarse filtered level (`coarse`, level 4, whose pixels are 16
// full-size ones; without filtered levels the trace's own) says so, and
// they skip the search.
@group(0) @binding(8) var coarse: texture_2d<f32>;

@fragment fn fs_fill(in: Full) -> @location(0) vec4<f32> {
    let own = textureLoad(image, vec2<i32>(in.at.xy), 0).rg;
    if own.y > 0.0 { return vec4<f32>(own.x); }
    let texel = 1.0 / vec2<f32>(textureDimensions(image));
    if textureSampleLevel(coarse, image_sampler, in.at.xy * texel, 0.0).a <= 0.0 { return vec4<f32>(0.0); }
    for (var ring = 0u; ring < 5u; ring++) {
        // Between four pixels, so each tap averages them.
        let r = f32(1u << ring) + 0.5;
        var sum = vec2<f32>(0.0);
        for (var k = 0; k < 8; k++) {
            let angle = f32(k) * 0.7853982;
            let at = in.at.xy + vec2<f32>(cos(angle), sin(angle)) * r;
            sum += textureSampleLevel(image, image_sampler, at * texel, 0.0).rg;
        }
        if sum.y > 0.0 { return vec4<f32>(sum.x / sum.y); }
    }
    return vec4<f32>(0.0);
}

// Without GTAO, the first level of the screen image: blue 1 says the
// reflections are there, alpha is the cone's spread (bound as `image`, at
// half size, filled).
@fragment fn fs_mark(in: Full) -> @location(0) vec4<f32> {
    let half = vec2<i32>(in.at.xy) / 2;
    let spread = textureLoad(image, min(half, vec2<i32>(textureDimensions(image)) - 1), 0).r;
    return vec4<f32>(0.0, 0.0, 1.0, spread);
}

// This frame's opaque image, for the next frame's reflections.
@fragment fn fs_copy(in: Full) -> @location(0) vec4<f32> {
    return textureLoad(image, vec2<i32>(in.at.xy), 0);
}

// The filtered levels: a four-tap, depth-aware downsample of premultiplied
// radiance/confidence, from the trace's level (bound as `image`) down to four
// smaller ones in one dispatch. Each workgroup reduces a 16x16 block of the
// trace's level to 8x8, 4x4, 2x2 and 1 pixel, keeping each step in shared
// memory. A tap counts less the farther it is from the plane of the
// receiving pixel, so reflections don't bleed onto unrelated geometry; sky
// counts not at all. Level 2 reads the depth at its pixels' centers (and,
// where there's anything to filter, at its taps' centers) and finds the
// planes' normals from its neighbors; the levels below take the plane of one
// of their four pixels, so they read no depth at all. The levels go to an
// image of their own (see `ssr.rs`); missing ones (tiny screens) to a 1x1
// stand-in.
@group(0) @binding(4) var level2: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var level3: texture_storage_2d<rgba16float, write>;
@group(0) @binding(6) var level4: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var level5: texture_storage_2d<rgba16float, write>;

const TILE: i32 = 8;
// Each step's results, and each pixel's plane: a point on it (w 0: sky) and
// its normal.
var<workgroup> filtered: array<vec4<f32>, 64>;
var<workgroup> centers: array<vec4<f32>, 64>;
var<workgroup> normals: array<vec3<f32>, 64>;

fn slot(x: vec2<i32>) -> i32 { return x.y * TILE + x.x; }
fn level_size(top: vec2<i32>, steps: u32) -> vec2<i32> {
    return max(top >> vec2<u32>(steps), vec2<i32>(1));
}
// The full-size pixel at the center of pixel `x` of an image of `size`.
fn center_pixel(x: vec2<i32>, size: vec2<i32>) -> vec2<i32> {
    return vec2<i32>((vec2<f32>(x) + 0.5) / vec2<f32>(size) * vec2<f32>(textureDimensions(depth)));
}
// The surface at the center of pixel `x` of an image of `size` (w 0: sky).
fn center_at(x: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    let p = center_pixel(x, size);
    return select(vec4<f32>(position(p), 1.0), vec4<f32>(0.0), is_sky(z_at(p)));
}
// The step from `c`, at pixel `x` of level 2 (`local` in this workgroup's
// part of it, `size` in all), to its neighbor along one axis, on the side
// that's less discontinuous (zero if neither neighbor is a surface).
fn neighbor_step(x: vec2<i32>, local: vec2<i32>, size: vec2<i32>, axis: vec2<i32>, c: vec3<f32>) -> vec3<f32> {
    let before = local - axis;
    let after = local + axis;
    let has_before = all(before >= vec2<i32>(0)) && centers[slot(before)].w > 0.0;
    let has_after = all(after < vec2<i32>(TILE)) && all(x + axis < size) && centers[slot(after)].w > 0.0;
    let back = select(vec3<f32>(0.0), c - centers[slot(before)].xyz, has_before);
    let ahead = select(vec3<f32>(0.0), centers[slot(after)].xyz - c, has_after);
    return select(back, ahead, has_after && (!has_before || dot(ahead, ahead) < dot(back, back)));
}
// The weighted average of `colors`, four pixels of the level above, for a
// pixel on the plane through `c` (w 0: sky) along `normal`: each counts less
// the farther its center (in `taps`) is from the plane, and not at all on
// sky. Nothing on sky, or where none of them reflects anything.
fn reduce(c: vec4<f32>, normal: vec3<f32>, colors: array<vec4<f32>, 4>, taps: array<vec4<f32>, 4>) -> vec4<f32> {
    let zero = vec4<f32>(0.0);
    if c.w == 0.0 || all((colors[0] == zero) & (colors[1] == zero) & (colors[2] == zero) & (colors[3] == zero)) {
        return zero;
    }
    let tolerance = 0.02 + distance(c.xyz, settings.camera.xyz) * 0.005;
    var sum = zero;
    var weight = 0.0;
    for (var k = 0; k < 4; k++) {
        let gap = abs(dot(taps[k].xyz - c.xyz, normal));
        let w = select(0.0, exp(-gap / tolerance), taps[k].w > 0.0);
        sum += colors[k] * w;
        weight += w;
    }
    return sum / max(weight, 0.00001);
}
fn store(level: u32, x: vec2<i32>, size: vec2<i32>, color: vec4<f32>) {
    if any(x >= size) { return; }
    switch level {
        case 2u: { textureStore(level2, x, color); }
        case 3u: { textureStore(level3, x, color); }
        case 4u: { textureStore(level4, x, color); }
        default: { textureStore(level5, x, color); }
    }
}

@compute @workgroup_size(8, 8)
fn cs_blur(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) id: vec3<u32>) {
    let top = vec2<i32>(textureDimensions(image));
    let local = vec2<i32>(id.xy);
    // Level 2 from the trace's level.
    let size = level_size(top, 1u);
    let x = vec2<i32>(group.xy) * TILE + local;
    let c = center_at(x, size);
    var colors: array<vec4<f32>, 4>;
    var taps: array<vec4<f32>, 4>;
    var blank = true;
    for (var k = 0; k < 4; k++) {
        colors[k] = textureLoad(image, min(x * 2 + vec2<i32>(k & 1, k >> 1u), top - 1), 0);
        blank = blank && all(colors[k] == vec4<f32>(0.0));
    }
    // The depth is read only where it counts (reduce skips the rest).
    if !blank && c.w > 0.0 {
        for (var k = 0; k < 4; k++) {
            taps[k] = center_at(min(x * 2 + vec2<i32>(k & 1, k >> 1u), top - 1), top);
        }
    }
    centers[slot(local)] = c;
    workgroupBarrier();
    var normal = vec3<f32>(0.0, 1.0, 0.0);
    if c.w > 0.0 {
        let dx = neighbor_step(x, local, size, vec2<i32>(1, 0), c.xyz);
        let dy = neighbor_step(x, local, size, vec2<i32>(0, 1), c.xyz);
        normal = safe_normalize(cross(dx, dy), normal);
    }
    var color = reduce(c, normal, colors, taps);
    store(2u, x, size, color);
    filtered[slot(local)] = color;
    normals[slot(local)] = normal;
    workgroupBarrier();
    // Levels 3 to 5 from the one before: 4x4, 2x2 and 1 thread. Each
    // pixel takes the plane of one of its four (alternating corners, so the
    // chosen point stays near the middle), or of any that isn't sky.
    for (var level = 3u; level <= 5u; level++) {
        let width = TILE >> (level - 2u);
        let busy = all(local < vec2<i32>(width));
        var plane = vec4<f32>(0.0);
        if busy {
            var pick = select(0, 3, level == 4u);
            for (var k = 0; k < 4; k++) {
                let child = slot(local * 2 + vec2<i32>(k & 1, k >> 1u));
                colors[k] = filtered[child];
                taps[k] = centers[child];
            }
            for (var k = 0; k < 4; k++) {
                if taps[pick].w == 0.0 { pick = (pick + 1) & 3; }
            }
            plane = taps[pick];
            normal = normals[slot(local * 2 + vec2<i32>(pick & 1, pick >> 1u))];
            color = reduce(plane, normal, colors, taps);
            store(level, vec2<i32>(group.xy) * width + local, level_size(top, level - 1u), color);
        }
        workgroupBarrier();
        if busy {
            filtered[slot(local)] = color;
            centers[slot(local)] = plane;
            normals[slot(local)] = normal;
        }
        workgroupBarrier();
    }
}
