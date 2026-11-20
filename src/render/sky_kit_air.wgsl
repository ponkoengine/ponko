// The sky kit's air lookup (see `KIT_AIR_ORIGIN` in sky_kit.wgsl): an
// entry point of the engine's, composed after the kit's sky shader and the
// sky shaders' own entry points (sky_shader.wgsl).

@fragment
fn fs_kit_air(in: FullScreen) -> @location(0) vec4<f32> {
    return vec4<f32>(max(kit_air(sky_dir(in.uv)), vec3<f32>(0.0)), 1.0);
}
