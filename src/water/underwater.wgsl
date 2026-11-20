
// The underwater view (the water kit's screen effect). It runs only while
// the camera may be under the surface; pixels whose ray starts above it
// are left alone. The material's values:
// custom[0]: absorption per metre (rgb), the rest level
// custom[1]: the water's own colour (linear rgb), caustics strength

// Bright, wandering lines of focused sunlight on what lies under water.
fn water_caustics(xz: vec2<f32>, t: f32) -> f32 {
    let a = noise2(xz * 1.4 + vec2<f32>(t * 0.35, t * 0.2));
    let b = noise2(xz * 1.9 - vec2<f32>(t * 0.25, -t * 0.3) + 11.0);
    return pow(1.0 - min(abs(a - b) * 2.5, 1.0), 6.0);
}

fn effect(p: Pixel) -> vec3<f32> {
    let level = material.custom[0].w;
    let absorb = material.custom[0].rgb;
    let t = frame.time.x;
    let camera = frame.camera.xyz;
    // The ray through this pixel (sky pixels have no position).
    let ndc = vec2<f32>(p.uv.x * 2.0 - 1.0, 1.0 - p.uv.y * 2.0);
    let mid = frame.inv_view_proj * vec4<f32>(ndc, 0.5, 1.0);
    let dir = normalize(mid.xyz / mid.w - camera);
    let start = camera + dir * 0.1;
    let surface_here = level + water_height(start.xz, t);
    if start.y >= surface_here {
        return p.color;
    }
    // The water between the camera and what it sees, up to the surface.
    var span = select(p.distance, 1e4, p.sky);
    if dir.y > 1e-4 {
        span = min(span, max((surface_here - camera.y) / dir.y, 0.0));
    }
    let through = exp(-absorb * span);
    // Daylight dims with depth on its way down.
    let depth = max(level - camera.y, 0.0);
    let light = (frame.sky.rgb + frame.sun_color.rgb * max(frame.sun_dir.y, 0.0) * 0.3) * exp(-absorb * depth);
    let haze = material.custom[1].rgb * light;
    // The view wobbles a little.
    let wobble = vec2<f32>(sin(p.screen.y * 0.045 + t * 2.1), cos(p.screen.x * 0.05 + t * 1.7)) * frame.screen.y * 0.003;
    var seen = scene_color(p.screen + wobble);
    if !p.sky && p.world.y < surface_here {
        let under = level - p.world.y;
        seen *= 1.0 + water_caustics(p.world.xz, t) * material.custom[1].w * exp(-under * 0.3) * max(frame.sun_dir.y, 0.0);
    }
    return seen * through + haze * (1.0 - through);
}
