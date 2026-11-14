//! What sorting see-through things far to near costs: a CPU-path instance
//! batch of a see-through material against the same batch opaque (same
//! culling and draws, no sort), while the camera moves every frame so the
//! order is made again each frame; and collecting particles of many alpha
//! emitters sorted together. No window is opened. Run explicitly, in
//! release:
//! `cargo test --release --lib --features 3d transparency_sort_benchmark -- --ignored --nocapture`
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::d3::{
    Camera as Camera3D, ParticleEmitter as ParticleEmitter3D, ParticleSettings as ParticleSettings3D,
    Transform as Transform3D,
};
use crate::prelude::*;
use crate::render::{BatchCulling, Blend, InstanceBatchOptions, MaterialDesc, MeshData};
use crate::three::particles::ParticleFrame;

const WARM_UP: usize = 10;
const FRAMES: usize = 60;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The median `prepare` time (ms) of a batch of `count` cubes in view,
/// with a see-through material or an opaque one, the camera moving.
fn batch_prepare_ms(count: usize, see_through: bool) -> Option<f64> {
    let mut rig = BatchRig::new(RigSetup {
        size: (1280, 720),
        ..Default::default()
    })?;
    rig.ctx.scene3d.camera.far = 2000.0;
    let material = MaterialDesc {
        color: Color::rgba(0.8, 0.8, 0.8, if see_through { 0.5 } else { 1.0 }),
        blend: if see_through { Blend::Transparent } else { Blend::Opaque },
        ..Default::default()
    };
    let cube = rig.mesh(&MeshData::cube(0.5), material);
    // A square field in front of the camera, all of it in view.
    let side = (count as f32).sqrt().ceil() as usize;
    let transforms: Vec<Mat4> = (0..count)
        .map(|i| {
            let (x, z) = ((i % side) as f32, (i / side) as f32);
            Mat4::from_translation(vec3(x - side as f32 / 2.0, 0.0, -z - 5.0))
        })
        .collect();
    let options = InstanceBatchOptions {
        cast_shadows: false,
        culling: BatchCulling::PreferCpu,
        ..Default::default()
    };
    rig.batch(cube, &transforms, options);
    let mut times = Vec::with_capacity(FRAMES);
    for frame in 0..WARM_UP + FRAMES {
        let t = frame as f32 * 0.01;
        rig.ctx.scene3d.camera.position = vec3(t.sin() * 5.0, side as f32 * 0.6, 10.0 + t.cos());
        rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -(side as f32) / 2.0));
        let cost = rig.frame();
        if frame >= WARM_UP {
            times.push(cost.prepare_ms);
        }
    }
    Some(median(&mut times))
}

/// The median time (ms) to collect `emitters` alpha emitters of
/// `per_emitter` particles each, sorted together, from a moving camera.
fn particle_collect_ms(emitters: usize, per_emitter: usize) -> f64 {
    let mut world = World::new();
    for e in 0..emitters {
        let points: Vec<Vec3> = (0..per_emitter)
            .map(|i| {
                let k = (e * per_emitter + i) as f32;
                vec3(
                    (k * 0.37).sin() * 20.0,
                    (k * 0.11).cos() * 5.0,
                    -(k * 0.73).fract() * 40.0 - 2.0,
                )
            })
            .collect();
        let settings = ParticleSettings3D {
            max_particles: per_emitter,
            ..Default::default()
        };
        world.spawn((
            Transform3D::default(),
            ParticleEmitter3D::at_points_for_test(settings, &points),
        ));
    }
    let mut camera = Camera3D::new(Vec2::new(1280.0, 720.0));
    let mut frame = ParticleFrame::default();
    let mut times = Vec::with_capacity(FRAMES);
    for f in 0..WARM_UP + FRAMES {
        camera.position = vec3((f as f32 * 0.1).sin(), 1.0, 5.0);
        camera.look_at(vec3(0.0, 0.0, -20.0));
        let start = std::time::Instant::now();
        frame.collect(&mut world, &camera);
        if f >= WARM_UP {
            times.push(start.elapsed().as_secs_f64() * 1e3);
        }
        assert_eq!(frame.instances.len(), emitters * per_emitter);
    }
    median(&mut times)
}

#[test]
#[ignore = "a benchmark: run it in release with --ignored --nocapture"]
fn transparency_sort_benchmark() {
    println!("see-through batch, CPU path, camera moving: median Renderer3D::prepare (ms)");
    println!(
        "{:>10} {:>10} {:>12} {:>10}",
        "instances", "opaque", "see-through", "sorting"
    );
    for count in [1_000, 10_000, 100_000, 400_000] {
        // Without a GPU the rig has logged the skip.
        let (Some(opaque), Some(sorted)) = (batch_prepare_ms(count, false), batch_prepare_ms(count, true)) else {
            return;
        };
        println!("{count:>10} {opaque:>10.3} {sorted:>12.3} {:>10.3}", sorted - opaque);
    }
    println!();
    println!("alpha particles sorted across emitters: median ParticleFrame::collect (ms)");
    println!("{:>9} {:>12} {:>10}", "emitters", "per emitter", "collect");
    for (emitters, per_emitter) in [(1, 10_000), (10, 1_000), (100, 1_000), (10, 10_000), (100, 10_000)] {
        let ms = particle_collect_ms(emitters, per_emitter);
        println!("{emitters:>9} {per_emitter:>12} {ms:>10.3}");
    }
}
