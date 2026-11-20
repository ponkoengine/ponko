// Baked lighting is not read by this material: its shader uses the third
// texture array slot (custom_array2) for its own array.
var<private> baked_entry: f32 = 0.0;
var<private> baked_face: vec3<f32> = vec3<f32>(0.0);

fn baked_light(world: vec3<f32>, n: vec3<f32>, uv1: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}
