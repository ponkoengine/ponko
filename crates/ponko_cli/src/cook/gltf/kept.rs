//! Serialize a model whose geometry must stay in its original layout.
//! Compressed views can address buffers by index and raw byte offset, so
//! original buffer and view indices, offsets and payloads stay intact.
//! The bytes of an image the cook replaced with KTX2 stay where they were,
//! zeroed when they are in the GLB's own chunk, so a pack deflates them.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{Doc, encode_glb};

/// The buffer views the file's images are stored in, before the cook
/// points them at KTX2 views.
pub(super) fn image_views(json: &Value) -> Vec<usize> {
    let images = json["images"].as_array().into_iter().flatten();
    images
        .filter_map(|image| image["bufferView"].as_u64().map(|v| v as usize))
        .collect()
}

pub(super) fn write(
    mut doc: Doc,
    original_buffers: usize,
    binary: bool,
    has_bin: bool,
    image_views: &[usize],
) -> Result<Vec<u8>, String> {
    let appended = doc.buffers.split_off(original_buffers);
    let mut data = if has_bin {
        let mut bin = std::mem::take(doc.buffers.first_mut().ok_or("binary model has no buffer")?);
        zero_replaced_images(&doc.json, &mut bin, image_views);
        bin
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
    if binary {
        encode_glb(&doc.json, &bin)
    } else {
        serde_json::to_vec(&doc.json).map_err(|e| e.to_string())
    }
}

/// Zeroes, in buffer 0, the bytes of each view in `image_views` that
/// nothing reads any more and that no view or meshopt stream still read
/// overlaps.
fn zero_replaced_images(json: &Value, bin: &mut [u8], image_views: &[usize]) {
    let mut read = std::collections::BTreeSet::new();
    view_references(json, &mut read);
    let views = json["bufferViews"].as_array().map_or(&[][..], Vec::as_slice);
    let range = |v: &Value| -> Option<std::ops::Range<usize>> {
        let start = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let end = start.checked_add(v["byteLength"].as_u64()? as usize)?;
        Some(start..end)
    };
    // What buffer 0 still holds for a reader: live views and meshopt streams.
    let mut live = Vec::new();
    for (i, view) in views.iter().enumerate() {
        if read.contains(&i) && view["buffer"].as_u64() == Some(0) {
            live.extend(range(view));
        }
        for name in super::COMPRESSED_GEOMETRY {
            let stream = &view["extensions"][*name];
            if stream["buffer"].as_u64() == Some(0) {
                live.extend(range(stream));
            }
        }
    }
    for &i in image_views {
        let Some(view) = views.get(i).filter(|v| v["buffer"].as_u64() == Some(0)) else {
            continue;
        };
        let Some(bytes) = range(view).filter(|r| r.end <= bin.len()) else {
            continue;
        };
        if read.contains(&i) || live.iter().any(|l| l.start < bytes.end && bytes.start < l.end) {
            continue;
        }
        bin[bytes].fill(0);
    }
}

/// Every buffer view index a `bufferView` key names, anywhere in the file
/// (accessors, sparse accessors, images, Draco primitives).
fn view_references(value: &Value, out: &mut std::collections::BTreeSet<usize>) {
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
