//! Serialize a model whose geometry must stay in its original layout.
//! Compressed views can address buffers by index and raw byte offset, so
//! original buffer and view indices and every payload stay intact.
//! The bytes of an image the cook replaced with KTX2 are dropped from the
//! buffers this file holds itself (the GLB's own chunk, data URIs): every
//! later view and compressed stream moves down by a multiple of 16 bytes,
//! so its alignment and its bytes stay the same. An external buffer file
//! gets the same treatment when the cook knows this model is its only
//! reader; the shortened file goes back to the cook as a companion.

use std::{collections::BTreeSet, ops::Range};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{Companions, Doc, encode_glb};

/// Offsets move by multiples of this, which keeps every view's alignment
/// (glTF asks for at most 4; meshopt decoders like 16).
const ALIGN: usize = 16;

/// The buffer views the file's images are stored in, before the cook
/// points them at KTX2 views.
pub(super) fn image_views(json: &Value) -> Vec<usize> {
    let images = json["images"].as_array().into_iter().flatten();
    images
        .filter_map(|image| image["bufferView"].as_u64().map(|v| v as usize))
        .collect()
}

/// Writes the model; also returns how many bytes of replaced images were
/// dropped from the buffers it holds and from `owned` (external buffers,
/// by index and path, that no other model reads), and each shortened
/// external buffer file as (path, bytes).
pub(super) fn write(
    mut doc: Doc,
    original_buffers: usize,
    binary: bool,
    has_bin: bool,
    image_views: &[usize],
    owned: &[(usize, String)],
) -> Result<(Vec<u8>, usize, Companions), String> {
    let appended = doc.buffers.split_off(original_buffers);
    // Drop the replaced images from the buffers this file holds: the BIN
    // chunk, and data-URI buffers, and from the external files only this
    // model reads. Any other external file may have other readers.
    let mut dropped = 0;
    let mut companions = Vec::new();
    for (b, data) in doc.buffers.iter_mut().enumerate() {
        let data_uri = doc.json["buffers"][b]["uri"]
            .as_str()
            .is_some_and(|uri| uri.starts_with("data:"));
        let own_file = owned.iter().find(|(i, _)| *i == b).map(|(_, path)| path);
        if !(data_uri || (b == 0 && has_bin) || own_file.is_some()) {
            continue;
        }
        let removed = drop_replaced_images(&mut doc.json, b, data, image_views);
        if removed > 0 && data_uri {
            doc.json["buffers"][b]["byteLength"] = json!(data.len());
            doc.json["buffers"][b]["uri"] = json!(format!(
                "data:application/octet-stream;base64,{}",
                STANDARD.encode(&data)
            ));
        } else if removed > 0
            && let Some(path) = own_file
        {
            doc.json["buffers"][b]["byteLength"] = json!(data.len());
            companions.push((path.clone(), data.clone()));
        }
        dropped += removed;
    }
    let mut data = if has_bin {
        std::mem::take(doc.buffers.first_mut().ok_or("binary model has no buffer")?)
    } else {
        Vec::new()
    };
    // Put the new KTX2 views in the existing BIN chunk, or in one new
    // data-URI buffer if the file did not have a BIN chunk.
    let buffer = if has_bin { 0 } else { original_buffers };
    let mut offsets = Vec::with_capacity(appended.len());
    for bytes in appended {
        data.resize(data.len().next_multiple_of(4), 0);
        offsets.push(data.len());
        data.extend(bytes);
    }
    for view in doc.json["bufferViews"].as_array_mut().into_iter().flatten() {
        let Some(index) = view["buffer"].as_u64().map(|i| i as usize) else {
            continue;
        };
        if let Some(&offset) = index.checked_sub(original_buffers).and_then(|i| offsets.get(i)) {
            view["buffer"] = json!(buffer);
            view["byteOffset"] = json!(offset);
        }
    }
    let bin = if has_bin {
        doc.json["buffers"][0]["byteLength"] = json!(data.len());
        data.resize(data.len().next_multiple_of(4), 0);
        data
    } else {
        if doc.json.get("buffers").is_none() {
            doc.json["buffers"] = json!([]);
        }
        let buffers = doc.json["buffers"]
            .as_array_mut()
            .ok_or("model buffers must be an array")?;
        buffers.push(json!({
            "byteLength": data.len(),
            "uri": format!("data:application/octet-stream;base64,{}", STANDARD.encode(&data)),
        }));
        Vec::new()
    };
    let out = if binary {
        encode_glb(&doc.json, &bin)?
    } else {
        serde_json::to_vec(&doc.json).map_err(|e| e.to_string())?
    };
    Ok((out, dropped, companions))
}

fn range(place: &Value) -> Option<Range<usize>> {
    let start = place["byteOffset"].as_u64().unwrap_or(0) as usize;
    let end = start.checked_add(place["byteLength"].as_u64()? as usize)?;
    Some(start..end)
}

/// Drops from buffer `b` (its bytes `data`) the bytes of each view in
/// `image_views` that nothing reads any more and that no other view or
/// compressed stream in `b` overlaps, and moves every later view and
/// stream down. Each run of such bytes keeps its last 1 to 16 bytes,
/// zeroed, so the unused views stay valid; what is dropped is a multiple
/// of 16 bytes. Returns the bytes dropped.
fn drop_replaced_images(json: &mut Value, b: usize, data: &mut Vec<u8>, image_views: &[usize]) -> usize {
    let mut read = BTreeSet::new();
    view_references(json, &mut read);
    let in_buffer = |place: &Value| place["buffer"].as_u64() == Some(b as u64);
    let views = json["bufferViews"].as_array().map_or(&[][..], Vec::as_slice);
    let candidate = |i: usize| image_views.contains(&i) && !read.contains(&i);
    // What a reader still needs: every other view, and compressed streams.
    let mut live = Vec::new();
    for (i, view) in views.iter().enumerate() {
        if in_buffer(view) && !candidate(i) {
            live.extend(range(view));
        }
        for name in super::COMPRESSED_GEOMETRY {
            let stream = &view["extensions"][*name];
            if in_buffer(stream) {
                live.extend(range(stream));
            }
        }
    }
    // An image view that something live overlaps stays, and then keeps
    // the image views it overlaps in turn.
    let mut dead: Vec<Range<usize>> = (0..views.len())
        .filter(|&i| candidate(i) && in_buffer(&views[i]))
        .filter_map(|i| range(&views[i]))
        .collect();
    loop {
        let overlaps = |r: &Range<usize>| live.iter().any(|l| l.start < r.end && r.start < l.end);
        let (stay, gone): (Vec<_>, Vec<_>) = dead
            .into_iter()
            .partition(|r| r.is_empty() || r.end > data.len() || overlaps(r));
        dead = gone;
        if stay.is_empty() {
            break;
        }
        live.extend(stay);
    }
    dead.sort_by_key(|r| r.start);
    // Merge overlapping and touching runs.
    let mut runs: Vec<Range<usize>> = Vec::new();
    for r in dead {
        match runs.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => runs.push(r),
        }
    }
    // Each run's dropped part: its first (len - 1) / 16 * 16 bytes.
    let cuts: Vec<(Range<usize>, usize)> = runs
        .into_iter()
        .map(|r| {
            let cut = (r.len() - 1) / ALIGN * ALIGN;
            (r, cut)
        })
        .filter(|(_, cut)| *cut > 0)
        .collect();
    if cuts.is_empty() {
        return 0;
    }
    // Where a byte at `at` (not inside a run) moves to.
    let moved = |at: usize| at - cuts.iter().filter(|(r, _)| r.end <= at).map(|(_, c)| c).sum::<usize>();
    let mut out = Vec::with_capacity(data.len());
    let mut from = 0;
    for (r, cut) in &cuts {
        out.extend_from_slice(&data[from..r.start]);
        out.resize(out.len() + (r.len() - cut), 0);
        from = r.end;
    }
    out.extend_from_slice(&data[from..]);
    let dropped = data.len() - out.len();
    *data = out;

    let views = json["bufferViews"]
        .as_array_mut()
        .map_or(&mut [][..], Vec::as_mut_slice);
    for (i, view) in views.iter_mut().enumerate() {
        if in_buffer(view)
            && let Some(old) = range(view)
        {
            if let Some((r, cut)) = cuts.iter().find(|(r, _)| r.start <= old.start && old.end <= r.end) {
                // A dropped image's view: on the zeroed bytes left of its run.
                debug_assert!(candidate(i), "only unread image views lie inside a run");
                view["byteOffset"] = json!(moved(r.start));
                view["byteLength"] = json!(old.len().min(r.len() - cut));
            } else if moved(old.start) != old.start {
                view["byteOffset"] = json!(moved(old.start));
            }
        }
        for name in super::COMPRESSED_GEOMETRY {
            if let Some(stream) = view.get_mut("extensions").and_then(|e| e.get_mut(*name))
                && in_buffer(stream)
                && let Some(old) = range(stream)
                && moved(old.start) != old.start
            {
                stream["byteOffset"] = json!(moved(old.start));
            }
        }
    }
    dropped
}

/// Every buffer view index a `bufferView` key names, anywhere in the file
/// (accessors, sparse accessors, images, Draco primitives).
fn view_references(value: &Value, out: &mut BTreeSet<usize>) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key == "bufferView"
                    && let Some(i) = v.as_u64()
                {
                    out.insert(i as usize);
                }
                view_references(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| view_references(v, out)),
        _ => {}
    }
}
