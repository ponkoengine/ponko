//! A skinned model drawn through the whole frame offscreen: posing its
//! skeleton must move its pixels (collection, posed mesh ids, the skinning
//! pass and the depth, opaque and shadow passes together). So must changing
//! the weights of an unskinned model's morph target. Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::anim::{Joint, MorphTarget, MorphWeights, Skeleton};
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{MaterialDesc, MeshData, Model};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 64;

#[test]
fn posing_a_skeleton_moves_the_drawn_pixels() {
    draw_bent(false, false, false, false);
}

#[test]
fn morph_weights_move_the_drawn_pixels() {
    draw_bent(true, false, false, false);
}

#[test]
fn culled_models_before_visible_skin_and_morph_models_keep_their_slots() {
    draw_bent(false, true, false, false);
    draw_bent(true, true, false, false);
}

#[test]
fn culled_models_keep_their_slots_on_the_cpu_skinning_fallback() {
    draw_bent(false, true, true, false);
    draw_bent(true, true, true, false);
}

#[test]
fn outgrown_skin_buffers_pose_and_draw_as_before() {
    // 80 more skinned (or morphed) models after the first frame: the skin
    // matrices and morph weights outgrow their buffers, which are made
    // anew with new bind groups; the bar still bends and comes back.
    draw_bent(false, false, false, true);
    draw_bent(true, false, false, true);
    draw_bent(false, false, true, true);
}

/// Draws a bar standing up, then bent left: by its skeleton, or (`morph`)
/// unskinned, by a morph target. `grow`: 80 more of them, out of view,
/// before it bends.
fn draw_bent(morph: bool, mixed: bool, downlevel: bool, grow: bool) {
    let Some(gpu) = crate::render::render_test_support::gpu_with(|_| {
        let descriptor = wgpu::DeviceDescriptor {
            required_limits: if downlevel {
                wgpu::Limits::downlevel_webgl2_defaults()
            } else {
                wgpu::Limits::default()
            },
            ..Default::default()
        };
        Ok(descriptor)
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

    // A bar 2 tall standing on the origin: the lower half on joint 0, the
    // upper half on joint 1 (1 up). Glowing, so light doesn't matter.
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
    let skeleton = Skeleton::new(vec![
        Joint::new("base", None),
        Joint::new("top", 0).with_rest(Transform3D::at(vec3(0.0, 1.0, 0.0))),
    ]);
    let mut bar = MeshData::cube(1.0);
    for p in &mut bar.positions {
        *p = vec3(p.x * 0.3, (p.y + 0.5) * 2.0, p.z * 0.3);
    }
    if morph {
        // "lean": the top corners go left and down.
        bar.morph_targets.push(MorphTarget::new(
            "lean",
            bar.positions
                .iter()
                .map(|p| if p.y > 1.0 { vec3(-1.6, -0.6, 0.0) } else { Vec3::ZERO })
                .collect(),
        ));
    } else {
        bar.joints = bar.positions.iter().map(|p| [u16::from(p.y > 1.0), 0, 0, 0]).collect();
        bar.weights = vec![[1.0, 0.0, 0.0, 0.0]; bar.positions.len()];
    }
    let glow = MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: Color::WHITE,
        ..Default::default()
    };
    let material = ctx.gfx.create_material(glow);
    let entity = if morph {
        let mesh = ctx.gfx.create_mesh(&bar, material);
        if mixed {
            for _ in 0..3 {
                ctx.world.spawn((
                    Transform3D::at(vec3(1e6, 0.0, 0.0)),
                    Model::new(mesh),
                    crate::anim::MorphWeights::default(),
                ));
            }
        }
        ctx.world.spawn((
            Transform3D::default(),
            Model::new(mesh),
            crate::anim::MorphWeights::default(),
        ))
    } else {
        let mesh = ctx.gfx.create_skinned_mesh(&bar, material, &skeleton);
        if mixed {
            for _ in 0..3 {
                ctx.world
                    .spawn((Transform3D::at(vec3(1e6, 0.0, 0.0)), Model::new(mesh), skeleton.clone()));
            }
        }
        ctx.world.spawn((Transform3D::default(), Model::new(mesh), skeleton))
    };
    ctx.scene3d.camera.position = vec3(0.0, 1.0, 6.0);
    ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
    ctx.scene3d.lighting.shadows = true; // the shadow passes draw it too
    ctx.scene3d.sky.atmosphere = false; // a black background

    let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
    renderer.create_material(&device, &GpuMaterialDesc::new(&glow), &textures);
    let mut weighted = bar.clone();
    for w in &mut weighted.weights {
        *w = crate::three::normalized_weights(*w);
    }
    renderer.create_mesh(&device, &weighted, &[(0..bar.indices.len() as u32, material)], true);

    let mut draw = |ctx: &mut Ctx| -> Vec<u8> {
        let mut skins = SkinFrame::default();
        skins.collect(&mut ctx.world, &ctx.gfx.assets3d);
        let mut frame = Frame3D::default();
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        frame.skins = skins;
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
    // Brightness of the pixel where the camera sees `at`.
    let lit = |pixels: &[u8], ctx: &Ctx, at: Vec3| {
        let clip = ctx.scene3d.camera.view_proj() * at.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as usize;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as usize;
        pixels[(y * SIZE as usize + x) * 4] > 128
    };
    // Leaning, the bar's top is at x -1.9 to -1.3, y 1.4.
    let (above, beside) = (vec3(0.0, 1.8, 0.0), vec3(if morph { -1.1 } else { -0.8 }, 1.0, 0.0));

    let rest = draw(&mut ctx);
    assert!(lit(&rest, &ctx, above), "at rest the top half stands up");
    assert!(!lit(&rest, &ctx, beside));

    if grow {
        let (model, transform) = (
            *ctx.world.get::<Model>(entity).unwrap(),
            Transform3D::at(vec3(1e6, 0.0, 0.0)),
        );
        for _ in 0..80 {
            if morph {
                let mut weights = MorphWeights::default();
                weights.set(0, 0.5);
                ctx.world.spawn((transform, model, weights));
            } else {
                let skeleton = ctx.world.get::<Skeleton>(entity).unwrap().clone();
                ctx.world.spawn((transform, model, skeleton));
            }
        }
    }
    if morph {
        ctx.world
            .get_mut::<crate::anim::MorphWeights>(entity)
            .unwrap()
            .set(0, 1.0);
    } else {
        let s = ctx.world.get_mut::<Skeleton>(entity).unwrap();
        s.pose[1].rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    }
    crate::render::traffic::take();
    let bent = draw(&mut ctx);
    let grew = crate::render::traffic::take();
    assert_eq!(grew.reallocations > 0, grow, "{grew:?}");
    assert!(!lit(&bent, &ctx, above), "bent, nothing stands above");
    assert!(lit(&bent, &ctx, beside), "bent, the top half points left");

    // Held, the pose isn't uploaded again: on the compute path nothing
    // changed in the matrices or weights, and the CPU fallback (no compute
    // shaders: `downlevel`, or a forced tier) keeps the corners it posed.
    crate::render::traffic::take();
    assert_eq!(draw(&mut ctx), bent);
    let held = crate::render::traffic::take();
    assert_eq!(held.skins, 0, "a held pose: {held:?}");
    // Back at rest: what changed is uploaded, and draws as at first.
    if morph {
        ctx.world.get_mut::<MorphWeights>(entity).unwrap().set(0, 0.0);
    } else {
        ctx.world.get_mut::<Skeleton>(entity).unwrap().pose[1].rotation = Quat::IDENTITY;
    }
    assert_eq!(draw(&mut ctx), rest, "back at rest");
    renderer.check_mirrors(&device, &queue);
}
