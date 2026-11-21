// Global illumination's probe update (see gi.rs, gi.wgsl): turns each
// updated probe's six views into its tiles, written to a scratch image the
// engine then copies into the atlas. Appended to the mesh prelude for the
// sky's light (`environment_sample`): of its group 0 only the sky texture and
// sampler are bound.
//
// One workgroup of 8x8 (times 4, see `GI_PARTS`) per 8x8 block of a probe's tiles: block 0 is its
// light tile, blocks 1 to 4 the four quarters of its distance tile.
//
// Block 0 also writes the probe's settled light tile, which the camera's
// view reads (the probes' own views read the light tile): light bouncing
// between bright walls gains less with every bounce, and each update is
// one bounce, so the light tile creeps towards where it settles over a
// hundred updates in a white room. The settled tile skips ahead: while the
// probe's light keeps changing by the same fraction of its last change
// (`gain`, below 1), what is still to come is the last change times
// gain / (1 - gain) (Aitken's extrapolation of a geometric series). Only
// the camera's view takes it: fed back into the bounces it would
// overshoot.

struct GiProbeUpdate {
    probe: vec4<f32>, // xyz: where the probe is; w: its code (see GiGrid::code)
    tile: vec4<f32>,  // xy: its tile column and row; z: how much old light stays; w: its slot
};

struct GiUpdates {
    list: array<GiProbeUpdate, 16>,
    params: vec4<f32>, // x: the views' near plane, y: far plane, z: rows of tiles, w: view size
    grid: vec4<f32>,   // xyz: probes along each axis, w: the spacing
};

@group(3) @binding(0) var<uniform> gi_updates: GiUpdates;
// The updated probes' views, side by side (+X, -X, +Y, -Y, +Z, -Z), one
// row per probe, and their depth.
@group(3) @binding(1) var gi_views: texture_2d<f32>;
@group(3) @binding(2) var gi_view_depth: texture_depth_multisampled_2d;
@group(3) @binding(3) var gi_old: texture_2d<f32>;
@group(3) @binding(4) var gi_out: texture_storage_2d<rgba16float, write>;
// Each slot's history: x its light tile's last change of brightness (the
// sum over its inside texels; 0: none yet), y the gain that change was
// measured at (0: none), z the brightness its light is expected to settle
// at (0: none yet).
@group(3) @binding(5) var<storage, read_write> gi_history: array<vec4<f32>>;

// The most a probe's light may gain from one update to the next where its
// settled light is worked out: at most 1 / (1 - GI_MAX_GAIN) times the
// last change is added.
const GI_MAX_GAIN: f32 = 0.95;
// Changes smaller than this fraction of the light are too small to tell a
// gain from (the light tiles are half floats, and the shadows the views
// are lit by flicker a little): the probe's guess of where it settles is
// held instead, fading towards its light (`GI_FADE`).
const GI_SETTLED: f32 = 0.01;
// How much of its last guess of where it settles a probe keeps when it
// makes a new one.
const GI_HOLD: f32 = 0.5;
// How far a held guess fades towards the light at each update: slowly while
// the light still rises towards it, fast when the light falls.
const GI_FADE: vec2<f32> = vec2<f32>(0.02, 0.5);

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
// Block 0's texels' brightness, new and old (inside texels only), and the
// factor its last change is added with for the settled tile.
var<workgroup> gi_brightness: array<vec2<f32>, 64>;
var<workgroup> gi_ahead: f32; // the settled tile over the light tile
var<workgroup> gi_buried: bool;

// A probe inside a wall thinner than the views' near plane, or thicker but
// seen from inside (its back faces are culled), sees through it into the
// rooms on both sides, and would light each with the other's light: its
// own views can't tell. Its neighbours can: when both neighbours along one
// axis see a surface just before reaching the probe (closer than the
// spacing by between GI_BURIED.x and GI_BURIED.y of it), the probe sits in
// a wall between them. Such a probe is marked (its code negated) and gives
// no light.
//
// What a neighbour sees that way is told by its distance tile's texel for
// it, which sums a cone of directions some 12 degrees wide: near the top
// of a wall, part of that cone passes over the wall and sees the far room,
// which lifts the mean distance past the band though most of the cone
// stops at the wall. So each distance texel also keeps (in b) how much of
// its cone sees a surface within the band, and the probe counts as seen
// that way when at least GI_BURIED_SHARE of the cone does, or the mean is
// in the band.
const GI_BURIED: vec2<f32> = vec2<f32>(-0.03, 0.25);
const GI_BURIED_SHARE: f32 = 0.5;

// What tells which probe a slot holds (GiGrid::code).
fn gi_code(cell: vec3<f32>, counts: vec3<f32>) -> f32 {
    let wrap = floor(cell / counts);
    let k = wrap - 8.0 * floor(wrap / 8.0);
    return 1.0 + k.x + 8.0 * k.y + 64.0 * k.z;
}

// Whether the probe at `cell`, if a slot holds it (and it isn't marked
// itself), sees a surface towards `toward` (a unit axis) before going
// `spacing` that way, near its end (see GI_BURIED).
fn gi_blocked(cell: vec3<f32>, toward: vec3<f32>) -> bool {
    let counts = gi_updates.grid.xyz;
    let spacing = gi_updates.grid.w;
    let rows = gi_updates.params.z;
    let wrap = floor(cell / counts);
    let s = cell - wrap * counts;
    let slot = s.x + counts.x * (s.y + counts.y * s.z);
    let tile = vec2<i32>(i32(slot % 64.0), i32(floor(slot / 64.0)));
    let code = textureLoad(gi_old, tile * 8 + vec2<i32>(1), 0).a;
    if abs(code - gi_code(cell, counts)) > 0.25 {
        return false;
    }
    // The distance tile's four texels around that way (its inside is 14
    // across): an axis falls on their corner, so the cone they make up is
    // centred on it, where any one of them leans to one side (and a probe
    // just inside a wall's side edge, or just under a wall in open air,
    // was taken for the other).
    let corner = vec2<i32>(floor((gi_octahedral(toward) * 0.5 + 0.5) * 14.0 - 0.5));
    let origin = vec2<i32>(tile.x * 16, i32(rows) * 8 + tile.y * 16) + 1;
    var seen = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        seen += textureLoad(gi_old, origin + gi_inside(corner + vec2<i32>(k & 1, k >> 1), 14), 0);
    }
    seen *= 0.25;
    let mean = seen.r;
    return (mean < spacing * (1.0 - GI_BURIED.x) && mean > spacing * (1.0 - GI_BURIED.y)) || seen.b >= GI_BURIED_SHARE;
}

// The octahedral map of unit direction `d` (as gi.wgsl's).
fn gi_octahedral(d: vec3<f32>) -> vec2<f32> {
    let p = d.xy / (abs(d.x) + abs(d.y) + abs(d.z));
    if d.z >= 0.0 {
        return p;
    }
    return (1.0 - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
}

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

// Where a view pixel's samples see surfaces this much further apart (as a
// fraction of the distance), one surface hides part of another there.
const GI_EDGE: f32 = 0.02;

// The light of view pixel `at` (`depth` its first sample's, `radiance` what
// it resolved to), from the surface in front where one hides another
// there. With multisampling a triangle is shaded once per pixel, at the
// pixel's center, for all the samples it covers; where it is partly hidden,
// that center can lie on its hidden part, on the far side of what hides it
// (a room's long wall seen past the end of a thin partition: its hidden
// part is in the next room, lit by that room's lamp and probes). Such a
// pixel's light came from somewhere the probe can't see, and a white room
// passes it on bounce after bounce. Instead it takes the light of the
// neighbouring pixel (in the same view) whose surface is nearest the
// pixel's front sample.
fn gi_front_light(at: vec2<i32>, depth: f32, radiance: vec3<f32>, near: f32, far: f32) -> vec3<f32> {
    let samples = i32(textureNumSamples(gi_view_depth));
    var front = depth;
    var back = depth;
    for (var k = 1; k < samples; k++) {
        let d = textureLoad(gi_view_depth, at, k);
        front = max(front, d);
        back = min(back, d);
    }
    // Reverse-Z: larger is nearer. Against the sky, nothing is hidden.
    if back <= 0.0 {
        return radiance;
    }
    let front_along = gi_along(front, near, far);
    if gi_along(back, near, far) <= front_along * (1.0 + GI_EDGE) {
        return radiance;
    }
    // The view's pixels around it, inside the view.
    let corner = (at / i32(GI_VIEW)) * i32(GI_VIEW);
    var best = radiance;
    var closest = 1e30;
    for (var k = 0; k < 4; k++) {
        var step = vec2<i32>(0, 0);
        step[k / 2] = select(-1, 1, (k & 1) == 1);
        let q = at + step;
        if any(q < corner) || any(q >= corner + vec2<i32>(i32(GI_VIEW))) {
            continue;
        }
        let d = textureLoad(gi_view_depth, q, 0);
        if d <= 0.0 {
            continue;
        }
        let off = abs(gi_along(d, near, far) - front_along);
        if off < closest {
            closest = off;
            best = textureLoad(gi_views, q, 0).rgb;
        }
    }
    return best;
}

// Distance along a view at depth `d` (reverse-Z: 1 near, 0 far).
fn gi_along(d: f32, near: f32, far: f32) -> f32 {
    return near * far / max(near + d * (far - near), 1e-6);
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
        } else {
            radiance = gi_front_light(at, depth, radiance, near, far);
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
    // The distances that tell a neighbour sits in a wall (see GI_BURIED).
    let band = gi_updates.grid.w * (1.0 - GI_BURIED);
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
                        let in_band = distance < band.x && distance > band.y;
                        sum += vec4<f32>(distance * w, distance * distance * w, select(0.0, w, in_band), w);
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
    let column = update.tile.x;
    let row = update.tile.y;
    let keep = update.tile.z;
    var result = vec4<f32>(0.0);
    var old = vec4<f32>(0.0);
    var old_at: vec2<i32>;
    var out_at = vec2<i32>(0, i32(block.z) * 16) + texel;
    if part == 0u {
        for (var k = 0u; k < GI_PARTS - 1u; k++) {
            sum += gi_partial[k * 64u + own];
        }
        if light {
            // Irradiance over pi: the sum times each texel's 4 / views² of the sphere.
            let irradiance = sum.rgb * 4.0 / (f32(GI_VIEW * GI_VIEW) * PI);
            result = vec4<f32>(irradiance, update.probe.w);
            old_at = vec2<i32>(i32(column) * 8, i32(row) * 8) + texel;
        } else {
            let moments = sum.xy / max(sum.w, 1e-12);
            result = vec4<f32>(moments, sum.z / max(sum.w, 1e-12), 1.0);
            old_at = vec2<i32>(i32(column) * 16, i32(rows) * 8 + i32(row) * 16) + texel;
            out_at.x += 8;
        }
        if keep > 0.0 {
            old = textureLoad(gi_old, old_at, 0);
            result = vec4<f32>(mix(result.rgb, old.rgb, keep), result.a);
        }
        if light {
            let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
            let is_inside = all(texel >= vec2<i32>(1)) && all(texel <= vec2<i32>(6));
            gi_brightness[own] = select(vec2<f32>(0.0), vec2<f32>(dot(result.rgb, luma), dot(old.rgb, luma)), is_inside);
        }
    }
    // `light` is the same across the workgroup: these barriers are reached
    // by all of its threads or none.
    if light {
        workgroupBarrier();
        if thread == 0u {
            var now = 0.0;
            var before = 0.0;
            for (var k = 0u; k < 64u; k++) {
                now += gi_brightness[k].x;
                before += gi_brightness[k].y;
            }
            let slot = u32(update.tile.w);
            let last = gi_history[slot];
            var change = 0.0;
            var gain = 0.0;
            var expected = now;
            if keep > 0.0 {
                // The same probe as last time: how much its light changed,
                // against how much it changed the time before.
                change = now - before;
                if last.x != 0.0 {
                    gain = max(change / last.x, 0.0);
                }
                // Only once two gains in a row say so (the smaller counts):
                // while light first spreads through a room, one update's
                // jump says little about the next.
                let g = min(min(gain, last.y), GI_MAX_GAIN);
                if abs(change) > GI_SETTLED * now {
                    var guess = now;
                    if g > 0.0 {
                        guess = now + change * g / (1.0 - g);
                    }
                    // The gain is a ratio of two small changes, so each
                    // guess is rough: it is averaged with the last one,
                    // unless the light went the other way (then the last
                    // guess is stale).
                    expected = guess;
                    if g > 0.0 && last.z > 0.0 && (change >= 0.0) == (last.z >= before) {
                        expected = mix(guess, last.z, GI_HOLD);
                    }
                } else if last.z > 0.0 {
                    expected = mix(last.z, now, select(GI_FADE.y, GI_FADE.x, change >= 0.0 && last.z >= now));
                }
                if change >= 0.0 {
                    // Rising light settles above where it is.
                    expected = max(expected, now);
                }
            }
            gi_history[slot] = vec4<f32>(change, gain, expected, 0.0);
            let cell = round(update.probe.xyz / gi_updates.grid.w);
            var buried = false;
            for (var axis = 0u; axis < 3u; axis++) {
                var step = vec3<f32>(0.0);
                step[axis] = 1.0;
                buried = buried || (gi_blocked(cell + step, -step) && gi_blocked(cell - step, step));
            }
            gi_buried = buried;
            gi_ahead = clamp(expected / max(now, 1e-6), 0.0, 1.0 / (1.0 - GI_MAX_GAIN));
        }
        workgroupBarrier();
    }
    if part > 0u {
        return;
    }
    if light && gi_buried {
        result.a = -result.a;
    }
    textureStore(gi_out, out_at, result);
    if light {
        // The settled tile, beside the light tiles' columns in the scratch image.
        let settled = result.rgb * gi_ahead;
        textureStore(gi_out, out_at + vec2<i32>(24, 0), vec4<f32>(settled, result.a));
    }
}
