use ponko::prelude::*;

#[test]
fn every_consuming_configuration_builder_is_forwarded() {
    // Scan signatures, including feature-gated methods, so a future Game builder
    // addition fails this test until ConfiguredGame forwards it as well.
    let source = include_str!("../src/game.rs");
    let game = source
        .split("impl Game {")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    let configured = source
        .split("impl ConfiguredGame {")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    for declaration in game.split("pub fn ").skip(1) {
        let signature = declaration.split('{').next().unwrap();
        let name = signature.split(['(', '<']).next().unwrap();
        if signature.contains("-> Self") && name != "new" {
            assert!(
                configured.contains(&format!("pub fn {name}(")) || configured.contains(&format!("pub fn {name}<")),
                "ConfiguredGame must forward Game::{name}"
            );
        }
    }
}

#[test]
fn configuration_after_setup_changes_the_simulation() {
    let world = Game::new("configured", 1, 1)
        .setup(|ctx| ctx.world.insert_resource(0_u32))
        .fixed_hz(20)
        .vsync(false)
        .fullscreen(false)
        .app_id("configured-test")
        .parallel_systems(false)
        .parallel_systems_env(false)
        .parallel_threshold(std::time::Duration::ZERO)
        .spatial_grid(16.0)
        .hot_reload(false)
        .inspector(false)
        .control_endpoint(false)
        .input_injection(false)
        .add(Phase::AfterPhysics, |ctx: &mut Ctx| {
            assert!((ctx.time.dt() - 0.05).abs() < 1e-6);
            *ctx.world.resource_mut::<u32>() += 1;
        })
        .try_run_headless(2)
        .unwrap();
    assert_eq!(*world.resource::<u32>(), 2);
}

#[cfg(feature = "physics")]
#[test]
fn physics_can_follow_setup() {
    Game::new("physics", 1, 1)
        .setup(|_| {})
        .physics(vec2(0.0, -9.8))
        .run_headless(1);
}
#[cfg(feature = "physics3d")]
#[test]
fn physics3d_can_follow_setup() {
    Game::new("physics", 1, 1)
        .setup(|_| {})
        .physics3d(vec3(0.0, -9.8, 0.0))
        .run_headless(1);
}
