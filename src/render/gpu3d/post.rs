//! Post-processing: what happens to the lit frame after the last model is
//! drawn, in one place.
//!
//! The post part of a frame is a fixed chain ([`POST_CHAIN`]):
//! temporal antialiasing settles the frame into the HDR image, the game's
//! scene compute passes read it, its target effects and screen effects run
//! over it, depth of field blurs what is out of focus, bloom blurs its bright
//! parts, the post pass tone maps it (exposure, bloom, grading), FXAA or
//! SMAA smooth it, and with a render scale a last pass scales it to the
//! window. `Renderer3D::record_post` walks the chain; each link is recorded
//! by its owner: the temporal resolve by `taa.rs`, the scene compute
//! passes by `record.rs`, the effects by `effects.rs`, the rest by [`PostChain`] here.
//!
//! [`PostChain`] owns the pipelines of its links, the post settings
//! uniform, and this frame's choices (the antialiasing mode, whether bloom
//! runs, the lookup table). Its images are [`PostTargets`], made with the
//! scene targets and replaced with them (`targets.rs`).
//!
//! # Adding a post effect
//!
//! A new link (as depth of field was, before bloom) is added here,
//! and declared in the frame's description (`passes.rs`), and nowhere else
//! in the renderer:
//!
//! 1. its pipeline: a field of [`PostChain`], built in [`PostChain::new`]
//!    from an entry point in `post.wgsl` (the `full_screen` helper);
//! 2. its images, if it needs any: a field of [`PostTargets`], made in
//!    [`PostChain::targets`]; if they exist only while the link is on,
//!    [`PostTargets::stale`] says when they must be made again;
//! 3. whether it runs this frame: a field set in [`PostChain::prepare`]
//!    from the game's `Post` settings;
//! 4. its pass: recorded in [`PostChain::record`] where it belongs in the
//!    chain, and named in [`POST_CHAIN`] (a test keeps that list and the
//!    source's order in step);
//! 5. its draw calls, for the frame's statistics: counted in
//!    [`PostChain::draw_calls`];
//! 6. its declaration: a step of the post pass in `passes.rs` (what it
//!    reads and writes, and the feature it is recorded under), marked
//!    where it is recorded (`passes::mark`) and switched on in
//!    [`PostChain::features`]. Debug builds then check every drawn frame
//!    against it. A link with an image of its own also declares that
//!    resource and lists the real texture in `frame_checks.rs`
//!    (`textures`), so its format and sample count are checked too.
//!
//! The renderer can't do any of this for it: the chain's choices and
//! images are private to this file, and the renderer asks only for its
//! stale check, its draw count and (for the frame checks) its features
//! and images (`post_links_render_test.rs` holds the stale check, the
//! draw count and the features for every link switched on and off).
//!
//! It reads the frame from the image the link before it wrote and writes
//! the one the next link reads; nothing in pass preparation, the model
//! passes, the shadow maps or the resources changes. An effect that needs
//! the scene's depth or motion (as GTAO and reflections do) belongs before
//! the lit pass instead, and declares what it reads in `passes.rs`.

use super::{
    BLOOM_MIPS, BLOOM_SCATTER, EffectPass, FrameParts, HDR_FORMAT, PassTimer, Renderer3D, SCENE_COMPUTE_STEPS, Targets,
    for_samples, passes,
};
use crate::render::AntiAliasing;
use crate::render::gpu_textures::TextureTable;
use crate::render::smaa::Smaa;
use crate::render::traffic::{MIRRORED, Mirror, NewBuffer, Upload};
use crate::three::PostUniform;

/// The links of the post chain, in the order they run. Each reads what the
/// one before it left.
/// Documentation that a test holds the recording to.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) const POST_CHAIN: [&str; 10] = [
    "temporal antialiasing",
    "scene compute passes",
    "target effects",
    "screen effects",
    "depth of field",
    "auto exposure",
    "bloom",
    "tone mapping",
    "antialiasing",
    "upscale",
];

/// The post chain's own passes: depth of field, auto exposure, bloom, tone
/// mapping, antialiasing and scaling.
pub(super) struct PostChain {
    /// The format of the image the chain ends in (the window's).
    output_format: wgpu::TextureFormat,
    /// What the pipelines ending in `output_format` are made from, to make
    /// them again when it changes ([`set_output_format`](Self::set_output_format)).
    shader: wgpu::ShaderModule,
    post_pipeline_layout: wgpu::PipelineLayout,
    bloom_pipeline_layout: wgpu::PipelineLayout,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    fxaa: wgpu::RenderPipeline,
    /// Scales the scene-sized LDR frame to the window (see `PostTargets::upscale`).
    upscale: wgpu::RenderPipeline,
    bloom_prefilter: wgpu::RenderPipeline,
    bloom_down: wgpu::RenderPipeline,
    bloom_up: wgpu::RenderPipeline,
    /// Depth of field: built the first time it is on (it reads the scene
    /// depth, declared as the renderer's scene layout says).
    depth_of_field: Option<wgpu::RenderPipeline>,
    /// The post settings as the shader reads them, and what the buffer holds.
    pub(super) buffer: wgpu::Buffer,
    pub(super) mirror: Mirror,
    linear_clamp: wgpu::Sampler,
    /// Bound as the bloom input of the bloom steps themselves.
    pub(super) black: wgpu::TextureView,
    /// This frame's lookup table texture (or white), and whether bloom runs.
    lut: u32,
    bloom_on: bool,
    /// Whether depth of field runs this frame.
    dof_on: bool,
    /// This frame's antialiasing, and SMAA's passes while it is the mode.
    aa: AntiAliasing,
    smaa: Option<Smaa>,
    /// HDR output's headroom over SDR white (`hdr_output.rs`), or `None`
    /// to tone map for SDR.
    pub(super) hdr_headroom: Option<f32>,
    /// Auto exposure: the layout of the adapted exposure's group (group 3
    /// of the tone mapping, bloom prefilter and adaptation passes), the
    /// group read while it is off (0 stops, never read: the shader skips
    /// it), and the pipeline layouts of the meter (the frame and the mask)
    /// and of the passes that read the exposure without a lookup table.
    exposure_layout: wgpu::BindGroupLayout,
    exposure_off: wgpu::BindGroup,
    meter_layout: wgpu::PipelineLayout,
    exposed_layout: wgpu::PipelineLayout,
    /// Auto exposure's passes, built the first time it is on.
    meter_pipelines: Option<MeterPipelines>,
    /// This camera's auto exposure while it is on (each window view has its
    /// own, swapped in: see `views.rs`), and the mask texture it meters
    /// with (the white texture without one).
    metering: Option<Metering>,
    mask: u32,
    white: u32,
}

/// Auto exposure's passes (`post.wgsl`): the meter, its reduction and the
/// adaptation.
struct MeterPipelines {
    meter: wgpu::RenderPipeline,
    reduce: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
}

/// The format of the adapted exposure: the lit frame's, which every device
/// that draws the 3D frame renders to (a 32-bit float target needs another
/// extension on WebGL2). The shader keeps it in two channels, the value
/// rounded to 1/64 of a stop and what that left over (`post.wgsl`'s
/// `exposure_texel`), so a slow adaptation's small steps aren't lost.
pub(super) const EXPOSURE_FORMAT: wgpu::TextureFormat = HDR_FORMAT;

/// One camera's auto exposure: the adapted exposure of the last frame and
/// this one (kept from frame to frame), and the meter's images for its
/// scene's size (made again when it changes).
pub(super) struct Metering {
    /// The two exposures, as targets and as group 3, and which one the
    /// next frame writes (it reads the other).
    states: [wgpu::TextureView; 2],
    state_groups: [wgpu::BindGroup; 2],
    write: usize,
    /// The next frame starts at the target: the first frame, or a reset.
    jump: bool,
    /// The scene size the levels were made for, the levels (an eighth of
    /// the scene, then a quarter of the one before, down to one texel),
    /// and the groups reading each level (for the next, the last for the
    /// adaptation).
    size: (u32, u32),
    levels: Vec<wgpu::TextureView>,
    reads: Vec<wgpu::BindGroup>,
}

impl Metering {
    /// The sizes of the meter's levels for a scene of `size`.
    fn level_sizes(size: (u32, u32)) -> Vec<(u32, u32)> {
        let mut level = (size.0.div_ceil(8).max(1), size.1.div_ceil(8).max(1));
        let mut sizes = vec![level];
        while level != (1, 1) {
            level = (level.0.div_ceil(4), level.1.div_ceil(4));
            sizes.push(level);
        }
        sizes
    }

    /// The first image of the meter and the adapted exposure this frame
    /// writes, for the frame checks.
    pub(super) fn images(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.levels[0], &self.states[self.write])
    }

    /// Counts the meter's levels as frame targets and the two exposures as
    /// other memory (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for view in &self.levels {
            t.view(C::FrameTargets, "auto exposure meter", view);
        }
        for view in &self.states {
            t.view(C::Other, "auto exposure", view);
        }
    }

    /// The adapted exposure written this frame: what bloom and tone
    /// mapping read (test hook).
    #[cfg(test)]
    pub(super) fn state(&self) -> &wgpu::TextureView {
        &self.states[self.write]
    }
}

/// The post chain's images, of the scene's size.
pub(super) struct PostTargets {
    /// The chain's output format when these were made.
    format: wgpu::TextureFormat,
    /// With FXAA: the tone mapped frame before it, and the bind group FXAA
    /// reads it by.
    ldr: Option<wgpu::TextureView>,
    aa_group: Option<wgpu::BindGroup>,
    /// With a render scale other than 1: the tone mapped (and antialiased)
    /// frame at the scene's size, and the bind group the scaling pass reads
    /// it by. The scaling pass then draws it into the window-sized output.
    upscale: Option<wgpu::TextureView>,
    upscale_group: Option<wgpu::BindGroup>,
    /// The post pass's input: the HDR frame and the bloom.
    post: wgpu::BindGroup,
    /// `post` and `bloom_prefilter`, reading the scene texture: for when an
    /// odd number of effects ran and the frame ended up there.
    post_of_scene: wgpu::BindGroup,
    bloom_prefilter_of_scene: wgpu::BindGroup,
    /// Bloom: render target of each mip, and the bind groups of its
    /// steps: the first downsample (reading the HDR frame), the other
    /// downsamples (reading the mip above) and the upsamples (reading the
    /// mip below).
    bloom_mips: Vec<wgpu::TextureView>,
    bloom_prefilter: wgpu::BindGroup,
    bloom_down: Vec<wgpu::BindGroup>,
    bloom_up: Vec<wgpu::BindGroup>,
}

impl PostTargets {
    /// Whether these images were made for other choices than `chain`'s
    /// this frame: a link with an image of its own (FXAA's input, the
    /// scaling pass's) switched on or off. The scene's images are then
    /// made again, these with them.
    pub(super) fn stale(&self, chain: &PostChain, upscaled: bool) -> bool {
        self.ldr.is_some() != (chain.aa == AntiAliasing::Fxaa)
            || self.upscale.is_some() != upscaled
            || self.format != chain.output_format
    }

    /// The first (largest) bloom image, the tone mapped frame FXAA reads
    /// and the scene-sized frame the scaling pass reads: for the frame
    /// checks (`frame_checks.rs`), which compare the real textures with
    /// what the passes declare.
    pub(super) fn bloom_image(&self) -> &wgpu::TextureView {
        &self.bloom_mips[0]
    }

    pub(super) fn ldr(&self) -> Option<&wgpu::TextureView> {
        self.ldr.as_ref()
    }

    pub(super) fn upscaled(&self) -> Option<&wgpu::TextureView> {
        self.upscale.as_ref()
    }

    /// Tests: whether the frame ends in the scaling pass.
    #[cfg(test)]
    pub(super) fn scales_output(&self) -> bool {
        self.upscale.is_some()
    }
}

impl PostChain {
    /// The antialiasing this frame uses (what was asked for, or its
    /// stand-in on this device).
    #[cfg(test)]
    pub(super) fn antialiasing(&self) -> AntiAliasing {
        self.aa
    }

    /// Whether depth of field runs this frame: it reads the scene depth,
    /// which the opaque pass then keeps.
    pub(super) fn depth_of_field_on(&self) -> bool {
        self.dof_on && self.depth_of_field.is_some()
    }

    /// The format of the image the chain ends in.
    pub(super) fn output_format(&self) -> wgpu::TextureFormat {
        self.output_format
    }

    /// Ends the chain in `format` from now on (the window switched to HDR
    /// output's frame image): the passes writing it are made again, SMAA's
    /// with the next frame, and the chain's images go stale
    /// ([`PostTargets::stale`]).
    pub(super) fn set_output_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if format == self.output_format {
            return;
        }
        [self.pipeline, self.fxaa, self.upscale] = output_pipelines(
            device,
            &self.shader,
            (&self.post_pipeline_layout, &self.bloom_pipeline_layout),
            format,
        );
        self.output_format = format;
        self.smaa = None;
    }

    /// Which of the chain's optional links this frame records, as the
    /// frame's description names them (`passes.rs`): the frame checks hold
    /// the recording to it.
    pub(super) fn features(&self, images: &PostTargets) -> passes::Features {
        use passes::Features;
        Features::BLOOM
            .only_if(self.bloom_on)
            .with(Features::DOF.only_if(self.depth_of_field_on()))
            .with(Features::FXAA.only_if(self.aa == AntiAliasing::Fxaa))
            .with(Features::SMAA.only_if(self.aa == AntiAliasing::Smaa))
            .with(Features::UPSCALE.only_if(images.upscale_group.is_some()))
            .with(Features::AUTO_EXPOSURE.only_if(self.metering.is_some()))
    }

    /// This camera's auto exposure, while it is on.
    pub(super) fn metering(&self) -> Option<&Metering> {
        self.metering.as_ref()
    }

    /// Trades this camera's auto exposure with `other`'s, for a window view
    /// (see `views.rs`).
    pub(super) fn swap_metering(&mut self, other: &mut Option<Metering>) {
        std::mem::swap(&mut self.metering, other);
    }

    /// Draw calls the chain's own links take this frame, for the frame's
    /// statistics: depth of field, auto exposure, the post pass, the bloom
    /// steps and the antialiasing passes (the scaling pass isn't counted). `images`: the chain's, once
    /// made.
    pub(super) fn draw_calls(&self, images: Option<&PostTargets>) -> usize {
        let antialiasing = match self.aa {
            AntiAliasing::None | AntiAliasing::Taa => 0,
            AntiAliasing::Fxaa => 1,
            AntiAliasing::Smaa => 3,
        };
        let bloom = if self.bloom_on {
            images.map_or(0, |t| t.bloom_mips.len() * 2 - 1)
        } else {
            0
        };
        let metering = self.metering.as_ref().map_or(0, |m| m.levels.len() + 1);
        1 + antialiasing + bloom + metering + usize::from(self.depth_of_field_on())
    }

    pub(super) fn new(
        device: &wgpu::Device,
        texture_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        white: u32,
    ) -> Self {
        let uniform_entry = |visibility| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let post_shader = device.create_shader_module(wgpu::include_wgsl!("../post.wgsl"));
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d post layout"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    ..uniform_entry(wgpu::ShaderStages::FRAGMENT)
                },
                texture_entry(3),
            ],
        });
        // Auto exposure's adapted exposure: one texel, read with
        // `textureLoad`.
        let exposure_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d exposure layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let exposure_off = exposure_group(device, &exposure_layout, &exposure_target(device));
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d post pipeline layout"),
            bind_group_layouts: &[Some(&post_layout), Some(texture_layout), None, Some(&exposure_layout)],
            immediate_size: 0,
        });
        let exposed_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d exposed pipeline layout"),
            bind_group_layouts: &[Some(&post_layout), None, None, Some(&exposure_layout)],
            immediate_size: 0,
        });
        let meter_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d auto exposure meter layout"),
            bind_group_layouts: &[Some(&post_layout), Some(texture_layout)],
            immediate_size: 0,
        });
        let bloom_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d bloom pipeline layout"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        let full_screen = |label, layout, entry_point, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &post_shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &post_shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let [post_pipeline, fxaa_pipeline, upscale_pipeline] = output_pipelines(
            device,
            &post_shader,
            (&post_pipeline_layout, &bloom_pipeline_layout),
            format,
        );
        let bloom_prefilter = full_screen(
            "3d bloom prefilter",
            &exposed_layout,
            "fs_bloom_prefilter",
            HDR_FORMAT,
            None,
        );
        let bloom_down = full_screen(
            "3d bloom down",
            &bloom_pipeline_layout,
            "fs_bloom_down",
            HDR_FORMAT,
            None,
        );
        // Each upsample is blended over the mip's own downsample:
        // lerp(mip, upsampled, scatter), the scatter set as blend constant.
        let lerp = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let bloom_up = full_screen(
            "bloom",
            &bloom_pipeline_layout,
            "fs_bloom_up",
            HDR_FORMAT,
            Some(wgpu::BlendState {
                color: lerp,
                alpha: lerp,
            }),
        );
        let post_buffer = device.new_buffer(&wgpu::BufferDescriptor {
            label: Some("3d post"),
            size: size_of::<PostUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | MIRRORED,
            mapped_at_creation: false,
        });
        let linear_clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("3d linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let black = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("3d black"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            output_format: format,
            shader: post_shader,
            post_pipeline_layout,
            bloom_pipeline_layout,
            layout: post_layout,
            pipeline: post_pipeline,
            fxaa: fxaa_pipeline,
            upscale: upscale_pipeline,
            bloom_prefilter,
            bloom_down,
            bloom_up,
            depth_of_field: None,
            buffer: post_buffer,
            mirror: Mirror::default(),
            linear_clamp,
            black,
            lut: white,
            bloom_on: false,
            dof_on: false,
            aa: AntiAliasing::None,
            smaa: None,
            hdr_headroom: None,
            exposure_layout,
            exposure_off,
            meter_layout,
            exposed_layout,
            meter_pipelines: None,
            metering: None,
            mask: white,
            white,
        }
    }

    /// This frame's post settings: the uniform, the lookup table, whether
    /// bloom and depth of field run, auto exposure, and the antialiasing
    /// (temporal antialiasing needs compute-class devices; the others get
    /// FXAA). `camera`: its near and far planes. `clock`: the seconds since
    /// this camera's last frame, and whether the game asked to forget the
    /// past frames (auto exposure then starts at its target).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        post: &crate::render::Post,
        lut: Option<(u32, u32)>,
        (size, window): ((u32, u32), (u32, u32)),
        camera: (f32, f32),
        white: u32,
        taa_supported: bool,
        (dt, reset): (f32, bool),
    ) {
        let jump = self.prepare_auto_exposure(device, post, size, reset);
        let post_uniform = PostUniform::new(post, lut.map(|(_, n)| n))
            .scaled(post, size, window)
            .camera(camera.0, camera.1)
            .hdr(self.hdr_headroom)
            .adapt(post, dt, jump);
        let post_uniform = bytemuck::bytes_of(&post_uniform);
        self.mirror
            .upload(queue, Upload::Uniforms, &self.buffer, 0, post_uniform);
        self.lut = lut.map_or(white, |(id, _)| id);
        self.bloom_on = post.bloom > 0.0;
        self.dof_on = post.depth_of_field.is_on();
        self.aa = match post.anti_aliasing {
            AntiAliasing::Taa if !taa_supported => AntiAliasing::Fxaa,
            aa => aa,
        };
    }

    /// Auto exposure for this frame: its passes, made the first time it is
    /// on, and this camera's exposures and meter (the meter made again for
    /// a new scene `size`), or nothing while it is off (its images are
    /// freed). Returns whether the frame jumps to the target: the first
    /// with it on, or after a `reset`.
    fn prepare_auto_exposure(
        &mut self,
        device: &wgpu::Device,
        post: &crate::render::Post,
        size: (u32, u32),
        reset: bool,
    ) -> bool {
        let auto = &post.auto_exposure;
        if !auto.enabled {
            self.metering = None;
            return false;
        }
        self.mask = auto.mask.map_or(self.white, |t| t.id);
        if self.meter_pipelines.is_none() {
            self.meter_pipelines = Some(self.meter_pipelines(device));
        }
        let metering = self.metering.get_or_insert_with(|| {
            let states = [exposure_target(device), exposure_target(device)];
            let state_groups = [
                exposure_group(device, &self.exposure_layout, &states[0]),
                exposure_group(device, &self.exposure_layout, &states[1]),
            ];
            Metering {
                states,
                state_groups,
                write: 1,
                jump: true,
                size: (0, 0),
                levels: Vec::new(),
                reads: Vec::new(),
            }
        });
        metering.write ^= 1;
        if metering.size != size {
            let sizes = Metering::level_sizes(size);
            let levels: Vec<wgpu::TextureView> = sizes
                .iter()
                .map(|&(width, height)| {
                    device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some("3d auto exposure meter"),
                            size: wgpu::Extent3d {
                                width,
                                height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: HDR_FORMAT,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                })
                .collect();
            metering.reads = levels
                .iter()
                .map(|level| {
                    post_group(
                        device,
                        &self.layout,
                        level,
                        &self.linear_clamp,
                        &self.buffer,
                        &self.black,
                    )
                })
                .collect();
            metering.levels = levels;
            metering.size = size;
        }
        std::mem::replace(&mut metering.jump, false) || reset
    }

    /// Auto exposure's passes (`post.wgsl`).
    fn meter_pipelines(&self, device: &wgpu::Device) -> MeterPipelines {
        let make = |label, layout, entry_point, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        MeterPipelines {
            meter: make("3d auto exposure meter", &self.meter_layout, "fs_meter", HDR_FORMAT),
            reduce: make(
                "3d auto exposure reduce",
                &self.bloom_pipeline_layout,
                "fs_meter_reduce",
                HDR_FORMAT,
            ),
            adapt: make("3d auto exposure", &self.exposed_layout, "fs_adapt", EXPOSURE_FORMAT),
        }
    }

    /// Depth of field's pipeline, built the first time it is on. It reads
    /// the scene depth through the renderer's scene layout (`scene_layout`,
    /// for a scene of `samples` per pixel, read as a float texture with
    /// `float_depth`), which a change of multisampling keeps.
    pub(super) fn prepare_depth_of_field(
        &mut self,
        device: &wgpu::Device,
        scene_layout: &wgpu::BindGroupLayout,
        samples: u32,
        float_depth: bool,
    ) {
        if !self.dof_on || self.depth_of_field.is_some() {
            return;
        }
        let source = format!("{}\n{}", include_str!("../post.wgsl"), include_str!("../dof.wgsl"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d depth of field"),
            source: wgpu::ShaderSource::Wgsl(for_samples(&source, samples, float_depth)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d depth of field layout"),
            bind_group_layouts: &[Some(&self.layout), None, Some(scene_layout)],
            immediate_size: 0,
        });
        self.depth_of_field = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d depth of field"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_depth_of_field"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        }));
    }

    /// Records depth of field when it is on: it reads the frame from where
    /// the screen effects left it (`in_hdr`: the HDR image, or else the
    /// scene texture) and its depth, and writes the other one. Returns
    /// where the frame is now, as `in_hdr` says it.
    pub(super) fn record_depth_of_field<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        in_hdr: bool,
        time: &PassTimer<'_, 't>,
    ) -> bool {
        let (true, Some(pipeline)) = (self.dof_on, &self.depth_of_field) else {
            return in_hdr;
        };
        // The scene group reads the depth; its color is the image read,
        // never the one written.
        let (post, scene, target) = if in_hdr {
            passes::mark("depth of field");
            (&targets.post.post, &targets.scene_of_hdr, &targets.scene_view)
        } else {
            passes::mark("depth of field back");
            (&targets.post.post_of_scene, &targets.scene, &targets.hdr)
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d depth of field"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: time("depth of field"),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, post, &[]);
        pass.set_bind_group(2, scene, &[]);
        pass.draw(0..3, 0..1);
        !in_hdr
    }

    /// SMAA's passes for a scene of `size`: made when it becomes the mode
    /// or the size changes (`made_for`: the size of the targets so far),
    /// dropped when it isn't the mode.
    pub(super) fn prepare_smaa(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        made_for: Option<(u32, u32)>,
    ) {
        if made_for.is_some_and(|s| s != size) || self.aa != AntiAliasing::Smaa {
            self.smaa = None;
        }
        if self.aa == AntiAliasing::Smaa && (self.smaa.is_none() || made_for.is_none_or(|s| s != size)) {
            self.smaa = Some(Smaa::new(device, queue, size.0, size.1, self.output_format));
        }
    }

    /// Trades SMAA's passes (sized for one view) with `other`'s, for a
    /// window view (see `views.rs`).
    pub(super) fn swap_smaa(&mut self, other: &mut Option<Smaa>) {
        std::mem::swap(&mut self.smaa, other);
    }

    /// The chain's images for a scene of `size`, reading the frame from
    /// `hdr` or (after an odd number of screen effects) `scene_view`.
    pub(super) fn targets(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        upscaled: bool,
        hdr: &wgpu::TextureView,
        scene_view: &wgpu::TextureView,
    ) -> PostTargets {
        let target = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        // Bloom starts at half size and halves until about 8 pixels.
        let half = (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1));
        let mips = (32 - half.0.min(half.1).max(1).leading_zeros())
            .saturating_sub(3)
            .clamp(1, BLOOM_MIPS);
        let bloom = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d bloom"),
            size: wgpu::Extent3d {
                width: half.0,
                height: half.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let bloom_mips: Vec<wgpu::TextureView> = (0..mips)
            .map(|mip| {
                bloom.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let group = |source: &wgpu::TextureView, bloom: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("3d post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.linear_clamp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(bloom),
                    },
                ],
            })
        };
        let ldr = (self.aa == AntiAliasing::Fxaa).then(|| target("3d LDR before AA", self.output_format));
        let aa_group = ldr.as_ref().map(|ldr| group(ldr, &self.black));
        let upscale = upscaled.then(|| target("3d scaled frame", self.output_format));
        let upscale_group = upscale.as_ref().map(|u| group(u, &self.black));
        let n = bloom_mips.len();
        PostTargets {
            format: self.output_format,
            ldr,
            aa_group,
            upscale,
            upscale_group,
            post: group(hdr, &bloom_mips[0]),
            bloom_prefilter: group(hdr, &self.black),
            post_of_scene: group(scene_view, &bloom_mips[0]),
            bloom_prefilter_of_scene: group(scene_view, &self.black),
            bloom_down: (1..n).map(|i| group(&bloom_mips[i - 1], &self.black)).collect(),
            bloom_up: (0..n - 1).map(|i| group(&bloom_mips[i + 1], &self.black)).collect(),
            bloom_mips,
        }
    }

    /// Records the chain's own links: bloom, tone mapping, antialiasing and
    /// the scaling pass, ending in `view`. `in_hdr`: whether the frame is in
    /// the HDR image (or, after an odd number of screen effects, the scene
    /// texture). `temporal`: the temporal upscaler, its histories and the
    /// one it reads, when it scales the frame instead of the stretch.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        images: &PostTargets,
        in_hdr: bool,
        view: &wgpu::TextureView,
        textures: &dyn TextureTable,
        time: &PassTimer<'_, 't>,
        temporal: Option<(&crate::render::taa::Taa, &crate::render::taa::UpscaleTargets, usize)>,
    ) {
        let (post, bloom_prefilter) = if in_hdr {
            (&images.post, &images.bloom_prefilter)
        } else {
            (&images.post_of_scene, &images.bloom_prefilter_of_scene)
        };
        // Auto exposure meters the frame before bloom: bloom's threshold and
        // the tone mapping see the exposed light.
        let exposure = self.record_auto_exposure(encoder, post, textures, time);
        if self.bloom_on {
            passes::mark("bloom");
            let step = |encoder: &mut wgpu::CommandEncoder,
                        label: &'static str,
                        pipeline,
                        group: &wgpu::BindGroup,
                        target: &wgpu::TextureView,
                        load| {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: time(label),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
                if std::ptr::eq(pipeline, &self.bloom_prefilter) {
                    pass.set_bind_group(3, exposure, &[]);
                }
                pass.set_blend_constant(wgpu::Color {
                    r: BLOOM_SCATTER,
                    g: BLOOM_SCATTER,
                    b: BLOOM_SCATTER,
                    a: BLOOM_SCATTER,
                });
                pass.draw(0..3, 0..1);
            };
            let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
            let mips = &images.bloom_mips;
            step(
                encoder,
                "bloom",
                &self.bloom_prefilter,
                bloom_prefilter,
                &mips[0],
                clear,
            );
            for (i, group) in images.bloom_down.iter().enumerate() {
                step(encoder, "bloom", &self.bloom_down, group, &mips[i + 1], clear);
            }
            for (i, group) in images.bloom_up.iter().enumerate().rev() {
                step(encoder, "bloom", &self.bloom_up, group, &mips[i], wgpu::LoadOp::Load);
            }
        }
        // With a render scale, everything up to antialiasing is drawn
        // at the scene's size, then scaled onto the window.
        let output = images.upscale.as_ref().unwrap_or(view);
        passes::mark("tone map");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: match self.aa {
                    crate::render::AntiAliasing::None | crate::render::AntiAliasing::Taa => output,
                    crate::render::AntiAliasing::Fxaa => images.ldr.as_ref().unwrap(),
                    crate::render::AntiAliasing::Smaa => self.smaa.as_ref().unwrap().input(),
                },
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: time("post"),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, post, &[]);
        let lut = textures
            .texture(self.lut)
            .expect("a live lookup table (checked in prepare)");
        pass.set_bind_group(1, &lut.bind_group, &[]);
        pass.set_bind_group(3, exposure, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        match self.aa {
            crate::render::AntiAliasing::None | crate::render::AntiAliasing::Taa => {}
            crate::render::AntiAliasing::Smaa => {
                passes::mark("smaa");
                self.smaa.as_ref().unwrap().draw(encoder, output);
            }
            crate::render::AntiAliasing::Fxaa => {
                passes::mark("fxaa");
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("FXAA"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: output,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.fxaa);
                pass.set_bind_group(0, images.aa_group.as_ref().unwrap(), &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if let (Some((taa, histories, read)), Some(_)) = (temporal, &images.upscale_group) {
            passes::mark("temporal upscale");
            taa.draw_upscale(encoder, histories, read, view, time("temporal upscale"));
        } else if let Some(group) = &images.upscale_group {
            passes::mark("upscale");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: time("upscale"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.upscale);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

impl PostChain {
    /// Records auto exposure while it is on: the meter of the frame (read
    /// through `post`, the tone mapping's group), its reduction to one
    /// texel and the adaptation. Returns the group bloom and tone mapping
    /// read the exposure from (a stand-in they skip while it is off).
    fn record_auto_exposure<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        post: &wgpu::BindGroup,
        textures: &dyn TextureTable,
        time: &PassTimer<'_, 't>,
    ) -> &wgpu::BindGroup {
        let (Some(metering), Some(pipelines)) = (&self.metering, &self.meter_pipelines) else {
            return &self.exposure_off;
        };
        let Some(mask) = textures.texture(self.mask).or_else(|| textures.texture(self.white)) else {
            return &self.exposure_off;
        };
        passes::mark("auto exposure");
        let pass = |encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView| {
            encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("3d auto exposure"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: time("auto exposure"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime()
        };
        {
            let mut meter = pass(encoder, &metering.levels[0]);
            meter.set_pipeline(&pipelines.meter);
            meter.set_bind_group(0, post, &[]);
            meter.set_bind_group(1, &mask.bind_group, &[]);
            meter.draw(0..3, 0..1);
        }
        for (level, read) in metering.levels[1..].iter().zip(&metering.reads) {
            let mut reduce = pass(encoder, level);
            reduce.set_pipeline(&pipelines.reduce);
            reduce.set_bind_group(0, read, &[]);
            reduce.draw(0..3, 0..1);
        }
        let write = metering.write;
        let mut adapt = pass(encoder, &metering.states[write]);
        adapt.set_pipeline(&pipelines.adapt);
        adapt.set_bind_group(0, &metering.reads[metering.levels.len() - 1], &[]);
        adapt.set_bind_group(3, &metering.state_groups[write ^ 1], &[]);
        adapt.draw(0..3, 0..1);
        &metering.state_groups[write]
    }
}

/// A one-texel image for an adapted exposure, cleared to 0 stops.
fn exposure_target(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("3d auto exposure"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXPOSURE_FORMAT,
            // Tests read it back.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | if cfg!(test) {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Group 3 of the passes that read an adapted exposure.
fn exposure_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, state: &wgpu::TextureView) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("3d auto exposure"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(state),
        }],
    })
}

/// A post group (`post.wgsl`'s group 0) reading `source`.
fn post_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
    bloom: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("3d post"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(bloom),
            },
        ],
    })
}

impl Renderer3D {
    /// The post part of the frame: walks [`POST_CHAIN`], each link recorded
    /// by its owner.
    pub(super) fn record_post(&self, encoder: &mut wgpu::CommandEncoder, frame: &FrameParts<'_, '_>) {
        let FrameParts {
            view,
            textures,
            targets,
            time,
            ..
        } = *frame;
        // Temporal antialiasing first: effects and bloom then see the
        // settled image, in `hdr`.
        if self.screen.taa_on
            && let (Some(taa), Some(images)) = (&self.screen.taa, &targets.taa)
        {
            passes::mark("taa");
            taa.draw(encoder, images, self.history.taa_frame_read, &targets.hdr, time("taa"));
        }
        // GPU buffers that keep their last frame copy what this frame drew,
        // for the next frame's motion pass, before anything changes them.
        if self.screen.velocity_on && self.resources.keeps_buffer_pasts() && !self.extra {
            let timestamps = time("buffer motion copy").map(|t| wgpu::ComputePassTimestampWrites {
                query_set: t.query_set,
                beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                end_of_pass_write_index: t.end_of_pass_write_index,
            });
            self.resources.record_buffer_pasts(encoder, timestamps);
        }
        // The game's scene compute passes: they read the settled frame
        // (in `hdr`) and its depth, and write their own targets or buffers,
        // so the effects below see this frame's results.
        self.record_computes(
            encoder,
            &self.lists.scene_computes,
            &targets.scene_of_hdr,
            time,
            SCENE_COMPUTE_STEPS,
        );
        let ctx = EffectPass {
            frame: &self.frame_bind_group,
            resources: &self.resources,
            storage_dummy: self.resources.storage_dummy.as_ref(),
            targets,
            time,
        };
        self.effects.record_targets(encoder, &ctx);
        let in_hdr = self.effects.record_screen(encoder, &ctx);
        let in_hdr = self.post.record_depth_of_field(encoder, targets, in_hdr, time);
        let temporal = self
            .screen
            .taa
            .as_ref()
            .filter(|_| self.screen.upscale_on)
            .zip(targets.upscaler.as_ref())
            .map(|(taa, histories)| (taa, histories, self.history.taa_frame_read));
        self.post
            .record(encoder, &targets.post, in_hdr, view, textures, time, temporal);
    }
}

impl PostChain {
    /// Counts SMAA's targets as frame targets, and the chain's uniform and
    /// black stand-in as other memory (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        if let Some(smaa) = &self.smaa {
            smaa.tally_memory(t);
        }
        if let Some(metering) = &self.metering {
            metering.tally_memory(t);
        }
        t.buffer(C::Other, "post uniforms", &self.buffer);
        t.view(C::Other, "post black stand-in", &self.black);
    }
}

#[cfg(test)]
impl Renderer3D {
    /// The exposure auto exposure adapted to in the last frame drawn, in
    /// stops added to `Post::exposure`: the main camera's (`view` `None`)
    /// or window view `view`'s. `None` while it is off (test hook).
    pub(crate) fn auto_exposure_stops(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: Option<usize>,
    ) -> Option<f32> {
        let metering = match view {
            None => self.post.metering(),
            Some(index) => self.view_metering(index),
        }?;
        let bytes = crate::render::render_test_support::read_texture(device, queue, metering.state().texture());
        // The rounded value and what it left over (`exposure_texel`).
        let half = |at: usize| {
            let bits = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
            let (exp, frac) = (i32::from((bits >> 10) & 31), f32::from(bits & 1023));
            let v = if exp == 0 {
                frac * 2f32.powi(-24)
            } else {
                (1.0 + frac / 1024.0) * 2f32.powi(exp - 15)
            };
            if bits >> 15 == 1 { -v } else { v }
        };
        Some(half(0) + half(2))
    }

    /// The sizes of the auto exposure meter's images, while it is on (test
    /// hook).
    pub(crate) fn auto_exposure_meter_sizes(&self) -> Option<Vec<(u32, u32)>> {
        self.post.metering().map(|m| {
            m.levels
                .iter()
                .map(|l| (l.texture().width(), l.texture().height()))
                .collect()
        })
    }
}

impl PostTargets {
    /// Counts bloom's levels and the antialiasing and scaling passes'
    /// inputs as frame targets (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        for view in &self.bloom_mips {
            t.view(C::FrameTargets, "3d bloom", view);
        }
        if let Some(view) = &self.ldr {
            t.view(C::FrameTargets, "post ldr", view);
        }
        if let Some(view) = &self.upscale {
            t.view(C::FrameTargets, "3d upscale", view);
        }
    }
}

/// The passes writing the chain's output in `format`: tone mapping, FXAA
/// and scaling (`post.wgsl`), with the post and bloom pipeline layouts.
fn output_pipelines(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    (post_layout, bloom_layout): (&wgpu::PipelineLayout, &wgpu::PipelineLayout),
    format: wgpu::TextureFormat,
) -> [wgpu::RenderPipeline; 3] {
    let make = |label, layout, entry_point| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    [
        make("3d post pipeline", post_layout, "fs_tonemap"),
        make("FXAA", bloom_layout, "fs_fxaa"),
        make("3d upscale", bloom_layout, "fs_upscale"),
    ]
}

#[cfg(test)]
mod tests {
    use super::POST_CHAIN;

    /// What records each link of [`POST_CHAIN`]: `(link, the function it is
    /// recorded from, the text that records it)`.
    const RECORDED: [(&str, &str, &str); 10] = [
        ("temporal antialiasing", "fn record_post(", "time(\"taa\")"),
        ("scene compute passes", "fn record_post(", "SCENE_COMPUTE_STEPS"),
        ("target effects", "fn record_post(", ".record_targets("),
        ("screen effects", "fn record_post(", ".record_screen("),
        ("depth of field", "fn record_post(", ".record_depth_of_field("),
        ("auto exposure", "fn record<'t>(", ".record_auto_exposure("),
        ("bloom", "fn record<'t>(", "\"bloom\","),
        ("tone mapping", "fn record<'t>(", "time(\"post\")"),
        ("antialiasing", "fn record<'t>(", "Some(\"FXAA\")"),
        ("upscale", "fn record<'t>(", "time(\"upscale\")"),
    ];

    /// This file without its tests, with `\n` line ends (a Windows checkout
    /// may have `\r\n`).
    fn source() -> String {
        let all = include_str!("post.rs").replace("\r\n", "\n");
        all[..all.find("\n#[cfg(test)]\nmod tests").unwrap()].to_string()
    }

    /// Keeps `POST_CHAIN` (the documented order) and this file's source in
    /// step: the text that records each link appears, in the function that
    /// records it, in the list's order. It reads the source, not a recorded
    /// frame; the frame's real pass order is checked by `passes.rs` and the
    /// image tests.
    #[test]
    fn the_post_chain_is_recorded_in_its_documented_order() {
        let source = &source();
        assert_eq!(POST_CHAIN.map(|link| link), RECORDED.map(|(link, ..)| link));
        // `record_post` walks the chain and ends in the chain's own links.
        let walk = &source[source.find("fn record_post(").unwrap()..];
        let walk: String = walk.split_whitespace().collect();
        assert!(walk.contains("self.post.record("), "the chain's own links come last");
        let mut last = ("", 0);
        for (link, function, text) in RECORDED {
            let body = &source[source.find(function).unwrap()..];
            let body = &body[..body.find("\n    }\n").unwrap()];
            let at = body
                .find(text)
                .unwrap_or_else(|| panic!("{link} is not recorded by `{function}`"));
            if last.0 == function {
                assert!(at > last.1, "{link} is recorded before the link ahead of it");
            }
            last = (function, at);
        }
    }
}
