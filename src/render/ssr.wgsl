// Screen-space reflections, at half size. Each pixel follows its reflected
// view ray across the depth buffer; where the ray first goes behind
// something on screen, the color there is what the pixel reflects. The color comes from the last
// frame's opaque image, reprojected (this frame's isn't lit yet: the opaque
// pass mixes the result into its reflection of the environment, by the
// surface's roughness and with its Fresnel, see environment_light in
// mesh.wgsl). Rays leave by the normal the opaque pass lights the pixel
// with (`surface_normals`, which follows normal maps); the depth's own shape, its
// faces' normal, places their start and stands in where that is missing.
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
// The depth prepass's depth, copied: one 32-bit float per pixel (sample 0
// of the multisampled depth, see `fs_depth`). The march reads it dozens of
// times a pixel, and a single-sampled copy holds four times as many pixels
// in the GPU's caches: copy and trace cost less together than the trace
// reading the multisampled depth did.
@group(0) @binding(0) var depth: texture_2d<f32>;
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

// Steps along the ray, then halvings of the step that crossed. 48 rather
// than 32: far along the ray, steps are tens of pixels long and step over
// thin things (a rail, a cable) without TAA to fill the gaps; with the depth
// copy (see `fs_depth`) the 48 cost less than the 32 did reading the
// multisampled depth.
const STEPS: i32 = 48;
const REFINE: i32 = 6;
// How fast the steps grow on screen: each is e^(GROWTH / STEPS) times
// (about 1.06 times) the one before.
const GROWTH: f32 = 3.0;

fn coordinate(p: vec2<i32>) -> vec2<i32> {
    return clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(depth)) - 1);
}
fn z_at(p: vec2<i32>) -> f32 { return textureLoad(depth, coordinate(p), 0).r; }
// The copy of the depth the trace and the filtered levels read (see
// `depth`), before the trace.
@group(0) @binding(12) var scene_depth: texture_depth_multisampled_2d;
@fragment fn fs_depth(in: Full) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(scene_depth, vec2<i32>(in.at.xy), 0), 0.0, 0.0, 0.0);
}
// The depth is reverse-Z: 1 at the near plane, 0 at the far plane and the
// sky, so farther is smaller. Next to the far plane counts as sky.
fn is_sky(z: f32) -> bool { return z <= 0.000001; }
fn ndc(p: vec2<i32>) -> vec2<f32> {
    let uv = (vec2<f32>(coordinate(p)) + 0.5) / vec2<f32>(textureDimensions(depth));
    return uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
}
fn position(p: vec2<i32>) -> vec3<f32> { return point(p, z_at(p)); }
// The point at pixel `p` with depth `z`.
fn point(p: vec2<i32>, z: f32) -> vec3<f32> {
    let h = settings.inverse * vec4<f32>(ndc(p), z, 1.0);
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
// The reflections' normals (see `reflection_normal` in mesh_main.wgsl):
// the normal the opaque pass lights each pixel with where a normal map
// (or the material's `surface`, or a decal) tilts it, and whether the
// surface is glossy enough to show reflections, written by a pass over
// the prepass depth.
@group(0) @binding(18) var surface_normals: texture_2d<f32>;
// The surface at pixel `p`: its normal, and in w 1 where it is too rough
// for the opaque pass to show a reflection. The stored normal where
// something tilted it from the mesh's; elsewhere, where nothing was
// drawn, and where a pixel's samples hold two surfaces (their average is
// shorter than 1), `fallback`: the depth's normal, which the march is
// tuned on.
fn surface_normal(p: vec2<i32>, fallback: vec3<f32>) -> vec4<f32> {
    let e = textureLoad(surface_normals, coordinate(p), 0);
    // Alpha, in two bits: 0 nothing drawn, 1/3 too rough, 2/3 glossy, 1
    // glossy and tilted. Multisampling averages them at edges: an edge of a
    // rough surface over nothing (1/6) or over a glossy one (1/2) traces.
    let rough = select(0.0, 1.0, e.a > 0.17 && e.a < 0.5);
    let n = (e.xyz * 1023.0 - 512.0) / 511.0;
    let size = length(n);
    if e.a < 0.83 || size < 0.997 { return vec4<f32>(fallback, rough); }
    return vec4<f32>(n / size, rough);
}
// The ray reflected off a surface whose shape faces `normal` (the depth's)
// and whose light faces `facing` (its normal map's), seen along `view`
// (toward the camera): kept just above the surface, where a normal map
// tilts it into it.
fn reflected(view: vec3<f32>, normal: vec3<f32>, facing: vec3<f32>) -> vec3<f32> {
    var ray = reflect(-view, facing);
    let above = dot(ray, normal);
    if above < 0.01 {
        ray = safe_normalize(ray + normal * (0.01 - above), reflect(-view, normal));
    }
    return ray;
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

// The march, on screen. Along the ray's line there, the depth (z / w) and
// 1 / w change linearly (perspective keeps lines straight and those two
// affine), so a point a fraction `s` of the way along it on screen is a mix
// of the ends': no division per step. `uv` and `depth` at the start, and
// how they change over the whole line; `inverse_w` likewise.
struct Line {
    uv: vec2<f32>,
    uv_span: vec2<f32>,
    depth: f32,
    depth_span: f32,
    inverse_w: f32,
    inverse_w_span: f32,
};
fn line_uv(l: Line, s: f32) -> vec2<f32> { return l.uv + l.uv_span * s; }
fn line_depth(l: Line, s: f32) -> f32 { return l.depth + l.depth_span * s; }
// How far in front of the camera (clip w) the ray is at `s`.
fn line_w(l: Line, s: f32) -> f32 { return 1.0 / (l.inverse_w + l.inverse_w_span * s); }
// How much the ray's w changes over a fraction `ds` of the line at `s`,
// where it is `w` in front of the camera (the derivative of `line_w`).
fn line_w_change(l: Line, w: f32, ds: f32) -> f32 { return w * w * abs(l.inverse_w_span) * ds; }
// Whether the ray at `s` is behind the depth buffer at its pixel.
fn line_behind(l: Line, s: f32, size: vec2<f32>) -> bool {
    let z = z_at(vec2<i32>(line_uv(l, s) * size));
    return !is_sky(z) && line_depth(l, s) < z;
}
// The fraction of the line in clip space (and in the world) that its
// fraction `s` on screen stands for, for ends `w0` and `w1` in front of the
// camera.
fn clip_fraction(s: f32, w0: f32, w1: f32) -> f32 {
    return s * w0 / (s * w0 + (1.0 - s) * w1);
}
// How far behind the depth buffer a point `w` in front of the camera still
// counts as a hit, besides the ray's own travel (see the march), at most:
// what's on screen is assumed to be no thicker. Where the ray first goes
// behind something, `surface_thickness` ties it to what the scene shows.
fn thickness(w: f32) -> f32 { return 0.1 + 0.02 * w; }

// How thick the thing the ray went behind at pixel `at` (`w` in front of
// the camera) is assumed to be: as deep as it is tall across the view,
// where the screen shows it along the ray (`direction`, a pixel long) from
// `at` on, up to `thickness`. The screen shows a thing's front, not how far
// back it goes: a thin rail is about as deep as it is tall, a wall is deep.
// A thickness that only grew with the distance made every thin thing far
// away most of a metre deep, and its reflection half again as wide as a
// mirror's. The face at `at` goes on while the depth stays on its plane
// (`on_face`); it is followed with probes 2, 4, 8, 16 and 32 pixels out
// until one is off it, then two halvings. It costs a few reads where the
// ray goes behind something, not at every step.
fn surface_thickness(at: vec2<i32>, w: f32, direction: vec2<f32>) -> f32 {
    let most = thickness(w);
    let start = position(at);
    let z = z_at(at);
    // A pixel's width there: a thing one pixel tall is that deep, at least.
    var stretch = distance(point(at + vec2<i32>(1, 0), z), point(at, z));
    let face = face_at(at, w, direction);
    if face.y == 0.0 {
        return min(most, 0.01 + 0.001 * w + stretch);
    }
    var on = 1.0;
    var off = 64.0;
    for (var k = 0; k < 5; k++) {
        let reach = f32(2 << u32(k));
        if !on_face(at, face, direction, reach) {
            off = reach;
            break;
        }
        on = reach;
    }
    if off < 64.0 {
        for (var k = 0; k < 2; k++) {
            let mid = (on + off) * 0.5;
            if on_face(at, face, direction, mid) { on = mid; } else { off = mid; }
        }
    }
    // Only across the view: a face seen at a slant (a floor far away) is
    // no taller as seen. The pixel at `at` counts too.
    let across = position(at + vec2<i32>(round(direction * on))) - start;
    let view = normalize(start - settings.camera.xyz);
    stretch = max(stretch, length(across - view * dot(across, view)) * (on + 1.0) / on);
    // And the depth's own precision.
    return min(most, 0.01 + 0.001 * w + stretch);
}
// The plane of the face at pixel `at` (`w` in front of the camera), along
// `direction` on screen: 1 / w at `at` and how it changes over a pixel
// (along a plane, 1 / w changes linearly across the screen), from the
// neighbor along the ray or the one before it, whichever is nearer to it
// (a silhouette next to `at` doesn't count). y 0: neither neighbor is on a
// face with `at` (a thing a pixel thin).
fn face_at(at: vec2<i32>, w: f32, direction: vec2<f32>) -> vec2<f32> {
    let step = vec2<i32>(round(direction));
    let inverse = 1.0 / w;
    let ahead = inverse_w_at(at + step) - inverse;
    let back = inverse - inverse_w_at(at - step);
    let change = select(back, ahead, abs(ahead) < abs(back));
    if abs(change) > 0.05 * inverse { return vec2<f32>(inverse, 0.0); }
    return vec2<f32>(inverse, change / length(vec2<f32>(step)) + 1e-12);
}
// 1 / w at pixel `p` (0 for the sky and off the screen).
fn inverse_w_at(p: vec2<i32>) -> f32 {
    if any(p < vec2<i32>(0)) || any(p >= vec2<i32>(textureDimensions(depth))) { return 0.0; }
    let z = z_at(p);
    return select(1.0 / scene_w(p, z), 0.0, is_sky(z));
}
// Whether the pixel `reach` pixels from `at` along `direction` is still on
// the plane of `face` (see `face_at`), within 2% of its distance.
fn on_face(at: vec2<i32>, face: vec2<f32>, direction: vec2<f32>, reach: f32) -> bool {
    let offset = vec2<i32>(round(direction * reach));
    let found = inverse_w_at(at + offset);
    let expected = face.x + face.y * length(vec2<f32>(offset));
    return found > 0.0 && abs(found - expected) < 0.02 * face.x;
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
    // Normalized first: a pixel's step across a far floor is short, and the
    // product of two short steps would fall under `safe_normalize`'s floor.
    var normal = safe_normalize(cross(safe_normalize(dx, view), safe_normalize(dy, view)), view);
    normal *= select(-1.0, 1.0, dot(normal, view) >= 0.0);
    let surface = surface_normal(p, normal);
    // Too rough for the opaque pass to show a reflection
    // (`environment_light` in mesh.wgsl fades them out by 0.7): no ray.
    if surface.w > 0.5 { return missed; }
    let ray = reflected(view, normal, surface.xyz);
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
    let uv0 = to_uv(start);
    let uv1 = to_uv(end);
    let line = Line(
        uv0, uv1 - uv0,
        start.z / start.w, end.z / end.w - start.z / start.w,
        1.0 / start.w, 1.0 / end.w - 1.0 / start.w,
    );
    let pixels = max(length((uv1 - uv0) * size), 0.0001);
    // The steps: on screen, from `first` on, each e^(GROWTH / STEPS) times
    // the one before. Short steps near the surface, where contact
    // reflections are, and as many pixels apart however far the ray goes
    // into the scene. The first lands a pixel and a half out: the start's
    // own pixel can't tell the surface from what's behind it at a grazing
    // angle. Step `k` (0 to 1) falls at `first + scale (e^(GROWTH k) - 1)`;
    // `grown` holds e^(GROWTH k), one multiplication a step.
    let first = min(1.5 / pixels, 1.0);
    let scale = (1.0 - first) / (exp(GROWTH) - 1.0);
    let ratio = exp(GROWTH / f32(STEPS));
    var grown = exp(GROWTH * jitter / f32(STEPS));
    // The most of a step a crossing may count as the ray's travel (see
    // below): two pixels at 1080p, as many of the screen's height at other
    // sizes, and at least two.
    let most = max(2.0, size.y / 540.0) / pixels;
    // The ray's direction on screen, a pixel long (see `surface_thickness`).
    let direction = (uv1 - uv0) * size / pixels;
    var before = 0.0;
    var was_behind = false;
    // How far in front of the camera the surface the ray was last behind
    // is (see the still-behind case below).
    var behind_w = 0.0;
    var hit = -1.0;
    for (var i = 0; i < STEPS; i++) {
        let s = first + scale * (grown - 1.0);
        // This step's share of the line: the march's slope here.
        let step = scale * GROWTH * grown / f32(STEPS);
        let uv = line_uv(line, s);
        if !on_screen(uv) { break; }
        let q = vec2<i32>(uv * size);
        let z = z_at(q);
        let is_behind = !is_sky(z) && line_depth(line, s) < z;
        if is_behind {
            var lo = before;
            var hi = s;
            var found = false;
            if !was_behind {
                // It went behind something: where it crossed, and whether
                // it was behind by less than the thickness of what it went
                // behind (see `surface_thickness`) and two of its steps
                // there. Testing where it crossed rather than where
                // the step fell keeps the reflection's edges where the
                // scene puts them, and allowing for the steps keeps where
                // a contact reflection ends from depending on where each
                // pixel's jittered steps fell (no grain without TAA). But
                // a step counts as at most two pixels (`most`): far along
                // the ray steps are tens of pixels long, and two of them
                // made a thin thing far away as thick as the ray's travel
                // over them, a blob in the reflection where a thin line
                // belongs.
                for (var j = 0; j < REFINE; j++) {
                    let mid = (lo + hi) * 0.5;
                    if line_behind(line, mid, size) { hi = mid; } else { lo = mid; }
                }
                let w = line_w(line, hi);
                let at = vec2<i32>(line_uv(line, hi) * size);
                let travel = 2.0 * line_w_change(line, w, min(step, most));
                behind_w = scene_w(at, z_at(at));
                found = w - behind_w < surface_thickness(at, behind_w, direction) + travel;
            } else {
                // Still behind, now by less than the thickness and two of
                // this step: in front of something new, which it hit
                // within the step (where, it doesn't know; large things
                // far away are found here, past a crossing too thin for
                // them). Only a new surface, farther than the one it was
                // behind: past a rejected crossing the ray is often still
                // behind the same thin thing (its top, a step further),
                // and taking that as a hit put back the blob and the
                // grain the tight crossing leaves out.
                let w = line_w(line, s);
                let surface = scene_w(q, z);
                let new_surface = surface > behind_w + thickness(behind_w);
                behind_w = surface;
                if new_surface && w - surface < thickness(w) + 2.0 * line_w_change(line, w, step) {
                    for (var j = 0; j < REFINE; j++) {
                        let mid = (lo + hi) * 0.5;
                        if line_behind(line, mid, size) { hi = mid; } else { lo = mid; }
                    }
                    found = true;
                }
            }
            // Further behind, the ray passed behind it; go on.
            if found {
                hit = hi;
                break;
            }
        }
        was_behind = is_behind;
        before = s;
        grown *= ratio;
    }
    if hit < 0.0 { return missed; }
    let uv = line_uv(line, hit);
    // Where the hit was in the last frame's image.
    let found = position(vec2<i32>(uv * size));
    let last = settings.previous * vec4<f32>(found, 1.0);
    if last.w <= 0.0 { return missed; }
    let last_uv = to_uv(last);
    if !on_screen(last_uv) { return missed; }
    let color = textureSampleLevel(image, image_sampler, last_uv, 0.0).rgb;
    let far = 1.0 - smoothstep(0.6, 1.0, clip_fraction(hit, start.w, end.w) * reach);
    let confidence = strength * far * edge_fade(uv) * edge_fade(last_uv);
    let spread = cone_spread(center, view, distance(origin, found));
    return Traced(vec4<f32>(color * confidence, confidence), vec2<f32>(spread, 1.0));
}

// The cone's spread where the ray found nothing: its nearest neighbors'
// (bound as `image`, the trace's: spread and 1 where there was a hit), at
// half size. The light the filtered levels spread over a miss is what those
// neighbors found, so it blurs as theirs does: a miss beside a contact
// reflection stays sharp, one beside a distant reflection takes its wide
// glow. The neighbors are read from two small images of the hits that the
// filtered levels' dispatch makes beside level 2 (see `cs_blur` and
// `hit_area`): blocks, the mean spread and share of hits over each 4x4 of
// the trace's pixels, and regions, the same over 16x16. One filtered read
// of the blocks finds the hits within about six of the trace's pixels;
// failing that, one of the regions those within 16 to 32, past where the
// coarsest level's glow fades out. Every miss within that reach finds
// them, with no gaps between search directions, and the spread changes
// smoothly across the misses, so a wide blurred reflection fades out
// instead of ending in a ragged edge. None: 0, and the miss shows the
// environment. (Without the filtered levels, on tiny screens, a blank
// stand-in: only hits have a spread.)
@group(0) @binding(8) var levels: texture_2d<f32>;

// Where the hits' blocks (`over` 4) or regions (16) are in the filtered
// levels' first image, for a trace of `traced` pixels: right of level 2,
// blocks first. The image's x of its first column, and its size.
struct HitArea { x: f32, size: vec2<f32> };
fn hit_area(traced: vec2<u32>, over: u32) -> HitArea {
    let level2 = max(traced.x >> 1u, 1u);
    let blocks = (traced + 3u) / 4u;
    let size = (traced + over - 1u) / over;
    return HitArea(f32(level2 + select(blocks.x, 0u, over == 4u)), vec2<f32>(size));
}
// The hits' mean in `area` at `at` (in its pixels), filtered, clamped to
// its edges.
fn hits_near(area: HitArea, at: vec2<f32>) -> vec2<f32> {
    let inside = clamp(at, vec2<f32>(0.5), area.size - 0.5) + vec2<f32>(area.x, 0.0);
    return textureSampleLevel(levels, image_sampler, inside / vec2<f32>(textureDimensions(levels)), 0.0).rg;
}

@fragment fn fs_fill(in: Full) -> @location(0) vec4<f32> {
    let own = textureLoad(image, vec2<i32>(in.at.xy), 0).rg;
    if own.y > 0.0 { return vec4<f32>(own.x); }
    let traced = textureDimensions(image);
    let near = hits_near(hit_area(traced, 4u), in.at.xy / 4.0);
    if near.y > 0.0 { return vec4<f32>(near.x / near.y); }
    let far = hits_near(hit_area(traced, 16u), in.at.xy / 16.0);
    if far.y > 0.0 { return vec4<f32>(far.x / far.y); }
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
// The trace's hits (spread and 1 where it found something). Their mean over
// each 4x4 and 16x16 of its pixels goes beside level 2, for the spread's
// fill (see `fs_fill`): each thread sums the 2x2 under its pixel of level
// 2, four threads make a block, and sixteen blocks the workgroup's region.
@group(0) @binding(10) var hits: texture_2d<f32>;

const TILE: i32 = 8;
// Each step's results, and each pixel's plane: a point on it (w 0: sky) and
// its normal.
var<workgroup> filtered: array<vec4<f32>, 64>;
var<workgroup> centers: array<vec4<f32>, 64>;
var<workgroup> normals: array<vec3<f32>, 64>;
var<workgroup> hit_sums: array<vec2<f32>, 64>;

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
    let traced = vec2<i32>(textureDimensions(hits));
    var hit_sum = vec2<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let at = x * 2 + vec2<i32>(k & 1, k >> 1u);
        if all(at < traced) { hit_sum += textureLoad(hits, at, 0).rg; }
    }
    hit_sums[slot(local)] = hit_sum;
    workgroupBarrier();
    // A block's sum goes to its first thread's slot (no other block reads
    // that one), for the region after the next barrier.
    if all((local & vec2<i32>(1)) == vec2<i32>(0)) {
        let block = hit_sums[slot(local)] + hit_sums[slot(local + vec2<i32>(1, 0))]
            + hit_sums[slot(local + vec2<i32>(0, 1))] + hit_sums[slot(local + vec2<i32>(1, 1))];
        hit_sums[slot(local)] = block;
        let blocks = hit_area(vec2<u32>(traced), 4u);
        if all(vec2<f32>(x / 2) < blocks.size) {
            textureStore(level2, x / 2 + vec2<i32>(i32(blocks.x), 0), vec4<f32>(block / 16.0, 0.0, 0.0));
        }
    }
    var normal = vec3<f32>(0.0, 1.0, 0.0);
    if c.w > 0.0 {
        let dx = neighbor_step(x, local, size, vec2<i32>(1, 0), c.xyz);
        let dy = neighbor_step(x, local, size, vec2<i32>(0, 1), c.xyz);
        normal = safe_normalize(cross(safe_normalize(dx, normal), safe_normalize(dy, normal)), normal);
    }
    var color = reduce(c, normal, colors, taps);
    store(2u, x, size, color);
    filtered[slot(local)] = color;
    normals[slot(local)] = normal;
    workgroupBarrier();
    let regions = hit_area(vec2<u32>(traced), 16u);
    if all(local == vec2<i32>(0)) && all(vec2<f32>(group.xy) < regions.size) {
        var region = vec2<f32>(0.0);
        for (var k = 0; k < 16; k++) {
            region += hit_sums[slot(vec2<i32>(k & 3, k >> 2u) * 2)];
        }
        let at = vec2<i32>(group.xy) + vec2<i32>(i32(regions.x), 0);
        textureStore(level2, at, vec4<f32>(region / 256.0, 0.0, 0.0));
    }
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
