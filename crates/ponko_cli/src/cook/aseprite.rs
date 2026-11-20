//! Aseprite files (`.aseprite`, `.ase`) read at cook time, with no export
//! step: each becomes the JSON Array sprite sheet and PNG that Aseprite's
//! own export makes (`hero.aseprite` -> `hero.json` and `hero.png`), which
//! the engine's `load_aseprite` reads. The game loads `hero.json`; the
//! engine needs no Aseprite reader, so games don't pay for one.
//!
//! The file is parsed by the `aseprite-loader` crate. Each frame is its
//! whole canvas, composited the way Aseprite exports it: visible layers
//! (inside visible groups), their blend modes, layer and cel opacity.
//! Durations, tags (direction and repeat count) and slices (bounds,
//! nine-slice centers and pivots, per key) are written as Aseprite writes
//! them. With `"layers": true` in `cook.json` the frames are split by layer
//! (Aseprite's `--split-layers`, named `{frame}:{layer}`, with the layer
//! list), for `load_aseprite(path, true)`.
//!
//! Tilemap layers are refused (the cook names the file): export those from
//! Aseprite instead.

#![deny(missing_docs)]

use aseprite_loader::binary::chunks::layer::LayerType;
use aseprite_loader::binary::chunks::tags::AnimationDirection;
use aseprite_loader::loader::{AsepriteFile, LayerSelection};
use serde_json::{Value, json};

/// Whether `name` is an Aseprite file.
pub fn is_aseprite(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".aseprite") || lower.ends_with(".ase")
}

/// The names an Aseprite file's sheet is written under: its JSON and PNG,
/// beside it.
pub fn outputs(name: &str) -> (String, String) {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    (format!("{stem}.json"), format!("{stem}.png"))
}

/// What [`export`] made of an Aseprite file.
#[derive(Debug)]
pub struct Sheet {
    /// The JSON Array export.
    pub json: Vec<u8>,
    /// The sheet: every frame (of every layer, when split), one canvas
    /// each, in rows.
    pub png: Vec<u8>,
    /// For the report: frames, layers, tags and slices.
    pub note: String,
}

/// Reads Aseprite file `name` and writes its sheet; `split` splits the
/// frames by layer.
pub fn export(name: &str, bytes: &[u8], split: bool) -> Result<Sheet, String> {
    let fail = |e: String| format!("{name}: {e}");
    let mut file = AsepriteFile::load(bytes).map_err(|e| fail(format!("not a readable Aseprite file: {e}")))?;
    let (width, height) = file.size();
    let (w, h) = (u32::from(width), u32::from(height));
    if w == 0 || h == 0 || file.frames.is_empty() {
        return Err(fail("it has no frames or an empty canvas".into()));
    }
    // The loader's layers skip tilemap layers, so cels would point at the
    // wrong layer: refuse them.
    if file.file.layers.iter().any(|l| l.layer_type == LayerType::Tilemap) {
        return Err(fail(
            "it has a tilemap layer, which the cook doesn't read: export it from Aseprite (the guide's \"Sprites and animations from Aseprite\")".into(),
        ));
    }
    // Every cel's pixels decode (the loader's compositing expects them to).
    for i in 0..file.images.len() {
        let image = &file.images[i];
        let mut pixels = vec![0u8; usize::from(image.width) * usize::from(image.height) * 4];
        file.load_image(i, &mut pixels)
            .map_err(|e| fail(format!("a cel's pixels can't be read: {e}")))?;
    }
    // A layer shows when it and every group above it are visible.
    let mut shown = Vec::with_capacity(file.file.layers.len());
    let mut groups: Vec<bool> = Vec::new();
    for layer in &file.file.layers {
        let level = usize::from(layer.child_level);
        groups.truncate(level);
        let parent = groups.last().copied().unwrap_or(true);
        let visible = parent && file.layers[shown.len()].visible;
        shown.push(visible && layer.layer_type == LayerType::Normal);
        if layer.layer_type == LayerType::Group {
            groups.resize(level, true);
            groups.push(visible);
        }
    }
    // The header says whether layer opacity is set (else every layer is opaque).
    let layer_opacity_valid = file.file.header.flags & 1 != 0;
    let opacities: Vec<u8> = file
        .layers
        .iter()
        .map(|l| if layer_opacity_valid { l.opacity } else { 255 })
        .collect();
    let layers: Vec<(usize, String)> = if split {
        shown
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| (i, file.layers[i].name.clone()))
            .collect()
    } else {
        Vec::new()
    };
    if split {
        let mut names: Vec<&str> = layers.iter().map(|(_, n)| n.as_str()).collect();
        names.sort_unstable();
        if let Some(pair) = names.windows(2).find(|p| p[0] == p[1]) {
            return Err(fail(format!(
                "two visible layers are named \"{}\": split layers need names of their own",
                pair[0]
            )));
        }
        if layers.is_empty() {
            return Err(fail("it has no visible layer to split".into()));
        }
    }

    // Each image of the sheet: (frame, layer name), composited.
    let frames = file.frames.len();
    let selections: Vec<(Option<&str>, LayerSelection)> = if split {
        layers
            .iter()
            .map(|(i, n)| {
                let mask = (0..shown.len()).map(|k| k == *i).collect();
                (Some(n.as_str()), LayerSelection::Mask(mask))
            })
            .collect()
    } else {
        vec![(None, LayerSelection::Mask(shown.clone()))]
    };
    let cells = frames * selections.len();
    let columns = (cells as f64).sqrt().ceil().max(1.0) as u32;
    let rows = (cells as u32).div_ceil(columns);
    let (sheet_w, sheet_h) = (columns * w, rows * h);
    if sheet_w > 16384 || sheet_h > 16384 {
        return Err(fail(format!(
            "its sheet would be {sheet_w} x {sheet_h} pixels, past 16384: split the file"
        )));
    }
    let mut sheet = vec![0u8; (sheet_w * sheet_h * 4) as usize];
    let mut frame_list = Vec::with_capacity(cells);
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let durations: Vec<u32> = file
        .frames
        .iter()
        .map(|f| if f.duration == 0 { 100 } else { u32::from(f.duration) })
        .collect();
    let mut cell = 0u32;
    for (layer, selection) in &selections {
        for f in 0..frames {
            // Layer opacity times the cel's, as Aseprite composites.
            for (l, layer) in file.layers.iter_mut().enumerate() {
                let cel = file.file.frames[f]
                    .cels
                    .get(l)
                    .and_then(Option::as_ref)
                    .map_or(255, |c| c.opacity);
                layer.opacity = ((u32::from(opacities[l]) * u32::from(cel) + 127) / 255) as u8;
            }
            canvas.fill(0);
            file.render_frame(f, &mut canvas, selection)
                .map_err(|e| fail(format!("frame {f} can't be drawn: {e}")))?;
            let (x, y) = ((cell % columns) * w, (cell / columns) * h);
            for row in 0..h {
                let from = (row * w * 4) as usize;
                let to = (((y + row) * sheet_w + x) * 4) as usize;
                sheet[to..to + (w * 4) as usize].copy_from_slice(&canvas[from..from + (w * 4) as usize]);
            }
            let filename = match layer {
                Some(layer) => format!("{f}:{layer}"),
                None => format!("{f}"),
            };
            frame_list.push(json!({
                "filename": filename,
                "frame": { "x": x, "y": y, "w": w, "h": h },
                "rotated": false,
                "trimmed": false,
                "spriteSourceSize": { "x": 0, "y": 0, "w": w, "h": h },
                "sourceSize": { "w": w, "h": h },
                "duration": durations[f],
            }));
            cell += 1;
        }
    }

    let tags: Vec<Value> = file
        .tags
        .iter()
        .map(|t| {
            let direction = match t.direction {
                AnimationDirection::Reverse => "reverse",
                AnimationDirection::PingPong => "pingpong",
                AnimationDirection::PingPongReverse => "pingpong_reverse",
                AnimationDirection::Forward | AnimationDirection::Unknown(_) => "forward",
            };
            let mut tag = json!({
                "name": t.name,
                "from": t.range.start(),
                "to": t.range.end(),
                "direction": direction,
            });
            if let Some(repeat) = t.repeat {
                tag["repeat"] = json!(repeat.to_string());
            }
            tag
        })
        .collect();
    let slices: Vec<Value> = file
        .slices()
        .iter()
        .map(|s| {
            let keys: Vec<Value> = s
                .slice_keys
                .iter()
                .map(|k| {
                    let mut key = json!({
                        "frame": k.frame_number,
                        "bounds": { "x": k.x, "y": k.y, "w": k.width, "h": k.height },
                    });
                    if let Some(c) = &k.nine_patch {
                        key["center"] = json!({ "x": c.x, "y": c.y, "w": c.width, "h": c.height });
                    }
                    if let Some(p) = &k.pivot {
                        key["pivot"] = json!({ "x": p.x, "y": p.y });
                    }
                    key
                })
                .collect();
            json!({ "name": s.name, "color": "#0000ffff", "keys": keys })
        })
        .collect();
    let png_name = outputs(name).1;
    let png_name = png_name.rsplit('/').next().unwrap_or(&png_name).to_string();
    let mut meta = json!({
        "app": "ponko cook",
        "version": env!("CARGO_PKG_VERSION"),
        "image": png_name,
        "format": "RGBA8888",
        "size": { "w": sheet_w, "h": sheet_h },
        "scale": "1",
        "frameTags": tags,
        "slices": slices,
    });
    if split {
        meta["layers"] = Value::Array(layers.iter().map(|(_, n)| json!({ "name": n })).collect());
    }
    let out = json!({ "frames": frame_list, "meta": meta });
    let mut text = serde_json::to_vec_pretty(&out).map_err(|e| fail(e.to_string()))?;
    text.push(b'\n');

    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        ),
        &sheet,
        sheet_w,
        sheet_h,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| fail(format!("its sheet can't be written as PNG: {e}")))?;
    let note = format!(
        "{frames} frames{}, {} tags, {} slices, {w} x {h}",
        if split {
            format!(" x {} layers", layers.len())
        } else {
            String::new()
        },
        file.tags.len(),
        file.slices().len()
    );
    Ok(Sheet { json: text, png, note })
}

#[cfg(test)]
pub(crate) mod tests;
