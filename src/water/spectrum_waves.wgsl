// The waves from a spectrum (generated constants first: WATER_TILE, the
// square's side in metres, and WATER_CELLS, its texels across). The field
// is the material's base texture, worked out each frame by the water kit's
// FFT passes (or uploaded from the CPU): height, sway along x and z, and
// how fast the surface rises. It repeats every WATER_TILE metres.
struct WaterWave {
    height: f32,      // above the rest level
    normal: vec3<f32>,
    squeeze: f32,     // 1 on flat water, towards 0 on sharp crests
};

// The field at ground point `p`, filtered between texels and wrapped
// around the square (as the CPU's `Field::at`).
fn water_field(p: vec2<f32>) -> vec4<f32> {
    let g = p / WATER_TILE * f32(WATER_CELLS);
    let c = floor(g);
    let f = g - c;
    let n = WATER_CELLS;
    let x0 = ((i32(c.x) % n) + n) % n;
    let z0 = ((i32(c.y) % n) + n) % n;
    let x1 = (x0 + 1) % n;
    let z1 = (z0 + 1) % n;
    let a = mix(textureLoad(base_texture, vec2<i32>(x0, z0), 0), textureLoad(base_texture, vec2<i32>(x1, z0), 0), f.x);
    let b = mix(textureLoad(base_texture, vec2<i32>(x0, z1), 0), textureLoad(base_texture, vec2<i32>(x1, z1), 0), f.x);
    return mix(a, b, f.y);
}

// Where the water at rest point `p` is at time `t` (x, height, z). The
// field is this frame's (drawn on the simulation's clock): `t` is unused.
fn water_displace(p: vec2<f32>, t: f32) -> vec3<f32> {
    let f = water_field(p);
    return vec3<f32>(p.x + f.y, f.x, p.y + f.z);
}

// The surface over rest point `p`, its slopes taken `e` metres apart.
fn water_wave_over(p: vec2<f32>, e: f32) -> WaterWave {
    let xp = water_displace(p + vec2<f32>(e, 0.0), 0.0);
    let xm = water_displace(p - vec2<f32>(e, 0.0), 0.0);
    let zp = water_displace(p + vec2<f32>(0.0, e), 0.0);
    let zm = water_displace(p - vec2<f32>(0.0, e), 0.0);
    let tx = xp - xm;
    let tz = zp - zm;
    let n = normalize(cross(tz, tx));
    let j = vec4<f32>(tx.x, tz.z, tz.x, tx.z) / (2.0 * e);
    return WaterWave(water_field(p).x, n, max(j.x * j.y - j.z * j.w, 0.0));
}

// The surface over rest point `p` at time `t`.
fn water_wave(p: vec2<f32>, t: f32) -> WaterWave {
    return water_wave_over(p, WATER_TILE / f32(WATER_CELLS));
}

// The same as seen where a pixel covers `footprint` metres: slopes taken
// over the pixel's width at least, so waves too short for the pixels
// flatten out instead of flickering.
fn water_wave_seen(p: vec2<f32>, t: f32, footprint: f32) -> WaterWave {
    return water_wave_over(p, max(WATER_TILE / f32(WATER_CELLS), footprint * 1.5));
}

// The rest point whose water is over `at` at time `t` (four fixed-point steps).
fn water_rest_point(at: vec2<f32>, t: f32) -> vec2<f32> {
    var p = at;
    for (var i = 0; i < 4; i++) {
        p -= water_displace(p, t).xz - at;
    }
    return p;
}

// The surface's height above the rest level over `at` at time `t`.
fn water_height(at: vec2<f32>, t: f32) -> f32 {
    return water_displace(water_rest_point(at, t), t).y;
}
