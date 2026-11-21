//! The water kit's WGSL: the surface material, the underwater screen
//! effect and the ripple simulation. Each water body's shaders are made
//! from its own [`Waves`](super::Waves) table (written out as literals, so
//! every backend sees plain arithmetic) and its quality's switches.

use std::fmt::Write;

use super::waves::{Waves, lit};
use super::{WaterDesc, WaterReflection};

/// The contact ripple simulation, a compute pass over the ripple target:
/// red is the height now, green the height one step ago (the wave
/// equation on a grid, damped). `material.custom[0]`: the region's −x,
/// −z corner, its side and the damping; `[1]` to `[3]`: contacts (x, z,
/// radius, strength), radius 0 for none.
pub(crate) const RIPPLES: &str = include_str!("ripples.wgsl");

/// Waves from a spectrum: the spectrum at the frame's time (a compute pass
/// over the field; `spectrum.rs` has the layout).
pub(crate) const SPECTRUM: &str = include_str!("spectrum.wgsl");

/// Waves from a spectrum: one stage of the inverse FFT, along x or z.
pub(crate) const BUTTERFLY: &str = include_str!("butterfly.wgsl");

/// The wave functions over a spectrum's field (the material's base texture).
const SPECTRUM_WAVES: &str = include_str!("spectrum_waves.wgsl");

/// The surface's `vertex` hook: each vertex rides the waves, and keeps its
/// rest point in `uv1` for the surface's normal.
pub(crate) const VERTEX: &str = include_str!("vertex.wgsl");
const SURFACE: &str = include_str!("surface.wgsl");
const UNDERWATER: &str = include_str!("underwater.wgsl");

/// The wave functions both the surface and the underwater effect use:
/// `water_displace`, `water_wave`, `water_rest_point` and `water_height`,
/// each the CPU's [`Waves`] function of the same name, unrolled over the
/// table.
pub(crate) fn wave_library(waves: &Waves) -> String {
    if let Some(s) = waves.spectrum_settings() {
        return format!(
            "// The waves (from a spectrum's field).\nconst WATER_TILE: f32 = {};\nconst WATER_CELLS: i32 = {};\n{SPECTRUM_WAVES}",
            lit(s.tile.max(1.0)),
            s.cells()
        );
    }
    // Every coefficient is worked out here and written as one literal
    // (in brackets when negative), so the shader does plain arithmetic.
    let l = |v: f32| if v < 0.0 { format!("({})", lit(v)) } else { lit(v) };
    let mut displace = String::new();
    let mut sample = String::new();
    let mut seen = String::new();
    for w in waves.terms() {
        let theta = format!(
            "{} * p.x + {} * p.y - {} * t + {}",
            l(w.k * w.dir.x),
            l(w.k * w.dir.y),
            l(w.omega),
            l(w.phase)
        );
        let (sx, sz, a) = (l(w.dir.x * w.sway), l(w.dir.y * w.sway), l(w.amplitude));
        let _ = write!(
            displace,
            "    th = {theta};\n    out += vec3<f32>({sx} * cos(th), {a} * sin(th), {sz} * cos(th));\n"
        );
        let (nx, ny, nz) = (
            l(-w.dir.x * w.k * w.amplitude),
            l(-w.k * w.sway),
            l(-w.dir.y * w.k * w.amplitude),
        );
        let (jx, jy, jz) = (
            l(w.dir.x * w.dir.x * w.sway * w.k),
            l(w.dir.x * w.dir.y * w.sway * w.k),
            l(w.dir.y * w.dir.y * w.sway * w.k),
        );
        let _ = write!(
            sample,
            "    th = {theta};\n    s = sin(th);\n    c = cos(th);\n    h += {a} * s;\n    \
             n += vec3<f32>({nx} * c, {ny} * s, {nz} * c);\n    \
             j -= vec3<f32>({jx} * s, {jy} * s, {jz} * s);\n"
        );
        // The same, each wave's slope kept by how many pixels it spans.
        let length = l(std::f32::consts::TAU / w.k);
        let _ = write!(
            seen,
            "    th = {theta};\n    s = sin(th);\n    c = cos(th);\n    h += {a} * s;\n    \
             keep = smoothstep(3.0, 8.0, {length} / fp);\n    \
             n += vec3<f32>({nx} * c, {ny} * s, {nz} * c) * keep;\n    \
             j -= vec3<f32>({jx} * s, {jy} * s, {jz} * s) * keep;\n"
        );
    }
    format!(
        "// The waves (generated from the game's wave table).
struct WaterWave {{
    height: f32,      // above the rest level
    normal: vec3<f32>,
    squeeze: f32,     // 1 on flat water, towards 0 on sharp crests
}};

// Where the water at rest point `p` is at time `t` (x, height, z).
fn water_displace(p: vec2<f32>, t: f32) -> vec3<f32> {{
    var out = vec3<f32>(p.x, 0.0, p.y);
    var th = 0.0;
{displace}    return out;
}}

// The surface over rest point `p` at time `t`.
fn water_wave(p: vec2<f32>, t: f32) -> WaterWave {{
    var h = 0.0;
    var n = vec3<f32>(0.0, 1.0, 0.0);
    var j = vec3<f32>(1.0, 0.0, 1.0);
    var th = 0.0;
    var s = 0.0;
    var c = 0.0;
{sample}    return WaterWave(h, normalize(n), max(j.x * j.z - j.y * j.y, 0.0));
}}

// The same as seen where a pixel covers `footprint` metres: waves too
// short for the pixels flatten out instead of flickering.
fn water_wave_seen(p: vec2<f32>, t: f32, footprint: f32) -> WaterWave {{
    var h = 0.0;
    var n = vec3<f32>(0.0, 1.0, 0.0);
    var j = vec3<f32>(1.0, 0.0, 1.0);
    var th = 0.0;
    var s = 0.0;
    var c = 0.0;
    var keep = 0.0;
    let fp = max(footprint, 1e-4);
{seen}    return WaterWave(h, normalize(n), max(j.x * j.z - j.y * j.y, 0.0));
}}

// The rest point whose water is over `at` at time `t` (four fixed-point steps).
fn water_rest_point(at: vec2<f32>, t: f32) -> vec2<f32> {{
    var p = at;
    for (var i = 0; i < 4; i++) {{
        p -= water_displace(p, t).xz - at;
    }}
    return p;
}}

// The surface's height above the rest level over `at` at time `t`.
fn water_height(at: vec2<f32>, t: f32) -> f32 {{
    return water_displace(water_rest_point(at, t), t).y;
}}
"
    )
}

/// The surface material's shader for `desc`.
pub(crate) fn surface(desc: &WaterDesc, planar: bool, ripples: bool) -> String {
    let spec = desc.quality.spec();
    let ssr = if desc.reflection == WaterReflection::ScreenSpace && !planar {
        spec.ssr_steps
    } else {
        0
    };
    let corner = desc.center - desc.size * 0.5;
    let size = desc.size.max(glam::Vec2::splat(1e-3));
    let mut out = String::new();
    let _ = writeln!(out, "// A water surface made by the water kit.");
    let _ = writeln!(out, "const WATER_OCTAVES: i32 = {};", spec.octaves);
    let _ = writeln!(out, "const WATER_REFRACT: bool = {};", spec.refraction);
    let _ = writeln!(out, "const WATER_SSR_STEPS: i32 = {ssr};");
    let _ = writeln!(out, "const WATER_PLANAR: bool = {planar};");
    let _ = writeln!(out, "const WATER_RIPPLES: bool = {ripples};");
    let _ = writeln!(out, "const WATER_FLOW_MAP: bool = {};", desc.flow_map.is_some());
    let _ = writeln!(out, "const WATER_CREST_FOAM: bool = {};", !desc.waves.is_empty());
    let _ = writeln!(
        out,
        "const WATER_FLOW_AREA = vec4<f32>({}, {}, {}, {});",
        lit(corner.x),
        lit(corner.y),
        lit(size.x),
        lit(size.y)
    );
    out.push_str(&wave_library(&desc.waves));
    out.push_str(VERTEX);
    out.push_str(SURFACE);
    out
}

/// The underwater screen effect's shader for `desc`.
pub(crate) fn underwater(desc: &WaterDesc) -> String {
    let mut out = String::from("// The underwater view made by the water kit.\n");
    out.push_str(&wave_library(&desc.waves));
    out.push_str(UNDERWATER);
    out
}
