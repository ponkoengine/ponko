//! glTF extensions: which ones the loader reads, and the parts of loading
//! that read them.
//!
//! [`prepare`] turns a file's bytes into a validated document and its
//! buffers: it fails on a required extension this build can't read, warns
//! about optional ones it ignores, decompresses meshopt buffer views
//! (`EXT_meshopt_compression`, feature `gltf-meshopt`) into a buffer of
//! their own and points textures at their KTX2 or WebP images when the
//! build reads those, and folds the nodes `ponko cook` adds for quantized
//! positions back into their parents ([`fold_quantized_positions`]). The
//! rest reads extension data while
//! [`load`](super::gltf::load) walks the file: quantized attributes
//! (`KHR_mesh_quantization`), texture transforms, emissive strength,
//! transmission, lights and cameras.

use glam::{Mat4, Vec2, Vec3};
use serde_json::Value;

use super::{MAP_KINDS, MaterialLayers, PointLight3D, SpotLight3D, Transform3D};
use crate::color::Color;

/// Whether this build decompresses meshopt views (the C++ decoder isn't
/// built for the web).
const MESHOPT: bool = cfg!(all(feature = "gltf-meshopt", not(target_arch = "wasm32")));

/// What the loader does with a glTF extension (the guide's table lists the
/// same, keep them in step).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Support {
    /// Read in every build.
    Always,
    /// Read when the named cargo feature is on.
    Feature(&'static str, bool),
    /// Not read, but what it changes is drawn close enough without it: a
    /// file that requires it loads, with a warning saying what is lost.
    Approximated,
}

/// The extensions the loader reads, and when.
fn support(extension: &str) -> Option<Support> {
    Some(match extension {
        "KHR_lights_punctual"
        | "KHR_texture_transform"
        | "KHR_materials_emissive_strength"
        | "KHR_mesh_quantization"
        | "KHR_materials_transmission"
        | "KHR_materials_unlit"
        | "KHR_materials_clearcoat"
        | "KHR_materials_sheen"
        | "KHR_materials_specular"
        | "KHR_materials_ior"
        | "KHR_materials_anisotropy"
        | "KHR_materials_iridescence"
        | "KHR_node_visibility"
        | "KHR_animation_pointer"
        | SPECULAR_GLOSSINESS
        | INSTANCING
        | QUANTIZED_POSITIONS => Support::Always,
        "EXT_meshopt_compression" | "KHR_meshopt_compression" => Support::Feature("gltf-meshopt", MESHOPT),
        // KTX2 images: block formats upload as they are; Basis Universal
        // UASTC ones also need `basis-universal` (they fail to read without
        // it, and the texture's core image loads instead if it has one).
        "KHR_texture_basisu" => Support::Feature("compressed-textures", cfg!(feature = "compressed-textures")),
        "EXT_texture_webp" => Support::Feature("image-webp", cfg!(feature = "image-webp")),
        // DDS images of BCn blocks, as the KTX2 ones upload.
        "MSFT_texture_dds" => Support::Feature("compressed-textures", cfg!(feature = "compressed-textures")),
        // Thickness and absorption of what transmission lets through.
        "KHR_materials_volume" => Support::Approximated,
        _ => return None,
    })
}

/// Extensions the renderer has no counterpart for, and what is lost
/// without them (for the warning).
fn ignored_because(extension: &str) -> &'static str {
    match extension {
        "KHR_materials_volume" => "transmission is drawn as see-through blending, without refraction or absorption",
        "KHR_draco_mesh_compression" => "Draco compressed meshes aren't read",
        _ => "the file loads as if it weren't there",
    }
}

/// Says `message` where a developer sees it: stderr, or the browser's
/// console on the web (stderr goes nowhere there).
pub(super) fn warn(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!("ponko: {message}")));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("ponko: {message}");
}

/// A file ready to read: its document, its buffers (meshopt views
/// decompressed into one more buffer at the end) and its JSON, for the
/// extension data the `gltf` crate doesn't model.
pub(super) struct Prepared {
    pub document: gltf::Document,
    pub buffers: Vec<Vec<u8>>,
    pub json: Value,
    /// For textures with both a core image and one the build reads through
    /// an extension (KTX2, WebP): the extension's image, tried first.
    pub preferred_image: Vec<Option<usize>>,
    /// Each mesh's dequantization, by primitive, for a mesh whose
    /// quantization nodes were folded into its parent: a primitive's
    /// positions go through its matrix first. Empty for other meshes.
    pub dequantize: Vec<Vec<Mat4>>,
    /// The file's `KHR_animation_pointer` channels, taken out of `json`
    /// (the `gltf` crate refuses them).
    pub pointers: Vec<PointerChannel>,
    pub warnings: Vec<String>,
}

/// The extension objects of `value` (`value.extensions.<name>`).
pub(super) fn extension<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
    value.get("extensions")?.get(name)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Reads a glTF file's JSON, checks its extensions and fetches its buffers
/// (`fetch` reads a URI: inline data or a file next to it).
pub(super) fn prepare(
    name: &str,
    bytes: &[u8],
    fetch: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Prepared, String> {
    let mut start = Start::new(name, bytes)?;
    while !start.next_buffer(name, fetch, usize::MAX)? {}
    start.finish(name)
}

/// [`prepare`] a piece at a time (a streamed mesh where there are no worker
/// threads): [`Start::new`] parses the JSON, [`Start::next_buffer`] fetches
/// the buffers (an inline one a bounded number of characters a call), and
/// [`Start::finish`] makes the document. The same [`Prepared`] either way.
pub(super) struct Start {
    json: Value,
    blob: Option<Vec<u8>>,
    warnings: Vec<String>,
    buffers: Vec<Vec<u8>>,
    /// The inline buffer being decoded: where its text is up to.
    inline: Option<(usize, super::gltf::Base64)>,
}

impl Start {
    /// Splits a GLB, parses the JSON and checks its extensions.
    pub(super) fn new(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let (json, blob) = if bytes.starts_with(b"glTF") {
            let glb = gltf::Glb::from_slice(bytes).map_err(|e| format!("{name}: not a glTF file: {e}"))?;
            (glb.json.into_owned(), glb.bin.map(std::borrow::Cow::into_owned))
        } else {
            (bytes.to_vec(), None)
        };
        let json: Value = serde_json::from_slice(&json).map_err(|e| format!("{name}: not a glTF file: {e}"))?;
        if !json.is_object() {
            return Err(format!("{name}: not a glTF file: its JSON isn't an object"));
        }
        let mut warnings = Vec::new();

        // Required extensions this build can't read fail; optional ones are
        // dropped with a warning naming them.
        let required = strings(&json["extensionsRequired"]);
        for extension in &required {
            match support(extension) {
                Some(Support::Always | Support::Feature(_, true)) => {}
                Some(Support::Approximated) => warnings.push(format!(
                    "{name}: approximates the glTF extension {extension} it needs: {}",
                    ignored_because(extension)
                )),
                Some(Support::Feature(feature, false)) => {
                    return Err(format!(
                        "{name}: needs the glTF extension {extension}, which this build of ponko doesn't read: \
                         turn on its `{feature}` cargo feature"
                    ));
                }
                None => {
                    return Err(format!(
                        "{name}: needs the glTF extension {extension}, which ponko doesn't read ({})",
                        ignored_because(extension)
                    ));
                }
            }
        }
        for extension in strings(&json["extensionsUsed"]) {
            if required.contains(&extension) {
                continue;
            }
            match support(&extension) {
                Some(Support::Always | Support::Feature(_, true)) => {}
                Some(Support::Feature(feature, false)) => warnings.push(format!(
                    "{name}: uses the glTF extension {extension} without the `{feature}` cargo feature; \
                     its uncompressed fallback loads instead"
                )),
                Some(Support::Approximated) | None => warnings.push(format!(
                    "{name}: ignores the optional glTF extension {extension}: {}",
                    ignored_because(&extension)
                )),
            }
        }
        Ok(Self {
            json,
            blob,
            warnings,
            buffers: Vec::new(),
            inline: None,
        })
    }

    /// Fetches the next buffer, or decodes the next `chars` characters at
    /// most of an inline one: `true` once every buffer is there.
    pub(super) fn next_buffer(
        &mut self,
        name: &str,
        fetch: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
        chars: usize,
    ) -> Result<bool, String> {
        let i = self.buffers.len();
        let Some(buffer) = self.json["buffers"].as_array().and_then(|b| b.get(i)) else {
            return Ok(true);
        };
        let length = buffer["byteLength"].as_u64().unwrap_or(0) as usize;
        let fallback = meshopt(buffer).is_some_and(|m| m["fallback"].as_bool() == Some(true));
        let data = match buffer["uri"].as_str() {
            // A meshopt file's fallback buffer holds nothing to read when
            // the views are decompressed (often it has no data at all).
            _ if fallback && MESHOPT => Vec::new(),
            Some(uri) => match super::gltf::inline_data(uri) {
                Some(text) => {
                    let text = text?;
                    let (at, mut decoder) = self
                        .inline
                        .take()
                        .unwrap_or_else(|| (0, super::gltf::Base64::with_capacity(text.len())));
                    let end = super::gltf::char_boundary(text, at.saturating_add(chars));
                    decoder.feed(&text[at..end])?;
                    if end < text.len() {
                        self.inline = Some((end, decoder));
                        return Ok(false);
                    }
                    decoder.finish()
                }
                None => super::gltf::fetch(uri, super::gltf::folder_of(name), fetch)?,
            },
            None if fallback => Vec::new(),
            None => self
                .blob
                .take()
                .ok_or_else(|| format!("{name}: missing its binary chunk"))?,
        };
        if data.len() < length && !(fallback && data.is_empty()) {
            return Err(format!("{name}: buffer {i} is shorter than it says"));
        }
        self.buffers.push(data);
        Ok(self.buffers.len() == self.json["buffers"].as_array().map_or(0, Vec::len))
    }

    /// Decompresses meshopt views and makes the document.
    pub(super) fn finish(self, name: &str) -> Result<Prepared, String> {
        let Self {
            mut json,
            mut buffers,
            mut warnings,
            ..
        } = self;
        decompress_views(name, &mut json, &mut buffers)?;
        if specular_glossiness_as_metal_rough(&mut json) {
            warnings.push(format!(
                "{name}: approximates its {SPECULAR_GLOSSINESS} materials: the diffuse color and texture \
                 show as the base color and the glossiness as roughness; the specular color and the \
                 specular-glossiness texture are left out"
            ));
        }
        let preferred_image = texture_sources(name, &mut json)?;
        let dequantize = fold_quantized_positions(&mut json);
        let unread = instances_as_nodes(&mut json, &buffers);
        if unread > 0 {
            warnings.push(format!(
                "{name}: draws the mesh of {unread} {INSTANCING} node(s) once, at the node: their instance \
                 transforms aren't readable (a sparse accessor, or a type the extension doesn't allow)"
            ));
        }
        let pointers = pointer_channels(&json);
        drop_channels_without_nodes(&mut json);

        // The `gltf` crate checks required extensions against its own list,
        // which doesn't know these; they were checked above.
        json["extensionsRequired"] = Value::Array(Vec::new());
        let root: gltf::json::Root =
            serde_json::from_value(json.clone()).map_err(|e| format!("{name}: not a glTF file: {e}"))?;
        super::gltf::check_position_indices(&root).map_err(|e| format!("{name}: not a valid glTF file: {e}"))?;
        let document = gltf::Document::from_json(root).map_err(|e| format!("{name}: not a valid glTF file: {e}"))?;
        Ok(Prepared {
            document,
            buffers,
            json,
            preferred_image,
            dequantize,
            pointers,
            warnings,
        })
    }
}

/// The retired specular-glossiness workflow of glTF materials, which
/// older exporters still write (often with no metallic-roughness block).
const SPECULAR_GLOSSINESS: &str = "KHR_materials_pbrSpecularGlossiness";

/// Gives every material of the retired specular-glossiness workflow
/// ([`SPECULAR_GLOSSINESS`]) without a metallic-roughness block of its own
/// one made from it, which the rest of the loader reads: the diffuse color
/// and texture as the base color, no metalness, and one minus the
/// glossiness as the roughness. The specular color and the
/// specular-glossiness texture have no counterpart there (`true`: some
/// material was converted, so the caller warns that it is approximated).
pub(super) fn specular_glossiness_as_metal_rough(json: &mut Value) -> bool {
    let Some(materials) = json.get_mut("materials").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut converted = false;
    for material in materials {
        if material.get("pbrMetallicRoughness").is_some() {
            continue;
        }
        let Some(sg) = extension(material, SPECULAR_GLOSSINESS).cloned() else {
            continue;
        };
        let mut pbr = serde_json::Map::new();
        if let Some(factor) = sg.get("diffuseFactor") {
            pbr.insert("baseColorFactor".into(), factor.clone());
        }
        if let Some(texture) = sg.get("diffuseTexture") {
            pbr.insert("baseColorTexture".into(), texture.clone());
        }
        pbr.insert("metallicFactor".into(), Value::from(0.0));
        let glossiness = sg["glossinessFactor"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0);
        pbr.insert("roughnessFactor".into(), Value::from(1.0 - glossiness));
        material["pbrMetallicRoughness"] = Value::Object(pbr);
        converted = true;
    }
    converted
}

/// glTF's GPU instancing extension: a node that draws its mesh once per
/// instance, each instance with the translation, rotation and scale its
/// accessors give (in the node's space).
const INSTANCING: &str = "EXT_mesh_gpu_instancing";

/// Gives every instance of each [`INSTANCING`] node a child node of its
/// own that shows the node's mesh with the instance's transform, and the
/// node itself shows nothing: the loader draws nodes, so each instance
/// draws (as a copy of the mesh in the loaded model). A node with a skin
/// is left as it is. Returns how many nodes' instances couldn't be read
/// (their mesh draws once, at the node, as without the extension).
pub(super) fn instances_as_nodes(json: &mut Value, buffers: &[Vec<u8>]) -> usize {
    let mut unread = 0;
    let mut added = Vec::new();
    for (n, node) in json["nodes"].as_array().into_iter().flatten().enumerate() {
        let Some(attributes) = node["extensions"][INSTANCING]["attributes"].as_object() else {
            continue;
        };
        if node.get("mesh").is_none() || node.get("skin").is_some() {
            continue;
        }
        let read = |name: &str, components: usize| {
            attributes
                .get(name)
                .map(|accessor| accessor_floats(json, buffers, accessor, components))
        };
        let parts = [read("TRANSLATION", 3), read("ROTATION", 4), read("SCALE", 3)];
        if parts.iter().any(|p| matches!(p, Some(None))) {
            unread += 1;
            continue;
        }
        let parts: Vec<(usize, Vec<f32>)> = [("translation", 3), ("rotation", 4), ("scale", 3)]
            .iter()
            .zip(parts)
            .filter_map(|(&(_, n), p)| Some((n, p.flatten()?)))
            .collect();
        let Some(count) = parts.first().map(|(n, values)| values.len() / n) else {
            continue;
        };
        if parts.iter().any(|(n, values)| values.len() != count * n) {
            unread += 1;
            continue;
        }
        let names: Vec<&str> = ["TRANSLATION", "ROTATION", "SCALE"]
            .iter()
            .zip(["translation", "rotation", "scale"])
            .filter(|(attribute, _)| attributes.contains_key(**attribute))
            .map(|(_, path)| path)
            .collect();
        let children: Vec<Value> = (0..count)
            .map(|i| {
                let mut child = serde_json::json!({ "mesh": node["mesh"] });
                if let Some(weights) = node.get("weights") {
                    child["weights"] = weights.clone();
                }
                for (path, (components, values)) in names.iter().zip(&parts) {
                    child[*path] = values[i * components..(i + 1) * components].into();
                }
                child
            })
            .collect();
        added.push((n, children));
    }
    for (n, children) in added {
        let Some(nodes) = json["nodes"].as_array_mut() else {
            break;
        };
        let first = nodes.len();
        nodes.extend(children);
        let last = nodes.len();
        let node = &mut nodes[n];
        let mut list = node["children"].as_array().cloned().unwrap_or_default();
        list.extend((first..last).map(Value::from));
        node["children"] = Value::Array(list);
        if let Some(node) = node.as_object_mut() {
            node.remove("mesh");
            node.remove("weights");
        }
    }
    unread
}

/// The values of a `components`-wide accessor (VEC3 or VEC4) as floats:
/// float components as they are, normalized integer ones scaled to 0..1 or
/// -1..1. `None` when it can't be read here: sparse, another type, or
/// pointing past its buffer.
fn accessor_floats(json: &Value, buffers: &[Vec<u8>], index: &Value, components: usize) -> Option<Vec<f32>> {
    let accessor = &json["accessors"][usize::try_from(index.as_u64()?).ok()?];
    let ty = ["", "SCALAR", "VEC2", "VEC3", "VEC4"].get(components)?;
    if accessor["type"] != *ty || accessor.get("sparse").is_some() {
        return None;
    }
    let field = |value: &Value, key: &str| value[key].as_u64().and_then(|v| usize::try_from(v).ok());
    let count = field(accessor, "count")?;
    let view = &json["bufferViews"][field(accessor, "bufferView")?];
    let data = buffers.get(field(view, "buffer")?)?;
    let (size, read): (usize, fn(&[u8]) -> f32) = match accessor["componentType"].as_u64()? {
        5126 => (4, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        5120 => (1, |b| (f32::from(b[0] as i8) / 127.0).max(-1.0)),
        5121 => (1, |b| f32::from(b[0]) / 255.0),
        5122 => (2, |b| (f32::from(i16::from_le_bytes([b[0], b[1]])) / 32767.0).max(-1.0)),
        5123 => (2, |b| f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0),
        _ => return None,
    };
    let start = field(view, "byteOffset").unwrap_or(0) + field(accessor, "byteOffset").unwrap_or(0);
    let stride = field(view, "byteStride").unwrap_or(size * components);
    let mut out = Vec::with_capacity(count * components);
    for i in 0..count {
        for c in 0..components {
            let at = start + i * stride + c * size;
            out.push(read(data.get(at..at + size)?));
        }
    }
    Some(out)
}

/// Takes out the animation channels that animate no node: those of
/// `KHR_animation_pointer` (a material's color, a texture's offset), whose
/// target has a pointer instead of a node. They are read apart
/// ([`pointer_channels`]): the `gltf` crate refuses a target without a
/// node, which would fail the whole file.
pub(super) fn drop_channels_without_nodes(json: &mut Value) {
    let Some(animations) = json.get_mut("animations").and_then(Value::as_array_mut) else {
        return;
    };
    for animation in animations {
        if let Some(channels) = animation.get_mut("channels").and_then(Value::as_array_mut) {
            channels.retain(|c| {
                c["target"]["node"].is_u64()
                    && matches!(
                        c["target"]["path"].as_str(),
                        Some("translation" | "rotation" | "scale" | "weights")
                    )
            });
        }
    }
}

/// One `KHR_animation_pointer` channel: its animation, its pointer, and
/// its sampler's key times, values and interpolation.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PointerChannel {
    pub animation: usize,
    pub pointer: String,
    /// Accessor indices.
    pub input: usize,
    pub output: usize,
    pub interpolation: crate::three::Interpolation,
}

/// The file's `KHR_animation_pointer` channels (a target with a pointer
/// and no node). A channel whose sampler doesn't exist is left out.
pub(super) fn pointer_channels(json: &Value) -> Vec<PointerChannel> {
    use crate::three::Interpolation;
    let mut out = Vec::new();
    for (a, animation) in json["animations"].as_array().into_iter().flatten().enumerate() {
        for channel in animation["channels"].as_array().into_iter().flatten() {
            let target = &channel["target"];
            if target["node"].is_u64() {
                continue;
            }
            let Some(pointer) = extension(target, "KHR_animation_pointer").and_then(|p| p["pointer"].as_str()) else {
                continue;
            };
            let sampler = channel["sampler"]
                .as_u64()
                .and_then(|i| animation["samplers"].get(usize::try_from(i).ok()?));
            let Some(sampler) = sampler else {
                continue;
            };
            let index = |key: &str| sampler[key].as_u64().and_then(|i| usize::try_from(i).ok());
            let (Some(input), Some(output)) = (index("input"), index("output")) else {
                continue;
            };
            out.push(PointerChannel {
                animation: a,
                pointer: pointer.to_string(),
                input,
                output,
                interpolation: match sampler["interpolation"].as_str() {
                    Some("STEP") => Interpolation::Step,
                    Some("CUBICSPLINE") => Interpolation::CubicSpline,
                    _ => Interpolation::Linear,
                },
            });
        }
    }
    out
}

/// What a node's pointer channel animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeValue {
    Translation,
    Rotation,
    Scale,
    /// `KHR_node_visibility`'s `visible`.
    Visible,
}

/// What a `KHR_animation_pointer` pointer animates, as the engine plays it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PointerTarget {
    /// Node `.0`'s value.
    Node(usize, NodeValue),
    /// Material `.0`'s value.
    Material(usize, crate::three::MaterialProperty),
}

impl PointerTarget {
    /// How many components each key has (glTF's accessor type).
    pub fn components(&self) -> usize {
        use crate::three::MaterialProperty as P;
        match self {
            Self::Node(_, NodeValue::Translation | NodeValue::Scale) => 3,
            Self::Node(_, NodeValue::Rotation) => 4,
            Self::Node(_, NodeValue::Visible) => 1,
            Self::Material(_, P::Color) => 4,
            Self::Material(_, P::Emissive | P::SheenColor | P::SpecularColor) => 3,
            Self::Material(_, P::TextureOffset | P::TextureScale) => 2,
            Self::Material(..) => 1,
        }
    }
}

/// The JSON paths of a material's textures whose transform its maps can
/// follow, in the order the first present one is picked
/// ([`material_extensions`]).
const MAPPED_TEXTURES: [&str; 15] = [
    "pbrMetallicRoughness/baseColorTexture",
    "emissiveTexture",
    "normalTexture",
    "pbrMetallicRoughness/metallicRoughnessTexture",
    "occlusionTexture",
    "extensions/KHR_materials_clearcoat/clearcoatTexture",
    "extensions/KHR_materials_clearcoat/clearcoatRoughnessTexture",
    "extensions/KHR_materials_clearcoat/clearcoatNormalTexture",
    "extensions/KHR_materials_sheen/sheenColorTexture",
    "extensions/KHR_materials_sheen/sheenRoughnessTexture",
    "extensions/KHR_materials_specular/specularTexture",
    "extensions/KHR_materials_specular/specularColorTexture",
    "extensions/KHR_materials_anisotropy/anisotropyTexture",
    "extensions/KHR_materials_iridescence/iridescenceTexture",
    "extensions/KHR_materials_iridescence/iridescenceThicknessTexture",
];

/// The texture (its path in [`MAPPED_TEXTURES`]) whose mapping material
/// `material` (its JSON) follows: its first present one.
fn mapped_texture(material: &Value) -> Option<&'static str> {
    MAPPED_TEXTURES
        .iter()
        .copied()
        .find(|path| path.split('/').fold(material, |v, key| &v[key]).is_object())
}

/// What `pointer` (a JSON pointer into the file) animates, or why the
/// engine doesn't play it. Node indices and material indices are checked
/// against the file.
pub(super) fn pointer_target(json: &Value, pointer: &str) -> Result<PointerTarget, &'static str> {
    use crate::three::MaterialProperty as P;
    let parts: Vec<&str> = pointer.strip_prefix('/').unwrap_or(pointer).split('/').collect();
    let index = |kind: &str| -> Result<usize, &'static str> {
        let i: usize = parts
            .get(1)
            .and_then(|i| i.parse().ok())
            .ok_or("its index isn't a number")?;
        if json[kind].get(i).is_none() {
            return Err("it points past the file's list");
        }
        Ok(i)
    };
    match parts.first().copied() {
        Some("nodes") => {
            let node = index("nodes")?;
            let value = match &parts[2..] {
                ["translation"] => NodeValue::Translation,
                ["rotation"] => NodeValue::Rotation,
                ["scale"] => NodeValue::Scale,
                ["extensions", "KHR_node_visibility", "visible"] => NodeValue::Visible,
                ["weights"] => return Err("morph weights animated through a pointer aren't played"),
                _ => return Err("that value of a node isn't animated"),
            };
            Ok(PointerTarget::Node(node, value))
        }
        Some("materials") => {
            let m = index("materials")?;
            let rest = &parts[2..];
            let property = match rest {
                ["pbrMetallicRoughness", "baseColorFactor"] => P::Color,
                ["pbrMetallicRoughness", "metallicFactor"] => P::Metallic,
                ["pbrMetallicRoughness", "roughnessFactor"] => P::Roughness,
                ["emissiveFactor"] => P::Emissive,
                ["alphaCutoff"] => P::AlphaCutoff,
                ["normalTexture", "scale"] => P::NormalStrength,
                ["extensions", "KHR_materials_emissive_strength", "emissiveStrength"] => P::EmissiveStrength,
                ["extensions", "KHR_materials_clearcoat", "clearcoatFactor"] => P::Clearcoat,
                ["extensions", "KHR_materials_clearcoat", "clearcoatRoughnessFactor"] => P::ClearcoatRoughness,
                ["extensions", "KHR_materials_sheen", "sheenColorFactor"] => P::SheenColor,
                ["extensions", "KHR_materials_sheen", "sheenRoughnessFactor"] => P::SheenRoughness,
                ["extensions", "KHR_materials_specular", "specularFactor"] => P::Specular,
                ["extensions", "KHR_materials_specular", "specularColorFactor"] => P::SpecularColor,
                ["extensions", "KHR_materials_ior", "ior"] => P::Ior,
                ["extensions", "KHR_materials_anisotropy", "anisotropyStrength"] => P::Anisotropy,
                ["extensions", "KHR_materials_anisotropy", "anisotropyRotation"] => P::AnisotropyRotation,
                ["extensions", "KHR_materials_iridescence", "iridescenceFactor"] => P::Iridescence,
                ["extensions", "KHR_materials_iridescence", "iridescenceIor"] => P::IridescenceIor,
                ["extensions", "KHR_materials_iridescence", "iridescenceThicknessMinimum"] => P::IridescenceThinnest,
                ["extensions", "KHR_materials_iridescence", "iridescenceThicknessMaximum"] => P::IridescenceThickest,
                [texture @ .., "extensions", "KHR_texture_transform", field] => {
                    let path = texture.join("/");
                    if !MAPPED_TEXTURES.contains(&path.as_str()) {
                        return Err("that texture isn't read");
                    }
                    if mapped_texture(&json["materials"][m]) != Some(path.as_str()) {
                        return Err("the material's maps follow its first texture's transform, not this texture's");
                    }
                    match *field {
                        "offset" => P::TextureOffset,
                        "rotation" => P::TextureRotation,
                        "scale" => P::TextureScale,
                        _ => return Err("that value of a texture transform isn't animated"),
                    }
                }
                _ => return Err("that value of a material isn't animated"),
            };
            Ok(PointerTarget::Material(m, property))
        }
        _ => Err("only nodes and materials are animated"),
    }
}

/// The key values of accessor `index`, `components` per key, as floats:
/// float components as they are, integer ones as numbers (scaled to 0..1
/// or -1..1 when the accessor is normalized). `None` when it can't be read
/// here: another type, sparse, or pointing past its buffer.
pub(super) fn accessor_values(json: &Value, buffers: &[Vec<u8>], index: usize, components: usize) -> Option<Vec<f32>> {
    let accessor = json["accessors"].get(index)?;
    let ty = ["", "SCALAR", "VEC2", "VEC3", "VEC4"].get(components)?;
    if accessor["type"] != *ty || accessor.get("sparse").is_some() {
        return None;
    }
    let field = |value: &Value, key: &str| value[key].as_u64().and_then(|v| usize::try_from(v).ok());
    let count = field(accessor, "count")?;
    let normalized = accessor["normalized"].as_bool() == Some(true);
    let view = &json["bufferViews"][field(accessor, "bufferView")?];
    let data = buffers.get(field(view, "buffer")?)?;
    /// A component's size in bytes, how to read one, and what
    /// normalizing divides by.
    type Component = (usize, fn(&[u8]) -> f32, f32);
    let (size, read, scale): Component = match accessor["componentType"].as_u64()? {
        5126 => (4, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]), 1.0),
        5120 => (1, |b| f32::from(b[0] as i8), 127.0),
        5121 => (1, |b| f32::from(b[0]), 255.0),
        5122 => (2, |b| f32::from(i16::from_le_bytes([b[0], b[1]])), 32767.0),
        5123 => (2, |b| f32::from(u16::from_le_bytes([b[0], b[1]])), 65535.0),
        _ => return None,
    };
    let start = field(view, "byteOffset").unwrap_or(0) + field(accessor, "byteOffset").unwrap_or(0);
    let stride = field(view, "byteStride").unwrap_or(size * components);
    let mut out = Vec::with_capacity(count * components);
    for i in 0..count {
        for c in 0..components {
            let at = start + i * stride + c * size;
            let v = read(data.get(at..at + size)?);
            out.push(if normalized && size < 4 {
                (v / scale).max(-1.0)
            } else {
                v
            });
        }
    }
    Some(out)
}

/// The extension `ponko cook` marks the node a quantized mesh hangs from
/// with: a child that only moves and scales the mesh's 16-bit positions
/// back to the size they were.
pub(super) const QUANTIZED_POSITIONS: &str = "PONKO_quantized_positions";

/// Folds each node marked [`QUANTIZED_POSITIONS`] back into its parent: the
/// parent shows the mesh again and the marked node leaves the tree (it
/// stays in the list, empty, so no index changes), and the mesh's positions
/// go through the marked node's transform as they are read (the returned
/// matrices, by mesh and primitive). So the node tree, transforms and
/// extras are the source's, and so are positions, within the cook's bound.
///
/// A marked node shows its parent's whole mesh (an empty extension), or,
/// for a mesh the cook gave a range per primitive, some of its primitives:
/// the extension names the source mesh and where each of the node's mesh's
/// primitives was in it (`{"mesh": 3, "primitives": [0, 2]}`). Those pieces
/// are put back together into the source mesh, in its order.
///
/// A mesh is folded only when every node showing it or one of its pieces
/// is such a node with one parent that shows nothing itself, every parent
/// has the same pieces with the same transforms, and they hold each
/// primitive once; otherwise the file is read as it is (and still draws
/// right).
pub(super) fn fold_quantized_positions(json: &mut Value) -> Vec<Vec<Mat4>> {
    let meshes = json["meshes"].as_array().map_or(0, Vec::len);
    let mut out = vec![Vec::new(); meshes];
    let primitive_count = |json: &Value, m: usize| json["meshes"][m]["primitives"].as_array().map_or(0, Vec::len);
    let counts: Vec<usize> = (0..meshes).map(|m| primitive_count(json, m)).collect();
    let Some(nodes) = json.get_mut("nodes").and_then(Value::as_array_mut) else {
        return out;
    };
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (p, node) in nodes.iter().enumerate() {
        for c in node["children"].as_array().into_iter().flatten() {
            if let Some(list) = c.as_u64().and_then(|c| parents.get_mut(c as usize)) {
                list.push(p);
            }
        }
    }
    let floats = |v: &Value, default: &[f32]| -> Option<[f32; 3]> {
        match v.as_array() {
            Some(a) => a
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32))
                .collect::<Option<Vec<f32>>>()?
                .try_into()
                .ok(),
            None => default.try_into().ok(),
        }
    };
    // One marked node: the source mesh, the mesh it shows, where that
    // mesh's primitives were in the source, its matrix and its parent.
    struct Piece {
        mesh: usize,
        primitives: Vec<usize>,
        matrix: Mat4,
        node: usize,
        parent: usize,
    }
    // Per source mesh: its pieces, or `None` once something rules folding out.
    let mut plans: Vec<Option<Vec<Piece>>> = (0..meshes).map(|_| Some(Vec::new())).collect();
    // The source each mesh is a piece of (itself by default), to find
    // nodes that show a piece without being marked.
    let mut source_of: Vec<usize> = (0..meshes).collect();
    for node in nodes.iter() {
        if let (Some(mesh), Some(ext)) = (node["mesh"].as_u64(), extension(node, QUANTIZED_POSITIONS))
            && let Some(source) = ext["mesh"].as_u64().map(|m| m as usize).filter(|m| *m < meshes)
            && let Some(slot) = source_of.get_mut(mesh as usize)
        {
            *slot = source;
        }
    }
    for (n, node) in nodes.iter().enumerate() {
        let Some(mesh) = node["mesh"].as_u64().map(|m| m as usize).filter(|m| *m < meshes) else {
            continue;
        };
        let source = source_of[mesh];
        let Some(list) = plans[source].as_mut() else {
            continue;
        };
        let ext = extension(node, QUANTIZED_POSITIONS);
        let foldable = ext.is_some()
            && node.get("children").is_none()
            && node.get("matrix").is_none()
            && node.get("rotation").is_none()
            && parents[n].len() == 1
            && nodes[parents[n][0]].get("mesh").is_none();
        let primitives: Option<Vec<usize>> = match ext.map(|e| &e["primitives"]) {
            Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().map(|i| i as usize)).collect(),
            _ => Some((0..counts[mesh]).collect()),
        };
        let (t, s) = (
            floats(&node["translation"], &[0.0; 3]),
            floats(&node["scale"], &[1.0; 3]),
        );
        let (true, Some(primitives), Some(t), Some(s)) = (foldable, primitives, t, s) else {
            plans[source] = None;
            continue;
        };
        if primitives.len() != counts[mesh] {
            plans[source] = None;
            continue;
        }
        list.push(Piece {
            mesh,
            primitives,
            matrix: Mat4::from_scale_rotation_translation(Vec3::from(s), glam::Quat::IDENTITY, Vec3::from(t)),
            node: n,
            parent: parents[n][0],
        });
    }
    // Each source mesh's primitives, back in their order: (piece mesh,
    // index in it, matrix) for each.
    type Order = Vec<(usize, usize, Mat4)>;
    let mut rebuilt: Vec<(usize, Order)> = Vec::new();
    for (source, plan) in plans.iter().enumerate() {
        let Some(pieces) = plan.as_ref().filter(|p| !p.is_empty()) else {
            continue;
        };
        // Every parent has the same pieces: compare each parent's layout
        // with the first's.
        let layout_of = |parent: usize| {
            let mut l: Vec<(usize, &[usize], [f32; 16])> = pieces
                .iter()
                .filter(|p| p.parent == parent)
                .map(|p| (p.mesh, p.primitives.as_slice(), p.matrix.to_cols_array()))
                .collect();
            l.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
            l
        };
        let first = layout_of(pieces[0].parent);
        if pieces.iter().any(|p| layout_of(p.parent) != first) {
            continue;
        }
        let total: usize = first.iter().map(|(_, list, _)| list.len()).sum();
        let mut slots: Vec<Option<(usize, usize, Mat4)>> = vec![None; total];
        let mut whole = true;
        for (mesh, list, matrix) in &first {
            for (k, &at) in list.iter().enumerate() {
                match slots.get_mut(at) {
                    Some(slot @ None) => *slot = Some((*mesh, k, Mat4::from_cols_array(matrix))),
                    _ => whole = false,
                }
            }
        }
        let Some(order) = whole.then(|| slots.into_iter().collect::<Option<Vec<_>>>()).flatten() else {
            continue;
        };
        rebuilt.push((source, order));
    }
    let mut folded = Vec::new();
    for (source, order) in rebuilt {
        out[source] = order.iter().map(|(_, _, m)| *m).collect();
        if let Some(pieces) = plans[source].take() {
            folded.push((source, pieces));
        }
        // The source mesh holds its primitives again; the other pieces'
        // meshes stay in the list, shown by no node.
        if order.iter().any(|(mesh, _, _)| *mesh != source) {
            let primitives: Vec<Value> = order
                .iter()
                .map(|(mesh, k, _)| json["meshes"][*mesh]["primitives"][*k].clone())
                .collect();
            json["meshes"][source]["primitives"] = Value::Array(primitives);
        }
    }
    let Some(nodes) = json.get_mut("nodes").and_then(Value::as_array_mut) else {
        return out;
    };
    for (source, pieces) in folded {
        for piece in pieces {
            if let Some(node) = nodes[piece.node].as_object_mut() {
                node.remove("mesh");
            }
            let parent = &mut nodes[piece.parent];
            parent["mesh"] = Value::from(source);
            if let Some(children) = parent.get_mut("children").and_then(Value::as_array_mut) {
                children.retain(|c| c.as_u64() != Some(piece.node as u64));
                if children.is_empty()
                    && let Some(o) = parent.as_object_mut()
                {
                    o.remove("children");
                }
            }
        }
    }
    out
}

/// A buffer's or buffer view's meshopt extension object, by either name.
fn meshopt(value: &Value) -> Option<&Value> {
    extension(value, "EXT_meshopt_compression").or_else(|| extension(value, "KHR_meshopt_compression"))
}

/// Decompresses every meshopt buffer view into one new buffer at the end
/// of `buffers`, and points the views at it. Without the `gltf-meshopt`
/// feature, views keep their fallback data, and one without any fails.
fn decompress_views(name: &str, json: &mut Value, buffers: &mut Vec<Vec<u8>>) -> Result<(), String> {
    let Some(views) = json.get_mut("bufferViews").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut decoded: Vec<u8> = Vec::new();
    for (i, view) in views.iter_mut().enumerate() {
        let Some(ext) = meshopt(view).cloned() else {
            continue;
        };
        if !MESHOPT {
            let buffer = view["buffer"].as_u64().unwrap_or(u64::MAX) as usize;
            if buffers.get(buffer).is_none_or(Vec::is_empty) {
                return Err(format!(
                    "{name}: buffer view {i} is meshopt compressed with no fallback, and this build \
                     of ponko doesn't decompress it: turn on its `gltf-meshopt` cargo feature"
                ));
            }
            continue;
        }
        let field = |key: &str| ext[key].as_u64().map(|v| v as usize);
        let (Some(buffer), Some(length), Some(stride), Some(count)) = (
            field("buffer"),
            field("byteLength"),
            field("byteStride"),
            field("count"),
        ) else {
            return Err(format!("{name}: buffer view {i} has an incomplete meshopt extension"));
        };
        let offset = field("byteOffset").unwrap_or(0);
        let source = buffers
            .get(buffer)
            .and_then(|b| b.get(offset..offset.checked_add(length)?))
            .ok_or_else(|| format!("{name}: buffer view {i}'s compressed data is past the end of its buffer"))?;
        let mode = ext["mode"].as_str().unwrap_or("");
        let filter = ext["filter"].as_str().unwrap_or("NONE");
        let bytes =
            decode_meshopt(source, count, stride, mode, filter).map_err(|e| format!("{name}: buffer view {i}: {e}"))?;
        // Views start 4-byte aligned, as glTF asks.
        decoded.resize(decoded.len().next_multiple_of(4), 0);
        let at = decoded.len();
        decoded.extend_from_slice(&bytes);
        let object = view
            .as_object_mut()
            .expect("a buffer view with an extension is an object");
        object.insert("buffer".into(), Value::from(buffers.len()));
        object.insert("byteOffset".into(), Value::from(at));
        object.insert("byteLength".into(), Value::from(bytes.len()));
        if let Some(extensions) = object.get_mut("extensions").and_then(Value::as_object_mut) {
            extensions.remove("EXT_meshopt_compression");
            extensions.remove("KHR_meshopt_compression");
        }
    }
    if !decoded.is_empty() {
        let length = decoded.len();
        buffers.push(decoded);
        if let Some(list) = json.get_mut("buffers").and_then(Value::as_array_mut) {
            list.push(serde_json::json!({ "byteLength": length }));
        }
    }
    Ok(())
}

/// Decompresses one meshopt buffer view: `count` elements of `stride`
/// bytes, by `mode` (ATTRIBUTES, TRIANGLES or INDICES), then `filter`.
#[cfg(all(feature = "gltf-meshopt", not(target_arch = "wasm32")))]
fn decode_meshopt(source: &[u8], count: usize, stride: usize, mode: &str, filter: &str) -> Result<Vec<u8>, String> {
    use meshopt::ffi;
    /// The most bytes one view may decompress to: a guard against a
    /// corrupt `count` asking for gigabytes.
    const MESHOPT_MAX_BYTES: usize = 1 << 30;
    let bytes = count
        .checked_mul(stride)
        .filter(|b| *b <= MESHOPT_MAX_BYTES)
        .ok_or_else(|| format!("{count} elements of {stride} bytes is too much to decompress"))?;
    let stride_ok = match mode {
        "ATTRIBUTES" => stride.is_multiple_of(4) && (4..=256).contains(&stride),
        "TRIANGLES" => (stride == 2 || stride == 4) && count.is_multiple_of(3),
        "INDICES" => stride == 2 || stride == 4,
        _ => return Err(format!("unknown meshopt mode {mode:?}")),
    };
    if !stride_ok {
        return Err(format!(
            "a meshopt {mode} view can't have {count} elements of {stride} bytes"
        ));
    }
    let mut out = vec![0u8; bytes];
    // SAFETY: `out` holds `count * stride` bytes, what each decoder writes
    // for `count` elements of `stride` bytes; the decoders read at most
    // `source.len()` bytes of `source` and check the data (meshoptimizer
    // decodes untrusted input safely, garbage in giving garbage out).
    let result = unsafe {
        let (dst, src) = (out.as_mut_ptr().cast(), source.as_ptr());
        match mode {
            "ATTRIBUTES" => ffi::meshopt_decodeVertexBuffer(dst, count, stride, src, source.len()),
            "TRIANGLES" => ffi::meshopt_decodeIndexBuffer(dst, count, stride, src, source.len()),
            _ => ffi::meshopt_decodeIndexSequence(dst, count, stride, src, source.len()),
        }
    };
    if result != 0 {
        return Err(format!("its meshopt data is corrupt (error {result})"));
    }
    let filter_ok = match filter {
        "NONE" => true,
        "OCTAHEDRAL" | "COLOR" => mode == "ATTRIBUTES" && (stride == 4 || stride == 8),
        "QUATERNION" => mode == "ATTRIBUTES" && stride == 8,
        "EXPONENTIAL" => mode == "ATTRIBUTES",
        _ => return Err(format!("unknown meshopt filter {filter:?}")),
    };
    if !filter_ok {
        return Err(format!(
            "the meshopt {filter} filter doesn't apply to {mode} of {stride} bytes"
        ));
    }
    // SAFETY: `out` holds `count` elements of `stride` bytes, the strides
    // each filter accepts were checked above, and the filters only rewrite
    // those elements in place.
    unsafe {
        let data = out.as_mut_ptr().cast();
        match filter {
            "OCTAHEDRAL" => ffi::meshopt_decodeFilterOct(data, count, stride),
            "QUATERNION" => ffi::meshopt_decodeFilterQuat(data, count, stride),
            "EXPONENTIAL" => ffi::meshopt_decodeFilterExp(data, count, stride),
            "COLOR" => ffi::meshopt_decodeFilterColor(data, count, stride),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(not(all(feature = "gltf-meshopt", not(target_arch = "wasm32"))))]
fn decode_meshopt(_: &[u8], _: usize, _: usize, _: &str, _: &str) -> Result<Vec<u8>, String> {
    unreachable!("meshopt views are only decompressed with the `gltf-meshopt` feature")
}

/// The extensions that give a texture an image in another format than
/// its core PNG or JPEG one.
const IMAGE_EXTENSIONS: [&str; 3] = ["KHR_texture_basisu", "EXT_texture_webp", "MSFT_texture_dds"];

/// Texture images given by extensions (`KHR_texture_basisu`'s KTX2,
/// `EXT_texture_webp`'s WebP, `MSFT_texture_dds`'s DDS) the build reads. A
/// texture with only such an image gets it as its source (the core image is
/// required by the `gltf` crate); one with both gets it as its preferred
/// image, its core image staying the fallback.
fn texture_sources(name: &str, json: &mut Value) -> Result<Vec<Option<usize>>, String> {
    let Some(textures) = json.get_mut("textures").and_then(Value::as_array_mut) else {
        return Ok(Vec::new());
    };
    let mut preferred = Vec::with_capacity(textures.len());
    for (i, texture) in textures.iter_mut().enumerate() {
        let readable = IMAGE_EXTENSIONS.into_iter().find_map(|ext| {
            let source = extension(texture, ext)?["source"].as_u64()? as usize;
            matches!(support(ext), Some(Support::Feature(_, true))).then_some(source)
        });
        let has_core = texture.get("source").is_some_and(|s| s.is_u64());
        match (readable, has_core) {
            (Some(source), false) => {
                texture["source"] = Value::from(source);
                preferred.push(None);
            }
            (Some(source), true) => preferred.push(Some(source)),
            (None, true) => preferred.push(None),
            (None, false) => {
                let wanted = IMAGE_EXTENSIONS
                    .into_iter()
                    .find(|ext| extension(texture, ext).is_some());
                return Err(match wanted.and_then(|ext| Some((ext, support(ext)?))) {
                    Some((ext, Support::Feature(feature, _))) => format!(
                        "{name}: texture {i} has only an {ext} image, which this build of ponko doesn't \
                         read: turn on its `{feature}` cargo feature"
                    ),
                    _ => format!("{name}: texture {i} has no image"),
                });
            }
        }
    }
    Ok(preferred)
}

/// Reads accessor `accessor` as `N`-component vectors of floats, whatever
/// its component type: floats, or integers (normalized to -1..1 or 0..1
/// when it says so), as `KHR_mesh_quantization` allows for positions,
/// normals, texture coordinates and morph target offsets. Sparse
/// accessors are filled in.
pub(super) fn read_floats<const N: usize>(
    accessor: &gltf::Accessor,
    buffers: &[Vec<u8>],
    name: &str,
) -> Result<Vec<[f32; N]>, String> {
    use gltf::accessor::DataType;
    let bad = |why: &str| format!("{name}: accessor {} {why}", accessor.index());
    if accessor.dimensions().multiplicity() != N {
        return Err(bad(&format!(
            "has {} components, not {N}",
            accessor.dimensions().multiplicity()
        )));
    }
    let ty = accessor.data_type();
    let size = ty.size();
    let normalized = accessor.normalized();
    let component = move |b: &[u8]| -> f32 {
        let v = match ty {
            DataType::I8 => f32::from(b[0] as i8),
            DataType::U8 => f32::from(b[0]),
            DataType::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])),
            DataType::U16 => f32::from(u16::from_le_bytes([b[0], b[1]])),
            DataType::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            DataType::F32 => return f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        };
        if !normalized {
            return v;
        }
        match ty {
            DataType::I8 => (v / 127.0).max(-1.0),
            DataType::U8 => v / 255.0,
            DataType::I16 => (v / 32767.0).max(-1.0),
            DataType::U16 => v / 65535.0,
            _ => v,
        }
    };
    let element = move |data: &[u8], at: usize| -> Option<[f32; N]> {
        let bytes = data.get(at..at.checked_add(size * N)?)?;
        Some(std::array::from_fn(|k| component(&bytes[k * size..])))
    };
    let count = accessor.count();
    let mut out = vec![[0.0; N]; count];
    if let Some(view) = accessor.view() {
        let data = view_bytes(&view, buffers).ok_or_else(|| bad("is past the end of its buffer"))?;
        let stride = view.stride().unwrap_or(size * N);
        for (i, v) in out.iter_mut().enumerate() {
            *v = element(data, accessor.offset() + i * stride).ok_or_else(|| bad("is past the end of its view"))?;
        }
    }
    if let Some(sparse) = accessor.sparse() {
        let indices = sparse.indices();
        let index_data =
            view_bytes(&indices.view(), buffers).ok_or_else(|| bad("has sparse indices past its buffer"))?;
        let index_size = indices.index_type().size();
        let values = sparse.values();
        let value_data = view_bytes(&values.view(), buffers).ok_or_else(|| bad("has sparse values past its buffer"))?;
        for k in 0..sparse.count() {
            let at = indices.offset() + k * index_size;
            let b = index_data
                .get(at..at + index_size)
                .ok_or_else(|| bad("has sparse indices past their view"))?;
            let index = match index_size {
                1 => usize::from(b[0]),
                2 => usize::from(u16::from_le_bytes([b[0], b[1]])),
                _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize,
            };
            let value = element(value_data, values.offset() + k * size * N)
                .ok_or_else(|| bad("has sparse values past their view"))?;
            *out.get_mut(index)
                .ok_or_else(|| bad("has a sparse index past its end"))? = value;
        }
    }
    Ok(out)
}

/// The bytes of buffer view `view`.
fn view_bytes<'b>(view: &gltf::buffer::View, buffers: &'b [Vec<u8>]) -> Option<&'b [u8]> {
    buffers
        .get(view.buffer().index())?
        .get(view.offset()..view.offset().checked_add(view.length())?)
}

/// How a material's textures are laid on its meshes: which texture
/// coordinate set, and how `KHR_texture_transform` moves it. The renderer
/// reads every map of a material with one set of coordinates: the set is
/// picked when the file loads, the transform is the material's
/// [`TextureTransform`](super::TextureTransform), applied as it draws (so
/// it can be animated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct UvMapping {
    pub set: u32,
    pub transform: super::TextureTransform,
}

impl UvMapping {
    pub const PLAIN: Self = Self {
        set: 0,
        transform: super::TextureTransform::IDENTITY,
    };
}

/// The mapping of one texture reference (`textureInfo` JSON).
fn mapping_of(info: &Value) -> UvMapping {
    let mut set = info["texCoord"].as_u64().unwrap_or(0) as u32;
    let transform = extension(info, "KHR_texture_transform").map_or(super::TextureTransform::IDENTITY, |t| {
        if let Some(s) = t["texCoord"].as_u64() {
            set = s as u32;
        }
        super::TextureTransform {
            offset: vec2_of(&t["offset"]).unwrap_or(Vec2::ZERO),
            rotation: t["rotation"].as_f64().unwrap_or(0.0) as f32,
            scale: vec2_of(&t["scale"]).unwrap_or(Vec2::ONE),
        }
    });
    UvMapping { set, transform }
}

fn vec2_of(v: &Value) -> Option<Vec2> {
    Some(Vec2::new(v.get(0)?.as_f64()? as f32, v.get(1)?.as_f64()? as f32))
}

/// Extension data of material `material` (its JSON).
pub(super) struct MaterialExtensions {
    pub mapping: UvMapping,
    /// `KHR_materials_emissive_strength`: multiplies the emissive color.
    pub emissive_strength: f32,
    /// `KHR_materials_transmission`'s factor: how much light passes through.
    pub transmission: f32,
    /// `KHR_materials_unlit`: drawn without lighting.
    pub unlit: bool,
    /// Clear coat, sheen, specular, index of refraction, anisotropy and
    /// iridescence (`KHR_materials_*`), without their maps.
    pub layers: MaterialLayers,
    /// The layers' maps, as the file's texture indices, in the shader's
    /// order ([`MapKind`](super::MapKind)).
    pub layer_textures: [Option<usize>; MAP_KINDS],
}

/// The texture infos of a material's layer maps (in the shader's order),
/// and which extension's which field each is.
fn layer_texture_infos(material: &Value) -> [&Value; MAP_KINDS] {
    let ext = |name: &str, field: &str| extension(material, name).map_or(&Value::Null, |e| &e[field]);
    [
        ext("KHR_materials_clearcoat", "clearcoatTexture"),
        ext("KHR_materials_clearcoat", "clearcoatRoughnessTexture"),
        ext("KHR_materials_clearcoat", "clearcoatNormalTexture"),
        ext("KHR_materials_sheen", "sheenColorTexture"),
        ext("KHR_materials_sheen", "sheenRoughnessTexture"),
        ext("KHR_materials_specular", "specularTexture"),
        ext("KHR_materials_specular", "specularColorTexture"),
        ext("KHR_materials_anisotropy", "anisotropyTexture"),
        ext("KHR_materials_iridescence", "iridescenceTexture"),
        ext("KHR_materials_iridescence", "iridescenceThicknessTexture"),
    ]
}

/// A number field of an extension object, or `otherwise`.
fn number(e: Option<&Value>, field: &str, otherwise: f32) -> f32 {
    e.and_then(|e| e[field].as_f64()).map_or(otherwise, |v| v as f32)
}

/// A linear RGB field as an sRGB color (glTF's factors are linear).
fn linear_rgb(e: Option<&Value>, field: &str, otherwise: [f32; 3]) -> Color {
    let v = e.map_or(&Value::Null, |e| &e[field]);
    let c = |i: usize| {
        v.get(i)
            .and_then(Value::as_f64)
            .map_or(otherwise[i], |c| c as f32)
            .max(0.0)
    };
    let srgb = super::gltf::to_srgb;
    Color::rgb(srgb(c(0)), srgb(c(1)), srgb(c(2)))
}

/// Material `material`'s layers (`KHR_materials_clearcoat`, `_sheen`,
/// `_specular`, `_ior`, `_anisotropy`, `_iridescence`), each value clamped
/// to what the renderer takes.
fn layers_of(material: &Value) -> MaterialLayers {
    let clearcoat = extension(material, "KHR_materials_clearcoat");
    let sheen = extension(material, "KHR_materials_sheen");
    let specular = extension(material, "KHR_materials_specular");
    let ior = extension(material, "KHR_materials_ior");
    let anisotropy = extension(material, "KHR_materials_anisotropy");
    let iridescence = extension(material, "KHR_materials_iridescence");
    let unit = |v: f32| v.clamp(0.0, 1.0);
    let mut layers = MaterialLayers::NONE
        .with_clearcoat(
            unit(number(clearcoat, "clearcoatFactor", 0.0)),
            unit(number(clearcoat, "clearcoatRoughnessFactor", 0.0)),
        )
        .with_sheen(
            linear_rgb(sheen, "sheenColorFactor", [0.0; 3]),
            unit(number(sheen, "sheenRoughnessFactor", 0.0)),
        )
        .with_specular(
            unit(number(specular, "specularFactor", 1.0)),
            linear_rgb(specular, "specularColorFactor", [1.0; 3]),
        )
        // An index of refraction of 0 (allowed by the schema) is drawn as 1.
        .with_ior(number(ior, "ior", 1.5).max(1.0))
        .with_anisotropy(
            number(anisotropy, "anisotropyStrength", 0.0).clamp(-1.0, 1.0),
            number(anisotropy, "anisotropyRotation", 0.0),
        );
    if let Some(n) = clearcoat
        .map(|c| &c["clearcoatNormalTexture"])
        .filter(|n| n.is_object())
    {
        layers.clearcoat_normal_strength = n["scale"].as_f64().map_or(1.0, |s| s as f32);
    }
    layers.iridescence = unit(number(iridescence, "iridescenceFactor", 0.0));
    layers.iridescence_ior = number(iridescence, "iridescenceIor", 1.3).max(1.0);
    layers.iridescence_thickness = [
        number(iridescence, "iridescenceThicknessMinimum", 100.0).max(0.0),
        number(iridescence, "iridescenceThicknessMaximum", 400.0).max(0.0),
    ];
    layers
}

/// Reads material `index`'s extensions; warns (into `warnings`) when its
/// textures ask for different mappings, which one set of coordinates
/// can't give.
pub(super) fn material_extensions(
    json: &Value,
    index: usize,
    file: &str,
    warnings: &mut Vec<String>,
) -> MaterialExtensions {
    let material = &json["materials"][index];
    let pbr = &material["pbrMetallicRoughness"];
    // The base color's mapping wins: it's what shows most.
    let core = [
        &pbr["baseColorTexture"],
        &material["emissiveTexture"],
        &material["normalTexture"],
        &pbr["metallicRoughnessTexture"],
        &material["occlusionTexture"],
    ];
    let layer_infos = layer_texture_infos(material);
    let mappings: Vec<UvMapping> = core
        .iter()
        .chain(&layer_infos)
        .filter(|i| i.is_object())
        .map(|i| mapping_of(i))
        .collect();
    let mapping = mappings.first().copied().unwrap_or(UvMapping::PLAIN);
    if mappings.iter().any(|m| *m != mapping) {
        let label = material["name"]
            .as_str()
            .map_or_else(|| format!("material {index}"), |n| format!("material {n:?}"));
        warnings.push(format!(
            "{file}: {label}'s textures use different texture coordinates or transforms; \
             all of them follow its first texture's"
        ));
    }
    MaterialExtensions {
        mapping,
        emissive_strength: extension(material, "KHR_materials_emissive_strength")
            .and_then(|e| e["emissiveStrength"].as_f64())
            .map_or(1.0, |s| s as f32),
        transmission: extension(material, "KHR_materials_transmission")
            .and_then(|e| e["transmissionFactor"].as_f64())
            .map_or(0.0, |t| (t as f32).clamp(0.0, 1.0)),
        unlit: extension(material, "KHR_materials_unlit").is_some(),
        layers: layers_of(material),
        layer_textures: layer_infos.map(|i| i["index"].as_u64().map(|i| i as usize)),
    }
}

/// What material `index` says beyond its description that animated values
/// combine with: its emissive factor (linear) and strength apart, and
/// what transmission leaves of its alpha.
pub(super) fn material_factors(json: &Value, index: usize) -> super::animated_material::MaterialFactors {
    let material = &json["materials"][index];
    let factor = &material["emissiveFactor"];
    let channel = |i: usize| factor.get(i).and_then(Value::as_f64).map_or(0.0, |c| c as f32);
    let transmission = number(
        extension(material, "KHR_materials_transmission"),
        "transmissionFactor",
        0.0,
    );
    super::animated_material::MaterialFactors {
        emissive: [channel(0), channel(1), channel(2)],
        emissive_strength: number(
            extension(material, "KHR_materials_emissive_strength"),
            "emissiveStrength",
            1.0,
        ),
        alpha_scale: 1.0 - transmission.clamp(0.0, 1.0),
    }
}

/// Whether node `node` is visible (`KHR_node_visibility`): a node marked
/// invisible hides itself and everything under it, meshes and lights.
/// This is its visibility at rest; clips may animate it
/// (`KHR_animation_pointer`).
pub(super) fn visible(json: &Value, node: usize) -> bool {
    extension(&json["nodes"][node], "KHR_node_visibility").and_then(|v| v["visible"].as_bool()) != Some(false)
}

/// The light a file's punctual light gives at a distance where ponko's
/// lights stop when the file says no range: its range is where its
/// intensity over distance squared falls to this.
const UNLIMITED_RANGE_CUTOFF: f32 = 0.01;

/// Where a local light of `intensity` with no range in its file stops.
fn unlimited_range(intensity: f32) -> f32 {
    (intensity / UNLIMITED_RANGE_CUTOFF).sqrt()
}

/// The range of a local light of `intensity` (engine units) placed with
/// [`ModelLight::place_exposed`]'s `scale`. A range the file gave stays.
/// One the loader derived (the file gave none) grows to where the exposed
/// light falls to the same cutoff, never below the loaded one.
fn exposed_range(range: f32, intensity: f32, scale: f32) -> f32 {
    if range.to_bits() == unlimited_range(intensity).to_bits() {
        range.max(unlimited_range(intensity * scale))
    } else {
        range
    }
}

/// What a glTF file's light is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelLightKind {
    /// A point light (`KHR_lights_punctual`'s `point`).
    Point(PointLight3D),
    /// A spot light (`spot`), shining along the light's -Z.
    Spot(SpotLight3D),
    /// A directional light (`directional`), shining along the light's -Z:
    /// the scene's sun, set on [`Lighting`](crate::render::Lighting).
    Sun {
        /// sRGB color.
        color: Color,
        /// Illuminance in lux, before exposure.
        intensity: f32,
    },
}

/// A light placed in a glTF file (`KHR_lights_punctual`), from
/// [`Graphics::load_lights`](crate::gfx::Graphics::load_lights).
///
/// Point and spot intensities are the file's candela divided by
/// [`CANDELA_PER_UNIT`](Self::CANDELA_PER_UNIT), as Blender converts its
/// watts; a light the file gives no range reaches where it has faded to
/// almost nothing. Lights don't cast shadows unless you turn that on.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::render::{ModelLight, ModelLightKind};
/// let lamp = ModelLight {
///     name: "lamp".into(),
///     transform: ponko::d3::Transform::at(vec3(0.0, 2.0, 0.0)),
///     kind: ModelLightKind::Point(ponko::d3::PointLight::default()),
///     joint: None,
/// };
/// let mut world = World::new();
/// let house = ponko::d3::Transform::at(vec3(10.0, 0.0, 0.0));
/// let light = lamp.spawn(&mut world, &house).unwrap();
/// let placed = world.get::<ponko::d3::Transform>(light).unwrap();
/// assert_eq!(placed.position, vec3(10.0, 2.0, 0.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLight {
    /// Its name in the file (its node's, or the light's), or empty.
    pub name: String,
    /// Where it is relative to the model; it points along its -Z.
    pub transform: Transform3D,
    /// What kind of light it is, with its color, intensity and reach.
    pub kind: ModelLightKind,
    /// The joint of the file's [`Skeleton`](crate::anim::Skeleton) its node
    /// is, in a file with skins or animations; `None` otherwise. Attach the
    /// light's entity there with
    /// [`AttachToJoint`](crate::anim::AttachToJoint) (at no offset) and it
    /// follows its node's animation, and goes out while its node is hidden
    /// (glTF's `KHR_node_visibility`, maybe animated).
    pub joint: Option<usize>,
}

impl ModelLight {
    /// The luminous efficacy Blender's glTF exporter uses (683 lm/W): a
    /// point or spot light of `I` candela in a file is `I / 683` in ponko's
    /// units on the legacy [`spawn`](Self::spawn) path. The directional
    /// light's intensity is stored in lux.
    pub const CANDELA_PER_UNIT: f32 = 683.0;

    /// Places this light at `model` with a camera exposure (EV100).
    ///
    /// Multiplies lux and candela by `1 / (1.2 * 2^ev100)` before shading.
    /// One stop higher halves the light. The model uses metres, so one
    /// candela at one metre gives the same incident light as one lux.
    /// Local lights use inverse-square falloff (softened inside 1 cm)
    /// and fade toward their loaded range. A light the file gives no
    /// range reaches at least until its exposed light has faded to the
    /// same small cutoff [`load_lights`](crate::gfx::Graphics::load_lights)
    /// uses, so a low EV100 doesn't cut it short. The directional light
    /// replaces the scene sun and returns `None`. Point and spot lights
    /// spawn entities and return their handles.
    ///
    /// Exposure is baked into the light's intensity when placed. Use the
    /// same EV100 for all imported lights in a scene. For a camera-wide
    /// brightness adjustment after placement, use [`Post::exposure`](crate::render::Post::exposure).
    /// A non-finite EV100 uses 0, and finite values are limited to ±32.
    pub fn place_exposed(
        &self,
        world: &mut ponko_ecs::World,
        lighting: &mut super::Lighting,
        model: &Transform3D,
        ev100: f32,
    ) -> Option<ponko_ecs::Entity> {
        let ev100 = if ev100.is_finite() {
            ev100.clamp(-32.0, 32.0)
        } else {
            0.0
        };
        let exposure = (-ev100).exp2() / 1.2;
        let at = self.placed(model);
        let local_scale = Self::CANDELA_PER_UNIT * exposure;
        use super::lights::PhotometricLight;
        match self.kind {
            ModelLightKind::Sun { color, intensity } => {
                lighting.sun_direction = at.forward();
                lighting.sun_color = color;
                lighting.sun_intensity = intensity * exposure;
                None
            }
            ModelLightKind::Point(mut light) => {
                light.range = exposed_range(light.range, light.intensity, local_scale);
                light.intensity *= local_scale;
                Some(world.spawn((at, light, PhotometricLight)))
            }
            ModelLightKind::Spot(mut light) => {
                light.range = exposed_range(light.range, light.intensity, local_scale);
                light.intensity *= local_scale;
                Some(world.spawn((at, light, PhotometricLight)))
            }
        }
    }

    /// Where it is in the world, for a model at `model`.
    #[must_use]
    pub fn placed(&self, model: &Transform3D) -> Transform3D {
        compose(model, &self.transform)
    }

    /// Spawns a point or spot light where it is in the world for a model at
    /// `model`, and returns it; a sun isn't an entity (set it on
    /// [`Lighting`](crate::render::Lighting)), so that returns `None`.
    pub fn spawn(&self, world: &mut ponko_ecs::World, model: &Transform3D) -> Option<ponko_ecs::Entity> {
        let at = self.placed(model);
        match self.kind {
            ModelLightKind::Point(light) => Some(world.spawn((at, light))),
            ModelLightKind::Spot(light) => Some(world.spawn((at, light))),
            ModelLightKind::Sun { .. } => None,
        }
    }
}

/// How a glTF camera projects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelProjection {
    /// Things farther away look smaller.
    Perspective {
        /// Vertical field of view, radians.
        fov_y: f32,
        /// Width over height, if the file gives one.
        aspect: Option<f32>,
        /// Nothing closer than this is drawn.
        near: f32,
        /// `None`: no far plane.
        far: Option<f32>,
    },
    /// Things keep their size at any distance.
    Orthographic {
        /// Half the view's width, in world units.
        half_width: f32,
        /// Half the view's height, in world units.
        half_height: f32,
        /// Nothing closer than this is drawn.
        near: f32,
        /// Nothing farther than this is drawn.
        far: f32,
    },
}

/// A camera placed in a glTF file, from
/// [`Graphics::load_cameras`](crate::gfx::Graphics::load_cameras).
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::render::{ModelCamera, ModelProjection};
/// let shot = ModelCamera {
///     name: "intro".into(),
///     transform: ponko::d3::Transform::at(vec3(0.0, 1.0, 5.0)),
///     projection: ModelProjection::Perspective { fov_y: 0.8, aspect: None, near: 0.05, far: Some(200.0) },
/// };
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     shot.apply(&mut ctx.scene3d.camera, &ponko::d3::Transform::default());
///     assert_eq!(ctx.scene3d.camera.fov_y, 0.8);
///     assert_eq!(ctx.scene3d.camera.position, vec3(0.0, 1.0, 5.0));
/// });
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCamera {
    /// Its name in the file (its node's, or the camera's), or empty.
    pub name: String,
    /// Where it is relative to the model; it looks along its -Z.
    pub transform: Transform3D,
    /// How it projects.
    pub projection: ModelProjection,
}

impl ModelCamera {
    /// Where it is in the world, for a model at `model`.
    #[must_use]
    pub fn placed(&self, model: &Transform3D) -> Transform3D {
        compose(model, &self.transform)
    }

    /// Puts `camera` where this one is for a model at `model`, with its
    /// field of view and clip planes. [`Camera3D`](crate::d3::Camera) is a
    /// perspective camera: an orthographic one sets only its place and
    /// clip planes. The window keeps its own aspect ratio.
    pub fn apply(&self, camera: &mut super::Camera3D, model: &Transform3D) {
        let at = self.placed(model);
        camera.position = at.position;
        camera.rotation = at.rotation;
        match self.projection {
            ModelProjection::Perspective { fov_y, near, far, .. } => {
                camera.fov_y = fov_y;
                camera.near = near;
                if let Some(far) = far {
                    camera.far = far;
                }
            }
            ModelProjection::Orthographic { near, far, .. } => {
                camera.near = near;
                camera.far = far;
            }
        }
    }
}

/// `child` placed in `parent`'s space.
fn compose(parent: &Transform3D, child: &Transform3D) -> Transform3D {
    transform_of(parent.matrix() * child.matrix())
}

fn transform_of(matrix: Mat4) -> Transform3D {
    let (scale, rotation, position) = matrix.to_scale_rotation_translation();
    Transform3D {
        position,
        rotation: rotation.normalize(),
        scale,
    }
}

/// The light of node `node` (its JSON index) placed by `world`, if it has
/// one.
pub(super) fn light_of(json: &Value, node: usize, world: Mat4, node_name: Option<&str>) -> Option<ModelLight> {
    let index = extension(&json["nodes"][node], "KHR_lights_punctual")?["light"].as_u64()? as usize;
    let light = extension(json, "KHR_lights_punctual")?["lights"].get(index)?;
    let [r, g, b] = [0, 1, 2].map(|i| light["color"].get(i).and_then(Value::as_f64).map_or(1.0, |c| c as f32));
    let color = Color::rgb(
        super::gltf::to_srgb(r),
        super::gltf::to_srgb(g),
        super::gltf::to_srgb(b),
    );
    let intensity = light["intensity"].as_f64().map_or(1.0, |i| i as f32).max(0.0);
    let units = intensity / ModelLight::CANDELA_PER_UNIT;
    let range = light["range"]
        .as_f64()
        .map(|r| r as f32)
        .filter(|r| *r > 0.0)
        .unwrap_or_else(|| unlimited_range(units));
    let kind = match light["type"].as_str()? {
        "directional" => ModelLightKind::Sun { color, intensity },
        "point" => ModelLightKind::Point(PointLight3D {
            color,
            intensity: units,
            range,
            ..Default::default()
        }),
        "spot" => {
            let spot = &light["spot"];
            // ponko's cones stop short of a half angle of 90°.
            let outer = spot["outerConeAngle"]
                .as_f64()
                .map_or(std::f32::consts::FRAC_PI_4, |a| a as f32)
                .clamp(0.0, std::f32::consts::FRAC_PI_2 - 1e-3);
            let inner = spot["innerConeAngle"]
                .as_f64()
                .map_or(0.0, |a| a as f32)
                .clamp(0.0, outer);
            ModelLightKind::Spot(SpotLight3D {
                color,
                intensity: units,
                range,
                inner_angle: inner,
                outer_angle: outer,
                ..Default::default()
            })
        }
        _ => return None,
    };
    let name = light["name"].as_str().or(node_name).unwrap_or("").to_string();
    Some(ModelLight {
        name,
        transform: transform_of(world),
        kind,
        joint: None,
    })
}

/// Node `node`'s camera placed by `world`, if it has one.
pub(super) fn camera_of(node: &gltf::Node, world: Mat4) -> Option<ModelCamera> {
    let camera = node.camera()?;
    let projection = match camera.projection() {
        gltf::camera::Projection::Perspective(p) => ModelProjection::Perspective {
            fov_y: p.yfov(),
            aspect: p.aspect_ratio(),
            near: p.znear(),
            far: p.zfar(),
        },
        gltf::camera::Projection::Orthographic(o) => ModelProjection::Orthographic {
            half_width: o.xmag(),
            half_height: o.ymag(),
            near: o.znear(),
            far: o.zfar(),
        },
    };
    let name = camera.name().or(node.name()).unwrap_or("").to_string();
    Some(ModelCamera {
        name,
        transform: transform_of(world),
        projection,
    })
}

#[cfg(test)]
mod tests {
    use super::super::gltf::{Loaded, load};
    use super::*;
    use serde_json::json;

    /// Builds a glTF file's buffer, views and accessors from values.
    #[derive(Default)]
    struct File {
        bin: Vec<u8>,
        views: Vec<Value>,
        accessors: Vec<Value>,
    }

    impl File {
        /// Adds `bytes` as a buffer view (with `stride` for vertex data).
        fn view(&mut self, bytes: &[u8], stride: Option<usize>) -> usize {
            self.bin.resize(self.bin.len().next_multiple_of(4), 0);
            let mut view = json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
            if let Some(stride) = stride {
                view["byteStride"] = json!(stride);
            }
            self.bin.extend_from_slice(bytes);
            self.views.push(view);
            self.views.len() - 1
        }

        /// Adds an accessor; `extra` is merged into it (min, max, sparse).
        fn accessor(
            &mut self,
            view: Option<usize>,
            component: u32,
            normalized: bool,
            count: usize,
            ty: &str,
            extra: Value,
        ) -> usize {
            let mut a = json!({ "componentType": component, "normalized": normalized, "count": count, "type": ty });
            if let Some(view) = view {
                a["bufferView"] = json!(view);
            }
            for (k, v) in extra.as_object().cloned().unwrap_or_default() {
                a[k] = v;
            }
            self.accessors.push(a);
            self.accessors.len() - 1
        }

        /// Floats as a view and a VEC`n` accessor.
        fn floats(&mut self, values: &[f32], n: usize) -> usize {
            let bytes: Vec<u8> = values.iter().flat_map(|f| f.to_le_bytes()).collect();
            let view = self.view(&bytes, None);
            let ty = ["", "SCALAR", "VEC2", "VEC3", "VEC4"][n];
            let extra = if n == 3 { bounds(values) } else { json!({}) };
            self.accessor(Some(view), 5126, false, values.len() / n, ty, extra)
        }

        /// The file's JSON: `doc` with this file's buffer, views and accessors.
        fn gltf(&self, mut doc: Value) -> String {
            let b64 = to_base64(&self.bin);
            doc["asset"] = json!({ "version": "2.0" });
            doc["buffers"] =
                json!([{ "byteLength": self.bin.len(), "uri": format!("data:application/octet-stream;base64,{b64}") }]);
            doc["bufferViews"] = json!(self.views);
            doc["accessors"] = json!(self.accessors);
            doc.to_string()
        }
    }

    /// Min and max of VEC3 floats, which glTF needs on positions.
    fn bounds(values: &[f32]) -> Value {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in values.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        json!({ "min": lo, "max": hi })
    }

    fn to_base64(bytes: &[u8]) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(alphabet[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        while !out.len().is_multiple_of(4) {
            out.push('=');
        }
        out
    }

    fn load_str(name: &str, text: &str) -> Result<Loaded, String> {
        load(name, text.as_bytes(), &mut |f| Err(format!("no {f}")), &|_| false)
    }

    /// A PNG of one pixel, as a data URI.
    fn png(pixel: [u8; 4]) -> String {
        let image = image::RgbaImage::from_fn(1, 1, |_, _| image::Rgba(pixel));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        format!("data:image/png;base64,{}", to_base64(&bytes.into_inner()))
    }

    const TRIANGLE: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    /// One triangle on node 0 with `material`, and whatever `extra` adds.
    fn triangle(file: &mut File, uvs: Option<&[f32]>, extra: Value) -> String {
        let position = file.floats(&TRIANGLE, 3);
        let mut attributes = json!({ "POSITION": position });
        if let Some(uvs) = uvs {
            attributes["TEXCOORD_0"] = json!(file.floats(uvs, 2));
            let second: Vec<f32> = uvs.iter().map(|v| v * 10.0).collect();
            attributes["TEXCOORD_1"] = json!(file.floats(&second, 2));
        }
        let mut doc = json!({
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{ "primitives": [{ "attributes": attributes, "material": 0 }] }],
            "materials": [{}],
        });
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            doc[k] = v;
        }
        file.gltf(doc)
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn a_mesh_cooked_with_a_range_per_primitive_folds_back_whole_in_its_order() {
        let mut file = File::default();
        // Primitive 1 of the source, quantized: steps of 1 mm from x = 10.
        let steps: Vec<u8> = [[0u16, 0, 0], [1000, 0, 0], [0, 1000, 0]]
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()).chain([0, 0]))
            .collect();
        let view = file.view(&steps, Some(8));
        let quantized = file.accessor(
            Some(view),
            5123,
            false,
            3,
            "VEC3",
            json!({ "min": [0, 0, 0], "max": [1000, 1000, 0] }),
        );
        // Primitive 0, past the bound: floats, under a child with no transform.
        let floats = file.floats(&[-50.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0, 0.0, 50.0], 3);
        let text = file.gltf(json!({
            "extensionsUsed": ["KHR_mesh_quantization", QUANTIZED_POSITIONS],
            "extensionsRequired": ["KHR_mesh_quantization"],
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [
                { "name": "level", "children": [1, 2], "extras": { "area": 1 } },
                { "mesh": 0, "translation": [10.0, 0.0, 0.0], "scale": [0.001, 0.001, 0.001],
                  "extensions": { QUANTIZED_POSITIONS: { "mesh": 0, "primitives": [1] } } },
                { "mesh": 1, "extensions": { QUANTIZED_POSITIONS: { "mesh": 0, "primitives": [0] } } }
            ],
            "materials": [{ "name": "far" }, { "name": "near" }],
            "meshes": [
                { "name": "level", "primitives": [{ "attributes": { "POSITION": quantized }, "material": 1 }] },
                { "name": "level", "primitives": [{ "attributes": { "POSITION": floats }, "material": 0 }] }
            ],
        }));
        let mut json: Value = serde_json::from_str(&text).unwrap();
        let matrices = fold_quantized_positions(&mut json);
        assert_eq!(json["nodes"][0]["mesh"], 0, "the level shows its mesh again");
        assert!(json["nodes"][0].get("children").is_none(), "{}", json["nodes"][0]);
        let order: Vec<&Value> = json["meshes"][0]["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| &p["material"])
            .collect();
        assert_eq!(order, [&json!(0), &json!(1)], "the source's order");
        assert_eq!(matrices[0].len(), 2);
        assert_eq!(matrices[0][0], Mat4::IDENTITY);
        assert!(
            (matrices[0][1].transform_point3(Vec3::new(1000.0, 0.0, 0.0)) - Vec3::new(11.0, 0.0, 0.0)).length() < 1e-5
        );
        assert!(matrices[1].is_empty());

        let loaded = load_str("level.gltf", &text).unwrap();
        let p = &loaded.data.positions;
        assert_eq!(p.len(), 6);
        assert!(
            p.iter().any(|v| (*v - Vec3::new(11.0, 0.0, 0.0)).length() < 1e-5),
            "{p:?}"
        );
        assert!(
            p.iter().any(|v| (*v - Vec3::new(50.0, 0.0, 0.0)).length() < 1e-5),
            "{p:?}"
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn pieces_that_dont_cover_the_mesh_once_are_read_as_they_are() {
        let mut file = File::default();
        let a = file.floats(&TRIANGLE, 3);
        let b = file.floats(&TRIANGLE, 3);
        // Both pieces claim primitive 0: nothing is folded, the file draws as it is.
        let text = file.gltf(json!({
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [
                { "children": [1, 2] },
                { "mesh": 0, "extensions": { QUANTIZED_POSITIONS: { "mesh": 0, "primitives": [0] } } },
                { "mesh": 1, "extensions": { QUANTIZED_POSITIONS: { "mesh": 0, "primitives": [0] } } }
            ],
            "meshes": [
                { "primitives": [{ "attributes": { "POSITION": a } }] },
                { "primitives": [{ "attributes": { "POSITION": b } }] }
            ],
        }));
        let mut json: Value = serde_json::from_str(&text).unwrap();
        let matrices = fold_quantized_positions(&mut json);
        assert!(matrices.iter().all(Vec::is_empty));
        assert!(json["nodes"][0].get("mesh").is_none());
        assert_eq!(json["nodes"][1]["mesh"], 0);
    }

    #[test]
    fn quantized_positions_normals_and_texture_coordinates_read_as_floats() {
        let mut file = File::default();
        // Positions as unsigned shorts scaled down by the node, as gltfpack
        // writes them; a stride of 8 keeps each one 4-byte aligned.
        let positions: Vec<u8> = [[0u16, 0, 0], [1000, 0, 0], [0, 1000, 0]]
            .iter()
            .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()).chain([0, 0]))
            .collect();
        let view = file.view(&positions, Some(8));
        let position = file.accessor(
            Some(view),
            5123,
            false,
            3,
            "VEC3",
            json!({ "min": [0, 0, 0], "max": [1000, 1000, 0] }),
        );
        // Normals as normalized bytes (not unit length: 127 rounds).
        let normals: Vec<u8> = [[0i8, 0, 127, 0]; 3].iter().flatten().map(|b| *b as u8).collect();
        let view = file.view(&normals, Some(4));
        let normal = file.accessor(Some(view), 5120, true, 3, "VEC3", json!({}));
        // Texture coordinates as normalized unsigned shorts.
        let uvs: Vec<u8> = [[0u16, 0], [65535, 0], [0, 32768]]
            .iter()
            .flatten()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let view = file.view(&uvs, None);
        let uv = file.accessor(Some(view), 5123, true, 3, "VEC2", json!({}));
        let text = file.gltf(json!({
            "extensionsUsed": ["KHR_mesh_quantization"],
            "extensionsRequired": ["KHR_mesh_quantization"],
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "scale": [0.001, 0.001, 0.001] }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv } }] }],
        }));
        let loaded = load_str("quantized.gltf", &text).unwrap();
        let p = &loaded.data.positions;
        assert!(
            (p[1] - Vec3::X).length() < 1e-6 && (p[2] - Vec3::Y).length() < 1e-6,
            "{p:?}"
        );
        assert!(loaded.data.normals.iter().all(|n| (*n - Vec3::Z).length() < 1e-6));
        let uv = &loaded.data.uvs;
        assert!(
            close(uv[1], Vec2::new(1.0, 0.0)) && (uv[2].y - 32768.0 / 65535.0).abs() < 1e-6,
            "{uv:?}"
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn quantized_sparse_morph_targets_fill_in_their_offsets() {
        let mut file = File::default();
        let position = file.floats(&TRIANGLE, 3);
        // Target offsets: zero but for corner 2, moved up by a normalized
        // short of half (sparse: one index, one value).
        let indices = file.view(&[2u8], None);
        let values: Vec<u8> = [0i16, 16384, 0].iter().flat_map(|c| c.to_le_bytes()).collect();
        let values = file.view(&values, None);
        let target = file.accessor(
            None,
            5122,
            true,
            3,
            "VEC3",
            json!({ "sparse": { "count": 1,
                "indices": { "bufferView": indices, "componentType": 5121 },
                "values": { "bufferView": values } } }),
        );
        let text = file.gltf(json!({
            "extensionsUsed": ["KHR_mesh_quantization"],
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position }, "targets": [{ "POSITION": target }] }],
                         "extras": { "targetNames": ["lift"] } }],
        }));
        let loaded = load_str("sparse.gltf", &text).unwrap();
        let lift = &loaded.data.morph_targets[0];
        assert_eq!(lift.name, "lift");
        assert_eq!(lift.positions[0], Vec3::ZERO);
        assert!((lift.positions[2] - Vec3::new(0.0, 16384.0 / 32767.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn texture_transforms_move_the_texture_coordinates() {
        let mut file = File::default();
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let transform = json!({ "offset": [0.5, 0.25], "rotation": std::f32::consts::FRAC_PI_2, "scale": [2.0, 3.0] });
        let text = triangle(
            &mut file,
            Some(&uvs),
            json!({
                "extensionsUsed": ["KHR_texture_transform"],
                "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": {
                    "index": 0, "extensions": { "KHR_texture_transform": transform } } } }],
                "textures": [{ "source": 0 }],
                "images": [{ "uri": png([255, 255, 255, 255]) }],
            }),
        );
        let loaded = load_str("moved.gltf", &text).unwrap();
        // The extension's order: scale, then rotate (counter-clockwise by
        // its matrix [cos sin; -sin cos]), then offset.
        let expect = |u: f32, v: f32| {
            let (s, c) = std::f32::consts::FRAC_PI_2.sin_cos();
            let (u, v) = (u * 2.0, v * 3.0);
            Vec2::new(c * u + s * v + 0.5, -s * u + c * v + 0.25)
        };
        // The coordinates load as the file has them; the material moves
        // them as it draws.
        assert!(close(loaded.data.uvs[1], Vec2::new(1.0, 0.0)), "{:?}", loaded.data.uvs);
        let transform = loaded.materials[0].texture_transform;
        let uv: Vec<Vec2> = loaded.data.uvs.iter().map(|uv| transform.apply(*uv)).collect();
        assert!(
            close(uv[0], expect(0.0, 0.0)) && close(uv[1], expect(1.0, 0.0)) && close(uv[2], expect(0.0, 1.0)),
            "{uv:?}"
        );
        assert!(close(uv[1], Vec2::new(0.5, -1.75)), "{uv:?}");
        // The second set, for shaders, is left as it is.
        assert!(close(loaded.data.uvs1[1], Vec2::new(10.0, 0.0)));
    }

    #[test]
    fn a_texture_transform_can_pick_the_second_coordinate_set() {
        let mut file = File::default();
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let text = triangle(
            &mut file,
            Some(&uvs),
            json!({
                "materials": [{ "emissiveTexture": { "index": 0, "extensions": { "KHR_texture_transform": {
                    "texCoord": 1, "scale": [0.5, 0.5] } } } }],
                "textures": [{ "source": 0 }],
                "images": [{ "uri": png([255, 255, 255, 255]) }],
            }),
        );
        let loaded = load_str("second.gltf", &text).unwrap();
        // TEXCOORD_1 is ten times TEXCOORD_0, halved by the material.
        assert!(close(loaded.data.uvs[1], Vec2::new(10.0, 0.0)), "{:?}", loaded.data.uvs);
        let transform = loaded.materials[0].texture_transform;
        assert!(close(transform.apply(loaded.data.uvs[1]), Vec2::new(5.0, 0.0)));
    }

    #[test]
    fn textures_asking_for_different_transforms_follow_the_base_color_and_warn() {
        let mut file = File::default();
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let text = triangle(
            &mut file,
            Some(&uvs),
            json!({
                "materials": [{ "name": "brick",
                    "pbrMetallicRoughness": { "baseColorTexture": { "index": 0,
                        "extensions": { "KHR_texture_transform": { "scale": [4.0, 4.0] } } } },
                    "normalTexture": { "index": 0 } }],
                "textures": [{ "source": 0 }],
                "images": [{ "uri": png([255, 255, 255, 255]) }],
            }),
        );
        let loaded = load_str("mixed.gltf", &text).unwrap();
        let transform = loaded.materials[0].texture_transform;
        assert!(close(transform.apply(loaded.data.uvs[1]), Vec2::new(4.0, 0.0)));
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("\"brick\"") && w.contains("transforms")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn emissive_strength_multiplies_the_emissive_color() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_materials_emissive_strength"],
                "materials": [{ "emissiveFactor": [1.0, 0.5, 0.0],
                    "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 4.0 } } }],
            }),
        );
        let loaded = load_str("bright.gltf", &text).unwrap();
        let [r, g, b, _] = super::super::linear(loaded.materials[0].emissive);
        assert!(
            (r - 4.0).abs() < 1e-3 && (g - 2.0).abs() < 1e-3 && b == 0.0,
            "{r} {g} {b}"
        );
    }

    #[test]
    fn transmission_draws_as_see_through_blending() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_materials_transmission", "KHR_materials_volume", "KHR_materials_ior"],
                "materials": [{ "extensions": { "KHR_materials_transmission": { "transmissionFactor": 0.75 },
                                                "KHR_materials_volume": { "thicknessFactor": 0.1 },
                                                "KHR_materials_ior": { "ior": 1.33 } } }],
            }),
        );
        let loaded = load_str("glass.gltf", &text).unwrap();
        let m = &loaded.materials[0];
        assert_eq!(m.blend, super::super::Blend::Transparent);
        assert!((m.color.a - 0.25).abs() < 1e-6);
        assert_eq!(m.layers.ior, 1.33, "the index of refraction sets its reflection");
        // Refraction isn't drawn: the warning says so and names the volume.
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("KHR_materials_volume") && w.contains("refraction")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn a_required_approximated_extension_loads_with_a_warning() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({ "extensionsUsed": ["KHR_materials_volume"], "extensionsRequired": ["KHR_materials_volume"] }),
        );
        let loaded = load_str("thick.gltf", &text).unwrap();
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("approximates") && w.contains("KHR_materials_volume")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn material_extensions_are_read_as_layers() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_materials_clearcoat", "KHR_materials_sheen", "KHR_materials_specular",
                                   "KHR_materials_ior", "KHR_materials_anisotropy", "KHR_materials_iridescence"],
                "extensionsRequired": ["KHR_materials_clearcoat", "KHR_materials_sheen", "KHR_materials_specular",
                                       "KHR_materials_ior", "KHR_materials_anisotropy", "KHR_materials_iridescence"],
                "materials": [{ "extensions": {
                    "KHR_materials_clearcoat": { "clearcoatFactor": 0.8, "clearcoatRoughnessFactor": 0.2 },
                    "KHR_materials_sheen": { "sheenColorFactor": [1.0, 0.0, 0.0], "sheenRoughnessFactor": 0.5 },
                    "KHR_materials_specular": { "specularFactor": 0.5, "specularColorFactor": [1.0, 1.0, 0.0] },
                    "KHR_materials_ior": { "ior": 1.8 },
                    "KHR_materials_anisotropy": { "anisotropyStrength": 2.0, "anisotropyRotation": 0.5 },
                    "KHR_materials_iridescence": { "iridescenceFactor": 1.0, "iridescenceIor": 1.5,
                                                   "iridescenceThicknessMaximum": 800 } } }],
            }),
        );
        let loaded = load_str("layered.gltf", &text).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        let m = &loaded.materials[0];
        assert!(!m.unlit);
        let l = m.layers;
        assert_eq!((l.clearcoat, l.clearcoat_roughness), (0.8, 0.2));
        assert_eq!((l.sheen_color, l.sheen_roughness), (Color::rgb(1.0, 0.0, 0.0), 0.5));
        assert_eq!(
            (l.specular, l.specular_color, l.ior),
            (0.5, Color::rgb(1.0, 1.0, 0.0), 1.8)
        );
        assert_eq!(
            (l.anisotropy, l.anisotropy_rotation),
            (1.0, 0.5),
            "strength clamped to 1"
        );
        assert_eq!(
            (l.iridescence, l.iridescence_ior, l.iridescence_thickness),
            (1.0, 1.5, [100.0, 800.0])
        );
        assert!(l.problem().is_none());
        // Without the extensions: no layers, nothing extra to draw.
        let plain = load_str("plain.gltf", &triangle(&mut File::default(), None, json!({}))).unwrap();
        assert!(!plain.materials[0].layers.is_on() && !plain.materials[0].unlit);
    }

    #[test]
    fn unlit_materials_are_read() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({ "extensionsUsed": ["KHR_materials_unlit"], "extensionsRequired": ["KHR_materials_unlit"],
                    "materials": [{ "extensions": { "KHR_materials_unlit": {} } }] }),
        );
        let loaded = load_str("flat.gltf", &text).unwrap();
        assert!(loaded.materials[0].unlit);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn layer_maps_read_their_images_and_at_most_two_of_them() {
        let mut file = File::default();
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let text = triangle(
            &mut file,
            Some(&uvs),
            json!({
                "materials": [{ "name": "busy", "extensions": {
                    "KHR_materials_clearcoat": { "clearcoatFactor": 1.0, "clearcoatTexture": { "index": 0 },
                                                 "clearcoatRoughnessTexture": { "index": 0 } },
                    "KHR_materials_sheen": { "sheenColorFactor": [1.0, 1.0, 1.0], "sheenColorTexture": { "index": 1 } },
                    "KHR_materials_iridescence": { "iridescenceFactor": 1.0, "iridescenceTexture": { "index": 2 } } } }],
                "textures": [{ "source": 0 }, { "source": 1 }, { "source": 2 }],
                "images": [{ "uri": png([255, 0, 0, 255]) }, { "uri": png([0, 255, 0, 255]) },
                           { "uri": png([0, 0, 255, 255]) }],
            }),
        );
        let loaded = load_str("busy.gltf", &text).unwrap();
        let maps = loaded.materials[0].layer_maps;
        assert!(
            maps[0].is_some() && maps[0] == maps[1],
            "one image for both clear coat maps"
        );
        assert!(maps[3].is_some() && maps[3] != maps[0], "the sheen's own image");
        assert_eq!(maps[8], None, "a third image is left out");
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("\"busy\"") && w.contains("3 images")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn hidden_nodes_and_everything_under_them_are_left_out() {
        let mut file = File::default();
        let position = file.floats(&TRIANGLE, 3);
        let text = file.gltf(json!({
            "extensionsUsed": ["KHR_node_visibility", "KHR_lights_punctual"],
            "extensionsRequired": ["KHR_node_visibility"],
            "extensions": { "KHR_lights_punctual": { "lights": [{ "type": "point", "intensity": 10.0 }] } },
            "scene": 0,
            "scenes": [{ "nodes": [0, 1] }],
            "nodes": [
                { "mesh": 0 },
                { "mesh": 0, "translation": [5.0, 0.0, 0.0], "children": [2],
                  "extensions": { "KHR_node_visibility": { "visible": false } } },
                { "mesh": 0, "extensions": { "KHR_lights_punctual": { "light": 0 },
                                             "KHR_node_visibility": { "visible": true } } }
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position } }] }],
        }));
        let loaded = load_str("hidden.gltf", &text).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.data.positions.len(), 3, "only the visible node's triangle");
        assert!(loaded.data.positions.iter().all(|p| p.x < 2.0));
        assert!(
            loaded.lights.is_empty(),
            "the light under the hidden node is hidden too"
        );
        // Negative control: the same file without the hiding draws all three.
        let shown = text.replace("\"visible\":false", "\"visible\":true");
        assert_ne!(shown, text);
        let loaded = load_str("shown.gltf", &shown).unwrap();
        assert_eq!(loaded.data.positions.len(), 9);
        assert_eq!(loaded.lights.len(), 1);
    }

    #[test]
    fn specular_glossiness_materials_show_their_diffuse_as_base_color() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_materials_pbrSpecularGlossiness"],
                "extensionsRequired": ["KHR_materials_pbrSpecularGlossiness"],
                "materials": [{ "extensions": { "KHR_materials_pbrSpecularGlossiness": {
                    "diffuseFactor": [0.5, 0.25, 1.0, 1.0], "glossinessFactor": 0.8,
                    "specularFactor": [0.1, 0.1, 0.1] } } }],
            }),
        );
        let loaded = load_str("old.gltf", &text).unwrap();
        let m = &loaded.materials[0];
        let [r, g, b, a] = super::super::linear(m.color);
        assert!(
            (r - 0.5).abs() < 1e-3 && (g - 0.25).abs() < 1e-3 && (b - 1.0).abs() < 1e-3 && a == 1.0,
            "{r} {g} {b} {a}"
        );
        assert!((m.roughness - 0.2).abs() < 1e-5, "{}", m.roughness);
        assert_eq!(m.metallic, 0.0);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("old.gltf") && w.contains("approximates")),
            "{:?}",
            loaded.warnings
        );
        // A material with a metallic-roughness block of its own keeps it.
        let mut both = serde_json::from_str::<Value>(&text).unwrap();
        both["materials"][0]["pbrMetallicRoughness"] =
            json!({ "baseColorFactor": [0, 1, 0, 1], "roughnessFactor": 0.5 });
        let loaded = load_str("both.gltf", &both.to_string()).unwrap();
        assert_eq!(loaded.materials[0].roughness, 0.5);
        let [r, g, _, _] = super::super::linear(loaded.materials[0].color);
        assert!(r.abs() < 1e-3 && (g - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_dds_image_is_the_preferred_image_of_its_texture() {
        let mut json = json!({
            "textures": [
                { "source": 0, "extensions": { "MSFT_texture_dds": { "source": 1 } } },
                { "source": 2 }
            ]
        });
        let preferred = texture_sources("dds.gltf", &mut json).unwrap();
        if cfg!(feature = "compressed-textures") {
            assert_eq!(preferred, [Some(1), None]);
        } else {
            assert_eq!(preferred, [None, None], "without the feature the core image loads");
        }
    }

    #[test]
    fn animation_pointer_channels_load_beside_node_channels() {
        let mut file = File::default();
        // One key: at time 0, the node at the origin and the material red.
        let times = file.floats(&[0.0], 1);
        let places = file.floats(&[0.0, 0.0, 0.0], 3);
        let red = file.floats(&[1.0, 0.0, 0.0, 1.0], 4);
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_animation_pointer"],
                "animations": [{
                    "name": "pulse",
                    "samplers": [{ "input": times, "output": places }, { "input": times, "output": red }],
                    "channels": [
                        { "sampler": 0, "target": { "node": 0, "path": "translation" } },
                        { "sampler": 1, "target": { "path": "pointer", "extensions": { "KHR_animation_pointer":
                            { "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor" } } } }
                    ]
                }],
            }),
        );
        let value: Value = serde_json::from_str(&text).unwrap();
        let mut channels = value.clone();
        drop_channels_without_nodes(&mut channels);
        assert_eq!(
            channels["animations"][0]["channels"].as_array().map(Vec::len),
            Some(1),
            "only the node's channel goes to the gltf crate"
        );
        assert_eq!(
            pointer_channels(&value),
            vec![PointerChannel {
                animation: 0,
                pointer: "/materials/0/pbrMetallicRoughness/baseColorFactor".into(),
                input: times,
                output: red,
                interpolation: crate::three::Interpolation::Linear,
            }]
        );
        // The whole file loads (the gltf crate refuses a target without a
        // node), with both channels and no warning.
        let loaded = load_str("pointer.gltf", &value.to_string()).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.clips[0].tracks().len(), 1);
        let (m, property, track) = &loaded.clip_materials[0][0];
        assert_eq!((*m, *property), (0, crate::three::MaterialProperty::Color));
        assert_eq!(track.values(), &[glam::Vec4::new(1.0, 0.0, 0.0, 1.0)]);
    }

    #[test]
    fn pointers_resolve_to_what_the_engine_plays_or_say_why_not() {
        use crate::three::MaterialProperty as P;
        let json = json!({
            "nodes": [{}, {}],
            "materials": [
                { "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } }, "normalTexture": { "index": 0 } },
                { "occlusionTexture": { "index": 0 } },
                {},
            ],
        });
        let ok = |pointer: &str| pointer_target(&json, pointer).unwrap();
        assert_eq!(
            ok("/nodes/1/translation"),
            PointerTarget::Node(1, NodeValue::Translation)
        );
        assert_eq!(ok("/nodes/0/rotation"), PointerTarget::Node(0, NodeValue::Rotation));
        assert_eq!(ok("/nodes/0/scale"), PointerTarget::Node(0, NodeValue::Scale));
        assert_eq!(
            ok("/nodes/1/extensions/KHR_node_visibility/visible"),
            PointerTarget::Node(1, NodeValue::Visible)
        );
        for (pointer, property) in [
            ("pbrMetallicRoughness/baseColorFactor", P::Color),
            ("pbrMetallicRoughness/metallicFactor", P::Metallic),
            ("pbrMetallicRoughness/roughnessFactor", P::Roughness),
            ("emissiveFactor", P::Emissive),
            ("alphaCutoff", P::AlphaCutoff),
            ("normalTexture/scale", P::NormalStrength),
            (
                "extensions/KHR_materials_emissive_strength/emissiveStrength",
                P::EmissiveStrength,
            ),
            ("extensions/KHR_materials_clearcoat/clearcoatFactor", P::Clearcoat),
            (
                "extensions/KHR_materials_clearcoat/clearcoatRoughnessFactor",
                P::ClearcoatRoughness,
            ),
            ("extensions/KHR_materials_sheen/sheenColorFactor", P::SheenColor),
            ("extensions/KHR_materials_sheen/sheenRoughnessFactor", P::SheenRoughness),
            ("extensions/KHR_materials_specular/specularFactor", P::Specular),
            (
                "extensions/KHR_materials_specular/specularColorFactor",
                P::SpecularColor,
            ),
            ("extensions/KHR_materials_ior/ior", P::Ior),
            ("extensions/KHR_materials_anisotropy/anisotropyStrength", P::Anisotropy),
            (
                "extensions/KHR_materials_anisotropy/anisotropyRotation",
                P::AnisotropyRotation,
            ),
            ("extensions/KHR_materials_iridescence/iridescenceFactor", P::Iridescence),
            ("extensions/KHR_materials_iridescence/iridescenceIor", P::IridescenceIor),
            (
                "extensions/KHR_materials_iridescence/iridescenceThicknessMinimum",
                P::IridescenceThinnest,
            ),
            (
                "extensions/KHR_materials_iridescence/iridescenceThicknessMaximum",
                P::IridescenceThickest,
            ),
            (
                "pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/offset",
                P::TextureOffset,
            ),
            (
                "pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/rotation",
                P::TextureRotation,
            ),
            (
                "pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/scale",
                P::TextureScale,
            ),
        ] {
            assert_eq!(
                ok(&format!("/materials/0/{pointer}")),
                PointerTarget::Material(0, property),
                "{pointer}"
            );
        }
        // A material whose only texture is its occlusion follows that one.
        assert_eq!(
            ok("/materials/1/occlusionTexture/extensions/KHR_texture_transform/scale"),
            PointerTarget::Material(1, P::TextureScale)
        );
        assert_eq!(ok("/materials/0/emissiveFactor").components(), 3);
        assert_eq!(ok("/materials/0/pbrMetallicRoughness/baseColorFactor").components(), 4);
        // What isn't played, and why.
        let why = |pointer: &str| pointer_target(&json, pointer).unwrap_err();
        assert!(why("/materials/0/normalTexture/extensions/KHR_texture_transform/offset").contains("first texture"));
        assert!(
            why("/materials/2/extensions/KHR_materials_transmission/transmissionTexture/extensions/KHR_texture_transform/offset")
                .contains("isn't read")
        );
        assert!(why("/materials/0/extensions/KHR_materials_transmission/transmissionFactor").contains("material"));
        assert!(why("/nodes/0/weights").contains("morph"));
        assert!(why("/nodes/2/translation").contains("past"));
        assert!(why("/nodes/x/translation").contains("number"));
        assert!(why("/cameras/0/perspective/yfov").contains("only nodes and materials"));
    }

    #[test]
    fn pointer_channels_become_visibility_and_material_tracks() {
        let mut file = File::default();
        let times = file.floats(&[0.0, 1.0], 1);
        // Visibility as glTF has it: unsigned bytes, not normalized.
        let flags = file.view(&[1, 0], None);
        let flags = file.accessor(Some(flags), 5121, false, 2, "SCALAR", json!({}));
        let offsets = file.floats(&[0.0, 0.0, 0.5, 0.25], 2);
        let strength = file.floats(&[1.0, 4.0], 1);
        let three = file.floats(&[0.0, 1.0, 2.0], 1);
        let position = file.floats(&TRIANGLE, 3);
        let uv = file.floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0], 2);
        let pointer =
            |p: &str| json!({ "path": "pointer", "extensions": { "KHR_animation_pointer": { "pointer": p } } });
        let doc = json!({
            "extensionsUsed": ["KHR_animation_pointer", "KHR_node_visibility", "KHR_lights_punctual"],
            "extensions": { "KHR_lights_punctual": { "lights": [{ "type": "point" }] } },
            "scene": 0,
            "scenes": [{ "nodes": [0, 1, 2] }],
            "nodes": [
                { "mesh": 0, "name": "shown" },
                // Hidden at rest, shown by the animation: it loads.
                { "mesh": 0, "name": "blinking", "translation": [5.0, 0.0, 0.0],
                  "extensions": { "KHR_node_visibility": { "visible": false }, "KHR_lights_punctual": { "light": 0 } } },
                // Hidden for good: it doesn't.
                { "mesh": 0, "name": "gone", "translation": [9.0, 0.0, 0.0],
                  "extensions": { "KHR_node_visibility": { "visible": false } } },
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position, "TEXCOORD_0": uv }, "material": 0 }] }],
            "materials": [{
                "emissiveFactor": [1.0, 0.5, 0.0],
                "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                "extensions": { "KHR_materials_emissive_strength": { "emissiveStrength": 2.0 } },
            }],
            "textures": [{ "source": 0 }],
            "images": [{ "uri": png([255, 255, 255, 255]) }],
            "animations": [{
                "name": "blink",
                "samplers": [
                    { "input": times, "output": flags, "interpolation": "STEP" },
                    { "input": times, "output": offsets },
                    { "input": times, "output": strength },
                    { "input": times, "output": three },
                ],
                "channels": [
                    { "sampler": 0, "target": pointer("/nodes/1/extensions/KHR_node_visibility/visible") },
                    { "sampler": 1, "target": pointer("/materials/0/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/offset") },
                    { "sampler": 2, "target": pointer("/materials/0/extensions/KHR_materials_emissive_strength/emissiveStrength") },
                    // Not played: a camera's field of view; and keys of the wrong size.
                    { "sampler": 2, "target": pointer("/cameras/0/perspective/yfov") },
                    { "sampler": 3, "target": pointer("/materials/0/emissiveFactor") },
                ]
            }],
        });
        let loaded = load_str("blink.gltf", &file.gltf(doc)).unwrap();
        // Two of the three nodes' triangles: the animated one loads hidden.
        assert_eq!(loaded.data.positions.len(), 6, "{:?}", loaded.data.positions);
        let skin = loaded.skin.as_ref().unwrap();
        let blinking = skin.joints.iter().position(|j| j.name == "blinking").unwrap();
        assert!(!skin.joints[blinking].visible && skin.joints[0].visible);
        assert!(!skin.joints.iter().find(|j| j.name == "gone").unwrap().visible);
        // Its light hangs from its joint.
        assert_eq!(loaded.lights.len(), 1);
        assert_eq!(loaded.lights[0].joint, Some(blinking));
        // The visibility is a property track of the clip, 1 then 0.
        let clip = &loaded.clips[0];
        let visible = &clip.property_tracks()[0];
        assert_eq!(visible.target, crate::three::AnimatedProperty::JointVisible(blinking));
        assert_eq!(visible.track.values(), &[glam::Vec4::X, glam::Vec4::ZERO]);
        assert_eq!(visible.track.interpolation(), crate::three::Interpolation::Step);
        assert_eq!(clip.duration(), 1.0);
        // The material values wait for the materials to be made.
        use crate::three::MaterialProperty as P;
        let animated: Vec<_> = loaded.clip_materials[0].iter().map(|(m, p, _)| (*m, *p)).collect();
        assert_eq!(animated, vec![(0, P::TextureOffset), (0, P::EmissiveStrength)]);
        assert_eq!(
            loaded.clip_materials[0][0].2.values()[1],
            glam::Vec4::new(0.5, 0.25, 0.0, 0.0)
        );
        let (m, factors) = loaded.material_factors[0];
        assert_eq!(
            (m, factors.emissive, factors.emissive_strength),
            (0, [1.0, 0.5, 0.0], 2.0)
        );
        // What isn't played says so.
        for why in ["yfov", "emissiveFactor"] {
            assert!(
                loaded
                    .warnings
                    .iter()
                    .any(|w| w.contains("blink.gltf") && w.contains(why)),
                "{why}: {:?}",
                loaded.warnings
            );
        }
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
    }

    #[test]
    fn a_file_without_animations_leaves_hidden_nodes_out() {
        let mut file = File::default();
        let position = file.floats(&TRIANGLE, 3);
        let doc = json!({
            "scene": 0,
            "scenes": [{ "nodes": [0, 1] }],
            "nodes": [
                { "mesh": 0 },
                { "mesh": 0, "extensions": { "KHR_node_visibility": { "visible": false } } },
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position } }] }],
        });
        let loaded = load_str("still.gltf", &file.gltf(doc)).unwrap();
        assert_eq!(loaded.data.positions.len(), 3);
        assert!(loaded.skin.is_none() && loaded.clips.is_empty());
    }

    #[test]
    fn each_gpu_instance_draws_the_mesh_with_its_own_transform() {
        let mut file = File::default();
        let places = file.floats(&[0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 5.0, 0.0], 3);
        let sizes = file.floats(&[1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0], 3);
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": [INSTANCING],
                "extensionsRequired": [INSTANCING],
                "nodes": [{ "mesh": 0, "translation": [0.0, 0.0, 10.0],
                    "extensions": { INSTANCING: { "attributes": { "TRANSLATION": places, "SCALE": sizes } } } }],
            }),
        );
        let loaded = load_str("instances.gltf", &text).unwrap();
        let p = &loaded.data.positions;
        assert_eq!(p.len(), 9, "three instances of a triangle: {p:?}");
        for corner in [
            Vec3::new(1.0, 0.0, 10.0),
            Vec3::new(7.0, 0.0, 10.0),
            Vec3::new(5.0, 2.0, 10.0),
            Vec3::new(0.0, 6.0, 10.0),
        ] {
            assert!(
                p.iter().any(|q| (*q - corner).length() < 1e-5),
                "no corner at {corner}: {p:?}"
            );
        }
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn unreadable_instances_draw_the_mesh_once_and_warn() {
        let mut file = File::default();
        // A scale that isn't a VEC3: the extension doesn't allow it.
        let sizes = file.floats(&[1.0, 2.0], 1);
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": [INSTANCING],
                "nodes": [{ "mesh": 0, "extensions": { INSTANCING: { "attributes": { "SCALE": sizes } } } }],
            }),
        );
        let loaded = load_str("instances.gltf", &text).unwrap();
        assert_eq!(loaded.data.positions.len(), 3);
        assert!(
            loaded.warnings.iter().any(|w| w.contains(INSTANCING)),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn unsupported_required_extensions_are_errors_naming_them() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({ "extensionsUsed": ["KHR_draco_mesh_compression"], "extensionsRequired": ["KHR_draco_mesh_compression"] }),
        );
        let error = load_str("squeezed.glb", &text).err().unwrap();
        assert!(
            error.contains("squeezed.glb") && error.contains("KHR_draco_mesh_compression"),
            "{error}"
        );
    }

    #[test]
    fn unsupported_optional_extensions_load_with_a_warning_naming_them() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({ "extensionsUsed": ["KHR_materials_volume", "VENDOR_something"],
                    "materials": [{ "extensions": { "KHR_materials_volume": { "thicknessFactor": 1.0 } } }] }),
        );
        let loaded = load_str("thick.gltf", &text).unwrap();
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("KHR_materials_volume") && loaded.warnings[0].contains("refraction"));
        assert!(loaded.warnings[1].contains("VENDOR_something"));
    }

    /// A file with a point, a spot and a directional light, the spot on a
    /// child node, and two cameras.
    fn lit(file: &mut File) -> String {
        let quarter = std::f32::consts::FRAC_1_SQRT_2;
        triangle(
            file,
            None,
            json!({
                "extensionsUsed": ["KHR_lights_punctual"],
                "extensions": { "KHR_lights_punctual": { "lights": [
                    { "type": "point", "name": "bulb", "color": [1.0, 0.5, 0.0], "intensity": 6830.0 },
                    { "type": "spot", "intensity": 683.0, "range": 7.0,
                      "spot": { "innerConeAngle": 0.2, "outerConeAngle": 1.6 } },
                    { "type": "directional", "intensity": 3.0 },
                ] } },
                "scenes": [{ "nodes": [0, 1, 3, 4] }],
                "nodes": [
                    { "mesh": 0 },
                    { "name": "porch", "translation": [0, 3, 0], "children": [2],
                      "extensions": { "KHR_lights_punctual": { "light": 0 } } },
                    { "name": "beam", "translation": [1, 0, 0], "rotation": [-quarter, 0, 0, quarter],
                      "extensions": { "KHR_lights_punctual": { "light": 1 } } },
                    { "name": "sun", "extensions": { "KHR_lights_punctual": { "light": 2 } } },
                    { "name": "rig", "translation": [0, 0, 5], "children": [5, 6] },
                    { "camera": 0, "translation": [0, 1, 0] },
                    { "camera": 1, "name": "map" },
                ],
                "cameras": [
                    { "type": "perspective", "name": "intro",
                      "perspective": { "yfov": 0.7, "aspectRatio": 1.5, "znear": 0.05, "zfar": 300.0 } },
                    { "type": "orthographic", "orthographic": { "xmag": 8.0, "ymag": 6.0, "znear": 0.1, "zfar": 50.0 } },
                ],
            }),
        )
    }

    #[test]
    fn lights_are_placed_by_their_nodes_in_ponko_units() {
        let mut file = File::default();
        let loaded = load_str("lit.gltf", &lit(&mut file)).unwrap();
        let by_name = |n: &str| {
            loaded
                .lights
                .iter()
                .find(|l| l.name == n)
                .unwrap_or_else(|| panic!("no {n}"))
        };
        let bulb = by_name("bulb");
        assert_eq!(bulb.transform.position, Vec3::new(0.0, 3.0, 0.0));
        let ModelLightKind::Point(point) = bulb.kind else {
            panic!("{:?}", bulb.kind)
        };
        assert!((point.intensity - 10.0).abs() < 1e-4, "6830 cd is 10 units");
        assert!(
            (point.range - (10.0f32 / 0.01).sqrt()).abs() < 1e-3,
            "no range: where it fades out"
        );
        assert!((point.color.r - 1.0).abs() < 1e-6 && (point.color.g - super::super::gltf::to_srgb(0.5)).abs() < 1e-6);
        // The spot hangs off the porch, turned to point down, its cone
        // narrowed to below 90 degrees.
        let beam = by_name("beam");
        assert!((beam.transform.position - Vec3::new(1.0, 3.0, 0.0)).length() < 1e-5);
        assert!(
            (beam.transform.forward() - Vec3::NEG_Y).length() < 1e-5,
            "{}",
            beam.transform.forward()
        );
        let ModelLightKind::Spot(spot) = beam.kind else {
            panic!("{:?}", beam.kind)
        };
        assert_eq!((spot.intensity, spot.range, spot.inner_angle), (1.0, 7.0, 0.2));
        assert!(spot.outer_angle < std::f32::consts::FRAC_PI_2 && spot.outer_angle > 1.56);
        let sun = by_name("sun");
        assert_eq!(
            sun.kind,
            ModelLightKind::Sun {
                color: Color::rgb(1.0, 1.0, 1.0),
                intensity: 3.0
            }
        );
    }

    #[test]
    fn exposed_lights_without_a_range_reach_until_their_exposed_light_fades() {
        let mut file = File::default();
        let loaded = load_str("lit.gltf", &lit(&mut file)).unwrap();
        let mut world = ponko_ecs::World::new();
        let mut lighting = super::super::Lighting::default();
        let at = Transform3D::default();
        let placed_range = |world: &mut ponko_ecs::World, lighting: &mut super::super::Lighting, name: &str, ev100| {
            let light = loaded.lights.iter().find(|l| l.name == name).unwrap();
            let entity = light.place_exposed(world, lighting, &at, ev100).unwrap();
            world
                .get::<PointLight3D>(entity)
                .map(|p| (p.intensity, p.range))
                .or_else(|| world.get::<SpotLight3D>(entity).map(|s| (s.intensity, s.range)))
                .unwrap()
        };
        // 6830 cd with no range, at EV100 0: 6830 / 1.2 in engine units,
        // reaching until that falls to the loader's cutoff.
        let (intensity, range) = placed_range(&mut world, &mut lighting, "bulb", 0.0);
        assert!((intensity - 6830.0 / 1.2).abs() < 0.05, "{intensity}");
        assert!(
            (range - (intensity / UNLIMITED_RANGE_CUTOFF).sqrt()).abs() < 1e-2,
            "{range}"
        );
        // A dim exposure keeps the loaded reach.
        let (_, range) = placed_range(&mut world, &mut lighting, "bulb", 20.0);
        assert_eq!(range, (10.0f32 / UNLIMITED_RANGE_CUTOFF).sqrt());
        // The file's own range stays as it is.
        let (_, range) = placed_range(&mut world, &mut lighting, "beam", 0.0);
        assert_eq!(range, 7.0);
    }

    #[test]
    fn cameras_are_placed_by_their_nodes() {
        let mut file = File::default();
        let loaded = load_str("lit.gltf", &lit(&mut file)).unwrap();
        assert_eq!(loaded.cameras.len(), 2);
        let intro = loaded.cameras.iter().find(|c| c.name == "intro").unwrap();
        assert_eq!(intro.transform.position, Vec3::new(0.0, 1.0, 5.0));
        assert_eq!(
            intro.projection,
            ModelProjection::Perspective {
                fov_y: 0.7,
                aspect: Some(1.5),
                near: 0.05,
                far: Some(300.0)
            }
        );
        let map = loaded.cameras.iter().find(|c| c.name == "map").unwrap();
        assert_eq!(
            map.projection,
            ModelProjection::Orthographic {
                half_width: 8.0,
                half_height: 6.0,
                near: 0.1,
                far: 50.0
            }
        );
        // Applied to the camera: where the model puts it, with its lens.
        let mut camera = super::super::Camera3D::new(Vec2::ONE);
        intro.apply(&mut camera, &Transform3D::at(Vec3::new(10.0, 0.0, 0.0)));
        assert_eq!(
            (camera.position, camera.fov_y, camera.near, camera.far),
            (Vec3::new(10.0, 1.0, 5.0), 0.7, 0.05, 300.0)
        );
    }

    /// Light and texture data as Blender's glTF exporter writes them (its
    /// default "Standard" lighting units): a 1000 W point lamp, a 500 W
    /// spot of 60° with a blend of 0.25, a sun of strength 3, and an image
    /// texture through a mapping node (scale 2, moved by a quarter).
    #[test]
    fn a_blender_style_export_loads_lights_and_textures_as_authored() {
        let watts_to_candela = |w: f32| w / (4.0 * std::f32::consts::PI) * 683.0;
        let spot_size = 60f32.to_radians();
        let mut file = File::default();
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let text = triangle(
            &mut file,
            Some(&uvs),
            json!({
                "extensionsUsed": ["KHR_lights_punctual", "KHR_texture_transform"],
                "extensions": { "KHR_lights_punctual": { "lights": [
                    { "name": "Point", "type": "point", "color": [1, 1, 1], "intensity": watts_to_candela(1000.0) },
                    { "name": "Spot", "type": "spot", "color": [1, 1, 1], "intensity": watts_to_candela(500.0),
                      "spot": { "innerConeAngle": spot_size / 2.0 * (1.0 - 0.25), "outerConeAngle": spot_size / 2.0 } },
                    { "name": "Sun", "type": "directional", "color": [1, 1, 1], "intensity": 3.0 },
                ] } },
                "scenes": [{ "nodes": [0, 1, 2, 3] }],
                "nodes": [
                    { "mesh": 0, "name": "Cube" },
                    { "name": "Point", "translation": [4.0, 5.9, -1.0], "extensions": { "KHR_lights_punctual": { "light": 0 } } },
                    { "name": "Spot", "translation": [0.0, 4.0, 0.0], "rotation": [-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2],
                      "extensions": { "KHR_lights_punctual": { "light": 1 } } },
                    { "name": "Sun", "rotation": [-0.5, 0.0, 0.0, 0.8660254], "extensions": { "KHR_lights_punctual": { "light": 2 } } },
                ],
                "materials": [{ "name": "Material", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0,
                    "extensions": { "KHR_texture_transform": { "offset": [0.25, -1.0], "scale": [2.0, 2.0] } } } } }],
                "textures": [{ "source": 0 }],
                "images": [{ "uri": png([200, 120, 40, 255]) }],
            }),
        );
        let loaded = load_str("scene.glb", &text).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        // Blender's watts, as Blender lights the scene: W / 4pi per unit of
        // distance squared, against the sun's strength as it is.
        let lamp = loaded.lights.iter().find(|l| l.name == "Point").unwrap();
        let ModelLightKind::Point(point) = lamp.kind else {
            panic!("{:?}", lamp.kind)
        };
        assert!(
            (point.intensity - 1000.0 / (4.0 * std::f32::consts::PI)).abs() < 1e-3,
            "{}",
            point.intensity
        );
        assert_eq!(lamp.transform.position, Vec3::new(4.0, 5.9, -1.0));
        let spot = loaded.lights.iter().find_map(|l| match l.kind {
            ModelLightKind::Spot(s) => Some((l, s)),
            _ => None,
        });
        let (spot_light, spot) = spot.unwrap();
        assert!(
            (spot.outer_angle - spot_size / 2.0).abs() < 1e-6 && (spot.inner_angle - spot_size * 0.375).abs() < 1e-6
        );
        assert!((spot_light.transform.forward() - Vec3::NEG_Y).length() < 1e-5);
        let sun = loaded.lights.iter().find(|l| l.name == "Sun").unwrap();
        assert!(matches!(sun.kind, ModelLightKind::Sun { intensity, .. } if intensity == 3.0));
        assert!((sun.transform.forward() - Vec3::new(0.0, -0.8660254, -0.5)).length() < 1e-5);
        // The mapping node's transform is the material's.
        let t = loaded.materials[0].texture_transform;
        assert!(
            close(t.apply(loaded.data.uvs[1]), Vec2::new(2.25, -1.0))
                && close(t.apply(loaded.data.uvs[2]), Vec2::new(0.25, 1.0))
        );
        assert!(loaded.materials[0].image.is_some());
    }

    #[test]
    fn spawned_lights_stand_where_the_model_puts_them() {
        let mut file = File::default();
        let loaded = load_str("lit.gltf", &lit(&mut file)).unwrap();
        let mut world = ponko_ecs::World::new();
        let model = Transform3D::at(Vec3::new(0.0, 0.0, -20.0));
        let spawned: Vec<_> = loaded
            .lights
            .iter()
            .filter_map(|l| l.spawn(&mut world, &model))
            .collect();
        assert_eq!(spawned.len(), 2, "the sun isn't an entity");
        let at = |e| world.get::<Transform3D>(e).unwrap().position;
        let positions: Vec<Vec3> = spawned.iter().map(|e| at(*e)).collect();
        assert!(positions.contains(&Vec3::new(0.0, 3.0, -20.0)) && positions.contains(&Vec3::new(1.0, 3.0, -20.0)));
        assert_eq!(world.query::<&PointLight3D>().iter().count(), 1);
        assert_eq!(world.query::<&SpotLight3D>().iter().count(), 1);
    }

    #[test]
    fn extension_images_fall_back_to_the_core_image_without_their_feature() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            Some(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            json!({
                "extensionsUsed": ["KHR_texture_basisu", "EXT_texture_webp"],
                "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }],
                "textures": [{ "source": 0, "extensions": {
                    "KHR_texture_basisu": { "source": 1 }, "EXT_texture_webp": { "source": 1 } } }],
                "images": [{ "uri": png([9, 8, 7, 255]) }, { "uri": "data:image/ktx2;base64,AAAA" }],
            }),
        );
        let loaded = load_str("fallback.gltf", &text).unwrap();
        let image = &loaded.images[loaded.materials[0].image.unwrap()];
        // The PNG, either because the build doesn't read the extension's
        // image or because that image (four zero bytes) can't be read.
        assert_eq!(&image.2[..4], &[9, 8, 7, 255]);
    }

    #[cfg(not(feature = "compressed-textures"))]
    #[test]
    fn a_texture_with_only_a_ktx2_image_needs_its_feature() {
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({
                "extensionsUsed": ["KHR_texture_basisu"],
                "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }],
                "textures": [{ "extensions": { "KHR_texture_basisu": { "source": 0 } } }],
                "images": [{ "uri": "data:image/ktx2;base64,AAAA" }],
            }),
        );
        let error = load_str("basis.gltf", &text).err().unwrap();
        assert!(
            error.contains("KHR_texture_basisu") && error.contains("`compressed-textures`"),
            "{error}"
        );
        let required = text.replace(
            r#""extensionsUsed":["KHR_texture_basisu"]"#,
            r#""extensionsUsed":["KHR_texture_basisu"],"extensionsRequired":["KHR_texture_basisu"]"#,
        );
        assert_ne!(required, text);
        let error = load_str("basis.gltf", &required).err().unwrap();
        assert!(
            error.contains("needs the glTF extension KHR_texture_basisu") && error.contains("`compressed-textures`"),
            "{error}"
        );
    }

    /// Two triangles over the same corners, every view meshopt compressed
    /// into buffer 1 with buffer 0 its fallback (`fallback_data`: holding
    /// the plain data too, else nothing), as gltfpack writes them:
    /// positions with the exponential filter, normals octahedral, one
    /// triangle's indices as TRIANGLES and the other's as INDICES.
    #[cfg(all(feature = "gltf-meshopt", not(target_arch = "wasm32")))]
    fn meshopt_gltf(corrupt: bool) -> String {
        use meshopt::{
            EncodeExpMode, encode_filter_exp, encode_filter_oct, encode_index_buffer, encode_index_sequence,
            encode_vertex_buffer,
        };
        let positions: Vec<[f32; 3]> = TRIANGLE.chunks(3).map(|c| [c[0], c[1] * 2.0, c[2]]).collect();
        let normals = [[0.0f32, 0.0, 1.0, 0.0]; 3];
        let streams = [
            (
                encode_vertex_buffer(&encode_filter_exp(&positions, 15, EncodeExpMode::Separate)).unwrap(),
                12usize,
                3usize,
                "ATTRIBUTES",
                "EXPONENTIAL",
            ),
            (
                encode_vertex_buffer(&encode_filter_oct::<i8>(&normals, 8)).unwrap(),
                4,
                3,
                "ATTRIBUTES",
                "OCTAHEDRAL",
            ),
            (encode_index_buffer(&[0, 1, 2], 3).unwrap(), 4, 3, "TRIANGLES", "NONE"),
            (encode_index_sequence(&[0, 2, 1], 3).unwrap(), 2, 3, "INDICES", "NONE"),
        ];
        let mut compressed = Vec::new();
        let mut views = Vec::new();
        let mut at = 0usize;
        for (i, (bytes, stride, count, mode, filter)) in streams.iter().enumerate() {
            compressed.resize(compressed.len().next_multiple_of(4), 0);
            let mut bytes = bytes.clone();
            if corrupt && i == 0 {
                bytes.truncate(3);
            }
            let mut view = json!({ "buffer": 0, "byteOffset": at, "byteLength": count * stride,
                "extensions": { "EXT_meshopt_compression": { "buffer": 1, "byteOffset": compressed.len(),
                    "byteLength": bytes.len(), "byteStride": stride, "count": count, "mode": mode, "filter": filter } } });
            if *mode == "ATTRIBUTES" {
                view["byteStride"] = json!(stride);
            }
            at += (count * stride).next_multiple_of(4);
            compressed.extend_from_slice(&bytes);
            views.push(view);
        }
        json!({
            "asset": { "version": "2.0" },
            "extensionsUsed": ["EXT_meshopt_compression", "KHR_mesh_quantization"],
            "extensionsRequired": ["EXT_meshopt_compression", "KHR_mesh_quantization"],
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{ "primitives": [
                { "attributes": { "POSITION": 0, "NORMAL": 1 }, "indices": 2 },
                { "attributes": { "POSITION": 0, "NORMAL": 1 }, "indices": 3 },
            ] }],
            "buffers": [
                { "byteLength": at, "extensions": { "EXT_meshopt_compression": { "fallback": true } } },
                { "byteLength": compressed.len(), "uri": format!("data:application/octet-stream;base64,{}", to_base64(&compressed)) },
            ],
            "bufferViews": views,
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 2, 0] },
                { "bufferView": 1, "componentType": 5120, "normalized": true, "count": 3, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5125, "count": 3, "type": "SCALAR" },
                { "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" },
            ],
        })
        .to_string()
    }

    #[cfg(all(feature = "gltf-meshopt", not(target_arch = "wasm32")))]
    #[test]
    fn meshopt_compressed_geometry_decompresses_at_load() {
        let loaded = load_str("packed.glb", &meshopt_gltf(false)).unwrap();
        let data = &loaded.data;
        assert_eq!(data.indices, vec![0, 1, 2, 3, 5, 4]);
        let p = &data.positions;
        assert!(
            (p[1] - Vec3::X).length() < 1e-4 && (p[2] - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4,
            "{p:?}"
        );
        assert!(
            data.normals.iter().all(|n| (*n - Vec3::Z).length() < 0.02),
            "{:?}",
            data.normals
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[cfg(all(feature = "gltf-meshopt", not(target_arch = "wasm32")))]
    #[test]
    fn corrupt_meshopt_data_is_an_error_naming_the_view() {
        let error = load_str("broken.glb", &meshopt_gltf(true)).err().unwrap();
        assert!(
            error.contains("broken.glb: buffer view 0") && error.contains("corrupt"),
            "{error}"
        );
    }

    #[cfg(not(all(feature = "gltf-meshopt", not(target_arch = "wasm32"))))]
    #[test]
    fn meshopt_files_need_their_feature_unless_they_have_fallback_data() {
        // Required, and with no fallback data: an error naming the feature.
        let mut file = File::default();
        let text = triangle(
            &mut file,
            None,
            json!({ "extensionsUsed": ["EXT_meshopt_compression"], "extensionsRequired": ["EXT_meshopt_compression"] }),
        );
        let error = load_str("packed.glb", &text).err().unwrap();
        assert!(
            error.contains("EXT_meshopt_compression") && error.contains("`gltf-meshopt`"),
            "{error}"
        );
        // Optional, with the plain data in the view's own buffer: that loads.
        let mut file = File::default();
        let position = file.floats(&TRIANGLE, 3);
        file.views[0]["extensions"] = json!({ "EXT_meshopt_compression": {
            "buffer": 0, "byteLength": 4, "byteStride": 12, "count": 3, "mode": "ATTRIBUTES" } });
        let text = file.gltf(json!({
            "extensionsUsed": ["EXT_meshopt_compression"],
            "scene": 0, "scenes": [{ "nodes": [0] }], "nodes": [{ "mesh": 0 }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": position } }] }],
        }));
        let loaded = load_str("packed.glb", &text).unwrap();
        assert_eq!(loaded.data.positions[1], Vec3::X);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("EXT_meshopt_compression") && w.contains("fallback"))
        );
    }
}
