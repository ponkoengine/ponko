// Global illumination (Lighting::gi): light bounced off the world's
// surfaces, from a grid of probes around the camera (see gi.rs).
//
// The probe atlas holds, for each probe slot, an 8x8 tile of light (6x6
// octahedral directions in a one-texel border: rgb is the irradiance over
// pi, a which probe the slot holds) in its top part, and a 16x16 tile of
// distances (14x14 in a border: r the mean distance to what the probe sees
// that way, g its mean square) below. Tiles go 64 to a row.
//
// frame.gi[0]: x 1 in the probes' own views (0 elsewhere), yz unused, w
// the probe spacing; frame.gi[1]: xyz probes along each axis, w the
// intensity (0: off); frame.gi[2]: xyz the grid's lowest probe (in probe
// cells), w the rows of tiles.
@group(0) @binding(10) var gi_atlas: texture_2d<f32>;

// The octahedral map of unit direction `d`: -1 to 1 in each axis.
fn gi_octahedral(d: vec3<f32>) -> vec2<f32> {
    let p = d.xy / (abs(d.x) + abs(d.y) + abs(d.z));
    if d.z >= 0.0 {
        return p;
    }
    return (1.0 - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
}

// Where direction `d` is in a probe's tile of `size` texels (the inside is
// `size - 2`), the tile's corner at `corner`, in atlas uv.
fn gi_tile_uv(corner: vec2<f32>, size: f32, d: vec3<f32>) -> vec2<f32> {
    let texel = corner + 1.0 + (gi_octahedral(d) * 0.5 + 0.5) * (size - 2.0);
    return texel / vec2<f32>(textureDimensions(gi_atlas));
}

// The bounced light reaching a surface at `world` facing `n`, seen from
// direction `view`: rgb, and how much it counts (a, 0 to 1) against the
// sky and ground ambient light. It fades out past the grid's edges and
// where its probes haven't looked around yet.
//
// Not in the probes' own views: there it always counts in full, a probe
// that hasn't looked around yet as dark and the outermost probes' light
// past the grid's edges. Light gets into the probes only from what they
// see (the sky, the sun, lamps), never from the ambient light: the
// ambient light that came in where the probes faded out was outdoor light
// in closed rooms, and a room whose walls send most light on (white ones
// nearly all) kept it for hundreds of bounces. A room lit through a small
// opening came out about as bright as the open sky would make it.
fn gi_light(world: vec3<f32>, n: vec3<f32>, view: vec3<f32>) -> vec4<f32> {
    let intensity = frame.gi[1].w;
    if intensity <= 0.0 {
        return vec4<f32>(0.0);
    }
    let spacing = frame.gi[0].w;
    let counts = frame.gi[1].xyz;
    let lowest = frame.gi[2].xyz;
    let rows = frame.gi[2].w;
    // Looked up a little off the surface, towards the normal and the
    // viewer, so a surface doesn't shadow itself in its probes' distances.
    let p = world + (n * 0.2 + view * 0.1) * spacing;
    let local = p / spacing - lowest;
    let base = clamp(floor(local), vec3<f32>(0.0), counts - 2.0);
    let t = clamp(local - base, vec3<f32>(0.0), vec3<f32>(1.0));
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    var covered = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let offset = vec3<f32>(f32(i & 1u), f32((i >> 1u) & 1u), f32((i >> 2u) & 1u));
        let cell = lowest + base + offset;
        let probe = cell * spacing;
        let corner = mix(1.0 - t, t, offset);
        let trilinear = corner.x * corner.y * corner.z;
        // The probe's slot and tiles (the grid wraps around in its slots).
        let wrap = floor(cell / counts);
        let s = cell - wrap * counts;
        let slot = s.x + counts.x * (s.y + counts.y * s.z);
        let tile = vec2<f32>(slot % 64.0, floor(slot / 64.0));
        let light = textureSampleLevel(gi_atlas, sky_sampler, gi_tile_uv(tile * 8.0, 8.0, n), 0.0);
        let k = wrap - 8.0 * floor(wrap / 8.0);
        if abs(light.a - (1.0 + k.x + 8.0 * k.y + 64.0 * k.z)) > 0.25 {
            continue; // the slot doesn't hold this probe (yet)
        }
        covered += trilinear;
        // Probes behind the surface count less.
        let toward = normalize(probe - world);
        var w = (dot(toward, n) + 1.0) * 0.5;
        w = w * w + 0.2;
        // Only as far as the probe can see the point: Chebyshev's
        // inequality on the distances it saw that way.
        let away = p - probe;
        let r = length(away);
        let depth_uv = gi_tile_uv(vec2<f32>(tile.x * 16.0, rows * 8.0 + tile.y * 16.0), 16.0, away / max(r, 1e-4));
        let moments = textureSampleLevel(gi_atlas, sky_sampler, depth_uv, 0.0).rg;
        if r > moments.x {
            let variance = abs(moments.x * moments.x - moments.y);
            let d = r - moments.x;
            let visible = variance / (variance + d * d);
            w *= max(visible * visible * visible, 0.0);
        }
        w = max(w, 1e-6);
        // Crush tiny weights, so a barely visible probe doesn't tint the point.
        if w < 0.2 {
            w *= w * w / 0.04;
        }
        w *= trilinear;
        sum += light.rgb * w;
        total += w;
    }
    if frame.gi[0].x > 0.5 {
        return vec4<f32>(sum / max(total, 1e-12) * intensity, 1.0);
    }
    if total <= 0.0 {
        return vec4<f32>(0.0);
    }
    // Past the outermost probes, fade out over one spacing.
    let inside = min(local, counts - 1.0 - local);
    let edge = clamp(1.0 + min(inside.x, min(inside.y, inside.z)), 0.0, 1.0);
    return vec4<f32>(sum / total * intensity, edge * clamp(covered, 0.0, 1.0));
}
