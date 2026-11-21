use aseprite_loader::binary::{
    chunks::{
        cel::CelContent,
        tileset::{TilesetChunk, TilesetFlags},
    },
    color_depth::ColorDepth,
    header::Header,
    palette::Palette,
};

use crate::{
    render::{decode_pixels, inflate, to_rgba},
    rgba_len,
};

pub(super) struct Tileset {
    width: usize,
    height: usize,
    count: usize,
    zero_empty: bool,
    rgba: Vec<u8>,
}

pub(super) struct Raster {
    pub(super) origin: (i16, i16),
    pub(super) size: (u16, u16),
    pub(super) rgba: Vec<u8>,
}

impl Tileset {
    pub(super) fn decode(
        chunk: &TilesetChunk<'_>,
        depth: ColorDepth,
        palette: Option<&Palette>,
        budget: usize,
    ) -> Result<Self, String> {
        let raw = chunk
            .tiles
            .as_ref()
            .ok_or_else(|| format!("tileset {:?} has no embedded tiles; embed them in Aseprite", chunk.name))?;
        let (width, height) = (usize::from(chunk.width), usize::from(chunk.height));
        let count = chunk.number_of_tiles as usize;
        let pixels = width
            .checked_mul(height)
            .and_then(|area| area.checked_mul(count))
            .filter(|&n| n > 0 && n <= budget / 4)
            .ok_or("invalid tileset dimensions or tilesets exceed the 256 MiB byte limit")?;
        let bytes = decode_pixels(raw.data, true, pixels, depth)?;
        let rgba = to_rgba(&bytes, depth, palette)?.into_owned();
        Ok(Self {
            width,
            height,
            count,
            zero_empty: chunk.flags.contains(TilesetFlags::TILE_0_EMPTY),
            rgba,
        })
    }

    pub(super) fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    pub(super) fn render(
        &self,
        content: &CelContent<'_>,
        origin: (i16, i16),
        header: &Header,
    ) -> Result<Option<Raster>, String> {
        let CelContent::CompressedTilemap {
            width,
            height,
            bits_per_tile,
            bitmask_tile_id: id,
            bitmask_x_flip,
            bitmask_y_flip,
            bitmask_diagonal_flip: diagonal,
            data,
        } = *content
        else {
            return Err("expected a tilemap cel".into());
        };
        if bits_per_tile != 32 || id == 0 {
            return Err("expected 32-bit tiles with a tile ID mask".into());
        }
        // aseprite-loader 0.4.2 reads the X and Y fields in the opposite
        // order to the Aseprite file specification.
        let (horizontal, vertical) = (bitmask_y_flip, bitmask_x_flip);
        let map_w = usize::from(width);
        let map_h = usize::from(height);
        if map_w == 0 || map_h == 0 {
            return Ok(None);
        }
        let map = inflate(data, rgba_len(width.into(), height.into())?)?;

        let (x, y) = (i64::from(origin.0), i64::from(origin.1));
        let (left, top) = (x.max(0), y.max(0));
        let right = (x + (map_w * self.width) as i64).min(i64::from(header.width));
        let bottom = (y + (map_h * self.height) as i64).min(i64::from(header.height));
        if right <= left || bottom <= top {
            return Ok(None);
        }
        let size = ((right - left) as u16, (bottom - top) as u16);
        let mut rgba = vec![0; rgba_len(size.0.into(), size.1.into())?];
        let map = map.as_chunks::<4>().0;
        let flag = |tile, mask| mask != 0 && tile & mask == mask;
        for (row, pixels) in rgba.chunks_exact_mut(usize::from(size.0) * 4).enumerate() {
            let local_y = (top - y) as usize + row;
            for (col, dst) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let local_x = (left - x) as usize + col;
                let tile = u32::from_le_bytes(map[(local_y / self.height) * map_w + local_x / self.width]);
                let index = ((tile & id) >> id.trailing_zeros()) as usize;
                if index >= self.count || if self.zero_empty { index == 0 } else { tile == u32::MAX } {
                    continue;
                }
                let (mut sx, mut sy) = (local_x % self.width, local_y % self.height);
                if flag(tile, horizontal) {
                    sx = self.width - 1 - sx;
                }
                if flag(tile, vertical) {
                    sy = self.height - 1 - sy;
                }
                if flag(tile, diagonal) {
                    (sx, sy) = (sy, sx);
                    let extent = self.width.min(self.height);
                    if sx >= extent || sy >= extent {
                        continue;
                    }
                }
                let at = ((index * self.height + sy) * self.width + sx) * 4;
                dst.copy_from_slice(&self.rgba[at..at + 4]);
            }
        }
        Ok(Some(Raster {
            origin: (left as i16, top as i16),
            size,
            rgba,
        }))
    }
}
