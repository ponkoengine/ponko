//! The water kit's shaders on every backend the engine checks: valid WGSL
//! with every capability and at one and four samples (what native devices
//! validate them with), and translated to GLSL ES 3.0 as WebGL2 runs them.
//! WebGL2 has no compute shaders, so the contact ripples and a spectrum's
//! field are worked out on the CPU there and uploaded; the surface shaders
//! that read them are the same. No GPU needed.
use super::webgl2_shaders_test::{WEBGL2, translate, webgl2_material};
use super::*;
use crate::water::{FlowMap, WaterDesc, WaterQuality, WaterReflection, WaveSpectrum, Waves, shaders};
use glam::{Vec2, vec2};

/// Every kind of water body at every preset and reflection.
fn bodies() -> Vec<(String, WaterDesc)> {
    let flow = FlowMap::from_fn(4, 4, 2.0, |uv| vec2(uv.x - 0.5, 1.0));
    let kinds = [
        ("lake", WaterDesc::lake(Vec2::ZERO, vec2(30.0, 30.0), 0.0)),
        (
            "river",
            WaterDesc::river(Vec2::ZERO, vec2(6.0, 60.0), 0.0, vec2(0.0, 1.0)).with_flow_map(flow),
        ),
        ("sea", WaterDesc::sea(0.0, vec2(1.0, 0.3), 1.5)),
        (
            "spectrum sea",
            WaterDesc::sea(0.0, vec2(1.0, 0.3), 1.5)
                .with_waves(Waves::spectrum(WaveSpectrum::new(vec2(9.0, 3.0), 1.5))),
        ),
        (
            "flat pool",
            WaterDesc::lake(Vec2::ZERO, vec2(4.0, 4.0), 0.0).with_waves(Waves::none()),
        ),
    ];
    let mut out = Vec::new();
    for (kind, desc) in kinds {
        for quality in [WaterQuality::Low, WaterQuality::Medium, WaterQuality::High] {
            for reflection in [
                WaterReflection::Sky,
                WaterReflection::ScreenSpace,
                WaterReflection::Planar,
            ] {
                let name = format!("{kind}, {quality:?}, {reflection:?}");
                out.push((name, desc.clone().with_quality(quality).with_reflection(reflection)));
            }
        }
    }
    out
}

#[test]
fn water_shaders_are_valid_on_native_backends() {
    for (name, desc) in bodies() {
        let planar = desc.reflection == WaterReflection::Planar;
        for ripples in [false, true] {
            let user = shaders::surface(&desc, planar, ripples);
            for samples in [1, 4] {
                check_shader(&name, &for_samples(&compose_shader(&user), samples, true)).unwrap();
                check_shader(
                    &name,
                    &for_samples(&compose_shader_storage(&user, true, true, true), samples, false),
                )
                .unwrap();
            }
        }
        let effect = compose_effect(&shaders::underwater(&desc));
        for samples in [1, 4] {
            check_shader(&name, &for_samples(&effect, samples, false)).unwrap();
        }
    }
    check_shader("water ripples", &compose_compute(shaders::RIPPLES)).unwrap();
    check_shader("water spectrum", &compose_compute(shaders::SPECTRUM)).unwrap();
    check_shader("water FFT stage", &compose_compute(shaders::BUTTERFLY)).unwrap();
}

#[test]
fn water_shaders_translate_to_glsl_es_300() {
    let mut count = 0;
    for (name, desc) in bodies() {
        let planar = desc.reflection == WaterReflection::Planar;
        // With and without ripples (the CPU steps them on WebGL2).
        for ripples in [false, true] {
            let surface = webgl2_material(&shaders::surface(&desc, planar, ripples));
            if let Err(e) = translate(&name, &surface, WEBGL2) {
                panic!("{name}'s surface (ripples {ripples}) can't run on WebGL2:\n{e}");
            }
            count += 1;
        }
        let effect = for_samples(&compose_effect(&shaders::underwater(&desc)), 1, true).into_owned();
        if let Err(e) = translate(&name, &effect, WEBGL2) {
            panic!("{name}'s underwater view can't run on WebGL2:\n{e}");
        }
        count += 1;
    }
    assert_eq!(count, 5 * 9 * 3);
}
