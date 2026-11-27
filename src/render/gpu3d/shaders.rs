//! Material shaders: the game's WGSL composed with the engine's prelude and
//! entry points, and the pipelines built from it.
//!
//! [`Shaders`] owns every material shader's pipeline sets, their two
//! pipeline layouts and the cache that shares one set between shaders with
//! the same code (`pipeline_cache.rs`). A set ([`ShaderPipelines`]) lives
//! as long as a shader slot or a pending GPU check holds its `Arc`; the
//! variants a frame first needs are built inside the set, lazily
//! (`variants.rs` lists them). Nothing here knows about a frame: passes
//! look a pipeline up by shader, variant and role while recording.
//!
//! The effect and compute pass builders live here too, because they are
//! built from the same prelude and layouts; who keeps the built pipelines
//! is the renderer's business.

use std::sync::Arc;

use super::pipeline_cache::{self, Features, MaterialCache, ModuleKey, TargetKey};
use super::shader_checks::{self, end_scope, rejected};
use super::{
    DEPTH_FORMAT, HDR_FORMAT, INSTANCE_ATTRIBUTES, MOVED_ATTRIBUTES, PREVIOUS_POSITION_ATTRIBUTES, VERTEX_ATTRIBUTES,
    Vertex,
};
use super::{Instancing, PipelineRole, ShaderVariant, Sides};
use crate::render::capabilities::GpuLimits;
use crate::render::taa;
use crate::three::motion::MovedInstance;
use crate::three::{Instance3D, ShaderDiagnostic, ShaderKind};

pub(super) use super::vertex_motion::VertexPast;

/// Every material shader's pipelines, and what they are built with.
pub(super) struct Shaders {
    /// Pipeline layouts of the material shaders: lit passes, and the
    /// shadow pass (which can't read the shadow map it draws into).
    pub(super) layout: wgpu::PipelineLayout,
    pub(super) shadow_layout: wgpu::PipelineLayout,
    /// Each shader's pipelines; 0 is the built-in shader. Shaders with the
    /// same code share one set (see `pipeline_cache.rs`).
    pub(super) sets: Vec<Arc<ShaderPipelines>>,
    /// The sets by key, and the counters behind `PipelineStats`.
    pub(super) cache: MaterialCache,
    /// Whether every shader builds all its variants up front (after
    /// [`Shaders::warm`]), instead of when first drawn.
    warm_variants: bool,
    /// The optional shader parts this device has room for.
    features: Features,
    /// Samples per pixel of the targets the pipelines draw into.
    samples: u32,
    /// Whether the scene depth is read as a float texture (see
    /// [`reads_depth_as_float`]).
    float_depth: bool,
}

impl Shaders {
    /// The layouts' owner, with the built-in shader (slot 0) built.
    pub(super) fn new(
        device: &wgpu::Device,
        layout: wgpu::PipelineLayout,
        shadow_layout: wgpu::PipelineLayout,
        features: Features,
        samples: u32,
        float_depth: bool,
    ) -> Self {
        let mut cache = MaterialCache::new(TargetKey {
            color: HDR_FORMAT,
            depth: DEPTH_FORMAT,
            velocity: taa::VELOCITY_FORMAT,
            samples,
        });
        let mut builtin = build_pipelines(
            device,
            &layout,
            &shadow_layout,
            "built-in",
            &for_samples(
                &with_reflection_spread(
                    compose_shader_storage("", features.decals, features.gi, features.storage),
                    features.spread,
                ),
                samples,
                float_depth,
            ),
            samples,
            DEPTH_FORMAT,
            &cache.counters,
            Default::default(),
        );
        builtin.in_use = true;
        // wgpu objects aren't `Send` on wasm (one thread): the `Arc` only shares them.
        #[allow(clippy::arc_with_non_send_sync)]
        let builtin = Arc::new(builtin);
        cache.insert(
            cache.key(ModuleKey {
                source: "".into(),
                features,
            }),
            &builtin,
        );
        Self {
            layout,
            shadow_layout,
            sets: vec![builtin],
            cache,
            warm_variants: false,
            features,
            samples,
            float_depth,
        }
    }

    /// Changes between supported multisampled counts. Both use the same
    /// WGSL and layouts; every pipeline (including lazy variants) must change.
    pub(super) fn set_samples(&mut self, samples: u32) {
        self.retarget(samples, self.cache.targets.depth);
    }

    /// The scene depth's format the pipelines draw into: plain depth, or
    /// depth with stencil once a material has a stencil (see `stencil.rs`).
    pub(super) fn depth_format(&self) -> wgpu::TextureFormat {
        self.cache.targets.depth
    }

    /// Changes the scene depth's format: every pipeline drawing into it
    /// (including lazy variants) is rebuilt, as for a new sample count.
    pub(super) fn set_depth_format(&mut self, depth: wgpu::TextureFormat) {
        if depth != self.cache.targets.depth {
            self.retarget(self.samples, depth);
        }
    }

    fn retarget(&mut self, samples: u32, depth: wgpu::TextureFormat) {
        let mut cache = MaterialCache::new(TargetKey {
            samples,
            depth,
            ..self.cache.targets
        });
        cache.counters = self.cache.counters.clone();
        for (key, old) in self.cache.live() {
            let builder = SideBuilder {
                samples,
                depth,
                ..old.builder.clone()
            };
            let new = ShaderPipelines {
                sides: Sides::ALL
                    .map(|sides| builder.timed_build(ShaderVariant::new(sides, Instancing::Plain), &cache.counters)),
                fade: Default::default(),
                shared: Default::default(),
                view_model_cutout: Default::default(),
                prepass_normals: Default::default(),
                builder,
                counters: cache.counters.clone(),
                in_use: true,
                reads_scene: old.reads_scene,
                moves_vertices: old.moves_vertices,
            };
            // Keep warmed variants warm, and retain sharing between shader ids.
            for variant in ShaderVariant::ALL {
                if old.built(variant) {
                    new.lookup(variant, false);
                }
            }
            #[allow(clippy::arc_with_non_send_sync)]
            let new = Arc::new(new);
            for slot in &mut self.sets {
                if Arc::ptr_eq(slot, &old) {
                    *slot = new.clone();
                }
            }
            cache.insert(cache.key(key.module.clone()), &new);
        }
        self.cache = cache;
        self.samples = samples;
    }

    /// Builds every shader's pipeline variants now, and from now on with
    /// each new shader (see `Renderer3D::warm_pipelines`).
    pub(super) fn warm(&mut self) {
        self.warm_variants = true;
        for shader in &self.sets {
            shader.warm();
        }
    }

    /// Shader `id` was freed: its slot draws with the built-in shader, and
    /// its pipelines go unless another shader shares them.
    pub(super) fn free(&mut self, id: u32) {
        let builtin = self.sets[0].clone();
        if let Some(slot) = self.sets.get_mut(id as usize) {
            *slot = builtin;
        }
    }

    /// Checks and builds a material shader's pipelines for this device's
    /// variant, or shares the ones a shader with the same code already
    /// built (see `pipeline_cache.rs`). Pipeline errors (a binding the
    /// layout doesn't have) come back as an error instead of breaking the
    /// device.
    pub(super) fn material_pipelines(
        &mut self,
        device: &wgpu::Device,
        name: &str,
        user: &str,
        replacing: Option<u32>,
        defer: bool,
    ) -> Result<(Arc<ShaderPipelines>, shader_checks::LaterVerdict), Box<ShaderDiagnostic>> {
        let key = self.cache.key(ModuleKey {
            source: user.into(),
            features: self.features,
        });
        if let Some(shared) = self.cache.get(&key) {
            // A reload with unchanged code finds its own set: nothing shared.
            let own = replacing
                .and_then(|id| self.sets.get(id as usize))
                .is_some_and(|current| Arc::ptr_eq(current, &shared));
            if !own {
                self.cache.counters.shared();
            }
            if self.warm_variants {
                shared.warm();
            }
            return Ok((shared, None));
        }
        let start = crate::perf::Instant::now();
        let built = self.compile_material(device, name, user, defer);
        self.cache.counters.compiled(start.elapsed());
        let (mut built, verdict) = built?;
        // Its builds count once the GPU accepted it; rejected code's don't.
        self.cache.counters.add_builds(&built.counters);
        built.counters = self.cache.counters.clone();
        built.in_use = true;
        // wgpu objects aren't `Send` on wasm (one thread): the `Arc` only shares them.
        #[allow(clippy::arc_with_non_send_sync)]
        let built = Arc::new(built);
        self.cache.insert(key, &built);
        Ok((built, verdict))
    }

    /// The source of `set`'s module composed by `compose` (from the game's
    /// code as drawn, with decals and global illumination as this device
    /// has them) instead of [`compose_material`], ready for the targets:
    /// what crowds build their own pipelines from (see `crowds.rs`). `None`
    /// for a set the cache no longer holds.
    pub(super) fn compose_like(
        &self,
        set: &Arc<ShaderPipelines>,
        compose: impl Fn(&str, bool, bool) -> String,
    ) -> Option<String> {
        let (key, _) = self.cache.live().find(|(_, s)| Arc::ptr_eq(s, set))?;
        let drawn = super::vertex_motion::with_past(&key.module.source);
        let source = with_reflection_spread(
            compose(&drawn, self.features.decals, self.features.gi),
            self.features.spread,
        );
        Some(for_samples(&source, self.samples, self.float_depth).into_owned())
    }

    /// Compiles a material shader that no other shader shares.
    pub(super) fn compile_material(
        &self,
        device: &wgpu::Device,
        name: &str,
        user: &str,
        defer: bool,
    ) -> Result<(ShaderPipelines, shader_checks::LaterVerdict), Box<ShaderDiagnostic>> {
        let storage = self.features.storage;
        let (decals, gi) = (self.features.decals, self.features.gi);
        let variant = material_variant(decals, gi, storage);
        if !storage {
            crate::render::shader_diagnostics::require_material_storage(name, &variant, user)?;
        }
        // Checked as the game wrote it, so messages quote its own code;
        // drawn with its reads of the last frame (see `vertex_motion.rs`).
        let drawn = super::vertex_motion::with_past(user);
        let spread = self.features.spread;
        let source = with_reflection_spread(compose_storage(user, decals, gi, storage), spread);
        crate::render::diagnose(name, ShaderKind::Material, &variant, &source)?;
        let source = match drawn {
            std::borrow::Cow::Borrowed(_) => source,
            std::borrow::Cow::Owned(ref code) => {
                with_reflection_spread(compose_storage(code, decals, gi, storage), spread)
            }
        };
        let past = super::vertex_motion::vertex_past(&drawn, &source);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut pipelines = build_pipelines(
            device,
            &self.layout,
            &self.shadow_layout,
            name,
            &for_samples(&source, self.samples, self.float_depth),
            self.samples,
            self.cache.targets.depth,
            // Its own until the GPU accepts it (see `material_pipelines`).
            &Arc::default(),
            past,
        );
        pipelines.reads_scene = reads_scene(user);
        pipelines.moves_vertices = defines(user, "vertex");
        if self.warm_variants {
            pipelines.warm();
        }
        match end_scope(scope, defer) {
            Ok(None) => Ok((pipelines, None)),
            Ok(Some(error)) => Err(rejected(name, ShaderKind::Material, &variant, &error)),
            Err(verdict) => Ok((pipelines, Some((variant, verdict)))),
        }
    }
}

/// The shader source for a frame of `samples` per pixel. Sources are written
/// for the multisampled frame; with one sample the scene depth the mesh
/// prelude declares is an ordinary depth texture (`textureLoad` takes a mip
/// level there instead of a sample index, and the engine's shaders pass 0 for
/// both). A game's own shader must read the scene depth at index 0 (or through
/// `scene_position`), the only value that means the same in both: the book's
/// shader section says so.
///
/// With `float_depth` (the GL backend, see [`reads_depth_as_float`]) the
/// scene depth is declared as a float texture instead (see [`depth_as_float`]).
pub(in crate::render) fn for_samples(source: &str, samples: u32, float_depth: bool) -> std::borrow::Cow<'_, str> {
    if float_depth {
        // OpenGL ES 3.0 has no textureQueryLevels, and its refraction
        // image has only the base level (see prepare_refraction_copy).
        let source = without_contact_shadows(source).replace("textureNumLevels(scene_color_texture)", "1u");
        std::borrow::Cow::Owned(depth_as_float(&source, samples))
    } else if samples > 1 {
        std::borrow::Cow::Borrowed(source)
    } else {
        std::borrow::Cow::Owned(source.replace("texture_depth_multisampled_2d", "texture_depth_2d"))
    }
}

/// Whether `device` reads the scene depth as an unfilterable float texture:
/// on the GL backend (WebGL2 and native OpenGL ES), which can't read a
/// depth texture the way the other backends do. Its shading language has no
/// way to load a texel of a depth texture (naga rejects WGSL's `textureLoad`
/// on one), and OpenGL ES 3.0 counts a depth texture read without a
/// comparison as incomplete unless its filter is `NEAREST` (wgpu leaves it
/// linear), so even a plain `texelFetch` would read 0. The scene depth is
/// then bound as a float texture and sampled through a non-filtering
/// sampler (see [`depth_as_float`]). WebGPU allows binding a depth format as
/// an unfilterable float texture on every backend; the other backends keep
/// the depth binding, so their shaders stay as they were.
pub(super) fn reads_depth_as_float(device: &wgpu::Device) -> bool {
    #[cfg(test)]
    if FORCE_FLOAT_DEPTH.get() {
        return true;
    }
    device.adapter_info().backend == wgpu::Backend::Gl
}

/// The first and last lines of the mesh prelude's contact-hardening sun
/// shadows (`ShadowFilter::ContactHardening`).
const CONTACT_BEGIN: &str = "// ---- contact-hardening sun shadows: begin ----";
const CONTACT_END: &str = "// ---- contact-hardening sun shadows: end ----";

/// What the GL backend draws instead of contact-hardening shadows: the
/// fixed 3x3 filter (`Softest`).
const CONTACT_FALLBACK: &str =
    "fn contact_light(i: i32, c: vec4<f32>, n: vec3<f32>, turn: f32) -> f32 { return cascade_light(i, c); }";

/// `source` with the mesh prelude's contact-hardening sun shadows replaced
/// by [`CONTACT_FALLBACK`], for the GL backend (WebGL2): they load texels
/// of the shadow map, and its shading language has no way to load a texel
/// of a depth texture (see [`reads_depth_as_float`]). The lines don't
/// move, so error messages point at the same lines. A source without them
/// is returned as it is.
pub(in crate::render) fn without_contact_shadows(source: &str) -> std::borrow::Cow<'_, str> {
    let (Some(begin), Some(end)) = (source.find(CONTACT_BEGIN), source.find(CONTACT_END)) else {
        return std::borrow::Cow::Borrowed(source);
    };
    let end = end + CONTACT_END.len();
    let lines = source[begin..end].matches('\n').count();
    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..begin]);
    out.push_str(CONTACT_FALLBACK);
    out.extend(std::iter::repeat_n('\n', lines));
    out.push_str(&source[end..]);
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
thread_local! {
    /// Tests: renderers made on this thread read the scene depth as a float
    /// texture on any backend, so the GL path runs on the test machine's GPU.
    pub(crate) static FORCE_FLOAT_DEPTH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a renderer with `float_depth` and `samples` per pixel samples the
/// scene depth through a non-filtering sampler (binding 2 of the scene
/// group): with one sample. A multisampled texture has no filter to make it
/// incomplete, so it is loaded per sample (`texelFetch`), and has no sampler.
pub(super) fn samples_scene_depth(float_depth: bool, samples: u32) -> bool {
    float_depth && samples <= 1
}

/// The sample type the scene depth binding (`scene_depth_texture`) is
/// declared with: see [`reads_depth_as_float`].
pub(super) fn scene_depth_sample_type(float_depth: bool) -> wgpu::TextureSampleType {
    if float_depth {
        wgpu::TextureSampleType::Float { filterable: false }
    } else {
        wgpu::TextureSampleType::Depth
    }
}

/// What [`depth_as_float`] adds at the end of a single-sampled shader: the
/// sampler and the texel read through it (at the texel's center, with
/// `NEAREST` filtering: the stored value).
pub(super) const SAMPLED_DEPTH: &str = "
// ---- the scene depth, sampled (the GL backend) ----
@group(2) @binding(2) var scene_depth_sampler: sampler;
fn scene_depth_texel(at: vec2<i32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene_depth_texture));
    return textureSampleLevel(scene_depth_texture, scene_depth_sampler, (vec2<f32>(at) + 0.5) / size, 0.0);
}
";

/// `source` with the scene depth (`scene_depth_texture`, declared
/// `texture_depth_multisampled_2d` by the mesh prelude) read as a float
/// texture of `samples` per pixel, the same values:
///
/// - With one sample the declaration becomes `texture_2d<f32>`, and each
///   `textureLoad(scene_depth_texture, at, i)` becomes
///   `scene_depth_texel(vec2<i32>(at)).x`, which samples it through a
///   non-filtering sampler (see [`reads_depth_as_float`] for why a load
///   won't do); the sampler and that function are added at the end.
/// - Multisampled, the declaration becomes `texture_multisampled_2d<f32>`,
///   and each load takes the red channel (`.x`) of the `vec4<f32>` it now
///   returns.
///
/// A game's shader that loads the scene depth itself keeps working: its
/// calls are rewritten the same way. Calls in `//` comments are left alone;
/// lines don't move, so error messages point at the same lines.
pub(super) fn depth_as_float(source: &str, samples: u32) -> String {
    let sampled = samples <= 1;
    let declared = if sampled {
        "texture_2d<f32>"
    } else {
        "texture_multisampled_2d<f32>"
    };
    let source = source.replace("texture_depth_multisampled_2d", declared);
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(source.len() + SAMPLED_DEPTH.len() + 64);
    let mut rest = source.as_str();
    while let Some(at) = rest.find("textureLoad") {
        let after = at + "textureLoad".len();
        let line_start = rest[..at].rfind('\n').map_or(0, |n| n + 1);
        let in_comment = rest[line_start..at].contains("//");
        let standalone = !rest[..at].ends_with(ident);
        let reads_depth = rest[after..]
            .trim_start()
            .strip_prefix('(')
            .map(str::trim_start)
            .and_then(|a| a.strip_prefix("scene_depth_texture"))
            .is_some_and(|tail| !tail.starts_with(ident));
        let call = (standalone && !in_comment && reads_depth)
            .then(|| load_arguments(rest, after))
            .flatten();
        match call {
            Some(call) => {
                out.push_str(&rest[..at]);
                if sampled {
                    out.push_str("scene_depth_texel(vec2<i32>(");
                    out.push_str(rest[call.coordinate.clone()].trim());
                    out.push_str(")).x");
                } else {
                    out.push_str(&rest[at..call.end]);
                    out.push_str(".x");
                }
                rest = &rest[call.end..];
            }
            // Not a scene depth load (or unbalanced: the compiler reports it).
            None => {
                out.push_str(&rest[..after]);
                rest = &rest[after..];
            }
        }
    }
    out.push_str(rest);
    if sampled {
        out.push_str(SAMPLED_DEPTH);
    }
    out
}

/// Where a `textureLoad(scene_depth_texture, coordinate, ..)` call's parts
/// are in `source`, the call's name ending at `after`.
struct LoadCall {
    /// The coordinate argument (the second).
    coordinate: std::ops::Range<usize>,
    /// Just past the closing parenthesis.
    end: usize,
}

/// The parts of the call whose name ends at `after`: `None` if its
/// parentheses don't close or it has fewer than two arguments.
fn load_arguments(source: &str, after: usize) -> Option<LoadCall> {
    let open = after + source[after..].find('(')?;
    let mut depth = 0usize;
    let mut commas = Vec::new();
    for (i, c) in source[open..].char_indices() {
        let i = open + i;
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    let start = *commas.first()? + 1;
                    let stop = commas.get(1).copied().unwrap_or(i);
                    return Some(LoadCall {
                        coordinate: start..stop,
                        end: i + 1,
                    });
                }
            }
            ',' if depth == 1 => commas.push(i),
            _ => {}
        }
    }
    None
}

/// One material shader's pipelines: a [`Pipelines`] set per
/// [`ShaderVariant`] (see `variants.rs` for the whole list).
pub(super) struct ShaderPipelines {
    /// [`Instancing::Plain`], one-sided and double-sided: built with the shader.
    pub(super) sides: [Pipelines; 2],
    /// [`Instancing::Crossfade`], built when first drawn (or all at once by
    /// [`Renderer3D::warm_pipelines`]).
    pub(super) fade: [std::sync::OnceLock<Pipelines>; 2],
    /// [`Instancing::Shared`]: buffer-driven instances, which all share
    /// their entity's one instance record (a zero-stride instance buffer);
    /// built when first drawn (or by [`Renderer3D::warm_pipelines`]).
    pub(super) shared: [std::sync::OnceLock<Pipelines>; 2],
    /// The view model's cut-out held items (see `view_model.rs`), one per
    /// [`ShaderVariant::ALL`]: lit, cutting their own holes and writing
    /// their own depth. Built the first time one is drawn; never without
    /// a view model.
    pub(super) view_model_cutout: [std::sync::OnceLock<wgpu::RenderPipeline>; ShaderVariant::ALL.len()],
    /// The depth prepass that also writes the reflections' normals (see
    /// [`SideBuilder::build_prepass_normals`]), one per
    /// [`ShaderVariant::ALL`], solid and cut-out (`[0]`, `[1]`). Built the
    /// first time one is drawn; never without screen-space reflections.
    pub(super) prepass_normals: [[std::sync::OnceLock<wgpu::RenderPipeline>; 2]; ShaderVariant::ALL.len()],
    pub(super) builder: SideBuilder,
    /// The cache's counters: builds, misses, and the sets this one drops.
    pub(super) counters: Arc<pipeline_cache::Counters>,
    /// Whether shaders draw with it: sets built for code the GPU then
    /// rejected were never used, so dropping them invalidates nothing.
    pub(super) in_use: bool,
    /// Whether it reads the scene behind it (`scene_color`, `scene_position`,
    /// `scene_distance`): its transparent surfaces then need a copy of it.
    pub(super) reads_scene: bool,
    /// Whether the game's code has a `vertex` function, which can move a
    /// model's corners anywhere: its see-through models are never moved
    /// in the draw order (see `draw_order.rs`).
    pub(super) moves_vertices: bool,
}

impl ShaderPipelines {
    /// The pipelines of `variant`. One not built yet is built now, and
    /// counted in `built`.
    pub(super) fn variant(&self, variant: ShaderVariant) -> &Pipelines {
        self.lookup(variant, true)
    }

    /// The pipelines of `variant`, built now if needed; `drawing`: a
    /// frame needs them (a miss), not warming.
    pub(super) fn lookup(&self, variant: ShaderVariant, drawing: bool) -> &Pipelines {
        let side = variant.sides.index();
        let lazy = match variant.instancing {
            Instancing::Plain => return &self.sides[side],
            Instancing::Crossfade => &self.fade[side],
            Instancing::Shared => &self.shared[side],
        };
        lazy.get_or_init(|| {
            if drawing {
                self.counters.missed();
            }
            self.builder.timed_build(variant, &self.counters)
        })
    }

    /// The view model's cut-out pipeline of `variant`, built now the
    /// first time (counted as a build).
    pub(super) fn view_model_cutout(&self, variant: ShaderVariant) -> &wgpu::RenderPipeline {
        let at = ShaderVariant::ALL.iter().position(|&v| v == variant).unwrap_or(0);
        self.view_model_cutout[at].get_or_init(|| {
            self.counters.missed();
            let start = crate::perf::Instant::now();
            let pipeline = self.builder.build_view_model_cutout(variant, Default::default());
            self.counters.built(start.elapsed());
            pipeline
        })
    }

    /// The prepass pipeline of `variant` and prepass `role` that also
    /// writes the reflections' normals, built now the first time (counted
    /// as a build); `None` for a role that isn't the prepass's.
    pub(super) fn prepass_normals(&self, variant: ShaderVariant, role: PipelineRole) -> Option<&wgpu::RenderPipeline> {
        let at = ShaderVariant::ALL.iter().position(|&v| v == variant).unwrap_or(0);
        prepass_normals(
            &self.prepass_normals[at],
            &self.builder,
            variant,
            role,
            Some(&self.counters),
        )
    }

    /// Whether `variant` is built.
    pub(super) fn built(&self, variant: ShaderVariant) -> bool {
        let side = variant.sides.index();
        match variant.instancing {
            Instancing::Plain => true,
            Instancing::Crossfade => self.fade[side].get().is_some(),
            Instancing::Shared => self.shared[side].get().is_some(),
        }
    }

    /// Builds every variant not built yet.
    pub(super) fn warm(&self) {
        for variant in ShaderVariant::ALL {
            self.lookup(variant, false);
        }
    }

    /// How many of its variant sets are built.
    pub(super) fn built_count(&self) -> usize {
        ShaderVariant::ALL.iter().filter(|&&v| self.built(v)).count()
    }

    /// Render pipelines in each of its sets: one per [`PipelineRole`] some
    /// pass draws with at its sample count.
    pub(super) fn pipelines_per_set(&self) -> usize {
        PipelineRole::per_set(self.builder.samples > 1) + usize::from(self.builder.past.any())
    }

    /// What its `vertex` reads of the last frame.
    pub(super) fn past(&self) -> VertexPast {
        self.builder.past
    }
}

impl Drop for ShaderPipelines {
    /// The last shader using these let go (a reload or a replaced shader):
    /// its built sets are invalidated, no other shader's.
    fn drop(&mut self) {
        if self.in_use {
            self.counters.invalidated(self.built_count() as u64);
        }
    }
}

pub(super) struct Pipelines {
    pub(super) prepass: wgpu::RenderPipeline,
    pub(super) opaque: wgpu::RenderPipeline,
    pub(super) transparent: wgpu::RenderPipeline,
    pub(super) shadow: wgpu::RenderPipeline,
    /// The depth passes of cut-out materials, which run the material's
    /// shader for its alpha. Solid ones write depth without any.
    pub(super) prepass_cutout: wgpu::RenderPipeline,
    pub(super) shadow_cutout: wgpu::RenderPipeline,
    /// Cut-out materials with multisampling: alpha to coverage in the
    /// prepass (with a color target it doesn't write) and the opaque pass.
    /// `None` without multisampling.
    pub(super) coverage: Option<[wgpu::RenderPipeline; 2]>,
    /// Solid surfaces lit without a prepass: they write their own depth.
    pub(super) opaque_solid: wgpu::RenderPipeline,
    /// Motion vectors of moved models, over the opaque pass's depth.
    pub(super) velocity: wgpu::RenderPipeline,
    /// Motion vectors of the models a shader's `vertex` moves while their
    /// matrix holds still: only for shaders whose `vertex` reads the last
    /// frame ([`SideBuilder::past`]), `None` for every other.
    pub(super) velocity_still: Option<wgpu::RenderPipeline>,
}

impl Pipelines {
    /// The pipeline a `kind` draw takes for `role`: the still motion draw
    /// takes [`Pipelines::velocity_still`] (`None` without one), every other
    /// the role's.
    pub(super) fn for_kind(&self, kind: super::record::Kind, role: PipelineRole) -> Option<&wgpu::RenderPipeline> {
        if kind == super::record::Kind::VelocityStill {
            self.velocity_still.as_ref()
        } else if kind.writes_normals() {
            // Only a shader's own lazily built set has them (see
            // `ShaderPipelines::prepass_normals`), and a crowd's.
            None
        } else {
            self.role(role)
        }
    }

    /// The pipeline of `role`; `None` for the coverage roles without
    /// multisampling.
    pub(super) fn role(&self, role: PipelineRole) -> Option<&wgpu::RenderPipeline> {
        Some(match role {
            PipelineRole::Prepass => &self.prepass,
            PipelineRole::PrepassCutout => &self.prepass_cutout,
            PipelineRole::PrepassCoverage => &self.coverage.as_ref()?[0],
            PipelineRole::Opaque => &self.opaque,
            PipelineRole::OpaqueCoverage => &self.coverage.as_ref()?[1],
            PipelineRole::OpaqueSolid => &self.opaque_solid,
            PipelineRole::Transparent => &self.transparent,
            PipelineRole::Shadow => &self.shadow,
            PipelineRole::ShadowCutout => &self.shadow_cutout,
            PipelineRole::Velocity => &self.velocity,
        })
    }
}

/// The prepass pipeline of `role` with the reflections' normals from
/// `slots` (solid, cut-out), built by `builder` the first time; counted in
/// `counters` when given. `None` for a role that isn't the prepass's.
pub(super) fn prepass_normals<'a>(
    slots: &'a [std::sync::OnceLock<wgpu::RenderPipeline>; 2],
    builder: &SideBuilder,
    variant: ShaderVariant,
    role: PipelineRole,
    counters: Option<&pipeline_cache::Counters>,
) -> Option<&'a wgpu::RenderPipeline> {
    let cutout = match role {
        PipelineRole::Prepass => false,
        PipelineRole::PrepassCutout | PipelineRole::PrepassCoverage => true,
        _ => return None,
    };
    Some(slots[usize::from(cutout)].get_or_init(|| {
        let start = crate::perf::Instant::now();
        let pipeline = builder.build_prepass_normals(variant, cutout);
        if let Some(counters) = counters {
            counters.missed();
            counters.built(start.elapsed());
        }
        pipeline
    }))
}

/// The built-in parts of every material shader.
pub(super) const MESH_PRELUDE: &str = concat!(
    include_str!("../sky_sampling.wgsl"),
    "\n",
    include_str!("../mesh.wgsl"),
    "\n",
    include_str!("../material_layers.wgsl"),
    "\n",
    include_str!("../transmission.wgsl")
);

/// The line of [`MESH_PRELUDE`] that turns the material layers' code off,
/// and what the layered build of the built-in shader has instead.
const LAYERS_OFF: &str = "const MATERIAL_LAYERS: bool = false;";
const LAYERS_ON: &str = "const MATERIAL_LAYERS: bool = true;";

pub(super) const MESH_MAIN: &str = include_str!("../mesh_main.wgsl");

/// Projected decals in the mesh shader (see `Decal3D`), or the stand-in
/// where the GPU has no texture slot for them.
pub(super) const DECALS: &str = include_str!("../decal.wgsl");

pub(super) const NO_DECALS: &str = include_str!("../decal_off.wgsl");

/// The first and last lines of the material shader's read of the
/// reflections' spread image (`mesh_main.wgsl`).
const SPREAD_BEGIN: &str = "// ---- reflection spread image: begin ----";
const SPREAD_END: &str = "// ---- reflection spread image: end ----";

/// What a device without a texture slot for the reflections' spread image
/// reads instead: the copy in the screen image's first level.
const SPREAD_FALLBACK: &str = "fn reflection_spread(pixel: vec2<f32>, copied: f32) -> f32 { return copied; }";

/// A material shader `source` for a device with (`spread`) or without a
/// texture slot for the reflections' half-size spread image (see
/// `GpuLimits::reflection_spread`): without one, its binding and read are
/// replaced by [`SPREAD_FALLBACK`]. The lines don't move, so error messages
/// point at the same lines.
pub(super) fn with_reflection_spread(source: String, spread: bool) -> String {
    if spread {
        return source;
    }
    let (Some(begin), Some(end)) = (source.find(SPREAD_BEGIN), source.find(SPREAD_END)) else {
        return source;
    };
    let end = end + SPREAD_END.len();
    let lines = source[begin..end].matches('\n').count();
    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..begin]);
    out.push_str(SPREAD_FALLBACK);
    out.extend(std::iter::repeat_n('\n', lines));
    out.push_str(&source[end..]);
    out
}

/// Whether `device` has a texture slot for the reflections' spread image
/// in the opaque pass.
pub(crate) fn supports_reflection_spread(device: &wgpu::Device) -> bool {
    GpuLimits::of(device).reflection_spread()
}

/// Whether `device` has a texture slot to spare for projected decals.
/// Native devices ask for more than the default when the adapter has it;
/// WebGL2 doesn't, and skips decals.
pub(crate) fn supports_decals(device: &wgpu::Device) -> bool {
    GpuLimits::of(device).decals()
}

/// Global illumination's probes in the mesh shader (see `gi.rs`), or the
/// stand-in where the GPU can't have them.
pub(super) const GI: &str = include_str!("../gi.wgsl");

pub(super) const NO_GI: &str = include_str!("../gi_off.wgsl");

/// Baked lighting in the mesh shader (see `three::bake`), read from the
/// material's third texture array slot, or the stand-in for a game shader
/// that uses that slot itself.
pub(super) const BAKED: &str = include_str!("../baked.wgsl");

pub(super) const NO_BAKED: &str = include_str!("../baked_off.wgsl");

/// Whether a game's material code `user` reads its own third texture array
/// (which then can't carry the baked lighting).
pub(super) fn uses_third_array(user: &str) -> bool {
    crate::render::shader_diagnostics::contract_tokens(user)
        .iter()
        .any(|t| t.0 == "custom_array2")
}

/// Global illumination's probe update pass (see `gi.rs`).
pub(super) const GI_UPDATE: &str = include_str!("../gi_update.wgsl");

/// The probe update shader: the mesh prelude (the frame, the sky's light)
/// and the update.
pub(in crate::render) fn gi_update_source() -> String {
    format!("{MESH_PRELUDE}\n{GI_UPDATE}")
}

/// Whether `device` can draw global illumination: compute shaders, and a
/// texture slot past the decals' for the probes (the 18th).
pub(crate) fn supports_gi(device: &wgpu::Device) -> bool {
    GpuLimits::of(device).gi()
}

pub(super) const EFFECT_PRELUDE: &str = include_str!("../effect.wgsl");

pub(super) const EFFECT_MAIN: &str = include_str!("../effect_main.wgsl");

pub(super) const COMPUTE_PRELUDE: &str = include_str!("../compute.wgsl");

pub(super) const COMPUTE_MAIN: &str = include_str!("../compute_main.wgsl");

/// Whether WGSL `source` defines a function called `name`.
pub(super) fn defines(source: &str, name: &str) -> bool {
    crate::render::shader_diagnostics::contract_tokens(source)
        .windows(3)
        .any(|t| t[0].0 == "fn" && t[1].0 == name && t[2].0 == "(")
}

/// Whether a shader reads the scene behind it.
pub(super) fn reads_scene(user: &str) -> bool {
    ["scene_color", "scene_position", "scene_distance"]
        .iter()
        .any(|f| user.contains(f))
}

/// A whole material shader: the engine's prelude, the game's code (with
/// pass-through `vertex` and `surface` functions when it has none) and the
/// engine's entry points.
pub(crate) fn compose_shader(user: &str) -> String {
    compose_material(&super::vertex_motion::with_past(user), true, true)
}

/// [`compose_shader`] for a device with (`decals`) or without a texture
/// slot for projected decals (and none for global illumination either).
#[cfg(test)]
pub(crate) fn compose_shader_with(user: &str, decals: bool) -> String {
    compose_material(&super::vertex_motion::with_past(user), decals, decals)
}

/// [`compose_shader`] for a device with or without the texture slots for
/// projected decals and global illumination, with the game's code `user` as
/// it is (see [`super::vertex_motion::with_past`] for the code it draws with).
pub(super) fn compose_material(user: &str, decals: bool, gi: bool) -> String {
    let decal_code = if decals { DECALS } else { NO_DECALS };
    let gi_code = if gi { GI } else { NO_GI };
    let baked_code = if uses_third_array(user) { NO_BAKED } else { BAKED };
    let mut source = String::with_capacity(
        MESH_PRELUDE.len() + decal_code.len() + gi_code.len() + baked_code.len() + user.len() + MESH_MAIN.len() + 200,
    );
    let layered = user == crate::three::LAYERED_SHADER || user == crate::three::api::LAYERED_MULTI_UV_SHADER;
    let multi_uv = user == crate::three::api::MULTI_UV_SHADER || user == crate::three::api::LAYERED_MULTI_UV_SHADER;
    let prelude = if layered {
        std::borrow::Cow::Owned(MESH_PRELUDE.replacen(LAYERS_OFF, LAYERS_ON, 1))
    } else {
        std::borrow::Cow::Borrowed(MESH_PRELUDE)
    };
    if multi_uv {
        let (head, tail) = prelude.split_once("// MULTI_UV_IMPLEMENTATION").unwrap_or_default();
        let (_, tail) = tail.split_once("// END_MULTI_UV_IMPLEMENTATION").unwrap_or_default();
        source.push_str(&head.replace(
            "// EXTRA_UV_UNIFORM",
            "uv_additional: array<vec4<f32>, 6>, uv_indices: vec4<u32>,",
        ));
        source.push_str(include_str!("../material_uv.wgsl"));
        source.push_str(tail);
    } else {
        source.push_str(&prelude);
    }
    source.push_str("@group(3) @binding(0) var<storage, read> items: array<vec4<f32>>;\n@group(3) @binding(1) var<storage, read> static_indices: array<u32>;\n");
    source.push_str(super::vertex_motion::ITEM_HELPER);
    source.push_str(decal_code);
    source.push_str(gi_code);
    source.push_str(baked_code);
    source.push_str("\n// ---- the game's shader ----\n");
    source.push_str(user);
    source.push_str("\n// ---- the engine's entry points ----\n");
    if !defines(user, "vertex") {
        source.push_str("fn vertex(v: Vertex) -> Vertex { return v; }\n");
    }
    if !defines(user, "surface") {
        source.push_str("fn surface(s: Surface) -> Surface { return s; }\n");
    }
    source.push_str(&MESH_MAIN.replace("// STATIC_INSTANCE_HELPER", include_str!("instance_vertex.wgsl")));
    source
}

/// The material shader a device with or without instance storage draws
/// with: the game's code `user` reading the last frame where the motion
/// pass asks it to (see `vertex_motion.rs`).
pub(super) fn compose_shader_storage(user: &str, decals: bool, gi: bool, storage: bool) -> String {
    compose_storage(&super::vertex_motion::with_past(user), decals, gi, storage)
}

/// [`compose_shader_storage`] with the game's code as it is.
fn compose_storage(user: &str, decals: bool, gi: bool, storage: bool) -> String {
    let source = compose_material(user, decals, gi);
    if storage {
        source
    } else {
        source.replace(
            include_str!("instance_vertex.wgsl"),
            "fn static_instance(v: VertexIn) -> VertexIn { return v; }",
        )
    }
}

/// A whole screen effect shader from the game's part: the mesh prelude
/// (bindings and helpers), `Pixel`, the game's `effect`, the entry points.
pub(crate) fn compose_effect(user: &str) -> String {
    let mut source =
        String::with_capacity(MESH_PRELUDE.len() + EFFECT_PRELUDE.len() + user.len() + EFFECT_MAIN.len() + 100);
    source.push_str(MESH_PRELUDE);
    source.push_str(EFFECT_PRELUDE);
    source.push_str("\n// ---- the game's effect ----\n");
    source.push_str(user);
    source.push_str("\n// ---- the engine's entry points ----\n");
    source.push_str(EFFECT_MAIN);
    source
}

/// A whole compute pass shader from the game's part: the mesh prelude
/// (bindings and helpers), `Cell`, the game's `compute`, the entry point.
pub(crate) fn compose_compute(user: &str) -> String {
    let mut source =
        String::with_capacity(MESH_PRELUDE.len() + COMPUTE_PRELUDE.len() + user.len() + COMPUTE_MAIN.len() + 100);
    source.push_str(MESH_PRELUDE);
    if defines(user, "update") {
        source.push_str("@group(3) @binding(0) var<storage, read_write> items: array<vec4<f32>>;\n");
    } else {
        source.push_str(COMPUTE_PRELUDE);
    }
    source.push_str("\n// ---- the game's compute pass ----\n");
    source.push_str(user);
    source.push_str("\n// ---- the engine's entry point ----\n");
    if defines(user, "update") {
        source.push_str("@compute @workgroup_size(64) fn cs_compute(@builtin(global_invocation_id) id: vec3<u32>) { if id.x < arrayLength(&items) { update(id.x); } }");
    } else {
        source.push_str(COMPUTE_MAIN);
    }
    source
}

/// Checks a composed shader, with an error message that points at the
/// line, as WGSL compilers print them.
#[cfg(test)]
pub(crate) fn check_shader(name: &str, source: &str) -> Result<(), String> {
    use wgpu::naga;
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string_with_path(source, name))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| e.emit_to_string_with_path(source, name))?;
    Ok(())
}

/// The variant the engine checks every material shader against first:
/// every optional part on.
pub(crate) const FULL_MATERIAL_VARIANT: &str = "material shader with decals, global illumination and instance storage";

pub(crate) const EFFECT_VARIANT: &str = "screen effect";

pub(crate) const COMPUTE_VARIANT: &str = "compute pass";

/// The material shader variant this device compiles.
pub(super) fn material_variant(decals: bool, gi: bool, storage: bool) -> String {
    let on = |b: bool| if b { "on" } else { "off" };
    format!(
        "material shader on this GPU: decals {}, global illumination {}, instance storage {}",
        on(decals),
        on(gi),
        on(storage)
    )
}

/// A material shader's pipelines: depth prepass, opaque, transparent and
/// shadow, each for one-sided and double-sided surfaces.
/// The pipeline of a compute pass.
pub(super) fn build_compute(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    name: &str,
    source: &str,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(name),
        layout: Some(layout),
        module: &module,
        entry_point: Some("cs_compute"),
        compilation_options: Default::default(),
        cache: None,
    })
}

/// The pipeline of a screen effect: a full-screen triangle into the HDR
/// frame, with the material pipelines' bindings.
pub(super) fn build_effect(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    name: &str,
    source: &str,
) -> (wgpu::ShaderModule, wgpu::RenderPipeline) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = effect_pipeline(device, layout, name, &module, "vs_effect", "fs_effect");
    (module, pipeline)
}

/// A full-screen pipeline of an effect shader's entry points, writing `HDR_FORMAT`.
pub(super) fn effect_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    name: &str,
    module: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(name),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(HDR_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// `source` is ready for the targets: through [`for_samples`].
#[allow(clippy::too_many_arguments)]
pub(super) fn build_pipelines(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shadow_layout: &wgpu::PipelineLayout,
    name: &str,
    source: &str,
    samples: u32,
    depth: wgpu::TextureFormat,
    counters: &Arc<pipeline_cache::Counters>,
    past: VertexPast,
) -> ShaderPipelines {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let builder = SideBuilder {
        device: device.clone(),
        module,
        layout: layout.clone(),
        shadow_layout: shadow_layout.clone(),
        samples,
        depth,
        past,
        crowd: false,
    };
    ShaderPipelines {
        sides: Sides::ALL.map(|sides| builder.timed_build(ShaderVariant::new(sides, Instancing::Plain), counters)),
        fade: Default::default(),
        shared: Default::default(),
        view_model_cutout: Default::default(),
        prepass_normals: Default::default(),
        builder,
        counters: counters.clone(),
        in_use: false,
        reads_scene: false,
        moves_vertices: false,
    }
}

/// What it takes to build a material shader's pipelines, kept to build the
/// crossfade variants when a frame first needs them.
#[derive(Clone)]
pub(super) struct SideBuilder {
    pub(super) device: wgpu::Device,
    pub(super) module: wgpu::ShaderModule,
    pub(super) layout: wgpu::PipelineLayout,
    pub(super) shadow_layout: wgpu::PipelineLayout,
    pub(super) samples: u32,
    /// The scene depth's format (shadow maps keep plain depth).
    pub(super) depth: wgpu::TextureFormat,
    /// What its `vertex` reads of the last frame: with anything, each set
    /// also has a still motion pipeline ([`Pipelines::velocity_still`]).
    pub(super) past: VertexPast,
    /// A crowd's set (see `crowds.rs`): a third vertex buffer brings each
    /// corner's joints and weights, the instance buffer is a zero-stride
    /// marker, and the motion vectors come from `vs_velocity_still` (the
    /// crowd's records as of the last frame), with no still variant.
    pub(super) crowd: bool,
}

impl SideBuilder {
    /// The pipeline of `role` in `variant` with a material's stencil state
    /// (see `stencil.rs`): the same as [`SideBuilder::build`]'s but for
    /// the stencil. Only the color roles have one (`None` for the others).
    pub(super) fn build_stencil(
        &self,
        variant: ShaderVariant,
        role: PipelineRole,
        stencil: wgpu::StencilState,
    ) -> Option<wgpu::RenderPipeline> {
        let (write, compare) = match role {
            PipelineRole::Opaque | PipelineRole::OpaqueCoverage => (false, wgpu::CompareFunction::Equal),
            PipelineRole::OpaqueSolid => (true, super::SCENE_DEPTH_NEARER),
            PipelineRole::Transparent => (false, super::SCENE_DEPTH_NEARER),
            _ => return None,
        };
        if role == PipelineRole::OpaqueCoverage && self.samples <= 1 {
            return None;
        }
        let shared = variant.instancing.shared();
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: if shared { 0 } else { size_of::<Instance3D>() as u64 },
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            }),
        ];
        let blend = (role == PipelineRole::Transparent).then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let targets = [Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        Some(self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d stencil material"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some(role.vertex_entry()),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: variant.sides.cull(),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil,
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                alpha_to_coverage_enabled: role == PipelineRole::OpaqueCoverage,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: role.fragment_entry(variant.instancing),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        }))
    }

    /// The view model's pipeline for cut-out held items in `variant`:
    /// drawn after the world with no prepass of their own, so they write
    /// their own depth (nearer, like [`PipelineRole::OpaqueSolid`]) minus
    /// the pixels their alpha cuts away: with multisampling through alpha
    /// to coverage (`fs_main_coverage`), else by discarding them
    /// (`fs_main_cutout`). `stencil`: a stencil material's state (the
    /// default for none).
    pub(super) fn build_view_model_cutout(
        &self,
        variant: ShaderVariant,
        stencil: wgpu::StencilState,
    ) -> wgpu::RenderPipeline {
        let coverage = self.samples > 1;
        let fade = variant.instancing.fades();
        let fragment = match (coverage, fade) {
            (true, _) => PipelineRole::OpaqueCoverage.fragment_entry(variant.instancing),
            (false, false) => Some("fs_main_cutout"),
            (false, true) => Some("fs_main_cutout_fade"),
        };
        let shared = variant.instancing.shared();
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: if shared { 0 } else { size_of::<Instance3D>() as u64 },
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            }),
        ];
        let targets = [Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d view model cut-out"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some(PipelineRole::OpaqueSolid.vertex_entry()),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: variant.sides.cull(),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(super::SCENE_DEPTH_NEARER),
                stencil,
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                alpha_to_coverage_enabled: coverage,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: fragment,
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The depth prepass of `variant` with the reflections' normals (see
    /// `fs_normals` in `mesh_main.wgsl`), for solid or `cutout` surfaces:
    /// the prepass's pipeline with a color target more, so the prepass writes each
    /// pixel's normal and roughness as it draws it, and no surface is drawn
    /// again for them. With multisampling the normals are the second
    /// target, after the coverage target cut-outs need (written by none);
    /// without, the only one. Nearer or equal: of surfaces at the same
    /// depth the last drawn leaves its normal, as the opaque pass (equal
    /// depths) leaves its color. A crowd's set writes no normal there.
    pub(super) fn build_prepass_normals(&self, variant: ShaderVariant, cutout: bool) -> wgpu::RenderPipeline {
        let shared = variant.instancing.shared() || self.crowd;
        let fade = variant.instancing.fades();
        let msaa = self.samples > 1;
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: if shared { 0 } else { size_of::<Instance3D>() as u64 },
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            }),
            self.crowd.then_some(super::crowds::INFLUENCE_LAYOUT),
        ];
        let buffers = if self.crowd { &buffers[..] } else { &buffers[..2] };
        let normals = Some(wgpu::ColorTargetState {
            format: crate::render::ssr::NORMALS_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        });
        let coverage = Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::empty(),
        });
        let targets_ms = [coverage, normals.clone()];
        let targets_one = [normals];
        let targets: &[Option<wgpu::ColorTargetState>] = if msaa { &targets_ms } else { &targets_one };
        let pick = |plain: &'static str, faded: &'static str| if fade { faded } else { plain };
        let (solid, cut) = match (self.crowd, msaa) {
            (false, false) => (
                pick("fs_normals", "fs_normals_fade"),
                pick("fs_depth_normals", "fs_depth_normals_fade"),
            ),
            (false, true) => (
                pick("fs_normals_ms", "fs_normals_ms_fade"),
                pick("fs_depth_coverage_normals", "fs_depth_coverage_normals_fade"),
            ),
            (true, false) => (
                pick("fs_no_normal", "fs_no_normal_fade"),
                pick("fs_depth_no_normal", "fs_depth_no_normal_fade"),
            ),
            (true, true) => (
                pick("fs_no_normal_ms", "fs_no_normal_ms_fade"),
                pick("fs_depth_coverage_no_normal", "fs_depth_coverage_no_normal_fade"),
            ),
        };
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if cutout {
                "3d depth prepass with normals, cut-out"
            } else {
                "3d depth prepass with normals"
            }),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some(PipelineRole::Prepass.vertex_entry()),
                compilation_options: Default::default(),
                buffers,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: variant.sides.cull(),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(super::SCENE_DEPTH_NEARER_OR_EQUAL),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: self.samples,
                // Cut-outs with multisampling: alpha to coverage, as
                // `PipelineRole::PrepassCoverage`.
                alpha_to_coverage_enabled: cutout && msaa,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: Some(if cutout { cut } else { solid }),
                compilation_options: Default::default(),
                targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// [`SideBuilder::build`], timed into `counters`.
    pub(super) fn timed_build(&self, variant: ShaderVariant, counters: &pipeline_cache::Counters) -> Pipelines {
        let start = crate::perf::Instant::now();
        let pipelines = self.build(variant);
        counters.built(start.elapsed());
        pipelines
    }

    /// The pipeline set of `variant`: its sides pick the culling, its
    /// instancing the instance buffer's stride and (crossfading) the
    /// dithering fragment entry points; each role's entry points come from
    /// [`PipelineRole`].
    pub(super) fn build(&self, variant: ShaderVariant) -> Pipelines {
        let cull = variant.sides.cull();
        let shared = variant.instancing.shared() || self.crowd;
        let vs = PipelineRole::vertex_entry;
        let fs = |role: PipelineRole| role.fragment_entry(variant.instancing);
        let (device, module, layout, shadow_layout, samples) = (
            &self.device,
            &self.module,
            &self.layout,
            &self.shadow_layout,
            self.samples,
        );
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: if shared { 0 } else { size_of::<Instance3D>() as u64 },
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &INSTANCE_ATTRIBUTES,
            }),
            self.crowd.then_some(super::crowds::INFLUENCE_LAYOUT),
        ];
        let buffers = if self.crowd { &buffers[..] } else { &buffers[..2] };
        let depth_format = self.depth;
        let depth = |write, compare, bias| {
            Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias,
            })
        };
        let msaa = wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        };
        let color = |blend| {
            [Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let opaque_target = color(None);
        let transparent_target = color(Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vertex: &str,
                        fragment: Option<&str>,
                        cull: Option<wgpu::Face>,
                        depth_stencil,
                        multisample,
                        targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: cull,
                    ..Default::default()
                },
                depth_stencil,
                multisample,
                fragment: fragment.map(|fragment| wgpu::FragmentState {
                    module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Moved models: only the motion, wherever the opaque pass drew them.
        let moved_buffers = [
            buffers[0].clone(),
            Some(wgpu::VertexBufferLayout {
                array_stride: if shared { 0 } else { size_of::<MovedInstance>() as u64 },
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &MOVED_ATTRIBUTES,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &PREVIOUS_POSITION_ATTRIBUTES,
            }),
        ];
        let velocity_target = [Some(wgpu::ColorTargetState {
            format: super::taa::VELOCITY_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let velocity = || {
            if self.crowd {
                pipeline(
                    "3d crowd motion vectors",
                    layout,
                    "vs_velocity_still",
                    fs(PipelineRole::Velocity),
                    cull,
                    depth(false, wgpu::CompareFunction::Equal, Default::default()),
                    msaa,
                    &velocity_target,
                )
            } else {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("3d motion vectors"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some(vs(PipelineRole::Velocity)),
                        compilation_options: Default::default(),
                        buffers: &moved_buffers,
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: cull,
                        ..Default::default()
                    },
                    depth_stencil: depth(false, wgpu::CompareFunction::Equal, Default::default()),
                    multisample: msaa,
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: fs(PipelineRole::Velocity),
                        compilation_options: Default::default(),
                        targets: &velocity_target,
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            }
        };
        let shadow_bias = wgpu::DepthBiasState {
            constant: 2,
            slope_scale: 2.0,
            clamp: 0.0,
        };
        // Solid surfaces write depth with no fragment shader at all, so the
        // GPU can reject hidden ones early; cut-out ones run it for the alpha.
        let prepass = |role: PipelineRole| {
            pipeline(
                "3d depth prepass",
                layout,
                vs(role),
                fs(role),
                cull,
                depth(true, super::SCENE_DEPTH_NEARER, Default::default()),
                msaa,
                &[],
            )
        };
        // Both faces cast: thin things (leaves, planes) too. The stored depth
        // is pushed back a little, against shadow acne.
        let shadow = |role: PipelineRole| {
            pipeline(
                "3d shadow",
                shadow_layout,
                vs(role),
                fs(role),
                None,
                Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: shadow_bias,
                }),
                wgpu::MultisampleState::default(),
                &[],
            )
        };
        let coverage_msaa = wgpu::MultisampleState {
            alpha_to_coverage_enabled: true,
            ..msaa
        };
        let unwritten = [Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::empty(),
        })];
        let prepass_coverage = || {
            pipeline(
                "3d depth prepass coverage",
                layout,
                vs(PipelineRole::PrepassCoverage),
                fs(PipelineRole::PrepassCoverage),
                cull,
                depth(true, super::SCENE_DEPTH_NEARER, Default::default()),
                coverage_msaa,
                &unwritten,
            )
        };
        let opaque_coverage = || {
            pipeline(
                "3d opaque coverage",
                layout,
                vs(PipelineRole::OpaqueCoverage),
                fs(PipelineRole::OpaqueCoverage),
                cull,
                depth(false, wgpu::CompareFunction::Equal, Default::default()),
                coverage_msaa,
                &opaque_target,
            )
        };
        // Only the fragments the prepass found nearest: each pixel is lit once.
        let opaque = || {
            pipeline(
                "3d opaque",
                layout,
                vs(PipelineRole::Opaque),
                fs(PipelineRole::Opaque),
                cull,
                depth(false, wgpu::CompareFunction::Equal, Default::default()),
                msaa,
                &opaque_target,
            )
        };
        let opaque_solid = || {
            pipeline(
                "3d opaque solid",
                layout,
                vs(PipelineRole::OpaqueSolid),
                fs(PipelineRole::OpaqueSolid),
                cull,
                depth(true, super::SCENE_DEPTH_NEARER, Default::default()),
                msaa,
                &opaque_target,
            )
        };
        // Over the opaque frame, without hiding what's behind it.
        let transparent = || {
            pipeline(
                "3d transparent",
                layout,
                vs(PipelineRole::Transparent),
                fs(PipelineRole::Transparent),
                cull,
                depth(false, super::SCENE_DEPTH_NEARER, Default::default()),
                msaa,
                &transparent_target,
            )
        };
        // Models that hold still but whose `vertex` moves them: drawn
        // from the frame's own instances, `vertex` run as of this frame
        // and the last (see `vertex_motion.rs`).
        let velocity_still = || {
            pipeline(
                "3d still motion vectors",
                layout,
                "vs_velocity_still",
                fs(PipelineRole::Velocity),
                cull,
                depth(false, wgpu::CompareFunction::Equal, Default::default()),
                msaa,
                &velocity_target,
            )
        };
        let (prepass_plain, prepass_cutout) = (
            || prepass(PipelineRole::Prepass),
            || prepass(PipelineRole::PrepassCutout),
        );
        let (shadow_plain, shadow_cutout) = (|| shadow(PipelineRole::Shadow), || shadow(PipelineRole::ShadowCutout));
        let mut fixed = [
            &prepass_plain as Job,
            &prepass_cutout,
            &opaque,
            &opaque_solid,
            &transparent,
            &shadow_plain,
            &shadow_cutout,
            &velocity,
        ]
        .map(AtOnce::new);
        let mut coverage = (samples > 1).then(|| [&prepass_coverage as Job, &opaque_coverage].map(AtOnce::new));
        let mut still = (self.past.any() && !self.crowd).then(|| AtOnce::new(&velocity_still));
        build_at_once(
            device,
            fixed
                .iter_mut()
                .chain(coverage.iter_mut().flatten())
                .chain(still.as_mut())
                .collect(),
        );
        let [
            prepass,
            prepass_cutout,
            opaque,
            opaque_solid,
            transparent,
            shadow,
            shadow_cutout,
            velocity,
        ] = fixed.map(AtOnce::take);
        Pipelines {
            coverage: coverage.map(|pair| pair.map(AtOnce::take)),
            prepass,
            prepass_cutout,
            opaque,
            opaque_solid,
            transparent,
            shadow,
            shadow_cutout,
            velocity,
            velocity_still: still.map(AtOnce::take),
        }
    }
}

/// One pipeline to build together with others ([`build_at_once`]).
#[cfg(not(target_arch = "wasm32"))]
type Job<'a> = &'a (dyn Fn() -> wgpu::RenderPipeline + Sync + 'a);
#[cfg(target_arch = "wasm32")]
type Job<'a> = &'a (dyn Fn() -> wgpu::RenderPipeline + 'a);

/// A pipeline's job, and the pipeline once [`build_at_once`] built it.
struct AtOnce<'a> {
    job: Job<'a>,
    built: Option<wgpu::RenderPipeline>,
}

impl<'a> AtOnce<'a> {
    fn new(job: Job<'a>) -> Self {
        Self { job, built: None }
    }

    /// The pipeline: as built at once, or else built now on this thread,
    /// where the caller's error scope sees what the GPU says about it.
    fn take(self) -> wgpu::RenderPipeline {
        self.built.unwrap_or_else(|| (self.job)())
    }
}

/// Builds a set's pipelines at once on the frame pool. Each pipeline's
/// shaders are compiled on the CPU when it is created, and a set has about
/// ten: on Direct3D 12 the built-in shader's three plain sets took 4 s one
/// after the other with FXC (9 s with a release build's optimizing flags)
/// on a 6-core desktop. Built at once they take about half as long with
/// DXC or on Vulkan; FXC gains less, as it mostly compiles one shader at a
/// time. GL and the browser build them on this thread (`AtOnce::take`
/// builds whatever wasn't built here).
///
/// wgpu's error scopes are per thread, so each pipeline is built in its
/// own: one the GPU rejects is dropped and built again by `take` on the
/// calling thread, where the caller's scope (a game shader's check) or the
/// device's handler sees the error as before.
#[cfg(not(target_arch = "wasm32"))]
fn build_at_once(device: &wgpu::Device, mut builds: Vec<&mut AtOnce<'_>>) {
    use rayon::prelude::*;
    if builds.len() < 2 || matches!(device.adapter_info().backend, wgpu::Backend::Gl) {
        return;
    }
    crate::frame_pool::run(|| {
        builds.par_iter_mut().for_each(|build| {
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let pipeline = (build.job)();
            if matches!(end_scope(scope, false), Ok(None)) {
                build.built = Some(pipeline);
            }
        });
    });
}

#[cfg(target_arch = "wasm32")]
fn build_at_once(_device: &wgpu::Device, _builds: Vec<&mut AtOnce<'_>>) {}
