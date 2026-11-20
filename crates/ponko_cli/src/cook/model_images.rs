//! The image files glTF models read by URI (beside the model, or anywhere
//! among the assets), and what the cook makes of each: compressed for
//! what the models' materials use it as, so the model loads it in its
//! block format whether the model itself is cooked or not.
//!
//! - Base color and emissive images: color (BC7 sRGB).
//! - Normal maps: normal maps (BC5).
//! - Metallic-roughness and occlusion images: the engine merges a
//!   material's two into one ORM image (occlusion in red, roughness in
//!   green, metalness in blue) on the CPU, as RGBA8. The cook does that
//!   merge into the metallic-roughness file (or the occlusion file, for a
//!   material without one) and compresses it (BC7, linear), with a KTX2
//!   key (`ponkoOrm`) naming the two files: the engine then uploads that
//!   file as it is and doesn't read the occlusion file. A material whose
//!   two are one image gets it compressed as it is.
//!
//! An image stays as it is when its uses differ (a color image also used
//! as a normal map, a metallic-roughness image merged with two occlusion
//! images), when it is only an occlusion image merged into another file,
//! or when a material pairs it with an image inside the model or reads it
//! through an extension (KTX2, WebP).

#![deny(missing_docs)]

use super::{TextureKind, gltf_json, normalize, percent_decode};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// One use of an image file by a model's material.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Use {
    Color,
    Normal,
    /// The ORM image of a material's metallic-roughness and occlusion
    /// files (either missing), held by this file.
    Orm {
        metal_rough: Option<String>,
        occlusion: Option<String>,
    },
    /// Only merged into another file's ORM image.
    MergedInto(String),
    /// Paired with an image the model holds, or read through an extension.
    Other,
}

/// What the cook does with an image file a model reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Compressed as this kind.
    Encode(TextureKind),
    /// The ORM image of these two files (either `None`), written into
    /// this one; the names are asset names.
    Orm {
        /// The metallic-roughness image.
        metal_rough: Option<String>,
        /// The occlusion image.
        occlusion: Option<String>,
    },
    /// Kept as it is, for this reason.
    Keep(String),
}

/// Where a material's texture gets its image.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    /// An image file, by its asset name.
    File(String),
    /// An image the model holds, or one of an extension.
    Inside,
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

/// The plan for every image file the glTF models among `files` read by URI.
pub fn plans(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Plan> {
    let mut uses: BTreeMap<String, BTreeSet<Use>> = BTreeMap::new();
    for (name, bytes) in files {
        if super::Kind::of(name) != super::Kind::Mesh {
            continue;
        }
        let Some(json) = gltf_json(bytes) else { continue };
        let folder = name.rfind('/').map_or("", |i| &name[..=i]);
        let images: Vec<Option<String>> = json["images"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|image| {
                image["uri"]
                    .as_str()
                    .filter(|u| !u.starts_with("data:"))
                    .map(|uri| normalize(&format!("{folder}{}", percent_decode(uri))))
            })
            .collect();
        // Every image file the model reads is a model's image, used or not.
        for file in images.iter().flatten() {
            uses.entry(file.clone()).or_default();
        }
        let source = |info: &Value| -> Option<Source> {
            let texture = &json["textures"][info["index"].as_u64()? as usize];
            let extension = texture
                .get("extensions")
                .is_some_and(|e| e.get("KHR_texture_basisu").is_some() || e.get("EXT_texture_webp").is_some());
            let image = texture["source"].as_u64()? as usize;
            // With an extension's image the engine reads that one first.
            match images.get(image) {
                Some(Some(file)) if !extension => Some(Source::File(file.clone())),
                _ => Some(Source::Inside),
            }
        };
        let mut add = |file: &str, u: Use| {
            uses.entry(file.to_string()).or_default().insert(u);
        };
        for material in json["materials"].as_array().into_iter().flatten() {
            let mut infos = Vec::new();
            texture_infos(material, &mut infos);
            for (key, info) in &infos {
                let Some(Source::File(file)) = source(info) else {
                    // An extension's image: its core image is read differently.
                    if let Some(i) = info["index"].as_u64()
                        && let Some(image) = json["textures"][i as usize]["source"].as_u64()
                        && let Some(Some(file)) = images.get(image as usize)
                    {
                        add(file, Use::Other);
                    }
                    continue;
                };
                match *key {
                    "baseColorTexture" | "emissiveTexture" => add(&file, Use::Color),
                    "normalTexture" => add(&file, Use::Normal),
                    "metallicRoughnessTexture" | "occlusionTexture" => {}
                    _ => add(&file, Use::Other),
                }
            }
            let mr = source(&material["pbrMetallicRoughness"]["metallicRoughnessTexture"]);
            let occlusion = source(&material["occlusionTexture"]);
            match (mr, occlusion) {
                (None, None) => {}
                (Some(Source::File(m)), Some(Source::File(o))) if m == o => add(
                    &m,
                    Use::Orm {
                        metal_rough: Some(m.clone()),
                        occlusion: Some(o.clone()),
                    },
                ),
                (Some(Source::File(m)), Some(Source::File(o))) => {
                    add(
                        &m,
                        Use::Orm {
                            metal_rough: Some(m.clone()),
                            occlusion: Some(o.clone()),
                        },
                    );
                    add(&o, Use::MergedInto(m));
                }
                (Some(Source::File(m)), None) => add(
                    &m,
                    Use::Orm {
                        metal_rough: Some(m.clone()),
                        occlusion: None,
                    },
                ),
                (None, Some(Source::File(o))) => add(
                    &o,
                    Use::Orm {
                        metal_rough: None,
                        occlusion: Some(o.clone()),
                    },
                ),
                (a, b) => {
                    for s in [a, b].into_iter().flatten() {
                        if let Source::File(f) = s {
                            add(&f, Use::Other);
                        }
                    }
                }
            }
        }
    }
    uses.into_iter().map(|(file, set)| (file, plan(&set))).collect()
}

/// What one file's uses make of it.
fn plan(uses: &BTreeSet<Use>) -> Plan {
    let list: Vec<&Use> = uses.iter().collect();
    match list[..] {
        [] => Plan::Keep("a glTF model's image that no material uses".into()),
        [Use::Color] => Plan::Encode(TextureKind::Color),
        [Use::Normal] => Plan::Encode(TextureKind::Normal),
        [Use::Orm { metal_rough, occlusion }] => Plan::Orm {
            metal_rough: metal_rough.clone(),
            occlusion: occlusion.clone(),
        },
        _ if list.iter().all(|u| matches!(u, Use::MergedInto(_))) => {
            let into: Vec<&str> = list
                .iter()
                .filter_map(|u| match u {
                    Use::MergedInto(f) => Some(f.as_str()),
                    _ => None,
                })
                .collect();
            Plan::Keep(format!(
                "a glTF model's occlusion image, merged into {} (the engine reads that)",
                into.join(" and ")
            ))
        }
        _ if list.contains(&&Use::Other) => Plan::Keep(
            "a glTF model's image paired with an image inside the model, or read through an extension".into(),
        ),
        _ => Plan::Keep("a glTF model's image with two uses (each needs its own compression)".into()),
    }
}

/// `target` relative to folder `from` (both `/`-separated asset names;
/// `from` ends with `/` or is empty).
#[cfg_attr(not(feature = "textures"), allow(dead_code))]
pub fn relative(from: &str, target: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    let to: Vec<&str> = target.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model(materials: Value, images: &[&str]) -> Vec<u8> {
        let images: Vec<Value> = images.iter().map(|u| json!({ "uri": u })).collect();
        let textures: Vec<Value> = (0..images.len()).map(|i| json!({ "source": i })).collect();
        serde_json::to_vec(&json!({
            "asset": { "version": "2.0" },
            "materials": materials,
            "textures": textures,
            "images": images,
        }))
        .unwrap()
    }

    #[test]
    fn each_image_file_is_planned_for_what_materials_use_it_as() {
        let files: BTreeMap<String, Vec<u8>> = [
            (
                "models/rock.gltf".to_string(),
                model(
                    json!([{
                        "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 },
                                                  "metallicRoughnessTexture": { "index": 2 } },
                        "normalTexture": { "index": 1 },
                        "occlusionTexture": { "index": 3 }
                    }, {
                        "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 4 } }
                    }, {
                        "occlusionTexture": { "index": 5 }
                    }]),
                    &[
                        "rock.png",
                        "rock_n.png",
                        "rock_mr.png",
                        "../shared/rock%20ao.png",
                        "metal.png",
                        "ao_only.png",
                    ],
                ),
            ),
            ("models/rock.png".to_string(), Vec::new()),
        ]
        .into_iter()
        .collect();
        let plans = plans(&files);
        assert_eq!(plans["models/rock.png"], Plan::Encode(TextureKind::Color));
        assert_eq!(plans["models/rock_n.png"], Plan::Encode(TextureKind::Normal));
        assert_eq!(
            plans["models/rock_mr.png"],
            Plan::Orm {
                metal_rough: Some("models/rock_mr.png".into()),
                occlusion: Some("shared/rock ao.png".into())
            }
        );
        assert!(
            matches!(&plans["shared/rock ao.png"], Plan::Keep(why) if why.contains("merged into models/rock_mr.png"))
        );
        assert_eq!(
            plans["models/metal.png"],
            Plan::Orm {
                metal_rough: Some("models/metal.png".into()),
                occlusion: None
            }
        );
        assert_eq!(
            plans["models/ao_only.png"],
            Plan::Orm {
                metal_rough: None,
                occlusion: Some("models/ao_only.png".into())
            }
        );
    }

    #[test]
    fn an_image_with_two_uses_or_beside_an_inside_image_is_kept() {
        let files: BTreeMap<String, Vec<u8>> = [
            (
                "a.gltf".to_string(),
                model(
                    json!([{
                        "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                        "normalTexture": { "index": 0 }
                    }, {
                        "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 1 } },
                        "occlusionTexture": { "index": 2 }
                    }, {
                        "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 1 } },
                        "occlusionTexture": { "index": 3 }
                    }]),
                    &["both.png", "mr.png", "ao1.png", "ao2.png"],
                ),
            ),
            (
                "b.glb.gltf".to_string(),
                model(
                    json!([{
                        "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 0 } },
                        "occlusionTexture": { "index": 1 }
                    }]),
                    &["paired.png", "data:image/png;base64,AAAA"],
                ),
            ),
        ]
        .into_iter()
        .collect();
        let plans = plans(&files);
        for (file, why) in [
            ("both.png", "two uses"),
            ("mr.png", "two uses"),
            ("paired.png", "inside the model"),
        ] {
            assert!(
                matches!(&plans[file], Plan::Keep(w) if w.contains(why)),
                "{file}: {:?}",
                plans[file]
            );
        }
    }

    #[test]
    fn relative_names_climb_out_of_folders() {
        assert_eq!(relative("models/", "models/a.png"), "a.png");
        assert_eq!(relative("models/rock/", "shared/ao.png"), "../../shared/ao.png");
        assert_eq!(relative("", "a/b.png"), "a/b.png");
    }
}
