// Area light shading, measured against the engine's point lights (see
// `area_lights_render_test.rs`). Not part of the renderer: a full-screen
// pass shades a procedural surface with every light in the records texture,
// as a pixel of the clustered path shades every light in its froxel.

const PI: f32 = 3.14159265;

struct Params {
    eye: vec4<f32>,
    // x: lights, y: 1 for diffuse only, z: reference samples per side
    count: vec4<u32>,
    // x, y: target size in pixels; z: surface roughness (0: varies)
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Four texels a light, one row each, like the engine's punctual records:
// 0: centre, range; 1: rgb radiance (point: intensity); 2: half axis A
// (tube: half the segment), w: tube radius; 3: half axis B.
@group(0) @binding(1) var records: texture_2d<f32>;
// Linearly transformed cosines tables (64x64): the inverse matrix, and the
// Fresnel scale and bias.
@group(0) @binding(2) var ltc_matrix: texture_2d<f32>;
@group(0) @binding(3) var ltc_fresnel: texture_2d<f32>;
@group(0) @binding(4) var ltc_sampler: sampler;

struct Surface {
    world: vec3<f32>,
    normal: vec3<f32>,
    view: vec3<f32>,
    albedo: vec3<f32>,
    roughness: f32,
    metallic: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn surface(frag: vec4<f32>) -> Surface {
    let uv = frag.xy / params.size.xy;
    var s: Surface;
    s.world = vec3<f32>(uv.x * 16.0 - 8.0, 0.0, uv.y * 9.0 - 4.5);
    s.normal = normalize(vec3<f32>(sin(uv.x * 37.0) * 0.15, 1.0, cos(uv.y * 23.0) * 0.15));
    s.view = normalize(params.eye.xyz - s.world);
    s.albedo = vec3<f32>(0.8, 0.7, 0.6);
    s.roughness = select(mix(0.25, 0.9, fract(uv.x * 5.0 + uv.y * 3.0)), params.size.z, params.size.z > 0.0);
    s.metallic = 0.0;
    return s;
}

fn diffuse_only() -> bool {
    return params.count.y == 1u;
}

// The engine's GGX direct light (`direct_light` in mesh.wgsl); diffuse
// only for comparisons of the diffuse term.
fn direct_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    let h = (l + s.view) / max(length(l + s.view), 1e-6);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(s.view, h), 0.0), 5.0);
    // Diffuse only is pure Lambert, as the area methods' form factors: the
    // engine's (1 - F) dims it by about 4 % more.
    let kept = select(vec3<f32>(1.0) - f, vec3<f32>(1.0), diffuse_only());
    let diffuse = kept * (1.0 - s.metallic) * s.albedo / PI * max(dot(s.normal, l), 0.0);
    return (diffuse + ggx_specular(s, l, 1.0)) * radiance;
}

// GGX specular times the cosine toward `l`, the lobe's peak scaled by
// `norm` (a representative point's energy fix); zero for diffuse only.
fn ggx_specular(s: Surface, l: vec3<f32>, norm: f32) -> vec3<f32> {
    if diffuse_only() { return vec3<f32>(0.0); }
    let n = s.normal;
    let v = s.view;
    let h = (l + v) / max(length(l + v), 1e-6);
    let n_l = max(dot(n, l), 0.0);
    let n_v = max(dot(n, v), 1e-4);
    let n_h = max(dot(n, h), 0.0);
    let v_h = max(dot(v, h), 0.0);
    let a = s.roughness * s.roughness;
    let a2 = a * a;
    let d_denom = n_h * n_h * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * d_denom * d_denom);
    let k = (s.roughness + 1.0) * (s.roughness + 1.0) / 8.0;
    let g = n_v / (n_v * (1.0 - k) + k) * n_l / (n_l * (1.0 - k) + k);
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - v_h, 5.0);
    return d * g * f / max(4.0 * n_v * n_l, 1e-4) * norm * n_l;
}

fn record(id: i32, texel: i32) -> vec4<f32> {
    return textureLoad(records, vec2<i32>(texel, id), 0);
}

// The engine's point light, as `punctual_light` shades one (four record
// loads, the range window, inverse-square falloff, GGX).
fn point_light(s: Surface, id: i32) -> vec3<f32> {
    let pr = record(id, 0);
    let delta = pr.xyz - s.world;
    let d2 = dot(delta, delta);
    if d2 >= pr.w * pr.w { return vec3<f32>(0.0); }
    let l = delta / max(sqrt(d2), 1e-6);
    let cone = record(id, 2);
    var attenuation = 1.0;
    if cone.w > -1.0 {
        let inner = record(id, 3).x;
        let angle = dot(-l, cone.xyz);
        attenuation = smoothstep(cone.w, max(inner, cone.w + 1e-6), angle);
    }
    let window = max(1.0 - (d2 / (pr.w * pr.w)) * (d2 / (pr.w * pr.w)), 0.0);
    attenuation *= window * window / max(d2, 1.0);
    return direct_light(s, l, record(id, 1).rgb * attenuation);
}

// The range window of an area light, by the distance to its centre.
fn window(pr: vec4<f32>, world: vec3<f32>) -> f32 {
    let delta = pr.xyz - world;
    let d2 = dot(delta, delta);
    let w = max(1.0 - (d2 / (pr.w * pr.w)) * (d2 / (pr.w * pr.w)), 0.0);
    return w * w;
}

// Edge integral of the clamped cosine over a polygon (Lambert's formula,
// as `IntegrateEdgeVec` of Hill and Heitz 2017): the vector form factor of
// one edge, in the frame the corners are given in.
fn edge(v1: vec3<f32>, v2: vec3<f32>) -> vec3<f32> {
    let x = dot(v1, v2);
    let y = abs(x);
    let a = 0.8543985 + (0.4965155 + 0.0145206 * y) * y;
    let b = 3.4175940 + (4.1616724 + y) * y;
    let v = a / b;
    let theta_sintheta = select(0.5 * inverseSqrt(max(1.0 - x * x, 1e-7)) - v, v, x > 0.0);
    return cross(v1, v2) * theta_sintheta;
}

// The form factor of the quad `corners` (relative to the shaded point, in
// the frame where z is the cosine lobe's axis), with the horizon handled by
// the sphere approximation of Hill and Heitz 2017 (no clipping).
fn quad_form_factor(c0: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> f32 {
    let l0 = normalize(c0);
    let l1 = normalize(c1);
    let l2 = normalize(c2);
    let l3 = normalize(c3);
    let f = edge(l0, l1) + edge(l1, l2) + edge(l2, l3) + edge(l3, l0);
    let len = length(f);
    // The clipped sphere's form factor (the analytic fit three.js uses
    // in place of a third table), one-sided.
    return max((len * len + f.z) / (len + 1.0), 0.0);
}

// The exact clamped-cosine form factor of the quad, unclipped: what the
// diffuse term of both rectangle methods uses (identity LTC).
fn rect_corners(id: i32, world: vec3<f32>) -> array<vec3<f32>, 4> {
    let c = record(id, 0).xyz - world;
    let a = record(id, 2).xyz;
    let b = record(id, 3).xyz;
    return array<vec3<f32>, 4>(c - a - b, c - a + b, c + a + b, c + a - b);
}

// A rectangle light with linearly transformed cosines (Heitz et al. 2016,
// the horizon by Hill and Heitz 2017): two table lookups, the four corners
// into the cosine's frame twice (diffuse with the identity, specular with
// the table's inverse matrix), four edge integrals each.
fn rect_ltc(s: Surface, id: i32) -> vec3<f32> {
    let pr = record(id, 0);
    let w = window(pr, s.world);
    if w <= 0.0 { return vec3<f32>(0.0); }
    let radiance = record(id, 1).rgb * w;
    let n = s.normal;
    let v = s.view;
    let n_v = clamp(dot(n, v), 1e-4, 1.0);
    let t1 = normalize(v - n * dot(v, n));
    let t2 = cross(n, t1);
    let corners = rect_corners(id, s.world);
    // To the tangent frame (rows t1, t2, n).
    var local: array<vec3<f32>, 4>;
    for (var i = 0; i < 4; i++) {
        local[i] = vec3<f32>(dot(corners[i], t1), dot(corners[i], t2), dot(corners[i], n));
    }
    let diffuse = quad_form_factor(local[0], local[1], local[2], local[3]);
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metallic);
    var color = s.albedo * (1.0 - s.metallic) * diffuse;
    if !diffuse_only() {
        let uv = vec2<f32>(s.roughness, sqrt(1.0 - n_v)) * (63.0 / 64.0) + 0.5 / 64.0;
        let m = textureSampleLevel(ltc_matrix, ltc_sampler, uv, 0.0);
        let fresnel = textureSampleLevel(ltc_fresnel, ltc_sampler, uv, 0.0);
        let inv = mat3x3<f32>(vec3<f32>(m.x, 0.0, m.y), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(m.z, 0.0, m.w));
        let spec = quad_form_factor(inv * local[0], inv * local[1], inv * local[2], inv * local[3]);
        color += (f0 * fresnel.x + (vec3<f32>(1.0) - f0) * fresnel.y) * spec;
    }
    return color * radiance;
}

// A rectangle light with a representative point (Drobot 2014, Karis 2013):
// diffuse from the exact form factor (four edge integrals in world space),
// specular from the point of the rectangle nearest the reflected ray, with
// the lobe widened by the rectangle's size.
fn rect_point(s: Surface, id: i32) -> vec3<f32> {
    let pr = record(id, 0);
    let w = window(pr, s.world);
    if w <= 0.0 { return vec3<f32>(0.0); }
    let radiance = record(id, 1).rgb * w;
    let corners = rect_corners(id, s.world);
    let diffuse = max(dot(
        edge(normalize(corners[0]), normalize(corners[1])) + edge(normalize(corners[1]), normalize(corners[2]))
            + edge(normalize(corners[2]), normalize(corners[3])) + edge(normalize(corners[3]), normalize(corners[0])),
        s.normal,
    ), 0.0);
    // Lambert's BRDF times the irradiance (pi * L * form factor).
    var color = s.albedo * (1.0 - s.metallic) * diffuse;
    if !diffuse_only() {
        let a = record(id, 2).xyz;
        let b = record(id, 3).xyz;
        let c = pr.xyz - s.world;
        let r = reflect(-s.view, s.normal);
        let plane = normalize(cross(a, b));
        let denom = dot(r, plane);
        var p = c;
        if abs(denom) > 1e-4 {
            let t = dot(c, plane) / denom;
            p = s.world + r * max(t, 0.0) - pr.xyz;
            let u = clamp(dot(p, a) / dot(a, a), -1.0, 1.0);
            let k = clamp(dot(p, b) / dot(b, b), -1.0, 1.0);
            p = c + a * u + b * k;
        }
        let d = length(p);
        let l = p / max(d, 1e-6);
        let size = sqrt(length(a) * length(b));
        let alpha = s.roughness * s.roughness;
        let norm = pow(alpha / clamp(alpha + size / (2.0 * d), 0.0, 1.0), 2.0);
        // Specular only: the diffuse came from the form factor. The solid
        // angle stands in for 1 / d^2 times the area.
        let solid = 4.0 * length(a) * length(b) * max(dot(-l, plane), 0.0) / max(d * d, 1e-4);
        color += ggx_specular(s, l, norm) * solid;
    }
    return color * radiance;
}

// A tube light (Karis 2013): a segment's diffuse from the average cosine
// at its ends and the exact inverse-cube integral, specular from the point
// of the segment nearest the reflected ray, moved by the radius toward it.
fn tube_point(s: Surface, id: i32) -> vec3<f32> {
    let pr = record(id, 0);
    let w = window(pr, s.world);
    if w <= 0.0 { return vec3<f32>(0.0); }
    let axis = record(id, 2);
    let radiance = record(id, 1).rgb * w;
    let l0 = pr.xyz - axis.xyz - s.world;
    let l1 = pr.xyz + axis.xyz - s.world;
    let ld = l1 - l0;
    let len0 = length(l0);
    let len1 = length(l1);
    let n = s.normal;
    let n_l = clamp(0.5 * (dot(n, l0) / len0 + dot(n, l1) / len1), 0.0, 1.0);
    // Integral of 1 / |x|^3 along the segment, times its length: a line of
    // intensity 2 * radius * radiance per unit length (its projected width).
    let falloff = 2.0 / (len0 * len1 + dot(l0, l1));
    let line = 2.0 * axis.w * length(ld);
    var color = s.albedo * (1.0 - s.metallic) / PI * n_l * falloff * line;
    if !diffuse_only() {
        let r = reflect(-s.view, n);
        let rd = dot(r, ld);
        let t = clamp((dot(r, l0) * rd - dot(l0, ld)) / max(dot(ld, ld) - rd * rd, 1e-6), 0.0, 1.0);
        var p = l0 + ld * t;
        let to_ray = dot(p, r) * r - p;
        p += to_ray * clamp(axis.w / max(length(to_ray), 1e-6), 0.0, 1.0);
        let d = length(p);
        let l = p / max(d, 1e-6);
        let alpha = s.roughness * s.roughness;
        let norm = alpha / clamp(alpha + 0.5 * (length(ld) * 0.5 + axis.w) / d, 0.0, 1.0);
        // The line's intensity with the diffuse term's falloff.
        color += ggx_specular(s, l, norm) * line * falloff;
    }
    return color * radiance;
}

fn hash(p: vec2<u32>) -> f32 {
    var h = p.x * 1664525u + p.y * 1013904223u;
    h ^= h >> 16u;
    h *= 2246822519u;
    h ^= h >> 13u;
    return f32(h & 0xffffu) / 65536.0;
}

// Ground truth: the light's surface sampled on a jittered grid (n x n),
// each sample shaded as a small point light with the engine's GGX. A
// rectangle (axes A, B) emits from its side facing along A x B; a tube
// emits from its whole cylinder.
fn reference(s: Surface, id: i32, frag: vec2<u32>) -> vec3<f32> {
    let pr = record(id, 0);
    let w = window(pr, s.world);
    let radiance = record(id, 1).rgb * w;
    let axis = record(id, 2);
    let b = record(id, 3).xyz;
    let tube = axis.w > 0.0;
    let n = params.count.z;
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < n; i++) {
        for (var j = 0u; j < n; j++) {
            let u = (f32(i) + hash(frag + vec2<u32>(i * 7u, j * 13u + 1u))) / f32(n) * 2.0 - 1.0;
            let k = (f32(j) + hash(frag + vec2<u32>(j * 5u + 3u, i * 11u))) / f32(n);
            var q: vec3<f32>;
            var normal: vec3<f32>;
            var area: f32;
            if tube {
                let along = normalize(axis.xyz);
                let side = normalize(cross(along, select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(along.y) > 0.9)));
                let up = cross(along, side);
                let angle = k * 2.0 * PI;
                normal = side * cos(angle) + up * sin(angle);
                q = pr.xyz + axis.xyz * u + normal * axis.w;
                area = 2.0 * PI * axis.w * 2.0 * length(axis.xyz);
            } else {
                normal = normalize(cross(axis.xyz, b));
                q = pr.xyz + axis.xyz * u + b * (k * 2.0 - 1.0);
                area = 4.0 * length(axis.xyz) * length(b);
            }
            let delta = q - s.world;
            let d2 = dot(delta, delta);
            let l = delta / sqrt(d2);
            let emit = max(dot(normal, -l), 0.0);
            sum += direct_light(s, l, radiance * emit * area / (d2 * f32(n * n)));
        }
    }
    return sum;
}

@fragment
fn fs_point(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let s = surface(frag);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < params.count.x; i++) {
        sum += point_light(s, i32(i));
    }
    return vec4<f32>(sum, 1.0);
}

@fragment
fn fs_rect_ltc(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let s = surface(frag);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < params.count.x; i++) {
        sum += rect_ltc(s, i32(i));
    }
    return vec4<f32>(sum, 1.0);
}

@fragment
fn fs_rect_point(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let s = surface(frag);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < params.count.x; i++) {
        sum += rect_point(s, i32(i));
    }
    return vec4<f32>(sum, 1.0);
}

@fragment
fn fs_tube(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let s = surface(frag);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < params.count.x; i++) {
        sum += tube_point(s, i32(i));
    }
    return vec4<f32>(sum, 1.0);
}

@fragment
fn fs_reference(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let s = surface(frag);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < params.count.x; i++) {
        sum += reference(s, i32(i), vec2<u32>(frag.xy));
    }
    return vec4<f32>(sum, 1.0);
}
