//! Material updates use the existing bindings until a handle or GPU resource changes.
use super::*;
use crate::three::MaterialDesc;

struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer3D,
    textures: Vec<GpuTexture>,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(|_| Ok(Default::default()))?;
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
        let white = Self::texture(&device, &queue, [255; 4], 1);
        let renderer = Renderer3D::new(&device, &layout, wgpu::TextureFormat::Rgba8Unorm, 0, &white, 4, true);
        let view = white.create_view(&Default::default());
        let sampler = device.create_sampler(&Default::default());
        let dummy = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        });
        Some(Self {
            device,
            queue,
            renderer,
            textures: vec![GpuTexture {
                texture: white,
                bind_group: dummy,
                nearest: false,
                mips: None,
            }],
        })
    }

    fn texture(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4], mips: u32) -> wgpu::Texture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("material test texture"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for mip in 0..mips {
            let size = 2 >> mip;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    mip_level: mip,
                    ..texture.as_image_copy()
                },
                &rgba.repeat((size * size) as usize),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * size),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
        }
        texture
    }

    /// Sample the actual material bind group, including its uniform buffer.
    fn pixel(&self, id: u32, array: bool) -> [u8; 4] {
        let sample = if array {
            "textureLoad(maps, vec2i(0), 0, 0)"
        } else {
            "textureLoad(base, vec2i(0), 0)"
        };
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    r#"
                struct Material {{ color: vec4f }}
                @group(0) @binding(0) var<uniform> material: Material;
                @group(0) @binding(2) var base: texture_2d<f32>;
                @group(0) @binding(8) var maps: texture_2d_array<f32>;
                @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {{
                    let p = array(vec2f(-1, -1), vec2f(3, -1), vec2f(-1, 3));
                    return vec4f(p[i], 0, 1);
                }}
                @fragment fn fs() -> @location(0) vec4f {{ return {sample} * material.color; }}
            "#
                )
                .into(),
            ),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&self.renderer.resources.material_layout)],
            immediate_size: 0,
        });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let output = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &self.renderer.resources.materials[id as usize].bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            output.size(),
        );
        self.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        readback.get_mapped_range(..).unwrap()[..4].try_into().unwrap()
    }

    fn update(&mut self, id: u32, desc: &GpuMaterialDesc, rebuild: bool) {
        let before = self.renderer.resources.materials[id as usize].bind_group.clone();
        self.renderer
            .update_material(&self.device, &self.queue, id, desc, &self.textures);
        assert_eq!(
            before != self.renderer.resources.materials[id as usize].bind_group,
            rebuild
        );
    }
}

#[test]
fn uniform_updates_reuse_bindings_and_resource_changes_sample_new_textures() {
    let Some(mut rig) = Rig::new() else { return };
    let mut desc = GpuMaterialDesc::new(&MaterialDesc::default());
    let id = rig.renderer.create_material(&rig.device, &desc, &rig.textures);
    // 0.4 of 255 is 102 exactly. A half (127.5) lands between two levels,
    // and adapters differ on which way they round it (127 or 128).
    desc.uniform.color = [0.4, 1.0, 1.0, 1.0];
    desc.transparent = true;
    desc.cutout = true;
    desc.double_sided = true;
    rig.update(id, &desc, false);
    assert!(rig.renderer.resources.materials[id as usize].transparent);
    assert!(rig.renderer.resources.materials[id as usize].cutout);
    assert!(rig.renderer.resources.materials[id as usize].double_sided);
    assert_eq!(rig.pixel(id, false), [102, 255, 255, 255]);

    let red = Rig::texture(&rig.device, &rig.queue, [255, 0, 0, 255], 1);
    rig.textures.push(GpuTexture {
        texture: red.clone(),
        bind_group: rig.textures[0].bind_group.clone(),
        nearest: false,
        mips: None,
    });
    desc.textures[0] = Some(1);
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, false), [102, 0, 0, 255]);
    rig.update(id, &desc, false);

    // Same ID, new resource (as with reload, free/reuse, or mip promotion).
    rig.textures[1].texture = Rig::texture(&rig.device, &rig.queue, [0, 0, 255, 255], 2);
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, false), [0, 0, 255, 255]);
    rig.update(id, &desc, false);

    // An unresolved ID initially uses white, then resolves after an upload.
    desc.textures[0] = Some(2);
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, false), [102, 255, 255, 255]);
    rig.textures.push(GpuTexture {
        texture: red.clone(),
        bind_group: rig.textures[0].bind_group.clone(),
        nearest: false,
        mips: None,
    });
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, false), [102, 0, 0, 255]);

    desc.arrays[0] = Some(0);
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, true), [102, 255, 255, 255]);
    rig.renderer.add_texture_array(&red);
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, true), [102, 0, 0, 255]);
    rig.update(id, &desc, false);
    let blue = Rig::texture(&rig.device, &rig.queue, [0, 0, 255, 255], 1);
    desc.arrays[0] = Some(rig.renderer.add_texture_array(&blue));
    rig.update(id, &desc, true);
    assert_eq!(rig.pixel(id, true), [0, 0, 255, 255]);
}

#[test]
#[ignore = "release GPU microbenchmark: cargo test --release --features 3d --lib material_update_cpu_cost -- --ignored --nocapture"]
fn material_update_cpu_cost() {
    let Some(mut rig) = Rig::new() else { return };
    let desc = GpuMaterialDesc::new(&MaterialDesc::default());
    for _ in 0..200 {
        rig.renderer.create_material(&rig.device, &desc, &rig.textures);
    }
    for rebuild in [true, false] {
        let mut times = Vec::new();
        for frame in 0..105 {
            let start = std::time::Instant::now();
            for id in 0..200 {
                if rebuild {
                    // Baseline: the previous unconditional view/bind group rebuild.
                    let material = &rig.renderer.resources.materials[id as usize];
                    rig.queue
                        .write_buffer(&material.buffer, 0, bytemuck::bytes_of(&desc.uniform));
                    let group =
                        rig.renderer
                            .resources
                            .material_bind_group(&rig.device, &material.buffer, &material.bindings);
                    rig.renderer.resources.materials[id as usize].bind_group = group;
                } else {
                    rig.renderer
                        .update_material(&rig.device, &rig.queue, id, &desc, &rig.textures);
                }
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            rig.queue.submit([]);
            rig.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if frame >= 5 {
                times.push(ms);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "200 materials/frame, rebuild={rebuild}, 5 warmup + 100 frames: CPU p50={:.3}ms p95={:.3}ms",
            times[49], times[94]
        );
    }
}
