//! The images a model holds (in its GLB binary chunk or as `data:` URIs),
//! compressed for the GPU as `ponko textures` compresses image files, and
//! given to their textures through `KHR_texture_basisu`: the engine uploads
//! them as they are, never decoding them to RGBA8.
//!
//! Each image is encoded for what the materials use it as: base color and
//! emissive images as colors (BC7 sRGB), normal maps as normal maps (BC5),
//! and occlusion, roughness and metalness as one linear ORM image (BC7), the
//! way the engine reads them (occlusion in red, roughness in green,
//! metalness in blue). The engine merges a material's separate
//! metallic-roughness and occlusion images into one on the CPU; for an
//! image that stays a block format the cook does that merge instead,
//! writing the merged image and pointing both of the material's textures
//! at it, so the engine has nothing to merge. Mobile platforms get Basis
//! Universal UASTC instead of BC blocks.
//!
//! An image stays as it is when it is a file of its own (shared by models,
//! read by URI), has 16 bits a channel or a side that isn't a multiple of 4,
//! or has two uses (a color and a normal map, a merge two materials want
//! differently), and so does everything else in the file.

use super::{Doc, Images};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// What compressing a model's images did.
#[derive(Default)]
pub(super) struct Report {
    /// Images compressed.
    pub images: usize,
    /// Their video memory as RGBA8 with mipmaps (what the engine gives a
    /// decoded image of a 3D material), and compressed.
    pub before: u64,
    pub after: u64,
}

/// What a material uses an image as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Use {
    Color,
    Normal,
    /// Occlusion (red), roughness (green) and metalness (blue).
    Orm(Merge),
    /// Something the cook doesn't compress (a texture of an extension).
    Other,
}

/// How an ORM image's pixels change so it holds what the engine would
/// merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Merge {
    /// Already an ORM image (the same image for both).
    None,
    /// Metallic-roughness only: no occlusion, red 255.
    NoOcclusion,
    /// Occlusion only: green and blue 255.
    OcclusionOnly,
    /// Red from this occlusion image (255 if its size differs).
    OcclusionFrom(usize),
}

/// A material textureInfo's texture index.
fn index(info: &Value) -> Option<usize> {
    info["index"].as_u64().map(|i| i as usize)
}

/// Texture `t`'s core image, when it has no image of an extension.
fn core_image(json: &Value, t: usize) -> Option<usize> {
    let texture = &json["textures"][t];
    if texture
        .get("extensions")
        .is_some_and(|e| e.get("KHR_texture_basisu").is_some() || e.get("EXT_texture_webp").is_some())
    {
        return None;
    }
    texture["source"].as_u64().map(|i| i as usize)
}

/// Every textureInfo of a material: each object under a key ending in
/// `Texture` with an `index` (core glTF's and its extensions' alike).
fn texture_infos<'a>(value: &'a Value, out: &mut Vec<(&'a str, &'a Value)>) {
    if let Some(object) = value.as_object() {
        for (key, v) in object {
            if key.ends_with("Texture") && v.get("index").is_some() {
                out.push((key.as_str(), v));
            } else {
                texture_infos(v, out);
            }
        }
    }
}

/// The same texture coordinates: the same set and transform.
fn same_mapping(a: &Value, b: &Value) -> bool {
    a["texCoord"].as_u64().unwrap_or(0) == b["texCoord"].as_u64().unwrap_or(0) && a["extensions"] == b["extensions"]
}

/// One material's ORM: its metallic-roughness and occlusion textures.
struct OrmPlan {
    material: usize,
    image: usize,
    merge: Merge,
}

/// Compresses the model's images per [`Images`]; see the module.
pub(super) fn compress(doc: &mut Doc, settings: Images) -> Result<Report, String> {
    let json = &doc.json;
    let images = json["images"].as_array().map_or(0, Vec::len);
    let mut uses: Vec<BTreeSet<Use>> = vec![BTreeSet::new(); images];
    let mut orm_plans = Vec::new();
    for (m, material) in json["materials"].as_array().into_iter().flatten().enumerate() {
        let pbr = &material["pbrMetallicRoughness"];
        let mut infos = Vec::new();
        texture_infos(material, &mut infos);
        for (key, info) in &infos {
            let Some(image) = index(info).and_then(|t| core_image(json, t)) else {
                continue;
            };
            let Some(set) = uses.get_mut(image) else { continue };
            match *key {
                "baseColorTexture" | "emissiveTexture" => {
                    set.insert(Use::Color);
                }
                "normalTexture" => {
                    set.insert(Use::Normal);
                }
                "metallicRoughnessTexture" | "occlusionTexture" => {}
                _ => {
                    set.insert(Use::Other);
                }
            }
        }
        let mr = &pbr["metallicRoughnessTexture"];
        let occlusion = &material["occlusionTexture"];
        let image_of = |info: &Value| index(info).and_then(|t| core_image(json, t));
        let (mr_image, occlusion_image) = (image_of(mr), image_of(occlusion));
        let unread = |info: &Value| info.is_object() && image_of(info).is_none();
        if unread(mr) || unread(occlusion) {
            // A texture with an extension's image: the other stays too.
            for i in [mr_image, occlusion_image].into_iter().flatten() {
                uses[i].insert(Use::Other);
            }
            continue;
        }
        let plan = match (mr_image, occlusion_image) {
            (None, None) => continue,
            (Some(i), Some(o)) if i == o => (i, Merge::None),
            (Some(i), None) => (i, Merge::NoOcclusion),
            (None, Some(o)) => (o, Merge::OcclusionOnly),
            (Some(i), Some(o)) if same_mapping(mr, occlusion) => (i, Merge::OcclusionFrom(o)),
            (Some(i), Some(o)) => {
                uses[i].insert(Use::Other);
                uses[o].insert(Use::Other);
                continue;
            }
        };
        uses[plan.0].insert(Use::Orm(plan.1));
        orm_plans.push(OrmPlan {
            material: m,
            image: plan.0,
            merge: plan.1,
        });
    }

    // What each image becomes: one use, of an image inside the file.
    let mut encoded: BTreeMap<usize, (Vec<u8>, u64, u64)> = BTreeMap::new();
    for (i, set) in uses.iter().enumerate() {
        let [only] = set.iter().copied().collect::<Vec<_>>()[..] else {
            continue;
        };
        if only == Use::Other {
            continue;
        }
        let Some(bytes) = embedded_bytes(doc, i)? else {
            continue;
        };
        let Some((mut rgba, width, height)) = decode(&bytes) else {
            continue;
        };
        if let Use::Orm(merge) = only {
            match merge {
                Merge::None => {}
                Merge::NoOcclusion => rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[0] = 255),
                Merge::OcclusionOnly => rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| {
                    p[1] = 255;
                    p[2] = 255;
                }),
                Merge::OcclusionFrom(o) => {
                    // Merged as the engine merges: any format, decoded to
                    // RGBA8. One the file doesn't hold, or can't be read:
                    // the ORM image stays two images.
                    let Some(occlusion) = embedded_bytes(doc, o)?
                        .and_then(|b| image::load_from_memory(&b).ok())
                        .map(image::DynamicImage::into_rgba8)
                    else {
                        continue;
                    };
                    let pixels = rgba.as_chunks_mut::<4>().0;
                    match Some(occlusion).filter(|ao| ao.dimensions() == (width, height)) {
                        Some(ao) => {
                            for (p, a) in pixels.iter_mut().zip(ao.as_raw().as_chunks::<4>().0) {
                                p[0] = a[0];
                            }
                        }
                        // The engine's merge: no occlusion where it doesn't line up.
                        None => pixels.iter_mut().for_each(|p| p[0] = 255),
                    }
                }
            }
        }
        let kind = match only {
            Use::Color => crate::textures::Kind::Color,
            Use::Normal => crate::textures::Kind::Normal,
            _ => crate::textures::Kind::Linear,
        };
        let rule = crate::textures::Rule {
            kind,
            basis: settings.basis,
            mipmaps: true,
        };
        let out = crate::textures::encode(&rgba, width, height, rule)?;
        let before = crate::cook::rgba8_with_mips(width, height, true);
        encoded.insert(i, (out.file, before, out.gpu_bytes));
    }

    // Each material's ORM image, when it was compressed: both textures on
    // the metallic-roughness one's (or the occlusion's, without one).
    let mut orphans = BTreeSet::new();
    for plan in &orm_plans {
        if !encoded.contains_key(&plan.image) {
            continue;
        }
        let material = &mut doc.json["materials"][plan.material];
        match plan.merge {
            Merge::None => {}
            Merge::NoOcclusion => {
                let mr = material["pbrMetallicRoughness"]["metallicRoughnessTexture"].clone();
                material["occlusionTexture"] = mapping_of(&mr);
            }
            Merge::OcclusionOnly => {
                let occlusion = material["occlusionTexture"].clone();
                material["pbrMetallicRoughness"]["metallicRoughnessTexture"] = mapping_of(&occlusion);
            }
            Merge::OcclusionFrom(_) => {
                let mr = index(&material["pbrMetallicRoughness"]["metallicRoughnessTexture"]);
                if let Some(old) = index(&material["occlusionTexture"]) {
                    orphans.insert(old);
                }
                material["occlusionTexture"]["index"] = json!(mr);
            }
        }
    }

    let mut report = Report::default();
    for (i, (file, before, after)) in encoded {
        let view = doc.add_view(file, None, 0);
        let views = doc.json["bufferViews"].as_array_mut().ok_or("no buffer views")?;
        if let Some(v) = views.last_mut().and_then(Value::as_object_mut) {
            v.remove("target");
        }
        let image = doc.json["images"][i]
            .as_object_mut()
            .ok_or("an image isn't an object")?;
        image.remove("uri");
        image.insert("bufferView".into(), json!(view));
        image.insert("mimeType".into(), json!("image/ktx2"));
        for texture in doc.json["textures"].as_array_mut().into_iter().flatten() {
            if texture["source"].as_u64() == Some(i as u64)
                && let Some(t) = texture.as_object_mut()
            {
                t.remove("source");
                let extensions = t.entry("extensions").or_insert_with(|| json!({}));
                extensions["KHR_texture_basisu"] = json!({ "source": i });
            }
        }
        report.images += 1;
        report.before += before;
        report.after += after;
    }
    if report.images > 0 {
        for key in ["extensionsUsed", "extensionsRequired"] {
            let mut list = doc.json[key].as_array().cloned().unwrap_or_default();
            if !list.iter().any(|e| e == "KHR_texture_basisu") {
                list.push(json!("KHR_texture_basisu"));
                doc.json[key] = Value::Array(list);
            }
        }
    }
    prune(&mut doc.json, &orphans);
    Ok(report)
}

/// A textureInfo for the same texture and coordinates as `info`.
fn mapping_of(info: &Value) -> Value {
    let mut out = json!({ "index": info["index"] });
    for key in ["texCoord", "extensions"] {
        if let Some(v) = info.get(key) {
            out[key] = v.clone();
        }
    }
    out
}

/// The bytes of image `i` when the file holds them (a buffer view or a
/// `data:` URI); `None` for an image file of its own.
fn embedded_bytes(doc: &Doc, i: usize) -> Result<Option<Vec<u8>>, String> {
    let image = &doc.json["images"][i];
    if let Some(view) = image["bufferView"].as_u64() {
        return Ok(Some(doc.view(view as usize)?.to_vec()));
    }
    match image["uri"].as_str() {
        Some(uri) if uri.starts_with("data:") => {
            let (_, data) = uri.split_once(";base64,").ok_or("an image's data URI isn't base64")?;
            Ok(Some(super::base64(data)?))
        }
        _ => Ok(None),
    }
}

/// An 8-bit image as RGBA8 when its sides are multiples of 4; `None` for
/// what stays as it is.
fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let image = image::load_from_memory(bytes).ok()?;
    use image::ColorType as C;
    if !matches!(image.color(), C::L8 | C::La8 | C::Rgb8 | C::Rgba8) {
        return None;
    }
    let image = image.into_rgba8();
    let (w, h) = image.dimensions();
    (w.is_multiple_of(4) && h.is_multiple_of(4)).then(|| (image.into_raw(), w, h))
}

/// Removes the textures in `candidates` no material uses any more, and
/// then the images no texture uses that only they used, renumbering what
/// points at them.
fn prune(json: &mut Value, candidates: &BTreeSet<usize>) {
    let mut used = BTreeSet::new();
    for material in json["materials"].as_array().into_iter().flatten() {
        let mut infos = Vec::new();
        texture_infos(material, &mut infos);
        used.extend(infos.iter().filter_map(|(_, info)| index(info)));
    }
    let dropped: BTreeSet<usize> = candidates.iter().copied().filter(|t| !used.contains(t)).collect();
    if dropped.is_empty() {
        return;
    }
    let sources = |texture: &Value| -> Vec<usize> {
        let mut out: Vec<usize> = texture["source"].as_u64().map(|i| i as usize).into_iter().collect();
        for ext in ["KHR_texture_basisu", "EXT_texture_webp"] {
            out.extend(texture["extensions"][ext]["source"].as_u64().map(|i| i as usize));
        }
        out
    };
    let textures = json["textures"].as_array().cloned().unwrap_or_default();
    let mut image_candidates = BTreeSet::new();
    let mut kept_images = BTreeSet::new();
    for (t, texture) in textures.iter().enumerate() {
        if dropped.contains(&t) {
            image_candidates.extend(sources(texture));
        } else {
            kept_images.extend(sources(texture));
        }
    }
    let new_texture = renumbering(textures.len(), &dropped);
    json["textures"] = Value::Array(
        textures
            .into_iter()
            .enumerate()
            .filter(|(t, _)| !dropped.contains(t))
            .map(|(_, t)| t)
            .collect(),
    );
    for material in json["materials"].as_array_mut().into_iter().flatten() {
        renumber_infos(material, &new_texture);
    }
    let dropped_images: BTreeSet<usize> = image_candidates.difference(&kept_images).copied().collect();
    if dropped_images.is_empty() {
        return;
    }
    let images = json["images"].as_array().cloned().unwrap_or_default();
    let new_image = renumbering(images.len(), &dropped_images);
    json["images"] = Value::Array(
        images
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !dropped_images.contains(i))
            .map(|(_, i)| i)
            .collect(),
    );
    for texture in json["textures"].as_array_mut().into_iter().flatten() {
        let fix = |v: &mut Value| {
            if let Some(i) = v.as_u64() {
                *v = json!(new_image[i as usize]);
            }
        };
        if texture.get("source").is_some() {
            fix(&mut texture["source"]);
        }
        for ext in ["KHR_texture_basisu", "EXT_texture_webp"] {
            if let Some(e) = texture.get_mut("extensions").and_then(|e| e.get_mut(ext)) {
                fix(&mut e["source"]);
            }
        }
    }
}

/// The new index of each of `len` items once `dropped` are removed (a
/// dropped one maps to its successor's, never read).
fn renumbering(len: usize, dropped: &BTreeSet<usize>) -> Vec<usize> {
    let mut next = 0;
    (0..len)
        .map(|i| {
            let at = next;
            if !dropped.contains(&i) {
                next += 1;
            }
            at
        })
        .collect()
}

fn renumber_infos(value: &mut Value, new_index: &[usize]) {
    if let Some(object) = value.as_object_mut() {
        for (key, v) in object.iter_mut() {
            if key.ends_with("Texture")
                && let Some(i) = v.get("index").and_then(Value::as_u64)
            {
                v["index"] = json!(new_index[i as usize]);
            } else {
                renumber_infos(v, new_index);
            }
        }
    }
}
