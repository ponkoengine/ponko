
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
