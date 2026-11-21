// Crowd culling (see `crowds.rs`): one thread per character tests its
// bounding sphere against every view of the frame and its origin's distance
// against each level's band, and appends it to the visible list of each
// (view, level) that draws it. `finish` then writes the counts into the
// indirect draws, one per (view, level, mesh part).

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
    // A sphere around every level's mesh in every baked pose, in model space.
    bounds: vec4<f32>,
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
    pad0: u32,
    pad1: u32,
    // Per level: nearest distance, farthest (exclusive), drawn in cameras
    // (1) or not (0), drawn in shadow views.
    bands: array<vec4<f32>, 8>,
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
// the probes global illumination updates this frame.
@group(0) @binding(2) var<storage, read> views: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> visible: array<u32>;
// Per (view, level) a count, then the indirect draws (five words each).
@group(0) @binding(4) var<storage, read_write> draws: array<atomic<u32>>;
@group(0) @binding(5) var<uniform> grid: Grid;

fn grid_depth(i: u32) -> f32 {
    return grid.depths[i / 4u][i % 4u];
}

// Whether the depth grid hides the sphere: its box's nearest depth is
// behind the farthest depth of every cell it covers (reverse-Z: nearer is
// greater). As the instance batches' test (`instances.wgsl`).
fn hidden(center: vec3<f32>, radius: f32) -> bool {
    let extent = vec3<f32>(radius + grid.head.z);
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

// Whether the sphere is at least one pixel wide seen from one of this
// frame's probes.
fn probe_sized(center: vec3<f32>, radius: f32) -> bool {
    for (var k = 0u; k < params.probes; k++) {
        if radius >= params.cut * distance(center, views[params.probes_at + k].xyz) {
            return true;
        }
    }
    return false;
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
    let b = params.bounds;
    let center = m0 * b.x + m1 * b.y + m2 * b.z + origin;
    let radius = b.w * length(vec3<f32>(length(m0), length(m1), length(m2)));
    for (var v = 0u; v < params.views; v++) {
        let at = v * 7u;
        var inside = true;
        for (var p = 0u; p < 6u; p++) {
            let plane = views[at + p];
            if dot(plane.xyz, center) + plane.w < -radius {
                inside = false;
                break;
            }
        }
        if !inside {
            continue;
        }
        if v == 0u && params.occlusion != 0u && hidden(center, radius) {
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
            let kind = select(band.z, band.w, shadow) > 0.5;
            if kind && far >= band.x && far < band.y {
                let list = v * params.levels + l;
                let slot = atomicAdd(&draws[list], 1u);
                visible[list * params.region + 1u + slot] = i;
            }
        }
    }
}

@compute @workgroup_size(64)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = id.x;
    if e >= params.views * params.parts {
        return;
    }
    let v = e / params.parts;
    let part = e % params.parts;
    let level = bitcast<u32>(views[params.views * 7u + part / 4u][part % 4u]);
    let count = atomicLoad(&draws[v * params.levels + level]);
    atomicStore(&draws[params.args + e * 5u + 1u], count);
}
