//! Headless GPU test of what the 3D renderer reports holding
//! (`Ctx::asset_inventory`'s `gpu`): meshes and GPU buffers made and freed
//! in a loop leave nothing behind, and a mesh's bytes match the estimate
//! the engine makes without a GPU. Skips without a GPU.
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{Material, MeshData};

fn renderer() -> Option<(wgpu::Device, wgpu::Queue, Renderer3D)> {
    let gpu = crate::render::render_test_support::gpu_with(|_| Ok(wgpu::DeviceDescriptor::default()))?;
    let (device, queue) = (gpu.device, gpu.queue);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let renderer = Renderer3D::new(&device, &layout, wgpu::TextureFormat::Rgba8Unorm, 0, &white, 4, true);
    Some((device, queue, renderer))
}

#[test]
fn freed_meshes_and_buffers_leave_no_gpu_memory() {
    let Some((device, queue, mut renderer)) = renderer() else {
        return;
    };
    let base = renderer.residency();
    let cube = MeshData::cube(1.0);
    let parts = [(0..cube.indices.len() as u32, Material { id: 0 })];
    let id = renderer.create_mesh(&device, &cube, &parts, false);
    renderer.flush_meshes_for_test(&device, &queue);
    let (meshes, bytes, _, _) = renderer.residency();
    assert_eq!(meshes, base.0 + 1);
    let estimate = crate::three::mesh_gpu_bytes(&cube);
    assert!(
        bytes - base.1 >= estimate && bytes - base.1 < 2 * estimate,
        "shared buffers reserve geometric growth room: {bytes}, payload {estimate}"
    );
    renderer.free_mesh(id);
    for _ in 0..50 {
        // Freed ids are made again in place, as the engine reuses them.
        renderer.create_mesh_at(&device, id, &cube, &parts, false);
        renderer.free_mesh(id);
    }
    assert_eq!(renderer.residency(), base);

    renderer.create_gpu_buffer(&device, 0, 64, &[]);
    let (_, _, buffers, buffer_bytes) = renderer.residency();
    if buffers == base.2 {
        crate::render::render_test_support::skip("the buffer part: no storage buffers on this GPU");
        return;
    }
    assert_eq!((buffers, buffer_bytes), (base.2 + 1, base.3 + 64 * 16));
    for _ in 0..50 {
        renderer.free_gpu_buffer(0);
        renderer.create_gpu_buffer(&device, 0, 64, &[]);
    }
    renderer.free_gpu_buffer(0);
    assert_eq!(renderer.residency(), base);
}

/// Everything the renderer holds, by class (`perf::GpuMemory`).
fn gpu_memory(renderer: &Renderer3D) -> crate::perf::GpuMemory {
    let mut t = crate::render::memory::MemoryTally::default();
    renderer.tally_memory(&mut t);
    t.finish()
}

#[test]
fn skinned_meshes_and_gpu_buffers_count_in_their_classes_until_freed() {
    let Some((device, queue, mut renderer)) = renderer() else {
        return;
    };
    let base = gpu_memory(&renderer);
    let mut cube = MeshData::cube(1.0);
    let n = cube.positions.len();
    cube.joints = vec![[0; 4]; n];
    cube.weights = vec![[1.0, 0.0, 0.0, 0.0]; n];
    let parts = [(0..cube.indices.len() as u32, Material { id: 0 })];
    let id = renderer.create_mesh(&device, &cube, &parts, true);
    renderer.flush_meshes_for_test(&device, &queue);
    let skinned = gpu_memory(&renderer);
    // Each corner's four joints (u16) and weights (f32), and its own
    // vertex and index buffers rather than the shared arena.
    assert!(
        skinned.skinning >= base.skinning + n as u64 * 24,
        "influences counted as skinning: {base}\n{skinned}"
    );
    assert!(skinned.meshes > base.meshes, "{base}\n{skinned}");
    assert_eq!(
        skinned.total - base.total,
        (skinned.skinning - base.skinning) + (skinned.meshes - base.meshes)
    );
    renderer.free_mesh(id);
    assert_eq!(gpu_memory(&renderer), base, "a freed skinned mesh leaves nothing");

    renderer.create_gpu_buffer(&device, 0, 64, &[]);
    let buffered = gpu_memory(&renderer);
    if buffered.gpu_buffers == 0 {
        crate::render::render_test_support::skip("the buffer part: no storage buffers on this GPU");
        return;
    }
    assert_eq!(buffered.gpu_buffers, 64 * 16, "{buffered}");
    renderer.free_gpu_buffer(0);
    assert_eq!(gpu_memory(&renderer), base);
}
