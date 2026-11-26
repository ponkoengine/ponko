// Only the multi-mapping shader binds the tail of the material uniform.
const MATERIAL_MULTI_UV: bool = true;

fn map_uv_index(kind: u32) -> u32 {
    return (material.uv_indices[kind / 16u] >> (2u * (kind % 16u))) & 3u;
}

fn map_uses_uv1(kind: u32) -> bool {
    return (material.uv_indices.z & (1u << kind)) != 0u;
}

fn map_uv(uv0: vec2<f32>, uv1: vec2<f32>, kind: u32) -> vec2<f32> {
    let uv = select(uv0, uv1, map_uses_uv1(kind));
    let i = map_uv_index(kind);
    if i == 0u {
        return material_uv(uv);
    }
    let row = (i - 1u) * 2u;
    let p = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(material.uv_additional[row].xyz, p), dot(material.uv_additional[row + 1u].xyz, p));
}

fn same_map_uv(a: u32, b: u32) -> bool {
    return map_uv_index(a) == map_uv_index(b) && map_uses_uv1(a) == map_uses_uv1(b);
}
