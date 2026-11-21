// The GPU baker (see gpu.rs): the CPU baker's path tracer (trace.rs and
// bvh.rs) as a compute shader. One invocation gathers one lightmap texel's
// or probe's light from a range of its samples and adds it to `out`.
//
// `scene` holds, from the offsets in `p`: the tree's nodes (two vec4s
// each: low corner and first index, high corner and count, as bits), the
// triangles in leaf order (three each: corner and triangle index, edges),
// per triangle six vec4s (its corners' normals, the first with its
// surface; its face normal; its corners' texture coordinates), per
// surface two (albedo and two-sidedness, emissive light and its albedo
// map or 0xffffffff), per lamp four (position and range, light and
// whether a spot, forward and the outer cosine, the inner cosine), per
// albedo map one (where its texels start, width, height) and then the
// maps' texels in linear color.

struct Params {
    nodes: u32,
    tris: u32,
    tri_count: u32,
    info: u32,
    surfaces: u32,
    lamps: u32,
    lamp_count: u32,
    maps: u32,
    // 1 when the sun shines.
    sun_dir: vec4<f32>,
    sun_light: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    // w: 1 with an equator color.
    equator: vec4<f32>,
    bias: f32,
    bounces: u32,
    // 0: lightmap texels (cosine around the normal), 1: probes (sphere).
    kind: u32,
    seed: u32,
    job_start: u32,
    job_count: u32,
    sample_start: u32,
    sample_count: u32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> scene: array<vec4<f32>>;
// Per job three vec4s: position, face normal, smoothed normal.
@group(0) @binding(2) var<storage, read> jobs: array<vec4<f32>>;
// Per job four vec4s, added to: a texel's light (rgb) and rays reaching a
// back face (w), light toward where it came from (xyz) and its weight (w),
// rays traced (x); a probe's sums of light (and back faces), times x, y, z.
@group(0) @binding(3) var<storage, read_write> out: array<vec4<f32>>;

const PI: f32 = 3.14159265358979;

var<private> rng: u32;
var<private> rays: u32;

fn hash(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn next() -> f32 {
    rng = hash(rng);
    return f32(rng >> 8u) / 16777216.0;
}

fn bits(v: f32) -> u32 {
    return bitcast<u32>(v);
}

fn nonzero(x: f32) -> f32 {
    if abs(x) < 1e-20 {
        return select(-1e-20, 1e-20, x >= 0.0);
    }
    return x;
}

// Where the ray enters the box before `t_max`, or -1.
fn enters(lo: vec3<f32>, hi: vec3<f32>, o: vec3<f32>, inv: vec3<f32>, t_max: f32) -> f32 {
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let a = min(t0, t1);
    let b = max(t0, t1);
    let near = max(max(max(a.x, a.y), a.z), 0.0);
    let far = min(min(min(b.x, b.y), b.z), t_max);
    return select(-1.0, near, near <= far);
}

struct Hit {
    found: bool,
    tri: u32,
    t: f32,
    u: f32,
    v: f32,
};

fn walk(o: vec3<f32>, d: vec3<f32>, t_far: f32, any: bool) -> Hit {
    var best: Hit;
    best.found = false;
    if p.tri_count == 0u {
        return best;
    }
    var t_max = t_far;
    let inv = vec3<f32>(1.0 / nonzero(d.x), 1.0 / nonzero(d.y), 1.0 / nonzero(d.z));
    var stack: array<u32, 64>;
    var top = 1u;
    stack[0] = 0u;
    while top > 0u {
        top -= 1u;
        let at = p.nodes + 2u * stack[top];
        let lo = scene[at];
        let hi = scene[at + 1u];
        if enters(lo.xyz, hi.xyz, o, inv, t_max) < 0.0 {
            continue;
        }
        let count = bits(hi.w);
        let first = bits(lo.w);
        if count > 0u {
            for (var k = first; k < first + count; k++) {
                let base = p.tris + 3u * k;
                let v0 = scene[base];
                let e1 = scene[base + 1u].xyz;
                let e2 = scene[base + 2u].xyz;
                let q0 = cross(d, e2);
                let det = dot(e1, q0);
                if abs(det) < 1e-12 {
                    continue;
                }
                let inv_det = 1.0 / det;
                let s = o - v0.xyz;
                let u = dot(s, q0) * inv_det;
                if u < 0.0 || u > 1.0 {
                    continue;
                }
                let q = cross(s, e1);
                let v = dot(d, q) * inv_det;
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let t = dot(e2, q) * inv_det;
                if t > 0.0 && t < t_max {
                    t_max = t;
                    best.found = true;
                    best.tri = bits(v0.w);
                    best.t = t;
                    best.u = u;
                    best.v = v;
                    if any {
                        return best;
                    }
                }
            }
            continue;
        }
        // Nearer child last, so it is walked first.
        let a = first;
        let b = first + 1u;
        let na = p.nodes + 2u * a;
        let nb = p.nodes + 2u * b;
        let da = enters(scene[na].xyz, scene[na + 1u].xyz, o, inv, t_max);
        let db = enters(scene[nb].xyz, scene[nb + 1u].xyz, o, inv, t_max);
        if top + 2u > 64u {
            continue;
        }
        if da >= 0.0 && db >= 0.0 {
            if db < da {
                stack[top] = a;
                stack[top + 1u] = b;
            } else {
                stack[top] = b;
                stack[top + 1u] = a;
            }
            top += 2u;
        } else if da >= 0.0 {
            stack[top] = a;
            top += 1u;
        } else if db >= 0.0 {
            stack[top] = b;
            top += 1u;
        }
    }
    return best;
}

fn sky_light(d: vec3<f32>) -> vec3<f32> {
    if p.equator.w > 0.5 {
        if d.y >= 0.0 {
            return mix(p.equator.xyz, p.sky.xyz, d.y);
        }
        return mix(p.equator.xyz, p.ground.xyz, -d.y);
    }
    return mix(p.ground.xyz, p.sky.xyz, d.y * 0.5 + 0.5);
}

// Light straight from the sun and the lamps at `x` facing `n`, shadowed.
fn direct(x: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    if p.sun_dir.w > 0.5 {
        let c = dot(n, p.sun_dir.xyz);
        if c > 0.0 {
            rays += 1u;
            if !walk(x, p.sun_dir.xyz, 3.4e38, true).found {
                sum += p.sun_light.xyz * c;
            }
        }
    }
    for (var i = 0u; i < p.lamp_count; i++) {
        let at = p.lamps + 4u * i;
        let pos = scene[at];
        let light = scene[at + 1u];
        let range = pos.w;
        let delta = pos.xyz - x;
        let d2 = dot(delta, delta);
        if d2 >= range * range || d2 <= 0.0 {
            continue;
        }
        let dist = sqrt(d2);
        let l = delta / dist;
        let c = dot(n, l);
        if c <= 0.0 {
            continue;
        }
        let ratio = d2 / (range * range);
        let window = max(1.0 - ratio * ratio, 0.0);
        var attenuation = window * window / max(d2, 1.0);
        if light.w > 0.5 {
            let cone = scene[at + 2u];
            let outer = cone.w;
            let inner = scene[at + 3u].x;
            let angle = dot(-l, cone.xyz);
            if inner == outer {
                attenuation *= select(0.0, 1.0, angle >= outer);
            } else {
                let t = clamp((angle - outer) / (max(inner, outer + 1e-6) - outer), 0.0, 1.0);
                attenuation *= t * t * (3.0 - 2.0 * t);
            }
        }
        if attenuation > 0.0 {
            rays += 1u;
            if !walk(x, l, dist - p.bias, true).found {
                sum += light.xyz * attenuation * c;
            }
        }
    }
    return sum;
}

fn cosine_sample(n: vec3<f32>, u1: f32, u2: f32) -> vec3<f32> {
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    let sign = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x);
    let bt = vec3<f32>(b, sign + n.y * n.y * a, -n.y);
    let d = t * (r * cos(phi)) + bt * (r * sin(phi)) + n * sqrt(max(1.0 - u1, 0.0));
    let len = length(d);
    return select(n, d / len, len > 0.0);
}

fn albedo_map(map: u32, uv: vec2<f32>) -> vec3<f32> {
    let head = scene[p.maps + map];
    let start = bits(head.x);
    let w = bits(head.y);
    let h = bits(head.z);
    let size = vec2<f32>(f32(w), f32(h));
    let q = uv * size - 0.5;
    let base = floor(q);
    let t = q - base;
    var c: array<vec3<f32>, 4>;
    for (var k = 0u; k < 4u; k++) {
        let o = vec2<f32>(f32(k & 1u), f32(k >> 1u));
        let at = base + o;
        let x = min(u32(at.x - size.x * floor(at.x / size.x)), w - 1u);
        let y = min(u32(at.y - size.y * floor(at.y / size.y)), h - 1u);
        c[k] = scene[start + y * w + x].xyz;
    }
    let top = c[0] + (c[1] - c[0]) * t.x;
    let bottom = c[2] + (c[3] - c[2]) * t.x;
    return top + (bottom - top) * t.y;
}

struct Incoming {
    light: vec3<f32>,
    back: bool,
};

// The light arriving at `origin` from `dir`, after up to `bounces`
// bounces: trace.rs's `incoming`, as a loop.
fn incoming(origin_in: vec3<f32>, dir_in: vec3<f32>) -> Incoming {
    var result: Incoming;
    result.light = vec3<f32>(0.0);
    result.back = false;
    var origin = origin_in;
    var dir = dir_in;
    var weight = vec3<f32>(1.0);
    for (var depth = 0u; ; depth++) {
        rays += 1u;
        let hit = walk(origin, dir, 3.4e38, false);
        if !hit.found {
            result.light += weight * sky_light(dir);
            break;
        }
        let info = p.info + 6u * hit.tri;
        let n0 = scene[info];
        let surface = p.surfaces + 2u * bits(n0.w);
        let s0 = scene[surface];
        let s1 = scene[surface + 1u];
        var ng = scene[info + 3u].xyz;
        var flip = false;
        if dot(ng, dir) > 0.0 {
            if s0.w < 0.5 {
                result.back = depth == 0u;
                break;
            }
            ng = -ng;
            flip = true;
        }
        result.light += weight * s1.xyz;
        var albedo = s0.xyz;
        let map = bits(s1.w);
        if map != 0xffffffffu {
            let uv01 = scene[info + 4u];
            let uv2 = scene[info + 5u].xy;
            let uv = uv01.xy * (1.0 - hit.u - hit.v) + uv01.zw * hit.u + uv2 * hit.v;
            albedo *= albedo_map(map, uv);
        }
        if depth >= p.bounces || all(albedo == vec3<f32>(0.0)) {
            break;
        }
        let n1 = scene[info + 1u].xyz;
        let n2 = scene[info + 2u].xyz;
        var n = n0.xyz * (1.0 - hit.u - hit.v) + n1 * hit.u + n2 * hit.v;
        let len = length(n);
        n = select(ng, n / len, len > 0.0);
        if flip {
            n = -n;
        }
        if dot(n, ng) <= 0.0 {
            n = ng;
        }
        let x = origin + dir * hit.t + ng * p.bias;
        let lit = direct(x, n);
        result.light += weight * albedo * (lit / PI);
        weight *= albedo;
        origin = x;
        dir = cosine_sample(n, next(), next());
    }
    return result;
}

// Sample `i` of the R2 sequence, shifted by `offset`.
fn r2(i: u32, offset: vec2<f32>) -> vec2<f32> {
    // The fractions of i times a1 and a2, kept exact in 32 bits: a1 and a2
    // as fixed point.
    let x = f32(i * 3242174889u) / 4294967296.0;
    let y = f32(i * 2447445414u) / 4294967296.0;
    return fract(offset + vec2<f32>(x, y));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.job_count {
        return;
    }
    let job = p.job_start + id.x;
    let position = jobs[3u * job].xyz;
    let face = jobs[3u * job + 1u].xyz;
    let normal = jobs[3u * job + 2u].xyz;
    rng = hash(p.seed ^ hash(job * 2u + p.kind));
    let offset = vec2<f32>(next(), next());
    rays = 0u;
    var a = vec4<f32>(0.0);
    var b = vec4<f32>(0.0);
    var c = vec4<f32>(0.0);
    var d = vec4<f32>(0.0);
    for (var s = p.sample_start; s < p.sample_start + p.sample_count; s++) {
        rng = hash(hash(p.seed ^ hash(job * 2u + p.kind)) ^ (s * 2654435761u + 1u));
        let u = r2(s, offset);
        if p.kind == 0u {
            let origin = position + face * p.bias;
            var dir = cosine_sample(normal, u.x, u.y);
            if dot(dir, face) <= 0.0 {
                let m = dir - face * (2.0 * dot(dir, face));
                dir = select(face, normalize(m), dot(m, m) > 0.0);
            }
            let got = incoming(origin, dir);
            let lum = dot(got.light, vec3<f32>(0.2126, 0.7152, 0.0722));
            a += vec4<f32>(got.light, select(0.0, 1.0, got.back));
            b += vec4<f32>(dir * lum, lum);
        } else {
            let z = 1.0 - 2.0 * u.x;
            let r = sqrt(max(1.0 - z * z, 0.0));
            let phi = 2.0 * PI * u.y;
            let dir = vec3<f32>(r * cos(phi), z, r * sin(phi));
            let got = incoming(position, dir);
            a += vec4<f32>(got.light, select(0.0, 1.0, got.back));
            b += vec4<f32>(got.light * dir.x, 0.0);
            c += vec4<f32>(got.light * dir.y, 0.0);
            d += vec4<f32>(got.light * dir.z, 0.0);
        }
    }
    let o = 4u * job;
    out[o] += a;
    out[o + 1u] += b;
    if p.kind == 0u {
        out[o + 2u] += vec4<f32>(f32(rays), 0.0, 0.0, 0.0);
    } else {
        out[o + 2u] += c;
        out[o + 3u] += d + vec4<f32>(0.0, 0.0, 0.0, f32(rays));
    }
}
