// Crowd culling (see `crowds.rs`): one thread per character tests its
// culling box (the box of the frame range its clip shows now, under its
// transform) against every view of
// the frame and its origin's distance against each level's band, and
// appends it to the visible list of each (view, level) that draws it:
// from the list's front when the level draws it whole, from its back when
// it draws it dithered (crossfading into the next level). `finish` then
// writes the counts into the indirect draws, two per (view, level, mesh
// part): the whole characters', the dithered ones'.

struct Params {
    // Characters.
    count: u32,
    views: u32,
    levels: u32,
    // Indirect draws per view (every level's parts).
    parts: u32,
    // Entries per (view, level) list: a leading 0, then up to `count` indices.
    region: u32,
    // Where the indirect draws start in `draws`, after the counts.
    args: u32,
    // Threads per row of the dispatch (it may be two-dimensional).
    row: u32,
    // Clips in the bake.
    clips: u32,
    // Where the cull table starts in `views` (`CrowdData::cull_table`): per
    // clip a vec4 (frames, fps, looping, its first frame range), then per
    // frame range two vec4s (lowest and highest corner, around every
    // level's mesh in the frames of the range, in model space).
    boxes_at: u32,
    // 1: the first view (the camera) leaves out what the depth grid hides.
    occlusion: u32,
    // Global illumination's face views (`gi_first` to `gi_end`) keep only
    // the characters at least one of their pixels wide (`cut` times the
    // distance) seen from one of this frame's `probes` probes, whose
    // positions start at `probes_at` in `views`.
    gi_first: u32,
    gi_end: u32,
    probes: u32,
    probes_at: u32,
    cut: f32,
    // Seconds the characters' clips played on since their times (as
    // `elapsed` in `crowd_vertex.wgsl`).
    elapsed: f32,
    // Per level: nearest distance, farthest (exclusive), the distance it
    // fades out over before the farthest, and in over before the nearest.
    bands: array<vec4<f32>, 8>,
    // Per level: 1 drawn in cameras, | 2 drawn in shadow views.
    kinds: array<vec4<u32>, 2>,
};

// An earlier frame's camera depth, shrunk to a grid of farthest depths
// (see `three/occlusion.rs`), four to a vec4.
struct Grid {
    // Width, height, how far the camera moved since (spheres grow by it).
    head: vec4<f32>,
    view_proj: mat4x4<f32>,
    depths: array<vec4<f32>, GRID_VEC4S>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Six vec4s a character: its model matrix's columns, tint, animation.
@group(0) @binding(1) var<storage, read> records: array<vec4<f32>>;
// Per view: six planes, then the eye (w: 1 for a shadow view). After the
// views, the level of each indirect draw, four to a vec4 (as bits), then
// the probes global illumination updates this frame, and from `boxes_at`
// the cull table.
@group(0) @binding(2) var<storage, read> views: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> visible: array<u32>;
// Per (view, level) two counts (whole, dithered), then the indirect draws
// (five words each).
@group(0) @binding(4) var<storage, read_write> draws: array<atomic<u32>>;
@group(0) @binding(5) var<uniform> grid: Grid;

fn grid_depth(i: u32) -> f32 {
    return grid.depths[i / 4u][i % 4u];
}

// Whether the depth grid hides the world-axis box around `center` (half
// its size `half`): its nearest depth is behind the farthest depth of
// every cell it covers (reverse-Z: nearer is greater). As the instance
// batches' test (`instances.wgsl`).
fn hidden(center: vec3<f32>, half: vec3<f32>) -> bool {
    let extent = half + vec3<f32>(grid.head.z);
    var lo = vec2<f32>(1e30);
    var hi = vec2<f32>(-1e30);
    var nearest = -1e30;
    for (var i = 0u; i < 8u; i++) {
        let sign = vec3<f32>(
            select(-1.0, 1.0, (i & 1u) != 0u),
            select(-1.0, 1.0, (i & 2u) != 0u),
            select(-1.0, 1.0, (i & 4u) != 0u),
        );
        let clip = grid.view_proj * vec4<f32>(center + extent * sign, 1.0);
        if clip.w <= 1e-5 {
            return false;
        }
        lo = min(lo, clip.xy / clip.w);
        hi = max(hi, clip.xy / clip.w);
        nearest = max(nearest, clip.z / clip.w);
    }
    if any(lo < vec2<f32>(-1.0)) || any(hi > vec2<f32>(1.0)) || nearest > 1.0 {
        return false;
    }
    let w = u32(grid.head.x);
    let h = u32(grid.head.y);
    let size = vec2<f32>(f32(w), f32(h));
    let a = min(vec2<u32>((vec2<f32>(lo.x, -hi.y) * 0.5 + 0.5) * size), vec2<u32>(w - 1u, h - 1u));
    let b = min(vec2<u32>((vec2<f32>(hi.x, -lo.y) * 0.5 + 0.5) * size), vec2<u32>(w - 1u, h - 1u));
    for (var y = a.y; y <= b.y; y++) {
        for (var x = a.x; x <= b.x; x++) {
            if nearest >= grid_depth(y * w + x) {
                return false;
            }
        }
    }
    return true;
}

// Whether a box of half diagonal `radius` is at least one pixel wide seen
// from one of this frame's probes.
fn probe_sized(center: vec3<f32>, radius: f32) -> bool {
    for (var k = 0u; k < params.probes; k++) {
        if radius >= params.cut * distance(center, views[params.probes_at + k].xyz) {
            return true;
        }
    }
    return false;
}

// Frames of a clip that share a box (`RANGE_FRAMES` in `three/crowd.rs`).
const RANGE_FRAMES: u32 = 4u;

struct Bounds {
    lo: vec3<f32>,
    hi: vec3<f32>,
};

// The box of clip `clip` at `t` seconds: its frame range's, the range of
// the first of the two frames the vertex shader blends (`crowd_frames`
// in `crowd_vertex.wgsl`; `range_at` in `three/crowd.rs`).
fn range_bounds(clip: u32, t: f32) -> Bounds {
    let c = views[params.boxes_at + min(clip, params.clips - 1u)];
    let last = max(c.x - 1.0, 0.0);
    var p = t * c.y;
    if c.z > 0.5 && last > 0.0 {
        p = p - floor(p / last) * last;
    } else {
        p = clamp(p, 0.0, last);
    }
    let first = u32(max(min(floor(p), last), 0.0));
    let range = min(first / RANGE_FRAMES, u32(last) / RANGE_FRAMES);
    let k = params.boxes_at + params.clips + (u32(c.w) + range) * 2u;
    return Bounds(views[k].xyz, views[k + 1u].xyz);
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * params.row;
    if i >= params.count {
        return;
    }
    let base = i * 6u;
    let m0 = records[base].xyz;
    let m1 = records[base + 1u].xyz;
    let m2 = records[base + 2u].xyz;
    let origin = records[base + 3u].xyz;
    // The box of the frame range its clip shows now, of the one its blend
    // clip shows, or of both while it blends (as `character_bounds_at` in
    // `three/crowd.rs`).
    let anim = bitcast<vec4<u32>>(records[base + 5u]);
    let blend = anim.w & 0xffffu;
    let speed = f32(bitcast<i32>(anim.w) >> 16u) / 256.0;
    var lo = vec3<f32>(1e30);
    var hi = vec3<f32>(-1e30);
    if blend < 0xffffu {
        let b = range_bounds(anim.x & 0xffffu, bitcast<f32>(anim.y) + speed * params.elapsed);
        lo = b.lo;
        hi = b.hi;
    }
    if blend > 0u {
        let b = range_bounds(anim.x >> 16u, bitcast<f32>(anim.z) + speed * params.elapsed);
        lo = min(lo, b.lo);
        hi = max(hi, b.hi);
    }
    // In the world: a center and three half axes (`CullBox`).
    let mid = (lo + hi) * 0.5;
    let size = (hi - lo) * 0.5;
    let center = m0 * mid.x + m1 * mid.y + m2 * mid.z + origin;
    let a0 = m0 * size.x;
    let a1 = m1 * size.y;
    let a2 = m2 * size.z;
    let extent = abs(a0) + abs(a1) + abs(a2);
    let radius = sqrt(dot(a0, a0) + dot(a1, a1) + dot(a2, a2));
    for (var v = 0u; v < params.views; v++) {
        let at = v * 7u;
        var inside = true;
        for (var p = 0u; p < 6u; p++) {
            let plane = views[at + p];
            let reach = abs(dot(plane.xyz, a0)) + abs(dot(plane.xyz, a1)) + abs(dot(plane.xyz, a2));
            if dot(plane.xyz, center) + plane.w < -reach {
                inside = false;
                break;
            }
        }
        if !inside {
            continue;
        }
        if v == 0u && params.occlusion != 0u && hidden(center, extent) {
            continue;
        }
        if v >= params.gi_first && v < params.gi_end && !probe_sized(center, radius) {
            continue;
        }
        let eye = views[at + 6u];
        let far = length(origin - eye.xyz);
        let shadow = eye.w > 0.5;
        for (var l = 0u; l < params.levels; l++) {
            let band = params.bands[l];
            let kinds = params.kinds[l / 4u][l % 4u];
            if (kinds & select(1u, 2u, shadow)) == 0u {
                continue;
            }
            // The share of its pixels the level keeps: 1 whole, negative
            // fading out, positive fading in (as `level_fade` in `crowds.rs`).
            var keep = 1.0;
            if far >= band.x && far < band.y {
                let start = band.y - band.z;
                if band.z > 0.0 && far >= start {
                    keep = -(1.0 - (far - start) / band.z);
                }
            } else if band.w > 0.0 && far < band.x && far >= band.x - band.w {
                keep = (far - (band.x - band.w)) / band.w;
            } else {
                continue;
            }
            let steps = floor(abs(keep) * 511.0 + 0.5);
            let list = v * params.levels + l;
            if steps >= 511.0 {
                let slot = atomicAdd(&draws[list * 2u], 1u);
                visible[list * params.region + 1u + slot] = i;
            } else if steps > 0.0 {
                // The dither in the entry's top ten bits: nine for the
                // steps, one for fading out.
                let slot = atomicAdd(&draws[list * 2u + 1u], 1u);
                let q = u32(steps) | select(0u, 512u, keep < 0.0);
                visible[list * params.region + params.region - 1u - slot] = i | (q << 22u);
            }
        }
    }
}

@compute @workgroup_size(64)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = id.x;
    let per_view = params.parts * 2u;
    if e >= params.views * per_view {
        return;
    }
    // Per view: every part's draw of the whole characters, then of the dithered.
    let v = e / per_view;
    let dithered = (e % per_view) / params.parts;
    let part = e % params.parts;
    let level = bitcast<u32>(views[params.views * 7u + part / 4u][part % 4u]);
    let count = atomicLoad(&draws[(v * params.levels + level) * 2u + dithered]);
    atomicStore(&draws[params.args + e * 5u + 1u], count);
}
