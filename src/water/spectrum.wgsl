// Waves from a spectrum, first pass (the water kit's compute pass over
// the field): the spectrum at this frame's time, from its random start.
// custom[0]: the field's texels across, the square's side in metres, the
// choppiness, the period the waves repeat in (seconds).
// custom_texture0: the start, h0(k) (rg) and conj(h0(-k)) (ba).
// Out: A = h + i·Dx (rg) and B = Dz + i·dh/dt (ba), whose inverse FFTs
// are the height, the sway and the rising speed (see `spectrum.rs`).

fn spectrum_mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn compute(c: Cell) -> vec4<f32> {
    let settings = material.custom[0];
    let n = c.size.x;
    let half = n / 2;
    // Wave numbers in the FFT's order: 0..n/2 up, then the negative ones.
    let m = vec2<f32>(select(c.pixel, c.pixel - vec2<i32>(n), c.pixel >= vec2<i32>(half)));
    let k = m * (6.2831853 / settings.y);
    let kl = length(k);
    let start = textureLoad(custom_texture0, c.pixel, 0);
    // Deep-water speed, rounded to whole turns per period (as `omega`).
    let turn = 6.2831853 / settings.w;
    let w = floor(sqrt(9.81 * kl) / turn) * turn;
    let t = frame.time.x - floor(frame.time.x / settings.w) * settings.w;
    let e = vec2<f32>(cos(w * t), sin(w * t));
    let up = spectrum_mul(start.xy, e);
    let down = spectrum_mul(start.zw, vec2<f32>(e.x, -e.y));
    let h = up + down;
    var u = vec2<f32>(0.0);
    if kl > 0.0 {
        u = k / kl;
    }
    let chop = settings.z;
    let a = h * (1.0 + chop * u.x);
    let b = spectrum_mul(h, vec2<f32>(0.0, -chop * u.y)) - (up - down) * w;
    return vec4<f32>(a, b);
}
