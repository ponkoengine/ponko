// The entry points of a game's sky shader (see `SkyShader`): composed after
// mesh.wgsl's prelude, sky.wgsl and the game's `sky` function.

// The sky texture: every direction's light, from the game's sky.
@fragment
fn fs_lut_shader(in: FullScreen) -> @location(0) vec4<f32> {
    return vec4<f32>(max(sky(sky_dir(in.uv)), vec3<f32>(0.0)), 1.0);
}

// The background: the game's sky at every pixel, sharper than the texture.
@fragment
fn fs_sky_shader(in: FullScreen) -> @location(0) vec4<f32> {
    let dir = far_dir(in.uv);
    sky_background = true;
    return background(dir, max(sky(dir), vec3<f32>(0.0)));
}
