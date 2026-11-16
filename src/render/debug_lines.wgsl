// Debug lines (`ctx.debug`): one instance per line, six vertices making a
// quad `width` pixels wide around it, in the lit 3D frame.

struct Camera {
    view_proj: mat4x4<f32>,
    // xy: the target's size in pixels.
    viewport: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct Line {
    @builtin(vertex_index) vertex: u32,
    @location(0) a: vec3<f32>,
    @location(1) width: f32,
    @location(2) b: vec3<f32>,
    // sRGB, 8 bits a channel.
    @location(3) color: vec4<f32>,
};

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vs(line: Line) -> Out {
    var out: Out;
    out.color = vec4<f32>(to_linear(line.color.rgb), line.color.a);
    var a = camera.view_proj * vec4<f32>(line.a, 1.0);
    var b = camera.view_proj * vec4<f32>(line.b, 1.0);
    // Reverse-Z: the near plane is z = w. Cut the part in front of it, so a
    // line passing the camera keeps the rest.
    let near_a = a.w - a.z;
    let near_b = b.w - b.z;
    if (near_a < 0.0 && near_b < 0.0) || (a.w <= 0.0 && b.w <= 0.0) {
        out.position = vec4<f32>(0.0, 0.0, -1.0, 1.0); // outside: dropped
        return out;
    }
    if near_a < 0.0 {
        a = mix(a, b, near_a / (near_a - near_b));
    } else if near_b < 0.0 {
        b = mix(b, a, near_b / (near_b - near_a));
    }
    // Corners 0..6: two triangles. `end` 0 is a, 1 is b; `side` across.
    let v = line.vertex;
    let end = select(0.0, 1.0, v == 2u || v == 3u || v == 5u);
    let side = select(-1.0, 1.0, v == 1u || v == 4u || v == 5u);
    let half_size = camera.viewport.xy * 0.5;
    let sa = a.xy / a.w * half_size;
    let sb = b.xy / b.w * half_size;
    let d = sb - sa;
    let len = length(d);
    let dir = select(vec2<f32>(1.0, 0.0), d / len, len > 1e-6);
    let across = vec2<f32>(-dir.y, dir.x);
    let half_width = max(line.width, 1.0) * 0.5;
    // Square ends, half a width past each point.
    let offset = (across * side + dir * (end * 2.0 - 1.0)) * half_width / half_size;
    let p = mix(a, b, end);
    // A little nearer than the surface it lies on (0.05% of the distance),
    // so lines drawn on a surface aren't half hidden by it.
    let z = min(p.z * 1.0005, p.w);
    out.position = vec4<f32>(p.xy + offset * p.w, z, p.w);
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}
