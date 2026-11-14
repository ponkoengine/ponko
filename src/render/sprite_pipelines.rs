//! Sprite blend modes and sprite materials on the GPU: the game's sprite
//! shaders, each material's parameters and textures (bind group 2), and a
//! pipeline per shader, blend mode and target format, made the first time
//! a frame draws with them. Plain sprites never come here: they keep the
//! renderer's own sprite pipeline.

use std::collections::HashMap;

use super::batch::Batch;
use super::gpu::GpuTexture;
use super::traffic::Counted;
use crate::render::{ShaderDiagnostic, ShaderKind};
use crate::sprite_material::{BlendMode, SPRITE_VARIANT, split_style};

/// How `blend` combines a sprite's fragment color with the target.
pub(super) fn blend_state(blend: BlendMode) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op};
    // The target's alpha stays as it is for the modes that only tint it.
    let keep_alpha = BlendComponent {
        src_factor: F::Zero,
        dst_factor: F::One,
        operation: Op::Add,
    };
    match blend {
        BlendMode::Alpha => wgpu::BlendState::ALPHA_BLENDING,
        BlendMode::Additive => wgpu::BlendState {
            color: BlendComponent {
                src_factor: F::SrcAlpha,
                dst_factor: F::One,
                operation: Op::Add,
            },
            alpha: keep_alpha,
        },
        // The shader premultiplies (`fs_multiply`): dst * (rgb * a + 1 - a).
        BlendMode::Multiply => wgpu::BlendState {
            color: BlendComponent {
                src_factor: F::Dst,
                dst_factor: F::OneMinusSrcAlpha,
                operation: Op::Add,
            },
            alpha: keep_alpha,
        },
        BlendMode::Premultiplied => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
    }
}

/// The fragment entry point for `blend`: the built-in shader's
/// (`sprite.wgsl`) or a composed sprite shader's (`sprite_entries.wgsl`).
fn entry(blend: BlendMode, custom: bool) -> &'static str {
    match (blend, custom) {
        (BlendMode::Multiply, _) => "fs_multiply",
        (BlendMode::Premultiplied, false) => "fs_premultiplied",
        _ => "fs_main",
    }
}

/// One pipeline: the shader (`None`: built-in) at a version, a blend mode
/// and a target format.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    shader: Option<(u32, u32)>,
    blend: BlendMode,
    format: wgpu::TextureFormat,
}

struct Shader {
    module: wgpu::ShaderModule,
    /// Bumped by each reload, so old pipelines are never picked again.
    version: u32,
}

struct Material {
    shader: Option<u32>,
    params: wgpu::Buffer,
    textures: [Option<u32>; 2],
    /// The bind group and the textures it was made from: made again when
    /// one of them was replaced or freed.
    bind_group: Option<(wgpu::BindGroup, [wgpu::Texture; 2])>,
}

pub(crate) struct SpritePipelines {
    builtin: wgpu::ShaderModule,
    builtin_layout: wgpu::PipelineLayout,
    custom_layout: wgpu::PipelineLayout,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::Texture,
    shaders: Vec<Option<Shader>>,
    versions: u32,
    materials: Vec<Option<Material>>,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
}

/// What a style key draws with: the pipeline key and the material's slot
/// (for its bind group), `None` for the built-in look.
type Resolved = (Key, Option<usize>);

impl SpritePipelines {
    /// `builtin` and `builtin_layout` are the renderer's sprite shader and
    /// its layout (camera, texture); materials add group 2.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        builtin: &wgpu::ShaderModule,
        builtin_layout: &wgpu::PipelineLayout,
        camera_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let texture = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite material layout"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARAMS_BYTES),
                    },
                ),
                entry(1, texture),
                entry(2, texture),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let custom_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite material pipeline layout"),
            bind_group_layouts: &[Some(camera_layout), Some(texture_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite material white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            white.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            white.size(),
        );
        Self {
            builtin: builtin.clone(),
            builtin_layout: builtin_layout.clone(),
            custom_layout,
            material_layout,
            sampler,
            white,
            shaders: Vec::new(),
            versions: 0,
            materials: Vec::new(),
            pipelines: HashMap::new(),
        }
    }

    /// Compiles sprite shader `id` from its composed WGSL (already checked
    /// by `render::diagnose`), new or replacing the old one. On an error
    /// the old version stays. `format`: the window's, to build its first
    /// pipeline now, inside the error check.
    pub(crate) fn set_shader(
        &mut self,
        device: &wgpu::Device,
        id: u32,
        name: &str,
        composed: &str,
        format: wgpu::TextureFormat,
    ) -> Result<(), Box<ShaderDiagnostic>> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(composed.into()),
        });
        self.versions += 1;
        let key = Key {
            shader: Some((id, self.versions)),
            blend: BlendMode::Alpha,
            format,
        };
        let pipeline = self.build(device, key, &module);
        if let Some(error) = end_scope(scope) {
            return Err(crate::render::without_location(
                name,
                ShaderKind::Sprite,
                SPRITE_VARIANT,
                format!("the GPU rejected its pipeline: {error}"),
            ));
        }
        let index = id as usize;
        if self.shaders.len() <= index {
            self.shaders.resize_with(index + 1, || None);
        }
        self.shaders[index] = Some(Shader {
            module,
            version: self.versions,
        });
        self.pipelines.retain(|k, _| k.shader.is_none_or(|(s, _)| s != id));
        self.pipelines.insert(key, pipeline);
        Ok(())
    }

    /// Sprite shader `id` was freed: its materials draw the built-in look.
    pub(crate) fn free_shader(&mut self, id: u32) {
        if let Some(slot) = self.shaders.get_mut(id as usize) {
            *slot = None;
        }
        self.pipelines.retain(|k, _| k.shader.is_none_or(|(s, _)| s != id));
    }

    /// Makes or changes sprite material `id`.
    pub(crate) fn set_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u32,
        shader: Option<u32>,
        params: &[glam::Vec4; 4],
        textures: [Option<u32>; 2],
    ) {
        let index = id as usize;
        if self.materials.len() <= index {
            self.materials.resize_with(index + 1, || None);
        }
        let material = self.materials[index].get_or_insert_with(|| Material {
            shader,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sprite material params"),
                size: PARAMS_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            textures,
            bind_group: None,
        });
        material.shader = shader;
        if material.textures != textures {
            material.textures = textures;
            material.bind_group = None;
        }
        queue.uploading(super::traffic::Upload::Uniforms).write_buffer(
            &material.params,
            0,
            bytemuck::cast_slice(&params.map(|p| p.to_array())),
        );
    }

    /// Sprite material `id` was freed: what uses it draws the built-in look.
    pub(crate) fn free_material(&mut self, id: u32) {
        if let Some(slot) = self.materials.get_mut(id as usize) {
            *slot = None;
        }
    }

    /// What style key `style` draws with into `format`.
    fn resolve(&self, style: u32, format: wgpu::TextureFormat) -> Resolved {
        let (blend, material) = split_style(style);
        let custom = material.and_then(|m| {
            let index = m as usize;
            let material = self.materials.get(index)?.as_ref()?;
            let shader = material.shader?;
            let version = self.shaders.get(shader as usize)?.as_ref()?.version;
            Some(((shader, version), index))
        });
        (
            Key {
                shader: custom.map(|c| c.0),
                blend,
                format,
            },
            custom.map(|c| c.1),
        )
    }

    /// Makes the pipelines and material bind groups `batches` need into
    /// `format`. Cheap when they exist: a lookup per batch with a look.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        textures: &[GpuTexture],
        batches: &[Batch],
        format: wgpu::TextureFormat,
    ) {
        let mut last = 0;
        for batch in batches {
            if batch.material == 0 || batch.material == last {
                continue;
            }
            last = batch.material;
            let (key, material) = self.resolve(batch.material, format);
            if !self.pipelines.contains_key(&key) {
                let module = match key.shader {
                    Some((id, _)) => self.shaders[id as usize].as_ref().map(|s| s.module.clone()),
                    None => Some(self.builtin.clone()),
                };
                if let Some(module) = module {
                    let pipeline = self.build(device, key, &module);
                    self.pipelines.insert(key, pipeline);
                }
            }
            if let Some(index) = material {
                self.bind_material(device, textures, index);
            }
        }
    }

    /// Makes material `index`'s bind group if it has none or a texture it
    /// shows was replaced.
    fn bind_material(&mut self, device: &wgpu::Device, textures: &[GpuTexture], index: usize) {
        let Some(material) = self.materials[index].as_mut() else {
            return;
        };
        let current = material.textures.map(|t| {
            t.and_then(|t| textures.get(t as usize))
                .map_or_else(|| self.white.clone(), |t| t.texture.clone())
        });
        if material.bind_group.as_ref().is_some_and(|(_, made)| *made == current) {
            return;
        }
        let views = current.clone().map(|t| {
            t.create_view(&wgpu::TextureViewDescriptor {
                mip_level_count: Some(1),
                ..Default::default()
            })
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: material.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        material.bind_group = Some((bind_group, current));
    }

    /// The pipeline and material bind group style key `style` draws with
    /// into `format`, once [`prepare`](Self::prepare)d. `None` if it
    /// couldn't be made (the batch is skipped).
    pub(crate) fn get(
        &self,
        style: u32,
        format: wgpu::TextureFormat,
    ) -> Option<(&wgpu::RenderPipeline, Option<&wgpu::BindGroup>)> {
        let (key, material) = self.resolve(style, format);
        let pipeline = self.pipelines.get(&key)?;
        let bind_group = match material {
            Some(index) => Some(&self.materials[index].as_ref()?.bind_group.as_ref()?.0),
            None => None,
        };
        Some((pipeline, bind_group))
    }

    fn build(&self, device: &wgpu::Device, key: Key, module: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
        let custom = key.shader.is_some();
        super::gpu::sprite_pipeline_with(
            device,
            if custom {
                &self.custom_layout
            } else {
                &self.builtin_layout
            },
            module,
            key.format,
            blend_state(key.blend),
            entry(key.blend, custom),
        )
    }
}

/// `SpriteMaterialDesc::params`: four `vec4<f32>`.
const PARAMS_BYTES: u64 = 64;

/// The error `scope` caught, if any. Native GPUs answer at once; a
/// browser's WebGPU answers later, and its rare late rejections (the
/// shader was checked against every supported GPU before) show in the
/// browser's console.
fn end_scope(scope: wgpu::ErrorScopeGuard) -> Option<String> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut future = Box::pin(scope.pop());
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(error) => error.map(|e| e.to_string()),
        #[cfg(target_arch = "wasm32")]
        Poll::Pending => None,
        #[cfg(not(target_arch = "wasm32"))]
        Poll::Pending => pollster::block_on(future).map(|e| e.to_string()),
    }
}

impl SpritePipelines {
    /// Counts sprite materials' parameters and the white stand-in as other
    /// memory (see `memory.rs`); their textures are the game's.
    pub(crate) fn tally_memory(&self, t: &mut super::memory::MemoryTally) {
        use super::memory::MemoryClass as C;
        t.texture(C::Other, &self.white);
        for material in self.materials.iter().flatten() {
            t.buffer(C::Other, &material.params);
        }
    }
}
