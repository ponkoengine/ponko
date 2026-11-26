//@@ponko-mesh-prelude@@
@group(3) @binding(0) var<storage, read> items: array<vec4<f32>>;
@group(3) @binding(1) var<storage, read> static_indices: array<u32>;

// The element of `items` the game's shader reads: as of the last frame
// while the motion pass runs `vertex` for it (a buffer that keeps its last
// frame has that copy past its live elements, at `static_indices[0]`).
fn motion_item(i: u32) -> u32 {
    if motion_past_items {
        return i + static_indices[0];
    }
    return i;
}

// Projected decals (Decal3D): boxes whose image is blended into the surfaces
// inside them before lighting, so they are lit like the surface they land on.
// Each frame the engine puts the decals in the lights' froxel grid: a second
// cell table (offset and count per cell) starts `frame.cluster_depth.w` words
// into the cluster texture, and each decal is eight texels of the lights' record
// texture, starting at the row the cell lists give.
@group(0) @binding(8) var decal_array: texture_2d_array<f32>;
@group(0) @binding(9) var decal_sampler: sampler;

// The most decals one cell blends (the engine keeps at most this many per cell).
const MAX_CELL_DECALS: u32 = 64u;

fn decal_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

// `s` with the decals in its froxel blended in. `geometric` is the unit
// normal of the surface itself (without a normal map), `layers` the decal
// layers its model takes and `dp_x`, `dp_y` the screen derivatives of its
// world position (taken in uniform control flow by the caller).
fn apply_decals(s_in: Surface, geometric: vec3<f32>, layers: u32, dp_x: vec3<f32>, dp_y: vec3<f32>) -> Surface {
    var s = s_in;
    if frame.cluster_depth.z <= 0.0 || layers == 0u { return s; }
    let found = cluster_cell(s.world);
    if found < 0 { return s; }
    let table = u32(frame.cluster_depth.w) + u32(found) * 2u;
    let offset = cluster_word(table);
    let count = min(cluster_word(table + 1u), MAX_CELL_DECALS);
    let texel = 0.5 / vec2<f32>(textureDimensions(decal_array));
    for (var i = 0u; i < count; i++) {
        let id = i32(cluster_word(offset + i));
        let images = textureLoad(punctual_records, vec2<i32>(5, id), 0);
        if (u32(images.w) & layers) == 0u { continue; }
        let r0 = textureLoad(punctual_records, vec2<i32>(0, id), 0);
        let r1 = textureLoad(punctual_records, vec2<i32>(1, id), 0);
        let r2 = textureLoad(punctual_records, vec2<i32>(2, id), 0);
        // Where the surface is in the box: -0.5 to 0.5 on each axis.
        let at = vec3<f32>(dot(r0.xyz, s.world) + r0.w, dot(r1.xyz, s.world) + r1.w, dot(r2.xyz, s.world) + r2.w);
        if any(abs(at) > vec3<f32>(0.5)) { continue; }
        let fades = textureLoad(punctual_records, vec2<i32>(6, id), 0);
        // The box's axes in the world: the rows of the inverse are axes over scales.
        let x_axis = normalize(r0.xyz);
        let y_axis = normalize(r1.xyz);
        let z_axis = normalize(r2.xyz);
        // Surfaces turned away from the projection fade out, so the image doesn't stretch.
        var weight = smoothstep(fades.y, max(fades.x, fades.y + 1e-4), dot(geometric, z_axis));
        // And so does each end of the box's depth.
        if fades.z > 0.0 {
            weight *= smoothstep(0.0, 1.0, (0.5 - abs(at.z)) / (0.5 * fades.z));
        }
        if weight <= 0.0 { continue; }
        let tint = textureLoad(punctual_records, vec2<i32>(3, id), 0);
        let rect = textureLoad(punctual_records, vec2<i32>(4, id), 0);
        let size = rect.zw - rect.xy;
        let uv = rect.xy + size * vec2<f32>(at.x + 0.5, 0.5 - at.y);
        // The image's rectangle shrunk by half a texel, so atlas neighbours don't leak in.
        let low = min(rect.xy, rect.zw) + texel;
        let high = max(rect.xy, rect.zw) - texel;
        let uv_at = clamp(uv, low, max(low, high));
        // Scaled by the mip bias, as the material's own textures are.
        let slope_x = vec2<f32>(dot(r0.xyz, dp_x), -dot(r1.xyz, dp_x)) * size * exp2(texture_mip_bias());
        let slope_y = vec2<f32>(dot(r0.xyz, dp_y), -dot(r1.xyz, dp_y)) * size * exp2(texture_mip_bias());
        var color = vec3<f32>(1.0);
        var alpha = 1.0;
        if images.x >= 0.0 {
            let image = textureSampleGrad(decal_array, decal_sampler, uv_at, i32(images.x), slope_x, slope_y);
            color = decal_srgb_to_linear(image.rgb);
            alpha = image.a;
        }
        let amount = alpha * tint.w * weight;
        if amount <= 0.0 { continue; }
        s.albedo = mix(s.albedo, color * tint.rgb, amount);
        if images.y >= 0.0 {
            var m = textureSampleGrad(decal_array, decal_sampler, uv_at, i32(images.y), slope_x, slope_y).xyz * 2.0 - 1.0;
            m = vec3<f32>(m.xy * images.z, m.z);
            // The map's own tilt, added to the surface's normal.
            let tilted = x_axis * m.x + y_axis * m.y + z_axis * m.z - z_axis;
            s.normal = normalize(mix(s.normal, normalize(s.normal + tilted), amount));
        }
        let surface = textureLoad(punctual_records, vec2<i32>(7, id), 0);
        if surface.x >= 0.0 {
            s.roughness = mix(s.roughness, surface.x, amount);
        }
        if surface.y >= 0.0 {
            s.metallic = mix(s.metallic, surface.y, amount);
        }
    }
    return s;
}
// Global illumination (Lighting::gi): light bounced off the world's
// surfaces, from a grid of probes around the camera (see gi.rs).
//
// The probe atlas holds, for each probe slot, an 8x8 tile of light (6x6
// octahedral directions in a one-texel border: rgb is the irradiance over
// pi, a which probe the slot holds) in its top part, and a 16x16 tile of
// distances (14x14 in a border: r the mean distance to what the probe sees
// that way, g its mean square, b how much of that sees a surface just
// before the next probe, which finds probes buried in walls; see
// gi_update.wgsl) below. Tiles go 64 to a row. The light
// tiles fill the left half of their rows; in the right half, beside each,
// is its settled tile (the same layout: the light extrapolated to where
// its bounces settle, see gi_update.wgsl), which the camera's view reads.
// The probes' own views read the light tiles, so each bounce starts from
// the light as it is.
//
// frame.gi[0]: x 1 in the probes' own views (0 elsewhere), y 1 to read
// the light tiles in the camera's view too (tests), z unused, w the probe
// spacing; frame.gi[1]: xyz probes along each axis, w the
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
    // The settled tiles (64 tiles of 8 texels to the right), except in the
    // probes' own views.
    let settled = vec2<f32>(select(512.0, 0.0, frame.gi[0].x > 0.5 || frame.gi[0].y > 0.5), 0.0);
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
        let light = textureSampleLevel(gi_atlas, sky_sampler, gi_tile_uv(tile * 8.0 + settled, 8.0, n), 0.0);
        let k = wrap - 8.0 * floor(wrap / 8.0);
        if abs(abs(light.a) - (1.0 + k.x + 8.0 * k.y + 64.0 * k.z)) > 0.25 {
            continue; // the slot doesn't hold this probe (yet)
        }
        covered += trilinear;
        if light.a < 0.0 {
            continue; // in a wall: it sees both sides (see gi_update.wgsl)
        }
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
// Baked lighting (BakedLighting, see three/bake): indirect light worked out
// ahead of time, from a lightmap for models with a Lightmapped component and
// from a probe volume for the others.
//
// Its header is the material's third texture array (custom_array2) when the
// material sets none there. Layer 0: texel 0 is (-1, pages, 1 when
// directional, intensity) (no array of images holds a negative value);
// texels 1 and 2 the probe grid's lowest probe in two parts (and its
// spacing in 1.w); texel 3 its probes along x, y, z and the row its probes
// start at (x 0: no probes); texel 4 the pages' size and layers. From
// texel 8, two texels per lightmap entry: (x, y, width, height) of its
// rectangle in texels, then (page, ...). Each probe is three texels:
// irradiance over pi toward normal n is c + cx * n.x + cy * n.y + cz * n.z,
// laid out as (c.rgb, cx.r), (cx.gb, cy.rg), (cy.b, cz.rgb).
//
// The pages are the second texture array (custom_array1) when the material
// sets none there (else its lightmapped models take the probes' light):
// layers 0 to pages - 1 hold the pages' light (irradiance over pi), the
// next ones (when directional) the mean direction the light comes from as
// d * 0.5 + 0.5, its length how much it comes from that way. Half floats,
// BC6H blocks or the shared-exponent format: the sampler decodes them.

// The entry plus one of the model this pixel is of (0: none), and its
// surface's own normal (before normal maps): set by the entry points.
var<private> baked_entry: f32 = 0.0;
var<private> baked_face: vec3<f32> = vec3<f32>(0.0);

// Texel `texel` (in texels, from the top left) of page layer `layer`,
// filtered.
fn baked_page(layer: i32, texel: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(custom_array1));
    return textureSampleLevel(custom_array1, sky_sampler, texel / size, layer, 0.0).rgb;
}

// Texel `i` of the header, counted row by row (read at its center: exact).
fn baked_at(i: f32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(custom_array2));
    let row = floor(i / size.x);
    let texel = vec2<f32>(i - row * size.x, row) + 0.5;
    return textureSampleLevel(custom_array2, sky_sampler, texel / size, 0, 0.0);
}

// Whether the second texture array holds the pages the header describes
// (not one the material set there itself).
fn baked_has_pages() -> bool {
    let pages = baked_at(4.0);
    return pages.y > 0.5 && f32(textureDimensions(custom_array1).x) == pages.x
        && f32(textureNumLayers(custom_array1)) == max(pages.y, 2.0);
}

// The baked indirect light at a surface at `world` facing `n` with
// lightmap coordinates `uv1`: irradiance over pi (rgb), and how much it
// replaces the ambient light (a). Zero without baked lighting.
fn baked_light(world: vec3<f32>, n: vec3<f32>, uv1: vec2<f32>) -> vec4<f32> {
    let header = baked_at(0.0);
    if header.x >= 0.0 {
        return vec4<f32>(0.0);
    }
    if baked_entry > 0.5 && baked_has_pages() {
        let at = 8.0 + 2.0 * (baked_entry - 1.0);
        let rect = baked_at(at);
        if rect.z > 0.0 {
            let page = baked_at(at + 1.0).x;
            // Filtered within the rectangle, never past its edge texels.
            let texel = clamp(rect.xy + uv1 * rect.zw, rect.xy + 0.5, rect.xy + rect.zw - 0.5);
            var light = baked_page(i32(page), texel);
            if header.z > 0.5 {
                // Normal maps tilt toward or away from where the light came
                // from; the surface's own normal gets the baked value.
                let d = baked_page(i32(header.y + page), texel) * 2.0 - 1.0;
                let face = select(n, baked_face, dot(baked_face, baked_face) > 0.5);
                light *= clamp(max(dot(n, d) + 0.5, 0.0) / max(dot(face, d) + 0.5, 0.05), 0.0, 3.0);
            }
            return vec4<f32>(light * header.w, 1.0);
        }
    }
    let counts = baked_at(3.0);
    if counts.x < 2.0 {
        return vec4<f32>(0.0);
    }
    let low = baked_at(1.0);
    let origin = low.xyz + baked_at(2.0).xyz;
    let spacing = low.w;
    // Looked up a little off the surface, so it doesn't take the light of
    // probes behind it.
    let local = (world + n * (0.25 * spacing) - origin) / spacing;
    let base = clamp(floor(local), vec3<f32>(0.0), counts.xyz - 2.0);
    let t = clamp(local - base, vec3<f32>(0.0), vec3<f32>(1.0));
    let first_row = counts.w * f32(textureDimensions(custom_array2).x);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let o = vec3<f32>(f32(i & 1u), f32((i >> 1u) & 1u), f32((i >> 2u) & 1u));
        let cell = base + o;
        let corner = mix(1.0 - t, t, o);
        // Probes behind the surface count less.
        let away = origin + cell * spacing - world;
        var w = (dot(away / max(length(away), 1e-4), n) + 1.0) * 0.5;
        w = (w * w + 0.2) * corner.x * corner.y * corner.z;
        let at = first_row + 3.0 * (cell.x + counts.x * (cell.y + counts.y * cell.z));
        let a = baked_at(at);
        let b = baked_at(at + 1.0);
        let c = baked_at(at + 2.0);
        let e = a.xyz + vec3<f32>(a.w, b.x, b.y) * n.x + vec3<f32>(b.z, b.w, c.x) * n.y + c.yzw * n.z;
        sum += max(e, vec3<f32>(0.0)) * w;
        total += w;
    }
    // Past the outermost probes, fade out over one spacing.
    let inside = min(local, counts.xyz - 1.0 - local);
    let edge = clamp(1.0 + min(inside.x, min(inside.y, inside.z)), 0.0, 1.0);
    return vec4<f32>(sum / max(total, 1e-6) * header.w, edge);
}

// ---- the game's shader ----
//@@ponko-game@@
// ---- the engine's entry points ----
fn vertex(v: Vertex) -> Vertex { return v; }
fn surface(s: Surface) -> Surface { return s; }
// The entry points of every material shader (see mesh.wgsl).

struct VertexIn {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,  // the model's tint
    @location(8) vertex_color: vec4<f32>,
    @location(9) uv1: vec2<f32>,
};

fn static_instance(v: VertexIn) -> VertexIn {
    if v.m0.w != -2.0 { return v; }
    let index = static_indices[1u+v.instance_index];
    let base = 1u+index*6u;
    var out = v;
    out.m0 = items[base]; out.m1=items[base+1u]; out.m2=items[base+2u]; out.m3=items[base+3u]; out.color=items[base+4u];
    return out;
}


struct VertexOut {
    // Invariant: the depth prepass and the main pass must land on exactly the same depth.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) @interpolate(flat) origin: vec3<f32>,
    // Which decal layers the model takes (see apply_decals).
    @location(6) @interpolate(flat) decal_layers: u32,
    // The crossfade dither (see `fade_keeps`), else 0.
    @location(7) @interpolate(flat) fade: f32,
    // The model's baked lightmap entry plus one, 0 without (see baked.wgsl).
    @location(8) @interpolate(flat) lightmap: f32,
};

// The model's tint (`Model::color`) on a corner's vertex colour: all over,
// or, for a material with a masked tint (`maps.w`), only as far as the
// vertex colour's alpha says (1 tinted, 0 not); that alpha is then a mask,
// not opacity, and the tint's alpha still fades the whole model.
fn tinted(tint: vec4<f32>, vertex_color: vec4<f32>) -> vec4<f32> {
    if material.maps.w == 1u {
        return vec4<f32>(vertex_color.rgb * mix(vec3<f32>(1.0), tint.rgb, vertex_color.a), tint.a);
    }
    return tint * vertex_color;
}

// The vertex in world space, after the shader's `vertex` function.
fn world_vertex(input: VertexIn) -> Vertex {
    let in = static_instance(input);
    // The matrix's last row is (0, 0, 0, 1); the first slot of it carries the
    // model's decal layers instead (see instance_decal_layers).
    let model = mat4x4<f32>(vec4<f32>(in.m0.xyz, 0.0), vec4<f32>(in.m1.xyz, 0.0), vec4<f32>(in.m2.xyz, 0.0), vec4<f32>(in.m3.xyz, 1.0));
    // For a scale-rotate matrix M = R S, the normal matrix R S^-1 is M S^-2.
    let scale = vec3<f32>(length(in.m0.xyz), length(in.m1.xyz), length(in.m2.xyz));
    let normal = mat3x3<f32>(in.m0.xyz, in.m1.xyz, in.m2.xyz) * (in.normal / max(scale * scale, vec3<f32>(1e-12)));
    var v: Vertex;
    v.instance_index = in.instance_index;
    v.position = (model * vec4<f32>(in.position, 1.0)).xyz;
    v.normal = normal;
    v.uv = material_uv(in.uv);
    v.color = tinted(in.color, in.vertex_color);
    v.uv1 = in.uv1;
    v.origin = in.m3.xyz;
    var out = vertex(v);
    out.origin = v.origin;
    return out;
}

// The decal layers the model takes, as the engine put them in the instance.
fn instance_decal_layers(in: VertexIn) -> u32 {
    return u32(static_instance(in).m0.w + 0.5);
}

// The model's lightmap entry plus one (0: none), from the matrix's spare
// third slot of its last row.
fn instance_lightmap(in: VertexIn) -> f32 {
    return static_instance(in).m2.w;
}

// `input` with `instance_index` read in the entry point itself. Direct3D
// 12's instance ID starts at 0 whatever the draw's first instance; naga adds
// the first instance to `instance_index` only where an entry point reads the
// builtin, not in a function the whole input struct is passed to. Every
// entry point taking a `VertexIn` passes it through this first, so the
// functions below see the same index as on Vulkan and Metal (instance cells
// drawn together start each cell's list at its first instance).
fn entry_input(input: VertexIn, instance_index: u32) -> VertexIn {
    var v = input;
    v.instance_index = instance_index;
    return v;
}

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    let in = entry_input(input, input.instance_index);
    let v = world_vertex(in);
    var out: VertexOut;
    out.decal_layers = instance_decal_layers(in);
    out.clip = frame.view_proj * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    out.uv = v.uv;
    out.color = v.color;
    out.uv1 = v.uv1;
    out.origin = v.origin;
    out.fade = in.m1.w;
    out.lightmap = instance_lightmap(in);
    return out;
}

// The shadow map: depth seen from the sun.
@vertex
fn vs_shadow(input: VertexIn) -> VertexOut {
    let in = entry_input(input, input.instance_index);
    let v = world_vertex(in);
    var out: VertexOut;
    out.decal_layers = 0u;
    out.clip = frame.caster_view_proj * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    out.uv = v.uv;
    out.color = v.color;
    out.uv1 = v.uv1;
    out.origin = v.origin;
    out.fade = in.m1.w;
    out.lightmap = 0.0;
    return out;
}

// Whether a pixel of a crossfading instance is drawn: a screen-door dither.
// `fade` is the share of pixels kept; negative for the level fading out,
// which keeps the pixels the level fading in doesn't (so together they
// cover every pixel once). The depth prepass, the main pass and the shadow
// pass all use this on the pixel they draw, so they agree.
fn fade_keeps(fade: f32, pixel: vec2<f32>) -> bool {
    if fade == 0.0 {
        return true;
    }
    // Interleaved gradient noise (Jimenez): an even spread of 0..1 per pixel.
    let p = floor(pixel);
    let t = fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
    if fade > 0.0 {
        return t < fade;
    }
    return t >= 1.0 + fade;
}

// The surface from the material and its textures, then the shader's `surface`.
fn surface_at(in: VertexOut, front: bool) -> Surface {
    let base = textureSampleBias(base_texture, material_sampler, in.uv, texture_mip_bias()) * material.color * in.color;
    var s: Surface;
    s.albedo = base.rgb;
    s.alpha = base.a;
    s.world = in.world;
    s.uv = in.uv;
    s.uv1 = in.uv1;
    s.origin = in.origin;
    s.color = in.color;
    s.view = normalize(frame.camera.xyz - in.world);
    s.screen = in.clip.xy;
    var n = normalize(in.normal);
    if !front {
        n = -n; // the back of a double-sided surface
    }
    let geometric = n;
    if material.maps.x != 0u {
        // Tangent space from screen derivatives (Schüler's cotangent frame):
        // no tangents needed in the mesh. `t` and `b` point where u and v
        // grow. The frame takes screen y up, as OpenGL's derivatives have
        // it: WGSL's `dpdy` runs down the screen, which taken as it is turns
        // `t` and `b` around (a normal map would lean the surface the wrong
        // way on both axes).
        let dp1 = dpdx(in.world);
        let dp2 = -dpdy(in.world);
        let duv1 = dpdx(in.uv);
        let duv2 = -dpdy(in.uv);
        let dp2perp = cross(dp2, n);
        let dp1perp = cross(n, dp1);
        let t = dp2perp * duv1.x + dp1perp * duv2.x;
        let b = dp2perp * duv1.y + dp1perp * duv2.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
        let texel = textureSampleBias(normal_texture, material_sampler, in.uv, texture_mip_bias());
        var m = texel.xyz * 2.0 - 1.0;
        // Two-channel normal maps (BC5, EAC; 3: X and Y in red and alpha)
        // store no Z: it is the rest of the unit length.
        if material.maps.x >= 2u {
            if material.maps.x == 3u {
                m.y = texel.a * 2.0 - 1.0;
            }
            m.z = sqrt(max(1.0 - dot(m.xy, m.xy), 0.0));
        }
        m = vec3<f32>(m.xy * material.params.z, m.z);
        // glTF: the map's +X is along +u; its +Y is up the image, and glTF's
        // v runs down the image, so +Y is along -v: minus the bitangent.
        n = normalize(t * inv * m.x - b * inv * m.y + n * m.z);
    }
    s.normal = n;
    s.roughness = material.params.x;
    s.metallic = material.params.y;
    s.occlusion = 1.0;
    if material.maps.y != 0u {
        let orm = textureSampleBias(orm_texture, material_sampler, in.uv, texture_mip_bias());
        s.occlusion = orm.r;
        s.roughness *= orm.g;
        s.metallic *= orm.b;
    }
    s.emissive = material.emissive.rgb;
    if material.maps.z != 0u {
        s.emissive *= textureSampleBias(emissive_texture, material_sampler, in.uv, texture_mip_bias()).rgb;
    }
    s.unlit = material.emissive.w != 0.0;
    if MATERIAL_LAYERS {
        fill_layers(s, in.uv, in.world, geometric);
    }
    s = surface(s);
    s.roughness = clamp(s.roughness, 0.03, 1.0);
    s.metallic = clamp(s.metallic, 0.0, 1.0);
    return s;
}

// The depth prepass and the shadow map of cut-out materials (Blend::Cutout):
// depth only, minus the pixels their alpha cuts out. Solid materials use no
// fragment shader there.
@fragment
fn fs_depth(in: VertexOut, @builtin(front_facing) front: bool) {
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
}

// The same for crossfading instances (see `fade_keeps`).
@fragment
fn fs_depth_fade(in: VertexOut, @builtin(front_facing) front: bool) {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
}

// Alpha to coverage (cut-out materials with multisampling): the alpha
// sharpened to a ramp about a pixel wide around the cutoff, so the edge gets
// partial coverage instead of a hard cut. Called in uniform control flow.
fn coverage(alpha: f32) -> f32 {
    return clamp((alpha - alpha_cutoff()) / max(fwidth(alpha), 0.0001) + 0.5, 0.0, 1.0);
}

// The depth prepass of cut-out materials with multisampling: the alpha goes
// to coverage (the color target isn't written).
@fragment
fn fs_depth_coverage(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, coverage(surface_at(in, front).alpha));
}

@fragment
fn fs_depth_coverage_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let a = coverage(surface_at(in, front).alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, a);
}

// Opaque cut-outs with multisampling: the same coverage as their prepass, so
// the depth test keeps exactly the samples it wrote.
@fragment
fn fs_main_coverage(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    let a = coverage(surface_at(in, front).alpha);
    return vec4<f32>(opaque_color(in, front, dp_x, dp_y).rgb, a);
}

@fragment
fn fs_main_coverage_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    let a = coverage(surface_at(in, front).alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return vec4<f32>(opaque_color(in, front, dp_x, dp_y).rgb, a);
}

// The depth prepass and shadow map of a crossfading instance of a solid
// material: only the dither.
@fragment
fn fs_fade(in: VertexOut) {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
}

// ---- Screen-space reflections' normals ----
// With reflections on, solid surfaces are drawn once more over the
// finished prepass depth (equal depths only, so each pixel once) to say
// which way each faces: the normal the opaque pass lights it with, normal
// maps, `surface` and decals included, so the trace's rays leave as the
// opaque pass reflects (see ssr.wgsl). Red, green and blue: the normal,
// each axis -1 to 1 in 10 bits with 0 exact. Multisampling averages a
// pixel's samples: two surfaces' normals, or one and the clear value (all
// 0), average to shorter than 1, which the trace can tell. Alpha, in two
// bits: 0 where nothing was drawn (crowds and solid stencil materials
// aren't: the trace keeps the depth's normal and traces), 1/3 where the
// surface is too rough to show reflections (roughness 0.7 and up, where
// `environment_light` has faded them out: no ray), 2/3 where it is glossy
// and nothing tilted its normal from the mesh's (`geometric`: the trace
// keeps the depth's normal, as without this pass), 1 where something did.
fn reflection_normal(n: vec3<f32>, geometric: vec3<f32>, roughness: f32) -> vec4<f32> {
    let tilted = dot(n, geometric) < 0.99996;
    let alpha = select(select(2.0 / 3.0, 1.0, tilted), 1.0 / 3.0, roughness >= 0.7);
    return vec4<f32>((n * 511.0 + 512.0) / 1023.0, alpha);
}

fn reflection_surface(in: VertexOut, front: bool, dp_x: vec3<f32>, dp_y: vec3<f32>) -> vec4<f32> {
    var s = surface_at(in, front);
    var geometric = normalize(in.normal);
    if !front {
        geometric = -geometric;
    }
    s = apply_decals(s, geometric, in.decal_layers, dp_x, dp_y);
    return reflection_normal(s.normal, geometric, clamp(s.roughness, 0.03, 1.0));
}

@fragment
fn fs_normals(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return reflection_surface(in, front, dpdx(in.world), dpdy(in.world));
}

@fragment
fn fs_normals_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    let n = reflection_surface(in, front, dp_x, dp_y);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return n;
}

// The opaque pass's contact shadow at this pixel (see opaque_color).
var<private> contact_shadow: f32 = 0.0;

fn shade(s: Surface) -> vec3<f32> {
    if s.unlit { return s.albedo + s.emissive; }
    let n = s.normal;
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    var sun = 0.0;
    if dot(n, frame.sun_dir.xyz) > 0.0 { sun = sunlight(s.world, n) * (1.0 - contact_shadow); }
    let direct = direct_light(s, frame.sun_dir.xyz, frame.sun_color.rgb * sun) + punctual_light(s);

    bounced_light = gi_light(s.world, n, s.view);
    // Baked light: a lightmap always, the probes where the real-time
    // probes don't reach.
    let baked = baked_light(s.world, n, s.uv1);
    if baked.a > 0.0 && (baked_entry > 0.5 || bounced_light.a <= 0.0) {
        bounced_light = baked;
    }
    let ambient = environment_light(s, f0);

    // Linear HDR light; the post pass maps it to the screen.
    return direct + ambient + s.emissive;
}

// Opaque: the prepass already cut the cut-outs.
@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Screen derivatives need uniform control flow: taken before any branch.
    return opaque_color(in, front, dpdx(in.world), dpdy(in.world));
}

// Opaque, for crossfading instances (see `fade_keeps`).
@fragment
fn fs_main_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

// The view model's cut-out held items without multisampling: in no
// prepass, so they cut their own holes and write their own depth.
@fragment
fn fs_main_cutout(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

@fragment
fn fs_main_cutout_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    // Sampled before the branch: `a || f()` only calls f where a is false,
    // non-uniform control flow, where WebGPU's WGSL forbids textureSample.
    let alpha = surface_at(in, front).alpha;
    if !fade_keeps(in.fade, in.clip.xy) || alpha < alpha_cutoff() {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

fn opaque_color(in: VertexOut, front: bool, dp_x: vec3<f32>, dp_y: vec3<f32>) -> vec4<f32> {
    var s = surface_at(in, front);
    var geometric = normalize(in.normal);
    if !front {
        geometric = -geometric;
    }
    s = apply_decals(s, geometric, in.decal_layers, dp_x, dp_y);
    baked_entry = in.lightmap;
    baked_face = geometric;
    s.roughness = clamp(s.roughness, 0.03, 1.0);
    s.metallic = clamp(s.metallic, 0.0, 1.0);
    // The opaque pass uses the otherwise unused scene-color slot for GTAO
    // occlusion (zero in the no-scene stand-in). Transparent passes retain
    // the actual scene color and never sample it as occlusion.
    // Green holds the contact shadow, which blocks only the sun.
    let ao_at = clamp(vec2<i32>(s.screen), vec2<i32>(0), vec2<i32>(textureDimensions(scene_color_texture)) - 1);
    let screen = textureLoad(scene_color_texture, ao_at, 0);
    s.occlusion *= 1.0 - screen.r;
    contact_shadow = screen.g;
    // Blue marks screen-space reflections in a second level, at half size;
    // alpha is their cone's spread (see ssr.wgsl), unless the spread image
    // has a slot of its own.
    if screen.b > 0.5 && s.roughness < 0.7 {
        let filtered = reflection_at(s.screen, s.roughness, reflection_spread(s.screen, screen.a));
        screen_reflection = vec4<f32>(filtered.rgb / max(filtered.a, 0.00001), filtered.a);
    }
    return vec4<f32>(fog(shade(s), s.world), 1.0);
}

// Transparent: blended over what's behind it (premultiplied alpha).
@fragment
fn fs_transparent(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return transparent_color(in, front);
}

@fragment
fn fs_transparent_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return transparent_color(in, front);
}

fn transparent_color(in: VertexOut, front: bool) -> vec4<f32> {
    let s = surface_at(in, front);
    baked_entry = in.lightmap;
    baked_face = select(-1.0, 1.0, front) * normalize(in.normal);
    if frame.fog_color.w > 0.0 && s.roughness < 0.7 {
        screen_reflection = transparent_reflection(s);
    }
    return vec4<f32>(fog(shade(s), s.world) * s.alpha, s.alpha);
}

// ---- Screen-space reflections on transparent surfaces ----
// With `Post::ssr_transparent` (the strength in frame.fog_color.w, how far
// a ray looks in frame.night.w), a glossy transparent surface such as water
// or glass marches its reflected ray across the opaque scene behind it: the
// depth and color the split transparent pass copies (see scene_color). 24
// jittered steps in screen space (in the offscreen test, 32 drew slightly
// smoother edges, 16 ragged ones); a step that went behind something closer than
// twice its own length is a hit, then 4 halvings find the surface. It shows the opaque scene of this
// frame, sharp (no cone blur: `environment_light` fades reflections out on
// rough surfaces anyway), fading out toward the screen's edges and the end
// of the ray. Other transparent surfaces are not in it.
fn transparent_reflection(s: Surface) -> vec4<f32> {
    let ray = reflect(-s.view, s.normal);
    var reach = frame.night.w;
    let start = frame.view_proj * vec4<f32>(s.world, 1.0);
    var end = frame.view_proj * vec4<f32>(s.world + ray * reach, 1.0);
    // A ray toward the camera stops before it passes the near plane.
    let nearest = start.w * 0.1;
    if end.w < nearest {
        let t = (start.w - nearest) / (start.w - end.w);
        reach *= t;
        end = mix(start, end, t);
    }
    let size = frame.screen.xy;
    let a = vec3<f32>(start.xy / start.w * vec2<f32>(0.5, -0.5) + 0.5, start.z / start.w);
    let b = vec3<f32>(end.xy / end.w * vec2<f32>(0.5, -0.5) + 0.5, end.z / end.w);
    let far = s.world + ray * reach;
    // Positions over w and 1 / w change linearly across the screen.
    let world_a = vec4<f32>(s.world, 1.0) / start.w;
    let world_b = vec4<f32>(far, 1.0) / end.w;
    let jitter = hash2(s.screen);
    var before = 0.0;
    var hit = -1.0;
    var previous = s.world;
    for (var i = 0u; i < 24u; i++) {
        let k = (f32(i) + jitter) / 24.0;
        let p = mix(a, b, k);
        if any(p.xy < vec2<f32>(0.0)) || any(p.xy > vec2<f32>(1.0)) {
            break;
        }
        let h = mix(world_a, world_b, k);
        let point = h.xyz / h.w;
        // Reversed depth: the opaque scene is nearer than the ray where its
        // depth is greater.
        if opaque_depth_at(p.xy * size) > p.z {
            let gap = distance(point, scene_position(p.xy * size));
            if gap < max(2.0 * distance(point, previous), 0.05) {
                hit = k;
                break;
            }
        }
        before = k;
        previous = point;
    }
    if hit < 0.0 {
        return vec4<f32>(0.0);
    }
    var low = before;
    for (var j = 0; j < 4; j++) {
        let k = 0.5 * (low + hit);
        let p = mix(a, b, k);
        if opaque_depth_at(p.xy * size) > p.z {
            hit = k;
        } else {
            low = k;
        }
    }
    let p = mix(a, b, hit);
    let h = mix(world_a, world_b, hit);
    let travelled = distance(h.xyz / h.w, s.world) / max(reach, 0.0001);
    let edge = max(abs(p.x - 0.5), abs(p.y - 0.5)) * 2.0;
    let fade = (1.0 - smoothstep(0.9, 1.0, edge)) * (1.0 - smoothstep(0.8, 1.0, travelled));
    return vec4<f32>(scene_color(p.xy * size), fade * min(frame.fog_color.w, 1.0));
}

// The opaque scene's depth at a pixel (reversed: 0 is the far plane).
fn opaque_depth_at(screen: vec2<f32>) -> f32 {
    let at = min(vec2<i32>(screen), vec2<i32>(textureDimensions(scene_depth_texture)) - 1);
    return textureLoad(scene_depth_texture, at, 0);
}

// ---- Motion vectors (temporal antialiasing) ----
// Models that moved since last frame are drawn again, over the depth the
// opaque pass left, to say where each pixel was: the same vertices through
// last frame's model matrix, with `vertex` run as of the last frame (its
// `frame.time` and `items` are last frame's, see `motion_past_time`). A
// skinned or morphing model whose pose changed brings its last pose's
// positions in `previous_position`; every other mesh brings its own
// positions again. A buffer-driven mesh (see `Vertex.instance_index`) reads
// the last frame's elements only when its buffer keeps them; otherwise both
// runs see the same element and it moves with its entity.
struct VelocityIn {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) vertex_color: vec4<f32>,
    @location(9) uv1: vec2<f32>,
    @location(10) p0: vec4<f32>,
    @location(11) p1: vec4<f32>,
    @location(12) p2: vec4<f32>,
    @location(13) p3: vec4<f32>,
    @location(14) previous_position: vec3<f32>,
};

struct VelocityOut {
    // The same position as `vs_main`, so depth matches exactly.
    @builtin(position) @invariant clip: vec4<f32>,
    // Previous camera, with the previous object transforms (`before`) and
    // as the depth's reprojection sees the current ones (`rigid`).
    @location(0) before: vec4<f32>,
    @location(1) rigid: vec4<f32>,
};

@vertex
fn vs_velocity(in: VelocityIn) -> VelocityOut {
    var a: VertexIn;
    a.instance_index = in.instance_index;
    a.position = in.position;
    a.normal = in.normal;
    a.uv = in.uv;
    a.m0 = in.m0;
    a.m1 = in.m1;
    a.m2 = in.m2;
    a.m3 = in.m3;
    a.color = in.color;
    a.vertex_color = in.vertex_color;
    a.uv1 = in.uv1;
    var b = a;
    b.m0 = in.p0;
    b.m1 = in.p1;
    b.m2 = in.p2;
    b.m3 = in.p3;
    b.position = in.previous_position;
    let now = world_vertex(a);
    let before = world_vertex_before(b);
    var out: VelocityOut;
    out.clip = frame.view_proj * vec4<f32>(now.position, 1.0);
    out.rigid = frame.reprojection * vec4<f32>(now.position, 1.0);
    out.before = frame.previous_view_proj * vec4<f32>(before.position, 1.0);
    return out;
}

// `world_vertex` as of the last frame: `vertex` reads its time and buffer.
fn world_vertex_before(in: VertexIn) -> Vertex {
    motion_past_time = true;
    motion_past_items = true;
    let v = world_vertex(in);
    motion_past_time = false;
    motion_past_items = false;
    return v;
}

// Models whose matrix and pose held still, drawn with a shader whose
// `vertex` reads the last frame (wind, waves, a buffer that keeps its last
// frame): from the frame's own instances, `vertex` run as of this frame
// and the last. Models that also moved are drawn by `vs_velocity` after,
// over these.
@vertex
fn vs_velocity_still(input: VertexIn) -> VelocityOut {
    let in = entry_input(input, input.instance_index);
    let now = world_vertex(in);
    let before = world_vertex_before(in);
    var out: VelocityOut;
    out.clip = frame.view_proj * vec4<f32>(now.position, 1.0);
    out.rigid = frame.reprojection * vec4<f32>(now.position, 1.0);
    out.before = frame.previous_view_proj * vec4<f32>(before.position, 1.0);
    return out;
}

// Object-only correction to backward history lookup in texture coordinates,
// minus the camera-only motion; static pixels keep the clear value zero.
@fragment
fn fs_velocity(in: VelocityOut) -> @location(0) vec2<f32> {
    if in.before.w <= 1e-5 || in.rigid.w <= 1e-5 {
        return vec2<f32>(0.0);
    }
    let a = in.rigid.xy / in.rigid.w;
    let b = in.before.xy / in.before.w;
    return vec2<f32>((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
}

// The tangent of the cone around the mirror direction that holds most of
// the light a surface of `roughness` reflects: the angle that holds 0.244
// of a Phong lobe of the specular power that matches its GGX lobe (alpha,
// roughness squared). 0.001 for a mirror's 0.03, 0.05 at 0.2, 0.19 at 0.4.
fn reflection_cone(roughness: f32) -> f32 {
    let a = roughness * roughness;
    let power = 2.0 / max(a * a, 0.000001) - 2.0;
    let c = pow(0.244, 1.0 / (power + 1.0));
    return sqrt(max(1.0 - c * c, 0.0)) / c;
}

// The reflection cone's spread at full-size pixel `pixel`: read from the
// half-size spread image (see ssr.wgsl), where the device has a texture
// slot for it. Without one, the engine replaces these lines with a function
// that returns `copied`, the copy in the screen image's first level.
// ---- reflection spread image: begin ----
@group(2) @binding(3) var reflection_spread_texture: texture_2d<f32>;
fn reflection_spread(pixel: vec2<f32>, copied: f32) -> f32 {
    let size = vec2<i32>(textureDimensions(reflection_spread_texture));
    return textureLoad(reflection_spread_texture, min(vec2<i32>(pixel) / 2, size - 1), 0).r;
}
// ---- reflection spread image: end ----

// The screen-space reflections at full-size pixel `pixel` of a surface of
// `roughness` whose ray's cone spreads by `spread` (see ssr.wgsl):
// confidence-premultiplied radiance, blurred over the cone's footprint,
// its tangent times the spread, in full-size pixels. A reflection that
// starts close to what it shows stays sharp on a rough surface; one that
// travels far blurs. A pixel of level n is 2^n full-size pixels wide.
fn reflection_at(pixel: vec2<f32>, roughness: f32, spread: f32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene_color_texture));
    let last = min(5.0, floor(log2(max(size.x, size.y))));
    let footprint = reflection_cone(roughness) * spread;
    let lod = clamp(log2(max(footprint, 1.0)), 1.0, last);
    let uv = pixel / size;
    if lod < 2.0 {
        // Footprints narrower than a pixel of level 2: four taps of the
        // trace's own level, from one point apart to one of its pixels, so
        // the blur grows smoothly instead of mixing in level 2, which
        // spreads a thin bright detail over four full-size pixels at once.
        let o = (lod - 1.0) * 0.5 / vec2<f32>(textureDimensions(scene_color_texture, 1));
        return 0.25 * (reflection_level(uv + o, 1) + reflection_level(uv - o, 1)
            + reflection_level(uv + vec2<f32>(o.x, -o.y), 1) + reflection_level(uv + vec2<f32>(-o.x, o.y), 1));
    }
    let low = i32(floor(lod));
    var filtered = reflection_level(uv, low);
    if fract(lod) > 0.0 && low < i32(last) {
        filtered = mix(filtered, reflection_level(uv, low + 1), fract(lod));
    }
    return filtered;
}

// Reflection mips contain confidence-premultiplied radiance. Material
// shaders have no sampler of their own to spare: this borrows the sky's
// linear one, which wraps around horizontally, so the lookup stays half a
// pixel inside the edges.
fn reflection_level(uv: vec2<f32>, level: i32) -> vec4<f32> {
    let half = 0.5 / vec2<f32>(textureDimensions(scene_color_texture, level));
    return textureSampleLevel(scene_color_texture, sky_sampler, clamp(uv, half, 1.0 - half), f32(level));
}
