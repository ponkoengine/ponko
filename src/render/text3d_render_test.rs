//! 3D text drawn offscreen through the particle pass: the SDF atlas, the
//! text pipeline and its shader together. Its fill and outline colors must
//! both show, and only where the letters are. Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::HDR_FORMAT;
use super::particles::Particles;
#[allow(unused_imports)]
use crate::d3::{Camera as Camera3D, Text as Text3D, Transform as Transform3D};
use crate::prelude::*;
use crate::text::Text;
use crate::three::particles::ParticleFrame;

const SIZE: u32 = 64;

/// Draws a big "I" (red, with `outline` around it) and returns the frame's
/// pixels as RGBA floats.
fn draw(outline: Color) -> Option<Vec<[f32; 4]>> {
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

    // Lay the text out: its glyphs land in the SDF atlas' uploads.
    let mut text = Text::new();
    let label = Text3D {
        size: 1.6,
        color: Color::rgb(1.0, 0.0, 0.0),
        outline,
        outline_width: 0.1,
        ..Text3D::new("I")
    };
    let mut camera = crate::three::Camera3D::new(Vec2::splat(SIZE as f32));
    camera.rotation = Quat::IDENTITY;
    let mut frame = ParticleFrame::default();
    frame.texts.push(&label, &Transform3D::default(), &camera, &mut text);
    frame.texts.atlas = Some(Texture::new(0, Text::ATLAS_SIZE, Text::ATLAS_SIZE));

    let side = Text::ATLAS_SIZE;
    let atlas = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (at, size, rgba) in &text.sdf.uploads {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: at[0],
                    y: at[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let view = atlas.create_view(&Default::default());
    let textures = [GpuTexture {
        texture: atlas.clone(),
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

    let target = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let color = target(
        HDR_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = target(
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let (color_view, depth_view) = (
        color.create_view(&Default::default()),
        depth.create_view(&Default::default()),
    );

    let mut particles = Particles::for_test(&device, &layout, 1);
    // Two units across, looking down -Z at the text on z = 0.
    let view_proj = glam::camera::rh::proj::directx::orthographic(-1.0, 1.0, -1.0, 1.0, -1.0, 1.0);
    particles.prepare(&device, &queue, &frame, view_proj, glam::Vec3::ZERO);
    assert!(particles.has_draws());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0), // the reverse-Z far plane
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        particles.draw(&mut pass, &textures, 0);
    }
    // Rgba16Float: 8 bytes a pixel, rows padded to 256 bytes.
    let row = (SIZE * 8).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        color.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = buffer.slice(..).get_mapped_range().unwrap();
    let mut pixels = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = (y * row + x * 8) as usize;
            let half = |i: usize| f16_to_f32(u16::from_le_bytes([data[at + i * 2], data[at + i * 2 + 1]]));
            pixels.push([half(0), half(1), half(2), half(3)]);
        }
    }
    Some(pixels)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

#[test]
fn text_draws_its_fill_and_outline_only_where_the_letters_are() {
    let Some(outlined) = draw(Color::rgb(0.0, 1.0, 0.0)) else {
        crate::render::render_test_support::no_gpu("no GPU adapter or device");
        return;
    };
    let red = |p: &[f32; 4]| p[0] > 0.5 && p[1] < 0.2;
    let green = |p: &[f32; 4]| p[1] > 0.5 && p[0] < 0.2;
    let reds = outlined.iter().filter(|p| red(p)).count();
    let greens = outlined.iter().filter(|p| green(p)).count();
    assert!(reds > 20, "the letter is filled: {reds} red pixels");
    assert!(greens > 20, "and outlined: {greens} green pixels");
    // The middle of the I is filled, the corners are clear.
    let at = |x: u32, y: u32| outlined[(y * SIZE + x) as usize];
    assert!(red(&at(SIZE / 2, SIZE / 2)), "{:?}", at(SIZE / 2, SIZE / 2));
    assert!(at(0, 0)[3] < 0.01 && at(SIZE - 1, SIZE - 1)[3] < 0.01);
    // Without an outline: the same fill, nothing green.
    let plain = draw(Color::TRANSPARENT).unwrap();
    assert_eq!(plain.iter().filter(|p| green(p)).count(), 0);
    let plain_reds = plain.iter().filter(|p| red(p)).count();
    assert!(plain_reds.abs_diff(reds) <= reds / 10, "{plain_reds} vs {reds}");
}
