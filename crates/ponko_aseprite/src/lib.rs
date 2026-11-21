//! Decode Aseprite sources to sprite sheets without an Aseprite installation.
//! Used by the cook and by loose asset loading in development.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod render;
mod tiles;

use aseprite_loader::binary::{chunk::Chunk, chunks::tags::AnimationDirection, raw_file::parse_raw_file};
use serde_json::{Value, json};

const MAX_BYTES: usize = 256 * 1024 * 1024;

/// A sheet of RGBA8 frames (all whole canvases), with JSON Array metadata.
#[derive(Debug)]
pub struct Sheet {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels of the sheet, row by row.
    pub rgba: Vec<u8>,
    /// JSON Array metadata. Set `meta.image` to the sheet's name when exporting.
    pub export: Value,
    /// A description of the frames, layers, tags and slices.
    pub note: String,
}

/// Reads another file by its asset path, for [`decode_linked`]: its bytes,
/// or why it can't be read.
pub type Load<'a> = dyn FnMut(&str) -> Result<Vec<u8>, String> + 'a;

/// Reads a source file and builds a sheet. Set `split` for separately
/// composited visible layers, and `max_side` to the largest sheet dimension.
///
/// # Errors
///
/// Returns a message for invalid data, a tileset that links to another
/// file (use [`decode_linked`]), or a sheet that would exceed the size
/// limit.
pub fn decode(bytes: &[u8], split: bool, max_side: u32) -> Result<Sheet, String> {
    decode_file(None, bytes, split, max_side)
}

/// [`decode`] for the file at asset path `path`, whose tilesets may link
/// to a tileset in another file instead of holding their tiles (Aseprite's
/// external tilesets). `load` reads such a file by its asset path, which is
/// the link resolved against `path`'s folder (`tiles/ground.aseprite`
/// beside `maps/a.aseprite` is `maps/tiles/ground.aseprite`). A linked
/// tileset may link on, up to 8 files deep.
///
/// # Errors
///
/// As [`decode`], and when a linked file can't be loaded, isn't an
/// Aseprite file, lacks the tileset or has tiles of another size, or a
/// link is an absolute path or leaves the root.
pub fn decode_linked(
    path: &str,
    bytes: &[u8],
    split: bool,
    max_side: u32,
    load: &mut Load<'_>,
) -> Result<Sheet, String> {
    decode_file(Some((path, load)), bytes, split, max_side)
}

/// The asset paths of the files the tilesets of the file at asset path
/// `path` link to, resolved as [`decode_linked`] resolves them; not the
/// files those link to in turn. For a cache key, or files to watch.
///
/// # Errors
///
/// Returns a message for invalid data or a link that is an absolute path
/// or leaves the root.
pub fn linked_files(path: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    let file = parse_raw_file(bytes).map_err(|e| format!("not a readable Aseprite file: {e}"))?;
    render::linked_files(path, &file)
}

/// Whether `bytes` is an Aseprite file of only tilesets: it holds at least
/// one tileset and no layer, so nothing in it can be drawn. Other files
/// link to such a file for its tiles (see [`decode_linked`]).
///
/// # Errors
///
/// Returns a message for invalid data.
pub fn is_tileset_only(bytes: &[u8]) -> Result<bool, String> {
    let file = parse_raw_file(bytes).map_err(|e| format!("not a readable Aseprite file: {e}"))?;
    let chunks = || file.frames.iter().flat_map(|frame| &frame.chunks);
    Ok(chunks().any(|c| matches!(c, Chunk::Tileset(_))) && !chunks().any(|c| matches!(c, Chunk::Layer(_))))
}

fn decode_file(
    linked: Option<(&str, &mut Load<'_>)>,
    bytes: &[u8],
    split: bool,
    max_side: u32,
) -> Result<Sheet, String> {
    let file = parse_raw_file(bytes).map_err(|e| format!("not a readable Aseprite file: {e}"))?;
    let (w, h) = (u32::from(file.header.width), u32::from(file.header.height));
    if w == 0 || h == 0 || file.frames.is_empty() {
        return Err("it has no frames or an empty canvas".into());
    }
    let mut tags = Vec::new();
    let mut slices = Vec::new();
    for chunk in file.frames.iter().flat_map(|frame| &frame.chunks) {
        match chunk {
            Chunk::Tags(chunk) => tags.extend(&chunk.tags),
            Chunk::Slice(slice) => slices.push(slice),
            _ => {}
        }
    }
    let mut renderer = render::Renderer::new(&file)?;
    if let Some((path, load)) = linked {
        renderer.link_tilesets(path, load)?;
    }
    let layers = if split { renderer.split_layers()? } else { Vec::new() };
    let selections: Vec<_> = if split {
        layers.iter().map(|(i, name)| (Some(*i), Some(*name))).collect()
    } else {
        vec![(None, None)]
    };
    let frames = file.frames.len();
    let cells = frames
        .checked_mul(selections.len())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("too many sheet frames")?;
    let columns = cells.isqrt() + u32::from(!cells.isqrt().pow(2).eq(&cells));
    let rows = cells.div_ceil(columns);
    let width = columns.checked_mul(w).ok_or("sheet width overflow")?;
    let height = rows.checked_mul(h).ok_or("sheet height overflow")?;
    if width > max_side || height > max_side {
        return Err(format!(
            "its sheet would be {width} x {height} pixels, past {max_side}: split the file"
        ));
    }
    let mut rgba = vec![0; rgba_len(width, height)?];
    let mut canvas = vec![0; rgba_len(w, h)?];
    let mut frame_list = Vec::with_capacity(cells as usize);
    let mut cell = 0u32;
    for &(layer, name) in &selections {
        for (f, frame) in file.frames.iter().enumerate() {
            canvas.fill(0);
            renderer
                .render(f, layer, &mut canvas)
                .map_err(|e| format!("frame {f}: {e}"))?;
            let (x, y) = ((cell % columns) * w, (cell / columns) * h);
            for row in 0..h {
                let from = (row * w * 4) as usize;
                let to = (((y + row) * width + x) * 4) as usize;
                rgba[to..to + (w * 4) as usize].copy_from_slice(&canvas[from..from + (w * 4) as usize]);
            }
            let filename = match name {
                Some(name) => format!("{f}:{name}"),
                None => f.to_string(),
            };
            #[allow(deprecated)]
            let duration = if frame.duration == 0 {
                match file.header.speed {
                    0 => 100,
                    speed => speed,
                }
            } else {
                frame.duration
            };
            frame_list.push(json!({
                "filename": filename,
                "frame": { "x": x, "y": y, "w": w, "h": h },
                "rotated": false,
                "trimmed": false,
                "spriteSourceSize": { "x": 0, "y": 0, "w": w, "h": h },
                "sourceSize": { "w": w, "h": h },
                "duration": duration,
            }));
            cell += 1;
        }
    }

    let tags: Vec<Value> = tags
        .iter()
        .map(|t| {
            let direction = match t.animation_direction {
                AnimationDirection::Reverse => "reverse",
                AnimationDirection::PingPong => "pingpong",
                AnimationDirection::PingPongReverse => "pingpong_reverse",
                AnimationDirection::Forward | AnimationDirection::Unknown(_) => "forward",
            };
            let mut tag = json!({
                "name": t.name,
                "from": t.frames.start(),
                "to": t.frames.end(),
                "direction": direction,
            });
            if t.animation_repeat > 0 {
                tag["repeat"] = json!(t.animation_repeat.to_string());
            }
            tag
        })
        .collect();
    let slices: Vec<Value> = slices
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
    let note = format!(
        "{frames} frames{}, {} tags, {} slices, {w} x {h}",
        if split {
            format!(" x {} layers", layers.len())
        } else {
            String::new()
        },
        tags.len(),
        slices.len(),
    );
    let mut meta = json!({
        "image": "",
        "format": "RGBA8888",
        "size": { "w": width, "h": height },
        "scale": "1",
        "frameTags": tags,
        "slices": slices,
    });
    if split {
        meta["layers"] = Value::Array(layers.iter().map(|(_, n)| json!({ "name": n })).collect());
    }
    let export = json!({ "frames": frame_list, "meta": meta });
    Ok(Sheet {
        width,
        height,
        rgba,
        export,
        note,
    })
}

fn rgba_len(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes <= MAX_BYTES)
        .ok_or_else(|| "image exceeds the 256 MiB byte limit".into())
}
