// The frame, drawn into an sRGB image (read back here in linear light),
// written into a window that can't be sRGB itself: the sRGB curve is
// applied here, so the window holds what an sRGB window would.

@group(0) @binding(0) var frame: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// Linear to sRGB (IEC 61966-2-1).
fn srgb(c: vec3<f32>) -> vec3<f32> {
    let l = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(l, vec3<f32>(1.0 / 2.4)) - 0.055, l * 12.92, l <= vec3<f32>(0.0031308));
}

@fragment
fn fs_encode(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(frame, vec2<i32>(at.xy), 0);
    return vec4<f32>(srgb(c.rgb), c.a);
}
