// Screen-space reflections that keep thin things whole without TAA
// (`Post::ssr_thin_objects`): appended to `ssr.wgsl` for the pipelines that
// read the coarse depth, and only for them. The trace without the setting
// is `fs_trace` there, untouched: this file adds a pass that makes the
// coarse depth (`cs_coarse`) and a trace that reads it (`fs_trace_thin`, as
// `fs_trace` but for the march; keep the two in step).
//
// Far along the ray the march's steps are tens of pixels long and jump over
// thin things: without TAA to fill the gaps, a rail near the camera was
// reflected dotted, keeping about half its light. Here a long step first
// reads the coarse depth along the stretch it jumps over, and where
// something there could be in front of the ray, the depth every half of
// the crossing's allowance (`most`); the step ends where the ray first
// went behind something. Flat surfaces (floors, walls, the sky) cost a read
// or two a step, so the depth is read where there are edges, where thin
// things are.
// The coarse depth: for each CELL x CELL block of `depth` (its level 0),
// and each 2x2 of the level before (levels 1 to 3: 16, 32 and 64 pixels),
// made by `cs_coarse` after the copy: (the nearest depth, the greatest in
// reverse-Z; then where the depth there is one plane, within
// `flat_tolerance`, the plane's depth at the middle and its change a pixel
// to the right and down, else -1, 0, 0). The depth of a flat surface
// changes linearly across the screen, and so does the ray's: whether the
// ray goes behind a flat block follows from where it starts and ends
// there, without reading the depth. A long step of the march reads the
// level whose blocks are as wide as the step at its ends, the blocks of
// level 0 only where the ray may go behind those, and the depth itself
// only in such blocks (see the march in `fs_trace_thin`). Floors, walls and the sky are flat,
// so the depth is read where there are edges, where thin things are.
@group(0) @binding(11) var coarse: texture_2d<f32>;
const CELL: i32 = 8;
const COARSE_LEVELS: u32 = 4u;
// How far from its plane a flat block's depth may be: the depth's precision
// and the fit's rounding, relative to the depth.
fn flat_tolerance(z: f32) -> f32 { return 1e-4 * z + 1e-7; }
// A block of the coarse depth, and its middle in pixels.
struct Coarse { depth: vec4<f32>, middle: vec2<f32> };
// The block of `level` at `uv`.
fn coarse_at(uv: vec2<f32>, size: vec2<f32>, level: u32) -> Coarse {
    let width = f32(CELL << level);
    // Floats: an integer division by a width only known here costs dozens
    // of instructions, and this runs several times a step.
    let p = clamp(floor(uv * size / width), vec2<f32>(0.0), ceil(size / width) - 1.0);
    let middle = (p + 0.5) * width;
    return Coarse(textureLoad(coarse, vec2<i32>(p), i32(level)), middle);
}
// One thread a block, four pixels a read (`textureGather`); a workgroup's
// threads then make its 4x4, 2x2 and one blocks of levels 1 to 3. (A
// full-screen pass loading the 64 pixels of each block one by one cost a
// fifth of a millisecond at 1080p.)
@group(0) @binding(13) var nearest_sampler: sampler;
@group(0) @binding(14) var coarse0: texture_storage_2d<rgba32float, write>;
@group(0) @binding(15) var coarse1: texture_storage_2d<rgba32float, write>;
@group(0) @binding(16) var coarse2: texture_storage_2d<rgba32float, write>;
@group(0) @binding(17) var coarse3: texture_storage_2d<rgba32float, write>;
var<workgroup> coarse_blocks: array<vec4<f32>, 64>;
// A block from its 2x2 (`child` pixels wide each): flat where they are, on
// one plane, the mean of theirs.
fn merged(at: u32, stride: u32, child: f32) -> vec4<f32> {
    var block = vec4<f32>(0.0);
    var flat = true;
    for (var k = 0u; k < 4u; k++) {
        let c = coarse_blocks[at + (k & 1u) * stride + (k >> 1u) * stride * 8u];
        block = vec4<f32>(max(block.x, c.x), block.yzw + c.yzw * 0.25);
        flat = flat && c.y >= 0.0;
    }
    let tolerance = flat_tolerance(block.x);
    for (var k = 0u; k < 4u; k++) {
        let c = coarse_blocks[at + (k & 1u) * stride + (k >> 1u) * stride * 8u];
        let offset = (vec2<f32>(f32(k & 1u), f32(k >> 1u)) - 0.5) * child;
        let off_middle = abs(c.y - block.y - dot(block.zw, offset));
        let off_change = dot(abs(c.zw - block.zw), vec2<f32>(0.5 * child));
        flat = flat && off_middle + off_change <= tolerance;
    }
    if !flat { block = vec4<f32>(block.x, -1.0, 0.0, 0.0); }
    return block;
}
@compute @workgroup_size(8, 8)
fn cs_coarse(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    let size = vec2<f32>(textureDimensions(depth));
    // The corner shared by the block's first 2x2 pixels.
    let base = vec2<f32>(vec2<i32>(id.xy) * CELL) + 1.0;
    // A gather's four pixels: x is the bottom left, y bottom right, z top
    // right, w top left.
    let left = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    let top = vec4<f32>(0.0, 0.0, 1.0, 1.0);
    var gathered: array<vec4<f32>, 16>;
    var nearest = vec4<f32>(0.0);
    var sum = vec4<f32>(0.0);
    var change = vec2<f32>(0.0);
    for (var k = 0; k < 16; k++) {
        let corner = vec2<f32>(f32(k % 4), f32(k / 4)) * 2.0;
        let d = textureGather(0, depth, nearest_sampler, (base + corner) / size);
        gathered[k] = d;
        nearest = max(nearest, d);
        sum += d;
        // A least-squares plane: the pixels' offsets from the block's
        // middle, whose squares add up to 336 on each axis.
        change += vec2<f32>(dot(d, corner.x - 2.5 - left), dot(d, corner.y - 2.5 - top));
    }
    let block = max(max(nearest.x, nearest.y), max(nearest.z, nearest.w));
    let middle = dot(sum, vec4<f32>(1.0)) / 64.0;
    change /= 336.0;
    var off = 0.0;
    for (var k = 0; k < 16; k++) {
        let corner = vec2<f32>(f32(k % 4), f32(k / 4)) * 2.0;
        let plane = middle + change.x * (corner.x - 2.5 - left) + change.y * (corner.y - 2.5 - top);
        let d = abs(gathered[k] - plane);
        off = max(off, max(max(d.x, d.y), max(d.z, d.w)));
    }
    // A block past the edge isn't flat: the gathers repeat the edge.
    let inside = all(vec2<f32>((id.xy + 1u) * u32(CELL)) <= size);
    var plane = vec4<f32>(block, -1.0, 0.0, 0.0);
    if inside && off <= flat_tolerance(block) { plane = vec4<f32>(block, middle, change); }
    if all(id.xy < textureDimensions(coarse0)) { textureStore(coarse0, id.xy, plane); }
    let at = local.y * 8u + local.x;
    coarse_blocks[at] = plane;
    // Each level in the slot of its block's first child.
    for (var level = 1u; level < COARSE_LEVELS; level++) {
        workgroupBarrier();
        let stride = 1u << (level - 1u);
        let first = all(local.xy % (stride * 2u) == vec2<u32>(0u));
        var made = vec4<f32>(0.0);
        if first { made = merged(at, stride, f32(CELL << (level - 1u))); }
        workgroupBarrier();
        if first {
            coarse_blocks[at] = made;
            let p = id.xy >> vec2<u32>(level);
            switch level {
                case 1u: { if all(p < textureDimensions(coarse1)) { textureStore(coarse1, p, made); } }
                case 2u: { if all(p < textureDimensions(coarse2)) { textureStore(coarse2, p, made); } }
                default: { if all(p < textureDimensions(coarse3)) { textureStore(coarse3, p, made); } }
            }
        }
    }
}

// Whether the ray between `a` and `b` on the line may be behind the depth
// of block or region `c`: behind its plane (within `flat_tolerance`; both
// change linearly along the ray, so its ends tell), or where it isn't flat,
// behind its nearest depth.
fn may_pass_behind(l: Line, a: f32, b: f32, c: Coarse, size: vec2<f32>) -> bool {
    let ray = vec2<f32>(line_depth(l, a), line_depth(l, b));
    if c.depth.y < 0.0 { return min(ray.x, ray.y) < c.depth.x; }
    let from_middle_a = line_uv(l, a) * size - c.middle;
    let from_middle_b = line_uv(l, b) * size - c.middle;
    let plane = c.depth.y + vec2<f32>(dot(c.depth.zw, from_middle_a), dot(c.depth.zw, from_middle_b));
    // Each level's planes are within the tolerance of the ones they were
    // made from.
    return min(ray.x - plane.x, ray.y - plane.y) < f32(2u * COARSE_LEVELS) * flat_tolerance(c.depth.x);
}
// Where between `a` and `b` on the line (the ray in front of the depth at
// `a`) the ray first goes behind something, looked at `fine` apart at most
// (fractions of the line), and how far apart the points looked at there
// were; (-1, 0): nothing there. For a stretch of a level above 0: by the
// blocks of level 0 (`cell`, a block's width as a fraction of the line),
// reading the depth only in those the ray may go behind.
fn skipped_in_blocks(l: Line, a: f32, b: f32, size: vec2<f32>, cell: f32, fine: f32) -> vec2<f32> {
    let spans = i32(min(ceil((b - a) / cell), f32(1 << (COARSE_LEVELS - 1u))));
    var head = a;
    var at_head = coarse_at(line_uv(l, a), size, 0u);
    for (var k = 1; k <= spans; k++) {
        let tail = mix(a, b, f32(k) / f32(spans));
        let at_tail = coarse_at(line_uv(l, tail), size, 0u);
        if may_pass_behind(l, head, tail, at_head, size) || may_pass_behind(l, head, tail, at_tail, size) {
            let found = skipped_pixels(l, head, tail, size, fine);
            if found.x >= 0.0 { return found; }
        }
        head = tail;
        at_head = at_tail;
    }
    return vec2<f32>(-1.0, 0.0);
}
// The same within a block's stretch, from `a` to `b`: the depth itself.
fn skipped_pixels(l: Line, a: f32, b: f32, size: vec2<f32>, fine: f32) -> vec2<f32> {
    let n = max(ceil((b - a) / fine), 1.0);
    for (var j = 1; j <= i32(n); j++) {
        let t = mix(a, b, f32(j) / n);
        if line_behind(l, t, size) { return vec2<f32>(t, (b - a) / n); }
    }
    return vec2<f32>(-1.0, 0.0);
}

@fragment fn fs_trace_thin(in: Full) -> Traced {
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
    // A step longer than half of `most` also reads the coarse depth along
    // the stretch it jumps over (see `skipped_in_blocks`), and the depth every
    // half of `most` where something there could be in front of the ray:
    // without TAA to fill the gaps, steps tens of pixels long jumped over
    // thin things (a rail near the camera came out dotted).
    let cell = f32(CELL) / pixels;
    var before = 0.0;
    var was_behind = false;
    // How far in front of the camera the surface the ray was last behind
    // is (see the still-behind case below).
    var behind_w = 0.0;
    var hit = -1.0;
    // The block of the coarse depth where the last step ended, and where on
    // the line and at which level (-1: none yet): the march's steps meet,
    // so a step doesn't read again what the one before read.
    var last_block = Coarse(vec4<f32>(0.0), vec2<f32>(0.0));
    var last_block_at = vec2<f32>(-1.0);
    var i = 0;
    // Shortened steps don't count toward STEPS, so the loop has its own
    // limit: a GPU hangs on a loop that doesn't end.
    var turns = 0;
    loop {
        if i >= STEPS || turns >= 2 * STEPS { break; }
        turns++;
        var s = first + scale * (grown - 1.0);
        // This step's share of the line: the march's slope here.
        var step = scale * GROWTH * grown / f32(STEPS);
        if !on_screen(line_uv(line, s)) { break; }
        // Whether the ray went behind something the step jumped over: then
        // this step ends there instead, and the next one is taken again
        // from it (once, as the ray is then behind).
        var shortened = false;
        if !was_behind && step > most * 0.5 {
            let level = u32(clamp(ceil(log2((s - before) / cell)), 0.0, f32(COARSE_LEVELS - 1u)));
            if any(last_block_at != vec2<f32>(before, f32(level))) {
                last_block = coarse_at(line_uv(line, before), size, level);
            }
            let at_end = coarse_at(line_uv(line, s), size, level);
            var skip = vec2<f32>(-1.0, 0.0);
            if may_pass_behind(line, before, s, last_block, size) || may_pass_behind(line, before, s, at_end, size) {
                if level == 0u {
                    skip = skipped_pixels(line, before, s, size, most * 0.5);
                } else {
                    skip = skipped_in_blocks(line, before, s, size, cell, most * 0.5);
                }
            }
            last_block = at_end;
            last_block_at = vec2<f32>(s, f32(level));
            if skip.x >= 0.0 && skip.x < s {
                s = skip.x;
                step = skip.y;
                shortened = true;
            }
        }
        let uv = line_uv(line, s);
        let q = vec2<i32>(uv * size);
        let z = z_at(q);
        let is_behind = !is_sky(z) && line_depth(line, s) < z;
        if is_behind {
            // A shortened step was looked at `step` apart.
            var lo = select(before, s - step, shortened);
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
                // The crossing was looked for at most half of `most` apart:
                // a quarter of what the trace without the coarse depth
                // allows, so a thin thing's reflection is as thin as the
                // thing.
                let travel = line_w_change(line, w, min(step, most) * 0.5);
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
        if !shortened {
            grown *= ratio;
            i++;
        }
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
