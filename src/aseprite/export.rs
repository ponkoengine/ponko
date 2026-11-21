use std::collections::{BTreeMap, BTreeSet};

use glam::{IVec2, UVec2};
use serde::Deserialize;

use super::{AsepriteSlice, Clip};
use crate::assets::Image;
use crate::camera::Rect;
use crate::error::{Error, ErrorKind};

#[derive(Deserialize)]
pub(super) struct Export {
    frames: Vec<Frame>,
    pub(super) meta: Meta,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Meta {
    pub(super) image: String,
    #[serde(default)]
    layers: Vec<Layer>,
    #[serde(default)]
    frame_tags: Vec<Tag>,
    #[serde(default)]
    slices: Vec<Slice>,
}

#[derive(Deserialize)]
struct Layer {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frame {
    #[serde(default)]
    filename: String,
    frame: PixelRect,
    #[serde(default)]
    rotated: bool,
    sprite_source_size: Option<PixelRect>,
    source_size: Size,
    duration: u32,
}

#[derive(Clone, Copy, Deserialize)]
struct Size {
    w: u32,
    h: u32,
}

#[derive(Clone, Copy, Deserialize)]
struct PixelRect {
    x: u32,
    y: u32,
    #[serde(flatten)]
    size: Size,
}

impl PixelRect {
    fn fits(self, w: u32, h: u32) -> bool {
        self.x.checked_add(self.size.w).is_some_and(|end| end <= w)
            && self.y.checked_add(self.size.h).is_some_and(|end| end <= h)
    }
}

#[derive(Deserialize)]
struct SignedRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl SignedRect {
    fn rect(&self) -> Rect {
        Rect::new(self.x as f32, self.y as f32, self.w as f32, self.h as f32)
    }
}

#[derive(Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct Slice {
    name: String,
    keys: Vec<SliceKey>,
}

#[derive(Deserialize)]
struct SliceKey {
    frame: u32,
    bounds: SignedRect,
    center: Option<SignedRect>,
    pivot: Option<Point>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    #[default]
    Forward,
    Reverse,
    Pingpong,
    PingpongReverse,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Repeat {
    Number(u32),
    Text(String),
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    from: u32,
    to: u32,
    #[serde(default)]
    direction: Direction,
    repeat: Option<Repeat>,
}

pub(super) struct Packed {
    pub size: UVec2,
    pub canvas: UVec2,
    pub rgba: Vec<u8>,
    pub durations: Vec<u32>,
    pub layers: Vec<String>,
    pub clips: BTreeMap<String, Clip>,
    pub slices: BTreeMap<String, Vec<AsepriteSlice>>,
}

fn malformed(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Decode, message)
}

impl Export {
    pub(super) fn parse(input: &str, path: &str) -> Result<Self, Error> {
        serde_json::from_str(input)
            .map_err(|e| malformed(format!("expected Aseprite JSON Array export: {e}")).with_path(path))
    }

    pub(super) fn pack(self, image: &Image, split: bool, max: u32) -> Result<Packed, Error> {
        let Some(first) = self.frames.first() else {
            return Err(malformed("Aseprite export has no frames"));
        };
        let Size { w, h } = first.source_size;
        if w == 0 || h == 0 {
            return Err(malformed("Aseprite canvas must have positive dimensions"));
        }
        let mut times = BTreeMap::new();
        let mut cels = BTreeMap::new();
        let mut names = BTreeSet::new();
        for (i, frame) in self.frames.iter().enumerate() {
            let (number, layer) = if split {
                let (number, layer) = frame
                    .filename
                    .split_once(':')
                    .ok_or_else(|| malformed("split Aseprite exports need filename format {frame}:{layer}"))?;
                let number = number
                    .parse::<u32>()
                    .map_err(|_| malformed("invalid split Aseprite frame index"))?;
                if layer.is_empty() {
                    return Err(malformed("split Aseprite frame has no layer name"));
                }
                names.insert(layer);
                (number, layer)
            } else {
                (u32::try_from(i).map_err(|_| atlas_limit())?, "")
            };
            if frame.duration == 0
                || times
                    .insert(number, frame.duration)
                    .is_some_and(|d| d != frame.duration)
            {
                return Err(malformed(format!("invalid duration for Aseprite frame {number}")));
            }
            if cels.insert((layer, number), frame).is_some() {
                return Err(malformed(format!("duplicate Aseprite cel {layer:?} frame {number}")));
            }
        }
        if times
            .keys()
            .copied()
            .ne(0..u32::try_from(times.len()).map_err(|_| atlas_limit())?)
        {
            return Err(malformed("Aseprite export must cover the entire timeline without gaps"));
        }
        let durations: Vec<_> = times.into_values().collect();
        let mut layers = Vec::new();
        for layer in &self.meta.layers {
            if names.remove(layer.name.as_str()) {
                layers.push(layer.name.clone());
            }
        }
        layers.extend(names.into_iter().map(str::to_owned));
        let slots = layers
            .len()
            .max(1)
            .checked_mul(durations.len())
            .ok_or_else(atlas_limit)?;
        let slots = u32::try_from(slots).map_err(|_| atlas_limit())?;
        let (max_columns, max_rows) = (max / w, max / h);
        if max_columns == 0 || max_rows == 0 || u64::from(slots) > u64::from(max_columns) * u64::from(max_rows) {
            return Err(atlas_limit());
        }
        let columns = slots.isqrt().max(slots.div_ceil(max_rows)).min(max_columns);
        let size = UVec2::new(columns * w, slots.div_ceil(columns) * h);
        let bytes = u64::from(size.x) * u64::from(size.y) * 4;
        if bytes > 256 * 1024 * 1024 {
            return Err(atlas_limit());
        }

        let mut clips = BTreeMap::from([("".to_owned(), clip(&durations, 0..durations.len() as u32))]);
        for tag in self.meta.frame_tags {
            let data = tag.clip(&durations)?;
            if clips.insert(tag.name, data).is_some() {
                return Err(malformed("duplicate or empty Aseprite tag name"));
            }
        }
        let mut slices = BTreeMap::new();
        for slice in self.meta.slices {
            let mut keys = Vec::new();
            for key in slice.keys {
                if key.frame as usize >= durations.len() {
                    return Err(malformed(format!(
                        "Aseprite slice {:?} has a key outside the timeline",
                        slice.name
                    )));
                }
                keys.push(AsepriteSlice {
                    frame: key.frame,
                    bounds: key.bounds.rect(),
                    center: key.center.map(|r| r.rect()),
                    pivot: key.pivot.map(|p| IVec2::new(p.x, p.y)),
                });
            }
            keys.sort_by_key(|key| key.frame);
            if keys.windows(2).any(|keys| keys[0].frame == keys[1].frame) || slices.insert(slice.name, keys).is_some() {
                return Err(malformed("duplicate Aseprite slice name or frame"));
            }
        }

        let mut rgba = vec![0; bytes as usize];
        let layer_names = if split {
            layers.iter().map(String::as_str).collect()
        } else {
            vec![""]
        };
        for (layer_index, layer) in layer_names.into_iter().enumerate() {
            for frame in 0..durations.len() {
                let Some(cel) = cels.get(&(layer, frame as u32)) else {
                    continue;
                };
                let i = (layer_index * durations.len() + frame) as u32;
                let at = UVec2::new(i % columns * w, i / columns * h);
                cel.copy(image, at, UVec2::new(w, h), size.x, &mut rgba)?;
            }
        }
        Ok(Packed {
            size,
            canvas: UVec2::new(w, h),
            rgba,
            durations,
            layers,
            clips,
            slices,
        })
    }
}

fn atlas_limit() -> Error {
    Error::new(ErrorKind::Limit, "Aseprite frames exceed the atlas size or byte limit")
}

impl Frame {
    fn copy(&self, image: &Image, at: UVec2, canvas: UVec2, width: u32, rgba: &mut [u8]) -> Result<(), Error> {
        if self.rotated {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "rotated Aseprite export frames are unsupported",
            ));
        }
        let src = self.frame;
        let dst = self.sprite_source_size.unwrap_or(PixelRect {
            x: 0,
            y: 0,
            size: src.size,
        });
        if (self.source_size.w, self.source_size.h) != (canvas.x, canvas.y)
            || (src.size.w, src.size.h) != (dst.size.w, dst.size.h)
            || !src.fits(image.width, image.height)
            || !dst.fits(canvas.x, canvas.y)
        {
            return Err(malformed(format!(
                "Aseprite cel {:?} does not fit its image or canvas",
                self.filename
            )));
        }

        let offset = |x: u32, y: u32, stride: u32| (y as usize * stride as usize + x as usize) * 4;
        for row in 0..src.size.h {
            let from = offset(src.x, src.y + row, image.width);
            let to = offset(at.x + dst.x, at.y + dst.y + row, width);
            let len = src.size.w as usize * 4;
            rgba[to..to + len].copy_from_slice(&image.rgba[from..from + len]);
        }
        Ok(())
    }
}

fn clip(durations: &[u32], frames: impl Iterator<Item = u32>) -> Clip {
    let mut duration = 0;
    let frames = frames
        .map(|frame| {
            duration += u64::from(durations[frame as usize]);
            (duration, frame)
        })
        .collect();
    Clip {
        frames,
        duration,
        finish: None,
    }
}

impl Tag {
    fn clip(&self, durations: &[u32]) -> Result<Clip, Error> {
        if self.from > self.to || self.to as usize >= durations.len() {
            return Err(malformed(format!(
                "Aseprite tag {:?} is outside the timeline",
                self.name
            )));
        }
        let repeats = match &self.repeat {
            None => 0,
            Some(Repeat::Number(n)) => *n,
            Some(Repeat::Text(s)) => s.parse().map_err(|_| malformed("invalid Aseprite tag repeat count"))?,
        };
        let range = self.from..=self.to;
        let inner = (self.from + 1)..self.to;
        let mut clip = match self.direction {
            Direction::Forward => clip(durations, range),
            Direction::Reverse => clip(durations, range.rev()),
            Direction::Pingpong => clip(durations, range.chain(inner.rev())),
            Direction::PingpongReverse => clip(durations, range.rev().chain(inner)),
        };
        if repeats > 0 {
            if matches!(self.direction, Direction::Pingpong | Direction::PingpongReverse) {
                // Aseprite counts each one-way traversal as a repeat.
                let cycles = f64::from(repeats / 2) * clip.duration as f64;
                let (extra, frame) = if repeats.is_multiple_of(2) {
                    (0, clip.frames[0].1)
                } else {
                    clip.frames[(self.to - self.from) as usize]
                };
                clip.finish = Some((cycles + extra as f64, frame));
            } else {
                clip.finish = clip
                    .frames
                    .last()
                    .map(|&(_, frame)| (f64::from(repeats) * clip.duration as f64, frame));
            }
        }
        Ok(clip)
    }
}

#[cfg(all(test, feature = "window", feature = "3d"))]
mod tests {
    use super::*;
    use crate::render::render_test_support::{gpu, read_texture};

    #[test]
    fn aseprite_atlas_gpu_pixels_restore_trimmed_cels_and_keep_blank_layers_transparent() {
        let Some(gpu) = gpu() else {
            return;
        };
        let input = include_str!("../../tests/fixtures/aseprite/layers.json");
        let image = Image {
            width: 2,
            height: 2,
            rgba: [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]].concat(),
        };
        let atlas = Export::parse(input, "layers.json")
            .unwrap()
            .pack(&image, true, 64)
            .unwrap();
        let pixels = upload_atlas(&gpu, &atlas);
        let cell_pixel = |slot: u32, x: u32, y: u32| {
            let columns = atlas.size.x / atlas.canvas.x;
            let x = slot % columns * atlas.canvas.x + x;
            let y = slot / columns * atlas.canvas.y + y;
            let offset = ((y * atlas.size.x + x) * 4) as usize;
            &pixels[offset..offset + 4]
        };
        assert_eq!(cell_pixel(0, 1, 0), [255, 0, 0, 255]);
        assert_eq!(
            cell_pixel(0, 0, 0),
            [0; 4],
            "trim placement preserves the canvas origin"
        );
        assert_eq!(cell_pixel(1, 0, 1), [0, 255, 0, 255]);
        assert_eq!(cell_pixel(2, 0, 0), [0, 0, 255, 255]);
        assert_eq!(cell_pixel(3, 1, 1), [255, 255, 0, 255], "hat cel");
        for slot in [4, 5] {
            for y in 0..2 {
                for x in 0..2 {
                    assert_eq!(cell_pixel(slot, x, y), [0; 4], "missing hat cel");
                }
            }
        }
    }

    #[cfg(feature = "aseprite-source")]
    #[test]
    fn aseprite_source_and_cooked_export_upload_identical_atlas_pixels() {
        let Some(gpu) = gpu() else {
            return;
        };
        let source = include_bytes!("../../tests/fixtures/cook/source/hero.aseprite");
        let cooked = include_str!("../../tests/fixtures/cook/cooked/hero.json");
        let png = include_bytes!("../../tests/fixtures/cook/cooked/hero.png");
        let sheet = ponko_aseprite::decode(source, false, 64).unwrap();
        let export: Export = serde_json::from_value(sheet.export).unwrap();
        let from_source = export
            .pack(
                &Image {
                    rgba: sheet.rgba,
                    width: sheet.width,
                    height: sheet.height,
                },
                false,
                64,
            )
            .unwrap();
        let image = crate::assets::decode_image(png, "hero.png", "hero.png").unwrap();
        let from_cook = Export::parse(cooked, "hero.json")
            .unwrap()
            .pack(&image, false, 64)
            .unwrap();
        let pixels = upload_atlas(&gpu, &from_source);
        assert_eq!(from_source.size, from_cook.size);
        assert_eq!(pixels, upload_atlas(&gpu, &from_cook));
        assert!(pixels.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
        assert!(pixels.as_chunks::<4>().0.contains(&[0; 4]));
    }

    fn upload_atlas(gpu: &crate::render::render_test_support::TestGpu, atlas: &Packed) -> Vec<u8> {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Aseprite atlas fixture"),
            size: wgpu::Extent3d {
                width: atlas.size.x,
                height: atlas.size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            texture.as_image_copy(),
            &atlas.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.size.x * 4),
                rows_per_image: None,
            },
            texture.size(),
        );
        read_texture(&gpu.device, &gpu.queue, &texture)
    }
}
