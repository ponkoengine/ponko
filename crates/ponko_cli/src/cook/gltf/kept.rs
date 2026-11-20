//! Serialize a model whose geometry must stay in its original layout.
//! Compressed views can address buffers by index and raw byte offset, so
//! original buffer and view indices, offsets and payloads stay intact.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;

use super::{Doc, encode_glb};

pub(super) fn write(mut doc: Doc, original_buffers: usize, binary: bool, has_bin: bool) -> Result<Vec<u8>, String> {
    let appended = doc.buffers.split_off(original_buffers);
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
    if binary {
        encode_glb(&doc.json, &bin)
    } else {
        serde_json::to_vec(&doc.json).map_err(|e| e.to_string())
    }
}
