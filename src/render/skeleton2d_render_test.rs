//! 2D skeletons drawn offscreen through the window's sprite pass: a skinned
//! mesh covers what its triangles cover, follows its bones when they turn
//! (vertices weighted between two bones land between them), samples its
//! texture by its UVs, and a rigid part on a bone draws over it in slot
//! order. The background where nothing is drawn, and a hidden slot, are the
//! controls. Skips without a GPU.

use std::sync::Arc;

use glam::{Vec2, vec2};

use super::batch::{DrawList, SpriteInstance};
use super::gpu::{GpuTexture, Renderer, SpritePass, sprite_pipeline};
use super::render_test_support::{gpu, gpu_adjusted, read_texture, target, white_textures};
use super::sprite_pipelines::SpritePipelines;
use crate::color::Color;
use crate::ecs::Transform2D;
use crate::skeleton2d::{BonePose, MeshVertex, Skeleton, SkeletonData, SkinnedMesh, SpritePart};
use crate::texture::Texture;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (64, 64);
const BACKGROUND: Color = Color::rgb(0.5, 0.5, 0.5);

struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture_layout: wgpu::BindGroupLayout,
    camera_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    looks: SpritePipelines,
    textures: Vec<GpuTexture>,
}

impl Rig {
    fn new(gpu: Option<super::render_test_support::TestGpu>) -> Option<Self> {
        let gpu = gpu?;
        let (device, queue) = (gpu.device, gpu.queue);
        let (texture_layout, _, textures) = white_textures(&device, &queue);
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("sprite.wgsl"));
        let pipeline = sprite_pipeline(&device, &layout, &shader, FORMAT);
        let looks = SpritePipelines::new(&device, &queue, &shader, &layout, &camera_layout, &texture_layout);
        Some(Self {
            device,
            queue,
            texture_layout,
            camera_layout,
            pipeline,
            looks,
            textures,
        })
    }

    /// Adds a `width` x 1 sRGB texture of `rgba` texels; returns it.
    fn texture(&mut self, rgba: &[[u8; 4]]) -> Texture {
        let width = rgba.len() as u32;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            rgba.as_flattened(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: None,
            },
            texture.size(),
        );
        let view = texture.create_view(&Default::default());
        let sampler = self.device.create_sampler(&Default::default());
        self.textures.push(GpuTexture {
            bind_group: self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.texture_layout,
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
            texture,
            nearest: false,
            mips: None,
        });
        Texture::new((self.textures.len() - 1) as u32, width, 1)
    }

    /// Draws `skeleton` at `at` over the background; returns the image.
    fn draw(&mut self, skeleton: &mut Skeleton, at: Vec2) -> Image {
        use wgpu::util::DeviceExt;
        let mut list = DrawList::default();
        crate::skeleton2d::draw::draw(&mut list, &Transform2D::at(at), skeleton, 0);
        let (instances, batches) = list.finish();
        self.looks.prepare(&self.device, &self.textures, batches, FORMAT);
        let device = &self.device;
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<SpriteInstance, u8>(instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let projection = crate::ctx::screen_projection(Vec2::new(SIZE.0 as f32, SIZE.1 as f32));
        let camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&projection.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let camera = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let output = target(device, SIZE, FORMAT);
        let view = output.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        Renderer::sprite_pass_with(
            &mut encoder,
            &view,
            Some(BACKGROUND),
            None,
            &SpritePass {
                viewport: [SIZE.0, SIZE.1],
                scale: 1.0,
                pipeline: &self.pipeline,
                instances: &instances,
                textures: &self.textures,
                layers: &[(&camera, batches, 0)],
            },
            Some((&self.looks, FORMAT)),
        );
        self.queue.submit([encoder.finish()]);
        Image(read_texture(device, &self.queue, &output))
    }
}

struct Image(Vec<u8>);

/// What a pixel shows.
#[derive(Debug, PartialEq)]
enum Seen {
    Background,
    Red,
    Blue,
    Green,
    Other([u8; 4]),
}

impl Image {
    fn at(&self, x: u32, y: u32) -> Seen {
        let i = ((y * SIZE.0 + x) * 4) as usize;
        let p = [self.0[i], self.0[i + 1], self.0[i + 2], self.0[i + 3]];
        let close = |c: [u8; 3]| (0..3).all(|k| p[k].abs_diff(c[k]) <= 8);
        let grey = BACKGROUND.to_rgba8();
        if close([grey[0], grey[1], grey[2]]) {
            Seen::Background
        } else if close([255, 0, 0]) {
            Seen::Red
        } else if close([0, 0, 255]) {
            Seen::Blue
        } else if close([0, 255, 0]) {
            Seen::Green
        } else {
            Seen::Other(p)
        }
    }

    fn covered(&self, x: u32, y: u32) -> bool {
        self.at(x, y) != Seen::Background
    }
}

/// A 40 x 8 band along +x from the root: its left column on `root`, its
/// middle column half on `root` and half on `bend` (at x = 20), its right
/// column on `bend`. Red on the left, blue on the right. A green 8 x 8 part
/// on `bend`, in the slot after the band.
fn band(rig: &mut Rig) -> Skeleton {
    let red_blue = rig.texture(&[[255, 0, 0, 255], [0, 0, 255, 255]]);
    let green = rig.texture(&[[0, 255, 0, 255]]);
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let bend = data.add_bone("bend", Some(root), BonePose::at(vec2(20.0, 0.0)), 20.0);
    let mut vertices = Vec::new();
    for y in [-4.0, 4.0] {
        for (x, weights) in [
            (0.0, &[(root, 1.0)][..]),
            (20.0, &[(root, 1.0), (bend, 1.0)][..]),
            (40.0, &[(bend, 1.0)][..]),
        ] {
            vertices.push(MeshVertex::new(vec2(x, y), vec2(x / 40.0, (y + 4.0) / 8.0), weights));
        }
    }
    let slot = data.add_slot("band", root);
    let mesh = SkinnedMesh::new(red_blue, vertices, vec![0, 1, 3, 1, 4, 3, 1, 2, 4, 2, 5, 4]);
    data.add_attachment(slot, "band", mesh);
    let slot = data.add_slot("part", bend);
    let part = SpritePart::new(green)
        .with_size(vec2(4.0, 4.0))
        .with_pivot(vec2(0.0, 0.5));
    data.add_attachment(slot, "part", part);
    Skeleton::new(Arc::new(data))
}

fn skinned_mesh_cases(rig: &mut Rig) {
    let mut s = band(rig);
    let at = vec2(10.0, 16.0);
    let rest = rig.draw(&mut s, at);
    // The band covers x 10..50, y 12..20: red at its left end, blue at its
    // right end, the green part at the joint drawn over it.
    assert_eq!(rest.at(12, 16), Seen::Red, "left end samples u = 0");
    assert_eq!(rest.at(48, 16), Seen::Blue, "right end samples u = 1");
    assert_eq!(rest.at(32, 16), Seen::Green, "the later slot draws over the mesh");
    assert!(rest.covered(25, 13) && rest.covered(45, 19));
    // Controls: outside the band, nothing.
    for (x, y) in [(5, 16), (55, 16), (30, 8), (30, 24), (30, 40)] {
        assert_eq!(rest.at(x, y), Seen::Background, "({x}, {y})");
    }

    // A quarter turn of `bend` (clockwise, y down) swings the right half
    // down: the tip now sits near (30, 36), and the middle column, half on
    // each bone, sits between (20, 0) and where `bend` alone takes it.
    s.set_bone_pose(
        1,
        BonePose::at(vec2(20.0, 0.0)).with_rotation(std::f32::consts::FRAC_PI_2),
    );
    let bent = rig.draw(&mut s, at);
    assert_eq!(bent.at(48, 16), Seen::Background, "the right end has moved");
    assert_eq!(bent.at(30, 34), Seen::Blue, "the tip, below the joint");
    assert_eq!(bent.at(12, 16), Seen::Red, "the root's part stays");
    assert!(bent.covered(30, 24), "the bend between them");
    // The part turns with its bone: now below the joint, not right of it.
    assert_eq!(bent.at(30, 18), Seen::Green);
    assert_eq!(bent.at(36, 16), Seen::Background);

    // A hidden slot draws nothing.
    s.set_attachment(0, Some(None));
    let hidden = rig.draw(&mut s, at);
    assert_eq!(hidden.at(12, 16), Seen::Background);
    assert_eq!(hidden.at(30, 18), Seen::Green);
}

#[test]
fn skinned_meshes_follow_their_bones_and_sample_their_uvs() {
    let Some(mut rig) = Rig::new(gpu()) else { return };
    skinned_mesh_cases(&mut rig);
}

/// A region a packer stored a quarter turn counter-clockwise draws upright:
/// the 2 x 1 page red | blue holds a 1 x 2 image, red on top.
#[test]
fn rotated_regions_draw_upright() {
    let Some(mut rig) = Rig::new(gpu()) else { return };
    let red_blue = rig.texture(&[[255, 0, 0, 255], [0, 0, 255, 255]]);
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let slot = data.add_slot("part", root);
    let part = SpritePart::new(red_blue).with_rotated_region(crate::camera::Rect::new(0.0, 0.0, 2.0, 1.0));
    assert_eq!(part.size, vec2(1.0, 2.0), "upright: as tall as the region is wide");
    data.add_attachment(slot, "part", part.with_size(vec2(8.0, 16.0)));
    let mut s = Skeleton::new(Arc::new(data));
    // Covers x 28..36, y 24..40.
    let image = rig.draw(&mut s, vec2(32.0, 32.0));
    for x in [29, 34] {
        assert_eq!(image.at(x, 26), Seen::Red, "the top, at x {x}");
        assert_eq!(image.at(x, 38), Seen::Blue, "the bottom, at x {x}");
    }
    // Control: drawn unturned, the same region is red left, blue right.
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let slot = data.add_slot("part", root);
    let flat = SpritePart::new(red_blue).with_region(crate::camera::Rect::new(0.0, 0.0, 2.0, 1.0));
    data.add_attachment(slot, "part", flat.with_size(vec2(8.0, 16.0)));
    let mut s = Skeleton::new(Arc::new(data));
    let image = rig.draw(&mut s, vec2(32.0, 32.0));
    assert_eq!(image.at(29, 38), Seen::Red);
    assert_eq!(image.at(34, 26), Seen::Blue);
    assert_eq!(image.at(40, 32), Seen::Background);
}

#[test]
fn skinned_meshes_draw_with_webgl2_limits() {
    let gpu = gpu_adjusted(
        |_| Ok(wgpu::DeviceDescriptor::default()),
        |d| d.required_limits = wgpu::Limits::downlevel_webgl2_defaults(),
    );
    let Some(mut rig) = Rig::new(gpu) else { return };
    skinned_mesh_cases(&mut rig);
}
