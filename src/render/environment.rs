//! Render-pass IBL baking using the existing sky texture binding.
use super::gpu3d::HDR_FORMAT;
use crate::render::traffic::{Counted, Upload};
use crate::three::{EnvironmentMap, probe::RefreshStep};
use std::sync::atomic::{AtomicBool, Ordering};

/// The GPU timer's name for a probe refresh's passes.
const REFRESH: &str = "probe refresh";

/// Timestamp writes for a pass of the given name (the frame's GPU timer).
pub(super) type Timer<'a, 't> = dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>> + 'a;

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
    /// Moves a slot's maps toward its refresh's copy by the blend constant.
    blend: wgpu::RenderPipeline,
    /// Each refreshing slot's own textures (index slot - 1), made when it
    /// first refreshes.
    staging: Vec<Option<Staging>>,
    /// This frame's steps of automatic refreshes (see `RefreshStep`).
    pub refresh: Vec<RefreshStep>,
    /// The six captured faces of this frame's refreshes that lay out their
    /// cube (a `Filter` of face 0), bound for the capture pass.
    refresh_captures: Vec<(u32, Vec<wgpu::BindGroup>)>,
    /// Set until the frame that records the sky bake is drawn: a frame that
    /// is prepared but then skipped (hidden window) must not lose the bake.
    brdf_pending: AtomicBool,
    dirty: AtomicBool,
    map: Option<EnvironmentMap>,
    map_intensity: f32,
    map_dirty: AtomicBool,
    pub bakes: Vec<(u32, [u32; 6])>,
    uploads: Vec<(u32, wgpu::Texture)>,
    /// The sky lighting the image-based light was last baked from.
    key: Option<SkyKey>,
    /// Frames since the sky drifted from `key` by less than a visible step.
    stale: u32,
    /// Frames since the sky was last baked: a sky shader's is baked at most
    /// every `SkyShader::REBUILD_FRAMES`.
    since_bake: u32,
}

/// What the sky's maps are baked from: the sun and sky light (22 values),
/// then whether a sky shader draws the sky, and its id, authored-ambient
/// flag and sixteen parameters.
type SkyKey = [f32; 41];

/// A refreshing probe's own textures: its cube strip, kept over the frames
/// its faces are filtered in, and the copy of its maps that is filtered into
/// and then blended over what the probe shows.
struct Staging {
    strip: wgpu::Texture,
    /// The slot's maps' layout from the atlas, from row 0: 896 x 256.
    maps: wgpu::Texture,
    /// The strip, with the copy's config: for filtering.
    strip_group: wgpu::BindGroup,
    /// The spherical harmonics, with the copy's config: for the diffuse light.
    sh_group: wgpu::BindGroup,
    /// The copy, with its config: for blending it in.
    maps_group: wgpu::BindGroup,
    /// The slot's config, with its maps moved up to row 0.
    config: wgpu::Buffer,
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
fn rebake(baked: Option<&SkyKey>, key: &SkyKey, stale: u32) -> (bool, u32) {
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
/// [`rebake`], on a budget for a sky shader: its parameters may change every
/// frame (a slider, a weather fade), so its maps are baked at most every
/// `SkyShader::REBUILD_FRAMES` frames (`since_bake` counts this one), and a
/// change in between waits for the next allowed frame. A new sky (another
/// shader, or none before, or the ambient light from elsewhere) is baked at
/// once, as the built-in skies always are.
fn budgeted_rebake(baked: Option<&SkyKey>, key: &SkyKey, stale: u32, since_bake: u32) -> (bool, u32) {
    let (bake, next) = rebake(baked, key, stale);
    let shader = key[22] > 0.0;
    // The same shader, with the same source of ambient light.
    let same_sky = baked.is_some_and(|k| k[22..25] == key[22..25]);
    if bake && shader && same_sky && since_bake < crate::three::SkyShader::REBUILD_FRAMES {
        // Waits: the drift count stays as it was, so the bake comes as soon
        // as the budget allows.
        return (false, stale);
    }
    (bake, next)
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
                // Copied from: a refresh starts a new probe from the sky's maps.
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
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
        let blend_add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let blend = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blend"),
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
                entry_point: Some("blend"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: blend_add,
                        alpha: blend_add,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            blend,
            staging: (0..4).map(|_| None).collect(),
            refresh: Vec::new(),
            refresh_captures: Vec::new(),
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
            since_bake: 0,
            map: None,
            map_intensity: 0.0,
            map_dirty: AtomicBool::new(false),
            uploads: Vec::new(),
        }
    }
    /// Stage an authored map in the source strip for the next recorded bake.
    /// The first pass of that frame consumes it before local probe captures
    /// reuse the strip. Until that frame the dirty bit stays set.
    pub fn prepare_map(&mut self, queue: &wgpu::Queue, map: Option<&EnvironmentMap>, intensity: f32) {
        let intensity = if intensity.is_finite() { intensity.max(0.0) } else { 0.0 };
        let Some(map) = map else {
            if self.map.take().is_some() {
                self.dirty.store(true, Ordering::Relaxed);
                self.map_dirty.store(false, Ordering::Relaxed);
                // The sky's settings may have changed while the map was in use.
                self.key = None;
            }
            return;
        };
        if self
            .map
            .as_ref()
            .is_some_and(|old| std::sync::Arc::ptr_eq(&old.0, &map.0))
            && self.map_intensity == intensity
        {
            return;
        }
        self.map = Some(map.clone());
        self.map_intensity = intensity;
        self.map_dirty.store(true, Ordering::Relaxed);
        queue.uploading(Upload::Textures).write_texture(
            self.source.as_image_copy(),
            bytemuck::cast_slice(&map.0),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(768 * 8),
                rows_per_image: Some(128),
            },
            self.source.size(),
        );
    }

    /// Decides whether this frame bakes the sky's maps again, and uploads
    /// the sky's settings. `lighting` is the sun and sky light the maps
    /// depend on; `shader`, a sky shader's id, authored-ambient flag and
    /// parameters. `atmosphere`: the maps come from the sky texture (the
    /// atmosphere or a sky shader), not from the flat colors; then
    /// `authored_ambient` still takes the diffuse light from those colors.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        lighting: [f32; 22],
        shader: Option<[f32; 18]>,
        atmosphere: bool,
        intensity: f32,
        authored_ambient: bool,
        // Sky, ground and equator light (w 1 to use the equator).
        [mut sky, mut ground, equator]: [[f32; 4]; 3],
    ) {
        let mut key = [0.0; 41];
        key[..22].copy_from_slice(&lighting);
        if let Some(shader) = shader {
            key[22] = 1.0;
            key[23..].copy_from_slice(&shader);
        }
        self.since_bake = self.since_bake.saturating_add(1);
        let (bake, stale) = budgeted_rebake(self.key.as_ref(), &key, self.stale, self.since_bake);
        self.stale = stale;
        if bake {
            self.dirty.store(true, Ordering::Relaxed);
            self.key = Some(key);
            self.since_bake = 0;
        }
        // sky.w: the diffuse light from the authored colors (see `sh`).
        let authored_map = self.map.is_some();
        sky[3] = f32::from(authored_ambient && !authored_map);
        ground[3] = f32::from(authored_map);
        let intensity = if authored_map { self.map_intensity } else { intensity };
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
        Self::draw_rects(encoder, target, pipeline, [(group, rect)], 1.0, None);
    }
    /// Draws `pipeline` over each rectangle of `target` with its bind group,
    /// in one pass (timed by `timestamps`), with blend constant `weight`
    /// (for `blend`).
    fn draw_rects<'g>(
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        pipeline: &wgpu::RenderPipeline,
        rects: impl IntoIterator<Item = (&'g wgpu::BindGroup, [u32; 4])>,
        weight: f64,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
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
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_blend_constant(wgpu::Color {
            r: weight,
            g: weight,
            b: weight,
            a: weight,
        });
        // The shaders work from the pixel's position in the target, so
        // each rectangle is only a viewport and scissor of the same pass.
        for (group, rect) in rects {
            pass.set_viewport(rect[0] as f32, rect[1] as f32, rect[2] as f32, rect[3] as f32, 0.0, 1.0);
            pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
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
            if self.map.is_none() {
                Self::draw(encoder, &self.source, &self.cube, &self.sky_group, [0, 0, 768, 128]);
                self.filter(encoder, 0);
            }
        }
        if self.map_dirty.swap(false, Ordering::Relaxed) {
            super::gpu3d::mark_step("sky maps");
            self.filter(encoder, 0);
        }
    }
    /// Bakes the sky's maps again on the next frame drawn (a sky shader
    /// was replaced).
    pub fn invalidate_sky(&mut self) {
        self.key = None;
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
        self.dirty.load(Ordering::Relaxed) || self.map_dirty.load(Ordering::Relaxed)
    }
    /// Whether render target `target` is a probe's face drawn this frame
    /// (lit by the sky alone, and cleared to nothing).
    pub fn is_capture(&self, target: u32) -> bool {
        self.bakes.iter().any(|(_, faces)| faces.contains(&target))
            || self
                .refresh
                .iter()
                .any(|s| matches!(s, RefreshStep::Capture { target: t, .. } if *t == target))
    }
    /// Whether a refresh step of this frame records into the maps.
    fn refreshes_maps(&self) -> bool {
        self.refresh.iter().any(|s| !matches!(s, RefreshStep::Capture { .. }))
    }
    pub fn has_probes(&self) -> bool {
        !self.bakes.is_empty() || !self.uploads.is_empty() || self.refreshes_maps()
    }
    /// Whether [`probes`](Self::probes) records anything: a loaded probe,
    /// a bake whose six faces are all render targets (see
    /// [`prepare_bakes`](Self::prepare_bakes)), or a refresh's step on the maps.
    pub fn records_probes(&self) -> bool {
        !self.uploads.is_empty() || !self.capture_groups.is_empty() || self.refreshes_maps()
    }
    /// The maps' atlas and the cube strip they are filtered from: for
    /// checking them against what the frame's passes declare.
    pub fn images(&self) -> [&wgpu::Texture; 2] {
        [&self.atlas, &self.source]
    }
    /// Binds face `texture` for the capture pass.
    fn capture_group(&self, device: &wgpu::Device, texture: &wgpu::Texture) -> wgpu::BindGroup {
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
    }
    /// The bind groups of faces `faces` among `targets`, if all six are there.
    fn face_groups(
        &self,
        device: &wgpu::Device,
        targets: &[(u32, wgpu::Texture)],
        faces: &[u32; 6],
    ) -> Option<Vec<wgpu::BindGroup>> {
        let groups: Vec<_> = faces
            .iter()
            .filter_map(|id| targets.iter().find(|(t, _)| t == id))
            .map(|(_, texture)| self.capture_group(device, texture))
            .collect();
        (groups.len() == 6).then_some(groups)
    }
    /// Binds each captured face of this frame's bakes, for `probes`.
    pub fn prepare_bakes(&mut self, device: &wgpu::Device, targets: &[(u32, wgpu::Texture)]) {
        self.capture_groups.clear();
        for i in 0..self.bakes.len() {
            let (slot, faces) = self.bakes[i];
            if let Some(groups) = self.face_groups(device, targets, &faces) {
                self.capture_groups.push((slot, groups));
            }
        }
    }
    /// Makes the textures of slots that refresh for the first time, and binds
    /// the faces this frame's refreshes lay out, for `probes`.
    pub fn prepare_refresh(&mut self, device: &wgpu::Device, targets: &[(u32, wgpu::Texture)]) {
        self.refresh_captures.clear();
        for i in 0..self.refresh.len() {
            let slot = match self.refresh[i] {
                RefreshStep::Capture { .. } => continue,
                RefreshStep::Seed(slot)
                | RefreshStep::Harmonics(slot)
                | RefreshStep::Finish(slot)
                | RefreshStep::Blend { slot, .. } => slot,
                RefreshStep::Filter { slot, face, faces } => {
                    if face == 0
                        && let Some(groups) = self.face_groups(device, targets, &faces)
                    {
                        self.refresh_captures.push((slot, groups));
                    }
                    slot
                }
            };
            let index = slot as usize - 1;
            if index < self.staging.len() && self.staging[index].is_none() {
                self.staging[index] = Some(self.make_staging(device, slot));
            }
        }
    }
    /// Slot `slot`'s refresh textures and their bindings.
    fn make_staging(&self, device: &wgpu::Device, slot: u32) -> Staging {
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
        let strip = texture("probe refresh cube", 768, 128);
        let maps = texture("probe refresh maps", 896, 256);
        // The slot's config, with its maps moved up to row 0 (see `maps_row`).
        let config = device
            .uploading(Upload::Uniforms)
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("probe refresh config"),
                contents: bytemuck::cast_slice(&[
                    [slot as f32, 0.0, 1.0, 128.0 + slot as f32 * 256.0],
                    [0.0; 4],
                    [0.0; 4],
                    [0.0; 4],
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let group = |texture: &wgpu::Texture| {
            let view = texture.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("probe refresh"),
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
                        resource: config.as_entire_binding(),
                    },
                ],
            })
        };
        Staging {
            strip_group: group(&strip),
            sh_group: group(&self.sh_texture),
            maps_group: group(&maps),
            strip,
            maps,
            config,
        }
    }
    /// The atlas rows of slot `slot`'s maps: the filtered cube and the
    /// diffuse light.
    fn slot_rects(slot: u32) -> [[u32; 4]; 2] {
        let y = 128 + slot * 256;
        [[0, y, 768, 255], [800, y, 96, 16]]
    }
    /// One refresh step on the maps (see `RefreshStep`), its passes timed
    /// as "probe refresh".
    fn refresh_step<'t>(&self, encoder: &mut wgpu::CommandEncoder, step: RefreshStep, time: &Timer<'_, 't>) {
        let slot = match step {
            RefreshStep::Capture { .. } => return,
            RefreshStep::Seed(slot)
            | RefreshStep::Harmonics(slot)
            | RefreshStep::Finish(slot)
            | RefreshStep::Blend { slot, .. } => slot,
            RefreshStep::Filter { slot, .. } => slot,
        };
        let Some(Some(staging)) = self.staging.get(slot as usize - 1) else {
            return;
        };
        super::gpu3d::mark_step("probes");
        match step {
            RefreshStep::Capture { .. } => {}
            RefreshStep::Seed(slot) => {
                // The sky's maps (slot 0) become the copy, then the content.
                for [x, y, width, height] in Self::slot_rects(0) {
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &self.atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x, y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &staging.maps,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x, y: 0, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                self.blend_in(encoder, staging, slot, 1.0, time);
            }
            RefreshStep::Filter { slot, face, .. } => {
                if face == 0
                    && let Some((_, groups)) = self.refresh_captures.iter().find(|(s, _)| *s == slot)
                {
                    let faces = groups
                        .iter()
                        .enumerate()
                        .map(|(face, group)| (group, [face as u32 * 128, 0, 128, 128]));
                    Self::draw_rects(encoder, &staging.strip, &self.capture, faces, 1.0, time(REFRESH));
                }
                // Every roughness level of one face (see `prefilter`): the
                // level of size `size` starts at row 256 - 2 * size.
                let levels = (0..8).map(|level| {
                    let size = 128 >> level;
                    (&staging.strip_group, [face * size, 256 - 2 * size, size, size])
                });
                Self::draw_rects(encoder, &staging.maps, &self.filter, levels, 1.0, time(REFRESH));
            }
            RefreshStep::Harmonics(slot) => {
                let sh = [(&staging.strip_group, [0, slot, 9 * SH_PARTS, 1])];
                Self::draw_rects(encoder, &self.sh_texture, &self.sh, sh, 1.0, time(REFRESH));
            }
            RefreshStep::Finish(_) => {
                let diffuse = [(&staging.sh_group, [800, 0, 96, 16])];
                Self::draw_rects(encoder, &staging.maps, &self.diffuse, diffuse, 1.0, time(REFRESH));
            }
            RefreshStep::Blend { slot, weight } => self.blend_in(encoder, staging, slot, weight, time),
        }
    }
    /// Moves slot `slot`'s maps `weight` of the way to the refresh's copy.
    fn blend_in<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        staging: &Staging,
        slot: u32,
        weight: f32,
        time: &Timer<'_, 't>,
    ) {
        let rects = Self::slot_rects(slot).map(|rect| (&staging.maps_group, rect));
        Self::draw_rects(
            encoder,
            &self.atlas,
            &self.blend,
            rects,
            f64::from(weight.clamp(0.0, 1.0)),
            time(REFRESH),
        );
    }
    /// Loaded probes, bakes and this frame's refresh steps; the refresh's
    /// passes are timed by `time`.
    pub fn probes<'t>(&self, encoder: &mut wgpu::CommandEncoder, time: &Timer<'_, 't>) {
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
        for &step in &self.refresh {
            self.refresh_step(encoder, step, time);
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
        t.texture(C::Probes, "environment cubemaps", &self.atlas);
        t.texture(C::Probes, "environment source", &self.source);
        t.texture(C::Probes, "environment harmonics", &self.sh_texture);
        for buffer in &self.buffers {
            t.buffer(C::Probes, "environment uniforms", buffer);
        }
        for (_, texture) in &self.uploads {
            t.texture(C::Probes, "environment upload", texture);
        }
        for staging in self.staging.iter().flatten() {
            t.texture(C::Probes, "environment bake strip", &staging.strip);
            t.texture(C::Probes, "environment bake maps", &staging.maps);
            t.buffer(C::Probes, "environment bake config", &staging.config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_sky_drift_rebakes_once_a_second_and_visible_changes_at_once() {
        let mut key = [0.5; 41];
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

    #[test]
    fn a_sky_shaders_changes_bake_on_a_budget_and_a_new_sky_at_once() {
        let budget = crate::three::SkyShader::REBUILD_FRAMES;
        let mut atmosphere = [0.5; 41];
        atmosphere[22..].fill(0.0);
        let mut shader = atmosphere;
        shader[22] = 1.0; // a sky shader, id 0
        // Switching to a sky shader bakes at once, however recent the last bake.
        assert_eq!(budgeted_rebake(Some(&atmosphere), &shader, 0, 1), (true, 0));
        // Its parameters changing every frame: one bake per budget, not per frame.
        let mut baked = shader;
        let mut bakes = 0;
        let mut since = 0;
        let mut stale = 0;
        for frame in 1..=budget * 4 {
            let mut key = shader;
            key[25] = frame as f32; // a parameter sweeping fast
            since += 1;
            let (bake, next) = budgeted_rebake(Some(&baked), &key, stale, since);
            stale = next;
            if bake {
                bakes += 1;
                baked = key;
                since = 0;
            }
        }
        assert_eq!(bakes, 4, "one bake every {budget} frames");
        // Another sky shader is a new sky: at once.
        let mut other = baked;
        other[23] = 1.0;
        assert_eq!(budgeted_rebake(Some(&baked), &other, 0, 1), (true, 0));
        // The built-in skies keep baking at once (negative control).
        let mut sun = atmosphere;
        sun[0] = 0.9;
        assert_eq!(budgeted_rebake(Some(&atmosphere), &sun, 0, 1), (true, 0));
    }
}
