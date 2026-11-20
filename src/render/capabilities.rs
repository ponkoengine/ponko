//! What the GPU this game runs on can do, and what the renderer does where
//! it can't: one machine-readable report, [`GpuCapabilities`].
//!
//! The renderer's own feature checks live here too (`DeviceSupport`), so
//! the report and the passes the renderer actually builds can't disagree.
//! The report types are plain data (no GPU types inside), so they build
//! without the `window` feature and serialize to JSON for tools.

use serde::{Deserialize, Serialize};

/// What this GPU can do: the adapter, its main limits, and for every
/// renderer feature that depends on the device, whether it's available and
/// what the renderer does instead when it isn't.
///
/// From [`Graphics::gpu_capabilities`](crate::gfx::Graphics::gpu_capabilities). It is
/// decided once, when the GPU is opened, and doesn't change while the game
/// runs. Serializes to JSON (`serde_json::to_value`) for tools.
///
/// ```
/// # use ponko::prelude::*;
/// fn report(ctx: &mut Ctx) {
///     if let Some(caps) = ctx.gfx.gpu_capabilities() {
///         for f in caps.features.iter().filter(|f| !f.available) {
///             println!("{:?}: {} ({})", f.feature, f.fallback.as_deref().unwrap_or("-"), f.reason.as_deref().unwrap_or("-"));
///         }
///     }
/// }
/// # Game::new("t", 1, 1).system(report).run_headless(1, |_| {});
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuCapabilities {
    /// The GPU the renderer opened.
    pub adapter: GpuAdapter,
    /// The device's limits that the renderer's choices depend on.
    pub limits: GpuLimits,
    /// One entry per [`GpuFeature`], in declaration order.
    pub features: Vec<GpuFeatureStatus>,
    /// The [`GpuTier`] these limits reach: the floor of what this GPU
    /// draws. The `features` entries are the exact answer and may show more.
    #[serde(default)]
    pub tier: GpuTier,
    /// The tier forced with `PONKO_GPU_TIER`, if any: the device was then
    /// opened with that tier's limits instead of the ones the engine asks
    /// for, to try a game on a weaker GPU (see [`GpuTier::forced`]).
    #[serde(default)]
    pub forced_tier: Option<GpuTier>,
    /// What these limits allow for instance batches: whether they can be
    /// GPU-driven, from how many instances they are, and how many instances
    /// a batch can hold on each path.
    #[serde(default)]
    pub instance_batches: GpuBatchLimits,
}

impl GpuCapabilities {
    /// The entry for `feature`.
    ///
    /// ```
    /// use ponko::render::{GpuFeature, GpuTier};
    /// let caps = GpuTier::WebGl2.report();
    /// let taa = caps.feature(GpuFeature::TemporalAntialiasing).unwrap();
    /// assert!(!taa.available);
    /// assert!(taa.fallback.is_some() && taa.reason.is_some());
    /// ```
    #[must_use]
    pub fn feature(&self, feature: GpuFeature) -> Option<&GpuFeatureStatus> {
        self.features.iter().find(|f| f.feature == feature)
    }

    /// Whether `feature` is available on this GPU.
    ///
    /// ```
    /// use ponko::render::{GpuFeature, GpuTier};
    /// // Draw an effect's settings only where it can run.
    /// let caps = GpuTier::Baseline.report();
    /// assert!(caps.available(GpuFeature::Compute));
    /// assert!(!GpuTier::WebGl2.report().available(GpuFeature::Compute));
    /// ```
    #[must_use]
    pub fn available(&self, feature: GpuFeature) -> bool {
        self.feature(feature).is_some_and(|f| f.available)
    }
}

/// The GPU the renderer opened, as the graphics driver names it.
///
/// ```
/// // A report built without a GPU (`GpuTier::report`) has an empty adapter.
/// let adapter = ponko::render::GpuTier::Baseline.report().adapter;
/// assert_eq!(adapter, ponko::render::GpuAdapter::default());
/// assert!(adapter.name.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuAdapter {
    /// The adapter's name, such as the GPU model.
    pub name: String,
    /// The graphics API: `vulkan`, `metal`, `dx12`, `gl` (WebGL2 in a
    /// browser), `webgpu` (the browser's WebGPU) or `noop`.
    pub backend: String,
    /// `discrete_gpu`, `integrated_gpu`, `virtual_gpu`, `cpu` or `other`.
    pub device_type: String,
    /// The driver's name, where the backend reports it (may be empty).
    pub driver: String,
    /// The driver's version details, where the backend reports them (may be empty).
    pub driver_info: String,
}

/// The device limits the renderer's feature choices depend on.
///
/// ```
/// use ponko::render::GpuTier;
/// // WebGL2 has no compute shaders and no storage buffers.
/// let webgl2 = GpuTier::WebGl2.minimum_limits();
/// assert_eq!(webgl2.max_compute_workgroups_per_dimension, 0);
/// assert_eq!(webgl2.max_storage_buffers_per_shader_stage, 0);
/// // Decals need 17 sampled textures, global illumination 18.
/// assert!(GpuTier::Desktop.minimum_limits().max_sampled_textures_per_shader_stage >= 18);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuLimits {
    /// Largest texture width or height.
    pub max_texture_dimension_2d: u32,
    /// Most layers in a texture array.
    pub max_texture_array_layers: u32,
    /// Sampled textures one shader stage may read (decals need 17, global
    /// illumination 18).
    pub max_sampled_textures_per_shader_stage: u32,
    /// Storage buffers one shader stage may use (0 on WebGL2).
    pub max_storage_buffers_per_shader_stage: u32,
    /// Storage textures one shader stage may use (0 on WebGL2).
    pub max_storage_textures_per_shader_stage: u32,
    /// Largest storage buffer binding, in bytes.
    pub max_storage_buffer_binding_size: u64,
    /// Most compute workgroups per dispatch dimension (0: no compute shaders).
    pub max_compute_workgroups_per_dimension: u32,
    /// Largest buffer, in bytes (0 in a report made before this was listed).
    #[serde(default)]
    pub max_buffer_size: u64,
}

/// What the device's limits allow for instance batches
/// (`InstanceBatch`): [`GpuCapabilities::instance_batches`].
///
/// A batch is *GPU-driven* when the device has compute culling and the
/// batch has one distance level and at least `min_gpu_instances` instances
/// (or what `BatchCulling` asks): one compute dispatch
/// culls every such batch per instance, and each is drawn with indirect
/// draws. Every other batch is culled on the CPU, cell by cell, and drawn
/// with direct draws. Both paths draw the same picture. A batch past a
/// limit falls back (or, past `max_cpu_instances`, is not drawn) and logs
/// it once; [`RenderStats::instance_batches`](crate::render::RenderStats)
/// counts the batches on each path while the game runs.
///
/// ```
/// let floor = ponko::render::GpuTier::Baseline.report().instance_batches;
/// assert!(floor.gpu_driven);
/// assert!(floor.max_gpu_instances >= 1_000_000);
/// assert!(!ponko::render::GpuTier::WebGl2.report().instance_batches.gpu_driven);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuBatchLimits {
    /// Whether batches can be GPU-driven (the same answer as
    /// [`GpuFeature::ComputeCulling`]).
    pub gpu_driven: bool,
    /// Batches with at least this many instances are GPU-driven where
    /// `gpu_driven` is set. Smaller ones are culled on the CPU, unless
    /// `min_gpu_triangles` sends them to the GPU: below this crossover
    /// (measured on one Apple GPU on Metal) that is cheaper. Batches of
    /// several distance levels skip this rule and `min_gpu_triangles`:
    /// they stay on the CPU at any size (see `min_gpu_triangles`).
    pub min_gpu_instances: u32,
    /// Smaller batches are GPU-driven all the same when their instances
    /// times the triangles of their heaviest mesh reach this: culling
    /// heavy meshes one by one saves more drawing than the compute pass
    /// costs. Measured on the same one GPU, with single-level batches only;
    /// a batch can overrule both with
    /// `BatchCulling`.
    ///
    /// Batches of several distance levels skip both rules under
    /// `BatchCulling::Auto` and stay on the CPU path, however many
    /// instances and triangles they have. The evidence is one scene of
    /// three levels with a 480-triangle top level, where the GPU path was
    /// slower at every size up to a million instances; heavier level
    /// meshes were not measured. `BatchCulling::PreferGpu` sends a level
    /// batch of heavy meshes to the GPU.
    #[serde(default)]
    pub min_gpu_triangles: u64,
    /// A GPU-driven batch of up to this many instances always fits one
    /// storage buffer binding (0 without `gpu_driven`). A larger batch may
    /// be culled on the CPU instead.
    pub max_gpu_instances: u64,
    /// The most instances a batch culled on the CPU holds: one vertex
    /// buffer's worth. A larger batch is not drawn; split it.
    pub max_cpu_instances: u64,
    /// The most cells (groups of up to 256 neighbouring instances) one
    /// culling dispatch takes, over all batches, views and distance
    /// levels of a frame (0 without `gpu_driven`).
    pub max_cull_cells: u64,
}

/// Batches with fewer instances than this are culled on the CPU even where
/// the device has compute culling. Having any GPU-driven batch costs a
/// frame a compute pass and its table (a fixed cost), and each such batch
/// a bind group and an indirect draw per level and pass; CPU culling costs
/// by the cell. Measured on an offscreen benchmark, on one Apple GPU on
/// Metal and nowhere else: see `crates/ponko_bench/STATIC_BATCHES.md` for
/// the numbers and the command. Other GPUs may cross over elsewhere.
pub(crate) const GPU_BATCH_MIN: u32 = 131_072;

/// Smaller batches are GPU-driven when instances times the triangles of
/// their heaviest mesh reach this. The CPU path culls by the cell, so it
/// draws the instances of a cell that are outside the view; with meshes of
/// thousands of triangles that costs the GPU more than the compute pass
/// does. Measured in the same benchmark (the "spread" rows): the paths
/// cross between 8 and 20 million instance-triangles.
pub(crate) const GPU_BATCH_TRIANGLES: u64 = 1 << 24;

/// Bytes a GPU-driven batch needs per instance at most: its record (six
/// vec4s), and a cell (two vec4s) when every instance has its own.
const BATCH_RECORD_BYTES: u64 = 96 + 32;
/// Bytes of an instance on the CPU path's vertex buffer (five vec4s).
const BATCH_VERTEX_BYTES: u64 = 80;

/// A renderer feature that depends on what the GPU can do.
///
/// New features may be added; match with a wildcard arm.
///
/// ```
/// use ponko::render::GpuFeature;
/// assert!(GpuFeature::ALL.contains(&GpuFeature::ScreenSpaceReflections));
/// assert!(GpuFeature::Decals.is_3d());
/// assert!(!GpuFeature::Compute.is_3d());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuFeature {
    /// GPU timestamp queries: per-pass GPU times and `RenderStats::gpu_ms`.
    Timestamps,
    /// Compute shaders and storage textures, which the effects below build on.
    Compute,
    /// Storage buffers in shaders: `GpuBuffer`s and material shaders that
    /// read `items`.
    StorageBuffers,
    /// Static instance batches culled per instance by a compute pass.
    ComputeCulling,
    /// Skinned and morphed meshes posed by a compute pass.
    GpuSkinning,
    /// Hiding models behind others with the previous frame's depth.
    OcclusionCulling,
    /// Temporal antialiasing (`AntiAliasing::Taa`).
    TemporalAntialiasing,
    /// Ground-truth ambient occlusion.
    AmbientOcclusion,
    /// Screen-space contact shadows.
    ContactShadows,
    /// Screen-space reflections.
    ScreenSpaceReflections,
    /// Probe-based global illumination.
    GlobalIllumination,
    /// Projected decals (`Decal3D`).
    Decals,
    /// The game's multisampling count for the 3D frame.
    Multisampling,
}

impl GpuFeature {
    /// Every feature, in declaration order (the report's order).
    pub const ALL: [GpuFeature; 13] = [
        GpuFeature::Timestamps,
        GpuFeature::Compute,
        GpuFeature::StorageBuffers,
        GpuFeature::ComputeCulling,
        GpuFeature::GpuSkinning,
        GpuFeature::OcclusionCulling,
        GpuFeature::TemporalAntialiasing,
        GpuFeature::AmbientOcclusion,
        GpuFeature::ContactShadows,
        GpuFeature::ScreenSpaceReflections,
        GpuFeature::GlobalIllumination,
        GpuFeature::Decals,
        GpuFeature::Multisampling,
    ];

    /// Whether the feature belongs to the 3D renderer (the `3d` Cargo feature).
    ///
    /// ```
    /// use ponko::render::GpuFeature;
    /// let only_3d: Vec<_> = GpuFeature::ALL.iter().filter(|f| f.is_3d()).collect();
    /// assert_eq!(only_3d.len(), GpuFeature::ALL.len() - 2);
    /// ```
    #[must_use]
    pub fn is_3d(self) -> bool {
        !matches!(self, GpuFeature::Timestamps | GpuFeature::Compute)
    }
}

/// Whether one [`GpuFeature`] is available, and if not, what the renderer
/// does instead and why.
///
/// ```
/// use ponko::render::{GpuFeature, GpuTier};
/// for status in GpuTier::WebGl2.report().features.iter().filter(|s| !s.available) {
///     // Every unavailable feature names its fallback and its reason.
///     assert!(status.fallback.is_some() && status.reason.is_some(), "{:?}", status.feature);
/// }
/// let compute = GpuTier::Desktop.report().feature(GpuFeature::Compute).unwrap().clone();
/// assert!(compute.available && compute.fallback.is_none() && compute.reason.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuFeatureStatus {
    /// The feature.
    pub feature: GpuFeature,
    /// Whether the renderer uses it where the game asks for it.
    pub available: bool,
    /// What the renderer does instead (`None` when available).
    pub fallback: Option<String>,
    /// Why it isn't available: the missing GPU feature or limit (`None`
    /// when available).
    pub reason: Option<String>,
}

/// A class of GPU, from the weakest the engine runs on to a desktop GPU:
/// what every device of the class can draw. [`GpuCapabilities::tier`] is
/// the tier a device's limits reach.
///
/// | | `WebGl2` | `Baseline` | `Desktop` |
/// |---|---|---|---|
/// | Typical device | WebGL2 in a browser, GLES 3.0 | WebGPU in a browser, GLES 3.1, D3D11-class, mobile | Vulkan, Metal, D3D12 desktop and laptop GPUs |
/// | Floor ([`minimum_limits`](Self::minimum_limits)) | wgpu's `downlevel_webgl2_defaults` | wgpu's `downlevel_defaults` | WebGPU's defaults with 18 sampled textures |
/// | Compute passes, storage buffers, per-instance culling, occlusion culling | no | yes | yes |
/// | TAA, ambient occlusion, contact shadows, screen-space reflections | no (FXAA, off) | yes | yes |
/// | GPU skinning (6 storage buffers) | no (CPU) | where the device has them | yes |
/// | Decals (17 sampled textures), global illumination (18) | no | where the device has them | yes |
/// | GPU timestamps | device-dependent | device-dependent | device-dependent |
///
/// A tier is a floor: a device may have more than its tier's minimum, and
/// the per-feature entries of [`GpuCapabilities`] are the exact answer.
/// Nothing in the table fails on a weaker tier: each unavailable feature
/// falls back as its [`GpuFeatureStatus`] says.
/// `SceneSettings::set_recommended_quality`
/// picks a preset per tier.
///
/// ```
/// use ponko::prelude::*;
/// let floor = ponko::render::GpuTier::Baseline.report();
/// assert!(floor.available(ponko::render::GpuFeature::Compute));
/// assert!(!floor.available(ponko::render::GpuFeature::Decals));
/// assert_eq!(ponko::render::GpuTier::of(&floor.limits), ponko::render::GpuTier::Baseline);
/// assert_eq!("webgl2".parse(), Ok(ponko::render::GpuTier::WebGl2));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u32)]
pub enum GpuTier {
    /// No compute shaders or storage: WebGL2 and every device below
    /// [`Baseline`](Self::Baseline). Everything draws, with the fallbacks.
    #[default]
    #[serde(rename = "webgl2")]
    WebGl2 = 0,
    /// Compute shaders, storage textures and at least four storage buffers:
    /// every screen-space effect and GPU culling.
    #[serde(rename = "baseline")]
    Baseline = 1,
    /// Also six storage buffers (GPU skinning) and 18 sampled textures per
    /// shader stage (decals and global illumination): every feature.
    #[serde(rename = "desktop")]
    Desktop = 2,
}

impl GpuTier {
    /// Every tier, weakest first.
    pub const ALL: [GpuTier; 3] = [GpuTier::WebGl2, GpuTier::Baseline, GpuTier::Desktop];

    /// The tier's name: `webgl2`, `baseline` or `desktop` (also its JSON
    /// form and what `PONKO_GPU_TIER` takes).
    ///
    /// ```
    /// use ponko::render::GpuTier;
    /// assert_eq!(GpuTier::WebGl2.name(), "webgl2");
    /// assert_eq!(GpuTier::Desktop.to_string(), "desktop");
    /// ```
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            GpuTier::WebGl2 => "webgl2",
            GpuTier::Baseline => "baseline",
            GpuTier::Desktop => "desktop",
        }
    }

    /// The highest tier whose minimum `limits` meet.
    ///
    /// ```
    /// use ponko::render::GpuTier;
    /// for tier in GpuTier::ALL {
    ///     assert_eq!(GpuTier::of(&tier.minimum_limits()), tier);
    /// }
    /// ```
    #[must_use]
    pub fn of(limits: &GpuLimits) -> GpuTier {
        if !limits.compute_culling() {
            GpuTier::WebGl2
        } else if limits.gpu_skinning() && limits.gi() {
            GpuTier::Desktop
        } else {
            GpuTier::Baseline
        }
    }

    /// The least a device of this tier has (see the table above).
    ///
    /// ```
    /// use ponko::render::GpuTier;
    /// let floor = GpuTier::Baseline.minimum_limits();
    /// assert!(floor.max_storage_buffers_per_shader_stage >= 4);
    /// assert!(floor.max_texture_dimension_2d >= 2048);
    /// ```
    #[must_use]
    pub fn minimum_limits(self) -> GpuLimits {
        match self {
            GpuTier::WebGl2 => GpuLimits {
                max_texture_dimension_2d: 2048,
                max_texture_array_layers: 256,
                max_sampled_textures_per_shader_stage: 16,
                max_storage_buffers_per_shader_stage: 0,
                max_storage_textures_per_shader_stage: 0,
                max_storage_buffer_binding_size: 0,
                max_compute_workgroups_per_dimension: 0,
                max_buffer_size: 256 << 20,
            },
            GpuTier::Baseline => GpuLimits {
                max_texture_dimension_2d: 2048,
                max_texture_array_layers: 256,
                max_sampled_textures_per_shader_stage: 16,
                max_storage_buffers_per_shader_stage: CULLING_STORAGE_BUFFERS,
                max_storage_textures_per_shader_stage: 4,
                max_storage_buffer_binding_size: 128 << 20,
                max_compute_workgroups_per_dimension: 65535,
                max_buffer_size: 256 << 20,
            },
            GpuTier::Desktop => GpuLimits {
                max_texture_dimension_2d: 8192,
                max_texture_array_layers: 256,
                max_sampled_textures_per_shader_stage: DECAL_SAMPLED_TEXTURES + 1,
                max_storage_buffers_per_shader_stage: 8,
                max_storage_textures_per_shader_stage: 4,
                max_storage_buffer_binding_size: 128 << 20,
                max_compute_workgroups_per_dimension: 65535,
                max_buffer_size: 256 << 20,
            },
        }
    }

    /// The report a device with exactly this tier's
    /// [`minimum_limits`](Self::minimum_limits) gives (no timestamp
    /// queries, 4x multisampling, an empty adapter): the features the tier
    /// guarantees, and for the others the fallback and the reason. Built
    /// without a GPU, so tools and docs can list them.
    ///
    /// ```
    /// use ponko::render::{GpuFeature, GpuTier};
    /// let caps = GpuTier::WebGl2.report();
    /// assert_eq!(caps.tier, GpuTier::WebGl2);
    /// assert!(!caps.available(GpuFeature::AmbientOcclusion));
    /// // The report serializes to JSON for tools.
    /// assert_eq!(serde_json::to_value(&caps).unwrap()["tier"], "webgl2");
    /// ```
    #[must_use]
    pub fn report(self) -> GpuCapabilities {
        report(
            GpuAdapter::default(),
            self.minimum_limits(),
            false,
            (4, 4),
            cfg!(feature = "3d"),
        )
    }

    /// The features a device of this tier may lack, in report order:
    /// the unavailable entries of [`report`](Self::report). GPU timestamps
    /// are listed at every tier: they depend on the device, not the tier.
    ///
    /// ```
    /// use ponko::render::{GpuFeature, GpuTier};
    /// assert!(GpuTier::WebGl2.unavailable_features().contains(&GpuFeature::Compute));
    /// assert!(!GpuTier::Desktop.unavailable_features().contains(&GpuFeature::Compute));
    /// ```
    #[must_use]
    pub fn unavailable_features(self) -> Vec<GpuFeature> {
        self.report()
            .features
            .iter()
            .filter(|f| !f.available)
            .map(|f| f.feature)
            .collect()
    }

    /// The tier `PONKO_GPU_TIER` forces (`webgl2`, `baseline` or
    /// `desktop`), if set. With `webgl2` or `baseline`, the engine opens the
    /// GPU with that tier's limits (except the largest 2D texture, kept so
    /// the window's surface fits) and without timestamp queries, so the
    /// game draws as on such a device (with this GPU's backend: it doesn't
    /// switch to WebGL2 itself). An unknown name is ignored with a warning.
    /// Always `None` on the web.
    ///
    /// ```
    /// use ponko::render::GpuTier;
    /// // Run the game's own checks against the tier it was asked to try.
    /// let tier = GpuTier::forced().unwrap_or(GpuTier::Desktop);
    /// assert!(GpuTier::ALL.contains(&tier));
    /// ```
    #[must_use]
    pub fn forced() -> Option<GpuTier> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let value = std::env::var("PONKO_GPU_TIER").ok()?;
        if value.is_empty() {
            return None;
        }
        match value.parse() {
            Ok(tier) => Some(tier),
            Err(e) => {
                eprintln!("ponko: {e}; ignoring PONKO_GPU_TIER");
                None
            }
        }
    }
}

impl std::fmt::Display for GpuTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for GpuTier {
    type Err = String;

    /// `webgl2`, `baseline` or `desktop` (any case).
    fn from_str(s: &str) -> Result<Self, String> {
        GpuTier::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown GPU tier {s:?}: use webgl2, baseline or desktop"))
    }
}

#[cfg(feature = "window")]
impl GpuTier {
    /// The limits to open a device with when this tier is forced, given the
    /// ones that would be asked for: each no better than the tier's floor
    /// (and never raised: a request below the floor stays as it is);
    /// `requested` itself for `Desktop`.
    pub(crate) fn device_limits(self, requested: wgpu::Limits) -> wgpu::Limits {
        match self {
            GpuTier::WebGl2 => requested.or_worse_values_from(&wgpu::Limits::downlevel_webgl2_defaults()),
            GpuTier::Baseline => requested.or_worse_values_from(&wgpu::Limits::downlevel_defaults()),
            GpuTier::Desktop => requested,
        }
    }

    /// The limits the engine opens its window's device with when this tier
    /// is forced: [`device_limits`](Self::device_limits), but keeping the
    /// requested largest 2D texture. The window's surface and its
    /// window-sized targets are as large as the window in physical pixels
    /// (a 1280x720 window on a 2x display is 2560x1440), more than the
    /// tiers' 2048 floor; a real device of the tier draws such a window
    /// because it has that much. Every feature check is still the tier's.
    pub(crate) fn window_limits(self, requested: wgpu::Limits) -> wgpu::Limits {
        let max_texture_dimension_2d = requested.max_texture_dimension_2d;
        wgpu::Limits {
            max_texture_dimension_2d,
            ..self.device_limits(requested)
        }
    }

    /// The optional device features to keep when this tier is forced:
    /// below `Desktop`, no timestamp queries.
    pub(crate) fn device_features(self, engine: wgpu::Features) -> wgpu::Features {
        match self {
            GpuTier::Desktop => engine,
            _ => engine - wgpu::Features::TIMESTAMP_QUERY,
        }
    }
}

/// Sampled textures a fragment shader needs for the mesh pipelines with
/// decals: the 16 WebGL2 and WebGPU's defaults allow, and the decal images.
pub(crate) const DECAL_SAMPLED_TEXTURES: u32 = 17;
/// Storage buffers the per-instance culling pass binds at once.
const CULLING_STORAGE_BUFFERS: u32 = 4;
/// Storage buffers the skinning pass binds at once (see `skinning.rs`).
const SKINNING_STORAGE_BUFFERS: u32 = 6;

impl GpuLimits {
    /// Whether the device runs compute passes: WebGL2 has no compute
    /// shaders or storage textures.
    pub(crate) fn compute(&self) -> bool {
        self.max_compute_workgroups_per_dimension > 0 && self.max_storage_textures_per_shader_stage > 0
    }

    /// Whether shaders can read and write storage buffers.
    pub(crate) fn storage_buffers(&self) -> bool {
        self.compute() && self.max_storage_buffers_per_shader_stage >= 2
    }

    /// Whether the per-instance culling pass can bind its buffers.
    pub(crate) fn compute_culling(&self) -> bool {
        self.compute() && self.max_storage_buffers_per_shader_stage >= CULLING_STORAGE_BUFFERS
    }

    /// What these limits allow for instance batches.
    pub(crate) fn instance_batches(&self) -> GpuBatchLimits {
        let gpu_driven = self.compute_culling();
        // 128 bytes an instance is the most a batch's records and cells
        // take (a cell per instance); 64 KiB covers the block's header and
        // rounding to the binding alignment.
        let binding = self.max_storage_buffer_binding_size.min(self.max_buffer_size);
        let side = u64::from(self.max_compute_workgroups_per_dimension.min(256));
        GpuBatchLimits {
            gpu_driven,
            min_gpu_instances: GPU_BATCH_MIN,
            min_gpu_triangles: GPU_BATCH_TRIANGLES,
            max_gpu_instances: if gpu_driven {
                binding.saturating_sub(64 << 10) / BATCH_RECORD_BYTES
            } else {
                0
            },
            max_cpu_instances: self.max_buffer_size / BATCH_VERTEX_BYTES,
            max_cull_cells: if gpu_driven {
                side * side * u64::from(self.max_compute_workgroups_per_dimension)
            } else {
                0
            },
        }
    }

    /// Whether the skinning pass can bind its buffers.
    pub(crate) fn gpu_skinning(&self) -> bool {
        self.compute() && self.max_storage_buffers_per_shader_stage >= SKINNING_STORAGE_BUFFERS
    }

    /// Whether the mesh shaders have a texture slot to spare for decals.
    pub(crate) fn decals(&self) -> bool {
        self.max_sampled_textures_per_shader_stage >= DECAL_SAMPLED_TEXTURES
    }

    /// Whether global illumination fits: compute shaders, and a texture
    /// slot past the decals' for the probes (the 18th).
    pub(crate) fn gi(&self) -> bool {
        self.compute() && self.max_sampled_textures_per_shader_stage > DECAL_SAMPLED_TEXTURES
    }

    /// Whether the opaque pass has a texture slot past the decals' and
    /// global illumination's for the screen-space reflections' half-size
    /// spread image (the 18th, or the 19th with global illumination).
    /// Without one, a full-size pass copies it into the image the opaque
    /// pass reads the reflections from (see `ssr.rs`).
    #[cfg(feature = "3d")]
    pub(crate) fn reflection_spread(&self) -> bool {
        self.max_sampled_textures_per_shader_stage > DECAL_SAMPLED_TEXTURES + u32::from(self.gi())
    }
}

/// Builds the report from what the device has: the inputs a GPU gives
/// (`timestamps`: whether the device has timestamp queries; `msaa`: the
/// samples the game asked for and the ones used), without the GPU itself.
pub(crate) fn report(
    adapter: GpuAdapter,
    limits: GpuLimits,
    timestamps: bool,
    msaa: (u32, u32),
    three: bool,
) -> GpuCapabilities {
    let no_compute = "the device has no compute shaders or storage textures (max_compute_workgroups_per_dimension or max_storage_textures_per_shader_stage is 0, as on WebGL2)";
    let features = GpuFeature::ALL
        .iter()
        .map(|&feature| {
            let (available, fallback, reason): (bool, String, String) = if feature.is_3d() && !three {
                (false, "not drawn".into(), "built without the `3d` Cargo feature".into())
            } else {
                match feature {
                    GpuFeature::Timestamps => (
                        timestamps,
                        "no GPU pass times; RenderStats::gpu_ms is None".into(),
                        "the device lacks the TIMESTAMP_QUERY feature".into(),
                    ),
                    GpuFeature::Compute => (
                        limits.compute(),
                        "compute passes and the effects that need them are skipped (see the other entries)".into(),
                        no_compute.into(),
                    ),
                    GpuFeature::StorageBuffers => (
                        limits.storage_buffers(),
                        "GPU buffers are not created and compute passes using them do nothing; material shaders reading `items` and GPU-buffer instance setup report an error with a fix to use CPU-backed instance batches or model entities".into(),
                        if limits.compute() {
                            storage_reason(&limits, 2, no_compute)
                        } else {
                            no_compute.into()
                        },
                    ),
                    GpuFeature::ComputeCulling => (
                        limits.compute_culling(),
                        "instance batches are culled on the CPU, cell by cell, and drawn with direct draws (the same picture); crowds are culled on the CPU and drawn from frames posed when they are made".into(),
                        storage_reason(&limits, CULLING_STORAGE_BUFFERS, no_compute),
                    ),
                    GpuFeature::GpuSkinning => (
                        limits.gpu_skinning(),
                        "skinned and morphed meshes are posed on the CPU".into(),
                        storage_reason(&limits, SKINNING_STORAGE_BUFFERS, no_compute),
                    ),
                    GpuFeature::OcclusionCulling => (
                        limits.compute(),
                        "off: hidden models are drawn".into(),
                        no_compute.into(),
                    ),
                    GpuFeature::TemporalAntialiasing => {
                        (limits.compute(), "FXAA instead of TAA".into(), no_compute.into())
                    }
                    GpuFeature::AmbientOcclusion => (limits.compute(), "off".into(), no_compute.into()),
                    GpuFeature::ContactShadows => (limits.compute(), "off".into(), no_compute.into()),
                    GpuFeature::ScreenSpaceReflections => (limits.compute(), "off".into(), no_compute.into()),
                    GpuFeature::GlobalIllumination => (
                        limits.gi(),
                        "off: sky and ambient light only".into(),
                        if limits.compute() {
                            format!(
                                "max_sampled_textures_per_shader_stage is {}; global illumination needs {}",
                                limits.max_sampled_textures_per_shader_stage,
                                DECAL_SAMPLED_TEXTURES + 1
                            )
                        } else {
                            no_compute.into()
                        },
                    ),
                    GpuFeature::Decals => (
                        limits.decals(),
                        "decals are not drawn".into(),
                        format!(
                            "max_sampled_textures_per_shader_stage is {}; decals need {DECAL_SAMPLED_TEXTURES}",
                            limits.max_sampled_textures_per_shader_stage
                        ),
                    ),
                    GpuFeature::Multisampling => multisampling(msaa.0, msaa.1),
                }
            };
            GpuFeatureStatus {
                feature,
                available,
                fallback: (!available).then_some(fallback),
                reason: (!available).then_some(reason),
            }
        })
        .collect();
    GpuCapabilities {
        adapter,
        tier: GpuTier::of(&limits),
        instance_batches: limits.instance_batches(),
        limits,
        features,
        forced_tier: None,
    }
}

/// Multisampling's entry: available when the 3D frame uses the `requested`
/// sample count; otherwise `used` is the fallback.
fn multisampling(requested: u32, used: u32) -> (bool, String, String) {
    (
        requested == used,
        if used <= 1 {
            "no multisampling: edges are smoothed only by FXAA or SMAA, if chosen".into()
        } else {
            format!("{used}x multisampling")
        },
        if matches!(requested, 2 | 4) {
            format!("the GPU can't multisample the 3D frame's color and depth {requested}x")
        } else {
            format!("the 3D frame takes 2 or 4 samples, not {requested}x")
        },
    )
}

/// Updates the Multisampling entry of `caps` after the game asked for
/// `requested` samples and the 3D frame uses `used` (`Post::msaa`).
#[cfg(all(feature = "window", feature = "3d"))]
pub(crate) fn set_multisampling(caps: &mut GpuCapabilities, requested: u32, used: u32) {
    let (available, fallback, reason) = multisampling(requested, used);
    if let Some(status) = caps
        .features
        .iter_mut()
        .find(|s| s.feature == GpuFeature::Multisampling)
    {
        status.available = available;
        status.fallback = (!available).then_some(fallback);
        status.reason = (!available).then_some(reason);
    }
}

fn storage_reason(limits: &GpuLimits, needed: u32, no_compute: &str) -> String {
    if limits.compute() {
        format!(
            "max_storage_buffers_per_shader_stage is {}; {needed} are needed",
            limits.max_storage_buffers_per_shader_stage
        )
    } else {
        no_compute.into()
    }
}

#[cfg(feature = "window")]
impl GpuLimits {
    /// The limits `device` was opened with.
    pub(crate) fn of(device: &wgpu::Device) -> Self {
        let l = device.limits();
        Self {
            max_texture_dimension_2d: l.max_texture_dimension_2d,
            max_texture_array_layers: l.max_texture_array_layers,
            max_sampled_textures_per_shader_stage: l.max_sampled_textures_per_shader_stage,
            max_storage_buffers_per_shader_stage: l.max_storage_buffers_per_shader_stage,
            max_storage_textures_per_shader_stage: l.max_storage_textures_per_shader_stage,
            max_storage_buffer_binding_size: l.max_storage_buffer_binding_size,
            max_compute_workgroups_per_dimension: l.max_compute_workgroups_per_dimension,
            max_buffer_size: l.max_buffer_size,
        }
    }
}

#[cfg(feature = "window")]
impl GpuAdapter {
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Self {
        let device_type = match info.device_type {
            wgpu::DeviceType::DiscreteGpu => "discrete_gpu",
            wgpu::DeviceType::IntegratedGpu => "integrated_gpu",
            wgpu::DeviceType::VirtualGpu => "virtual_gpu",
            wgpu::DeviceType::Cpu => "cpu",
            wgpu::DeviceType::Other => "other",
        };
        Self {
            name: info.name.clone(),
            backend: info.backend.to_str().to_owned(),
            device_type: device_type.to_owned(),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
        }
    }
}

/// The report for `device`, opened on the adapter described by `info`;
/// `msaa`: the 3D frame's samples the game asked for, and the ones used.
#[cfg(feature = "window")]
pub(crate) fn of_device(info: &wgpu::AdapterInfo, device: &wgpu::Device, msaa: (u32, u32)) -> GpuCapabilities {
    report(
        GpuAdapter::of(info),
        GpuLimits::of(device),
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        msaa,
        cfg!(feature = "3d"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WebGL2's limits, as wgpu's `Limits::downlevel_webgl2_defaults` has them.
    fn webgl2() -> GpuLimits {
        GpuLimits {
            max_texture_dimension_2d: 2048,
            max_texture_array_layers: 256,
            max_sampled_textures_per_shader_stage: 16,
            max_storage_buffers_per_shader_stage: 0,
            max_storage_textures_per_shader_stage: 0,
            max_storage_buffer_binding_size: 0,
            max_compute_workgroups_per_dimension: 0,
            max_buffer_size: 256 << 20,
        }
    }

    /// WebGPU's default limits, with `slots` sampled textures.
    fn webgpu(slots: u32) -> GpuLimits {
        GpuLimits {
            max_texture_dimension_2d: 8192,
            max_texture_array_layers: 256,
            max_sampled_textures_per_shader_stage: slots,
            max_storage_buffers_per_shader_stage: 8,
            max_storage_textures_per_shader_stage: 4,
            max_storage_buffer_binding_size: 128 << 20,
            max_compute_workgroups_per_dimension: 65535,
            max_buffer_size: 256 << 20,
        }
    }

    fn caps(limits: GpuLimits, timestamps: bool) -> GpuCapabilities {
        report(GpuAdapter::default(), limits, timestamps, (4, 4), true)
    }

    #[test]
    fn every_feature_is_reported_once_in_order() {
        let r = caps(webgpu(16), false);
        let order: Vec<_> = r.features.iter().map(|f| f.feature).collect();
        assert_eq!(order, GpuFeature::ALL);
    }

    #[test]
    fn unavailable_features_name_a_fallback_and_a_reason() {
        for r in [caps(webgl2(), false), caps(webgpu(16), false), caps(webgpu(24), true)] {
            for f in &r.features {
                assert_eq!(f.fallback.is_none(), f.available, "{f:?}");
                assert_eq!(f.reason.is_none(), f.available, "{f:?}");
            }
        }
    }

    #[test]
    fn webgl2_limits_fall_back_from_every_compute_feature() {
        let r = caps(webgl2(), false);
        assert!(!r.available(GpuFeature::Compute));
        let taa = r.feature(GpuFeature::TemporalAntialiasing).unwrap();
        assert!(!taa.available);
        assert_eq!(taa.fallback.as_deref(), Some("FXAA instead of TAA"));
        assert!(taa.reason.as_deref().unwrap().contains("compute"), "{taa:?}");
        for f in [
            GpuFeature::StorageBuffers,
            GpuFeature::ComputeCulling,
            GpuFeature::GpuSkinning,
            GpuFeature::OcclusionCulling,
            GpuFeature::AmbientOcclusion,
            GpuFeature::ContactShadows,
            GpuFeature::ScreenSpaceReflections,
            GpuFeature::GlobalIllumination,
            GpuFeature::Decals,
            GpuFeature::Timestamps,
        ] {
            assert!(!r.available(f), "{f:?}");
        }
    }

    #[test]
    fn default_texture_slots_leave_out_decals_and_gi_only() {
        let r = caps(webgpu(16), true);
        for f in GpuFeature::ALL {
            let expected = !matches!(f, GpuFeature::Decals | GpuFeature::GlobalIllumination);
            assert_eq!(r.available(f), expected, "{f:?}");
        }
        assert!(
            r.feature(GpuFeature::Decals)
                .unwrap()
                .reason
                .as_deref()
                .unwrap()
                .contains("16")
        );
        // One more slot: decals, but no room for GI's probes yet.
        let r = caps(webgpu(17), true);
        assert!(r.available(GpuFeature::Decals));
        assert!(!r.available(GpuFeature::GlobalIllumination));
        assert!(caps(webgpu(18), true).available(GpuFeature::GlobalIllumination));
    }

    #[test]
    fn few_storage_buffers_turn_off_skinning_before_culling() {
        let limits = GpuLimits {
            max_storage_buffers_per_shader_stage: 4,
            ..webgpu(16)
        };
        let r = caps(limits, false);
        assert!(r.available(GpuFeature::ComputeCulling));
        let skinning = r.feature(GpuFeature::GpuSkinning).unwrap();
        assert!(!skinning.available);
        assert_eq!(
            skinning.reason.as_deref(),
            Some("max_storage_buffers_per_shader_stage is 4; 6 are needed")
        );
        assert!(!r.available(GpuFeature::Timestamps));
    }

    #[test]
    fn a_lowered_multisample_count_is_a_fallback() {
        let r = report(GpuAdapter::default(), webgpu(24), true, (8, 4), true);
        let msaa = r.feature(GpuFeature::Multisampling).unwrap();
        assert!(!msaa.available);
        assert_eq!(msaa.fallback.as_deref(), Some("4x multisampling"));
        assert!(msaa.reason.as_deref().unwrap().contains("8x"));
    }

    #[cfg(all(feature = "window", feature = "3d"))]
    #[test]
    fn a_later_request_updates_the_multisampling_entry() {
        let mut r = report(GpuAdapter::default(), webgpu(24), true, (4, 4), true);
        set_multisampling(&mut r, 2, 4);
        let msaa = r.feature(GpuFeature::Multisampling).unwrap();
        assert!(!msaa.available);
        assert_eq!(msaa.fallback.as_deref(), Some("4x multisampling"));
        assert!(msaa.reason.as_deref().unwrap().contains("2x"));
        set_multisampling(&mut r, 8, 4);
        let reason = r.feature(GpuFeature::Multisampling).unwrap().reason.clone().unwrap();
        assert!(reason.contains("takes 2 or 4 samples, not 8x"), "{reason}");
        set_multisampling(&mut r, 2, 2);
        assert!(r.available(GpuFeature::Multisampling));
        assert_eq!(r.feature(GpuFeature::Multisampling).unwrap().fallback, None);
    }

    #[test]
    fn no_multisampling_is_reported_as_a_fallback_without_a_count() {
        let r = report(GpuAdapter::default(), webgl2(), false, (4, 1), true);
        let msaa = r.feature(GpuFeature::Multisampling).unwrap();
        assert!(!msaa.available);
        assert!(msaa.fallback.as_deref().unwrap().starts_with("no multisampling"));
        assert!(msaa.reason.as_deref().unwrap().contains("4x"));
    }

    #[cfg(all(feature = "window", feature = "3d"))]
    #[test]
    fn only_a_browsers_gl_lacks_multisampled_textures() {
        use crate::render::gpu3d::browser_gl_has_no_multisampled_textures as lacks;
        // On the desktop even GL has them; in a browser (wasm32) only GL lacks them.
        assert!(!lacks(wgpu::Backend::Vulkan));
        assert!(!lacks(wgpu::Backend::BrowserWebGpu));
        assert_eq!(lacks(wgpu::Backend::Gl), cfg!(target_arch = "wasm32"));
    }

    #[test]
    fn without_the_3d_renderer_its_features_are_off() {
        let r = report(GpuAdapter::default(), webgpu(24), true, (4, 4), false);
        for f in GpuFeature::ALL {
            assert_eq!(r.available(f), !f.is_3d(), "{f:?}");
        }
    }

    #[test]
    fn the_report_is_snake_case_json_and_round_trips() {
        let r = caps(webgl2(), false);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["features"][6]["feature"], "temporal_antialiasing");
        assert_eq!(json["features"][6]["available"], false);
        assert_eq!(json["features"][6]["fallback"], "FXAA instead of TAA");
        assert_eq!(json["limits"]["max_compute_workgroups_per_dimension"], 0);
        let back: GpuCapabilities = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn each_tier_floor_is_classified_as_that_tier() {
        for tier in GpuTier::ALL {
            assert_eq!(GpuTier::of(&tier.minimum_limits()), tier, "{tier}");
            assert_eq!(tier.report().tier, tier, "{tier}");
        }
        assert!(GpuTier::WebGl2 < GpuTier::Baseline && GpuTier::Baseline < GpuTier::Desktop);
    }

    #[test]
    fn limits_between_floors_get_the_lower_tier() {
        // WebGPU's defaults: compute and skinning, but 16 texture slots.
        assert_eq!(GpuTier::of(&webgpu(16)), GpuTier::Baseline);
        assert_eq!(GpuTier::of(&webgpu(17)), GpuTier::Baseline);
        assert_eq!(GpuTier::of(&webgpu(18)), GpuTier::Desktop);
        assert_eq!(GpuTier::of(&webgl2()), GpuTier::WebGl2);
        // Compute, but too few storage buffers to cull: below baseline.
        let few = GpuLimits {
            max_storage_buffers_per_shader_stage: 2,
            ..webgpu(24)
        };
        assert_eq!(GpuTier::of(&few), GpuTier::WebGl2);
    }

    #[test]
    fn tiers_list_exactly_the_documented_unavailable_features() {
        use GpuFeature as F;
        // Without the `3d` Cargo feature, every 3D feature is off as well.
        let expect = |documented: &[F]| -> Vec<F> {
            GpuFeature::ALL
                .into_iter()
                .filter(|f| documented.contains(f) || (f.is_3d() && !cfg!(feature = "3d")))
                .collect()
        };
        let all_but_msaa: Vec<F> = GpuFeature::ALL.into_iter().filter(|f| *f != F::Multisampling).collect();
        assert_eq!(GpuTier::WebGl2.unavailable_features(), expect(&all_but_msaa));
        assert_eq!(
            GpuTier::Baseline.unavailable_features(),
            expect(&[F::Timestamps, F::GpuSkinning, F::GlobalIllumination, F::Decals])
        );
        assert_eq!(GpuTier::Desktop.unavailable_features(), expect(&[F::Timestamps]));
    }

    #[test]
    fn higher_tiers_never_lose_a_feature() {
        for w in GpuTier::ALL.windows(2) {
            let (lo, hi) = (w[0].report(), w[1].report());
            for f in GpuFeature::ALL {
                assert!(!lo.available(f) || hi.available(f), "{f:?} from {} to {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn tier_names_parse_and_serialize_alike() {
        for tier in GpuTier::ALL {
            assert_eq!(tier.name().parse::<GpuTier>(), Ok(tier));
            assert_eq!(tier.name().to_uppercase().parse::<GpuTier>(), Ok(tier));
            assert_eq!(serde_json::to_value(tier).unwrap(), tier.name());
        }
        assert!(
            "webgl"
                .parse::<GpuTier>()
                .unwrap_err()
                .contains("webgl2, baseline or desktop")
        );
        let json = serde_json::to_value(GpuTier::Baseline.report()).unwrap();
        assert_eq!(json["tier"], "baseline");
        assert_eq!(json["forced_tier"], serde_json::Value::Null);
    }

    #[test]
    fn batch_limits_follow_the_device_limits() {
        // No compute: nothing is GPU-driven, the CPU path is bounded by
        // one vertex buffer.
        let web = caps(webgl2(), false).instance_batches;
        assert!(!web.gpu_driven);
        assert_eq!((web.max_gpu_instances, web.max_cull_cells), (0, 0));
        assert_eq!(web.max_cpu_instances, (256 << 20) / BATCH_VERTEX_BYTES);
        // Compute: one pool's storage binding bounds a GPU-driven batch,
        // one dispatch its cells.
        let full = caps(webgpu(16), false).instance_batches;
        assert!(full.gpu_driven);
        assert_eq!(full.min_gpu_instances, GPU_BATCH_MIN);
        assert_eq!(full.min_gpu_triangles, GPU_BATCH_TRIANGLES);
        let binding = webgpu(16).max_storage_buffer_binding_size;
        assert_eq!(full.max_gpu_instances, (binding - (64 << 10)) / BATCH_RECORD_BYTES);
        assert!(full.max_gpu_instances >= u64::from(GPU_BATCH_MIN), "{full:?}");
        assert_eq!(full.max_cull_cells, 256 * 256 * 65_535);
        // A buffer smaller than the binding limit bounds both paths.
        let mut small = webgpu(16);
        small.max_buffer_size = 1 << 20;
        let small = small.instance_batches();
        assert_eq!(small.max_gpu_instances, ((1 << 20) - (64 << 10)) / BATCH_RECORD_BYTES);
        assert_eq!(small.max_cpu_instances, (1 << 20) / BATCH_VERTEX_BYTES);
        // Few workgroups per dimension shrink the cell grid.
        let mut narrow = webgpu(16);
        narrow.max_compute_workgroups_per_dimension = 16;
        assert_eq!(narrow.instance_batches().max_cull_cells, 16 * 16 * 16);
    }

    #[test]
    fn a_report_without_batch_limits_still_reads() {
        // Reports saved before these fields existed read back, as zeros.
        let r = caps(webgpu(16), false);
        let mut json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["instance_batches"]["gpu_driven"], true);
        assert_eq!(json["limits"]["max_buffer_size"], 256u64 << 20);
        json.as_object_mut().unwrap().remove("instance_batches");
        json["limits"].as_object_mut().unwrap().remove("max_buffer_size");
        let back: GpuCapabilities = serde_json::from_value(json).unwrap();
        assert_eq!(back.instance_batches, GpuBatchLimits::default());
        assert_eq!(back.limits.max_buffer_size, 0);
        assert_eq!(back.features, r.features);
    }

    #[test]
    fn a_report_without_tier_fields_still_reads() {
        let mut json = serde_json::to_value(caps(webgl2(), false)).unwrap();
        let map = json.as_object_mut().unwrap();
        map.remove("tier");
        map.remove("forced_tier");
        let back: GpuCapabilities = serde_json::from_value(json).unwrap();
        assert_eq!((back.tier, back.forced_tier), (GpuTier::WebGl2, None));
    }

    #[cfg(feature = "window")]
    #[test]
    fn tier_floors_match_wgpus_limit_sets() {
        let of = |l: wgpu::Limits| GpuLimits {
            max_texture_dimension_2d: l.max_texture_dimension_2d,
            max_texture_array_layers: l.max_texture_array_layers,
            max_sampled_textures_per_shader_stage: l.max_sampled_textures_per_shader_stage,
            max_storage_buffers_per_shader_stage: l.max_storage_buffers_per_shader_stage,
            max_storage_textures_per_shader_stage: l.max_storage_textures_per_shader_stage,
            max_storage_buffer_binding_size: l.max_storage_buffer_binding_size,
            max_compute_workgroups_per_dimension: l.max_compute_workgroups_per_dimension,
            max_buffer_size: l.max_buffer_size,
        };
        let engine = wgpu::Limits {
            max_sampled_textures_per_shader_stage: 18,
            ..wgpu::Limits::default()
        };
        for tier in GpuTier::ALL {
            assert_eq!(of(tier.device_limits(engine.clone())), tier.minimum_limits(), "{tier}");
        }
        // A forced window keeps the texture size a high-DPI window needs,
        // and still lacks what the tier lacks.
        for tier in GpuTier::ALL {
            let window = of(tier.window_limits(engine.clone()));
            assert_eq!(window.max_texture_dimension_2d, 8192, "{tier}");
            assert!(
                window.max_texture_dimension_2d >= 2560,
                "{tier}: a 1280x720 window at 2x"
            );
            assert_eq!(
                GpuLimits {
                    max_texture_dimension_2d: tier.minimum_limits().max_texture_dimension_2d,
                    ..window
                },
                tier.minimum_limits(),
                "{tier}: everything else is the floor"
            );
            assert_eq!(GpuTier::of(&window), tier, "{tier}");
        }
        let all = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        assert_eq!(GpuTier::Desktop.device_features(all), all);
        // Forcing never raises a limit a request already lowered.
        let lowered = wgpu::Limits {
            max_compute_workgroups_per_dimension: 0,
            ..engine.clone()
        };
        assert_eq!(
            GpuTier::Baseline
                .device_limits(lowered)
                .max_compute_workgroups_per_dimension,
            0
        );
        assert_eq!(
            GpuTier::Baseline.device_features(all),
            wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        );
    }
}
