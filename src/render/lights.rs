//! Camera-local light textures, grown only when a new high-water mark is reached.
use crate::render::traffic::{Counted, Upload};
use crate::three::lights::{LIST_WIDTH, LightGrid};
use crate::three::projected::{DECAL_TEXELS, DecalGrid};

pub(super) struct LightTextures {
    pub records: wgpu::Texture,
    pub indices: wgpu::Texture,
    pub records_view: wgpu::TextureView,
    pub indices_view: wgpu::TextureView,
    /// What the four writes (lights, decals; light lists, decal lists)
    /// last put where, to skip those that would write the same again.
    written: [Written; 4],
}

/// One region of a light texture as last written: its first row and data.
#[derive(Default)]
struct Written {
    row: u32,
    mirror: crate::render::traffic::Mirror,
}
impl LightTextures {
    pub fn new(device: &wgpu::Device) -> Self {
        // Four texels per light, then eight per decal.
        let records = texture(
            device,
            "punctual lights",
            DECAL_TEXELS as u32,
            1,
            wgpu::TextureFormat::Rgba32Float,
        );
        let indices = texture(
            device,
            "light clusters",
            LIST_WIDTH as u32,
            8,
            wgpu::TextureFormat::R32Uint,
        );
        let records_view = records.create_view(&Default::default());
        let indices_view = indices.create_view(&Default::default());
        Self {
            records,
            indices,
            records_view,
            indices_view,
            written: Default::default(),
        }
    }
    /// True when bindings must be rebuilt (growth only). Lights come first
    /// in both textures, then the decals' rows.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, grid: &LightGrid, decals: &DecalGrid) -> bool {
        let light_rows = grid.lights.len() as u32;
        let list_rows = (grid.lists.len() / LIST_WIDTH) as u32;
        let mut changed = false;
        for (target, view, height) in [
            (
                &mut self.records,
                &mut self.records_view,
                (light_rows + decals.records.len() as u32).max(1),
            ),
            (
                &mut self.indices,
                &mut self.indices_view,
                (list_rows + (decals.lists.len() / LIST_WIDTH) as u32).max(1),
            ),
        ] {
            if height > target.height() {
                let height = height.next_power_of_two();
                super::traffic::reallocated();
                assert!(
                    height <= device.limits().max_texture_dimension_2d,
                    "punctual light texture exceeds adapter capacity"
                );
                *target = texture(device, "grown light data", target.width(), height, target.format());
                *view = target.create_view(&Default::default());
                changed = true;
            }
        }
        if changed {
            // New textures: nothing in them yet.
            for w in &mut self.written {
                w.mirror.forget();
            }
        }
        let [lights, decal_records, lists, decal_lists] = &mut self.written;
        write(
            queue,
            lights,
            &self.records,
            0,
            4,
            bytemuck::cast_slice(&grid.lights),
            light_rows,
            64,
        );
        write(
            queue,
            decal_records,
            &self.records,
            light_rows,
            DECAL_TEXELS as u32,
            bytemuck::cast_slice(&decals.records),
            decals.records.len() as u32,
            DECAL_TEXELS as u32 * 16,
        );
        write(
            queue,
            lists,
            &self.indices,
            0,
            LIST_WIDTH as u32,
            bytemuck::cast_slice(&grid.lists),
            list_rows,
            LIST_WIDTH as u32 * 4,
        );
        write(
            queue,
            decal_lists,
            &self.indices,
            list_rows,
            LIST_WIDTH as u32,
            bytemuck::cast_slice(&decals.lists),
            (decals.lists.len() / LIST_WIDTH) as u32,
            LIST_WIDTH as u32 * 4,
        );
        changed
    }
}
fn texture(device: &wgpu::Device, label: &str, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
/// Writes `height` rows of `data` from `first_row`, unless `written` says
/// they're already there.
#[allow(clippy::too_many_arguments)]
fn write(
    queue: &wgpu::Queue,
    written: &mut Written,
    texture: &wgpu::Texture,
    first_row: u32,
    width: u32,
    data: &[u8],
    height: u32,
    row: u32,
) {
    if height == 0 {
        return;
    }
    if written.row != first_row {
        written.mirror.forget();
        written.row = first_row;
    }
    let mut changed = false;
    written.mirror.sync(0, data, data.len(), |_, _| changed = true);
    if !changed {
        return;
    }
    queue.uploading(Upload::Lights).write_texture(
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d {
                x: 0,
                y: first_row,
                z: 0,
            },
            ..texture.as_image_copy()
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

impl LightTextures {
    /// Counts the light and decal grids under `class` (see `memory.rs`).
    pub fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally, class: crate::render::memory::MemoryClass) {
        t.texture(class, &self.records);
        t.texture(class, &self.indices);
    }
}
