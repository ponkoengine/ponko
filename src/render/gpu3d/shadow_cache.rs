//! Keeps shadow views whose content hasn't changed since they were drawn.
//!
//! Each sun cascade has a layer of the shadow map, and each point and spot
//! light view a tile of the light atlas. A view is drawn again only when
//! what it would draw differs from what its layer or tile already holds:
//! its view-projection, its tile, its casters (meshes, transforms, tints
//! and crossfades, compared exactly) and every resource their pass reads
//! (meshes, materials, shaders, textures and the shadow maps themselves,
//! tracked by an epoch for shared geometry and per-view material dependencies).
//! Views drawn from data that can change without the renderer seeing it (skinned or
//! morphing models, buffer-driven meshes, shaders with their own `vertex`
//! function reading untracked inputs, cut-outs reading time, view inputs or
//! a render target)
//! are drawn every frame. So a kept view is the same depth the pass would
//! draw again, and the frame is unchanged.
//!
//! What a view holds is recorded once the frame that drew it is recorded
//! (see [`ShadowCache::commit`]), so a frame that is prepared but never
//! drawn leaves the record matching the maps.
//!
//! Comparing a view's casters costs a pass over its instances, so a view
//! is first checked cheaply: the collector numbers each view's caster list
//! (`Frame3D::shadow_generations`, a new number whenever an instance,
//! mesh or material override in it changes), and a view placed the same,
//! with nothing it reads changed (the epoch) and the same number as when
//! it was found to hold its casters, is kept without the comparison. The
//! renderer's own changes reach only the views they concern: a material by
//! the views that use it, an instance batch by the views its box (before
//! or after) can be seen in. The camera's position counts only for
//! cascades with instance batches culled by distance, and only when one
//! of their cells may have crossed a band edge.
//!
//! The sun's far cascades hold their box between frames
//! (`Lighting::shadow_refresh`), so a camera moving inside it keeps their
//! maps; held or not, every cascade's casters are listed and checked here,
//! so a kept map never lags behind its casters.

use super::*;
use std::sync::atomic::AtomicBool;

/// Whether a custom function participates in shadows, and whether its
/// inputs can vary independently of the cached geometry and material.
#[derive(Clone, Copy, Default)]
pub(super) struct ShaderShadow {
    pub vertex: bool,
    pub surface: bool,
    vertex_volatile: bool,
    surface_volatile: bool,
}
impl ShaderShadow {
    fn unknown() -> Self {
        Self {
            vertex: true,
            surface: true,
            vertex_volatile: true,
            surface_volatile: true,
        }
    }
}

/// What one shadow view drew (or would draw): enough to tell whether
/// drawing it again would give the same depth.
#[derive(Default)]
pub(super) struct ShadowContent {
    /// `ShadowCache::epoch` when drawn; 0 never matches (it starts at 1).
    epoch: u64,
    /// The collector's generation of the view's caster list it was last
    /// found to hold (`Frame3D::shadow_generations`); 0 never matches.
    generation: u64,
    /// The view's view-projection, its eye (static batch distance culling),
    /// and its tile in the light atlas (zero for the sun's cascades).
    caster: [[f32; 4]; 4],
    eye: [f32; 3],
    tile: [u32; 3],
    /// (mesh, material, count, per-part materials) per run (see `Ident`),
    /// then every run's per-part materials and instances in order.
    runs: Vec<(u32, u32, u32, u32)>,
    materials: Vec<u32>,
    parts: Vec<(u32, u32)>,
    instances: Vec<Instance3D>,
}

impl ShadowContent {
    /// Whether it was drawn as `key` places it, with nothing it reads
    /// changed since: `eye` says whether its eye draws what `key`'s would.
    fn placed(&self, epoch: u64, key: &ViewKey, eye: bool) -> bool {
        self.epoch == epoch && self.caster == key.caster && eye && self.tile == key.tile
    }

    fn matches(
        &self,
        epoch: u64,
        key: &ViewKey,
        eye: bool,
        runs: &[Run],
        ident: Ident<'_>,
        instances: &[Instance3D],
    ) -> bool {
        self.placed(epoch, key, eye)
            && self.runs.len() == runs.len()
            && {
                let mut at = 0;
                self.runs
                    .iter()
                    .zip(runs)
                    .all(|(&(m, material, n, listed), &(mesh, _, count))| {
                        let (mesh, its_material, its_parts) = ident(mesh);
                        let mine = &self.parts[at..at + listed as usize];
                        at += listed as usize;
                        (m, material, n) == (mesh, its_material, count) && mine == its_parts
                    })
            }
            && {
                let mut at = 0;
                runs.iter().all(|&(_, start, count)| {
                    let theirs = &instances[start as usize..(start + count) as usize];
                    let mine = &self.instances[at..at + count as usize];
                    at += count as usize;
                    bytemuck::cast_slice::<_, u8>(mine) == bytemuck::cast_slice::<_, u8>(theirs)
                })
            }
    }

    fn set(
        &mut self,
        epoch: u64,
        generation: u64,
        key: &ViewKey,
        runs: &[Run],
        ident: Ident<'_>,
        instances: &[Instance3D],
    ) {
        self.epoch = epoch;
        self.generation = generation;
        self.caster = key.caster;
        self.eye = key.eye;
        self.tile = key.tile;
        self.runs.clear();
        self.parts.clear();
        for &(mesh, _, count) in runs {
            let (mesh, material, parts) = ident(mesh);
            self.runs.push((mesh, material, count, parts.len() as u32));
            self.parts.extend_from_slice(parts);
        }
        self.instances.clear();
        for &(_, start, count) in runs {
            self.instances
                .extend_from_slice(&instances[start as usize..(start + count) as usize]);
        }
    }
}

/// What a run's mesh id draws, the same from frame to frame: (mesh with
/// its [`FADED`] bit, material override or `u32::MAX`). Material override
/// ids are numbered afresh each frame, so they're compared by what they draw.
type Ident<'a> = &'a dyn Fn(u32) -> (u32, u32, &'a [(u32, u32)]);

/// What places a view: see [`ShadowContent`].
pub(super) struct ViewKey {
    pub caster: [[f32; 4]; 4],
    pub eye: [f32; 3],
    pub tile: [u32; 3],
    /// The collector's generation of the view's caster list.
    pub generation: u64,
}

/// Which views this frame draws and keeps, and what each layer and tile holds.
pub(super) struct ShadowCache {
    /// Bumped by every change to what shadow passes read.
    epoch: u64,
    /// What each view's layer or tile holds now.
    drawn: [ShadowContent; MAX_SHADOW_VIEWS],
    /// What this frame draws into each view in `pending`.
    pending: [ShadowContent; MAX_SHADOW_VIEWS],
    pending_mask: u32,
    /// Views whose record goes once this frame is drawn: drawn from data
    /// that can't be compared, or light views whose tile may be overdrawn.
    forget_mask: u32,
    /// The light views kept this frame (bit `i`: light view `i`).
    pub light_kept: u32,
    /// Shadow views this frame keeps, cascades and light views together
    /// (bit `v`: shadow view `v`).
    pub kept: u32,
    /// Set when a frame is recorded, so its views' records are committed.
    recorded: AtomicBool,
    /// Per material shader: whether it has its own `vertex` and `surface`.
    pub shaders: Vec<ShaderShadow>,
    materials: Vec<u32>,
    /// Tests: keep views placed the same whatever their casters do (models,
    /// or instance batches' distance bands): the lag kept views used to
    /// have, a negative control for comparisons.
    #[cfg(test)]
    pub ignore_casters: bool,
    /// Tests: views whose casters this frame compared one by one.
    #[cfg(test)]
    pub compared: u32,
}

impl Default for ShadowCache {
    fn default() -> Self {
        Self {
            epoch: 1,
            drawn: Default::default(),
            pending: Default::default(),
            pending_mask: 0,
            forget_mask: 0,
            light_kept: 0,
            kept: 0,
            recorded: AtomicBool::new(false),
            shaders: vec![ShaderShadow::default()],
            materials: Vec::new(),
            #[cfg(test)]
            ignore_casters: false,
            #[cfg(test)]
            compared: 0,
        }
    }
}

impl ShadowCache {
    /// Something shadow passes read changed: no view is kept on its record.
    pub fn invalidate(&mut self) {
        self.epoch += 1;
    }

    /// Classify only functions used by shadow passes, including callees.
    /// Parsing fails closed; this runs at shader installation, never per frame.
    pub fn shader_traits(user: &str) -> ShaderShadow {
        use wgpu::naga;
        if user.is_empty() {
            return ShaderShadow::default();
        }
        let source = compose_shader(user);
        let Ok(module) = naga::front::wgsl::parse_str(&source) else {
            return ShaderShadow::unknown();
        };
        let Ok(info) =
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
        else {
            return ShaderShadow::unknown();
        };
        let volatile = |name: &str| {
            module
                .functions
                .iter()
                .find(|(_, f)| f.name.as_deref() == Some(name))
                .is_none_or(|(h, _)| {
                    module.global_variables.iter().any(|(g, global)| {
                        !info[h][g].is_empty()
                            && !global.binding.as_ref().is_some_and(|b| b.group == 1 && b.binding <= 7)
                    })
                })
        };
        ShaderShadow {
            vertex: defines(user, "vertex"),
            surface: defines(user, "surface"),
            // Instance indices depend on camera-list packing. Surface view
            // inputs depend on the observing camera. Keep these conservative.
            vertex_volatile: volatile("vertex") || user.contains("instance_index"),
            surface_volatile: volatile("surface") || user.contains("view"),
        }
    }

    /// An instance batch changed: only the views it reaches (`reaches`,
    /// given a view's view-projection: whether the batch, before or after
    /// the change, can be drawn in it) are drawn again, including a frame
    /// already recorded but not committed yet.
    pub fn batch_changed(&mut self, reaches: impl Fn(Mat4) -> bool) {
        for content in self.drawn.iter_mut().chain(&mut self.pending) {
            if content.epoch != 0 && reaches(Mat4::from_cols_array_2d(&content.caster)) {
                content.epoch = 0;
            }
        }
    }

    /// The eye view `v`'s layer or tile was drawn from.
    pub fn drawn_eye(&self, v: usize) -> [f32; 3] {
        self.drawn[v].eye
    }

    /// Keeps view `v` without comparing its casters when its layer or tile
    /// holds what this frame would draw: placed the same (`eye`: drawn from
    /// an eye that draws the same) with nothing it reads changed, and the
    /// same generation of its caster list. True if kept.
    pub fn unchanged(&mut self, v: usize, key: &ViewKey, eye: bool) -> bool {
        let drawn = &self.drawn[v];
        #[cfg(test)]
        let (same, eye) = if self.ignore_casters {
            (drawn.generation != 0, true)
        } else {
            (drawn.generation == key.generation, eye)
        };
        #[cfg(not(test))]
        let same = drawn.generation == key.generation;
        let kept = same && drawn.placed(self.epoch, key, eye);
        if kept {
            self.kept |= 1 << v;
        }
        kept
    }

    /// Invalidate only maps which actually used this material, including a
    /// frame already recorded but not committed yet.
    pub fn material_changed(&mut self, id: u32) {
        for content in self.drawn.iter_mut().chain(&mut self.pending) {
            if content.materials.contains(&id) {
                content.epoch = 0;
            }
        }
    }

    /// Notes that this frame's commands were recorded (called by `draw`).
    pub fn recorded(&self) {
        self.recorded.store(true, Ordering::Relaxed);
    }

    /// Takes the last recorded frame's views as what the maps now hold.
    pub fn commit(&mut self) {
        if !self.recorded.swap(false, Ordering::Relaxed) {
            return;
        }
        for v in 0..MAX_SHADOW_VIEWS {
            if self.pending_mask & (1 << v) != 0 {
                std::mem::swap(&mut self.drawn[v], &mut self.pending[v]);
            }
            if self.forget_mask & (1 << v) != 0 {
                self.drawn[v].epoch = 0;
            }
        }
        self.pending_mask = 0;
        self.forget_mask = 0;
    }

    /// Decides for view `v` (which the frame would draw) by comparing its
    /// casters with what its layer or tile holds: true to keep it.
    /// `eye`: see [`unchanged`](Self::unchanged). `volatile`: drawn from
    /// data that can't be compared.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        &mut self,
        v: usize,
        key: &ViewKey,
        eye: bool,
        runs: &[Run],
        ident: Ident<'_>,
        instances: &[Instance3D],
        volatile: bool,
    ) -> bool {
        let bit = 1 << v;
        #[cfg(test)]
        {
            self.compared += u32::from(!volatile);
        }
        if !volatile && self.drawn[v].matches(self.epoch, key, eye, runs, ident, instances) {
            // The same casters: the next frames with this list skip the comparison.
            self.drawn[v].generation = key.generation;
            self.kept |= bit;
            return true;
        }
        if volatile {
            self.forget_mask |= bit;
        } else {
            self.pending[v].set(self.epoch, key.generation, key, runs, ident, instances);
            self.pending[v].materials.clear();
            self.pending[v].materials.extend_from_slice(&self.materials);
            self.pending_mask |= bit;
        }
        false
    }

    /// Starts planning a frame (records of the last one are committed).
    pub fn begin(&mut self) {
        self.commit();
        self.pending_mask = 0;
        self.forget_mask = 0;
        self.kept = 0;
        self.light_kept = 0;
        #[cfg(test)]
        {
            self.compared = 0;
        }
    }

    /// A view the frame doesn't draw or keep: its record goes, as its layer
    /// or tile may be drawn over.
    pub fn forget(&mut self, v: usize) {
        self.forget_mask |= 1 << v;
    }
}

impl Renderer3D {
    /// Whether shadow view `v`'s runs, or the static batches it draws, use
    /// data the shadow cache can't compare (see [`ShadowCache`]).
    fn shadow_volatile(&mut self, runs: &[Run]) -> bool {
        let real = self.resources.meshes.len() as u32;
        let mut volatile = false;
        let mut materials = std::mem::take(&mut self.shadow_cache.materials);
        materials.clear();
        let mut mesh_volatile = |run_mesh: u32| {
            let mesh = self.base(run_mesh);
            if self.material_override(run_mesh).is_none() && run_mesh & !FADED >= real {
                volatile = true;
            }
            let m = &self.resources.meshes[mesh as usize];
            volatile |= m.gpu_buffer.is_some();
            for (part, &(_, own)) in m.parts.iter().enumerate() {
                let material = self.part_material(run_mesh, part, own);
                volatile |= self.material_volatile(material);
                materials.push(material);
            }
        };
        for &(mesh, _, _) in runs {
            mesh_volatile(mesh);
        }
        for batch in self.static_batches.iter().flatten().filter(|b| b.casts()) {
            for level in batch.levels.iter().filter(|l| l.shadows) {
                mesh_volatile(level.mesh.slot());
            }
        }
        self.shadow_cache.materials = materials;
        volatile
    }

    fn material_volatile(&self, material: u32) -> bool {
        let Some(m) = self.resources.materials.get(material as usize) else {
            return false;
        };
        if m.transparent {
            return false; // casts no shadow
        }
        let shader = self
            .shadow_cache
            .shaders
            .get(m.shader as usize)
            .copied()
            .unwrap_or_else(ShaderShadow::unknown);
        shader.vertex_volatile
            || (m.cutout && shader.surface_volatile)
            || ((shader.vertex || m.cutout)
                && m.bindings
                    .maps
                    .iter()
                    .flatten()
                    .any(|t| self.render_targets.textures.iter().any(|&(id, _)| id == *t)))
    }

    /// Whether a built-in cutout material reads texture `id`'s alpha for
    /// its shadows (as its base map), so an exact alpha comparison of its
    /// uploads keeps those shadows (see `texture_alpha`).
    pub fn alpha_tested(&self, id: u32) -> bool {
        self.resources
            .materials
            .iter()
            .any(|m| m.cutout && m.bindings.maps[0] == Some(id))
    }

    /// A texture's pixels changed: only dependent shadow views are refreshed.
    /// Built-in cutouts read alpha; custom functions may also read RGB.
    pub fn texture_alpha_changed(&mut self, id: u32, alpha_changed: bool) {
        for (material, m) in self.resources.materials.iter().enumerate() {
            let shader = self
                .shadow_cache
                .shaders
                .get(m.shader as usize)
                .copied()
                .unwrap_or_else(ShaderShadow::unknown);
            // Built-in cutouts read only base alpha. Normal/ORM/emissive
            // maps cannot affect it. Custom functions may read any map.
            let reads = if shader.vertex || (m.cutout && shader.surface) {
                m.bindings.maps.contains(&Some(id))
            } else {
                m.cutout && alpha_changed && m.bindings.maps[0] == Some(id)
            };
            if reads {
                self.shadow_cache.material_changed(material as u32);
            }
        }
    }

    /// Picks the shadow views this frame keeps from what their maps hold,
    /// and drops their runs (nothing of theirs is drawn or counted).
    pub(super) fn plan_shadows(&mut self, frame: &Frame3D, camera: Vec3) {
        self.shadow_cache.begin();
        // Static batches cull the cascades' casters by distance from the camera.
        let by_distance = self.static_batches.iter().flatten().any(|b| b.shadow_distance());
        for v in shadow_views(self.lists.cascades, self.lists.light_views) {
            // A crowd moves: the views it reaches are drawn anew.
            if self.crowds.reaches_shadow_view(v) {
                self.shadow_cache.forget(v);
                continue;
            }
            let generation = frame.shadow_generations[v];
            let key = if v < MAX_CASCADES {
                ViewKey {
                    caster: frame.sun_view_proj[v].to_cols_array_2d(),
                    eye: if by_distance { camera.to_array() } else { [0.0; 3] },
                    tile: [0; 3],
                    generation,
                }
            } else {
                let i = v - MAX_CASCADES;
                ViewKey {
                    caster: frame.lights.shadow_views[i].to_cols_array_2d(),
                    eye: frame.lights.shadow_positions.get(i).map_or([0.0; 3], |p| p.to_array()),
                    tile: self.lists.light_tiles.get(i).copied().unwrap_or_default(),
                    generation,
                }
            };
            // An eye that moved draws the same while no instance batch's
            // distance band can have changed what it draws.
            let drawn_eye = self.shadow_cache.drawn_eye(v);
            let eye = drawn_eye == key.eye || (v < MAX_CASCADES && self.shadow_bands_hold(drawn_eye, key.eye));
            let kept = self.shadow_cache.unchanged(v, &key, eye) || {
                let runs = std::mem::take(&mut self.lists.shadow_runs[v]);
                let volatile = self.shadow_volatile(&runs);
                self.lists.shadow_runs[v] = runs;
                let (overrides, base) = (&self.lists.material_overrides, self.lists.override_base);
                let listed = &self.lists.override_parts;
                let ident = |mesh: u32| {
                    let k = (mesh & !FADED).checked_sub(base);
                    match k.and_then(|k| overrides.get(k as usize)) {
                        Some(o) => {
                            let (start, len) = (o.parts.0 as usize, o.parts.1 as usize);
                            (o.mesh | (mesh & FADED), o.material, &listed[start..start + len])
                        }
                        None => (mesh, u32::MAX, &[][..]),
                    }
                };
                self.shadow_cache.plan(
                    v,
                    &key,
                    eye,
                    &self.lists.shadow_runs[v],
                    &ident,
                    &frame.instances,
                    volatile,
                )
            };
            if kept {
                self.lists.shadow_runs[v].clear();
                if v < MAX_CASCADES {
                    self.lists.redrawn &= !(1 << v);
                } else {
                    self.shadow_cache.light_kept |= 1 << (v - MAX_CASCADES);
                }
            }
        }
        // Light views not drawn this frame: their tiles may be drawn over.
        for v in MAX_CASCADES + self.lists.light_views..MAX_SHADOW_VIEWS {
            self.shadow_cache.forget(v);
        }
    }
}

/// Clears one tile of the light atlas to the far depth (1), where the
/// pass's viewport and scissor are: a triangle over the whole viewport at
/// depth 1, written whatever is there. The same as a load-op clear, for
/// one tile while the others are kept.
pub(super) fn tile_clear_pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("3d shadow tile clear"),
        source: wgpu::ShaderSource::Wgsl(
            "@vertex fn main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
                return vec4<f32>(p, 1.0, 1.0);
            }"
            .into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("3d shadow tile clear"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("3d shadow tile clear"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
impl Renderer3D {
    /// Number of sun cascade passes actually redrawn after both cache layers.
    pub fn shadow_cascades_redrawn(&self) -> u32 {
        self.lists.redrawn.count_ones()
    }

    /// The shadow views the last prepared frame kept (bit `v`: view `v`).
    pub fn shadow_views_kept(&self) -> u32 {
        self.shadow_cache.kept
    }

    /// How many shadow views the last prepared frame compared caster by
    /// caster (the others were kept by their list's generation, or drawn).
    pub fn shadow_views_compared(&self) -> u32 {
        self.shadow_cache.compared
    }

    /// Makes the next frame draw every shadow view (a reference to compare with).
    pub fn forget_shadows(&mut self) {
        self.shadow_cache.invalidate();
    }

    /// Keeps shadow views placed the same whatever their casters do from
    /// now on: the lag kept views used to have (a negative control).
    pub fn ignore_shadow_casters(&mut self) {
        self.shadow_cache.ignore_casters = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_dependencies_follow_helpers_and_ignore_opaque_surface_work() {
        let pure = ShadowCache::shader_traits(
            "fn vertex(v: Vertex) -> Vertex { var out = v; out.position.y += material.custom[0].x; return out; }",
        );
        assert!(pure.vertex && !pure.vertex_volatile);
        let clock = ShadowCache::shader_traits(
            "fn clock() -> f32 { return frame.time.x; } fn vertex(v: Vertex) -> Vertex { var out = v; out.position.y += clock(); return out; }",
        );
        assert!(clock.vertex_volatile);
        let surface = ShadowCache::shader_traits(
            "fn surface(s: Surface) -> Surface { var out = s; out.alpha *= s.uv.x; return out; }",
        );
        assert!(surface.surface && !surface.surface_volatile && !surface.vertex_volatile);
        let camera = ShadowCache::shader_traits(
            "fn surface(s: Surface) -> Surface { var out = s; out.alpha *= s . view.x; return out; }",
        );
        assert!(camera.surface_volatile);
        assert!(ShadowCache::shader_traits("not WGSL").vertex_volatile);
    }

    #[test]
    fn targeted_invalidation_covers_recorded_pending_maps_and_retains_other_views() {
        let mut cache = ShadowCache::default();
        cache.pending[0].epoch = cache.epoch;
        cache.pending[0].materials.push(3);
        cache.pending[1].epoch = cache.epoch;
        cache.pending[1].materials.push(7);
        cache.pending_mask = 3;
        cache.recorded();
        cache.material_changed(3);
        cache.begin();
        assert_eq!(cache.drawn[0].epoch, 0);
        assert_eq!(cache.drawn[1].epoch, cache.epoch);
        cache.material_changed(7);
        assert_eq!(cache.drawn[1].epoch, 0);
    }
}
