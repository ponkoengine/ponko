// Moves a crowd's clock forward (see `crowds.rs`): every character's two
// clip times become what they show `seconds` later, wrapped into a looping
// clip and held at the end of one that holds, so the times the vertex
// shader adds the elapsed time to stay small in a long session.

struct Advance {
    count: u32,
    // Threads per row of the dispatch (it may be two-dimensional).
    row: u32,
    clips: u32,
    seconds: f32,
};

@group(0) @binding(0) var<uniform> advance: Advance;
// Six vec4s a character; the sixth is its animation (see `crowd_vertex.wgsl`).
@group(0) @binding(1) var<storage, read_write> records: array<vec4<u32>>;
// The clip table (first frame, frames, frames a second, looping), then the bake.
@group(0) @binding(2) var<storage, read> anim: array<vec4<f32>>;

// Clip `clip` at `t` seconds, `seconds` on at `speed`: the same pose as
// `crowd_frames` shows for `t + speed * seconds`, now and later.
fn moved(clip: u32, t: f32, speed: f32) -> f32 {
    if speed == 0.0 {
        return t;
    }
    let c = anim[min(clip, advance.clips - 1u)];
    let last = max(c.y - 1.0, 0.0);
    var p = (t + speed * advance.seconds) * c.z;
    if c.w > 0.5 && last > 0.0 {
        p = p - floor(p / last) * last;
    } else if speed > 0.0 {
        // Past the end it only goes further: the end shows the same.
        p = min(p, last);
    } else {
        p = max(p, 0.0);
    }
    return p / c.z;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * advance.row;
    if i >= advance.count {
        return;
    }
    let at = i * 6u + 5u;
    let anim_words = records[at];
    let speed = f32(bitcast<i32>(anim_words.w) >> 16u) / 256.0;
    let a = moved(anim_words.x & 0xffffu, bitcast<f32>(anim_words.y), speed);
    let b = moved(anim_words.x >> 16u, bitcast<f32>(anim_words.z), speed);
    records[at] = vec4<u32>(anim_words.x, bitcast<u32>(a), bitcast<u32>(b), anim_words.w);
}
