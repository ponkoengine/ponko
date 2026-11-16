// Instanced sprite shader. One instance = one sprite; each instance draws a
// 6-vertex quad generated from the vertex index (no vertex buffer).

struct Camera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) pivot: vec2<f32>,
    @location(3) rotation: f32,    // radians
    @location(4) color: vec4<f32>, // sRGB tint
    @location(5) uv: vec4<f32>,    // u0, v0, width, height
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    // Mesh vertices (zero size) draw from one vertex each, so `vi` runs
    // past the six corners: the corner then doesn't matter.
    let corner = corners[vi % 6u];

    let local = (corner - inst.pivot) * inst.size;
    let c = cos(inst.rotation);
    let s = sin(inst.rotation);
    let rotated = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);

    var out: VertexOut;
    out.clip = camera.view_proj * vec4<f32>(inst.pos + rotated, 0.0, 1.0);
    out.uv = inst.uv.xy + corner * inst.uv.zw;
    out.color = vec4<f32>(srgb_to_linear(inst.color.rgb), inst.color.a);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv) * in.color;
}

// Blend modes (`BlendMode`): multiply premultiplies the color, so that
// transparent parts leave what's behind unchanged.
@fragment
fn fs_multiply(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(sprite_texture, sprite_sampler, in.uv) * in.color;
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Premultiplied: the texture already is; the tint is premultiplied here.
@fragment
fn fs_premultiplied(in: VertexOut) -> @location(0) vec4<f32> {
    let tint = vec4<f32>(in.color.rgb * in.color.a, in.color.a);
    return textureSample(sprite_texture, sprite_sampler, in.uv) * tint;
}
