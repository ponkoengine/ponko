//! Render-pass IBL baking using the existing sky texture binding.
use super::gpu3d::HDR_FORMAT;
use crate::render::traffic::{Counted, Upload};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const SHADER: &str = concat!(
    include_str!("sky_sampling.wgsl"),
    "\n",
    include_str!("environment.wgsl")
);

pub(super) struct Environment {
    pub atlas: wgpu::Texture,
    source: wgpu::Texture,
    source_group: wgpu::BindGroup,
    sky_group: wgpu::BindGroup,
    buffers: Vec<wgpu::Buffer>,
    /// What `buffers[0]` (the sky settings) holds: only changes are uploaded.
    settings_mirror: crate::render::traffic::Mirror,
    groups: Vec<wgpu::BindGroup>,
    cube: wgpu::RenderPipeline,
    filter: wgpu::RenderPipeline,
    diffuse: wgpu::RenderPipeline,
    /// Projects a slot's cube onto spherical harmonics, one row of `sh` per slot.
    sh: wgpu::RenderPipeline,
    sh_texture: wgpu::Texture,
    /// Each slot's config, reading `sh_texture`: for the diffuse pass.
    sh_groups: Vec<wgpu::BindGroup>,
    brdf: wgpu::RenderPipeline,
    capture: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    capture_groups: Vec<(u32, Vec<wgpu::BindGroup>)>,
    /// Set until the frame that records the sky bake is drawn: a frame that
    /// is prepared but then skipped (hidden window) must not lose the bake.
    brdf_pending: AtomicBool,
    dirty: AtomicBool,
    pub bakes: Vec<(u32, [u32; 6])>,
    uploads: Vec<(u32, wgpu::Texture)>,
    /// The sky lighting the image-based light was last baked from.
    key: Option<[f32; 22]>,
    /// Frames since the sky drifted from `key` by less than a visible step.
    stale: u32,
}

/// Bakes are full-screen filters of the whole cube (several milliseconds), so
/// a sky that drifts every frame (a moving sun, the camera's height) is baked
/// again only when it has visibly changed, or after this many frames of small
/// drift, so the lighting still follows slow changes.
const REBAKE_AFTER: u32 = 60;
/// Parts each spherical harmonic coefficient is summed in (`SH_PARTS` in
/// environment.wgsl): six faces, eight bands of rows each.
const SH_PARTS: u32 = 48;
/// A change of this share of a value (or of 1, for values below 1) is visible:
/// about half a degree of sun, or one percent of a light's color.
const VISIBLE_CHANGE: f32 = 0.01;

/// Whether the image-based light should be baked again for sky `key`, having
/// last been baked from `baked`, with `stale` frames of small drift so far.
/// Returns the decision and the new drift count.
fn rebake(baked: Option<&[f32; 22]>, key: &[f32; 22], stale: u32) -> (bool, u32) {
    let Some(baked) = baked else {
        return (true, 0);
    };
    let mut drift = false;
    for (a, b) in baked.iter().zip(key) {
        let change = (a - b).abs();
        if change.is_nan() || change > VISIBLE_CHANGE * a.abs().max(b.abs()).max(1.0) {
            return (true, 0);
        }
        drift |= change > 0.0;
    }
    match drift {
        false => (false, 0),
        true if stale + 1 >= REBAKE_AFTER => (true, 0),
        true => (false, stale + 1),
    }
}
impl Environment {
    pub fn new(device: &wgpu::Device, sky: &wgpu::TextureView) -> Self {
        let texture = |label, width, height| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let atlas = texture("IBL atlas", 1024, 1536);
        let source = texture("IBL cube source", 768, 128);
        // Each of the five slots' nine coefficients in SH_PARTS parts.
        let sh_texture = texture("IBL spherical harmonics", 9 * SH_PARTS, 5);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("IBL bake"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let buffers: Vec<_> = (0..5)
            .map(|slot| {
                device
                    .uploading(Upload::Uniforms)
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("IBL config"),
                        contents: bytemuck::cast_slice(&[[slot as f32, 0.0, 1.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4]]),
                        usage: wgpu::BufferUsages::UNIFORM
                            | wgpu::BufferUsages::COPY_DST
                            | crate::render::traffic::MIRRORED,
                    })
            })
            .collect();
        let group = |view: &wgpu::TextureView, buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("IBL source"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let source_view = source.create_view(&Default::default());
        let source_group = group(&source_view, &buffers[0]);
        let sky_group = group(sky, &buffers[0]);
        let groups = buffers.iter().map(|b| group(&source_view, b)).collect();
        let sh_view = sh_texture.create_view(&Default::default());
        let sh_groups = buffers.iter().map(|b| group(&sh_view, b)).collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("IBL bake"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("IBL bake"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(HDR_FORMAT.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            cube: pipeline("cube"),
            filter: pipeline("prefilter"),
            diffuse: pipeline("diffuse"),
            sh: pipeline("sh"),
            sh_texture,
            sh_groups,
            brdf: pipeline("brdf"),
            capture: pipeline("capture"),
            layout,
            sampler,
            capture_groups: Vec::new(),
            atlas,
            source,
            source_group,
            sky_group,
            buffers,
            settings_mirror: Default::default(),
            groups,
            brdf_pending: AtomicBool::new(true),
            dirty: AtomicBool::new(true),
            bakes: Vec::new(),
            key: None,
            stale: 0,
            uploads: Vec::new(),
        }
    }
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        key: [f32; 22],
        atmosphere: bool,
        intensity: f32,
        // Sky, ground and equator light (w 1 to use the equator).
        [sky, ground, equator]: [[f32; 4]; 3],
    ) {
        let (bake, stale) = rebake(self.key.as_ref(), &key, self.stale);
        self.stale = stale;
        if bake {
            self.dirty.store(true, Ordering::Relaxed);
            self.key = Some(key);
        }
        let settings = [[0.0, f32::from(atmosphere), intensity, 0.0], sky, ground, equator];
        let settings = bytemuck::cast_slice(&settings);
        self.settings_mirror
            .upload(queue, Upload::Uniforms, &self.buffers[0], 0, settings);
    }
    fn draw(
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
        rect: [u32; 4],
    ) {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("IBL bake"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(rect[0] as f32, rect[1] as f32, rect[2] as f32, rect[3] as f32, 0.0, 1.0);
        pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
    fn filter(&self, encoder: &mut wgpu::CommandEncoder, slot: u32) {
        let y = 128 + slot * 256;
        Self::draw(
            encoder,
            &self.atlas,
            &self.filter,
            &self.groups[slot as usize],
            [0, y, 768, 255],
        );
        Self::draw(
            encoder,
            &self.sh_texture,
            &self.sh,
            &self.groups[slot as usize],
            [0, slot, 9 * SH_PARTS, 1],
        );
        Self::draw(
            encoder,
            &self.atlas,
            &self.diffuse,
            &self.sh_groups[slot as usize],
            [800, y, 96, 16],
        );
    }
    pub fn sky(&self, encoder: &mut wgpu::CommandEncoder, sky: &wgpu::Texture) {
        if self.brdf_pending.swap(false, Ordering::Relaxed) {
            super::gpu3d::mark_step("environment lookup");
            Self::draw(encoder, &self.atlas, &self.brdf, &self.source_group, [800, 0, 128, 128]);
        }
        if self.dirty.swap(false, Ordering::Relaxed) {
            super::gpu3d::mark_step("sky maps");
            encoder.copy_texture_to_texture(sky.as_image_copy(), self.atlas.as_image_copy(), sky.size());
            Self::draw(encoder, &self.source, &self.cube, &self.sky_group, [0, 0, 768, 128]);
            self.filter(encoder, 0);
        }
    }
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, uploads: &[(u32, std::sync::Arc<Vec<u16>>)]) {
        self.uploads.clear();
        for (slot, data) in uploads {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("loaded probe"),
                size: self.source.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            queue.uploading(Upload::Textures).write_texture(
                texture.as_image_copy(),
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(768 * 8),
                    rows_per_image: Some(128),
                },
                texture.size(),
            );
            self.uploads.push((*slot, texture));
        }
    }
    /// Whether the lookup table is still to draw (see [`sky`](Self::sky)).
    pub fn needs_lookup(&self) -> bool {
        self.brdf_pending.load(Ordering::Relaxed)
    }
    /// Whether this frame re-renders the sky environment.
    pub fn needs_sky(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
    pub fn is_capture(&self, target: u32) -> bool {
        self.bakes.iter().any(|(_, faces)| faces.contains(&target))
    }
    pub fn has_probes(&self) -> bool {
        !self.bakes.is_empty() || !self.uploads.is_empty()
    }
    /// Whether [`probes`](Self::probes) records anything: a loaded probe,
    /// or a bake whose six faces are all render targets (see
    /// [`prepare_bakes`](Self::prepare_bakes)).
    pub fn records_probes(&self) -> bool {
        !self.uploads.is_empty() || !self.capture_groups.is_empty()
    }
    /// The maps' atlas and the cube strip they are filtered from: for
    /// checking them against what the frame's passes declare.
    pub fn images(&self) -> [&wgpu::Texture; 2] {
        [&self.atlas, &self.source]
    }
    /// Binds each captured face of this frame's bakes, for `probes`.
    pub fn prepare_bakes(&mut self, device: &wgpu::Device, targets: &[(u32, wgpu::Texture)]) {
        self.capture_groups.clear();
        for &(slot, faces) in &self.bakes {
            let groups: Vec<_> = faces
                .iter()
                .filter_map(|id| targets.iter().find(|(t, _)| t == id))
                .map(|(_, texture)| {
                    let view = texture.create_view(&Default::default());
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("IBL capture"),
                        layout: &self.layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: self.buffers[0].as_entire_binding(),
                            },
                        ],
                    })
                })
                .collect();
            if groups.len() == 6 {
                self.capture_groups.push((slot, groups));
            }
        }
    }
    pub fn probes(&self, encoder: &mut wgpu::CommandEncoder) {
        for (slot, texture) in &self.uploads {
            super::gpu3d::mark_step("probes");
            encoder.copy_texture_to_texture(texture.as_image_copy(), self.source.as_image_copy(), texture.size());
            self.filter(encoder, *slot);
        }
        for (slot, groups) in &self.capture_groups {
            super::gpu3d::mark_step("probes");
            // Each face lands in the cube strip, with the sky behind what the
            // capture didn't cover.
            for (face, group) in groups.iter().enumerate() {
                Self::draw(
                    encoder,
                    &self.source,
                    &self.capture,
                    group,
                    [face as u32 * 128, 0, 128, 128],
                );
            }
            self.filter(encoder, *slot);
        }
    }
}

#[cfg(test)]
impl Environment {
    /// See `Renderer3D::check_mirrors`.
    pub fn check_mirrors(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.settings_mirror
            .check(device, queue, &self.buffers[0], 0, "environment settings");
    }
}

impl Environment {
    /// Counts the sky and reflection probe cubemaps, their filtered levels
    /// and harmonics, and what bakes them as probes (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.texture(C::Probes, &self.atlas);
        t.texture(C::Probes, &self.source);
        t.texture(C::Probes, &self.sh_texture);
        for buffer in &self.buffers {
            t.buffer(C::Probes, buffer);
        }
        for (_, texture) in &self.uploads {
            t.texture(C::Probes, texture);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_sky_drift_rebakes_once_a_second_and_visible_changes_at_once() {
        let mut key = [0.5; 22];
        assert_eq!(rebake(None, &key, 0), (true, 0));
        let baked = key;
        assert_eq!(rebake(Some(&baked), &key, 0), (false, 0));
        // The sun creeping along: no bake until it has drifted for a while.
        let mut stale = 0;
        for frame in 1..REBAKE_AFTER {
            key[0] = 0.5 + frame as f32 * 1e-5;
            let (bake, next) = rebake(Some(&baked), &key, stale);
            assert!(!bake, "frame {frame}");
            stale = next;
        }
        assert_eq!(rebake(Some(&baked), &key, stale), (true, 0));
        // A visible step (a cloud over the sun, a teleport) bakes at once.
        let mut jump = baked;
        jump[4] = 0.5 + 1.5 * VISIBLE_CHANGE;
        assert_eq!(rebake(Some(&baked), &jump, 0), (true, 0));
        // Large values compare relatively: the camera 1 m higher at 180 m drifts.
        let mut high = baked;
        high[4] = 180.0;
        let mut higher = high;
        higher[4] = 181.0;
        assert_eq!(rebake(Some(&high), &higher, 0), (false, 1));
        higher[4] = 183.0;
        assert_eq!(rebake(Some(&high), &higher, 0), (true, 0));
        let mut nan = baked;
        nan[1] = f32::NAN;
        assert!(rebake(Some(&baked), &nan, 0).0);
    }
}
