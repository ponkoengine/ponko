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
    pad: u32,
    // A sphere around every level's mesh in every baked pose, in model space.
    bounds: vec4<f32>,
    // Per level: nearest distance, farthest (exclusive), drawn in cameras
    // (1) or not (0), drawn in shadow views.
    bands: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Six vec4s a character: its model matrix's columns, tint, animation.
@group(0) @binding(1) var<storage, read> records: array<vec4<f32>>;
// Per view: six planes, then the eye (w: 1 for a shadow view). After the
// views, the level of each indirect draw, four to a vec4 (as bits).
@group(0) @binding(2) var<storage, read> views: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> visible: array<u32>;
// Per (view, level) a count, then the indirect draws (five words each).
@group(0) @binding(4) var<storage, read_write> draws: array<atomic<u32>>;

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
