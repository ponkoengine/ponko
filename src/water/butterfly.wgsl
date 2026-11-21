// Waves from a spectrum, the FFT (the water kit's compute passes over the
// field): one stage of a Stockham inverse FFT of both packed signals,
// along x or z (as `butterfly` in the reference FFT of `spectrum.rs`'s tests).
// custom[0]: the field's texels across, the stage's size (1, 2, 4, ...
// n/2), 0 along x or 1 along z.

fn butterfly_mul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn compute(c: Cell) -> vec4<f32> {
    let settings = material.custom[0];
    let n = i32(settings.x);
    let ns = i32(settings.y);
    let along_z = settings.z > 0.5;
    let o = select(c.pixel.x, c.pixel.y, along_z);
    let r = (o % (2 * ns)) / ns;
    let j = (o / (2 * ns)) * ns + o % ns;
    var pa = vec2<i32>(j, c.pixel.y);
    var pb = vec2<i32>(j + n / 2, c.pixel.y);
    if along_z {
        pa = vec2<i32>(c.pixel.x, j);
        pb = vec2<i32>(c.pixel.x, j + n / 2);
    }
    let a = cell_at(pa);
    let b = cell_at(pb);
    let angle = 6.2831853 * f32(j % ns) / f32(2 * ns);
    let w = vec2<f32>(cos(angle), sin(angle));
    let bw = vec4<f32>(butterfly_mul(b.xy, w), butterfly_mul(b.zw, w));
    return select(a + bw, a - bw, r == 1);
}
