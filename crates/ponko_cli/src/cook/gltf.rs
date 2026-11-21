//! Model cooking: glTF and GLB files with their meshes optimized, written
//! as one self-contained GLB under the same name.
//!
//! For every indexed triangle primitive, [`cook`] reorders the triangles for
//! the GPU's vertex cache and then for less overdraw, and the vertices in
//! the order the triangles first use them (meshoptimizer's algorithms;
//! unused vertices are dropped). Normals become 16-bit normalized integers
//! and texture coordinates in 0..1 16-bit unsigned normalized ones
//! (`KHR_mesh_quantization`, which the engine always reads): an error under
//! 2e-5, far below what a texel or a lighting gradient shows. Positions
//! become 16-bit integers under a node scale where that keeps them within
//! [`POSITION_ERROR`] (see `quantize_positions`); the engine folds the
//! scale back, so node transforms, colliders and navigation meshes read the
//! source's numbers.
//!
//! Only the mesh data changes: nodes, materials, animations, skins and the
//! `extras` of every object are written back as they were read. An
//! accessor that something else also uses (another primitive, an
//! animation) keeps its vertex order. A file with compressed geometry
//! (meshopt, Draco) retains its geometry byte for byte while eligible
//! embedded images are compressed; the replaced images' bytes are dropped
//! from the buffers the file holds itself. An unknown extension prevents rewriting
//! the file, since its data may point to objects this module would change.

#![deny(missing_docs)]

use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(feature = "textures")]
mod kept;
#[cfg(feature = "textures")]
mod textures;

/// What [`cook`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Reorder triangles and vertices.
    pub optimize: bool,
    /// Quantize normals and texture coordinates.
    pub quantize: bool,
    /// Compress the images the file holds (needs the `textures` feature;
    /// without it they stay as they are).
    pub images: Option<Images>,
}

/// How [`cook`] compresses a model's images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Images {
    /// Basis Universal UASTC (mobile) instead of BC blocks.
    pub basis: bool,
}

/// Extensions whose data never points into buffers, or does so only in the
/// places this module rewrites (images, mesh instancing attributes).
const KNOWN: &[&str] = &[
    "KHR_lights_punctual",
    "KHR_texture_transform",
    "KHR_mesh_quantization",
    "KHR_materials_emissive_strength",
    "KHR_materials_transmission",
    "KHR_materials_clearcoat",
    "KHR_materials_ior",
    "KHR_materials_volume",
    "KHR_materials_sheen",
    "KHR_materials_specular",
    "KHR_materials_iridescence",
    "KHR_materials_anisotropy",
    "KHR_materials_unlit",
    "KHR_materials_variants",
    "KHR_texture_basisu",
    "EXT_texture_webp",
    "EXT_mesh_gpu_instancing",
    POSITIONS_EXTENSION,
];

const COMPRESSED_GEOMETRY: &[&str] = &[
    "EXT_meshopt_compression",
    "KHR_meshopt_compression",
    "KHR_draco_mesh_compression",
];

/// The extension that marks the node a quantized mesh hangs from (see
/// [`quantize_positions`]).
pub const POSITIONS_EXTENSION: &str = "PONKO_quantized_positions";

/// The most a quantized position may be off, in the file's units (meters):
/// a tenth of a millimeter. A mesh whose 16-bit steps would be coarser (one
/// more than 13.1 m across) gets a range per primitive, and a primitive
/// that big keeps its float positions.
pub const POSITION_ERROR: f32 = 1e-4;

/// The files a glTF file reads by URI that `all` holds (buffers and
/// images), named as in `all`: what its cache key covers.
pub fn dependencies(
    name: &str,
    bytes: &[u8],
    all: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let json = super::gltf_json(bytes).ok_or_else(|| format!("{name}: not a glTF file"))?;
    let folder = name.rfind('/').map_or("", |i| &name[..=i]);
    let mut out = Vec::new();
    for item in json["buffers"].as_array().into_iter().flatten() {
        if let Some(uri) = item["uri"].as_str().filter(|u| !u.starts_with("data:")) {
            let path = super::normalize(&format!("{folder}{}", super::percent_decode(uri)));
            if let Some(bytes) = all.get(&path) {
                out.push((path, bytes.clone()));
            }
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    Ok(out)
}

/// An accessor's layout.
struct Layout {
    view: usize,
    offset: usize,
    stride: usize,
    count: usize,
    /// Bytes of one element.
    size: usize,
    component: u64,
    components: usize,
}

fn component_size(component: u64) -> Option<usize> {
    match component {
        5120 | 5121 => Some(1),
        5122 | 5123 => Some(2),
        5125 | 5126 => Some(4),
        _ => None,
    }
}

fn components(kind: &str) -> Option<usize> {
    match kind {
        "SCALAR" => Some(1),
        "VEC2" => Some(2),
        "VEC3" => Some(3),
        "VEC4" | "MAT2" => Some(4),
        "MAT3" => Some(9),
        "MAT4" => Some(16),
        _ => None,
    }
}

/// A file being cooked: its JSON and its buffers.
struct Doc {
    json: Value,
    buffers: Vec<Vec<u8>>,
}

impl Doc {
    /// The layout of accessor `i`, `None` when it has no view or is sparse.
    fn layout(&self, i: usize) -> Result<Option<Layout>, String> {
        let a = &self.json["accessors"][i];
        if !a.is_object() {
            return Err(format!("accessor {i} doesn't exist"));
        }
        let Some(view) = a["bufferView"].as_u64().map(|v| v as usize) else {
            return Ok(None);
        };
        if a.get("sparse").is_some() {
            return Ok(None);
        }
        let component = a["componentType"].as_u64().unwrap_or(0);
        let size = component_size(component).ok_or_else(|| format!("accessor {i} has an unknown component type"))?;
        let comps = components(a["type"].as_str().unwrap_or_default())
            .ok_or_else(|| format!("accessor {i} has an unknown type"))?;
        if comps > 4 {
            return Ok(None); // matrices: never a vertex attribute here
        }
        let v = &self.json["bufferViews"][view];
        let elem = size * comps;
        let stride = v["byteStride"].as_u64().map_or(elem, |s| s as usize);
        Ok(Some(Layout {
            view,
            offset: a["byteOffset"].as_u64().unwrap_or(0) as usize,
            stride,
            count: a["count"].as_u64().unwrap_or(0) as usize,
            size: elem,
            component,
            components: comps,
        }))
    }

    /// The bytes of a view.
    fn view(&self, view: usize) -> Result<&[u8], String> {
        let v = &self.json["bufferViews"][view];
        let buffer = v["buffer"].as_u64().unwrap_or(u64::MAX) as usize;
        let start = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = v["byteLength"].as_u64().unwrap_or(0) as usize;
        self.buffers
            .get(buffer)
            .and_then(|b| b.get(start..start.checked_add(len)?))
            .ok_or_else(|| format!("buffer view {view} is past the end of its buffer"))
    }

    /// An accessor's elements, packed.
    fn elements(&self, l: &Layout) -> Result<Vec<u8>, String> {
        let view = self.view(l.view)?;
        let mut out = Vec::with_capacity(l.count * l.size);
        for i in 0..l.count {
            let at = l.offset + i * l.stride;
            out.extend_from_slice(
                view.get(at..at + l.size)
                    .ok_or_else(|| format!("an accessor of view {} is past its end", l.view))?,
            );
        }
        Ok(out)
    }

    /// Indices as `u32`.
    fn indices(&self, i: usize) -> Result<Option<Vec<u32>>, String> {
        let Some(l) = self.layout(i)? else { return Ok(None) };
        if l.components != 1 {
            return Err(format!("index accessor {i} isn't SCALAR"));
        }
        let bytes = self.elements(&l)?;
        Ok(Some(match l.component {
            5121 => bytes.iter().map(|&b| u32::from(b)).collect(),
            5123 => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u32::from(u16::from_le_bytes(*c)))
                .collect(),
            5125 => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect(),
            _ => return Err(format!("index accessor {i} isn't unsigned")),
        }))
    }

    /// Adds `bytes` as a new view of a new buffer; returns the view.
    fn add_view(&mut self, bytes: Vec<u8>, stride: Option<usize>, target: u64) -> usize {
        let buffer = self.buffers.len();
        let mut view = json!({ "buffer": buffer, "byteLength": bytes.len(), "target": target });
        if let Some(stride) = stride {
            view["byteStride"] = json!(stride);
        }
        self.buffers.push(bytes);
        let views = self.json["bufferViews"].as_array_mut().expect("checked to be an array");
        views.push(view);
        views.len() - 1
    }
}

/// How many times each accessor is used, in every place glTF 2.0 and the
/// known extensions use accessors.
fn accessor_uses(json: &Value) -> Vec<usize> {
    let mut uses = vec![0; json["accessors"].as_array().map_or(0, Vec::len)];
    let mut count = |v: &Value| {
        if let Some(i) = v.as_u64().and_then(|i| uses.get_mut(i as usize)) {
            *i += 1;
        }
    };
    for mesh in json["meshes"].as_array().into_iter().flatten() {
        for p in mesh["primitives"].as_array().into_iter().flatten() {
            count(&p["indices"]);
            p["attributes"]
                .as_object()
                .into_iter()
                .flatten()
                .for_each(|(_, a)| count(a));
            for target in p["targets"].as_array().into_iter().flatten() {
                target.as_object().into_iter().flatten().for_each(|(_, a)| count(a));
            }
        }
    }
    for skin in json["skins"].as_array().into_iter().flatten() {
        count(&skin["inverseBindMatrices"]);
    }
    for animation in json["animations"].as_array().into_iter().flatten() {
        for sampler in animation["samplers"].as_array().into_iter().flatten() {
            count(&sampler["input"]);
            count(&sampler["output"]);
        }
    }
    for node in json["nodes"].as_array().into_iter().flatten() {
        if let Some(attributes) = node["extensions"]["EXT_mesh_gpu_instancing"]["attributes"].as_object() {
            attributes.values().for_each(&mut count);
        }
    }
    uses
}

/// A cooked model: its bytes, the note for the report, and its images'
/// video memory before and after, when it compressed any.
pub type Cooked = (Vec<u8>, String, Option<(u64, u64)>);

/// Cooks one glTF or GLB file. `Ok(Err(reason))` keeps it as it is;
/// `Err` is a broken file.
pub fn cook(
    name: &str,
    bytes: &[u8],
    deps: &[(String, Vec<u8>)],
    options: Options,
) -> Result<Result<Cooked, String>, String> {
    let fail = |e: String| format!("{name}: {e}");
    let (json, blob) = if bytes.starts_with(b"glTF") {
        split_glb(bytes).map_err(fail)?
    } else {
        let json: Value = serde_json::from_slice(bytes).map_err(|e| fail(format!("not a glTF file: {e}")))?;
        (json, None)
    };
    if !json.is_object() {
        return Err(fail("not a glTF file".into()));
    }
    let mut kept_geometry = None;
    for extension in ["extensionsUsed", "extensionsRequired"]
        .into_iter()
        .flat_map(|key| json[key].as_array().into_iter().flatten())
    {
        let extension = extension.as_str().unwrap_or_default();
        if let Some(compression) = COMPRESSED_GEOMETRY.iter().copied().find(|known| *known == extension) {
            kept_geometry = Some(compression);
        } else if !KNOWN.contains(&extension) {
            return Ok(Err(format!(
                "it uses {extension}, whose data the cooker doesn't rewrite"
            )));
        }
    }
    let kept_reason = |ext| format!("it uses {ext}, whose geometry the cooker keeps");
    if let Some(ext) = kept_geometry
        && (!cfg!(feature = "textures") || options.images.is_none())
    {
        return Ok(Err(kept_reason(ext)));
    }
    #[cfg(feature = "textures")]
    let binary = bytes.starts_with(b"glTF");
    #[cfg(feature = "textures")]
    let has_bin = blob.is_some();
    let folder = name.rfind('/').map_or("", |i| &name[..=i]);
    let mut buffers = Vec::new();
    let mut blob = blob;
    for (i, buffer) in json["buffers"].as_array().into_iter().flatten().enumerate() {
        let data = match buffer["uri"].as_str() {
            Some(uri) if uri.starts_with("data:") => {
                let (_, data) = uri
                    .split_once(";base64,")
                    .ok_or_else(|| fail(format!("buffer {i}: only base64 data URIs are read")))?;
                base64(data).map_err(|e| fail(format!("buffer {i}: {e}")))?
            }
            Some(uri) => {
                let path = super::normalize(&format!("{folder}{}", super::percent_decode(uri)));
                match deps.iter().find(|(n, _)| *n == path) {
                    Some((_, bytes)) => bytes.clone(),
                    None => return Err(fail(format!("buffer {i}: {uri} is not among the assets"))),
                }
            }
            None if i == 0 && blob.is_some() => blob.take().unwrap_or_default(),
            // With required meshopt, a fallback placeholder can have no URI.
            None if kept_geometry.is_some() => Vec::new(),
            None => return Err(fail(format!("buffer {i} has no data"))),
        };
        buffers.push(data);
    }
    let mut doc = Doc { json, buffers };
    if !doc.json["bufferViews"].is_array() {
        doc.json["bufferViews"] = json!([]);
    }
    #[cfg(feature = "textures")]
    if let (Some(ext), Some(images)) = (kept_geometry, options.images) {
        let count = doc.buffers.len();
        let image_views = kept::image_views(&doc.json);
        let report = textures::compress(&mut doc, images).map_err(fail)?;
        if report.images == 0 {
            return Ok(Err(kept_reason(ext)));
        }
        let (out, dropped) = kept::write(doc, count, binary, has_bin, &image_views).map_err(fail)?;
        let mut note = format!("{ext} geometry kept, {} images compressed", report.images);
        if dropped > 0 {
            note.push_str(&format!(", {dropped} bytes of replaced images dropped"));
        }
        return Ok(Ok((out, note, Some((report.before, report.after)))));
    }

    let mut stats = optimize(&mut doc, options).map_err(fail)?;
    if options.quantize {
        let q = quantize_positions(&mut doc).map_err(fail)?;
        stats.positions = q.meshes;
        stats.split = q.split;
        stats.position_primitives = q.primitives;
        stats.position_error = q.error;
    }
    #[cfg(feature = "textures")]
    let images = match options.images {
        Some(settings) => textures::compress(&mut doc, settings).map_err(fail)?,
        None => textures::Report::default(),
    };
    #[cfg(not(feature = "textures"))]
    let images = NoImages::default();
    if stats.primitives == 0 && images.images == 0 {
        return Ok(Err("it has no indexed triangle meshes to optimize".into()));
    }
    let out = write_glb(doc).map_err(fail)?;
    let mut note = format!("{} meshes", stats.primitives);
    if options.optimize && stats.indices > 0 {
        note.push_str(&format!(
            ", vertex cache misses per triangle {:.2} -> {:.2}",
            stats.acmr_before / stats.indices as f64 * 3.0,
            stats.acmr_after / stats.indices as f64 * 3.0
        ));
    }
    if stats.quantized > 0 {
        note.push_str(&format!(", {} attributes quantized", stats.quantized));
    }
    if stats.positions > 0 {
        note.push_str(&format!(
            ", positions of {} meshes 16-bit (off by up to {:.3} mm)",
            stats.positions,
            stats.position_error * 1000.0
        ));
        if stats.split > 0 {
            note.push_str(&format!(
                ", {} of them a range per primitive ({} primitives)",
                stats.split, stats.position_primitives
            ));
        }
    }
    let gpu = (images.images > 0).then(|| {
        note.push_str(&format!(", {} images compressed", images.images));
        (images.before, images.after)
    });
    Ok(Ok((out, note, gpu)))
}

/// Without the `textures` feature no image is compressed.
#[cfg(not(feature = "textures"))]
#[derive(Default)]
struct NoImages {
    images: usize,
    before: u64,
    after: u64,
}

/// What [`optimize`] did.
#[derive(Default)]
struct Stats {
    primitives: usize,
    /// Sums of ACMR x index count, before and after.
    acmr_before: f64,
    acmr_after: f64,
    indices: usize,
    quantized: usize,
    /// Meshes whose positions are quantized, of them split by primitive,
    /// primitives quantized, and the largest error.
    positions: usize,
    split: usize,
    position_primitives: usize,
    position_error: f32,
}

fn optimize(doc: &mut Doc, options: Options) -> Result<Stats, String> {
    let uses = accessor_uses(&doc.json);
    let mut stats = Stats::default();
    let meshes = doc.json["meshes"].as_array().map_or(0, Vec::len);
    let mut quantization = false;
    for m in 0..meshes {
        let primitives = doc.json["meshes"][m]["primitives"].as_array().map_or(0, Vec::len);
        for p in 0..primitives {
            let prim = doc.json["meshes"][m]["primitives"][p].clone();
            if prim["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let Some(index_accessor) = prim["indices"].as_u64().map(|i| i as usize) else {
                continue;
            };
            let Some(position) = prim["attributes"]["POSITION"].as_u64().map(|i| i as usize) else {
                continue;
            };
            let Some(pos) = doc.layout(position)? else { continue };
            if pos.component != 5126 || pos.components != 3 {
                continue; // quantized already
            }
            let vertex_count = pos.count;
            let Some(mut indices) = doc.indices(index_accessor)? else {
                continue;
            };
            if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= vertex_count) {
                return Err(format!("mesh {m} primitive {p} has indices past its vertices"));
            }
            if indices.is_empty() {
                continue;
            }
            // Every attribute (and morph target) this primitive alone uses
            // can be reordered and rewritten in place.
            let mut attributes: Vec<(String, usize)> = prim["attributes"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.clone(), v.as_u64()? as usize)))
                .collect();
            for (t, target) in prim["targets"].as_array().into_iter().flatten().enumerate() {
                for (k, v) in target.as_object().into_iter().flatten() {
                    if let Some(v) = v.as_u64() {
                        attributes.push((format!("target {t} {k}"), v as usize));
                    }
                }
            }
            let mut layouts = Vec::new();
            let mut exclusive = true;
            for (_, a) in &attributes {
                match doc.layout(*a)? {
                    Some(l) if l.count == vertex_count && uses.get(*a) == Some(&1) => layouts.push(l),
                    _ => exclusive = false,
                }
            }
            let positions = doc.elements(&pos)?;
            stats.primitives += 1;
            if options.optimize {
                let before = meshopt::analyze_vertex_cache(&indices, vertex_count, 16, 0, 0);
                indices = meshopt::optimize_vertex_cache(&indices, vertex_count);
                let adapter = meshopt::VertexDataAdapter::new(&positions, 12, 0).map_err(|e| e.to_string())?;
                meshopt::optimize_overdraw_in_place(&mut indices, &adapter, 1.05);
                let after = meshopt::analyze_vertex_cache(&indices, vertex_count, 16, 0, 0);
                stats.acmr_before += f64::from(before.acmr) * indices.len() as f64 / 3.0;
                stats.acmr_after += f64::from(after.acmr) * indices.len() as f64 / 3.0;
                stats.indices += indices.len();
            }
            let mut count = vertex_count;
            if exclusive {
                let remap = if options.optimize {
                    let remap = meshopt::optimize_vertex_fetch_remap(&indices, vertex_count);
                    for i in &mut indices {
                        *i = remap[*i as usize];
                    }
                    count = remap.iter().filter(|&&r| r != u32::MAX).count();
                    Some(remap)
                } else {
                    None
                };
                for ((semantic, accessor), layout) in attributes.iter().zip(&layouts) {
                    let old = doc.elements(layout)?;
                    let mut data = match &remap {
                        Some(remap) => {
                            let mut data = vec![0u8; count * layout.size];
                            for (v, &to) in remap.iter().enumerate() {
                                if to != u32::MAX {
                                    let to = to as usize;
                                    data[to * layout.size..(to + 1) * layout.size]
                                        .copy_from_slice(&old[v * layout.size..(v + 1) * layout.size]);
                                }
                            }
                            data
                        }
                        None => old,
                    };
                    let a = &mut doc.json["accessors"][*accessor];
                    let mut elem = layout.size;
                    let float = layout.component == 5126 && a["normalized"].as_bool() != Some(true);
                    if options.quantize && float {
                        if semantic == "NORMAL" && layout.components == 3 {
                            data = quantize_normals(&data);
                            elem = 6;
                            a["componentType"] = json!(5122);
                            a["normalized"] = json!(true);
                            stats.quantized += 1;
                            quantization = true;
                        } else if semantic.starts_with("TEXCOORD_")
                            && layout.components == 2
                            && let Some(q) = quantize_unit(&data)
                        {
                            data = q;
                            elem = 4;
                            a["componentType"] = json!(5123);
                            a["normalized"] = json!(true);
                            stats.quantized += 1;
                            quantization = true;
                        }
                    }
                    let (packed, stride) = pad_elements(&data, elem);
                    let obj = a.as_object_mut().ok_or("an accessor isn't an object")?;
                    obj.remove("byteOffset");
                    obj.insert("count".into(), json!(count));
                    let is_float = obj.get("componentType").and_then(Value::as_u64) == Some(5126);
                    if is_float && (obj.contains_key("min") || semantic == "POSITION") {
                        let (min, max) = bounds(&data, layout.components);
                        obj.insert("min".into(), json!(min));
                        obj.insert("max".into(), json!(max));
                    } else if !is_float {
                        obj.remove("min");
                        obj.remove("max");
                    }
                    let data = packed;
                    let view = doc.add_view(data, stride, 34962);
                    doc.json["accessors"][*accessor]["bufferView"] = json!(view);
                }
            }
            // The indices: in place when only this primitive uses them.
            let wide = count > 65_535;
            let data: Vec<u8> = if wide {
                indices.iter().flat_map(|i| i.to_le_bytes()).collect()
            } else {
                indices.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect()
            };
            let view = doc.add_view(data, None, 34963);
            let accessor = json!({
                "bufferView": view,
                "componentType": if wide { 5125 } else { 5123 },
                "count": indices.len(),
                "type": "SCALAR",
            });
            if uses.get(index_accessor) == Some(&1) {
                doc.json["accessors"][index_accessor] = accessor;
            } else {
                let list = doc.json["accessors"].as_array_mut().ok_or("no accessors")?;
                list.push(accessor);
                doc.json["meshes"][m]["primitives"][p]["indices"] = json!(list.len() - 1);
            }
        }
    }
    if quantization {
        for key in ["extensionsUsed", "extensionsRequired"] {
            let list = doc.json[key].as_array().cloned().unwrap_or_default();
            if !list.iter().any(|e| e == "KHR_mesh_quantization") {
                let mut list = list;
                list.push(json!("KHR_mesh_quantization"));
                doc.json[key] = Value::Array(list);
            }
        }
    }
    Ok(stats)
}

/// Positions as 16-bit unsigned integers with a node scale
/// (`KHR_mesh_quantization`), where that is off by at most
/// [`POSITION_ERROR`]: positions become steps of one 65535th of their
/// largest side from their least corner, and each node that shows the mesh
/// gets a child node that shows it instead, moved to that corner and scaled
/// by that step (the same on every axis, so normals stay as they are). Any
/// glTF reader draws the same shape. The engine folds such a child (marked
/// [`POSITIONS_EXTENSION`]) back into its parent as it loads the file, so
/// game code sees the node tree, transforms and extras of the source, and
/// positions within the bound.
///
/// One range for the whole mesh when that keeps the bound. A mesh too big
/// for one (a level) gets a range per primitive: the mesh is split into one
/// mesh per primitive that fits the bound, each shown by a child node of its
/// own scale, and one more for the primitives that don't (their positions
/// stay floats, under a child without a transform). The children's
/// extension names the source mesh and the primitives each holds, so the
/// engine puts the mesh back together as it was, primitive by primitive.
///
/// Left as floats: meshes with morph targets, meshes a skinned or
/// instanced node shows (their node's transform doesn't apply), meshes no
/// node shows, positions another primitive shares, and primitives past the
/// bound. Returns what it quantized.
fn quantize_positions(doc: &mut Doc) -> Result<Quantized, String> {
    let uses = accessor_uses(&doc.json);
    let meshes = doc.json["meshes"].as_array().map_or(0, Vec::len);
    let nodes = doc.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut out = Quantized::default();
    for m in 0..meshes {
        let showing: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n["mesh"].as_u64() == Some(m as u64))
            .map(|(i, _)| i)
            .collect();
        let movable = |n: &usize| {
            let node = &nodes[*n];
            node.get("skin").is_none() && node["extensions"].get("EXT_mesh_gpu_instancing").is_none()
        };
        if showing.is_empty() || !showing.iter().all(movable) {
            continue;
        }
        let primitives = doc.json["meshes"][m]["primitives"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if primitives.is_empty() || primitives.iter().any(|p| p.get("targets").is_some()) {
            continue;
        }
        // Each primitive's float positions, when only it uses them.
        let mut candidates: Vec<Option<(usize, Vec<f32>)>> = Vec::with_capacity(primitives.len());
        for p in &primitives {
            let accessor = p["attributes"]["POSITION"].as_u64().map(|a| a as usize);
            let layout = match accessor {
                Some(a) => doc.layout(a)?,
                None => None,
            };
            candidates.push(match (accessor, layout) {
                (Some(a), Some(l)) if l.component == 5126 && l.components == 3 && uses.get(a) == Some(&1) => {
                    let data: Vec<f32> = doc
                        .elements(&l)?
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c))
                        .collect();
                    Some((a, data))
                }
                _ => None,
            });
        }
        // One range for the mesh, when every primitive can move and it
        // keeps the bound.
        if candidates.iter().all(Option::is_some) {
            let all: Vec<&[f32]> = candidates.iter().flatten().map(|(_, d)| d.as_slice()).collect();
            if let Some(range) = Range::of(&all)
                && range.error <= POSITION_ERROR
            {
                for (accessor, data) in candidates.iter().flatten() {
                    write_quantized(doc, *accessor, &range.quantize(data))?;
                }
                for &n in &showing {
                    let child = json!({
                        "mesh": m,
                        "translation": range.corner,
                        "scale": [range.step, range.step, range.step],
                        "extensions": { POSITIONS_EXTENSION: {} },
                    });
                    hang(doc, n, vec![child])?;
                }
                out.meshes += 1;
                out.primitives += primitives.len();
                out.error = out.error.max(range.error);
                continue;
            }
        }
        // Else a range per primitive: those that keep the bound.
        let ranges: Vec<Option<Range>> = candidates
            .iter()
            .map(|c| {
                c.as_ref()
                    .and_then(|(_, d)| Range::of(&[d.as_slice()]))
                    .filter(|r| r.error <= POSITION_ERROR)
            })
            .collect();
        let fitting = ranges.iter().flatten().count();
        if fitting == 0 || primitives.len() == 1 {
            continue;
        }
        for (candidate, range) in candidates.iter().zip(&ranges) {
            if let (Some((accessor, data)), Some(range)) = (candidate, range) {
                write_quantized(doc, *accessor, &range.quantize(data))?;
                out.error = out.error.max(range.error);
            }
        }
        // The pieces, by their first primitive: each quantized primitive
        // alone, the float ones together.
        let mut pieces: Vec<(Vec<usize>, Option<&Range>)> = Vec::new();
        let mut floats: Option<usize> = None;
        for (p, range) in ranges.iter().enumerate() {
            match range {
                Some(r) => pieces.push((vec![p], Some(r))),
                None => match floats {
                    Some(at) => pieces[at].0.push(p),
                    None => {
                        floats = Some(pieces.len());
                        pieces.push((vec![p], None));
                    }
                },
            }
        }
        let name = doc.json["meshes"][m].get("name").cloned();
        let mut piece_meshes = Vec::with_capacity(pieces.len());
        for (k, (list, _)) in pieces.iter().enumerate() {
            let prims: Vec<Value> = list.iter().map(|&p| primitives[p].clone()).collect();
            if k == 0 {
                doc.json["meshes"][m]["primitives"] = Value::Array(prims);
                piece_meshes.push(m);
            } else {
                let mut mesh = json!({ "primitives": prims });
                if let Some(name) = &name {
                    mesh["name"] = name.clone();
                }
                let list = doc.json["meshes"].as_array_mut().ok_or("no meshes")?;
                list.push(mesh);
                piece_meshes.push(list.len() - 1);
            }
        }
        for &n in &showing {
            let children = pieces
                .iter()
                .zip(&piece_meshes)
                .map(|((list, range), &piece)| {
                    let mut child = json!({
                        "mesh": piece,
                        "extensions": { POSITIONS_EXTENSION: { "mesh": m, "primitives": list } },
                    });
                    if let Some(r) = range {
                        child["translation"] = json!(r.corner);
                        child["scale"] = json!([r.step, r.step, r.step]);
                    }
                    child
                })
                .collect();
            hang(doc, n, children)?;
        }
        out.meshes += 1;
        out.split += 1;
        out.primitives += fitting;
    }
    if out.meshes > 0 {
        for (key, extension) in [
            ("extensionsUsed", "KHR_mesh_quantization"),
            ("extensionsRequired", "KHR_mesh_quantization"),
            ("extensionsUsed", POSITIONS_EXTENSION),
        ] {
            let mut list = doc.json[key].as_array().cloned().unwrap_or_default();
            if !list.iter().any(|e| e == extension) {
                list.push(json!(extension));
                doc.json[key] = Value::Array(list);
            }
        }
    }
    Ok(out)
}

/// What [`quantize_positions`] did.
#[derive(Default)]
struct Quantized {
    /// Meshes with quantized positions.
    meshes: usize,
    /// Of those, meshes split into a range per primitive.
    split: usize,
    /// Primitives with quantized positions.
    primitives: usize,
    /// The largest error.
    error: f32,
}

/// The 16-bit grid some positions are put on: its least corner, its step
/// and how far it moves them (measured, as the engine reads them back:
/// `q * step + corner`, in f32).
struct Range {
    corner: [f32; 3],
    step: f32,
    error: f32,
}

impl Range {
    /// The grid over every position (x, y, z) of `lists`; `None` when one
    /// isn't finite.
    fn of(lists: &[&[f32]]) -> Option<Range> {
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in lists.iter().flat_map(|d| d.as_chunks::<3>().0) {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        if !min.iter().chain(&max).all(|v| v.is_finite()) {
            return None;
        }
        let side = (0..3).map(|k| max[k] - min[k]).fold(0.0f32, f32::max);
        let step = if side > 0.0 { side / 65535.0 } else { 1.0 };
        let mut range = Range {
            corner: min,
            step,
            error: 0.0,
        };
        for d in lists {
            for p in d.as_chunks::<3>().0 {
                for k in 0..3 {
                    let q = range.step_of(p[k], k);
                    range.error = range.error.max((f32::from(q) * step + min[k] - p[k]).abs());
                }
            }
        }
        Some(range)
    }

    fn step_of(&self, v: f32, axis: usize) -> u16 {
        ((v - self.corner[axis]) / self.step).round().clamp(0.0, 65535.0) as u16
    }

    /// Positions as steps, three a position.
    fn quantize(&self, data: &[f32]) -> Vec<u16> {
        data.as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [0, 1, 2].map(|k| self.step_of(p[k], k)))
            .collect()
    }
}

/// Points position accessor `accessor` at `q` (three steps a position),
/// 8 bytes a position.
fn write_quantized(doc: &mut Doc, accessor: usize, q: &[u16]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(q.len() / 3 * 8);
    let (mut lo, mut hi) = ([u16::MAX; 3], [0u16; 3]);
    for p in q.as_chunks::<3>().0 {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
            bytes.extend_from_slice(&p[k].to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
    }
    let view = doc.add_view(bytes, Some(8), 34962);
    let a = doc.json["accessors"][accessor]
        .as_object_mut()
        .ok_or("an accessor isn't an object")?;
    a.remove("byteOffset");
    a.remove("normalized");
    a.insert("bufferView".into(), json!(view));
    a.insert("componentType".into(), json!(5123));
    a.insert("min".into(), json!(lo));
    a.insert("max".into(), json!(hi));
    Ok(())
}

/// Takes node `n`'s mesh off it and gives it `children` (the nodes that
/// show the mesh instead).
fn hang(doc: &mut Doc, n: usize, children: Vec<Value>) -> Result<(), String> {
    let list = doc.json["nodes"].as_array_mut().ok_or("no nodes")?;
    let first = list.len();
    let count = children.len();
    list.extend(children);
    let node = doc.json["nodes"][n].as_object_mut().ok_or("a node isn't an object")?;
    node.remove("mesh");
    let at = node.entry("children").or_insert_with(|| json!([]));
    at.as_array_mut()
        .ok_or("a node's children aren't a list")?
        .extend((first..first + count).map(|i| json!(i)));
    Ok(())
}

/// Unit normals as 16-bit normalized integers.
fn quantize_normals(data: &[u8]) -> Vec<u8> {
    data.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|c| {
            let f = f32::from_le_bytes(*c).clamp(-1.0, 1.0);
            ((f * 32767.0).round() as i16).to_le_bytes()
        })
        .collect()
}

/// Elements of `elem` bytes padded to a multiple of 4 (vertex attributes
/// start on 4-byte boundaries), and the stride when there is padding.
fn pad_elements(data: &[u8], elem: usize) -> (Vec<u8>, Option<usize>) {
    let stride = elem.next_multiple_of(4);
    if stride == elem || elem == 0 {
        return (data.to_vec(), None);
    }
    let mut out = Vec::with_capacity(data.len() / elem * stride);
    for e in data.chunks_exact(elem) {
        out.extend_from_slice(e);
        out.resize(out.len() + stride - elem, 0);
    }
    (out, Some(stride))
}

/// Texture coordinates all in 0..1 as 16-bit unsigned normalized integers;
/// `None` if any is outside (a tiled texture: kept as floats).
fn quantize_unit(data: &[u8]) -> Option<Vec<u8>> {
    let floats: Vec<f32> = data.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    if floats.iter().any(|f| !(0.0..=1.0).contains(f)) {
        return None;
    }
    Some(
        floats
            .iter()
            .flat_map(|f| ((f * 65535.0).round() as u16).to_le_bytes())
            .collect(),
    )
}

/// Each component's least and greatest value.
fn bounds(data: &[u8], components: usize) -> (Vec<f32>, Vec<f32>) {
    let mut min = vec![f32::INFINITY; components];
    let mut max = vec![f32::NEG_INFINITY; components];
    for (i, c) in data.as_chunks::<4>().0.iter().enumerate() {
        let f = f32::from_le_bytes(*c);
        min[i % components] = min[i % components].min(f);
        max[i % components] = max[i % components].max(f);
    }
    if data.is_empty() {
        return (vec![0.0; components], vec![0.0; components]);
    }
    (min, max)
}

/// A GLB file's JSON and binary chunk.
fn split_glb(bytes: &[u8]) -> Result<(Value, Option<Vec<u8>>), String> {
    let word = |at: usize| -> Result<usize, String> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .ok_or("its GLB header is cut short")?
                .try_into()
                .expect("4 bytes"),
        ) as usize)
    };
    if word(4)? != 2 {
        return Err("only GLB version 2 is read".into());
    }
    let total = word(8)?.min(bytes.len());
    let mut at = 12;
    let (mut json, mut bin) = (None, None);
    while at + 8 <= total {
        let len = word(at)?;
        let kind = bytes.get(at + 4..at + 8).ok_or("a GLB chunk is cut short")?;
        let body = bytes.get(at + 8..at + 8 + len).ok_or("a GLB chunk is cut short")?;
        match kind {
            b"JSON" => json = Some(serde_json::from_slice(body).map_err(|e| format!("not a glTF file: {e}"))?),
            b"BIN\0" if bin.is_none() => bin = Some(body.to_vec()),
            _ => {}
        }
        at += 8 + len;
    }
    Ok((json.ok_or("its GLB has no JSON chunk")?, bin))
}

/// Writes the document as one GLB: every view still used packed into one
/// buffer, in view order, 4-byte aligned.
fn write_glb(mut doc: Doc) -> Result<Vec<u8>, String> {
    let views = doc.json["bufferViews"].as_array().map_or(0, Vec::len);
    let mut used = vec![false; views];
    let mut mark = |v: &Value| {
        if let Some(u) = v.as_u64().and_then(|i| used.get_mut(i as usize)) {
            *u = true;
        }
    };
    for a in doc.json["accessors"].as_array().into_iter().flatten() {
        mark(&a["bufferView"]);
        mark(&a["sparse"]["indices"]["bufferView"]);
        mark(&a["sparse"]["values"]["bufferView"]);
    }
    for image in doc.json["images"].as_array().into_iter().flatten() {
        mark(&image["bufferView"]);
    }
    let mut bin = Vec::new();
    let mut new_index = vec![None; views];
    let mut new_views = Vec::new();
    for (i, used) in used.iter().enumerate() {
        if !used {
            continue;
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let data = doc.view(i)?.to_vec();
        let mut view = doc.json["bufferViews"][i].clone();
        view["buffer"] = json!(0);
        view["byteOffset"] = json!(bin.len());
        view["byteLength"] = json!(data.len());
        bin.extend_from_slice(&data);
        new_index[i] = Some(new_views.len());
        new_views.push(view);
    }
    let renumber = |v: &mut Value| {
        if let Some(i) = v.as_u64() {
            *v = json!(new_index[i as usize].expect("a used view"));
        }
    };
    for a in doc
        .json
        .get_mut("accessors")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        renumber(&mut a["bufferView"]);
        if a.get("sparse").is_some() {
            renumber(&mut a["sparse"]["indices"]["bufferView"]);
            renumber(&mut a["sparse"]["values"]["bufferView"]);
        }
    }
    for image in doc
        .json
        .get_mut("images")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if image.get("bufferView").is_some() {
            renumber(&mut image["bufferView"]);
        }
    }
    // Accessors without views (all zeros) stay as they are.
    for a in doc
        .json
        .get_mut("accessors")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if a["bufferView"].is_null()
            && let Some(o) = a.as_object_mut()
        {
            o.remove("bufferView");
        }
    }
    doc.json["bufferViews"] = Value::Array(new_views);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    doc.json["buffers"] = if bin.is_empty() {
        json!([])
    } else {
        json!([{ "byteLength": bin.len() }])
    };
    encode_glb(&doc.json, &bin)
}

fn encode_glb(json: &Value, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut text = serde_json::to_vec(json).map_err(|e| e.to_string())?;
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
    }
    Ok(out)
}

/// Decodes standard base64.
fn base64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' => continue,
            _ => return Err("its base64 data is broken".into()),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests;
