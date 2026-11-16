// Global illumination's probe update (see gi.rs, gi.wgsl): turns each
// updated probe's six views into its tiles, written to a scratch image the
// engine then copies into the atlas. Appended to the mesh prelude for the
// sky's light (`environment_sample`): of its group 0 only the sky texture and
// sampler are bound.
//
// One workgroup of 8x8 (times 4, see `GI_PARTS`) per 8x8 block of a probe's tiles: block 0 is its
// light tile, blocks 1 to 4 the four quarters of its distance tile.

struct GiProbeUpdate {
    probe: vec4<f32>, // xyz: where the probe is; w: its code (see GiGrid::code)
    tile: vec4<f32>,  // xy: its tile column and row; z: how much old light stays
};

struct GiUpdates {
    list: array<GiProbeUpdate, 16>,
    params: vec4<f32>, // x: the views' near plane, y: far plane, z: rows of tiles, w: view size
};

@group(3) @binding(0) var<uniform> gi_updates: GiUpdates;
// The updated probes' views, side by side (+X, -X, +Y, -Y, +Z, -Z), one
// row per probe, and their depth.
@group(3) @binding(1) var gi_views: texture_2d<f32>;
@group(3) @binding(2) var gi_view_depth: texture_depth_multisampled_2d;
@group(3) @binding(3) var gi_old: texture_2d<f32>;
@group(3) @binding(4) var gi_out: texture_storage_2d<rgba16float, write>;

// View `face`'s direction and up (+X, -X, +Y, -Y, +Z, -Z; the same table
// as `GI_FACES`). Worked out, not looked up: a dynamically indexed `const`
// array is copied to memory at each lookup.
fn gi_forward(face: u32) -> vec3<f32> {
    var forward = vec3<f32>(0.0);
    forward[face / 2u] = select(1.0, -1.0, (face & 1u) == 1u);
    return forward;
}

fn gi_up(face: u32) -> vec3<f32> {
    if face / 2u == 1u {
        return vec3<f32>(0.0, 0.0, select(-1.0, 1.0, (face & 1u) == 1u));
    }
    return vec3<f32>(0.0, 1.0, 0.0);
}

// The unit direction at octahedral coordinates `e` (-1 to 1 each).
fn gi_unoctahedral(e: vec2<f32>) -> vec3<f32> {
    var d = vec3<f32>(e, 1.0 - abs(e.x) - abs(e.y));
    if d.z < 0.0 {
        let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), d.xy >= vec2<f32>(0.0));
        d = vec3<f32>((1.0 - abs(d.yx)) * s, d.z);
    }
    return normalize(d);
}

// The inside texel a tile texel stands for: the border copies the texel
// across the octahedral map's edge, so filtering wraps around it.
fn gi_inside(p: vec2<i32>, size: i32) -> vec2<i32> {
    var q = p;
    if p.y < 0 || p.y >= size {
        q.x = size - 1 - p.x;
    }
    if p.x < 0 || p.x >= size {
        q.y = size - 1 - p.y;
    }
    return clamp(q, vec2<i32>(0), vec2<i32>(size - 1));
}

// Each probe view's width and height (`VIEW_SIZE` in gi.rs), and the
// texels of a probe's six views.
const GI_VIEW: u32 = 16u;
const GI_TEXELS: u32 = 6u * GI_VIEW * GI_VIEW;

// The probe's six views, loaded once per workgroup: rg and b of the light
// seen that way, and the distance to it, as half floats.
var<workgroup> gi_seen: array<vec2<u32>, GI_TEXELS>;
// Each tile texel's sum is split between 4 threads (a quarter of each
// view's rows each, so the GPU has enough threads to keep busy); threads
// 1 to 3 leave theirs here for thread 0.
const GI_PARTS: u32 = 4u;
var<workgroup> gi_partial: array<vec4<f32>, 192>;

// The direction view texel `i` looks in, unnormalized (its length squared
// tells its share of the sphere).
fn gi_ray(i: u32) -> vec3<f32> {
    let face = i / (GI_VIEW * GI_VIEW);
    let at = i % (GI_VIEW * GI_VIEW);
    let uv = (vec2<f32>(f32(at % GI_VIEW), f32(at / GI_VIEW)) + 0.5) / f32(GI_VIEW) * 2.0 - 1.0;
    let forward = gi_forward(face);
    let up = gi_up(face);
    return forward + cross(forward, up) * uv.x - up * uv.y;
}

@compute @workgroup_size(8, 8, 4)
fn cs_update(
    @builtin(workgroup_id) block: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
    @builtin(local_invocation_index) thread: u32,
) {
    let update = gi_updates.list[block.z];
    let near = gi_updates.params.x;
    let far = gi_updates.params.y;
    let rows = gi_updates.params.z;
    // First the views into workgroup memory, 6 texels per thread: the
    // sums below then read nothing from textures.
    for (var i = thread; i < GI_TEXELS; i += 64u * GI_PARTS) {
        let face = i / (GI_VIEW * GI_VIEW);
        let at_view = i % (GI_VIEW * GI_VIEW);
        let at = vec2<i32>(
            i32(face * GI_VIEW + at_view % GI_VIEW),
            i32(block.z * GI_VIEW + at_view / GI_VIEW),
        );
        let ray = gi_ray(i);
        let length1 = length(ray);
        let depth = textureLoad(gi_view_depth, at, 0);
        var radiance = textureLoad(gi_views, at, 0).rgb;
        if depth <= 0.0 {
            // Nothing there: the sky.
            radiance = environment_sample(0.0, ray / length1, 0.3, false);
        }
        // Distance along the view (reverse-Z: 1 near, 0 far), then along
        // the ray.
        let along = near * far / max(near + depth * (far - near), 1e-6);
        let distance = min(along * length1, far);
        gi_seen[i] = vec2<u32>(pack2x16float(radiance.rg), pack2x16float(vec2<f32>(radiance.b, distance)));
    }
    workgroupBarrier();

    let light = block.x == 0u;
    var texel = vec2<i32>(local.xy);
    var size = 8;
    if !light {
        let quarter = i32(block.x) - 1;
        texel += vec2<i32>(quarter % 2, quarter / 2) * 8;
        size = 16;
    }
    let inside = size - 2;
    let e = (vec2<f32>(gi_inside(texel - 1, inside)) + 0.5) / f32(inside) * 2.0 - 1.0;
    let dir = gi_unoctahedral(e);
    var sum = vec4<f32>(0.0);
    let part = local.z;
    let rows_each = GI_VIEW / GI_PARTS;
    for (var face = 0u; face < 6u; face++) {
        let forward = gi_forward(face);
        let up = gi_up(face);
        let right = cross(forward, up);
        for (var y = part * rows_each; y < (part + 1u) * rows_each; y++) {
            let v = (f32(y) + 0.5) / f32(GI_VIEW) * 2.0 - 1.0;
            var i = (face * GI_VIEW + y) * GI_VIEW;
            for (var x = 0u; x < GI_VIEW; x++) {
                let u = (f32(x) + 0.5) / f32(GI_VIEW) * 2.0 - 1.0;
                let ray = forward + right * u - up * v;
                let seen = gi_seen[i];
                i++;
                let length2 = dot(ray, ray);
                let length1 = sqrt(length2);
                let c = dot(dir, ray) / length1;
                if c <= 0.0 {
                    continue;
                }
                // Each texel's share of the sphere (times 4 / views²).
                let solid = 1.0 / (length2 * length1);
                if light {
                    let rgb = vec3<f32>(unpack2x16float(seen.x), unpack2x16float(seen.y).x);
                    sum += vec4<f32>(rgb * (c * solid), 0.0);
                } else {
                    let w = pow(c, 50.0) * solid;
                    if w > 1e-7 {
                        let distance = unpack2x16float(seen.y).y;
                        sum += vec4<f32>(distance * w, distance * distance * w, 0.0, w);
                    }
                }
            }
        }
    }
    let own = local.x + 8u * local.y;
    if part > 0u {
        gi_partial[(part - 1u) * 64u + own] = sum;
    }
    workgroupBarrier();
    if part > 0u {
        return;
    }
    for (var k = 0u; k < GI_PARTS - 1u; k++) {
        sum += gi_partial[k * 64u + own];
    }
    let column = update.tile.x;
    let row = update.tile.y;
    var result: vec4<f32>;
    var old_at: vec2<i32>;
    var out_at = vec2<i32>(0, i32(block.z) * 16) + texel;
    if light {
        // Irradiance over pi: the sum times each texel's 4 / views² of the sphere.
        let irradiance = sum.rgb * 4.0 / (f32(GI_VIEW * GI_VIEW) * PI);
        result = vec4<f32>(irradiance, update.probe.w);
        old_at = vec2<i32>(i32(column) * 8, i32(row) * 8) + texel;
    } else {
        let moments = sum.xy / max(sum.w, 1e-12);
        result = vec4<f32>(moments, 0.0, 1.0);
        old_at = vec2<i32>(i32(column) * 16, i32(rows) * 8 + i32(row) * 16) + texel;
        out_at.x += 8;
    }
    let keep = update.tile.z;
    if keep > 0.0 {
        let old = textureLoad(gi_old, old_at, 0);
        result = vec4<f32>(mix(result.rgb, old.rgb, keep), result.a);
    }
    textureStore(gi_out, out_at, result);
}

