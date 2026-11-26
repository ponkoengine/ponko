// ---- crowds (see `crowds.rs`) ----
// A crowd draws every character of one (view, level) with two indirect
// draws: `static_indices` holds the visible characters' indices, those
// drawn whole after a leading 0, those drawn dithered (crossfading between
// levels) from the list's back with their dither in the top ten bits;
// `items` their records (six vec4s each: the model matrix's columns, the
// tint, the animation). Each corner is skinned here from the baked
// animation, between its clip's two nearest frames and blended with a
// second clip, before the material's `vertex` runs.
@group(3) @binding(2) var<storage, read> crowd_past: array<vec4<f32>>;
// The clip table (first frame, frames, frames a second, looping), then each
// level's skin matrices: three rows per joint, joints of a frame together.
@group(3) @binding(3) var<storage, read> crowd_anim: array<vec4<f32>>;

struct CrowdLevel {
    // Where this level's matrices start in `crowd_anim`, and joints a frame.
    section: u32,
    slots: u32,
    // Joints moving each corner (1 to 4).
    influences: u32,
    clips: u32,
    // Seconds the clips played on since the records were first drawn, and
    // as of the last frame for its records (`crowd_past`).
    elapsed: f32,
    past_elapsed: f32,
    // Where this draw's entries start in the list, and which way they go
    // (1 for the whole characters, -1 for the dithered ones).
    first: u32,
    step: i32,
};
@group(3) @binding(4) var<uniform> crowd_level: CrowdLevel;

// Clip `clip` at `t` seconds: its two baked frames around it (as indices
// into the whole bake) and how far between them.
fn crowd_frames(clip: u32, t: f32) -> vec3<f32> {
    let c = crowd_anim[min(clip, crowd_level.clips - 1u)];
    let last = max(c.y - 1.0, 0.0);
    var p = t * c.z;
    if c.w > 0.5 && last > 0.0 {
        p = p - floor(p / last) * last;
    } else {
        p = clamp(p, 0.0, last);
    }
    let f0 = min(floor(p), last);
    let f1 = min(f0 + 1.0, last);
    return vec3<f32>(c.x + f0, c.x + f1, p - f0);
}

// This draw's entry for instance `i`: a character's index, and its
// dither above bit 22.
fn crowd_entry(i: u32) -> u32 {
    return static_indices[u32(i32(crowd_level.first) + crowd_level.step * i32(i))];
}

// The crossfade dither of the corner's character (see `fade_keeps`): 0
// drawn whole, else the share of pixels kept, negative fading out.
fn crowd_fade(v: VertexIn) -> f32 {
    let q = crowd_entry(v.instance_index) >> 22u;
    let keep = f32(q & 511u) / 511.0;
    return select(keep, -keep, (q & 512u) != 0u);
}

fn crowd_row(frame: f32, slot: u32, row: u32) -> vec4<f32> {
    return crowd_anim[crowd_level.section + (u32(frame) * crowd_level.slots + slot) * 3u + row];
}

// Word `i` of the records: this frame's (`items`), or the last frame's
// (`crowd_past`) in the motion pass's second run.
fn crowd_record(past: bool, i: u32) -> vec4<f32> {
    if past {
        return crowd_past[i];
    }
    return items[i];
}

// Nothing here writes to a local array or vector at an index known only
// at run time: FXC (Direct3D 12's default shader compiler) rejects such a
// write in a loop it can't unroll ("array reference cannot be used as an
// l-value"). So the record is six values, the sorting network swaps named
// scalars, and the weights past `influences` are masked with a `select`.
fn static_instance(v: VertexIn) -> VertexIn {
    // The motion pass runs this a second time as of the last frame.
    let past = motion_past_time;
    let base = (crowd_entry(v.instance_index) & 0x3fffffu) * 6u;
    let m0 = crowd_record(past, base);
    let m1 = crowd_record(past, base + 1u);
    let m2 = crowd_record(past, base + 2u);
    let m3 = crowd_record(past, base + 3u);
    let tint = crowd_record(past, base + 4u);
    let anim = bitcast<vec4<u32>>(crowd_record(past, base + 5u));
    let blend = f32(anim.w & 0xffffu) / 65535.0;
    let speed = f32(bitcast<i32>(anim.w) >> 16u) / 256.0;
    let elapsed = select(crowd_level.elapsed, crowd_level.past_elapsed, past);
    let a = crowd_frames(anim.x & 0xffffu, bitcast<f32>(anim.y) + speed * elapsed);
    let b = crowd_frames(anim.x >> 16u, bitcast<f32>(anim.z) + speed * elapsed);
    // The strongest `influences` joints of the corner, their weights scaled
    // back to a whole (a sorting network over the four: a bubble sort,
    // heaviest first, equal weights keeping their order).
    var w0 = v.crowd_weights.x;
    var w1 = v.crowd_weights.y;
    var w2 = v.crowd_weights.z;
    var w3 = v.crowd_weights.w;
    var j0 = v.crowd_joints.x;
    var j1 = v.crowd_joints.y;
    var j2 = v.crowd_joints.z;
    var j3 = v.crowd_joints.w;
    var tw = 0.0;
    var tj = 0u;
    // First round: (0, 1), (1, 2), (2, 3); then (0, 1), (1, 2); then (0, 1).
    if w1 > w0 { tw = w0; w0 = w1; w1 = tw; tj = j0; j0 = j1; j1 = tj; }
    if w2 > w1 { tw = w1; w1 = w2; w2 = tw; tj = j1; j1 = j2; j2 = tj; }
    if w3 > w2 { tw = w2; w2 = w3; w3 = tw; tj = j2; j2 = j3; j3 = tj; }
    if w1 > w0 { tw = w0; w0 = w1; w1 = tw; tj = j0; j0 = j1; j1 = tj; }
    if w2 > w1 { tw = w1; w1 = w2; w2 = tw; tj = j1; j1 = j2; j2 = tj; }
    if w1 > w0 { tw = w0; w0 = w1; w1 = tw; tj = j0; j0 = j1; j1 = tj; }
    let kept = vec4<u32>(0u, 1u, 2u, 3u) < vec4<u32>(crowd_level.influences);
    let w = select(vec4<f32>(0.0), vec4<f32>(w0, w1, w2, w3), kept);
    let j = vec4<u32>(j0, j1, j2, j3);
    let total = w.x + w.y + w.z + w.w;
    var r0 = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    var r1 = vec4<f32>(0.0, 1.0, 0.0, 0.0);
    var r2 = vec4<f32>(0.0, 0.0, 1.0, 0.0);
    if total > 0.0 {
        r0 = vec4<f32>(0.0);
        r1 = vec4<f32>(0.0);
        r2 = vec4<f32>(0.0);
        let last = max(crowd_level.slots, 1u) - 1u;
        for (var k = 0u; k < 4u; k++) {
            let weight = w[k] / total;
            if weight <= 0.0 {
                continue;
            }
            let slot = min(j[k], last);
            // Clip a between its frames, clip b between its, then blended.
            let parts = array<vec2<f32>, 4>(
                vec2<f32>(a.x, weight * (1.0 - blend) * (1.0 - a.z)),
                vec2<f32>(a.y, weight * (1.0 - blend) * a.z),
                vec2<f32>(b.x, weight * blend * (1.0 - b.z)),
                vec2<f32>(b.y, weight * blend * b.z),
            );
            for (var q = 0u; q < 4u; q++) {
                let f = parts[q];
                if f.y > 0.0 {
                    r0 += f.y * crowd_row(f.x, slot, 0u);
                    r1 += f.y * crowd_row(f.x, slot, 1u);
                    r2 += f.y * crowd_row(f.x, slot, 2u);
                }
            }
        }
    }
    let p = vec4<f32>(v.position, 1.0);
    var out = v;
    out.position = vec3<f32>(dot(r0, p), dot(r1, p), dot(r2, p));
    let n = vec3<f32>(dot(r0.xyz, v.normal), dot(r1.xyz, v.normal), dot(r2.xyz, v.normal));
    out.normal = select(vec3<f32>(0.0, 1.0, 0.0), normalize(n), dot(n, n) > 1e-20);
    out.m0 = m0;
    out.m1 = m1;
    out.m2 = m2;
    out.m3 = m3;
    out.color = tint;
    return out;
}
