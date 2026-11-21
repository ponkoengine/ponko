//! The game's sky shaders (`SkyShader`): each one's pipelines for the sky
//! texture and for the background. The sky texture is what fog,
//! reflections and the ambient light read, so a sky shader drawn into it
//! reaches all of them; the background runs it at every pixel.

use super::shader_checks::{end_scope, rejected};
use super::shaders::{MESH_PRELUDE, for_samples};
use super::{HDR_FORMAT, Renderer3D, SKY};
use crate::render::{ShaderDiagnostic, ShaderKind};

/// The engine's entry points for a sky shader.
const SKY_SHADER_MAIN: &str = include_str!("../sky_shader.wgsl");

/// The variant name sky shader errors report.
pub(crate) const SKY_VARIANT: &str = "sky shader";

/// A whole sky shader from the game's `sky` function: the mesh prelude,
/// the built-in sky's helpers (stars, the sun's disk, fog), the game's
/// code, the entry points.
pub(crate) fn compose_sky(user: &str) -> String {
    let mut source = String::with_capacity(MESH_PRELUDE.len() + SKY.len() + user.len() + SKY_SHADER_MAIN.len() + 100);
    source.push_str(MESH_PRELUDE);
    source.push('\n');
    source.push_str(SKY);
    source.push_str("\n// ---- the game's sky shader ----\n");
    source.push_str(user);
    source.push_str("\n// ---- the engine's entry points ----\n");
    source.push_str(SKY_SHADER_MAIN);
    source
}

/// One sky shader's module and pipelines.
struct SkyPipelines {
    module: wgpu::ShaderModule,
    /// Into the sky texture.
    lut: wgpu::RenderPipeline,
    /// Behind everything, at the frame's sample count and depth format.
    sky: wgpu::RenderPipeline,
}

/// Every sky shader the game made, by id (`None` once freed).
pub(super) struct SkyShaders {
    /// The sky texture pass's layout (the shadow frames' bind group).
    lut_layout: wgpu::PipelineLayout,
    shaders: Vec<Option<SkyPipelines>>,
    /// The sky kit's sky shader (`SkyKit`), made the first frame it's on.
    kit: Option<SkyPipelines>,
    /// The kit's air lookup (`fs_kit_air`): the atmosphere alone, into the
    /// sky texture, then copied beside the cloud shadow map for the
    /// background to read.
    kit_air: Option<wgpu::RenderPipeline>,
    /// The kit's shader failed to build (a bug, reported once).
    kit_failed: bool,
}

impl SkyShaders {
    pub fn new(lut_layout: wgpu::PipelineLayout) -> Self {
        Self {
            lut_layout,
            shaders: Vec::new(),
            kit: None,
            kit_air: None,
            kit_failed: false,
        }
    }
}

/// A full-screen sky pipeline of `module`'s `entry`: into the sky texture
/// without `depth`, or behind the scene with it.
fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry: &str,
    samples: u32,
    depth: Option<wgpu::TextureFormat>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_far"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        // Behind everything: only where the depth is still the far plane
        // (0, reverse-Z).
        depth_stencil: depth.map(|format| wgpu::DepthStencilState {
            format,
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
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(HDR_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Renderer3D {
    /// Compiles a sky shader from the game's WGSL (its `sky` function);
    /// returns its id.
    pub fn create_sky_shader(
        &mut self,
        device: &wgpu::Device,
        name: &str,
        user: &str,
    ) -> Result<u32, Box<ShaderDiagnostic>> {
        let pipelines = self.checked_sky(device, name, user, "")?;
        let id = self.sky_shaders.shaders.len() as u32;
        self.installed(ShaderKind::Sky, id);
        self.sky_shaders.shaders.push(Some(pipelines));
        Ok(id)
    }

    /// Recompiles sky shader `id` (hot reload). On an error the old one stays.
    pub fn replace_sky_shader(
        &mut self,
        device: &wgpu::Device,
        id: u32,
        name: &str,
        user: &str,
    ) -> Result<(), Box<ShaderDiagnostic>> {
        let pipelines = self.checked_sky(device, name, user, "")?;
        if let Some(Some(old)) = self.sky_shaders.shaders.get_mut(id as usize) {
            *old = pipelines;
            self.installed(ShaderKind::Sky, id);
            // The fog, reflections and ambient light read the new sky.
            self.environment.invalidate_sky();
            self.history.content_changed = true;
        }
        Ok(())
    }

    /// Checks a sky shader's WGSL (pointing at the game's code) and builds
    /// its pipelines in a GPU error scope. `engine` is more of the engine's
    /// code after its entry points (the sky kit's air lookup).
    fn checked_sky(
        &self,
        device: &wgpu::Device,
        name: &str,
        user: &str,
        engine: &str,
    ) -> Result<SkyPipelines, Box<ShaderDiagnostic>> {
        let source = compose_sky(user) + engine;
        crate::render::diagnose(name, ShaderKind::Sky, SKY_VARIANT, &source)?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(for_samples(&source, self.samples, self.float_depth).into_owned().into()),
        });
        let pipelines = self.sky_pipelines_of(device, module);
        match end_scope(scope, self.defer_checks()) {
            Ok(Some(error)) => Err(rejected(name, ShaderKind::Sky, SKY_VARIANT, &error)),
            // A GPU that answers later (WebGL2) reports a rejection as a
            // device error; naga has checked the shader by then.
            Ok(None) | Err(_) => Ok(pipelines),
        }
    }

    fn sky_pipelines_of(&self, device: &wgpu::Device, module: wgpu::ShaderModule) -> SkyPipelines {
        SkyPipelines {
            lut: pipeline(device, &module, &self.sky_shaders.lut_layout, "fs_lut_shader", 1, None),
            sky: pipeline(
                device,
                &module,
                &self.sky_layout,
                "fs_sky_shader",
                self.samples,
                Some(self.shaders.depth_format()),
            ),
            module,
        }
    }

    /// Builds every sky shader's background again, after the frame's sample
    /// count or depth format changed.
    pub(super) fn rebuild_sky_shaders(&mut self, device: &wgpu::Device) {
        let shaders = std::mem::take(&mut self.sky_shaders.shaders);
        self.sky_shaders.shaders = shaders
            .into_iter()
            .map(|s| s.map(|s| self.sky_pipelines_of(device, s.module)))
            .collect();
        if let Some(kit) = self.sky_shaders.kit.take() {
            self.sky_shaders.kit = Some(self.sky_pipelines_of(device, kit.module));
        }
    }

    /// Compiles the sky kit's sky shader, the first time; whether it is
    /// there to draw with.
    pub(super) fn ensure_kit_sky(&mut self, device: &wgpu::Device) -> bool {
        if self.sky_shaders.kit_failed {
            return false;
        }
        if self.sky_shaders.kit.is_none() {
            let (kit, air) = (crate::render::clouds::sky_source(), crate::render::clouds::AIR_MAIN);
            match self.checked_sky(device, "sky kit", &kit, air) {
                Ok(pipelines) => {
                    let air = pipeline(
                        device,
                        &pipelines.module,
                        &self.sky_shaders.lut_layout,
                        "fs_kit_air",
                        1,
                        None,
                    );
                    self.sky_shaders.kit_air = Some(air);
                    self.sky_shaders.kit = Some(pipelines);
                }
                // The engine's own shader: a failure is a bug, reported
                // once, and the atmosphere is drawn instead.
                Err(e) => {
                    eprintln!("ponko: the sky kit's shader failed to build, drawing the atmosphere: {e}");
                    self.sky_shaders.kit_failed = true;
                    return false;
                }
            }
        }
        true
    }

    /// Drops freed sky shader `id`'s pipelines; a sky still set to it
    /// draws the atmosphere.
    pub fn free_sky_shader(&mut self, id: u32) {
        if let Some(slot) = self.sky_shaders.shaders.get_mut(id as usize) {
            *slot = None;
        }
    }

    /// The sky texture's and the background's pipelines this frame: the
    /// active sky shader's, the sky kit's, or the atmosphere's.
    pub(super) fn sky_pipelines(&self) -> (&wgpu::RenderPipeline, &wgpu::RenderPipeline) {
        let kit = self.kit.and(self.sky_shaders.kit.as_ref());
        match self
            .sky_shader
            .and_then(|id| self.sky_shaders.shaders.get(id as usize)?.as_ref())
            .or(kit)
        {
            Some(s) => (&s.lut, &s.sky),
            None => (&self.sky_lut, &self.sky),
        }
    }

    /// The sky kit's air lookup pipeline, while the kit is the sky.
    pub(super) fn kit_air(&self) -> Option<&wgpu::RenderPipeline> {
        self.kit.and(self.sky_shaders.kit_air.as_ref())
    }

    /// Whether sky shader `id` was made and not freed (and so can be drawn).
    pub(super) fn has_sky_shader(&self, id: u32) -> bool {
        self.sky_shaders.shaders.get(id as usize).is_some_and(Option::is_some)
    }
}

impl Renderer3D {
    /// The sky kit's work this frame, before the uniforms: its clouds blown
    /// on by the wind, its passes made the first time (the volumetric ones
    /// the first time a volumetric preset is used where the GPU can).
    pub(super) fn prepare_kit(&mut self, device: &wgpu::Device, scene: &crate::three::Scene3D<'_>) {
        if self.extra {
            // A window view: the main frame blew the clouds and drew the
            // shadow map; the view draws the flat layer.
            return;
        }
        let Some(kit) = self.kit else {
            if let Some(clouds) = &mut self.clouds {
                (clouds.shadows, clouds.volumetric) = (false, false);
                clouds.forget();
            }
            return;
        };
        let wind = kit.clouds.wind;
        if wind.is_finite() && scene.time.1.is_finite() {
            self.cloud_drift += wind * scene.time.1.max(0.0);
        }
        let volumetric = kit.quality.is_volumetric() && volumetric_clouds(device);
        let clouds = self
            .clouds
            .get_or_insert_with(|| Box::new(crate::render::clouds::Clouds::new(device)));
        clouds.shadows = kit.casts_shadows();
        clouds.volumetric = volumetric;
        if volumetric {
            let (samples, float_depth) = (self.samples, self.float_depth);
            clouds.ensure_volume(
                device,
                &crate::render::clouds::Sources {
                    frame_layout: &self.frame_sources.layout,
                    compose: &|code| for_samples(&compose_sky(code), samples, float_depth).into_owned(),
                    samples,
                    depth_format: self.shaders.depth_format(),
                },
            );
        } else {
            clouds.forget();
        }
    }

    /// What the frame uniform holds of the sky kit (`frame.sky_kit`), and
    /// its passes' settings for this frame (main view): zero without it.
    pub(super) fn kit_uniform(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (to_sun, camera, stars): (glam::Vec3, glam::Vec3, f32),
        size: (u32, u32),
        unjittered: glam::Mat4,
    ) -> [[f32; 4]; 6] {
        let Some(kit) = self.kit else {
            return [[0.0; 4]; 6];
        };
        let extra = self.extra;
        let Some(clouds) = self.clouds.as_deref_mut().filter(|_| !extra) else {
            return kit.frame_params(to_sun, camera, self.cloud_drift, false, stars);
        };
        if clouds.volumetric {
            clouds.prepare_volume(device, queue, size, unjittered, kit.quality);
        }
        let params = kit.frame_params(to_sun, camera, self.cloud_drift, clouds.volumetric, stars);
        if clouds.shadows {
            clouds.prepare_shadows(queue, params, to_sun);
        }
        params
    }

    /// Tests: makes the cloud passes march every pixel each frame whatever
    /// the preset (the presets' cost before the interleaved march), once
    /// they are made. Returns whether they are.
    #[cfg(test)]
    pub(crate) fn set_full_cloud_march(&mut self, on: bool) -> bool {
        self.clouds.as_deref_mut().map(|c| c.full_march = on).is_some()
    }

    /// The size of the cloud trace drawn last frame (for tests).
    #[cfg(test)]
    pub(crate) fn cloud_trace_size(&self) -> Option<(u32, u32)> {
        let (trace, _) = self.clouds.as_deref()?.images()?;
        Some((trace.width(), trace.height()))
    }

    /// Whether anything of the sky kit was made: its sky pipelines or its
    /// cloud passes (for tests: none until a game turns the kit on).
    #[cfg(test)]
    pub(crate) fn kit_made(&self) -> bool {
        self.clouds.is_some() || self.sky_shaders.kit.is_some()
    }

    /// The cloud shadow map as last drawn, row by row (for tests): how
    /// much sunlight gets through, 0..1, per texel.
    #[cfg(test)]
    pub(crate) fn cloud_shadow_map(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<f32> {
        let side = crate::three::CLOUD_SHADOW_TEXELS;
        let row = side * 8;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * side),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.environment.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 1408, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(side),
                },
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = readback.get_mapped_range(..).unwrap().to_vec();
        // Rgba16Float: the red half of each texel.
        let (texels, _) = bytes.as_chunks::<8>();
        texels
            .iter()
            .map(|t| f16_to_f32(u16::from_le_bytes([t[0], t[1]])))
            .collect()
    }

    /// Whether this frame draws the cloud shadow map and the volumetric
    /// clouds (main view only).
    pub(super) fn kit_passes(&self) -> (bool, bool) {
        match self.clouds.as_deref().filter(|_| self.kit.is_some() && !self.extra) {
            Some(c) => (c.shadows, c.volumetric && c.has_volume()),
            None => (false, false),
        }
    }
}

/// Whether `device` draws the sky kit's volumetric clouds: not a
/// WebGL2-class GPU, which draws the flat layer.
fn volumetric_clouds(device: &wgpu::Device) -> bool {
    crate::render::GpuTier::of(&crate::render::capabilities::GpuLimits::of(device)) != crate::render::GpuTier::WebGl2
}

/// A half float's value (tests read the HDR atlas back with it).
#[cfg(test)]
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = f32::from(h & 0x3ff);
    match exponent {
        0 => sign * mantissa * 2f32.powi(-24),
        31 => sign * f32::INFINITY,
        e => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    }
}
