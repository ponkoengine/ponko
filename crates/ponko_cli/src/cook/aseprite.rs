//! Converts Aseprite sources to the JSON Array export and PNG used by the engine.

#![deny(missing_docs)]

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
    let sheet = ponko_aseprite::decode(bytes, split, 16384).map_err(|e| format!("{name}: {e}"))?;
    let mut export = sheet.export;
    export["meta"]["app"] = serde_json::json!("ponko cook");
    export["meta"]["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    let png_name = outputs(name).1;
    export["meta"]["image"] = serde_json::json!(png_name.rsplit('/').next().unwrap_or(&png_name));
    let mut text = serde_json::to_vec_pretty(&export).map_err(|e| format!("{name}: {e}"))?;
    text.push(b'\n');

    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        ),
        &sheet.rgba,
        sheet.width,
        sheet.height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("{name}: its sheet can't be written as PNG: {e}"))?;
    Ok(Sheet {
        json: text,
        png,
        note: sheet.note,
    })
}

// Store the two outputs in one cache entry. A reader gets the complete
// pair or regenerates both.
pub(super) fn cache_pair(sheet: &Sheet) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + sheet.json.len() + sheet.png.len());
    out.extend_from_slice(&(sheet.json.len() as u64).to_le_bytes());
    out.extend_from_slice(&sheet.json);
    out.extend_from_slice(&sheet.png);
    out
}

pub(super) fn cached_pair(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let len = usize::try_from(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?)).ok()?;
    let (json, png) = bytes.get(8..)?.split_at_checked(len)?;
    (json.first() == Some(&b'{') && png.starts_with(b"\x89PNG\r\n\x1a\n")).then_some((json, png))
}

#[cfg(test)]
pub(crate) mod tests;
