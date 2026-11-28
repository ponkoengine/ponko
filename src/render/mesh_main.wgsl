// The entry points of every material shader (see mesh.wgsl).

struct VertexIn {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,  // the model's tint
    @location(8) vertex_color: vec4<f32>,
    @location(9) uv1: vec2<f32>,
};

// STATIC_INSTANCE_HELPER

struct VertexOut {
    // Invariant: the depth prepass and the main pass must land on exactly the same depth.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) @interpolate(flat) origin: vec3<f32>,
    // Which decal layers the model takes (see apply_decals).
    @location(6) @interpolate(flat) decal_layers: u32,
    // The crossfade dither (see `fade_keeps`), else 0.
    @location(7) @interpolate(flat) fade: f32,
    // The model's baked lightmap entry plus one, 0 without (see baked.wgsl).
    @location(8) @interpolate(flat) lightmap: f32,
};

// The model's tint (`Model::color`) on a corner's vertex colour: all over,
// or, for a material with a masked tint (`maps.w`), only as far as the
// vertex colour's alpha says (1 tinted, 0 not); that alpha is then a mask,
// not opacity, and the tint's alpha still fades the whole model.
fn tinted(tint: vec4<f32>, vertex_color: vec4<f32>) -> vec4<f32> {
    if material.maps.w == 1u {
        return vec4<f32>(vertex_color.rgb * mix(vec3<f32>(1.0), tint.rgb, vertex_color.a), tint.a);
    }
    return tint * vertex_color;
}

// The vertex in world space, after the shader's `vertex` function.
fn world_vertex(input: VertexIn) -> Vertex {
    let in = static_instance(input);
    // The matrix's last row is (0, 0, 0, 1); the first slot of it carries the
    // model's decal layers instead (see instance_decal_layers).
    let model = mat4x4<f32>(vec4<f32>(in.m0.xyz, 0.0), vec4<f32>(in.m1.xyz, 0.0), vec4<f32>(in.m2.xyz, 0.0), vec4<f32>(in.m3.xyz, 1.0));
    // For a scale-rotate matrix M = R S, the normal matrix R S^-1 is M S^-2.
    let scale = vec3<f32>(length(in.m0.xyz), length(in.m1.xyz), length(in.m2.xyz));
    let normal = mat3x3<f32>(in.m0.xyz, in.m1.xyz, in.m2.xyz) * (in.normal / max(scale * scale, vec3<f32>(1e-12)));
    var v: Vertex;
    v.instance_index = in.instance_index;
    v.position = (model * vec4<f32>(in.position, 1.0)).xyz;
    v.normal = normal;
    v.uv = material_uv(in.uv);
    v.color = tinted(in.color, in.vertex_color);
    v.uv1 = in.uv1;
    v.origin = in.m3.xyz;
    var out = vertex(v);
    out.origin = v.origin;
    return out;
}

// The decal layers the model takes, as the engine put them in the instance.
fn instance_decal_layers(in: VertexIn) -> u32 {
    return u32(static_instance(in).m0.w + 0.5);
}

// The model's lightmap entry plus one (0: none), from the matrix's spare
// third slot of its last row.
fn instance_lightmap(in: VertexIn) -> f32 {
    return static_instance(in).m2.w;
}

// `input` with `instance_index` read in the entry point itself. Direct3D
// 12's instance ID starts at 0 whatever the draw's first instance; naga adds
// the first instance to `instance_index` only where an entry point reads the
// builtin, not in a function the whole input struct is passed to. Every
// entry point taking a `VertexIn` passes it through this first, so the
// functions below see the same index as on Vulkan and Metal (instance cells
// drawn together start each cell's list at its first instance).
fn entry_input(input: VertexIn, instance_index: u32) -> VertexIn {
    var v = input;
    v.instance_index = instance_index;
    return v;
}

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    let in = entry_input(input, input.instance_index);
    let v = world_vertex(in);
    var out: VertexOut;
    out.decal_layers = instance_decal_layers(in);
    out.clip = frame.view_proj * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    // The multi-UV build interpolates the two input coordinate sets.
    // The shared-transform build interpolates the final vertex UV.
    out.uv = select(v.uv, in.uv, MATERIAL_MULTI_UV);
    out.color = v.color;
    out.uv1 = v.uv1;
    out.origin = v.origin;
    out.fade = in.m1.w;
    out.lightmap = instance_lightmap(in);
    return out;
}

// The shadow map: depth seen from the sun.
@vertex
fn vs_shadow(input: VertexIn) -> VertexOut {
    let in = entry_input(input, input.instance_index);
    let v = world_vertex(in);
    var out: VertexOut;
    out.decal_layers = 0u;
    out.clip = frame.caster_view_proj * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    // The multi-UV build interpolates the two input coordinate sets.
    // The shared-transform build interpolates the final vertex UV.
    out.uv = select(v.uv, in.uv, MATERIAL_MULTI_UV);
    out.color = v.color;
    out.uv1 = v.uv1;
    out.origin = v.origin;
    out.fade = in.m1.w;
    out.lightmap = 0.0;
    return out;
}

// Whether a pixel of a crossfading instance is drawn: a screen-door dither.
// `fade` is the share of pixels kept; negative for the level fading out,
// which keeps the pixels the level fading in doesn't (so together they
// cover every pixel once). The depth prepass, the main pass and the shadow
// pass all use this on the pixel they draw, so they agree.
fn fade_keeps(fade: f32, pixel: vec2<f32>) -> bool {
    if fade == 0.0 {
        return true;
    }
    // Interleaved gradient noise (Jimenez): an even spread of 0..1 per pixel.
    let p = floor(pixel);
    let t = fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
    if fade > 0.0 {
        return t < fade;
    }
    return t >= 1.0 + fade;
}

// The surface from the material and its textures, then the shader's `surface`.
fn surface_at(in: VertexOut, front: bool) -> Surface {
    let base_uv = map_uv(in.uv, in.uv1, UV_BASE);
    let base = textureSampleBias(base_texture, material_sampler, base_uv, texture_mip_bias()) * material.color * in.color;
    var s: Surface;
    s.albedo = base.rgb;
    s.alpha = base.a;
    s.world = in.world;
    s.uv = base_uv;
    s.uv1 = in.uv1;
    s.origin = in.origin;
    s.color = in.color;
    s.view = normalize(frame.camera.xyz - in.world);
    s.screen = in.clip.xy;
    var n = normalize(in.normal);
    if !front {
        n = -n; // the back of a double-sided surface
    }
    let geometric = n;
    if material.maps.x != 0u {
        // Tangent space from screen derivatives (Schüler's cotangent frame):
        // no tangents needed in the mesh. `t` and `b` point where u and v
        // grow. The frame takes screen y up, as OpenGL's derivatives have
        // it: WGSL's `dpdy` runs down the screen, which taken as it is turns
        // `t` and `b` around (a normal map would lean the surface the wrong
        // way on both axes).
        let dp1 = dpdx(in.world);
        let dp2 = -dpdy(in.world);
        let normal_uv = map_uv(in.uv, in.uv1, UV_NORMAL);
        let duv1 = dpdx(normal_uv);
        let duv2 = -dpdy(normal_uv);
        let dp2perp = cross(dp2, n);
        let dp1perp = cross(n, dp1);
        let t = dp2perp * duv1.x + dp1perp * duv2.x;
        let b = dp2perp * duv1.y + dp1perp * duv2.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
        let texel = textureSampleBias(normal_texture, material_sampler, normal_uv, texture_mip_bias());
        var m = texel.xyz * 2.0 - 1.0;
        // Two-channel normal maps (BC5, EAC; 3: X and Y in red and alpha)
        // store no Z: it is the rest of the unit length.
        if material.maps.x >= 2u {
            if material.maps.x == 3u {
                m.y = texel.a * 2.0 - 1.0;
            }
            m.z = sqrt(max(1.0 - dot(m.xy, m.xy), 0.0));
        }
        m = vec3<f32>(m.xy * material.params.z, m.z);
        // glTF: the map's +X is along +u; its +Y is up the image, and glTF's
        // v runs down the image, so +Y is along -v: minus the bitangent.
        n = normalize(t * inv * m.x - b * inv * m.y + n * m.z);
    }
    s.normal = n;
    s.roughness = material.params.x;
    s.metallic = material.params.y;
    s.occlusion = 1.0;
    if material.maps.y != 0u {
        let mr_uv = map_uv(in.uv, in.uv1, UV_METALLIC_ROUGHNESS);
        let orm = textureSampleBias(orm_texture, material_sampler, mr_uv, texture_mip_bias());
        s.occlusion = orm.r;
        if !same_map_uv(UV_METALLIC_ROUGHNESS, UV_OCCLUSION) {
            let ao_uv = map_uv(in.uv, in.uv1, UV_OCCLUSION);
            s.occlusion = textureSampleBias(orm_texture, material_sampler, ao_uv, texture_mip_bias()).r;
        }
        s.roughness *= orm.g;
        s.metallic *= orm.b;
    }
    s.emissive = material.emissive.rgb;
    if material.maps.z != 0u {
        s.emissive *= textureSampleBias(emissive_texture, material_sampler, map_uv(in.uv, in.uv1, UV_EMISSIVE), texture_mip_bias()).rgb;
    }
    s.unlit = material.emissive.w != 0.0;
    if MATERIAL_LAYERS {
        // Here, not in `fill_layers`: see `ScreenSteps`.
        let steps = ScreenSteps(dpdx(in.world), dpdy(in.world), dpdx(in.uv), dpdy(in.uv), dpdx(in.uv1), dpdy(in.uv1));
        fill_layers(s, in.uv, in.uv1, steps, geometric, in.normal);
    }
    s = surface(s);
    s.roughness = clamp(s.roughness, 0.03, 1.0);
    s.metallic = clamp(s.metallic, 0.0, 1.0);
    return s;
}

// The depth prepass and the shadow map of cut-out materials (Blend::Cutout):
// depth only, minus the pixels their alpha cuts out. Solid materials use no
// fragment shader there.
@fragment
fn fs_depth(in: VertexOut, @builtin(front_facing) front: bool) {
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
}

// The same for crossfading instances (see `fade_keeps`).
@fragment
fn fs_depth_fade(in: VertexOut, @builtin(front_facing) front: bool) {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
}

// Alpha to coverage (cut-out materials with multisampling): the alpha
// sharpened to a ramp about a pixel wide around the cutoff, so the edge gets
// partial coverage instead of a hard cut. Called in uniform control flow.
fn coverage(alpha: f32) -> f32 {
    return clamp((alpha - alpha_cutoff()) / max(fwidth(alpha), 0.0001) + 0.5, 0.0, 1.0);
}

// The depth prepass of cut-out materials with multisampling: the alpha goes
// to coverage (the color target isn't written).
@fragment
fn fs_depth_coverage(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, coverage(surface_at(in, front).alpha));
}

@fragment
fn fs_depth_coverage_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let a = coverage(surface_at(in, front).alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, a);
}

// Opaque cut-outs with multisampling: the same coverage as their prepass, so
// the depth test keeps exactly the samples it wrote.
@fragment
fn fs_main_coverage(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    let a = coverage(surface_at(in, front).alpha);
    return vec4<f32>(opaque_color(in, front, dp_x, dp_y).rgb, a);
}

@fragment
fn fs_main_coverage_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    let a = coverage(surface_at(in, front).alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return vec4<f32>(opaque_color(in, front, dp_x, dp_y).rgb, a);
}

// The depth prepass and shadow map of a crossfading instance of a solid
// material: only the dither.
@fragment
fn fs_fade(in: VertexOut) {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
}

// ---- Screen-space reflections' normals ----
// With reflections on, the depth prepass also says which way each pixel
// faces, in a color target of its own (location 1 with multisampling,
// beside the cut-outs' coverage target; location 0 without): the normal
// the opaque pass lights it with, normal maps, `surface` and decals
// included, so the trace's rays leave as the opaque pass reflects (see
// ssr.wgsl). Each surface is drawn once: the prepass writes the depth and
// the normal together, and the nearest surface's normal is what is left.
// Red, green and blue: the normal, each axis -1 to 1 in 10 bits with 0
// exact. Multisampling averages a pixel's samples: two surfaces' normals,
// or one and the clear value (all 0), average to shorter than 1, which the
// trace can tell. Alpha, in two bits: 0 where nothing was drawn (and where
// a GPU-path crowd or a solid stencil material is: the trace keeps the
// depth's normal and traces), 1/3 where the surface is too rough to show
// reflections (roughness 0.7 and up, where `environment_light` has faded
// them out: no ray), 2/3 where it is glossy and nothing tilted its normal
// from the mesh's (`geometric`: the trace keeps the depth's normal, as
// without this pass), 1 where something did.
fn reflection_normal(n: vec3<f32>, geometric: vec3<f32>, roughness: f32) -> vec4<f32> {
    let tilted = dot(n, geometric) < 0.99996;
    let alpha = select(select(2.0 / 3.0, 1.0, tilted), 1.0 / 3.0, roughness >= 0.7);
    return vec4<f32>((n * 511.0 + 512.0) / 1023.0, alpha);
}

// The reflections' normal of surface `s` (`surface_at(in, front)`).
fn reflection_of(s_in: Surface, in: VertexOut, front: bool, dp_x: vec3<f32>, dp_y: vec3<f32>) -> vec4<f32> {
    var geometric = normalize(in.normal);
    if !front {
        geometric = -geometric;
    }
    let s = apply_decals(s_in, geometric, in.decal_layers, dp_x, dp_y);
    return reflection_normal(s.normal, geometric, clamp(s.roughness, 0.03, 1.0));
}

// What the prepass writes where nothing tells the trace a normal (see
// `NO_NORMAL` in ssr.rs).
const NO_NORMAL: vec4<f32> = vec4<f32>(512.0 / 1023.0, 512.0 / 1023.0, 512.0 / 1023.0, 0.0);

// With multisampling: the cut-outs' coverage (only its alpha, which alpha
// to coverage reads; nothing is written there) and the normal.
struct PrepassNormals {
    @location(0) coverage: vec4<f32>,
    @location(1) normal: vec4<f32>,
};

// Solid surfaces, one sample a pixel.
@fragment
fn fs_normals(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return reflection_of(surface_at(in, front), in, front, dpdx(in.world), dpdy(in.world));
}

@fragment
fn fs_normals_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let n = reflection_of(surface_at(in, front), in, front, dpdx(in.world), dpdy(in.world));
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return n;
}

// Cut-out surfaces, one sample a pixel: as `fs_depth`.
@fragment
fn fs_depth_normals(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let s = surface_at(in, front);
    let n = reflection_of(s, in, front, dpdx(in.world), dpdy(in.world));
    if s.alpha < alpha_cutoff() {
        discard;
    }
    return n;
}

@fragment
fn fs_depth_normals_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let s = surface_at(in, front);
    let n = reflection_of(s, in, front, dpdx(in.world), dpdy(in.world));
    if !fade_keeps(in.fade, in.clip.xy) || s.alpha < alpha_cutoff() {
        discard;
    }
    return n;
}

// Solid surfaces with multisampling.
@fragment
fn fs_normals_ms(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    let n = reflection_of(surface_at(in, front), in, front, dpdx(in.world), dpdy(in.world));
    return PrepassNormals(vec4<f32>(0.0), n);
}

@fragment
fn fs_normals_ms_fade(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    let n = reflection_of(surface_at(in, front), in, front, dpdx(in.world), dpdy(in.world));
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return PrepassNormals(vec4<f32>(0.0), n);
}

// Cut-out surfaces with multisampling: as `fs_depth_coverage`.
@fragment
fn fs_depth_coverage_normals(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    let s = surface_at(in, front);
    let n = reflection_of(s, in, front, dpdx(in.world), dpdy(in.world));
    return PrepassNormals(vec4<f32>(0.0, 0.0, 0.0, coverage(s.alpha)), n);
}

@fragment
fn fs_depth_coverage_normals_fade(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    let s = surface_at(in, front);
    let n = reflection_of(s, in, front, dpdx(in.world), dpdy(in.world));
    let a = coverage(s.alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return PrepassNormals(vec4<f32>(0.0, 0.0, 0.0, a), n);
}

// GPU-path crowds: no normal (the trace keeps the depth's), over whatever
// they hide: solid ones, cut-out ones as `fs_depth`, and with
// multisampling as `fs_depth_coverage`.
@fragment
fn fs_no_normal() -> @location(0) vec4<f32> {
    return NO_NORMAL;
}

@fragment
fn fs_no_normal_fade(in: VertexOut) -> @location(0) vec4<f32> {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return NO_NORMAL;
}

@fragment
fn fs_depth_no_normal(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
    return NO_NORMAL;
}

@fragment
fn fs_depth_no_normal_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let alpha = surface_at(in, front).alpha;
    if !fade_keeps(in.fade, in.clip.xy) || alpha < alpha_cutoff() {
        discard;
    }
    return NO_NORMAL;
}

@fragment
fn fs_no_normal_ms() -> PrepassNormals {
    return PrepassNormals(vec4<f32>(0.0), NO_NORMAL);
}

@fragment
fn fs_no_normal_ms_fade(in: VertexOut) -> PrepassNormals {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return PrepassNormals(vec4<f32>(0.0), NO_NORMAL);
}

@fragment
fn fs_depth_coverage_no_normal(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    return PrepassNormals(vec4<f32>(0.0, 0.0, 0.0, coverage(surface_at(in, front).alpha)), NO_NORMAL);
}

@fragment
fn fs_depth_coverage_no_normal_fade(in: VertexOut, @builtin(front_facing) front: bool) -> PrepassNormals {
    let a = coverage(surface_at(in, front).alpha);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return PrepassNormals(vec4<f32>(0.0, 0.0, 0.0, a), NO_NORMAL);
}

// The opaque pass's contact shadow at this pixel (see opaque_color).
var<private> contact_shadow: f32 = 0.0;

fn shade(s: Surface) -> vec3<f32> {
    if s.unlit { return s.albedo + s.emissive; }
    let n = s.normal;
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    var sun = 0.0;
    let facing_sun = dot(n, frame.sun_dir.xyz) > 0.0;
    if facing_sun || (MATERIAL_LAYERS && (transmission.diffuse > 0.0 || transmission.scatter > 0.0)) {
        sun = sunlight(s.world, select(-n, n, facing_sun)) * (1.0 - contact_shadow);
    }
    let direct = direct_light(s, frame.sun_dir.xyz, frame.sun_color.rgb * sun) + punctual_light(s);

    bounced_light = gi_light(s.world, n, s.view);
    // Baked light: a lightmap always, the probes where the real-time
    // probes don't reach.
    let baked = baked_light(s.world, n, s.uv1);
    if baked.a > 0.0 && (baked_entry > 0.5 || bounced_light.a <= 0.0) {
        bounced_light = baked;
    }
    let ambient = environment_light(s, f0);

    // Linear HDR light; the post pass maps it to the screen.
    return direct + ambient + s.emissive;
}

// Opaque: the prepass already cut the cut-outs.
@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Screen derivatives need uniform control flow: taken before any branch.
    return opaque_color(in, front, dpdx(in.world), dpdy(in.world));
}

// Opaque, for crossfading instances (see `fade_keeps`).
@fragment
fn fs_main_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

// The view model's cut-out held items without multisampling: in no
// prepass, so they cut their own holes and write their own depth.
@fragment
fn fs_main_cutout(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    if surface_at(in, front).alpha < alpha_cutoff() {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

@fragment
fn fs_main_cutout_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dp_x = dpdx(in.world);
    let dp_y = dpdy(in.world);
    // Sampled before the branch: `a || f()` only calls f where a is false,
    // non-uniform control flow, where WebGPU's WGSL forbids textureSample.
    let alpha = surface_at(in, front).alpha;
    if !fade_keeps(in.fade, in.clip.xy) || alpha < alpha_cutoff() {
        discard;
    }
    return opaque_color(in, front, dp_x, dp_y);
}

fn opaque_color(in: VertexOut, front: bool, dp_x: vec3<f32>, dp_y: vec3<f32>) -> vec4<f32> {
    var s = surface_at(in, front);
    var geometric = normalize(in.normal);
    if !front {
        geometric = -geometric;
    }
    s = apply_decals(s, geometric, in.decal_layers, dp_x, dp_y);
    baked_entry = in.lightmap;
    baked_face = geometric;
    s.roughness = clamp(s.roughness, 0.03, 1.0);
    s.metallic = clamp(s.metallic, 0.0, 1.0);
    // The opaque pass uses the otherwise unused scene-color slot for GTAO
    // occlusion (zero in the no-scene stand-in). Transparent passes retain
    // the actual scene color and never sample it as occlusion.
    // Green holds the contact shadow, which blocks only the sun.
    let ao_at = clamp(vec2<i32>(s.screen), vec2<i32>(0), vec2<i32>(textureDimensions(scene_color_texture)) - 1);
    let screen = textureLoad(scene_color_texture, ao_at, 0);
    s.occlusion *= 1.0 - screen.r;
    contact_shadow = screen.g;
    // Blue marks screen-space reflections in a second level, at half size;
    // alpha is their cone's spread (see ssr.wgsl), unless the spread image
    // has a slot of its own.
    if screen.b > 0.5 && s.roughness < 0.7 {
        let filtered = reflection_at(s.screen, s.roughness, reflection_spread(s.screen, screen.a));
        screen_reflection = vec4<f32>(filtered.rgb / max(filtered.a, 0.00001), filtered.a);
    }
    return vec4<f32>(fog(shade(s), s.world), 1.0);
}

// Transparent: blended over what's behind it (premultiplied alpha).
@fragment
fn fs_transparent(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return transparent_color(in, front);
}

@fragment
fn fs_transparent_fade(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if !fade_keeps(in.fade, in.clip.xy) {
        discard;
    }
    return transparent_color(in, front);
}

fn transparent_color(in: VertexOut, front: bool) -> vec4<f32> {
    let s = surface_at(in, front);
    baked_entry = in.lightmap;
    baked_face = select(-1.0, 1.0, front) * normalize(in.normal);
    if frame.fog_color.w > 0.0 && s.roughness < 0.7 {
        screen_reflection = transparent_reflection(s);
    }
    if MATERIAL_LAYERS && !s.unlit && material.layers[5].x > 0.0 {
        let coverage = material.layer_info.w;
        if coverage == 2u && s.alpha < 0.5 {
            discard;
        }
        let alpha = select(1.0, s.alpha, coverage == 1u);
        let lit = fog(shade(s), s.world) + specular_transmission(s);
        return vec4<f32>(lit * alpha, alpha);
    }
    return vec4<f32>(fog(shade(s), s.world) * s.alpha, s.alpha);
}

// ---- Screen-space reflections on transparent surfaces ----
// With `Post::ssr_transparent` (the strength in frame.fog_color.w, how far
// a ray looks in frame.night.w), a glossy transparent surface such as water
// or glass marches its reflected ray across the opaque scene behind it: the
// depth and color the split transparent pass copies (see scene_color). 24
// jittered steps in screen space (in the offscreen test, 32 drew slightly
// smoother edges, 16 ragged ones); a step that went behind something closer than
// twice its own length is a hit, then 4 halvings find the surface. It shows the opaque scene of this
// frame, sharp (no cone blur: `environment_light` fades reflections out on
// rough surfaces anyway), fading out toward the screen's edges and the end
// of the ray. Other transparent surfaces are not in it.
fn transparent_reflection(s: Surface) -> vec4<f32> {
    let ray = reflect(-s.view, s.normal);
    var reach = frame.night.w;
    let start = frame.view_proj * vec4<f32>(s.world, 1.0);
    var end = frame.view_proj * vec4<f32>(s.world + ray * reach, 1.0);
    // A ray toward the camera stops before it passes the near plane.
    let nearest = start.w * 0.1;
    if end.w < nearest {
        let t = (start.w - nearest) / (start.w - end.w);
        reach *= t;
        end = mix(start, end, t);
    }
    let size = frame.screen.xy;
    let a = vec3<f32>(start.xy / start.w * vec2<f32>(0.5, -0.5) + 0.5, start.z / start.w);
    let b = vec3<f32>(end.xy / end.w * vec2<f32>(0.5, -0.5) + 0.5, end.z / end.w);
    let far = s.world + ray * reach;
    // Positions over w and 1 / w change linearly across the screen.
    let world_a = vec4<f32>(s.world, 1.0) / start.w;
    let world_b = vec4<f32>(far, 1.0) / end.w;
    let jitter = hash2(s.screen);
    var before = 0.0;
    var hit = -1.0;
    var previous = s.world;
    for (var i = 0u; i < 24u; i++) {
        let k = (f32(i) + jitter) / 24.0;
        let p = mix(a, b, k);
        if any(p.xy < vec2<f32>(0.0)) || any(p.xy > vec2<f32>(1.0)) {
            break;
        }
        let h = mix(world_a, world_b, k);
        let point = h.xyz / h.w;
        // Reversed depth: the opaque scene is nearer than the ray where its
        // depth is greater.
        if opaque_depth_at(p.xy * size) > p.z {
            let gap = distance(point, scene_position(p.xy * size));
            if gap < max(2.0 * distance(point, previous), 0.05) {
                hit = k;
                break;
            }
        }
        before = k;
        previous = point;
    }
    if hit < 0.0 {
        return vec4<f32>(0.0);
    }
    var low = before;
    for (var j = 0; j < 4; j++) {
        let k = 0.5 * (low + hit);
        let p = mix(a, b, k);
        if opaque_depth_at(p.xy * size) > p.z {
            hit = k;
        } else {
            low = k;
        }
    }
    let p = mix(a, b, hit);
    let h = mix(world_a, world_b, hit);
    let travelled = distance(h.xyz / h.w, s.world) / max(reach, 0.0001);
    let edge = max(abs(p.x - 0.5), abs(p.y - 0.5)) * 2.0;
    let fade = (1.0 - smoothstep(0.9, 1.0, edge)) * (1.0 - smoothstep(0.8, 1.0, travelled));
    return vec4<f32>(scene_color(p.xy * size), fade * min(frame.fog_color.w, 1.0));
}

// The opaque scene's depth at a pixel (reversed: 0 is the far plane).
fn opaque_depth_at(screen: vec2<f32>) -> f32 {
    let at = min(vec2<i32>(screen), vec2<i32>(textureDimensions(scene_depth_texture)) - 1);
    return textureLoad(scene_depth_texture, at, 0);
}

// ---- Motion vectors (temporal antialiasing) ----
// Models that moved since last frame are drawn again, over the depth the
// opaque pass left, to say where each pixel was: the same vertices through
// last frame's model matrix, with `vertex` run as of the last frame (its
// `frame.time` and `items` are last frame's, see `motion_past_time`). A
// skinned or morphing model whose pose changed brings its last pose's
// positions in `previous_position`; every other mesh brings its own
// positions again. A buffer-driven mesh (see `Vertex.instance_index`) reads
// the last frame's elements only when its buffer keeps them; otherwise both
// runs see the same element and it moves with its entity.
struct VelocityIn {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) vertex_color: vec4<f32>,
    @location(9) uv1: vec2<f32>,
    @location(10) p0: vec4<f32>,
    @location(11) p1: vec4<f32>,
    @location(12) p2: vec4<f32>,
    @location(13) p3: vec4<f32>,
    @location(14) previous_position: vec3<f32>,
};

struct VelocityOut {
    // The same position as `vs_main`, so depth matches exactly.
    @builtin(position) @invariant clip: vec4<f32>,
    // Previous camera, with the previous object transforms (`before`) and
    // as the depth's reprojection sees the current ones (`rigid`).
    @location(0) before: vec4<f32>,
    @location(1) rigid: vec4<f32>,
};

@vertex
fn vs_velocity(in: VelocityIn) -> VelocityOut {
    var a: VertexIn;
    a.instance_index = in.instance_index;
    a.position = in.position;
    a.normal = in.normal;
    a.uv = in.uv;
    a.m0 = in.m0;
    a.m1 = in.m1;
    a.m2 = in.m2;
    a.m3 = in.m3;
    a.color = in.color;
    a.vertex_color = in.vertex_color;
    a.uv1 = in.uv1;
    var b = a;
    b.m0 = in.p0;
    b.m1 = in.p1;
    b.m2 = in.p2;
    b.m3 = in.p3;
    b.position = in.previous_position;
    let now = world_vertex(a);
    let before = world_vertex_before(b);
    var out: VelocityOut;
    out.clip = frame.view_proj * vec4<f32>(now.position, 1.0);
    out.rigid = frame.reprojection * vec4<f32>(now.position, 1.0);
    out.before = frame.previous_view_proj * vec4<f32>(before.position, 1.0);
    return out;
}

// `world_vertex` as of the last frame: `vertex` reads its time and buffer.
fn world_vertex_before(in: VertexIn) -> Vertex {
    motion_past_time = true;
    motion_past_items = true;
    let v = world_vertex(in);
    motion_past_time = false;
    motion_past_items = false;
    return v;
}

// Models whose matrix and pose held still, drawn with a shader whose
// `vertex` reads the last frame (wind, waves, a buffer that keeps its last
// frame): from the frame's own instances, `vertex` run as of this frame
// and the last. Models that also moved are drawn by `vs_velocity` after,
// over these.
@vertex
fn vs_velocity_still(input: VertexIn) -> VelocityOut {
    let in = entry_input(input, input.instance_index);
    let now = world_vertex(in);
    let before = world_vertex_before(in);
    var out: VelocityOut;
    out.clip = frame.view_proj * vec4<f32>(now.position, 1.0);
    out.rigid = frame.reprojection * vec4<f32>(now.position, 1.0);
    out.before = frame.previous_view_proj * vec4<f32>(before.position, 1.0);
    return out;
}

// Object-only correction to backward history lookup in texture coordinates,
// minus the camera-only motion; static pixels keep the clear value zero.
@fragment
fn fs_velocity(in: VelocityOut) -> @location(0) vec2<f32> {
    if in.before.w <= 1e-5 || in.rigid.w <= 1e-5 {
        return vec2<f32>(0.0);
    }
    let a = in.rigid.xy / in.rigid.w;
    let b = in.before.xy / in.before.w;
    return vec2<f32>((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
}

// The tangent of the cone around the mirror direction that holds most of
// the light a surface of `roughness` reflects: the angle that holds 0.244
// of a Phong lobe of the specular power that matches its GGX lobe (alpha,
// roughness squared). 0.001 for a mirror's 0.03, 0.05 at 0.2, 0.19 at 0.4.
fn reflection_cone(roughness: f32) -> f32 {
    let a = roughness * roughness;
    let power = 2.0 / max(a * a, 0.000001) - 2.0;
    let c = pow(0.244, 1.0 / (power + 1.0));
    return sqrt(max(1.0 - c * c, 0.0)) / c;
}

// The reflection cone's spread at full-size pixel `pixel`: read from the
// half-size spread image (see ssr.wgsl), where the device has a texture
// slot for it. Without one, the engine replaces these lines with a function
// that returns `copied`, the copy in the screen image's first level.
// ---- reflection spread image: begin ----
@group(2) @binding(3) var reflection_spread_texture: texture_2d<f32>;
fn reflection_spread(pixel: vec2<f32>, copied: f32) -> f32 {
    let size = vec2<i32>(textureDimensions(reflection_spread_texture));
    return textureLoad(reflection_spread_texture, min(vec2<i32>(pixel) / 2, size - 1), 0).r;
}
// ---- reflection spread image: end ----

// The screen-space reflections at full-size pixel `pixel` of a surface of
// `roughness` whose ray's cone spreads by `spread` (see ssr.wgsl):
// confidence-premultiplied radiance, blurred over the cone's footprint,
// its tangent times the spread, in full-size pixels. A reflection that
// starts close to what it shows stays sharp on a rough surface; one that
// travels far blurs. A pixel of level n is 2^n full-size pixels wide.
fn reflection_at(pixel: vec2<f32>, roughness: f32, spread: f32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene_color_texture));
    let last = min(5.0, floor(log2(max(size.x, size.y))));
    let footprint = reflection_cone(roughness) * spread;
    let lod = clamp(log2(max(footprint, 1.0)), 1.0, last);
    let uv = pixel / size;
    if lod < 2.0 {
        // Footprints narrower than a pixel of level 2: four taps of the
        // trace's own level, from one point apart to one of its pixels, so
        // the blur grows smoothly instead of mixing in level 2, which
        // spreads a thin bright detail over four full-size pixels at once.
        let o = (lod - 1.0) * 0.5 / vec2<f32>(textureDimensions(scene_color_texture, 1));
        return 0.25 * (reflection_level(uv + o, 1) + reflection_level(uv - o, 1)
            + reflection_level(uv + vec2<f32>(o.x, -o.y), 1) + reflection_level(uv + vec2<f32>(-o.x, o.y), 1));
    }
    let low = i32(floor(lod));
    var filtered = reflection_level(uv, low);
    if fract(lod) > 0.0 && low < i32(last) {
        filtered = mix(filtered, reflection_level(uv, low + 1), fract(lod));
    }
    return filtered;
}

// Reflection mips contain confidence-premultiplied radiance. Material
// shaders have no sampler of their own to spare: this borrows the sky's
// linear one, which wraps around horizontally, so the lookup stays half a
// pixel inside the edges.
fn reflection_level(uv: vec2<f32>, level: i32) -> vec4<f32> {
    let half = 0.5 / vec2<f32>(textureDimensions(scene_color_texture, level));
    return textureSampleLevel(scene_color_texture, sky_sampler, clamp(uv, half, 1.0 - half), f32(level));
}
