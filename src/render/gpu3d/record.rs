//! Pass encoding: turning a prepared frame into GPU commands.
//!
//! `Renderer3D::draw` lists the frame's parts ([`Part`], in the order
//! `passes.rs` declares valid) and records each into a command buffer of
//! its own, on several cores. Recording takes `&self`: everything it reads
//! was decided by `prepare` (`prepare.rs`), and nothing here changes what a
//! later frame draws. The few counters it bumps are atomics.
//!
//! One part is one arm of `Renderer3D::record`. The model passes share
//! `draw_model_runs`, which looks each mesh part's pipeline up by shader,
//! variant and role ([`Kind`] says which pass is drawing). The post part's
//! passes are recorded by the post chain (`post.rs`).

use super::*;

// Native command recording uses rayon; browser GPU handles stay on one thread.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type PassTimer<'a, 't> = dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>> + Sync + 'a;
#[cfg(target_arch = "wasm32")]
pub(super) type PassTimer<'a, 't> = dyn Fn(&'static str) -> Option<wgpu::RenderPassTimestampWrites<'t>> + 'a;

/// The names a stage of compute passes records under: its steps in
/// `passes.rs` (for GPU buffers, for textures) and its GPU timer.
#[derive(Clone, Copy)]
pub(super) struct ComputeSteps {
    pub(super) buffer: &'static str,
    pub(super) texture: &'static str,
    pub(super) timer: &'static str,
}

/// `RenderPasses::compute`, in the prelude.
pub(super) const COMPUTE_STEPS: ComputeSteps = ComputeSteps {
    buffer: "buffer compute",
    texture: "texture compute",
    timer: "compute",
};

/// `RenderPasses::scene_compute`, in the post part.
pub(super) const SCENE_COMPUTE_STEPS: ComputeSteps = ComputeSteps {
    buffer: "scene buffer compute",
    texture: "scene texture compute",
    timer: "scene compute",
};

/// A part of a frame, recorded into a command buffer of its own (see
/// `Renderer3D::draw`).
#[derive(Clone, Copy, Debug)]
pub(super) enum Part {
    /// Compute passes and the sky texture.
    Prelude,
    /// A shadow cascade's runs `first..end`: big cascades are drawn in
    /// several passes (the first clears the map), recorded on several cores.
    Shadow {
        cascade: usize,
        first: u32,
        end: u32,
    },
    /// Light views `first..=last` (shadow view indices), each drawn whole
    /// into its tile of the atlas, in one pass: small views share a pass
    /// instead of starting one each. Views whose tiles are kept are skipped.
    LightViews {
        first: usize,
        last: usize,
    },
    /// Every target camera.
    Views,
    /// Global illumination's probe views and their update.
    Gi,
    Prepass,
    /// The opaque pass, with the transparent surfaces when none reads the scene.
    Opaque,
    /// A copy of the opaque scene, then the transparent surfaces.
    Transparent,
    /// Screen effects, bloom and tone mapping.
    Post,
}

impl Part {
    /// Its pass in [`passes::FRAME_PASSES`]: what it reads and writes.
    pub(super) fn pass(self) -> usize {
        match self {
            Part::Prelude => passes::PRELUDE,
            Part::Shadow { .. } | Part::LightViews { .. } => passes::SHADOW,
            Part::Views => passes::VIEWS,
            Part::Gi => passes::GI,
            Part::Prepass => passes::PREPASS,
            Part::Opaque => passes::OPAQUE,
            Part::Transparent => passes::TRANSPARENT,
            Part::Post => passes::POST,
        }
    }

    pub(super) fn label(self) -> &'static str {
        passes::FRAME_PASSES[self.pass()].name
    }
}

/// What every part of a frame is recorded with.
pub(super) struct FrameParts<'a, 't> {
    pub(super) view: &'a wgpu::TextureView,
    pub(super) clear: wgpu::Color,
    pub(super) textures: &'a dyn TextureTable,
    pub(super) targets: &'a Targets,
    pub(super) split: bool,
    pub(super) time: &'a PassTimer<'a, 't>,
    /// What the parts are checked against in debug builds (see `passes`).
    pub(super) checks: frame_checks::FrameChecks,
}

/// Draws per shadow pass before a cascade is split into more passes, each
/// recorded on its own core. A pass costs little GPU time on a depth-only
/// map; recording a few hundred draws costs a fraction of a millisecond.
pub(super) const SHADOW_DRAWS_PER_PASS: usize = 300;

/// What starting a pass costs to record, in draws (see `Renderer3D::part_work`).
const PASS_WORK: usize = 8;

/// The recording work, in draws, worth a command encoder of its own (see
/// [`encoder_groups`]). An encoder costs wgpu a command buffer and resource
/// trackers sized to every resource the device holds (a few dozen heap
/// allocations each); below this much work, recording on another core
/// doesn't pay for it.
pub(super) const ENCODER_WORK: usize = 250;

/// The most encoders a frame records into, on as many cores.
pub(super) const MAX_ENCODERS: usize = 8;

/// A frame's command buffers (see `Renderer3D::draw`): never more than
/// [`MAX_ENCODERS`], so kept inline.
pub(crate) type FrameBuffers = smallvec::SmallVec<[wgpu::CommandBuffer; MAX_ENCODERS]>;

/// Splits `parts` (recorded in this order) into consecutive groups, each
/// recorded into one encoder: as many groups as the frame has
/// [`ENCODER_WORK`] (at least one, at most [`MAX_ENCODERS`] and `cores`),
/// each with about the same work (`work` estimates a part's). Gives each
/// group's end in `parts` and the number of groups. A small frame (a
/// minimap's, a simple scene's) is one encoder.
pub(super) fn encoder_groups(
    parts: &[Part],
    work: impl Fn(&Part) -> usize,
    cores: usize,
) -> ([usize; MAX_ENCODERS], usize) {
    let mut ends = [parts.len(); MAX_ENCODERS];
    let total: usize = parts.iter().map(&work).sum();
    let groups = total
        .div_ceil(ENCODER_WORK)
        .clamp(1, MAX_ENCODERS.min(cores.max(1)).min(parts.len().max(1)));
    // Close a group once it holds its share of what is left.
    let (mut group, mut done, mut held) = (0, 0, 0);
    for (i, part) in parts.iter().enumerate() {
        let w = work(part);
        let left = groups - group;
        if left > 1 && held > 0 && held + w / 2 > (total - done).div_ceil(left) {
            ends[group] = i;
            group += 1;
            done += held;
            held = 0;
        }
        held += w;
    }
    ends[group] = parts.len();
    (ends, group + 1)
}

/// GPU timer names of the shadow cascades' passes.
pub(super) const SHADOW_PASSES: [&str; 8] = [
    "shadows 0",
    "shadows 1",
    "shadows 2",
    "shadows 3",
    "shadows 4",
    "shadows 5",
    "shadows 6",
    "shadows 7",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Shadow(usize),
    Prepass,
    /// Cut-out materials' prepass with alpha to coverage (multisampling).
    PrepassCoverage,
    Opaque,
    Transparent,
    /// Motion vectors of the models that moved (temporal antialiasing).
    Velocity,
    /// Motion vectors of the models a shader's `vertex` moves while their
    /// matrix holds still, from the frame's own instances (drawn first;
    /// `Velocity` draws the ones that also moved over them).
    VelocityStill,
    /// Global illumination's probe views: lit, solid surfaces writing
    /// their own depth (cut-outs went through `PrepassCoverage`).
    Capture,
    /// The view model's held items (`view_model.rs`): lit, solid and
    /// cut-out surfaces writing their own depth at the end of the opaque
    /// pass.
    ViewModel,
    /// The view model's see-through held items: as `Transparent`, but
    /// always writing the stencil, for they draw where it can be written.
    ViewModelBlended,
}

impl Kind {
    pub(super) fn pass(self) -> Pass {
        match self {
            Kind::Shadow(_) => Pass::Shadow,
            Kind::Prepass => Pass::Prepass,
            Kind::PrepassCoverage => Pass::PrepassCoverage,
            Kind::Opaque => Pass::Opaque,
            Kind::Transparent => Pass::Transparent,
            Kind::Velocity | Kind::VelocityStill => Pass::Velocity,
            Kind::Capture => Pass::Capture,
            Kind::ViewModel => Pass::Opaque,
            Kind::ViewModelBlended => Pass::Transparent,
        }
    }
}

impl Renderer3D {
    /// The stencil operations of a pass on the scene depth: `None` while it
    /// has no stencil bits or for a read-only pass, else cleared to 0
    /// (`Some(true)`) or kept (`Some(false)`), and stored.
    pub(super) fn stencil_ops(&self, clear: Option<bool>) -> Option<wgpu::Operations<u32>> {
        let depth = clear.map(|clear| wgpu::Operations {
            load: if clear {
                wgpu::LoadOp::Clear(super::SCENE_DEPTH_CLEAR)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        });
        super::stencil::ops(self.shaders.depth_format() == super::stencil::STENCIL_FORMAT, depth)
    }

    /// The role material `id` (`m`) draws with in `kind`'s pass, or `None`
    /// when it isn't drawn there (see [`PipelineRole::for_pass`]). Solid
    /// stencil materials skip the depth prepass and the motion vectors and
    /// write their own depth in the opaque pass (see `stencil.rs`).
    pub(super) fn draw_role(&self, kind: Kind, id: u32, m: &GpuMaterial) -> Option<PipelineRole> {
        let stencil_solid = !m.cutout && !m.transparent && self.stencil.has(id);
        if kind == Kind::ViewModel {
            // In no prepass: solid surfaces write their own depth, and so
            // do cut-outs, through a pipeline of their own that cuts their
            // holes (see `draw_pipeline`).
            return (!m.transparent).then_some(PipelineRole::OpaqueSolid);
        }
        if stencil_solid && matches!(kind, Kind::Velocity | Kind::VelocityStill) {
            return None;
        }
        PipelineRole::for_pass(
            kind.pass(),
            m.cutout,
            m.transparent,
            self.samples > 1,
            self.full_prepass() && !stencil_solid,
        )
    }

    /// The pipeline material `id` draws with: its stencil pipeline in the
    /// color passes and the view model's when it has a stencil, its
    /// shader's otherwise.
    pub(super) fn draw_pipeline<'a>(
        &'a self,
        shader: &'a super::shaders::ShaderPipelines,
        id: u32,
        variant: ShaderVariant,
        kind: Kind,
        role: PipelineRole,
    ) -> Option<&'a wgpu::RenderPipeline> {
        if kind == Kind::ViewModel && self.resources.material(id).cutout {
            return Some(match self.stencil.view_model_cutout(id, variant.instancing) {
                Some(pipeline) => pipeline,
                None => shader.view_model_cutout(variant),
            });
        }
        if matches!(
            kind,
            Kind::Opaque | Kind::Transparent | Kind::ViewModel | Kind::ViewModelBlended
        ) && let Some(pipeline) = self.stencil.pipeline(
            id,
            variant.instancing,
            role,
            kind == Kind::Transparent && self.stencil_read_only,
        ) {
            return Some(pipeline);
        }
        shader.variant(variant).for_kind(kind, role)
    }
}

/// Whether a still model drawn with `shader` moves on its own, for the
/// motion pass: its `vertex` reads the time, or a GPU buffer that keeps its
/// last frame (`kept`).
pub(super) fn still_moves(shader: &super::shaders::ShaderPipelines, kept: bool) -> bool {
    let past = shader.past();
    past.time || (past.items && kept)
}

impl Renderer3D {
    /// Where the lit frame's color pass draws, and what it resolves into:
    /// the multisampled color target resolving into the lit image, or with
    /// one sample the lit image itself.
    pub(super) fn frame_color<'a>(
        &self,
        targets: &'a Targets,
    ) -> (&'a wgpu::TextureView, Option<&'a wgpu::TextureView>) {
        let lit = targets.lit_view(self.screen.taa_on);
        if self.samples > 1 {
            (&targets.color, Some(lit))
        } else {
            (lit, None)
        }
    }

    /// Records the compute passes of `list` (see `FrameLists::computes`)
    /// in order, with `scene` as what their shaders see of the scene, under
    /// `steps`' names (buffer, texture) and timer.
    pub(super) fn record_computes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        list: &[(u32, usize, u32)],
        scene: &wgpu::BindGroup,
        time: &PassTimer<'_, '_>,
        steps: ComputeSteps,
    ) {
        for &(compute, state, material) in list {
            if state & (1 << 31) != 0 {
                passes::mark(steps.buffer);
                let buffer = self.resources.storage_buffers[state & !(1 << 31)].as_ref().unwrap();
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("GPU buffer update"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(self.compute_pipelines[compute as usize].as_ref().unwrap());
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &self.resources.material(material).bind_group, &[]);
                pass.set_bind_group(2, scene, &[]);
                pass.set_bind_group(3, &buffer.write, &[]);
                pass.dispatch_workgroups(buffer.len.div_ceil(64), 1, 1);
                continue;
            }
            let state = &self.render_targets.computes[state].1;
            passes::mark(steps.texture);
            {
                let timestamps = time(steps.timer).map(|t| wgpu::ComputePassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: t.end_of_pass_write_index,
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("3d compute"),
                    timestamp_writes: timestamps,
                });
                pass.set_pipeline(self.compute_pipelines[compute as usize].as_ref().unwrap());
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(1, &self.resources.material(material).bind_group, &[]);
                pass.set_bind_group(2, scene, &[]);
                pass.set_bind_group(3, &state.group, &[]);
                let size = state.target.size();
                pass.dispatch_workgroups(size.width.div_ceil(8), size.height.div_ceil(8), 1);
            }
            encoder.copy_texture_to_texture(
                state.scratch.as_image_copy(),
                state.target.as_image_copy(),
                state.target.size(),
            );
        }
    }

    /// Records the occlusion depth grid's reduction, timed as a compute pass.
    pub(super) fn record_occlusion(&self, encoder: &mut wgpu::CommandEncoder, frame: &FrameParts<'_, '_>) {
        if self.extra {
            return; // the grid is the main camera's
        }
        let timestamps = (frame.time)("occlusion depth").map(|t| wgpu::ComputePassTimestampWrites {
            query_set: t.query_set,
            beginning_of_pass_write_index: t.beginning_of_pass_write_index,
            end_of_pass_write_index: t.end_of_pass_write_index,
        });
        self.occlusion.record(encoder, timestamps);
    }

    /// Whether solid surfaces go through the depth prepass too: on GPUs
    /// that need it, and for the passes that read the complete depth
    /// before the opaque pass (GTAO, screen-space reflections).
    pub(super) fn full_prepass(&self) -> bool {
        self.solid_prepass || self.screen.ao_enabled || self.screen.ssr_on || self.screen.velocity_on
    }

    /// Draws the frame's passes (see the module docs) and leaves the tone
    /// mapped result in `view`. `clear` is the background, in linear color.
    ///
    /// Each part of the frame (each shadow cascade, the depth prepass, the
    /// opaque and transparent passes, ...) is recorded into its own command
    /// buffer, all on different cores: turning draw calls into GPU commands
    /// (wgpu's `finish`) is most of a busy frame's CPU time. The buffers come
    /// back in the order they must be submitted.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        clear: wgpu::Color,
        textures: &dyn TextureTable,
        timer: Option<&GpuTimer>,
    ) -> FrameBuffers {
        self.state_counters.take();
        let Some(targets) = self.targets.as_ref().filter(|_| !self.too_many) else {
            return FrameBuffers::new();
        };
        let time = |name| timer.and_then(|t| t.pass(name));
        let split = self.splits_transparent();
        if !self.extra {
            self.shadow_cache.recorded();
        }
        // The list is kept between frames (see `frame_parts_into`).
        let mut parts = std::mem::take(&mut *self.parts.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        self.frame_parts_into(split, &mut parts);
        // The frame must keep what its passes declare (see `passes`):
        // checked in debug builds, so in every render test.
        let checks = self.check_frame(&parts, targets, view);
        let frame = FrameParts {
            view,
            clear,
            textures,
            targets,
            split,
            time: &time,
            checks,
        };
        // Consecutive parts share an encoder: each one costs wgpu a command
        // buffer, its resource trackers and their allocations. A busy
        // frame still records on several cores, in a few encoders of about
        // equal work (see `encoder_groups`).
        let weights = |part: &Part| self.part_work(*part);
        #[cfg(not(target_arch = "wasm32"))]
        let most = rayon::current_num_threads();
        #[cfg(target_arch = "wasm32")]
        let most = 1;
        let (ends, groups) = encoder_groups(&parts, weights, most);
        let record = |group: usize| {
            let start = if group == 0 { 0 } else { ends[group - 1] };
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(parts[start].label()),
            });
            // The prepared frame's buffer writes land first.
            if group == 0 {
                self.record_pending_writes(&mut encoder);
            }
            for part in &parts[start..ends[group]] {
                let outer = checks.begin(part.pass());
                self.record(*part, &mut encoder, &frame);
                checks.end(*part, outer);
            }
            encoder.finish()
        };
        let mut slots: [Option<wgpu::CommandBuffer>; MAX_ENCODERS] = Default::default();
        #[cfg(not(target_arch = "wasm32"))]
        if groups > 1 {
            use rayon::prelude::*;
            slots[..groups]
                .par_iter_mut()
                .enumerate()
                .for_each(|(group, slot)| *slot = Some(record(group)));
        } else {
            slots[0] = Some(record(0));
        }
        #[cfg(target_arch = "wasm32")]
        for (group, slot) in slots[..groups].iter_mut().enumerate() {
            *slot = Some(record(group));
        }
        *self.parts.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = parts;
        slots.into_iter().flatten().collect()
    }

    /// [`frame_parts_into`](Self::frame_parts_into) a new list (tests).
    #[cfg(test)]
    pub(super) fn frame_parts(&self, split: bool) -> Vec<Part> {
        let mut parts = Vec::new();
        self.frame_parts_into(split, &mut parts);
        parts
    }

    /// The parts of this frame, in the order they are recorded and
    /// submitted (see [`draw`](Self::draw)), into `parts` (emptied first).
    pub(super) fn frame_parts_into(&self, split: bool, parts: &mut Vec<Part>) {
        parts.clear();
        parts.reserve(8 + self.lists.cascades + self.lists.light_views);
        parts.push(Part::Prelude);
        // Small light views drawn this frame, waiting to share a pass:
        // (first, last, draws).
        let mut lights: Option<(usize, usize, usize)> = None;
        for cascade in shadow_views(self.lists.cascades, self.lists.light_views) {
            if cascade < MAX_CASCADES && self.lists.redrawn & (1 << cascade) == 0 {
                continue; // kept from an earlier frame: not even cleared
            }
            if cascade >= MAX_CASCADES && self.shadow_cache.light_kept & (1 << (cascade - MAX_CASCADES)) != 0 {
                continue; // its tile already holds this
            }
            let runs = &self.lists.shadow_parts[cascade];
            if cascade >= MAX_CASCADES && runs.len() < SHADOW_DRAWS_PER_PASS {
                match &mut lights {
                    Some((_, last, draws)) if *draws + runs.len() < SHADOW_DRAWS_PER_PASS => {
                        (*last, *draws) = (cascade, *draws + runs.len());
                    }
                    _ => {
                        if let Some((first, last, _)) = lights {
                            parts.push(Part::LightViews { first, last });
                        }
                        lights = Some((cascade, cascade, runs.len()));
                    }
                }
                continue;
            }
            // Views keep their order: the first one may clear the atlas.
            if let Some((first, last, _)) = lights.take() {
                parts.push(Part::LightViews { first, last });
            }
            let mut first = 0;
            let mut draws = 0;
            for i in 0..runs.len() {
                draws += 1;
                if draws >= SHADOW_DRAWS_PER_PASS {
                    parts.push(Part::Shadow {
                        cascade,
                        first,
                        end: i as u32 + 1,
                    });
                    (first, draws) = (i as u32 + 1, 0);
                }
            }
            if first == 0 || (first as usize) < runs.len() {
                parts.push(Part::Shadow {
                    cascade,
                    first,
                    end: runs.len() as u32,
                });
            }
        }
        if let Some((first, last, _)) = lights {
            parts.push(Part::LightViews { first, last });
        }
        // A window view (see `views.rs`) draws only its own camera's passes.
        if !self.extra && (!self.lists.views.is_empty() || self.environment.has_probes()) {
            parts.push(Part::Views);
        }
        if !self.extra && self.gi.as_ref().is_some_and(|g| g.update_count() > 0) {
            parts.push(Part::Gi);
        }
        parts.extend([Part::Prepass, Part::Opaque]);
        if split {
            parts.push(Part::Transparent);
        }
        parts.push(Part::Post);
    }

    /// How many passes draw into the light shadow atlas this frame (test hook).
    #[cfg(test)]
    pub(crate) fn light_shadow_passes(&self) -> usize {
        self.frame_parts(self.splits_transparent())
            .iter()
            .filter(|p| match p {
                Part::Shadow { cascade, .. } => *cascade >= MAX_CASCADES,
                Part::LightViews { .. } => true,
                _ => false,
            })
            .count()
    }

    /// Records one part of the frame (see [`draw`](Self::draw)).
    pub(super) fn record(&self, part: Part, encoder: &mut wgpu::CommandEncoder, frame: &FrameParts<'_, '_>) {
        let FrameParts {
            clear,
            textures,
            targets,
            split,
            time,
            checks,
            ..
        } = *frame;
        match part {
            Part::Prelude => {
                self.cull_static(encoder, time);
                self.cull_crowds(encoder, time);
                if self.extra {
                    return; // the main frame posed, computed and drew the sky
                }
                // Skinned models first: every pass draws them posed.
                if self.skinning.has_work() {
                    passes::mark("skinning");
                    let timestamps = time("skinning").map(|t| wgpu::ComputePassTimestampWrites {
                        query_set: t.query_set,
                        beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                        end_of_pass_write_index: t.end_of_pass_write_index,
                    });
                    self.skinning.record(encoder, timestamps);
                }
                // Compute passes first, so this frame's materials see their results.
                self.record_computes(encoder, &self.lists.computes, &self.no_scene, time, COMPUTE_STEPS);
                let redraw_sky = self.atmosphere && self.environment.needs_sky();
                if redraw_sky {
                    passes::mark("sky texture");
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("3d sky texture"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.sky_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: time("sky texture"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(self.sky_pipelines().0);
                    pass.set_bind_group(0, &self.shadows.bind_group, &[0]);
                    pass.draw(0..3, 0..1);
                }
                self.environment.sky(encoder, &self.sky_texture);
                // The sky kit's air alone, for its background: into the sky
                // texture (copied into the atlas just above), then beside
                // the cloud shadow map.
                if let Some(air) = self.kit_air().filter(|_| redraw_sky) {
                    passes::mark("sky kit lookup");
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("3d sky kit lookup"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.sky_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: time("sky kit lookup"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(air);
                    pass.set_bind_group(0, &self.shadows.bind_group, &[0]);
                    pass.draw(0..3, 0..1);
                    drop(pass);
                    let [x, y] = super::super::clouds::AIR_ORIGIN;
                    encoder.copy_texture_to_texture(
                        self.sky_texture.as_image_copy(),
                        wgpu::TexelCopyTextureInfo {
                            texture: &self.environment.atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x, y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        self.sky_texture.size(),
                    );
                }
                // The sky kit's cloud shadows and clouds (see `clouds.rs`).
                if let Some(clouds) = self.clouds.as_deref() {
                    let (shadows, volumetric) = self.kit_passes();
                    if shadows {
                        passes::mark("cloud shadows");
                        clouds.record_shadows(encoder, &self.environment.atlas, time("cloud shadows"));
                    }
                    if volumetric {
                        clouds.record_volume(encoder, &self.frame_bind_group, time);
                    }
                }
            }
            Part::Shadow { cascade, first, end } => {
                // Light views share the atlas: the first one clears all of
                // it, unless some keep their tiles; then each drawn view
                // clears its own.
                let whole = self.shadow_cache.light_kept == 0;
                let (view, clear) = if cascade < MAX_CASCADES {
                    (&self.shadows.layers[cascade], first == 0)
                } else if let Some(atlas) = &self.shadows.light_atlas {
                    (atlas, whole && cascade == MAX_CASCADES && first == 0)
                } else {
                    return;
                };
                let mut pass = self.begin_shadow_pass(encoder, view, clear, cascade < MAX_CASCADES, cascade, time);
                self.draw_shadow_view(&mut pass, cascade, first, end, whole);
            }
            Part::LightViews { first, last } => {
                let Some(atlas) = &self.shadows.light_atlas else {
                    return;
                };
                let whole = self.shadow_cache.light_kept == 0;
                let clear = whole && first == MAX_CASCADES;
                let mut pass = self.begin_shadow_pass(encoder, atlas, clear, false, first, time);
                for cascade in first..=last {
                    if self.shadow_cache.light_kept & (1 << (cascade - MAX_CASCADES)) != 0 {
                        continue; // its tile already holds this
                    }
                    let end = self.lists.shadow_parts[cascade].len() as u32;
                    self.draw_shadow_view(&mut pass, cascade, 0, end, whole);
                }
            }
            Part::Views => {
                // Target cameras, before the main view, whose materials may read them.
                for (target, runs, batch_view) in &self.lists.views {
                    let (id, target) = &self.render_targets.views[*target];
                    // Probe captures start empty (alpha 0): the sky fills in behind them.
                    let capture = self.environment.is_capture(*id);
                    let clear = if capture { wgpu::Color::TRANSPARENT } else { clear };
                    let particles = self.particles.target(*id).map(|p| (p, textures));
                    self.draw_view(encoder, (*id, target), runs, *batch_view, clear, time, particles);
                }
                self.environment.probes(encoder, time);
            }
            Part::Gi => self.record_gi(encoder, time),
            Part::Prepass => {
                passes::mark("depth");
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("3d depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(super::SCENE_DEPTH_CLEAR),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: self.stencil_ops(Some(true)),
                    }),
                    timestamp_writes: time("depth prepass"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                pass.set_bind_group(2, &self.no_scene, &[]);
                self.draw_solid(&mut pass, Kind::Prepass);
                drop(pass);
                self.coverage_prepass(
                    encoder,
                    &targets.color,
                    &targets.depth,
                    &self.frame_bind_group,
                    &self.lists.solid_runs,
                    Some(0),
                );
                // With every solid surface in it, the depth is complete here.
                if self.solid_prepass {
                    self.record_occlusion(encoder, frame);
                }
                if self.screen.velocity_on
                    && let Some(taa) = &targets.velocity
                {
                    // Motion of the models that moved, over the final depth
                    // (read-only); everything else moves with the camera.
                    passes::mark("motion vectors");
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("3d motion vectors"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &taa.velocity_color,
                            depth_slice: None,
                            resolve_target: Some(taa.velocity()),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                // Kept for the held items' motion, drawn
                                // over it after the color pass.
                                store: if self.view_model_motion() {
                                    wgpu::StoreOp::Store
                                } else {
                                    wgpu::StoreOp::Discard
                                },
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &targets.depth,
                            depth_ops: None,
                            stencil_ops: None,
                        }),
                        timestamp_writes: time("motion vectors"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.frame_bind_group, &[]);
                    pass.set_bind_group(2, &self.no_scene, &[]);
                    if self.screen.still_motion {
                        self.draw_solid(&mut pass, Kind::VelocityStill);
                    }
                    self.draw_models(&mut pass, &self.lists.moved_runs, Kind::Velocity, self.moved.buffer());
                }
                // The trace first: GTAO's denoise copies its cone's spread.
                if let (Some(ssr), Some(images)) = (&self.screen.ssr, &targets.ssr) {
                    ssr.trace(encoder, images, &time);
                }
                if self.screen.ao_enabled {
                    self.screen
                        .ao
                        .as_ref()
                        .unwrap()
                        .draw(encoder, targets.ao.as_ref().unwrap(), time);
                }
            }
            Part::Opaque => {
                {
                    passes::mark("color");
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("3d opaque"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: self.frame_color(targets).0,
                            depth_slice: None,
                            resolve_target: self.frame_color(targets).1,
                            ops: wgpu::Operations {
                                // Alpha 0 marks the background for the post pass.
                                load: wgpu::LoadOp::Clear(wgpu::Color { a: 0.0, ..clear }),
                                // Without multisampling this is the lit frame itself.
                                store: checks.store(
                                    passes::OPAQUE,
                                    "color",
                                    frame_checks::Kept::Color,
                                    if split || self.samples <= 1 {
                                        wgpu::StoreOp::Store
                                    } else {
                                        wgpu::StoreOp::Discard
                                    },
                                ),
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &targets.depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                // Later passes read it (the motion pass,
                                // scene compute passes, screen effects and
                                // depth of field too); without a solid
                                // prepass, it's only complete here.
                                store: checks.store(
                                    passes::OPAQUE,
                                    "color",
                                    frame_checks::Kept::Depth,
                                    if split
                                        // Solid stencil materials write theirs here.
                                        || self.stencil.any()
                                        || self.screen.taa_on
                                        || !self.effects.frame.is_empty()
                                        || !self.effects.target_frame.is_empty()
                                        || !self.lists.scene_computes.is_empty()
                                        || self.post.depth_of_field_on()
                                        || !self.full_prepass()
                                        // The occlusion grid is reduced from it below.
                                        || (!self.solid_prepass && self.occlusion.recording())
                                    {
                                        wgpu::StoreOp::Store
                                    } else {
                                        wgpu::StoreOp::Discard
                                    },
                                ),
                            }),
                            stencil_ops: self.stencil_ops(Some(false)),
                        }),
                        timestamp_writes: time("opaque"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    let opaque_scene = targets.opaque_scene.as_ref().unwrap_or(&self.no_scene);
                    pass.set_bind_group(0, &self.frame_bind_group, &[]);
                    pass.set_bind_group(2, opaque_scene, &[]);
                    self.draw_solid(&mut pass, Kind::Opaque);
                    if self.atmosphere {
                        pass.set_pipeline(self.sky_pipelines().1);
                        pass.set_bind_group(0, &self.frame_bind_group, &[]);
                        pass.draw(0..3, 0..1);
                        // The sky kit's clouds over the sky.
                        if let Some(clouds) = self.clouds.as_deref().filter(|_| self.kit_passes().1) {
                            clouds.composite(&mut pass);
                        }
                        pass.set_bind_group(2, opaque_scene, &[]);
                    }
                    if !split {
                        self.draw_models(&mut pass, &self.lists.blended_runs, Kind::Transparent, &self.instances);
                        self.particles.draw(&mut pass, textures, self.resources.white);
                        self.draw_debug_lines(&mut pass);
                    }
                    // Last: the held items, over everything (see `view_model.rs`).
                    self.draw_view_model(&mut pass, targets.size, true, !split);
                }
                // The held items' motion, now that they are in the depth.
                self.record_view_model_motion(encoder, targets, time);
                if let (Some(ssr), Some(images)) = (&self.screen.ssr, &targets.ssr) {
                    ssr.keep(encoder, images);
                }
                if !self.solid_prepass {
                    self.record_occlusion(encoder, frame);
                }
            }
            Part::Transparent => {
                if split {
                    let held_own_pass = self.view_model_see_through_pass();
                    let lit = targets.lit_texture(self.screen.taa_on);
                    passes::mark("scene copy");
                    passes::mark("color");
                    encoder.copy_texture_to_texture(
                        lit.as_image_copy(),
                        targets.scene_texture.as_image_copy(),
                        lit.size(),
                    );
                    if let Some(copy) = &targets.depth_copy {
                        passes::mark("depth copy");
                        copy.record(encoder);
                    }
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("3d transparent"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: self.frame_color(targets).0,
                            depth_slice: None,
                            resolve_target: self.frame_color(targets).1,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: checks.store(
                                    passes::TRANSPARENT,
                                    "color",
                                    frame_checks::Kept::Color,
                                    if self.samples <= 1 || held_own_pass {
                                        wgpu::StoreOp::Store
                                    } else {
                                        wgpu::StoreOp::Discard
                                    },
                                ),
                            },
                        })],
                        // Read-only depth: the shaders sample it at the same time.
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &targets.depth,
                            depth_ops: None,
                            stencil_ops: None,
                        }),
                        timestamp_writes: time("transparent"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, &self.frame_bind_group, &[]);
                    pass.set_bind_group(2, &targets.scene, &[]);
                    self.draw_models(&mut pass, &self.lists.blended_runs, Kind::Transparent, &self.instances);
                    self.particles.draw(&mut pass, textures, self.resources.white);
                    self.draw_debug_lines(&mut pass);
                    // Held see-through items that use the stencil draw in a
                    // pass of their own, which can write it.
                    self.draw_view_model(&mut pass, targets.size, false, !held_own_pass);
                    drop(pass);
                    if held_own_pass {
                        self.record_view_model_see_through(encoder, targets, time);
                    }
                }
            }
            Part::Post => self.record_post(encoder, frame),
        }
    }

    /// Starts a depth-only pass into a shadow map: `view` is a sun cascade's
    /// layer (`sun`, timed under that cascade's name) or the light atlas,
    /// cleared first or kept.
    fn begin_shadow_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: bool,
        sun: bool,
        cascade: usize,
        time: &PassTimer<'_, '_>,
    ) -> wgpu::RenderPass<'e> {
        passes::mark(if sun { "sun shadows" } else { "light shadows" });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d shadows"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(1.0)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: time(if sun {
                SHADOW_PASSES[cascade.min(SHADOW_PASSES.len() - 1)]
            } else {
                "light shadows"
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Draws shadow view `cascade`'s runs `first..end` into `pass`: a light
    /// view into its tile of the atlas (cleared first when the atlas isn't
    /// cleared whole), and with the first runs the view's instance batches.
    fn draw_shadow_view(&self, pass: &mut wgpu::RenderPass<'_>, cascade: usize, first: u32, end: u32, whole: bool) {
        if cascade >= MAX_CASCADES
            && let Some(&[x, y, side]) = self.lists.light_tiles.get(cascade - MAX_CASCADES)
        {
            pass.set_viewport(x as f32, y as f32, side as f32, side as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, side, side);
            if !whole
                && first == 0
                && let Some(tile_clear) = &self.shadows.tile_clear
            {
                pass.set_pipeline(tile_clear);
                pass.draw(0..3, 0..1);
            }
        }
        let offset = (self.shadows.stride * cascade as u64) as u32;
        pass.set_bind_group(0, &self.shadows.bind_group, &[offset]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        let runs = &self.lists.shadow_parts[cascade][first as usize..end as usize];
        self.record_model_parts(
            pass,
            runs.iter().map(|p| (p.run, Some(p.part))),
            Kind::Shadow(cascade),
            &self.instances,
        );
        if first == 0 {
            self.draw_models(pass, &[], Kind::Shadow(cascade), &self.instances);
        }
    }

    /// About how much recording `part` costs, in draws: its draws, and a
    /// share for each pass it starts (see [`encoder_groups`]).
    fn part_work(&self, part: Part) -> usize {
        let l = &self.lists;
        match part {
            Part::Prelude => PASS_WORK * (1 + l.computes.len()),
            Part::Shadow { first, end, .. } => PASS_WORK + (end - first) as usize,
            Part::LightViews { first, last } => {
                PASS_WORK + (first..=last).map(|v| l.shadow_parts[v].len()).sum::<usize>()
            }
            Part::Views => l.views.iter().map(|(_, runs, _)| 2 * (PASS_WORK + runs.len())).sum(),
            Part::Gi => 3 * PASS_WORK + l.gi_runs.len() + 2 * l.gi_face_runs.len(),
            // The depth prepass, and the screen-space passes after it.
            Part::Prepass => 3 * PASS_WORK + l.solid_runs.len(),
            Part::Opaque => PASS_WORK + l.solid_runs.len() + l.blended_runs.len(),
            Part::Transparent => PASS_WORK + l.blended_runs.len(),
            // Bloom's chain, the effects and tone mapping: many short passes.
            Part::Post => 12 * PASS_WORK + l.scene_computes.len() * PASS_WORK,
        }
    }

    /// Global illumination's part of the frame (see `gi.rs`): the probe
    /// views' sun shadow map, the views (cut-outs' coverage first, then
    /// lit), and the probes' update from them.
    pub(super) fn record_gi<'t>(&self, encoder: &mut wgpu::CommandEncoder, time: &PassTimer<'_, 't>) {
        let Some(gi) = &self.gi else {
            return;
        };
        passes::mark("shadows");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d gi shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gi.shadow,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: time("gi shadows"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let offset = (self.shadows.stride * MAX_SHADOW_VIEWS as u64) as u32;
            pass.set_bind_group(0, &self.shadows.bind_group, &[offset]);
            pass.set_bind_group(2, &self.no_scene, &[]);
            self.draw_model_runs(&mut pass, &self.lists.gi_runs, Kind::Shadow(0), &self.instances);
            if let Some(slot) = self.lists.gi_batch_slot {
                self.draw_static(&mut pass, Kind::Shadow(0), slot);
                self.draw_crowds(&mut pass, Kind::Shadow(0), slot);
            }
        }
        // Each view in its own square of the views image.
        let views = |pass: &mut wgpu::RenderPass<'_>, kind| {
            let size = crate::render::gi::VIEW_SIZE;
            pass.set_bind_group(2, &self.no_scene, &[]);
            for (i, &(start, end)) in self.lists.gi_faces.iter().enumerate() {
                let (x, y) = ((i % 6) as u32 * size, (i / 6) as u32 * size);
                pass.set_viewport(x as f32, y as f32, size as f32, size as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, size, size);
                pass.set_bind_group(0, &gi.face_groups[i], &[]);
                self.draw_model_runs(
                    pass,
                    &self.lists.gi_face_runs[start as usize..end as usize],
                    kind,
                    &self.instances,
                );
                // The batches this view sees: those culled on the GPU in
                // its face direction's slot, the others in its own.
                if let Some(slot) = self.lists.gi_batch_slot {
                    self.draw_static(pass, kind, slot + 1 + i % 6);
                    self.draw_static(pass, kind, slot + crate::three::gi::GI_BATCH_FACES + i);
                    // Crowds in their face direction's view.
                    self.draw_crowds(pass, kind, slot + 1 + i % 6);
                }
            }
        };
        let pass = |encoder: &mut wgpu::CommandEncoder, label, first: bool, timestamps, kind| {
            passes::mark(if first { "cut-out views" } else { "views" });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gi.views,
                    depth_slice: None,
                    resolve_target: (!first).then_some(&gi.resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gi.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: if first {
                            wgpu::LoadOp::Clear(super::SCENE_DEPTH_CLEAR)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: self.stencil_ops(Some(first)),
                }),
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            views(&mut pass, kind);
        };
        pass(encoder, "3d gi cut-out views", true, None, Kind::PrepassCoverage);
        pass(encoder, "3d gi views", false, time("gi views"), Kind::Capture);
        let timestamps = time("gi update").map(|t| wgpu::ComputePassTimestampWrites {
            query_set: t.query_set,
            beginning_of_pass_write_index: t.beginning_of_pass_write_index,
            end_of_pass_write_index: t.end_of_pass_write_index,
        });
        gi.update(encoder, timestamps);
    }

    /// One target camera's view: depth prepass, then the lit solid models
    /// and the sky, resolved into its render target.
    /// Draws a target camera's view into `target` (render target `id`): its
    /// prepass, then its models and the sky, then `particles` (its
    /// particles, ribbons and lines, with the textures they read), if any,
    /// and `ctx.debug`'s lines if it draws them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_view<'t>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (id, target): (u32, &ViewTarget),
        runs: &[(u32, u32, u32)],
        batch_view: usize,
        clear: wgpu::Color,
        time: &PassTimer<'_, 't>,
        particles: Option<(&super::super::particles::ParticleView, &dyn TextureTable)>,
    ) {
        passes::mark("prepass");
        passes::mark("color");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d target camera prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(super::SCENE_DEPTH_CLEAR),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: self.stencil_ops(Some(true)),
                }),
                timestamp_writes: time("target cameras"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &target.frame_bind_group, &[]);
            pass.set_bind_group(2, &self.no_scene, &[]);
            self.draw_models_view(&mut pass, runs, Kind::Prepass, &self.instances, batch_view);
        }
        self.coverage_prepass(
            encoder,
            &target.color,
            &target.depth,
            &target.frame_bind_group,
            runs,
            Some(batch_view),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d target camera"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: if self.samples > 1 {
                    &target.color
                } else {
                    &target.resolve
                },
                depth_slice: None,
                resolve_target: (self.samples > 1).then_some(&target.resolve),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: if self.samples > 1 {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: self.stencil_ops(Some(false)),
            }),
            timestamp_writes: time("target cameras"),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &target.frame_bind_group, &[]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        self.draw_models_view(&mut pass, runs, Kind::Opaque, &self.instances, batch_view);
        if self.atmosphere {
            pass.set_pipeline(self.sky_pipelines().1);
            pass.set_bind_group(0, &target.frame_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some((view, textures)) = particles {
            // No scene to read: these particles are drawn without softness.
            pass.set_bind_group(2, &self.no_scene, &[]);
            self.particles
                .draw_view(view, &mut pass, textures, self.resources.white);
        }
        if let Some(lines) = &self.debug_lines {
            lines.draw_target(&mut pass, id);
        }
        // Debug labels on top last, over everything, as on screen.
        if let Some((view, textures)) = particles {
            self.particles.draw_on_top(view, &mut pass, textures);
        }
    }

    /// Whether see-through surfaces and particles get a pass of their own
    /// after the opaque one, which binds the scene's color and depth: when
    /// a transparent surface reads the scene behind it, or a soft particle
    /// its depth.
    pub(super) fn splits_transparent(&self) -> bool {
        self.needs_scene() || self.particles.soft()
    }

    /// Whether a transparent surface this frame reads the scene behind it.
    pub(super) fn needs_scene(&self) -> bool {
        self.lists
            .runs
            .iter()
            .map(|&(mesh, _, _)| mesh)
            .chain(
                self.static_batches
                    .iter()
                    .flatten()
                    .flat_map(|b| b.levels.iter().map(|l| l.mesh.slot())),
            )
            .any(|mesh| {
                self.resources.meshes[self.base(mesh) as usize]
                    .parts
                    .iter()
                    .enumerate()
                    .any(|(part, &(_, m))| {
                        let m = self.resources.material(self.part_material(mesh, part, m));
                        m.transparent && self.shaders.sets[m.shader as usize].reads_scene
                    })
            })
    }

    /// The mesh parts of `runs` that `kind` draws, each with its material
    /// and all its models at once.
    /// With multisampling, the cut-out materials' prepass, with alpha to
    /// coverage: a pass of its own, as coverage needs a color target (`color`,
    /// attached but not written; the opaque pass clears it).
    pub(super) fn coverage_prepass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame_bind_group: &wgpu::BindGroup,
        runs: &[(u32, u32, u32)],
        batches: Option<usize>,
    ) {
        if !self.has_cutouts(runs, batches.is_some()) {
            return;
        }
        passes::mark("cut-outs");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3d cut-out prepass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: self.stencil_ops(Some(false)),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_bind_group(2, &self.no_scene, &[]);
        if let Some(view) = batches {
            self.draw_models_view(&mut pass, runs, Kind::PrepassCoverage, &self.instances, view);
        } else {
            self.draw_model_runs(&mut pass, runs, Kind::PrepassCoverage, &self.instances);
        }
    }

    pub(super) fn draw_models(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        runs: &[(u32, u32, u32)],
        kind: Kind,
        instances: &wgpu::Buffer,
    ) {
        let view = match kind {
            Kind::Shadow(v) => v + 1,
            _ => 0,
        };
        self.draw_models_view(pass, runs, kind, instances, view);
    }

    pub(super) fn draw_models_view(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        runs: &[(u32, u32, u32)],
        kind: Kind,
        instances: &wgpu::Buffer,
        view: usize,
    ) {
        if runs.as_ptr() == self.lists.solid_runs.as_ptr() && runs.len() == self.lists.solid_runs.len() {
            self.record_model_parts(
                pass,
                self.lists.solid_parts.iter().map(|d| (d.run, Some(d.part))),
                kind,
                instances,
            );
        } else if let Some(i) = self
            .lists
            .views
            .iter()
            .position(|(_, r, _)| r.as_ptr() == runs.as_ptr() && r.len() == runs.len())
        {
            self.record_model_parts(
                pass,
                self.lists.view_parts[i].iter().map(|p| (p.run, Some(p.part))),
                kind,
                instances,
            );
        } else {
            self.draw_model_runs(pass, runs, kind, instances);
        }
        if kind != Kind::Velocity {
            self.draw_static(pass, kind, view);
        }
        self.draw_crowds(pass, kind, view);
    }

    pub(super) fn draw_model_runs(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        runs: &[(u32, u32, u32)],
        kind: Kind,
        instances: &wgpu::Buffer,
    ) {
        self.record_model_parts(pass, runs.iter().copied().map(|r| (r, None)), kind, instances);
    }

    fn draw_solid(&self, pass: &mut wgpu::RenderPass<'_>, kind: Kind) {
        self.record_model_parts(
            pass,
            self.lists.solid_parts.iter().map(|d| (d.run, Some(d.part))),
            kind,
            &self.instances,
        );
        self.draw_static(pass, kind, 0);
        self.draw_crowds(pass, kind, 0);
    }

    pub(super) fn record_model_parts(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        runs: impl Iterator<Item = (Run, Option<usize>)>,
        kind: Kind,
        instances: &wgpu::Buffer,
    ) {
        let mut n = StateChanges::default();
        // Pipeline lookups, added to the cache's counters once at the end.
        let mut lookups = 0;
        pass.set_vertex_buffer(1, instances.slice(..));
        n.vertex_buffers += 1;
        if let Some(dummy) = &self.resources.storage_dummy {
            pass.set_bind_group(3, dummy, &[]);
            n.bind_groups += 1;
        }
        // Whether group 3 holds the dummy and slot 1 the whole `instances`
        // (storage meshes and batches change them).
        let (mut dummy_bound, mut instances_bound) = (true, true);
        // Only what changes is set again: each `set_*` costs wgpu
        // validation and a Metal/Vulkan call per draw otherwise.
        let mut bound = None;
        let mut bound_material = u32::MAX;
        let mut bound_buffers = None;
        // Unknown: an earlier call in the pass may have set any.
        let mut bound_reference = u32::MAX;
        for ((mesh, start, models), only_part) in runs {
            let fading = mesh & FADED != 0;
            let mesh = mesh & !FADED;
            let run_mesh = mesh;
            let posed = (mesh as usize)
                .checked_sub(self.resources.meshes.len())
                .filter(|_| self.material_override(mesh).is_none());
            let mesh = &self.resources.meshes[self.base(mesh) as usize];
            let storage = if let Some((id, count)) = mesh.gpu_buffer {
                let Some(buffer) = self.resources.storage_buffers.get(id as usize).and_then(Option::as_ref) else {
                    continue;
                };
                Some((buffer, count.min(buffer.len)))
            } else {
                None
            };
            let Some((rest, indices)) = mesh.buffers(&self.resources.mesh_arena) else {
                continue;
            };
            let vertices = match posed {
                None => rest,
                Some(k) => match self.skinning.vertices(k) {
                    Some(v) => v,
                    None => continue,
                },
            };
            // The motion pass also reads where each corner was: a posed
            // model whose pose changed has its last pose posed apart.
            let previous = match (kind, posed) {
                (Kind::Velocity, Some(k)) => self.skinning.previous_vertices(k).unwrap_or(vertices),
                _ => vertices,
            };
            let mut buffers_set = false;
            let (first_part, parts) = match only_part {
                Some(part) => (part, &mesh.parts[part..part + 1]),
                None => (0, mesh.parts.as_slice()),
            };
            for (index, (range, material)) in parts.iter().enumerate() {
                let part = first_part + index;
                let material = &self.part_material(run_mesh, part, *material);
                let m = self.resources.material(*material);
                // The role for the material's blend in this pass (see
                // `PipelineRole::for_pass`): transparent surfaces are drawn
                // last and cast no shadows.
                let Some(role) = self.draw_role(kind, *material, m) else {
                    continue;
                };
                let shader = &self.shaders.sets[m.shader as usize];
                if kind == Kind::VelocityStill && !still_moves(shader, storage.is_some_and(|(b, _)| b.keeps_past())) {
                    continue;
                }
                // Crossfading instances dither in every pass (their
                // pipelines are built the first time they're drawn, unless
                // `warm_pipelines` built them while loading).
                let variant = ShaderVariant::new(Sides::of(m.double_sided), Instancing::of(fading, storage.is_some()));
                lookups += 1;
                let Some(pipeline) = self.draw_pipeline(shader, *material, variant, kind, role) else {
                    continue;
                };
                if bound != Some(pipeline as *const _) {
                    pass.set_pipeline(pipeline);
                    n.pipelines += 1;
                    bound = Some(pipeline as *const _);
                }
                if let Some(reference) = self.stencil.reference(*material)
                    && bound_reference != reference
                {
                    pass.set_stencil_reference(reference);
                    bound_reference = reference;
                }
                if !buffers_set && bound_buffers != Some((vertices, indices, previous)) {
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    n.vertex_buffers += 1;
                    n.index_buffers += 1;
                    if kind == Kind::Velocity {
                        pass.set_vertex_buffer(2, previous.slice(..));
                        n.vertex_buffers += 1;
                    }
                    buffers_set = true;
                    bound_buffers = Some((vertices, indices, previous));
                }
                if bound_material != *material {
                    pass.set_bind_group(1, &m.bind_group, &[]);
                    n.bind_groups += 1;
                    bound_material = *material;
                }
                if let Some((b, count)) = storage {
                    // Each model gets its own draw: its one instance record
                    // (transform and tint) is shared by all `count` instances.
                    if count > 0 {
                        pass.set_bind_group(3, &b.read, &[]);
                        n.bind_groups += 1;
                        (dummy_bound, instances_bound) = (false, false);
                        let record = if kind == Kind::Velocity {
                            size_of::<MovedInstance>()
                        } else {
                            size_of::<Instance3D>()
                        } as u64;
                        for model in start..start + models {
                            pass.set_vertex_buffer(
                                1,
                                instances.slice(model as u64 * record..(model as u64 + 1) * record),
                            );
                            pass.draw_indexed(range.clone(), 0, 0..count);
                            n.vertex_buffers += 1;
                            n.draws += 1;
                        }
                    }
                } else {
                    if let Some(dummy) = self.resources.storage_dummy.as_ref().filter(|_| !dummy_bound) {
                        pass.set_bind_group(3, dummy, &[]);
                        n.bind_groups += 1;
                        dummy_bound = true;
                    }
                    if !instances_bound {
                        pass.set_vertex_buffer(1, instances.slice(..));
                        n.vertex_buffers += 1;
                        instances_bound = true;
                    }
                    pass.draw_indexed(range.clone(), 0, start..start + models);
                    n.draws += 1;
                }
            }
        }
        self.state_counters.add(&n);
        self.shaders.cache.counters.looked_up(lookups);
    }
}

impl Renderer3D {
    /// `ctx.debug`'s 3D lines, last in the pass that draws the particles.
    fn draw_debug_lines(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(lines) = &self.debug_lines {
            lines.draw(pass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ENCODER_WORK, MAX_ENCODERS, Part, encoder_groups};

    fn shadows(n: usize) -> Vec<Part> {
        (0..n)
            .map(|cascade| Part::Shadow {
                cascade,
                first: 0,
                end: 0,
            })
            .collect()
    }

    /// Each group's parts, as ranges of `parts`.
    fn groups(parts: &[Part], work: &[usize], cores: usize) -> Vec<std::ops::Range<usize>> {
        let index = |p: &Part| match p {
            Part::Shadow { cascade, .. } => *cascade,
            _ => unreachable!(),
        };
        let (ends, n) = encoder_groups(parts, |p| work[index(p)], cores);
        (0..n).map(|g| if g == 0 { 0 } else { ends[g - 1] }..ends[g]).collect()
    }

    #[test]
    fn a_small_frame_is_recorded_into_one_encoder() {
        let parts = shadows(6);
        assert_eq!(groups(&parts, &[10; 6], 8), vec![0..6]);
        assert_eq!(groups(&parts, &[ENCODER_WORK / 6; 6], 8), vec![0..6]);
    }

    #[test]
    fn a_busy_frame_is_split_into_groups_of_about_equal_work_in_order() {
        let parts = shadows(2 * MAX_ENCODERS);
        let work = [ENCODER_WORK; 2 * MAX_ENCODERS];
        let g = groups(&parts, &work, 2 * MAX_ENCODERS);
        assert_eq!(g.len(), MAX_ENCODERS);
        // Consecutive and covering every part once, in order.
        assert_eq!(g[0].start, 0);
        assert_eq!(g[g.len() - 1].end, parts.len());
        assert!(g.windows(2).all(|w| w[0].end == w[1].start && !w[0].is_empty()));
        assert!(g.iter().all(|r| r.len() == 2), "{g:?}");
        // One big part with small ones around it: the big one is a group
        // of its own, the small ones share.
        let parts = shadows(8);
        let work = [20, 20, 3 * ENCODER_WORK, 20, 20, 20, 20, 20];
        let g = groups(&parts, &work, 16);
        assert!(
            g.contains(&(2..3)) || g.iter().any(|r| r.contains(&2) && r.len() <= 2),
            "{g:?}"
        );
        assert!(g.len() <= MAX_ENCODERS);
    }

    #[test]
    fn no_more_encoders_than_cores() {
        let parts = shadows(8);
        assert_eq!(groups(&parts, &[ENCODER_WORK; 8], 2).len(), 2);
        assert_eq!(groups(&parts, &[ENCODER_WORK; 8], 1), vec![0..8]);
        assert_eq!(groups(&parts, &[ENCODER_WORK; 8], 0), vec![0..8]);
    }
}
