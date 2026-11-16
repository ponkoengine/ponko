
struct SpriteInstance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) pivot: vec2<f32>,
    @location(3) rotation: f32,
    @location(4) color: vec4<f32>,
    @location(5) uv: vec4<f32>,
};

struct SpriteVertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) local: vec2<f32>,
    @location(3) world: vec2<f32>,
};

fn ponko_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: SpriteInstance) -> SpriteVertexOut {
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
    let world = inst.pos + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    var out: SpriteVertexOut;
    out.clip = camera.view_proj * vec4<f32>(world, 0.0, 1.0);
    out.uv = inst.uv.xy + corner * inst.uv.zw;
    out.color = vec4<f32>(ponko_srgb_to_linear(inst.color.rgb), inst.color.a);
    // A mesh vertex carries where it is in the mesh's bounds as its pivot.
    out.local = select(corner, inst.pivot, all(inst.size == vec2<f32>(0.0)));
    out.world = world;
    return out;
}

fn ponko_pixel(in: SpriteVertexOut) -> SpritePixel {
    let texel = textureSample(sprite_texture, sprite_sampler, in.uv);
    return SpritePixel(texel * in.color, texel, in.color, in.uv, in.local, in.world);
}

// Alpha, additive and premultiplied blending take the hook's color as it is.
@fragment
fn fs_main(in: SpriteVertexOut) -> @location(0) vec4<f32> {
    return fragment(ponko_pixel(in));
}

// Multiply blending: the color premultiplied, so transparent parts leave
// what's behind unchanged.
@fragment
fn fs_multiply(in: SpriteVertexOut) -> @location(0) vec4<f32> {
    let c = fragment(ponko_pixel(in));
    return vec4<f32>(c.rgb * c.a, c.a);
}
