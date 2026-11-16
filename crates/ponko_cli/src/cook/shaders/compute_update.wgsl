//@@ponko-mesh-prelude@@
@group(3) @binding(0) var<storage, read_write> items: array<vec4<f32>>;

// ---- the game's compute pass ----
//@@ponko-game@@
// ---- the engine's entry point ----
@compute @workgroup_size(64) fn cs_compute(@builtin(global_invocation_id) id: vec3<u32>) { if id.x < arrayLength(&items) { update(id.x); } }