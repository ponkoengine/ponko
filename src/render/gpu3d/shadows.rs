//! The shadow maps: the sun's cascades and the point and spot lights'
//! atlas, with the per-view frame uniforms their passes are drawn with.
//!
//! [`ShadowMaps`] owns the textures. Each is made again only when its size
//! (or the cascade count) changes, shrinks to a 1x1 stand-in while nothing
//! casts shadows, and is otherwise kept from frame to frame: which views
//! are redrawn in a frame is the shadow cache's decision
//! (`shadow_cache.rs`), not made here. A remade map is handed to the frame
//! sources, and the caller makes the frame bind groups again.

use super::{DEPTH_FORMAT, FrameSources, SHADOW_READBACK, shadow_cache};
use crate::render::traffic::Mirror;
use crate::three::Frame3D;

/// The sun's shadow map, the lights' shadow atlas and their passes' frames.
pub(super) struct ShadowMaps {
    /// The frame uniform alone, for the shadow pass (which can't also read
    /// the shadow map it draws into): one copy per cascade, each with its
    /// `caster_view_proj`, `stride` bytes apart.
    pub(super) frames: wgpu::Buffer,
    /// What each copy in `frames` holds.
    pub(super) mirrors: Vec<Mirror>,
    pub(super) stride: u64,
    pub(super) bind_group: wgpu::BindGroup,
    /// Each cascade's layer of the shadow map, drawn into.
    pub(super) layers: Vec<wgpu::TextureView>,
    /// The sun's shadow map as made: its width and its layers (a 1x1 one
    /// without shadows).
    pub(super) dims: (u32, u32),
    /// The sun's shadow map, one layer per cascade (tests read it back).
    #[cfg(test)]
    pub(super) map: wgpu::Texture,
    /// The width of the point and spot lights' shadow atlas, once made.
    pub(super) light_dims: Option<u32>,
    /// The atlas, drawn into (each view into its tile); `None` until a
    /// light first casts shadows.
    pub(super) light_atlas: Option<wgpu::TextureView>,
    /// Clears one light atlas tile (made with the atlas).
    pub(super) tile_clear: Option<wgpu::RenderPipeline>,
    /// The 1x1 light shadow map frame bind groups use without light shadows.
    pub(super) no_light_shadow: wgpu::TextureView,
}

impl ShadowMaps {
    /// Makes the sun's shadow map for this frame's size and cascades, if it
    /// isn't the one made already. Without shadows it shrinks to a 1x1
    /// stand-in, freeing the memory. The shadow passes redraw every layer
    /// each frame, so a new map never shows what the old one held.
    ///
    /// True when the map was made again: kept shadow views are stale, and
    /// the frame bind groups must be made again from `sources`.
    pub(super) fn set_sun_map(&mut self, device: &wgpu::Device, frame: &Frame3D, sources: &mut FrameSources) -> bool {
        let wanted = if frame.cascades > 0 {
            (frame.shadow_size.max(1), frame.cascades as u32)
        } else {
            (1, 1)
        };
        if self.dims == wanted {
            return false;
        }
        let map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d shadow map"),
            size: wgpu::Extent3d {
                width: wanted.0,
                height: wanted.0,
                depth_or_array_layers: wanted.1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | SHADOW_READBACK,
            view_formats: &[],
        });
        sources.shadow_view = map.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        #[cfg(test)]
        {
            self.map = map.clone();
        }
        self.layers = (0..wanted.1)
            .map(|layer| {
                map.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("3d shadow cascade"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        self.dims = wanted;
        true
    }

    /// Makes the point and spot lights' shadow atlas (the first time a light
    /// casts shadows) and binds it to every frame. Made again when its size
    /// changes, and freed when point and spot light shadows are turned off.
    ///
    /// True when the atlas changed (as [`ShadowMaps::set_sun_map`]).
    pub(super) fn set_light_map(&mut self, device: &wgpu::Device, frame: &Frame3D, sources: &mut FrameSources) -> bool {
        let wanted = if frame.light_budget == 0 {
            None
        } else if frame.light_views > 0 || self.light_dims.is_some() {
            Some(frame.light_atlas.max(1))
        } else {
            return false; // not needed yet
        };
        if self.light_dims == wanted {
            return false;
        }
        self.light_dims = wanted;
        let Some(size) = wanted else {
            // Back to the 1x1 stand-in the frame bind groups start with.
            self.light_atlas = None;
            sources.light_shadow_view = self.no_light_shadow.clone();
            return true;
        };
        let map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d light shadow atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = map.create_view(&Default::default());
        if self.tile_clear.is_none() {
            self.tile_clear = Some(shadow_cache::tile_clear_pipeline(device));
        }
        sources.light_shadow_view = view.clone();
        self.light_atlas = Some(view);
        true
    }
}

impl ShadowMaps {
    /// Counts the sun's shadow map and its passes' uniforms as shadows, and
    /// the lights' atlas as atlases (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        t.buffer(C::Shadows, &self.frames);
        for layer in &self.layers {
            t.view(C::Shadows, layer);
        }
        if let Some(atlas) = &self.light_atlas {
            t.view(C::Atlases, atlas);
        }
        t.view(C::Atlases, &self.no_light_shadow);
    }
}
