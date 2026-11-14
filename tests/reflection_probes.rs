#![cfg(feature = "3d")]
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::{ReflectionProbe as ReflectionProbe3D, Transform as Transform3D};
use ponko::prelude::*;

#[test]
fn probe_weights_fade_inside_box_and_projection_hits_wall() {
    let probe = ReflectionProbe3D::default();
    assert_eq!(probe.weight(Vec3::ZERO, Vec3::ZERO), 1.0);
    assert_eq!(probe.weight(Vec3::ZERO, vec3(6.0, 0.0, 0.0)), 0.0);
    assert_eq!(
        probe.project(Vec3::ZERO, vec3(1.0, 0.0, 0.0), Vec3::Z),
        vec3(1.0, 0.0, 5.0)
    );
    let invalid = ReflectionProbe3D {
        size: Vec3::splat(f32::NAN),
        ..probe
    };
    assert_eq!(invalid.weight(Vec3::ZERO, Vec3::ZERO), 0.0);
}

#[test]
fn bake_requires_a_live_probe_and_allocates_content_once() {
    Game::new("probe", 1, 1).run_headless(0, |ctx| {
        let entity = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
        ctx.gfx.bake_reflection_probe(&mut ctx.world, entity).unwrap();
        let content = ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content;
        assert_ne!(content, 0);
        ctx.gfx.bake_reflection_probe(&mut ctx.world, entity).unwrap();
        assert_eq!(ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content, content);
        #[derive(Component)]
        struct Empty;
        let empty = ctx.world.spawn((Empty,));
        assert!(ctx.gfx.bake_reflection_probe(&mut ctx.world, empty).is_err());
    });
}

fn write_image(name: &str, width: u32, height: u32, value: f32) -> String {
    let path = std::env::temp_dir().join(format!("ponko-probe-{}-{name}.exr", std::process::id()));
    image::Rgb32FImage::from_pixel(width, height, image::Rgb([value; 3]))
        .save(&path)
        .unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn loading_needs_a_two_to_one_finite_image() {
    let good = write_image("good", 64, 32, 1.0);
    let square = write_image("square", 32, 32, 1.0);
    let broken = write_image("nan", 64, 32, f32::NAN);
    Game::new("probe", 1, 1).run_headless(0, |ctx| {
        let entity = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
        let bad = ctx
            .gfx
            .try_load_reflection_probe(&mut ctx.world, entity, &square)
            .unwrap_err();
        assert_eq!(bad.kind(), ErrorKind::Invalid);
        assert!(bad.message().contains("2:1"), "{bad}");
        let broken = ctx
            .gfx
            .try_load_reflection_probe(&mut ctx.world, entity, &broken)
            .unwrap_err();
        assert_eq!(broken.kind(), ErrorKind::Decode);
        assert!(broken.message().contains("finite"), "{broken}");
        let missing = ctx
            .gfx
            .try_load_reflection_probe(&mut ctx.world, entity, "missing-probe.exr")
            .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
        assert_eq!(missing.path(), Some("missing-probe.exr"));
        assert_eq!(ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content, 0);
        ctx.gfx.load_reflection_probe(&mut ctx.world, entity, &good);
        assert_ne!(ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content, 0);
    });
    for path in [good, square, broken] {
        std::fs::remove_file(path).ok();
    }
}

#[test]
fn a_fifth_environment_is_refused() {
    Game::new("probe", 1, 1).run_headless(0, |ctx| {
        for _ in 0..4 {
            let e = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
            ctx.gfx.bake_reflection_probe(&mut ctx.world, e).unwrap();
        }
        let fifth = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
        assert!(
            ctx.gfx
                .bake_reflection_probe(&mut ctx.world, fifth)
                .unwrap_err()
                .message()
                .contains("four")
        );
    });
}

#[test]
fn a_refresh_request_needs_a_live_probe_and_gives_it_a_slot() {
    use ponko::d3::{ProbeRefresh, ReflectionRefresh};
    Game::new("probe", 1, 1).run_headless(0, |ctx| {
        let entity = ctx.world.spawn((
            Transform3D::default(),
            ReflectionProbe3D::default(),
            ReflectionRefresh {
                when: ProbeRefresh::Every(5.0),
                blend_seconds: 0.5,
            },
        ));
        ctx.gfx.refresh_reflection_probe(&mut ctx.world, entity).unwrap();
        let content = ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content;
        assert_ne!(content, 0, "the request allocates the probe's environment");
        ctx.gfx.refresh_reflection_probe(&mut ctx.world, entity).unwrap();
        assert_eq!(ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content, content);
        // Baking it at once still works on a refreshing probe, in the same slot.
        ctx.gfx.bake_reflection_probe(&mut ctx.world, entity).unwrap();
        assert_eq!(ctx.world.get::<ReflectionProbe3D>(entity).unwrap().content, content);
        let bare = ctx.world.spawn((Transform3D::default(),));
        let err = ctx.gfx.refresh_reflection_probe(&mut ctx.world, bare).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        let empty = ctx.world.spawn((
            Transform3D::default(),
            ReflectionProbe3D {
                size: Vec3::ZERO,
                ..Default::default()
            },
        ));
        let err = ctx.gfx.refresh_reflection_probe(&mut ctx.world, empty).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
    });
}

#[test]
fn refresh_settings_default_to_on_demand_with_a_one_second_blend_and_load_from_partial_json() {
    use ponko::d3::{ProbeRefresh, ReflectionRefresh};
    let refresh = ReflectionRefresh::default();
    assert_eq!(refresh.when, ProbeRefresh::OnDemand);
    assert_eq!(refresh.blend_seconds, 1.0);
    let loaded: ReflectionRefresh = serde_json::from_str(r#"{"when":{"LightingChanged":0.05}}"#).unwrap();
    assert_eq!(loaded.when, ProbeRefresh::LightingChanged(0.05));
    assert_eq!(loaded.blend_seconds, 1.0);
}
