//! A spot light shining past a box onto the ground, drawn offscreen: with
//! shadows on, the ground under the box goes dark; off, it stays lit (the
//! light views' collection, passes and lookup together). Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::{PointLight as PointLight3D, SpotLight as SpotLight3D, Transform as Transform3D};
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{InstanceBatchOptions, InstanceLevel, MaterialDesc, MeshData, Model};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 64;

#[test]
fn a_shadowed_spot_light_darkens_the_ground_behind_a_box() {
    local_shadow(false, false, true, true);
}

#[test]
fn batches_cast_spot_light_shadows() {
    local_shadow(true, false, true, true);
    local_shadow(true, false, false, true);
}

#[test]
fn batches_cast_point_light_shadows() {
    local_shadow(true, true, true, true);
    local_shadow(true, true, false, true);
}

#[test]
fn batches_can_disable_local_light_shadows() {
    local_shadow(true, false, true, false);
    local_shadow(true, true, true, false);
}

fn local_shadow(batch: bool, point: bool, compute: bool, casts: bool) {
    local_shadow_levels(batch, point, compute, casts, None);
}
#[test]
fn instance_levels_separate_camera_and_local_shadow_geometry() {
    for compute in [true, false] {
        for point in [true, false] {
            local_shadow_levels(true, point, compute, true, Some(false));
            local_shadow_levels(true, point, compute, false, Some(true));
        }
    }
}
fn local_shadow_levels(batch: bool, point: bool, compute: bool, casts: bool, camera: Option<bool>) {
    let Some(gpu) = crate::render::render_test_support::gpu_with(|_| {
        Ok(wgpu::DeviceDescriptor {
            required_limits: if compute {
                wgpu::Limits::default()
            } else {
                wgpu::Limits::downlevel_webgl2_defaults()
            },
            ..Default::default()
        })
    }) else {
        return;
    };
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    queue.write_texture(
        white.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        white.size(),
    );
    let sampler = device.create_sampler(&Default::default());
    let view = white.create_view(&Default::default());
    let textures = [GpuTexture {
        texture: white.clone(),
        nearest: false,
        mips: None,
        bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
    }];
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = output.create_view(&Default::default());

    // A white ground, a box 3 up, and a spot light 6 up pointing down.
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let matte = MaterialDesc {
        color: Color::WHITE,
        roughness: 1.0,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(matte);
    let ground = MeshData::plane(20.0);
    let cube = MeshData::cube(1.0);
    let ground_mesh = ctx.gfx.create_mesh(&ground, material);
    let cube_mesh = ctx.gfx.create_mesh(&cube, material);
    ctx.world.spawn((Transform3D::default(), Model::new(ground_mesh)));
    if !batch {
        ctx.world
            .spawn((Transform3D::at(vec3(0.0, 3.0, 0.0)), Model::new(cube_mesh)));
    }
    let light = ctx.world.spawn((
        Transform3D::at(vec3(0.0, 6.0, 0.0)).looking_at(vec3(0.0, 0.0, 0.001)),
        SpotLight3D {
            intensity: 400.0,
            range: 20.0,
            inner_angle: 0.5,
            outer_angle: 0.7,
            shadows: true,
            ..Default::default()
        },
    ));
    let point_light = point.then(|| {
        ctx.world.spawn((
            Transform3D::at(vec3(0.0, 6.0, 0.0)),
            PointLight3D {
                intensity: 400.0,
                range: 20.0,
                shadows: true,
                ..Default::default()
            },
        ))
    });
    if point {
        ctx.world.get_mut::<SpotLight3D>(light).unwrap().intensity = 0.0;
    }
    ctx.scene3d.camera.position = vec3(0.0, 6.0, 6.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.light_shadow_atlas = 2048;
    ctx.scene3d.sky.atmosphere = false;

    let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
    // The GPU-driven path wherever the device has it, whatever the batch's size.
    renderer.batching.path = BatchPath::Gpu;
    renderer.create_material(&device, &GpuMaterialDesc::new(&matte), &textures);
    renderer.create_mesh(&device, &ground, &[(0..ground.indices.len() as u32, material)], true);
    renderer.create_mesh(&device, &cube, &[(0..cube.indices.len() as u32, material)], true);

    if batch {
        let handle = if let Some(camera) = camera {
            ctx.gfx.create_instance_batch_levels(
                &[InstanceLevel {
                    mesh: cube_mesh,
                    min_distance: 0.0,
                    max_distance: 20.0,
                    camera,
                    shadows: casts,
                }],
                &[Mat4::from_translation(vec3(0.0, 3.0, 0.0))],
                &[],
                Default::default(),
            )
        } else {
            ctx.gfx.create_instance_batch(
                cube_mesh,
                &[Mat4::from_translation(vec3(0.0, 3.0, 0.0))],
                InstanceBatchOptions {
                    max_distance: 4.0,
                    cast_shadows: casts,
                    ..Default::default()
                },
            )
        };
        renderer.set_instance_batch(
            &device,
            &queue,
            handle.id,
            ctx.gfx.assets3d.instance_batches[handle.id as usize].as_ref(),
        );
    }
    // How many passes drew into the light shadow atlas, last draw.
    let light_passes = std::cell::Cell::new(0);
    let mut draw = |ctx: &mut Ctx| -> Vec<u8> {
        let mut frame = Frame3D::default();
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::ZERO,
                sky: Vec3::ZERO,
                ground: Vec3::ZERO,
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: false,
        };
        renderer.prepare(
            &device,
            &queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (SIZE, SIZE),
            },
        );
        light_passes.set(renderer.light_shadow_passes());
        let buffers = renderer.draw(&device, &target, wgpu::Color::BLACK, &textures, None);
        queue.submit(buffers);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: None,
                },
            },
            output.size(),
        );
        queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        readback.get_mapped_range(..).unwrap().to_vec()
    };
    // Where a ground point lands on screen, and how bright it is there.
    let view_proj = ctx.scene3d.camera.view_proj();
    let brightness = |pixels: &[u8], p: Vec3| -> u32 {
        let ndc = view_proj.project_point3(p);
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
        let i = ((y * SIZE + x) * 4) as usize;
        u32::from(pixels[i]) + u32::from(pixels[i + 1]) + u32::from(pixels[i + 2])
    };
    // The box's shadow covers the ground within 1 of the middle; 2.5 out
    // is still in the cone, in the open.
    let (under, open) = (vec3(0.0, 0.0, 0.6), vec3(0.0, 0.0, 2.5));
    let shadowed = draw(&mut ctx);
    // The spot light's view and the point light's six share one pass.
    assert_eq!(light_passes.get(), 1, "the light views are drawn in one pass");
    ctx.world.get_mut::<SpotLight3D>(light).unwrap().shadows = false;
    if let Some(light) = point_light {
        ctx.world.get_mut::<PointLight3D>(light).unwrap().shadows = false;
    }
    let unshadowed = draw(&mut ctx);
    assert!(brightness(&unshadowed, open) > 60, "the light reaches the open ground");
    assert!(
        brightness(&unshadowed, under) > 60,
        "without shadows the light passes the box"
    );
    if !casts {
        assert_eq!(brightness(&shadowed, under), brightness(&unshadowed, under));
        return;
    }
    assert!(
        brightness(&shadowed, under) * 4 < brightness(&unshadowed, under),
        "the box shadows the ground under it: {} vs {}",
        brightness(&shadowed, under),
        brightness(&unshadowed, under)
    );
    assert!(
        brightness(&shadowed, open) * 10 > brightness(&unshadowed, open) * 8,
        "open ground stays lit with shadows on"
    );
}
