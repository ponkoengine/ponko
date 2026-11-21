//! The textures UI groups fading as one image are drawn into
//! (`DrawList::offscreen`, `ui::group_fade`), the screen's and world-space
//! panels': one per layer drawn at once, kept between frames and made
//! again only when the size the UI asks for (`group_fade::layer_texture_size`)
//! or the target's format changes. Each layer is drawn through a camera of
//! its own: the target's, moved to the layer's origin.

use super::{GpuTexture, Renderer, SpritePass, batch::Offscreen};
use crate::color::Color;
use crate::render::traffic::{Counted, Upload};
use glam::{Mat4, Vec2, Vec3};

/// One layer texture: its id, size and format (`None` until first drawn).
pub(super) struct UiLayerSlot {
    id: u32,
    size: [u32; 2],
    format: Option<wgpu::TextureFormat>,
}

impl Renderer {
    /// The id of the texture of the `slot`th layer a fading UI group is
    /// drawn into this frame (`DrawList::offscreen`), made the first time.
    /// It has the size and format its layer needs when a frame draws into it.
    pub fn ui_layer_texture(&mut self, slot: usize) -> u32 {
        while self.ui_layers.len() <= slot {
            let texture = self.ui_layer_target([1, 1], self.view_format);
            let bind_group = self.sprite_bind_group(&texture, true);
            let id = self.textures.add(GpuTexture {
                bind_group,
                texture,
                nearest: true,
                #[cfg(feature = "3d")]
                mips: Some(false),
            });
            self.ui_layers.push(UiLayerSlot {
                id,
                size: [0, 0],
                format: None,
            });
        }
        self.ui_layers[slot].id
    }

    /// How many layer textures were made so far (a texture made again
    /// in another size counts again).
    #[cfg(test)]
    pub(crate) fn ui_layer_textures_made(&self) -> u64 {
        self.ui_layers_made
    }

    /// `layer`'s texture made its size and `format`, under the same id, if
    /// it isn't.
    pub(super) fn fit_ui_layer(&mut self, layer: &Offscreen, format: wgpu::TextureFormat) {
        if layer.size[0] == 0 || layer.size[1] == 0 {
            return;
        }
        let Some(slot) = self.ui_layers.iter().position(|s| s.id == layer.texture) else {
            return;
        };
        let (size, made) = (layer.texture_size, &self.ui_layers[slot]);
        if made.size == size && made.format == Some(format) || self.textures.live(layer.texture).is_none() {
            return;
        }
        let texture = self.ui_layer_target(size, format);
        let bind_group = self.sprite_bind_group(&texture, true);
        if let Some(live) = self.textures.live_mut(layer.texture) {
            live.texture = texture;
            live.bind_group = bind_group;
        }
        self.ui_layers[slot].size = size;
        self.ui_layers[slot].format = Some(format);
        self.ui_layers_made += 1;
    }

    /// A texture of `size` sprites can be drawn into, as onto the target,
    /// and then sampled.
    fn ui_layer_target(&self, [width, height]: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui group layer"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// Writes the camera of this frame's `i`th layer (made the first time):
    /// the target's projection in `layer.scale` pixels a unit, its top-left
    /// at the layer's origin, over the layer's texture.
    pub(super) fn ui_layer_camera(&mut self, i: usize, layer: &Offscreen) {
        while self.ui_layer_cameras.len() <= i {
            let camera = self.make_camera("ui layer camera");
            self.ui_layer_cameras.push(camera);
        }
        let scale = layer.scale.max(f32::MIN_POSITIVE);
        let [w, h] = layer.texture_size;
        let origin = Vec2::new(layer.origin[0] as f32, layer.origin[1] as f32) / scale;
        let view = crate::ctx::screen_projection(Vec2::new(w as f32, h as f32) / scale)
            * Mat4::from_translation(Vec3::new(-origin.x, -origin.y, 0.0));
        self.queue.uploading(Upload::Uniforms).write_buffer(
            &self.ui_layer_cameras[i].0,
            0,
            bytemuck::cast_slice(&view.to_cols_array()),
        );
    }

    /// Records `layers` (each with where its sprites start and its camera's
    /// index), innermost first, each cleared to transparent and drawn with
    /// `pass`'s pipeline into its texture's top-left `size` pixels.
    pub(super) fn draw_ui_layers(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layers: &[(&Offscreen, u32, usize)],
        pass: &SpritePass<'_>,
        format: wgpu::TextureFormat,
    ) {
        // A group inside another comes after it: drawn before it.
        for &(layer, start, camera) in layers.iter().rev() {
            if layer.size[0] == 0 || layer.size[1] == 0 {
                continue;
            }
            let Some(target) = self.textures.live(layer.texture) else {
                continue;
            };
            let view = target.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let cameras = [(&self.ui_layer_cameras[camera].1, layer.finished().1, start)];
            let origin = [layer.origin[0] as f32, layer.origin[1] as f32];
            Self::sprite_pass_from(
                encoder,
                &view,
                Some(Color::TRANSPARENT),
                self.timer.as_ref().and_then(|t| t.pass("ui layers")),
                &SpritePass {
                    viewport: layer.size,
                    scale: layer.scale,
                    layers: &cameras,
                    ..*pass
                },
                Some((&self.sprite_pipelines, format)),
                origin,
            );
        }
    }
}
