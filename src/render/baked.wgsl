// Baked lighting (BakedLighting, see three/bake): indirect light worked out
// ahead of time, from a lightmap for models with a Lightmapped component and
// from a probe volume for the others.
//
// It is the material's third texture array (custom_array2) when the
// material sets none there. Layer 0: texel 0 is (-1, pages, 1 when
// directional, intensity) (no array of images holds a negative value);
// texels 1 and 2 the probe grid's lowest probe in two parts (and its
// spacing in 1.w); texel 3 its probes along x, y, z and the row its probes
// start at (x 0: no probes). From texel 8, two texels per lightmap entry:
// (x, y, width, height) of its rectangle in texels, then (page, ...). Each
// probe is three texels: irradiance over pi toward normal n is
// c + cx * n.x + cy * n.y + cz * n.z, laid out as (c.rgb, cx.r),
// (cx.gb, cy.rg), (cy.b, cz.rgb). Layers 1 to pages hold the pages' light
// (irradiance over pi), the next ones (when directional) the mean direction
// the light comes from, its length how much it comes from that way.

// The entry plus one of the model this pixel is of (0: none), and its
// surface's own normal (before normal maps): set by the entry points.
var<private> baked_entry: f32 = 0.0;
var<private> baked_face: vec3<f32> = vec3<f32>(0.0);

// Texel `texel` (in texels, from the top left) of layer `layer`, filtered.
fn baked_texel(layer: i32, texel: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(custom_array2));
    return textureSampleLevel(custom_array2, sky_sampler, texel / size, layer, 0.0);
}

// Texel `i` of layer 0, counted row by row (read at its center: exact).
fn baked_at(i: f32) -> vec4<f32> {
    let w = f32(textureDimensions(custom_array2).x);
    let row = floor(i / w);
    return baked_texel(0, vec2<f32>(i - row * w, row) + 0.5);
}

// The baked indirect light at a surface at `world` facing `n` with
// lightmap coordinates `uv1`: irradiance over pi (rgb), and how much it
// replaces the ambient light (a). Zero without baked lighting.
fn baked_light(world: vec3<f32>, n: vec3<f32>, uv1: vec2<f32>) -> vec4<f32> {
    let header = baked_at(0.0);
    if header.x >= 0.0 {
        return vec4<f32>(0.0);
    }
    if baked_entry > 0.5 {
        let at = 8.0 + 2.0 * (baked_entry - 1.0);
        let rect = baked_at(at);
        if rect.z > 0.0 {
            let page = baked_at(at + 1.0).x;
            // Filtered within the rectangle, never past its edge texels.
            let texel = clamp(rect.xy + uv1 * rect.zw, rect.xy + 0.5, rect.xy + rect.zw - 0.5);
            var light = baked_texel(i32(1.0 + page), texel).rgb;
            if header.z > 0.5 {
                // Normal maps tilt toward or away from where the light came
                // from; the surface's own normal gets the baked value.
                let d = baked_texel(i32(1.0 + header.y + page), texel).xyz;
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
