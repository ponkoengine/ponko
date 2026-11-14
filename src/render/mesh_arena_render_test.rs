//! The shared mesh arena, drawn offscreen: meshes made before, during and
//! after the arena's buffers grow all draw (growth copies the old buffers on
//! the GPU, or uploads the index mirror where index buffers can't be copied),
//! and once a frame has drawn the arena keeps no CPU copy of the geometry.
//! Skips without a GPU (a skip is not a pass).
use super::render_test_support::Image;
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::prelude::*;
use crate::render::{InstanceBatchOptions, Material, MaterialDesc, MeshData};

fn glow(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::BLACK,
        emissive: color,
        ..Default::default()
    }
}

/// The fraction of the frame's third `third` (0, 1, 2 from the left) where
/// `channel` clearly dominates.
fn share(image: &Image, third: u32, channel: usize) -> f32 {
    let w = image.width / 3;
    image.fraction(third * w..(third + 1) * w, 0..image.height, |p| {
        let c = [p[0], p[1], p[2]];
        c[channel] > 100 && (0..3).all(|i| i == channel || c[i] < 60)
    })
}

/// A red cube made before the arena grows, a green one made in the frame
/// it grows and a blue one after, drawn left to right. Returns the frame
/// and the arena's GPU bytes before and after the growth.
fn draw_across_growth(rig: &mut BatchRig) -> (Image, u64, u64) {
    let cube = MeshData::cube(1.0);
    let at = |x: f32| [Mat4::from_translation(Vec3::new(x, 0.0, 0.0))];
    let red = rig.mesh(&cube, glow(Color::RED));
    rig.batch(red, &at(-2.6), InstanceBatchOptions::default());
    rig.ctx.scene3d.camera.position = Vec3::new(0.0, 1.5, 8.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.image();
    let before = rig.renderer.residency().1;
    let green = rig.mesh(&cube, glow(Color::GREEN));
    // Never drawn: enough geometry to grow the arena a few times over.
    let filler = MeshData::sphere(0.5, 48);
    for _ in 0..8 {
        // Made on both sides: the renderer's mesh ids follow the game's.
        rig.mesh(&filler, glow(Color::WHITE));
    }
    rig.batch(green, &at(0.0), InstanceBatchOptions::default());
    rig.image();
    let after = rig.renderer.residency().1;
    let blue = rig.mesh(&cube, glow(Color::BLUE));
    rig.batch(blue, &at(2.6), InstanceBatchOptions::default());
    (rig.image(), before, after)
}

fn shares(image: &Image) -> [f32; 3] {
    [share(image, 0, 0), share(image, 1, 1), share(image, 2, 2)]
}

fn meshes_made_before_and_after_growth_draw(setup: RigSetup, index_mirror: bool) {
    let Some(mut rig) = BatchRig::new(setup) else {
        return;
    };
    if index_mirror {
        rig.renderer.force_index_mirror_for_test();
    }
    let (image, before, after) = draw_across_growth(&mut rig);
    assert!(after >= 4 * before, "the arena grew: {before} -> {after} bytes");
    let [r, g, b] = shares(&image);
    eprintln!("index mirror {index_mirror}: red {r:.3}, green {g:.3}, blue {b:.3}");
    assert!(r > 0.02 && g > 0.02 && b > 0.02, "all three cubes draw");
    assert_eq!(
        rig.renderer.mesh_arena_cpu_bytes() > 0,
        index_mirror,
        "only the index mirror stays on the CPU once drawn"
    );

    // Negative control: a growth that forgets the old buffers loses the
    // cubes made before it, and the shares above catch it.
    let Some(mut rig) = BatchRig::new(setup) else {
        return;
    };
    rig.renderer.lose_mesh_arena_on_grow_for_test();
    let (image, ..) = draw_across_growth(&mut rig);
    let [r, _, b] = shares(&image);
    eprintln!("growth without the copy: red {r:.3}, blue {b:.3}");
    assert!(r < 0.005, "the red cube was made before the growth and is gone");
    assert!(b > 0.02, "the blue one, made after it, still draws");
}

#[test]
fn meshes_made_before_and_after_the_arena_grows_still_draw() {
    meshes_made_before_and_after_growth_draw(RigSetup::default(), false);
}

#[test]
fn the_webgl2_tier_grows_the_arena_from_its_index_mirror() {
    let webgl2 = RigSetup {
        compute: false,
        ..Default::default()
    };
    meshes_made_before_and_after_growth_draw(webgl2, true);
}

/// Streams a large synthetic world through the renderer in cells, freeing
/// the oldest as new ones come in, and prints the arena's GPU and CPU bytes.
/// `cargo test --release --lib -- --ignored streaming_world_arena_memory --nocapture`
#[test]
#[ignore = "a measurement: prints numbers"]
fn streaming_world_arena_memory() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    // 64 cells of 16 meshes, each a sphere of 8385 vertices and 16 384
    // triangles (about 590 KiB); the last 48 cells stay loaded.
    let grid = MeshData::sphere(1.0, 128);
    let parts = [(0..grid.indices.len() as u32, Material { id: 0 })];
    let (device, queue) = (rig.gpu.device.clone(), rig.gpu.queue.clone());
    let mut live = std::collections::VecDeque::new();
    let mut peak_staged = 0;
    for _cell in 0..64 {
        for _ in 0..16 {
            live.push_back(rig.renderer.create_mesh(&device, &grid, &parts, false));
        }
        peak_staged = peak_staged.max(rig.renderer.mesh_arena_cpu_bytes());
        rig.renderer.flush_meshes_for_test(&device, &queue);
        while live.len() > 48 * 16 {
            rig.renderer.free_mesh(live.pop_front().unwrap());
        }
    }
    let (meshes, gpu_bytes, ..) = rig.renderer.residency();
    let geometry = crate::three::mesh_gpu_bytes(&grid);
    println!(
        "streamed {} meshes of {} KiB, {meshes} live: arena GPU {:.1} MiB, CPU {} bytes after the uploads (peak staged before a flush {:.1} MiB)",
        64 * 16,
        geometry / 1024,
        gpu_bytes as f64 / 1048576.0,
        rig.renderer.mesh_arena_cpu_bytes(),
        peak_staged as f64 / 1048576.0,
    );
}
