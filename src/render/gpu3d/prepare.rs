//! Pass preparation: everything a frame decides and uploads before any
//! pass is recorded.
//!
//! `Renderer3D::prepare` runs once per frame with `&mut self`; recording
//! (`record.rs`) then only reads. It is a fixed sequence of steps, each a
//! method here that writes one part of the renderer's per-frame state:
//! which effects are on, the temporal history decision, the draw lists,
//! the instance and uniform uploads, and the targets. A step that draws
//! nothing new for an effect stays out of the others' state: what one step
//! decides for a later one is passed along ([`FrameView`], the returned
//! values), not looked up.

use glam::{Mat4, Vec3};

use super::custom_passes::{Stage, Writer};
use super::*;

/// The frame's screen and camera, decided first and read by the later steps.
#[derive(Clone, Copy)]
struct FrameView {
    /// The size the scene is drawn at: the window's times the render scale.
    size: (u32, u32),
    /// Whether that differs from the window's (a scaling pass follows).
    upscaled: bool,
    /// The window's size (what the scaling pass draws into).
    window: (u32, u32),
    /// The camera as the game set it, and as the passes draw with it:
    /// jittered by a sub-pixel amount with temporal antialiasing.
    unjittered: Mat4,
    view_proj: Mat4,
    jitter: [f32; 2],
    /// Whether ambient occlusion is on, and the contact shadows' length (0: off).
    ao_on: bool,
    contact: f32,
}

impl Renderer3D {
    /// Uploads this frame's instances, camera, light and post settings.
    ///
    /// The steps run in this order; each is a method below, and says what it
    /// decides for the later ones.
    ///
    /// Its small buffer writes are gathered into one upload (see
    /// `traffic::WriteBatch`), whose copies [`draw`](Self::draw) puts first.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: &Prepare3D<'_>) {
        self.batch_writes(device, queue, |r| r.prepare_frame(device, queue, input));
    }

    /// Runs `prepare` (a frame's or a window view's) with this thread's
    /// buffer writes gathered into one upload, whose copies `draw` records.
    /// Copies of a frame prepared but not drawn are submitted first: they
    /// land before anything this one writes.
    pub(super) fn batch_writes(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, prepare: impl FnOnce(&mut Self)) {
        self.submit_pending_writes(device, queue);
        let writes = self.writes.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
        let gathering = traffic::begin_batch(std::mem::take(writes));
        prepare(self);
        if gathering {
            let writes = self.writes.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            *writes = traffic::end_batch();
            writes.upload(device, queue);
        }
    }

    /// Submits the copies of writes uploaded but not drawn yet, if any.
    pub(super) fn submit_pending_writes(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut writes = self.writes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if writes.has_copies() {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame writes"),
            });
            writes.record(&mut encoder);
            traffic::submit(queue, [encoder.finish()]);
        }
    }

    /// Records the copies of the writes `prepare` uploaded, first in `encoder`.
    pub(super) fn record_pending_writes(&self, encoder: &mut wgpu::CommandEncoder) {
        self.writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(encoder);
    }

    /// The steps of [`prepare`](Self::prepare).
    fn prepare_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: &Prepare3D<'_>) {
        self.resources.mesh_arena.flush(device, queue);
        self.prepare_stencil(device);
        let &Prepare3D {
            scene,
            lut,
            size: window,
        } = input;
        let &Scene3D { frame, camera, .. } = scene;
        let view = self.prepare_view(device, queue, scene, lut, window);
        let gi_frame = frame.gi.as_deref().filter(|_| self.frame_sources.gi);
        self.prepare_gi(device, queue, gi_frame);
        // A reload changes every view's shading: each window view drops
        // its history too, the next time it draws.
        if self.history.content_changed {
            for v in &mut self.extra_views {
                v.content_changed();
            }
        }
        let (rebuild, previous_view_proj) = self.begin_history(scene, &view);
        self.prepare_screen_passes(device, queue, scene, &view);
        let total = self.prepare_lists(device, queue, scene, &frame.skins, gi_frame);
        if !self.upload_instances(device, queue, frame, gi_frame, total) {
            return;
        }
        self.prepare_crowds(device, queue, frame, scene.time, (scene.occlusion, scene.camera));
        self.prepare_shadows_and_lights(device, queue, frame, camera);
        self.upload_uniforms(device, queue, scene, &view, previous_view_proj, gi_frame);
        self.prepare_targets(device, &view, rebuild);
        self.stencil_read_only = self.stencil.any() && self.splits_transparent();
        self.advance_history(queue, scene, &view, previous_view_proj);
        self.prepare_occlusion(device, queue, scene, &view);
    }

    /// [`prepare`](Self::prepare) for a window view (see `views.rs`), with
    /// its state swapped in and `self.extra` set: the view's camera, post
    /// settings, history, lists, instances, lights and targets. It reads
    /// the main frame's shadow maps (`scene.frame` carries their cascades),
    /// skinned poses and global illumination (from `main`, the main
    /// frame), and leaves everything the main frame updates for all views
    /// alone.
    pub(super) fn prepare_extra(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &Prepare3D<'_>,
        main: &Frame3D,
    ) {
        let skins = &main.skins;
        let gi_frame = main
            .gi
            .as_deref()
            .filter(|_| self.frame_sources.gi && self.gi.is_some());
        let &Prepare3D {
            scene,
            lut,
            size: window,
        } = input;
        let &Scene3D { frame, .. } = scene;
        let view = self.prepare_view(device, queue, scene, lut, window);
        let (rebuild, previous_view_proj) = self.begin_history(scene, &view);
        self.prepare_screen_passes(device, queue, scene, &view);
        let total = self.prepare_lists(device, queue, scene, skins, None);
        if !self.upload_instances(device, queue, frame, None, total) {
            return;
        }
        self.finish_static(device, queue);
        // The crowds, for this view's camera at the main frame's time.
        self.prepare_crowds(device, queue, frame, scene.time, (false, scene.camera));
        let rebind = self.bind_decal_array(frame.decal_array);
        if self.lights.upload(device, queue, &frame.lights, &frame.decals) || rebind {
            self.frame_bind_group = self.frame_sources.bind_group(device, &self.frame_buffer, &self.lights);
            self.rebind_view_model(device);
        }
        self.upload_uniforms(device, queue, scene, &view, previous_view_proj, gi_frame);
        self.prepare_targets(device, &view, rebuild);
        self.stencil_read_only = self.stencil.any() && self.splits_transparent();
        self.advance_history(queue, scene, &view, previous_view_proj);
    }

    /// Switches between adapter-supported multisampled counts at a frame
    /// boundary. A pending asynchronous shader verdict must settle first,
    /// otherwise it could restore pipelines for the previous sample count.
    pub(crate) fn set_samples(&mut self, device: &wgpu::Device, samples: u32) -> bool {
        if samples == self.samples {
            return true;
        }
        if !matches!(samples, 2 | 4)
            || self.samples == 1
            || !self.shader_checks.is_empty()
            || (samples == 2
                && !device
                    .features()
                    .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES))
        {
            return false;
        }
        self.shaders.set_samples(samples);
        self.samples = samples;
        self.rebuild_scene_depth_users(device);
        // Keep the old targets until prepare: targets_stale observes their
        // count and History::decide resets every temporal consumer together.
        true
    }

    /// Rebuilds what draws into the scene depth apart from the material
    /// shaders (the sky, particles, the depth copy), and drops what holds
    /// attachments of the old sample count or depth format (global
    /// illumination's views, target cameras), after either changed.
    fn rebuild_scene_depth_users(&mut self, device: &wgpu::Device) {
        let samples = self.samples;
        self.sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d sky"),
            layout: Some(&self.sky_layout),
            vertex: wgpu::VertexState {
                module: &self.sky_module,
                entry_point: Some("vs_far"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.shaders.depth_format(),
                depth_write_enabled: Some(false),
                depth_compare: Some(super::SCENE_DEPTH_NEARER_OR_EQUAL),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.sky_module,
                entry_point: Some("fs_sky"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.rebuild_sky_shaders(device);
        let depth_format = self.shaders.depth_format();
        if let Some(clouds) = &mut self.clouds {
            clouds.rebuild_composite(device, samples, depth_format);
        }
        let float_depth = self.float_depth;
        self.particles = super::super::particles::Particles::new(
            device,
            &self.texture_layout,
            &super::super::particles::SceneDepth {
                layout: &self.scene_layout,
                adapt: &|source| super::shaders::for_samples(source, samples, float_depth).into_owned(),
            },
            samples,
            self.shaders.depth_format(),
        );
        if self.depth_copy.is_some() {
            self.depth_copy = Some(super::depth_copy::DepthCopy::new(device, samples));
        }
        self.gi = None; // Its capture attachments also use the scene's count and format.
        self.render_targets.views.clear();
        self.shadow_cache.invalidate();
    }

    /// Gives the scene depth stencil bits once a material has a stencil
    /// state (see `stencil.rs`), and drops stencil pipelines built from
    /// shader sets that changed since. Without stencil materials, nothing.
    fn prepare_stencil(&mut self, device: &wgpu::Device) {
        if !self.stencil.wanted {
            return;
        }
        if self.shaders.depth_format() != super::stencil::STENCIL_FORMAT {
            self.shaders.set_depth_format(super::stencil::STENCIL_FORMAT);
            self.rebuild_scene_depth_users(device);
            // New targets of the new format, made by this prepare.
            self.targets = None;
        }
        self.stencil.refresh(&self.shaders);
    }

    /// Presentation stopped or its surface was recreated. The next drawn
    /// frame must have new targets even when its dimensions did not change.
    pub(crate) fn invalidate_targets(&mut self) {
        self.targets = None;
        // begin_history observes the missing targets and makes the single
        // reset decision before any temporal consumer prepares again.
    }

    /// Step 9: the scene-sized targets. Targets made for settings that have
    /// since changed (a different size, or an effect turned on or off) are
    /// replaced; the old ones are freed once the commands that used them
    /// have run.
    fn prepare_targets(&mut self, device: &wgpu::Device, view: &FrameView, rebuild: bool) {
        if rebuild {
            self.targets = Some(self.make_targets(device, view.size, (view.upscaled, view.window)));
        }
    }

    /// Step 11: what occlusion culling takes from this frame: the count of
    /// models it hid, and the depth grid the next frames test against.
    fn prepare_occlusion(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene3D<'_>, view: &FrameView) {
        let &Scene3D {
            frame,
            camera,
            occlusion,
            ..
        } = scene;
        self.occluded = frame.occluded;
        if let Some(targets) = &self.targets {
            self.occlusion.prepare(
                device,
                queue,
                &targets.depth_read,
                view.size,
                self.samples,
                frame.view_proj,
                camera,
                occlusion,
            );
        }
    }

    /// Step 1: which effects this frame has (making their passes when first
    /// on), the post settings, and the camera every pass draws with.
    fn prepare_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3D<'_>,
        lut: Option<(u32, u32)>,
        window: (u32, u32),
    ) -> FrameView {
        let &Scene3D {
            frame, lighting, post, ..
        } = scene;
        // The scene is drawn at `size` and scaled to the window's `window`.
        let size = scaled_size(window, post.render_scale, device.limits().max_texture_dimension_2d);
        let upscaled = size != window;
        let (near, far) = frame.camera_planes;
        // A frame no camera set (some tests) gets a new camera's planes.
        let planes = if near > 0.0 && far > near {
            (near, far)
        } else {
            (0.1, 1000.0)
        };
        self.post.prepare(
            queue,
            post,
            lut,
            (size, window),
            planes,
            self.resources.white,
            self.screen.taa_supported,
        );
        self.post
            .prepare_depth_of_field(device, &self.scene_layout, self.samples, self.float_depth);
        self.screen.taa_on = post.anti_aliasing == crate::render::AntiAliasing::Taa && self.screen.taa_supported;
        if self.screen.taa_on && self.screen.taa.is_none() {
            self.screen.taa = Some(Taa::new(device));
        }
        // The temporal upscaler stands in for the resolve when the scene is
        // drawn smaller than the window.
        self.screen.upscale_on = self.screen.taa_on
            && post.upscaler == crate::render::Upscaler::Temporal
            && (size.0 < window.0 || size.1 < window.1);
        if self.screen.upscale_on
            && let Some(taa) = &mut self.screen.taa
        {
            taa.prepare_upscale(device, self.post.output_format());
        }
        // The camera the frame is drawn with: jittered by a sub-pixel amount
        // with TAA (depth, GTAO and particles all use it), and the one that
        // moves are measured with.
        let unjittered = frame.view_proj;
        let jitter = if self.screen.upscale_on {
            // More positions, as each drawn pixel covers several of the window's.
            let phases = taa::jitter_phases(size.0 as f32 / window.0.max(1) as f32);
            taa::jitter_of(self.history.taa_frame, phases)
        } else if self.screen.taa_on {
            taa::jitter(self.history.taa_frame)
        } else {
            [0.0; 2]
        };
        let view_proj = if self.screen.taa_on {
            taa::jittered(unjittered, jitter, size)
        } else {
            unjittered
        };
        self.lists.moved_runs.clear();
        self.particles
            .prepare(device, queue, &frame.particles, view_proj, scene.camera);
        let for_targets = frame.views.iter().any(|v| v.debug);
        self.prepare_debug_lines(device, queue, &frame.debug_lines, (view_proj, size), for_targets);
        let ao_on = post.ambient_occlusion.enabled
            && post.ambient_occlusion.radius.is_finite()
            && post.ambient_occlusion.intensity.is_finite()
            && post.ambient_occlusion.radius > 0.0
            && post.ambient_occlusion.intensity > 0.0;
        // Contact shadows block only the sun, and only while it's up.
        let contact = if post.contact_shadows.is_finite() && lighting.sun_intensity > 0.0 {
            post.contact_shadows.max(0.0)
        } else {
            0.0
        };
        // One full-screen pass makes both.
        self.screen.ao_enabled = (ao_on || contact > 0.0) && can_compute(device);
        if self.screen.ao_enabled && self.screen.ao.is_none() {
            self.screen.ao = Some(crate::render::screen::Ao::new(device));
        }
        self.screen.ssr_on = post.ssr.is_finite()
            && post.ssr > 0.0
            && post.ssr_max_distance.is_finite()
            && post.ssr_max_distance > 0.0
            && can_compute(device);
        if self.screen.ssr_on && self.screen.ssr.is_none() {
            self.screen.ssr = Some(crate::render::ssr::Ssr::new(device));
        }
        self.screen.ssr_thin = self.screen.ssr_on && post.ssr_thin_objects;
        if self.screen.ssr_thin
            && let Some(ssr) = &mut self.screen.ssr
        {
            ssr.use_thin(device);
        }
        self.screen.velocity_on = self.screen.taa_on || (self.screen.ao_enabled && post.ao_temporal && ao_on);
        FrameView {
            size,
            upscaled,
            window,
            unjittered,
            view_proj,
            jitter,
            ao_on,
            contact,
        }
    }

    /// Step 2: global illumination's probes, made when first on and bound to
    /// every frame (again when their atlas is remade).
    fn prepare_gi(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, gi_frame: Option<&crate::three::gi::GiFrame>) {
        // Global illumination: the probes are made when first on, and their
        // atlas bound to every frame (again when it's remade).
        if let Some(g) = gi_frame {
            let gi = self.gi.get_or_insert_with(|| {
                crate::render::gi::Gi::new(
                    device,
                    self.samples,
                    self.shaders.depth_format(),
                    size_of::<FrameUniform>() as u64,
                )
            });
            let mut remade = gi.prepare(device, queue, g) || gi.face_groups.is_empty();
            remade |= gi.lights.upload(device, queue, &g.capture.lights, &g.capture.decals);
            if remade {
                self.frame_sources.gi_view = gi.atlas_view.clone();
                self.rebind_frames(device);
            }
        } else if self.gi.take().is_some() {
            self.frame_sources.gi_view = self.no_gi.clone();
            self.rebind_frames(device);
        }
    }

    /// Step 3: whether the targets are made again and whether the temporal
    /// history is dropped. Returns the first, and the camera motion is
    /// measured from.
    fn begin_history(&mut self, scene: &Scene3D<'_>, view: &FrameView) -> (bool, Mat4) {
        let &Scene3D { frame, post, .. } = scene;
        let FrameView {
            size,
            upscaled,
            window,
            unjittered,
            ao_on,
            ..
        } = *view;
        // Temporal history: one decision for every effect that reuses the
        // last frame (see `HistoryReset`), made before any of them prepares.
        let rebuild = self.targets_stale(size, post, ao_on, (upscaled, window));
        let reset = self.history.decide(frame.motion.reset, rebuild, unjittered);
        if reset.any() {
            self.forget_history();
        }
        // Camera motion is measured from the last frame's camera; after a
        // reset there is none (the last frame is no past of this one).
        let previous_view_proj = self.history.previous.unwrap_or(unjittered);
        (rebuild, previous_view_proj)
    }

    /// Drops every effect's memory of the frames before this one: TAA's
    /// history and the camera motion is measured from, temporal AO's
    /// history and SSR's last image. Each starts over from the next frame
    /// it draws, so a reset asked for on a frame that draws nothing holds.
    pub(super) fn forget_history(&mut self) {
        self.history.previous = None;
        if let Some(ao) = &mut self.screen.ao {
            ao.reset();
        }
        if let Some(ssr) = &mut self.screen.ssr {
            ssr.forget();
        }
    }

    /// Step 4: the screen-space passes' settings for this frame's camera.
    fn prepare_screen_passes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3D<'_>,
        view: &FrameView,
    ) {
        let &Scene3D {
            lighting, camera, post, ..
        } = scene;
        let FrameView {
            size,
            view_proj,
            ao_on,
            contact,
            ..
        } = *view;
        if let Some(ao) = &mut self.screen.ao {
            let sun = (-lighting.sun_direction).try_normalize().unwrap_or(Vec3::Y);
            ao.prepare(
                queue,
                view_proj,
                camera,
                ao_on.then_some(&post.ambient_occlusion),
                (sun, contact),
                (post.ao_half_resolution, post.ao_temporal && ao_on),
            );
        }
        let made_for = self.targets.as_ref().map(|t| t.size);
        self.post.prepare_smaa(device, queue, size, made_for);
    }

    /// The compute passes of `stage` that can run, from the frame's
    /// `wanted` (compute, target texture or GPU buffer with the top bit
    /// set, material), into `list` (compute, index into the render
    /// targets' `computes` or the GPU buffer with the top bit set,
    /// material). Freed passes, buffers and targets, and passes the hazard
    /// check skips, are left out.
    fn list_computes(
        &mut self,
        device: &wgpu::Device,
        wanted: &[(u32, u32, u32)],
        stage: Stage,
        list: &mut Vec<(u32, usize, u32)>,
    ) {
        list.clear();
        for (index, &(compute, target, material)) in wanted.iter().enumerate() {
            let compute = compute & crate::handle::INDEX_MASK;
            let Some(material) = self.resources.material_slot(material) else {
                continue;
            };
            if self.custom_passes.skips(stage, index) {
                continue;
            }
            if self.buffer_computes.get(compute as usize).copied() != Some(target & (1 << 31) != 0) {
                continue;
            }
            // A freed pass has no pipeline.
            if !self
                .compute_pipelines
                .get(compute as usize)
                .is_some_and(Option::is_some)
            {
                continue;
            }
            if target & (1 << 31) != 0 {
                let id = target & !(1 << 31);
                if self
                    .resources
                    .storage_buffers
                    .get(id as usize)
                    .is_some_and(Option::is_some)
                {
                    list.push((compute, target as usize, material));
                }
                continue;
            }
            if let Some(state) =
                self.render_targets
                    .compute_state(device, target, self.compute_layouts.as_ref().map(|l| &l.0))
            {
                list.push((compute, state, material));
            }
        }
    }

    /// Lists the frame's writers of render targets in frame order and
    /// checks them (see `custom_passes.rs`): `custom_passes.skips` then
    /// says which passes this frame leaves out.
    fn check_custom_passes(&mut self, frame: &Frame3D) {
        let writers = &mut self.custom_passes.writers;
        writers.clear();
        fn textures(stage: Stage, list: &[(u32, u32, u32)]) -> impl Iterator<Item = Writer> + '_ {
            list.iter()
                .enumerate()
                .filter(|(_, (_, target, _))| target & (1 << 31) == 0)
                .map(move |(index, &(shader, target, _))| Writer {
                    stage,
                    index,
                    shader,
                    target,
                })
        }
        writers.extend(textures(Stage::Compute, &frame.computes));
        writers.extend(frame.views.iter().enumerate().map(|(index, view)| Writer {
            stage: Stage::TargetCamera,
            index,
            shader: index as u32,
            target: view.target,
        }));
        writers.extend(textures(Stage::SceneCompute, &frame.scene_computes));
        writers.extend(textures(Stage::TargetEffect, &frame.target_effects));
        let (resources, targets) = (&self.resources, &self.render_targets.textures);
        self.custom_passes.run(|pass| {
            // Only target effects render to their target (see `check`).
            let Some(&(_, _, material)) = frame.target_effects.get(pass.index) else {
                return false;
            };
            let target = targets.iter().find(|(t, _)| *t == pass.target);
            resources
                .material_slot(material)
                .map(|slot| resources.material(slot))
                .zip(target)
                .is_some_and(|(m, (_, texture))| m.bindings.textures.contains(texture))
        });
    }

    /// Step 5: this frame's draw lists (runs of every view, effects, compute
    /// passes, material overrides, posed skins), sorted for the passes.
    /// Returns how many instances the frame uploads.
    fn prepare_lists(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3D<'_>,
        skins: &crate::three::SkinFrame,
        gi_frame: Option<&crate::three::gi::GiFrame>,
    ) -> usize {
        let &Scene3D {
            frame,
            sky,
            camera,
            occlusion,
            ..
        } = scene;
        let extra = self.extra;
        // A sky shader takes the place of the atmosphere and the flat sky,
        // and so does the sky kit (when the game has no sky shader of its own).
        self.sky_shader = sky.shader.map(|s| s.id).filter(|&id| self.has_sky_shader(id));
        self.kit = sky
            .kit
            .filter(|_| sky.shader.is_none())
            .filter(|_| self.ensure_kit_sky(device));
        self.atmosphere = sky.atmosphere || self.sky_shader.is_some() || self.kit.is_some();
        self.prepare_kit(device, scene);
        self.lists.override_base = frame.override_base;
        self.lists.material_overrides.clear();
        let meshes = self.resources.meshes.len() as u32;
        self.lists
            .material_overrides
            .extend(frame.material_overrides.list.iter().map(|o| MaterialOverride {
                mesh: if o.mesh < meshes { o.mesh } else { 0 },
                ..*o
            }));
        self.lists.override_parts.clear();
        self.lists
            .override_parts
            .extend_from_slice(&frame.material_overrides.parts);
        // Skinned models carry their own: listed only when one has any.
        self.lists.skin_overrides.clear();
        let none = MaterialOverride::NONE;
        if skins.draws.iter().any(|d| (d.material, d.parts) != none) {
            let offset = self.lists.override_parts.len() as u32;
            self.lists.override_parts.extend_from_slice(&skins.part_materials);
            self.lists
                .skin_overrides
                .extend(skins.draws.iter().map(|d| MaterialOverride {
                    mesh: d.mesh,
                    material: d.material,
                    parts: (d.parts.0 + offset, d.parts.1),
                }));
        }
        self.prepare_static(device, queue, frame, camera, occlusion);
        self.lists.runs.clear();
        self.lists.runs.extend_from_slice(&frame.runs);
        self.effects.frame.clear();
        let pipelines = &self.effects.pipelines;
        self.effects
            .frame
            .extend(frame.effects.iter().filter_map(|&(effect, material)| {
                let effect = effect & crate::handle::INDEX_MASK;
                pipelines.get(effect as usize)?.as_ref()?;
                Some((effect, self.resources.material_slot(material)?))
            }));
        // The game's passes that would break a resource rule are left out
        // (see `custom_passes.rs`).
        self.check_custom_passes(frame);
        let custom = &self.custom_passes;
        self.effects.prepare_targets(
            device,
            &frame.target_effects,
            &self.resources,
            &self.render_targets.textures,
            &self.shaders.layout,
            |i| custom.skips(Stage::TargetEffect, i),
        );
        // A window view draws no shadow map: it reads the main frame's.
        self.lists.cascades = if extra { 0 } else { frame.cascades };
        // Every cascade is drawn unless its map already holds what it would
        // draw (see `plan_shadows`).
        self.lists.redrawn = (1 << self.lists.cascades) - 1;
        self.lists.light_views = if extra { 0 } else { frame.light_views };
        self.lists.light_tiles.clear();
        self.lists.light_tiles.extend_from_slice(&frame.lights.shadow_tiles);
        for (mine, theirs) in self.lists.shadow_runs.iter_mut().zip(&frame.shadow_runs) {
            mine.clear();
            mine.extend_from_slice(theirs);
        }
        let mut computes = std::mem::take(&mut self.lists.computes);
        self.list_computes(device, &frame.computes, Stage::Compute, &mut computes);
        self.lists.computes = computes;
        let mut computes = std::mem::take(&mut self.lists.scene_computes);
        self.list_computes(device, &frame.scene_computes, Stage::SceneCompute, &mut computes);
        self.lists.scene_computes = computes;
        if extra {
            // The game's compute passes and target effects run once, in the
            // main frame.
            self.lists.computes.clear();
            self.lists.scene_computes.clear();
            self.effects.target_frame.clear();
        }
        // Target cameras' instances follow the main view's in one buffer,
        // then those of global illumination's probe views.
        let total = frame.instances.len()
            + frame.views.iter().map(|v| v.frame.instances.len()).sum::<usize>()
            + gi_frame.map_or(0, |g| g.capture.instances.len())
            + frame.view_model.as_ref().map_or(0, |v| v.instances.len());
        let mut views = std::mem::take(&mut self.lists.views);
        let mut used = 0;
        let mut offset = frame.instances.len() as u32;
        if !extra {
            self.particles.begin_targets();
        }
        for (camera_index, view) in frame.views.iter().enumerate().filter(|_| !extra) {
            let Some(target) = self.render_targets.view_target(
                device,
                view.target,
                self.samples,
                self.shaders.depth_format(),
                &self.frame_sources,
            ) else {
                continue;
            };
            // Debug labels are drawn with the particles' text.
            if view.particles || view.debug {
                let particles = &view.frame.particles;
                self.particles
                    .prepare_target(device, queue, view.target, particles, view.view_proj, view.camera);
            }
            if view.debug
                && let Some(lines) = &mut self.debug_lines
            {
                lines.prepare_target(device, queue, view.target, view.view_proj, view.size);
            }
            if used == views.len() {
                views.push((0, Vec::new(), 0));
            }
            let (index, runs, batch_view) = &mut views[used];
            *batch_view = 1 + MAX_CASCADES + frame.light_views + camera_index;
            *index = target;
            runs.clear();
            runs.extend(
                view.frame
                    .runs
                    .iter()
                    .map(|&(mesh, start, count)| (mesh, start + offset, count)),
            );
            offset += view.frame.instances.len() as u32;
            used += 1;
        }
        views.truncate(used);
        self.lists.views = views;
        if !extra {
            self.particles.end_targets();
            if let Some(lines) = &mut self.debug_lines {
                lines.end_targets();
            }
        }
        self.lists.gi_runs.clear();
        self.lists.gi_face_runs.clear();
        self.lists.gi_faces.clear();
        if let Some(g) = gi_frame {
            let moved = |&(mesh, start, count): &Run| (mesh, start + offset, count);
            self.lists.gi_runs.extend(g.capture.runs.iter().map(moved));
            self.lists.gi_face_runs.extend(g.face_runs.iter().map(moved));
            self.lists.gi_faces.extend_from_slice(&g.faces);
        }
        // The view model's held items after them (see `view_model.rs`).
        let offset = offset + gi_frame.map_or(0, |g| g.capture.instances.len() as u32);
        self.prepare_view_model(device, frame, offset);
        // Skinned models: only those some pass draws are posed (by the
        // main frame, for every view).
        if extra {
            self.order_runs();
            self.sort_blended(&frame.instances, camera, total as u32);
            self.order_solid_parts(&frame.instances, camera);
            return total + self.lists.blended.len();
        }
        let real = self.resources.meshes.len() as u32;
        let mut drawn = std::mem::take(&mut self.lists.skin_drawn);
        drawn.clear();
        drawn.resize(frame.skins.draws.len(), false);
        let runs = self
            .lists
            .runs
            .iter()
            .chain(shadow_views(self.lists.cascades, self.lists.light_views).flat_map(|v| &self.lists.shadow_runs[v]))
            .chain(self.lists.views.iter().flat_map(|(_, r, _)| r))
            .chain(&self.lists.gi_runs)
            .chain(self.view_model_runs());
        for &(mesh, _, _) in runs {
            if let Some(d) = (mesh & !FADED)
                .checked_sub(real)
                .and_then(|k| drawn.get_mut(k as usize))
            {
                *d = true;
            }
        }
        let meshes = &self.resources.meshes;
        self.skinning.prepare(
            device,
            queue,
            &frame.skins,
            |id| {
                let m = meshes.get(id as usize)?;
                Some((&m.buffers.as_ref()?.0, m.skin.as_ref()?))
            },
            |k| drawn[k],
            // Poses that moved are posed as they were too, for the motion
            // pass (only when it draws something: not on a reset frame).
            self.screen.velocity_on
                && (!frame.motion.runs.is_empty()
                    || frame.view_model.as_ref().is_some_and(|v| !v.motion.runs.is_empty())),
        );
        self.lists.skin_drawn = drawn;
        self.order_runs();
        // Blended models are drawn from sorted copies of their instances,
        // which follow all the others.
        self.sort_blended(&frame.instances, camera, total as u32);
        self.order_solid_parts(&frame.instances, camera);
        total + self.lists.blended.len()
    }

    /// Step 6: the instances of every view, uploaded where they changed.
    /// False when the frame draws nothing (no instances, or more than the
    /// device's largest buffer holds): the later steps are skipped.
    fn upload_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame3D,
        gi_frame: Option<&crate::three::gi::GiFrame>,
        total: usize,
    ) -> bool {
        if self.static_batches.iter().all(Option::is_none)
            && total == 0
            && frame.particles.instances.is_empty()
            && frame.particles.strips.runs.is_empty()
            && frame.debug_lines.is_empty()
            && !self.crowds.has_draws()
            // A target camera clears its render target even with nothing
            // in sight, instead of leaving its last picture there.
            && frame.views.is_empty()
        {
            self.too_many = false;
            return false;
        }
        let record = size_of::<Instance3D>();
        let most = traffic::most_bytes(device, wgpu::BufferUsages::VERTEX) / record;
        if traffic::outgrown(&mut self.capacity, total, most) {
            self.instances = Self::instance_buffer(device, self.capacity);
            self.instance_mirror.forget();
        }
        let too_many = total > self.capacity;
        if too_many && !self.too_many {
            let then = "the 3D scene isn't drawn";
            traffic::too_big(
                "3d instances (models, every view's)",
                total * record,
                most * record,
                then,
            );
        }
        self.too_many = too_many;
        if too_many {
            return false;
        }
        // Only the pages that changed since the last frame are uploaded:
        // models that stand still (and keep their place in the lists) cost
        // nothing.
        let uploads = queue.uploading(Upload::Instances);
        let buffer = &self.instances;
        let mut upload = |at: usize, bytes: &[u8]| uploads.write_buffer(buffer, at as u64, bytes);
        // A frame uploads the instances that changed, with the few
        // unchanged ones between two of them, in at most `MAX_WRITES`
        // writes a list.
        let page = size_of::<Instance3D>();
        let mut at = 0;
        let views = frame.views.iter().map(|v| &v.frame.instances);
        for instances in std::iter::once(&frame.instances)
            .chain(views)
            .chain(gi_frame.map(|g| &g.capture.instances))
            .chain(frame.view_model.as_ref().map(|v| &v.instances))
            .chain(std::iter::once(&self.lists.blended))
        {
            let bytes = bytemuck::cast_slice(instances);
            self.instance_mirror
                .sync_capped(at, bytes, (page, INSTANCE_BRIDGE), MAX_WRITES, &mut upload);
            at += bytes.len();
        }
        // Shaders whose `vertex` moves models on its own (wind, waves, a
        // buffer that keeps its last frame): their models are drawn into the
        // motion image whenever the last frame is a past of this one.
        self.screen.still_motion = self.screen.velocity_on
            && frame.motion.tracking()
            && !super::vertex_motion::disabled()
            && self.shaders.sets.iter().any(|s| s.past().any());
        if self.screen.velocity_on && !frame.motion.runs.is_empty() {
            let moved = &frame.motion.instances;
            // Bound when drawn: a new buffer needs no rebinding.
            let moved = bytemuck::cast_slice(moved);
            self.moved.reserve(device, moved.len());
            // Past the most the device holds (reported), no motion is drawn.
            if moved.len() <= self.moved.fits() {
                self.moved.write(queue, 0, moved);
                self.lists.moved_runs.extend_from_slice(&frame.motion.runs);
            }
        }
        self.upload_view_model_motion(device, queue, frame);
        true
    }

    /// Step 7: the shadow maps for this frame's sizes, which shadow views
    /// are redrawn, the instance batches' buffers, and the lights and decals
    /// (with the frame bind groups made again where a binding changed).
    fn prepare_shadows_and_lights(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame3D,
        camera: Vec3,
    ) {
        self.set_shadow_map(device, frame);
        self.set_light_shadow_map(device, frame);
        // Shadow views whose maps already hold what they'd draw are kept.
        self.plan_shadows(frame, camera);
        self.finish_static(device, queue);
        let rebind = self.bind_decal_array(frame.decal_array);
        if self.lights.upload(device, queue, &frame.lights, &frame.decals) || rebind {
            self.frame_bind_group = self.frame_sources.bind_group(device, &self.frame_buffer, &self.lights);
            self.rebind_view_model(device);
        }
        if rebind {
            for (_, target) in &mut self.render_targets.views {
                target.frame_bind_group = self
                    .frame_sources
                    .bind_group(device, &target.frame_buffer, &target.lights);
            }
            self.rebind_gi(device);
        }
    }

    /// Makes the sun's shadow map for this frame, if it isn't the one made
    /// already (see `ShadowMaps::set_sun_map`).
    fn set_shadow_map(&mut self, device: &wgpu::Device, frame: &Frame3D) {
        if self.shadows.set_sun_map(device, frame, &mut self.frame_sources) {
            self.shadow_cache.invalidate();
            self.rebind_frames(device);
        }
    }

    /// Makes, resizes or frees the point and spot lights' shadow atlas (see
    /// `ShadowMaps::set_light_map`).
    fn set_light_shadow_map(&mut self, device: &wgpu::Device, frame: &Frame3D) {
        if self.shadows.set_light_map(device, frame, &mut self.frame_sources) {
            self.shadow_cache.invalidate();
            self.rebind_frames(device);
        }
    }

    /// Step 8: the frame uniform of the main view, of every target camera,
    /// probe view and shadow view, and the environment's.
    fn upload_uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3D<'_>,
        view: &FrameView,
        previous_view_proj: Mat4,
        gi_frame: Option<&crate::three::gi::GiFrame>,
    ) {
        let &Scene3D {
            frame,
            lighting,
            sky,
            light,
            camera,
            time,
            ..
        } = scene;
        let FrameView {
            size,
            unjittered,
            view_proj,
            jitter,
            ..
        } = *view;
        let scaled = |c, k: f32| {
            let [r, g, b, _] = linear(c);
            [r * k, g * k, b * k, 0.0]
        };
        let ambient = |light: Vec3| (light * lighting.ambient_intensity).extend(0.0).to_array();
        let sun = (-lighting.sun_direction).try_normalize().unwrap_or(Vec3::Y);
        let mut light_view_proj = [Mat4::IDENTITY.to_cols_array_2d(); MAX_LIGHT_SHADOW_VIEWS];
        for (mine, theirs) in light_view_proj.iter_mut().zip(&frame.lights.shadow_views) {
            *mine = theirs.to_cols_array_2d();
        }
        let atlas = frame.lights.atlas.max(1) as f32;
        let mut light_rect = [[0.0; 4]; MAX_LIGHT_SHADOW_VIEWS];
        for (mine, &[x, y, side]) in light_rect.iter_mut().zip(&frame.lights.shadow_tiles) {
            *mine = [x as f32 / atlas, y as f32 / atlas, side as f32 / atlas, 0.0];
        }
        let past_time = self.history.time.unwrap_or(time);
        let (time_seconds, time_fraction) = super::frame::split_elapsed(time.0, past_time.0);
        let mut uniform = FrameUniform {
            time_seconds,
            time_fraction,
            light_view_proj,
            light_rect,
            probes: frame.probes,
            // Light shadows move out along the normal by 1.5 texels too; w
            // is the sun shadow's filter (`Lighting::shadow_filter`).
            light_shadow: [
                frame.light_views as f32,
                1.0 / atlas,
                1.5,
                lighting.shadow_filter as u32 as f32,
            ],
            cluster_depth: cluster_depth(frame),
            cluster_view_proj: frame.view_proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            unjittered_view_proj: unjittered.to_cols_array_2d(),
            previous_view_proj: previous_view_proj.to_cols_array_2d(),
            reprojection: previous_view_proj.to_cols_array_2d(),
            jitter: [jitter[0], jitter[1], 0.0, 0.0],
            sun_view_proj: frame.sun_view_proj.map(|m| m.to_cols_array_2d()),
            caster_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera: camera.extend(1.0).to_array(),
            // w: how much a contact-hardening shadow's edge widens per
            // unit of distance from its caster (0 with the other filters).
            sun_dir: sun.extend(lighting.penumbra_per_distance()).to_array(),
            sun_color: light.sun.extend(0.0).to_array(),
            sky: ambient(light.sky),
            ground: ambient(light.ground),
            // The shadow lookup moves out along the normal by 1.5 texels,
            // against acne on surfaces facing away from the sun.
            shadow: [
                frame.cascades as f32,
                1.0 / frame.shadow_size.max(1) as f32,
                1.5,
                lighting.shadow_distance,
            ],
            cascade_texel: frame.cascade_texel,
            // zw: the last drawn frame's, which the motion pass runs
            // `vertex` as of (this frame's on the first).
            time: [time.0 as f32, time.1, past_time.0 as f32, past_time.1],
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            screen: [
                size.0 as f32,
                size.1 as f32,
                1.0 / size.0.max(1) as f32,
                1.0 / size.1.max(1) as f32,
            ],
            atmosphere: [
                if self.atmosphere { 1.0 } else { 0.0 },
                sky.haze.max(0.0),
                (sky.sun_size.clamp(0.01, 20.0).to_radians() * 0.5).cos(),
                sky.stars.max(0.0),
            ],
            sky_light: {
                let [r, g, b, _] = scaled(lighting.sun_color, lighting.sun_intensity);
                [r, g, b, (camera.y / 1000.0).max(0.0)]
            },
            fog: [
                sky.fog_density.max(0.0),
                sky.fog_height,
                1.0 / sky.fog_falloff.max(1e-3),
                sky.light_shafts.clamp(0.0, 1.0),
            ],
            fog_color: scaled(sky.fog_color, 1.0),
            night: scaled(sky.night_light, 1.0),
            fog_far: [
                sky.fog_start.max(0.0),
                if sky.fog_end > sky.fog_start.max(0.0) {
                    sky.fog_end
                } else {
                    0.0
                },
                sky.fog_sun_glow.max(0.0),
                sky.fog_sun_focus.clamp(0.0, 0.99),
            ],
            gi: [[0.0; 4]; 3],
            sky_params: match self.sky_shader {
                Some(_) => sky.shader_params.map(|p| p.to_array()),
                None => [[0.0; 4]; 4],
            },
            sky_kit: self.kit_uniform(device, queue, (sun, camera, sky.stars), size, unjittered),
        };
        if let (Some(g), Some(gi)) = (gi_frame, &self.gi) {
            let (n, origin) = (g.grid.counts.as_vec3(), g.grid.origin.as_vec3());
            uniform.gi = [
                [0.0, 0.0, 0.0, g.grid.spacing],
                [n.x, n.y, n.z, g.intensity],
                [origin.x, origin.y, origin.z, gi.rows() as f32],
            ];
            #[cfg(test)]
            if self.gi_unsettled {
                uniform.gi[0][1] = 1.0;
            }
        }
        // A three-color ambient, from the authored colors.
        let authored_ambient = match (self.sky_shader, self.kit) {
            (Some(_), _) | (None, Some(_)) => sky.authored_ambient,
            (None, None) => !sky.atmosphere,
        };
        let equator = match sky.equator_color {
            Some(c) if authored_ambient => {
                let [r, g, b, _] = linear(c);
                [
                    r * lighting.ambient_intensity,
                    g * lighting.ambient_intensity,
                    b * lighting.ambient_intensity,
                    1.0,
                ]
            }
            _ => [0.0; 4],
        };
        self.environment.prepare(
            queue,
            [
                sun.x,
                sun.y,
                sun.z,
                sky.haze,
                camera.y,
                lighting.ambient_intensity,
                light.sky.x,
                light.sky.y,
                light.sky.z,
                light.ground.x,
                light.ground.y,
                light.ground.z,
                uniform.sky_light[0],
                uniform.sky_light[1],
                uniform.sky_light[2],
                uniform.night[0],
                uniform.night[1],
                uniform.night[2],
                equator[0],
                equator[1],
                equator[2],
                equator[3],
            ],
            self.sky_shader
                .map(|id| {
                    let mut key = [0.0; 18];
                    key[0] = id as f32;
                    key[1] = f32::from(sky.authored_ambient);
                    for (k, v) in key[2..]
                        .iter_mut()
                        .zip(sky.shader_params.iter().flat_map(|p| p.to_array()))
                    {
                        *k = v;
                    }
                    key
                })
                .or_else(|| {
                    // The kit, as a sky shader none of the game's ids can be.
                    let kit = self.kit?;
                    let mut key = [0.0; 18];
                    key[0] = -1.0;
                    key[1] = f32::from(sky.authored_ambient);
                    key[2..].copy_from_slice(&kit.bake_key(self.cloud_drift));
                    Some(key)
                }),
            self.atmosphere,
            lighting.ambient_intensity,
            authored_ambient && self.atmosphere,
            [uniform.sky, uniform.ground, equator],
        );
        sync_frame(queue, &self.frame_buffer, 0, &mut self.frame_mirror, &uniform);
        self.upload_view_model(queue, frame, &uniform, view_proj * unjittered.inverse());
        if self.extra {
            // A window view: its frame alone; the probes, target cameras
            // and shadow views are the main frame's.
            return;
        }
        self.environment.upload(device, queue, &frame.probe_uploads);
        self.environment.bakes.clear();
        self.environment.bakes.extend_from_slice(&frame.probe_bakes);
        self.environment.prepare_bakes(device, &self.render_targets.textures);
        self.environment.refresh.clear();
        self.environment.refresh.extend_from_slice(&frame.probe_refresh);
        self.environment.prepare_refresh(device, &self.render_targets.textures);
        // Each target camera's frame: the same, seen from it.
        for view in &frame.views {
            let Some(target) = self.render_targets.views.iter().position(|&(id, _)| id == view.target) else {
                continue;
            };
            let target_lights = &mut self.render_targets.views[target].1;
            if target_lights
                .lights
                .upload(device, queue, &view.frame.lights, &view.frame.decals)
            {
                target_lights.frame_bind_group =
                    self.frame_sources
                        .bind_group(device, &target_lights.frame_buffer, &target_lights.lights);
            }
            let mut theirs = uniform;
            // Captures use sky IBL only, preventing recursive probe feedback.
            if self.environment.is_capture(view.target) {
                theirs.probes.fill(Default::default());
            }
            theirs.cluster_depth = cluster_depth(&view.frame);
            theirs.cluster_view_proj = view.frame.view_proj.to_cols_array_2d();
            theirs.view_proj = view.view_proj.to_cols_array_2d();
            theirs.inv_view_proj = view.view_proj.inverse().to_cols_array_2d();
            // Target cameras are not resolved over time.
            theirs.unjittered_view_proj = theirs.view_proj;
            theirs.previous_view_proj = theirs.view_proj;
            theirs.reprojection = theirs.view_proj;
            theirs.jitter = [0.0; 4];
            theirs.camera = view.camera.extend(1.0).to_array();
            theirs.screen = [
                view.size.0 as f32,
                view.size.1 as f32,
                1.0 / view.size.0.max(1) as f32,
                1.0 / view.size.1.max(1) as f32,
            ];
            theirs.sky_light[3] = (view.camera.y / 1000.0).max(0.0);
            // The sky kit's cloud passes are the main view's: target cameras
            // draw the flat cloud layer.
            theirs.sky_kit[5][0] = 0.0;
            let target = &mut self.render_targets.views[target].1;
            sync_frame(queue, &target.frame_buffer, 0, &mut target.frame_mirror, &theirs);
        }
        // Global illumination's probe views: the same, from each probe, lit
        // by their own sun shadow map (one cascade around the grid, not
        // fading out) and the main view's point and spot light shadows.
        if let (Some(g), Some(gi)) = (gi_frame, &mut self.gi) {
            let mut theirs = uniform;
            theirs.cluster_depth = cluster_depth(&g.capture);
            theirs.cluster_view_proj = g.capture_view_proj.to_cols_array_2d();
            theirs.jitter = [0.0; 4];
            let size = crate::render::gi::VIEW_SIZE as f32;
            theirs.screen = [size, size, 1.0 / size, 1.0 / size];
            theirs.shadow = [1.0, 1.0 / GI_SHADOW_SIZE as f32, 1.5, f32::MAX];
            theirs.sun_view_proj[0] = g.sun_view_proj.to_cols_array_2d();
            theirs.cascade_texel[0] = g.sun_texel;
            theirs.sky_kit[5][0] = 0.0;
            // Tells `gi_light` it draws a probe view (see gi.wgsl).
            theirs.gi[0][0] = 1.0;
            for (i, view_proj) in g.face_view_proj.iter().enumerate() {
                let probe = g.updates[i / 6].position;
                theirs.view_proj = view_proj.to_cols_array_2d();
                theirs.inv_view_proj = view_proj.inverse().to_cols_array_2d();
                theirs.unjittered_view_proj = theirs.view_proj;
                theirs.previous_view_proj = theirs.view_proj;
                theirs.reprojection = theirs.view_proj;
                theirs.camera = probe.extend(1.0).to_array();
                theirs.sky_light[3] = (probe.y / 1000.0).max(0.0);
                sync_frame(queue, &gi.face_buffers[i], 0, &mut gi.face_mirrors[i], &theirs);
            }
        }
        // Copy 0 also serves the sky texture's pass. `camera.w` 0 tells
        // material shaders they're drawing a shadow map (`drawing_shadows`).
        uniform.camera[3] = 0.0;
        for i in 0..frame.cascades.max(1) {
            uniform.caster_view_proj = uniform.sun_view_proj[i.min(MAX_CASCADES - 1)];
            let mirror = &mut self.shadows.mirrors[i];
            sync_frame(
                queue,
                &self.shadows.frames,
                self.shadows.stride * i as u64,
                mirror,
                &uniform,
            );
        }
        for i in 0..frame.light_views {
            uniform.caster_view_proj = uniform.light_view_proj[i];
            let (at, mirror) = (MAX_CASCADES + i, &mut self.shadows.mirrors[MAX_CASCADES + i]);
            sync_frame(
                queue,
                &self.shadows.frames,
                self.shadows.stride * at as u64,
                mirror,
                &uniform,
            );
        }
        if let Some(g) = gi_frame {
            uniform.caster_view_proj = g.sun_view_proj.to_cols_array_2d();
            let mirror = &mut self.shadows.mirrors[MAX_SHADOW_VIEWS];
            sync_frame(
                queue,
                &self.shadows.frames,
                self.shadows.stride * MAX_SHADOW_VIEWS as u64,
                mirror,
                &uniform,
            );
        }
    }

    /// Step 10: the temporal effects' settings for this frame, and what the
    /// next frame measures motion and cuts from.
    fn advance_history(
        &mut self,
        queue: &wgpu::Queue,
        scene: &Scene3D<'_>,
        view: &FrameView,
        previous_view_proj: Mat4,
    ) {
        let &Scene3D { camera, post, time, .. } = scene;
        let FrameView {
            size,
            window,
            unjittered,
            view_proj,
            jitter,
            ..
        } = *view;
        // Temporal antialiasing: this frame's resolve, and what the next reads.
        if !self.screen.taa_on {
            self.history.previous = self.screen.velocity_on.then_some(unjittered);
        } else if self.has_draws() {
            // No camera of the last frame: the first frame with TAA, or
            // the history was just forgotten.
            let reset = self.history.previous.is_none();
            if let Some(taa) = &mut self.screen.taa {
                taa.prepare(
                    queue,
                    &taa::Resolve {
                        jittered: view_proj,
                        current: unjittered,
                        previous: previous_view_proj,
                        input_size: size,
                        // The temporal upscaler writes the window's size.
                        output_size: if self.screen.upscale_on { window } else { size },
                        jitter,
                        sharpness: post.taa_sharpness,
                        reset,
                    },
                );
            }
            self.history.resolved(unjittered);
        }
        // Screen-space reflections: the last frame's image is their colors,
        // unless they were off or the history was forgotten above.
        let drawn = self.has_draws();
        if let Some(ssr) = &mut self.screen.ssr {
            if !self.screen.ssr_on {
                ssr.forget();
            }
            if self.screen.ssr_on && drawn {
                ssr.prepare(queue, view_proj, camera, post.ssr.min(1.0), post.ssr_max_distance);
            }
        }
        // Cuts are measured from the last frame that left an image behind.
        if drawn {
            self.history.camera = Some(unjittered);
            self.history.time = Some(time);
        }
    }
}

impl Renderer3D {
    /// Uploads `ctx.debug`'s 3D lines (see `debug_lines.rs`) seen through
    /// `camera` (its view-projection and target size), making the
    /// pipelines the first time there are some, and again when the scene's
    /// sample count or depth format changed. `for_targets`: target cameras
    /// draw them too.
    fn prepare_debug_lines(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lines: &crate::debug::GpuLines,
        (view_proj, size): (Mat4, (u32, u32)),
        for_targets: bool,
    ) {
        use super::super::debug_lines::{DebugLines, SeeThrough};
        let see_through = SeeThrough::for_samples(self.samples);
        #[cfg(test)]
        let see_through = self.debug_see_through.unwrap_or(see_through);
        let made_for = (self.samples, self.shaders.depth_format(), see_through);
        if self.debug_lines.as_ref().is_some_and(|l| l.made_for != made_for) {
            self.debug_lines = None;
        }
        if self.debug_lines.is_none() && lines.is_empty() {
            return;
        }
        self.debug_lines
            .get_or_insert_with(|| Box::new(DebugLines::new(device, made_for.0, made_for.1, made_for.2)))
            .prepare(device, queue, lines, view_proj, size, for_targets);
    }
}
