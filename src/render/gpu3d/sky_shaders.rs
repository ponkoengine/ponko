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

/// Every sky shader the game made, by id.
pub(super) struct SkyShaders {
    /// The sky texture pass's layout (the shadow frames' bind group).
    lut_layout: wgpu::PipelineLayout,
    shaders: Vec<SkyPipelines>,
}

impl SkyShaders {
    pub fn new(lut_layout: wgpu::PipelineLayout) -> Self {
        Self {
            lut_layout,
            shaders: Vec::new(),
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
        let pipelines = self.checked_sky(device, name, user)?;
        let id = self.sky_shaders.shaders.len() as u32;
        self.installed(ShaderKind::Sky, id);
        self.sky_shaders.shaders.push(pipelines);
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
        let pipelines = self.checked_sky(device, name, user)?;
        if let Some(old) = self.sky_shaders.shaders.get_mut(id as usize) {
            *old = pipelines;
            self.installed(ShaderKind::Sky, id);
            // The fog, reflections and ambient light read the new sky.
            self.environment.invalidate_sky();
            self.history.content_changed = true;
        }
        Ok(())
    }

    /// Checks a sky shader's WGSL (pointing at the game's code) and builds
    /// its pipelines in a GPU error scope.
    fn checked_sky(
        &self,
        device: &wgpu::Device,
        name: &str,
        user: &str,
    ) -> Result<SkyPipelines, Box<ShaderDiagnostic>> {
        let source = compose_sky(user);
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
            .map(|s| self.sky_pipelines_of(device, s.module))
            .collect();
    }

    /// The sky texture's and the background's pipelines this frame: the
    /// active sky shader's, or the atmosphere's.
    pub(super) fn sky_pipelines(&self) -> (&wgpu::RenderPipeline, &wgpu::RenderPipeline) {
        match self.sky_shader.and_then(|id| self.sky_shaders.shaders.get(id as usize)) {
            Some(s) => (&s.lut, &s.sky),
            None => (&self.sky_lut, &self.sky),
        }
    }

    /// Whether sky shader `id` was made (and so can be drawn).
    pub(super) fn has_sky_shader(&self, id: u32) -> bool {
        (id as usize) < self.sky_shaders.shaders.len()
    }
}
