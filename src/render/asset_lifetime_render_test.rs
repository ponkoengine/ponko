//! Asset lifetimes, drawn offscreen: an instance batch whose mesh was freed
//! never draws the mesh made later in the freed one's slot, and renderers
//! made, filled, emptied and dropped again and again on one device leave
//! nothing behind. Skips without a GPU (a skip is not a pass).
use super::gpu3d::*;
use super::render_test_support::{self as support, Image};
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

/// The fraction of the frame where `channel` clearly dominates.
fn share(image: &Image, channel: usize) -> f32 {
    image.fraction(0..image.width, 0..image.height, |p| {
        let c = [p[0], p[1], p[2]];
        c[channel] > 100 && (0..3).all(|i| i == channel || c[i] < 60)
    })
}

#[test]
fn a_batch_whose_mesh_was_freed_never_draws_the_mesh_made_in_its_slot() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    let cube = MeshData::cube(1.0);
    let red = rig.mesh(&cube, glow(Color::RED));
    // Makes the green material on both sides (the renderer's ids follow
    // the game's); its own mesh is never drawn.
    let next = rig.ctx.gfx.assets3d.material_ids.next("material");
    let _spare = rig.mesh(&MeshData::cube(0.1), glow(Color::GREEN));
    let green_material = Material { id: next };
    let transforms: Vec<Mat4> = (-2..=2)
        .map(|x| Mat4::from_translation(Vec3::new(x as f32 * 1.5, 0.0, 0.0)))
        .collect();
    let batch = rig.batch(red, &transforms, InstanceBatchOptions::default());
    rig.ctx.scene3d.camera.position = Vec3::new(0.0, 2.0, 9.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let drawn = rig.image();
    let (r, g) = (share(&drawn, 0), share(&drawn, 1));
    eprintln!("live mesh: red {r:.3}, green {g:.3}");
    assert!(r > 0.05 && g == 0.0, "the batch draws its red cubes");

    // The game frees the mesh (the renderer frees its slot, as `Graphics`
    // does) and makes a green one, which takes the freed slot.
    rig.ctx.gfx.free(red);
    rig.renderer.free_mesh(red.slot());
    let green = rig.ctx.gfx.create_mesh(&cube, green_material);
    assert_eq!(green.slot(), red.slot(), "the freed slot is reused");
    rig.renderer.create_mesh_at(
        &rig.gpu.device,
        green.slot(),
        &cube,
        &[(0..cube.indices.len() as u32, green_material)],
        true,
    );
    // Negative control: without the frame's check the renderer, which
    // finds meshes by slot, draws the new mesh in the old one's place.
    let unchecked = rig.image();
    let leaked = share(&unchecked, 1);
    eprintln!("unchecked: green {leaked:.3}");
    assert!(leaked > 0.05, "the check below has something to catch");

    // What `Ctx::prepare_3d` runs before every frame.
    let (assets, renderer, gpu) = (&mut rig.ctx.gfx.assets3d, &mut rig.renderer, &rig.gpu);
    assets.validate_instance_batches(|id, b| renderer.set_instance_batch(&gpu.device, &gpu.queue, id, b));
    let checked = rig.image();
    let (r, g) = (share(&checked, 0), share(&checked, 1));
    eprintln!("checked: red {r:.3}, green {g:.3}");
    assert!(r == 0.0 && g == 0.0, "the batch draws nothing");
    assert!(rig.ctx.is_live(batch), "its handle stays live until freed");
}

/// What a renderer holds that the game can free: (meshes, mesh bytes, GPU
/// buffers, buffer bytes, instance batches, batch pools, batch pages).
fn holdings(renderer: &Renderer3D) -> (usize, u64, usize, u64, u32, u32, u32) {
    let (meshes, mesh_bytes, buffers, buffer_bytes) = renderer.residency();
    let b = renderer.instance_batch_stats();
    (meshes, mesh_bytes, buffers, buffer_bytes, b.batches, b.pools, b.pages)
}

/// Renderer teardown: a renderer is made on one device, filled with
/// meshes, materials, GPU buffers and instance batches, emptied, checked
/// against what it held when new, and dropped, many times over. Each new
/// renderer starts from the same holdings, each emptied one is back at
/// them, and where the backend reports its GPU allocations they do not
/// grow from one round to the next.
#[test]
fn renderers_made_filled_emptied_and_dropped_leave_nothing_behind() {
    let Some(gpu) = support::gpu() else {
        return;
    };
    let (layout, white, textures) = support::white_textures(&gpu.device, &gpu.queue);
    let cube = MeshData::cube(1.0);
    let parts = [(0..cube.indices.len() as u32, Material { id: 0 })];
    let mut fresh = None;
    let mut allocated = Vec::new();
    for round in 0..8 {
        let mut renderer = Renderer3D::new(
            &gpu.device,
            &layout,
            wgpu::TextureFormat::Rgba8Unorm,
            0,
            &white,
            4,
            true,
        );
        let new = holdings(&renderer);
        assert_eq!(*fresh.get_or_insert(new), new, "round {round}: a new renderer");
        let desc = crate::three::GpuMaterialDesc::new(&MaterialDesc::default());
        renderer.create_material(&gpu.device, &desc, &textures);
        let meshes: Vec<u32> = (0..16)
            .map(|_| renderer.create_mesh(&gpu.device, &cube, &parts, false))
            .collect();
        for slot in 0..4 {
            renderer.create_gpu_buffer(&gpu.device, slot, 256, &[]);
        }
        let mut game = Ctx::new(1.0 / 60.0, Vec2::new(64.0, 64.0));
        let material = game.gfx.default_material();
        let mesh = game.gfx.create_mesh(&cube, material);
        let batch = game
            .gfx
            .create_instance_batch(mesh, &[Mat4::IDENTITY; 64], InstanceBatchOptions::default());
        let made = game.gfx.assets3d.instance_batches[batch.id as usize].as_ref();
        renderer.set_instance_batch(&gpu.device, &gpu.queue, batch.id, made);
        assert_ne!(holdings(&renderer), new, "round {round}: filled");
        for id in meshes {
            renderer.free_mesh(id);
        }
        for slot in 0..4 {
            renderer.free_gpu_buffer(slot);
        }
        renderer.set_instance_batch(&gpu.device, &gpu.queue, batch.id, None);
        let emptied = holdings(&renderer);
        assert_eq!(
            (emptied.0, emptied.1, emptied.2, emptied.3, emptied.4),
            (new.0, new.1, new.2, new.3, new.4),
            "round {round}: emptied"
        );
        drop(renderer);
        // Nothing was drawn, so the batch's queue writes are still pending,
        // and pending writes keep their buffers alive. A game submits every
        // frame; an empty submission does the same here.
        gpu.queue.submit([]);
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        if let Some(report) = gpu.device.generate_allocator_report() {
            allocated.push(report.total_allocated_bytes);
        }
    }
    eprintln!("GPU bytes allocated after each round: {allocated:?}");
    if allocated.len() > 2 {
        // The first rounds may warm the allocator's own blocks up.
        let settled = allocated[1];
        assert!(
            allocated[2..].iter().all(|&b| b <= settled),
            "allocations grow round after round: {allocated:?}"
        );
    } else {
        eprintln!("this backend reports no allocations: the holdings above are the check");
    }
}
