// A painted sky for the stylized_sky example: a gradient in a few bands
// from the horizon up, flat bands of cloud, and a ground colour below. The
// game sets the colours through `Sky::shader_params` as the day goes by:
//
//   frame.sky_params[0]: rgb the horizon, w unused
//   frame.sky_params[1]: rgb overhead,    w cloud cover (0 to 1)
//   frame.sky_params[2]: rgb the ground,  w unused
//   frame.sky_params[3]: rgb the clouds,  w unused
//
// The engine adds the sun's disk and the stars, and uses this sky for the
// fog, the reflections and the ambient light too.

fn sky(dir: vec3<f32>) -> vec3<f32> {
    let horizon = frame.sky_params[0].rgb;
    let overhead = frame.sky_params[1].rgb;
    let up = clamp(dir.y, 0.0, 1.0);
    // Six bands, wider towards the top.
    let band = floor(sqrt(up) * 6.0) / 5.0;
    var light = mix(horizon, overhead, clamp(band, 0.0, 1.0));
    // Clouds on a flattened dome, cut into a hard-edged shape.
    if dir.y > 0.0 {
        let p = dir.xz / (dir.y + 0.25) * 1.2;
        let cover = frame.sky_params[1].w;
        let shape = fbm2(p + vec2<f32>(frame.time.x * 0.004, 0.0));
        let cloud = step(1.0 - cover * 0.6, shape) * smoothstep(0.0, 0.15, dir.y);
        light = mix(light, frame.sky_params[3].rgb, cloud);
    }
    // Below the horizon: the ground's colour.
    return mix(light, frame.sky_params[2].rgb, smoothstep(0.0, -0.04, dir.y));
}
