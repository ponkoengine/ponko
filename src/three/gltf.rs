//! Loading glTF 2.0 files (`.glb`, or `.gltf` with its buffers and images
//! next to it) into one [`MeshData`] with a part per material.
//!
//! Every mesh in the file's default scene is placed where its node puts it
//! and merged, so a file is one [`Mesh`](crate::render::Mesh): a tree, a house, a
//! car. A file with skins keeps its nodes as the joints of a skeleton:
//! skinned meshes stay in the space they were bound in, and other meshes
//! follow their node rigidly. Animation clips retain local joint tracks.

use std::collections::HashMap;
use std::ops::Range;

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};

use super::gltf_ext::{self, ModelCamera, ModelLight, UvMapping};
use super::{Blend, Joint, MeshData, MorphTarget, Transform3D};
use crate::color::Color;

/// A loaded file, before it goes to the GPU.
pub(crate) struct Loaded {
    pub data: MeshData,
    /// Index ranges of `data`, each with its material (an index into `materials`).
    pub parts: Vec<(Range<u32>, usize)>,
    pub materials: Vec<LoadedMaterial>,
    /// The materials' images: width, height, RGBA8 pixels (none when the
    /// image is in `blocks`).
    pub images: Vec<Image>,
    /// Each image that stays in a GPU block format (a KTX2 image the GPU
    /// samples), uploaded as it is instead of its RGBA8.
    pub blocks: Vec<Option<Blocks>>,
    /// Where each image came from, when that's files (asset names, or
    /// `orm:` plus two of them for a merged image), so files sharing an
    /// image upload it once. One the caller said it has is left empty.
    pub image_sources: Vec<Option<String>>,
    /// With skins: the file's nodes as joints, and what `data.joints`
    /// index (each slot's joint and inverse bind matrix).
    pub skin: Option<LoadedSkin>,
    pub clips: Vec<super::AnimationClip>,
    /// The weights `data.morph_targets` show by default (glTF's mesh
    /// weights), one per target.
    pub morph_defaults: Vec<f32>,
    /// Lights (`KHR_lights_punctual`) and cameras its scene places.
    pub lights: Vec<ModelLight>,
    pub cameras: Vec<ModelCamera>,
    /// What loading it left out (optional extensions it doesn't read, and
    /// the like), each already reported.
    #[cfg_attr(not(test), expect(dead_code, reason = "reported as the file loads; tests read them"))]
    pub warnings: Vec<String>,
}

pub(crate) struct LoadedSkin {
    pub joints: std::sync::Arc<[Joint]>,
    pub slots: Vec<u16>,
    pub inverse_binds: Vec<Mat4>,
}

/// The joints of a file with skins while its meshes are read: every node
/// of the scene, parents first.
struct Rig {
    joints: Vec<Joint>,
    /// Joint index of each node of the file (`u16::MAX`: not in the scene).
    of_node: Vec<u16>,
    /// Each joint's matrix at rest, in the file's space.
    rest: Vec<Mat4>,
    /// The slots so far: joint and inverse bind matrix.
    slots: Vec<(u16, Mat4)>,
}

impl Rig {
    fn new(gltf: &gltf::Document, scene: &gltf::Scene, name: &str) -> Result<Self, String> {
        let mut rig = Self {
            joints: Vec::new(),
            of_node: vec![u16::MAX; gltf.nodes().len()],
            rest: Vec::new(),
            slots: Vec::new(),
        };
        let mut stack: Vec<(gltf::Node, Option<usize>)> = scene.nodes().map(|n| (n, None)).collect();
        stack.reverse();
        while let Some((node, parent)) = stack.pop() {
            if rig.joints.len() >= usize::from(u16::MAX) {
                return Err(format!("{name}: more than 65535 nodes in a skinned file"));
            }
            let (t, r, s) = node.transform().decomposed();
            let rest = Transform3D {
                position: Vec3::from(t),
                rotation: Quat::from_array(r),
                scale: Vec3::from(s),
            };
            let at = rig.joints.len();
            rig.of_node[node.index()] = at as u16;
            let matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
            rig.rest.push(parent.map_or(matrix, |p| rig.rest[p] * matrix));
            rig.joints.push(
                Joint::new(
                    node.name()
                        .map_or_else(|| format!("node {}", node.index()), str::to_string),
                    parent,
                )
                .with_rest(rest),
            );
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|c| (c, Some(at))));
        }
        Ok(rig)
    }

    /// The slot of `joint` bound with `inverse_bind`, made if it's new.
    fn slot(&mut self, joint: u16, inverse_bind: Mat4) -> Result<u16, String> {
        if let Some(i) = self.slots.iter().position(|s| *s == (joint, inverse_bind)) {
            return Ok(i as u16);
        }
        if self.slots.len() >= usize::from(u16::MAX) {
            return Err("more than 65535 joint bindings".into());
        }
        self.slots.push((joint, inverse_bind));
        Ok(self.slots.len() as u16 - 1)
    }
}

/// How one primitive's corners get their joints.
enum Binding {
    /// No skins in the file.
    None,
    /// Rigidly on one slot.
    Rigid(u16),
    /// Its skin's joints, as slots.
    Skin(Vec<u16>),
}

/// Width, height and RGBA8 pixels.
pub(crate) type Image = (u32, u32, Vec<u8>);

/// The GPU block formats a model's KTX2 images (`KHR_texture_basisu`) stay
/// in, so they upload as they are, with their mipmaps, never decoded to
/// RGBA8: the GPU's (`Graphics::compressed_formats`). The default (none)
/// decodes every image.
#[cfg(feature = "compressed-textures")]
pub(crate) use crate::compressed::GpuFormats;

/// Without `compressed-textures` every image is decoded.
#[cfg(not(feature = "compressed-textures"))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuFormats {}

/// An image kept in a GPU block format.
#[cfg(feature = "compressed-textures")]
pub(crate) type Blocks = crate::compressed::CompressedImage;

/// Without `compressed-textures` there are none.
#[cfg(not(feature = "compressed-textures"))]
#[derive(Debug)]
pub(crate) enum Blocks {}

pub(crate) struct LoadedMaterial {
    /// Its name in the file (empty without one).
    pub name: String,
    pub color: Color,
    pub roughness: f32,
    pub metallic: f32,
    pub emissive: Color,
    /// Images, as indices into `Loaded::images`.
    pub image: Option<usize>,
    pub normal_map: Option<usize>,
    pub normal_strength: f32,
    /// Occlusion in red, roughness in green, metalness in blue.
    pub orm_map: Option<usize>,
    pub emissive_map: Option<usize>,
    /// glTF's alpha mode: OPAQUE, MASK (cut out) or BLEND (transparent).
    pub blend: Blend,
    pub double_sided: bool,
}

impl LoadedMaterial {
    fn plain() -> Self {
        Self {
            name: String::new(),
            color: Color::WHITE,
            roughness: 0.6,
            metallic: 0.0,
            emissive: Color::rgb(0.0, 0.0, 0.0),
            image: None,
            normal_map: None,
            normal_strength: 1.0,
            orm_map: None,
            emissive_map: None,
            blend: Blend::Opaque,
            double_sided: false,
        }
    }
}

/// Parses glTF `bytes`; `read` fetches files it refers to, by asset name.
/// Images from files that `have` says are loaded already aren't read (they
/// come back 0×0, with their source). Every image is decoded to RGBA8;
/// [`load_for`] keeps the ones a GPU samples as they are.
pub(crate) fn load(
    name: &str,
    bytes: &[u8],
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    have: &dyn Fn(&str) -> bool,
) -> Result<Loaded, String> {
    load_for(name, bytes, read, have, GpuFormats::default())
}

std::thread_local! {
    /// Images of the file being parsed on this thread that were decoded
    /// before the parse, by image index ([`load_with_images`]).
    static DECODED: std::cell::RefCell<HashMap<usize, Image>> = std::cell::RefCell::new(HashMap::new());
}

/// [`load_for`], with some of the file's images decoded already (to RGBA8,
/// as `decode_image` does), by image index: a streamed mesh decodes them a
/// slice at a time first where there are no worker threads.
pub(crate) fn load_with_images(
    name: &str,
    bytes: &[u8],
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    have: &dyn Fn(&str) -> bool,
    gpu: GpuFormats,
    decoded: HashMap<usize, Image>,
) -> Result<Loaded, String> {
    DECODED.with(|d| *d.borrow_mut() = decoded);
    let loaded = load_for(name, bytes, read, have, gpu);
    DECODED.with(|d| d.borrow_mut().clear());
    loaded
}

/// [`load`], with KTX2 images in a block format of `gpu` kept as they are
/// (in [`Loaded::blocks`]).
pub(crate) fn load_for(
    name: &str,
    bytes: &[u8],
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    have: &dyn Fn(&str) -> bool,
    gpu: GpuFormats,
) -> Result<Loaded, String> {
    check_glb_length(name, bytes)?;
    let gltf_ext::Prepared {
        document: gltf,
        buffers,
        json,
        preferred_image,
        dequantize,
        mut warnings,
    } = gltf_ext::prepare(name, bytes, read)?;
    check_accessors(&gltf).map_err(|e| format!("{name}: {e}"))?;
    check_node_tree(&gltf).map_err(|e| format!("{name}: {e}"))?;
    let FileMaterials {
        mut materials,
        mappings,
        images,
        blocks,
        image_sources,
    } = read_materials(
        &gltf,
        &buffers,
        &json,
        &preferred_image,
        name,
        (have, gpu),
        read,
        &mut warnings,
    )?;
    let default_material = materials.len();

    // Every primitive in the scene, placed by its node, grouped by material.
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or_else(|| format!("{name}: has no scene"))?;
    let mut rig = if gltf.skins().len() > 0 || gltf.animations().len() > 0 {
        Some(Rig::new(&gltf, &scene, name)?)
    } else {
        None
    };
    let mut by_material: Vec<(usize, MeshData)> = Vec::new();
    // Morph targets' default weights, by name.
    let mut defaults: Vec<(String, f32)> = Vec::new();
    let mut lights = Vec::new();
    let mut cameras = Vec::new();
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let mut matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let children = matrix;
        lights.extend(gltf_ext::light_of(&json, node.index(), matrix, node.name()));
        cameras.extend(gltf_ext::camera_of(&node, matrix));
        if let Some(mesh) = node.mesh() {
            // A skinned mesh is in the space it was bound in, whatever its
            // node; any other follows its node.
            let binding = match (&mut rig, node.skin()) {
                (None, _) => Binding::None,
                (Some(rig), Some(skin)) => {
                    matrix = Mat4::IDENTITY;
                    let reader = skin.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                    let inverse: Vec<Mat4> = reader
                        .read_inverse_bind_matrices()
                        .map(|m| m.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                        .unwrap_or_default();
                    let mut slots = Vec::new();
                    for (i, joint) in skin.joints().enumerate() {
                        let j = rig.of_node[joint.index()];
                        if j == u16::MAX {
                            return Err(format!("{name}: a skin's joint isn't in the scene"));
                        }
                        let ib = inverse.get(i).copied().unwrap_or(Mat4::IDENTITY);
                        slots.push(rig.slot(j, ib).map_err(|e| format!("{name}: {e}"))?);
                    }
                    Binding::Skin(slots)
                }
                (Some(rig), None) => {
                    let j = rig.of_node[node.index()];
                    let ib = rig.rest[usize::from(j)].inverse();
                    Binding::Rigid(rig.slot(j, ib).map_err(|e| format!("{name}: {e}"))?)
                }
            };
            let targets = target_names(&mesh);
            for (k, target) in targets.iter().enumerate() {
                if !defaults.iter().any(|(n, _)| n == target) {
                    let weight = node
                        .weights()
                        .or_else(|| mesh.weights())
                        .and_then(|w| w.get(k))
                        .copied()
                        .unwrap_or(0.0);
                    defaults.push((target.clone(), weight));
                }
            }
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue; // lines and points aren't drawn
                }
                // Positions `ponko cook` quantized: back to their size first.
                let matrix = matrix * dequantization(&dequantize, &mesh, &primitive);
                let material = primitive.material().index().unwrap_or(default_material);
                let at = match by_material.iter().position(|(m, _)| *m == material) {
                    Some(i) => i,
                    None => {
                        by_material.push((material, MeshData::default()));
                        by_material.len() - 1
                    }
                };
                let part = Part {
                    matrix,
                    binding: &binding,
                    targets: &targets,
                    mapping: mappings.get(material).copied().unwrap_or(UvMapping::PLAIN),
                };
                append(&mut by_material[at].1, &primitive, &buffers, &part, name)?;
            }
        }
        stack.extend(node.children().map(|c| (c, children)));
    }
    if by_material.is_empty() {
        return Err(format!("{name}: has no triangles"));
    }
    if by_material.iter().any(|(m, _)| *m == default_material) {
        materials.push(LoadedMaterial::plain());
    }

    let (mut data, parts) = merge_by_material(by_material);
    // Importer-authored in-between frames are ordinary glTF targets plus
    // a channel mapping in mesh extras. Resolve indices after name merging.
    for mesh in gltf.meshes() {
        let names = target_names(&mesh);
        let extras = mesh
            .extras()
            .as_ref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok());
        if let Some(frames) = extras
            .as_ref()
            .and_then(|v| v.get("ponkoMorphFrames"))
            .and_then(|v| v.as_array())
        {
            for f in frames {
                let parse = || -> Option<super::MorphFrame> {
                    let channel = names.get(f["channel"].as_u64()? as usize)?;
                    let target = names.get(f["target"].as_u64()? as usize)?;
                    Some(super::MorphFrame {
                        channel: data.morph_targets.iter().position(|t| t.name == *channel)?,
                        target: data.morph_targets.iter().position(|t| t.name == *target)?,
                        weight: f["weight"].as_f64()? as f32,
                    })
                };
                let frame = parse().ok_or("invalid morph frame extras")?;
                if !data.morph_frames.contains(&frame) {
                    data.morph_frames.push(frame);
                }
            }
        }
    }
    data.problem().map_err(|e| format!("{name}: {e}"))?;
    let morph_defaults = data
        .morph_targets
        .iter()
        .map(|t| defaults.iter().find(|(n, _)| *n == t.name).map_or(0.0, |d| d.1))
        .collect();
    let clips = match &rig {
        Some(rig) => read_clips(&gltf, &buffers, rig, &data)?,
        None => Vec::new(),
    };
    let skin = rig.map(|rig| LoadedSkin {
        joints: clips
            .first()
            .map_or_else(|| rig.joints.into(), |c| c.source_joints().clone()),
        slots: rig.slots.iter().map(|s| s.0).collect(),
        inverse_binds: rig.slots.iter().map(|s| s.1).collect(),
    });
    for warning in &warnings {
        gltf_ext::warn(warning);
    }
    Ok(Loaded {
        data,
        parts,
        materials,
        images,
        blocks,
        image_sources,
        skin,
        clips,
        morph_defaults,
        lights,
        cameras,
        warnings,
    })
}

/// One node of a file's scene, read on its own by [`load_nodes`].
pub(crate) struct LoadedNode {
    /// Its name in the file.
    pub name: Option<String>,
    /// Its parent, as an index into [`LoadedNodes::nodes`] (parents come
    /// before their children).
    pub parent: Option<usize>,
    /// Where it is in its parent's space.
    pub local: Transform3D,
    /// Its `extras`, as the file has them (any JSON).
    pub extras: Option<serde_json::Value>,
    /// Its own mesh in its own space (children's meshes are theirs), each
    /// part with its material; `None` without triangles.
    pub mesh: Option<NodeMesh>,
    /// Its light (`KHR_lights_punctual`), relative to the node.
    pub light: Option<ModelLight>,
    /// Whether its mesh is skinned (read in its bind pose).
    pub skinned: bool,
    /// An earlier node (an index into [`LoadedNodes::nodes`]) with the
    /// same mesh: the file's same glTF mesh, with the same morph weights.
    /// Its [`mesh`](Self::mesh) is a copy of that node's, so one uploaded
    /// mesh serves both.
    pub same_mesh_as: Option<usize>,
}

/// The mesh of one node, from [`load_nodes`].
pub(crate) struct NodeMesh {
    pub data: MeshData,
    /// Index ranges of `data`, each with its material (an index into
    /// [`LoadedNodes::materials`]).
    pub parts: Vec<(Range<u32>, usize)>,
    /// Its morph targets' default weights.
    pub morph_defaults: Vec<f32>,
}

/// A file read node by node, by [`load_nodes`].
pub(crate) struct LoadedNodes {
    /// Every node of the file's default scene: depth first, parents first,
    /// children in the file's order.
    pub nodes: Vec<LoadedNode>,
    pub materials: Vec<LoadedMaterial>,
    pub images: Vec<Image>,
    pub blocks: Vec<Option<Blocks>>,
    pub image_sources: Vec<Option<String>>,
}

/// Parses glTF `bytes` as [`load`] does, but keeps every node of the
/// default scene apart: its place in the tree, its transform, its extras
/// and its own mesh in its own space, instead of one merged mesh.
/// Skinned meshes are read in their bind pose, without joints. KTX2 images
/// in a block format of `gpu` are kept as they are, as in [`load_for`].
pub(crate) fn load_nodes(
    name: &str,
    bytes: &[u8],
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    have: &dyn Fn(&str) -> bool,
    gpu: GpuFormats,
) -> Result<LoadedNodes, String> {
    check_glb_length(name, bytes)?;
    let gltf_ext::Prepared {
        document: gltf,
        buffers,
        json,
        preferred_image,
        dequantize,
        mut warnings,
    } = gltf_ext::prepare(name, bytes, read)?;
    check_accessors(&gltf).map_err(|e| format!("{name}: {e}"))?;
    check_node_tree(&gltf).map_err(|e| format!("{name}: {e}"))?;
    let FileMaterials {
        mut materials,
        mappings,
        images,
        blocks,
        image_sources,
    } = read_materials(
        &gltf,
        &buffers,
        &json,
        &preferred_image,
        name,
        (have, gpu),
        read,
        &mut warnings,
    )?;
    let default_material = materials.len();
    let mut uses_default = false;
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or_else(|| format!("{name}: has no scene"))?;
    let mut nodes: Vec<LoadedNode> = Vec::new();
    // The first node of each glTF mesh (with its node's own morph weights,
    // which override the mesh's): later nodes copy its mesh.
    let mut first_of_mesh: HashMap<(usize, Option<Vec<u32>>), usize> = HashMap::new();
    let mut stack: Vec<(gltf::Node, Option<usize>)> = scene.nodes().map(|n| (n, None)).collect();
    stack.reverse();
    while let Some((node, parent)) = stack.pop() {
        let (t, r, s) = node.transform().decomposed();
        let local = Transform3D {
            position: Vec3::from(t),
            rotation: Quat::from_array(r).normalize(),
            scale: Vec3::from(s),
        };
        let extras = node
            .extras()
            .as_ref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok());
        let mut mesh = None;
        let mut same_mesh_as = None;
        let key = node.mesh().map(|m| {
            (
                m.index(),
                node.weights().map(|w| w.iter().map(|x| x.to_bits()).collect()),
            )
        });
        if let Some(&first) = key.as_ref().and_then(|k| first_of_mesh.get(k)) {
            mesh = nodes[first].mesh.as_ref().map(|m| NodeMesh {
                data: m.data.clone(),
                parts: m.parts.clone(),
                morph_defaults: m.morph_defaults.clone(),
            });
            same_mesh_as = mesh.as_ref().map(|_| first);
        } else if let Some(m) = node.mesh() {
            let targets = target_names(&m);
            let mut by_material: Vec<(usize, MeshData)> = Vec::new();
            for primitive in m.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let material = primitive.material().index().unwrap_or(default_material);
                uses_default |= material == default_material;
                let at = match by_material.iter().position(|(mat, _)| *mat == material) {
                    Some(i) => i,
                    None => {
                        by_material.push((material, MeshData::default()));
                        by_material.len() - 1
                    }
                };
                let part = Part {
                    matrix: dequantization(&dequantize, &m, &primitive),
                    binding: &Binding::None,
                    targets: &targets,
                    mapping: mappings.get(material).copied().unwrap_or(UvMapping::PLAIN),
                };
                append(&mut by_material[at].1, &primitive, &buffers, &part, name)?;
            }
            if !by_material.is_empty() {
                let (data, parts) = merge_by_material(by_material);
                let label = node.name().unwrap_or("a node");
                data.problem().map_err(|e| format!("{name}: {label}: {e}"))?;
                let weights = node.weights().or_else(|| m.weights());
                let morph_defaults = data
                    .morph_targets
                    .iter()
                    .map(|t| {
                        let k = targets.iter().position(|n| *n == t.name);
                        k.and_then(|k| weights.and_then(|w| w.get(k))).copied().unwrap_or(0.0)
                    })
                    .collect();
                mesh = Some(NodeMesh {
                    data,
                    parts,
                    morph_defaults,
                });
            }
        }
        nodes.push(LoadedNode {
            name: node.name().map(str::to_string),
            parent,
            local,
            extras,
            mesh,
            light: gltf_ext::light_of(&json, node.index(), Mat4::IDENTITY, node.name()),
            skinned: node.skin().is_some(),
            same_mesh_as,
        });
        let at = nodes.len() - 1;
        if let Some(key) = key {
            first_of_mesh.entry(key).or_insert(at);
        }
        let children: Vec<_> = node.children().collect();
        stack.extend(children.into_iter().rev().map(|c| (c, Some(at))));
    }
    if uses_default {
        materials.push(LoadedMaterial::plain());
    }
    for warning in &warnings {
        gltf_ext::warn(warning);
    }
    Ok(LoadedNodes {
        nodes,
        materials,
        images,
        blocks,
        image_sources,
    })
}

/// One mesh of meshes grouped by material, with each group's index range
/// and material.
fn merge_by_material(by_material: Vec<(usize, MeshData)>) -> (MeshData, Vec<(Range<u32>, usize)>) {
    let mut data = MeshData::default();
    let mut parts = Vec::with_capacity(by_material.len());
    for (material, part) in by_material {
        let base = data.positions.len() as u32;
        let first = data.indices.len() as u32;
        data.positions.extend(part.positions);
        data.normals.extend(part.normals);
        data.uvs.extend(part.uvs);
        data.colors.extend(part.colors);
        data.uvs1.extend(part.uvs1);
        data.joints.extend(part.joints);
        data.weights.extend(part.weights);
        merge_targets(
            &mut data.morph_targets,
            base as usize,
            part.morph_targets,
            data.positions.len(),
        );
        data.indices.extend(part.indices.iter().map(|i| i + base));
        parts.push((first..data.indices.len() as u32, material));
    }
    (data, parts)
}

/// A glTF file's materials, how their textures lie on meshes, and their
/// decoded images (see [`Loaded`]).
struct FileMaterials {
    materials: Vec<LoadedMaterial>,
    /// How each material's textures lie on its meshes (`KHR_texture_transform`).
    mappings: Vec<UvMapping>,
    images: Vec<Image>,
    blocks: Vec<Option<Blocks>>,
    image_sources: Vec<Option<String>>,
}

/// Reads every material of `gltf`, decoding each image it uses once (or
/// keeping it in its block format, when `gpu` samples that).
#[allow(clippy::too_many_arguments)]
fn read_materials(
    gltf: &gltf::Document,
    buffers: &[Vec<u8>],
    json: &serde_json::Value,
    preferred_image: &[Option<usize>],
    name: &str,
    (have, gpu): (&dyn Fn(&str) -> bool, GpuFormats),
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    warnings: &mut Vec<String>,
) -> Result<FileMaterials, String> {
    let folder = folder_of(name);

    // Materials, with each image decoded once.
    let mut images = Images {
        decoded: Vec::new(),
        blocks: Vec::new(),
        gpu,
        keys: Vec::new(),
        sources: Vec::new(),
        have,
        document: gltf,
        buffers,
        folder,
        name,
    };
    let mut materials: Vec<LoadedMaterial> = Vec::new();
    // How each material's textures lie on its meshes (`KHR_texture_transform`).
    let mut mappings: Vec<UvMapping> = Vec::new();
    for material in gltf.materials() {
        let pbr = material.pbr_metallic_roughness();
        let [r, g, b, a] = pbr.base_color_factor();
        let extensions = material
            .index()
            .map(|i| gltf_ext::material_extensions(json, i, name, warnings));
        let strength = extensions.as_ref().map_or(1.0, |e| e.emissive_strength);
        let [er, eg, eb] = material.emissive_factor().map(|c| c * strength);
        let transmission = extensions.as_ref().map_or(0.0, |e| e.transmission);
        mappings.push(extensions.map_or(UvMapping::PLAIN, |e| e.mapping));
        let mut get = |t: Option<gltf::Texture>| match t {
            Some(t) => images.texture(&t, preferred_image, warnings, read).map(Some),
            None => Ok(None),
        };
        let image = get(pbr.base_color_texture().map(|i| i.texture()))?;
        let emissive_map = get(material.emissive_texture().map(|i| i.texture()))?;
        let normal = material.normal_texture();
        let normal_strength = normal.as_ref().map_or(1.0, |n| n.scale());
        let normal_map = get(normal.map(|n| n.texture()))?;
        // glTF keeps roughness in green and metalness in blue, and
        // occlusion in red of its own image, often the same one.
        let mr = pbr.metallic_roughness_texture().map(|i| i.texture().source().index());
        let occlusion = material.occlusion_texture().map(|i| i.texture().source().index());
        let orm_map = match (mr, occlusion) {
            (None, None) => None,
            (Some(m), Some(o)) if m == o => Some(images.get(m, read)?),
            (m, o) => Some(images.orm(m, o, read)?),
        };
        // glTF factors are linear; Color is sRGB. Transmission
        // (`KHR_materials_transmission`) is drawn as see-through blending:
        // what passes through isn't covered.
        let blend = match material.alpha_mode() {
            _ if transmission > 0.0 => Blend::Transparent,
            gltf::material::AlphaMode::Opaque => Blend::Opaque,
            gltf::material::AlphaMode::Mask => Blend::Cutout,
            gltf::material::AlphaMode::Blend => Blend::Transparent,
        };
        materials.push(LoadedMaterial {
            name: material.name().unwrap_or("").to_string(),
            color: Color::rgba(to_srgb(r), to_srgb(g), to_srgb(b), a * (1.0 - transmission)),
            roughness: pbr.roughness_factor(),
            metallic: pbr.metallic_factor(),
            emissive: Color::rgb(to_srgb(er), to_srgb(eg), to_srgb(eb)),
            image,
            normal_map,
            normal_strength,
            orm_map,
            emissive_map,
            blend,
            double_sided: material.double_sided(),
        });
    }
    Ok(FileMaterials {
        materials,
        mappings,
        image_sources: images.sources,
        images: images.decoded,
        blocks: images.blocks,
    })
}

/// The names of `mesh`'s morph targets: from its extras' `targetNames`
/// (the usual way exporters name them), else `<mesh>.<i>`.
fn target_names(mesh: &gltf::Mesh) -> Vec<String> {
    let count = mesh.primitives().map(|p| p.morph_targets().len()).max().unwrap_or(0);
    let named: Vec<String> = mesh
        .extras()
        .as_ref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok())
        .and_then(|v| {
            let names = v.get("targetNames")?.as_array()?;
            names.iter().map(|n| n.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default();
    let mesh_name = mesh
        .name()
        .map_or_else(|| format!("mesh {}", mesh.index()), str::to_string);
    (0..count)
        .map(|i| named.get(i).cloned().unwrap_or_else(|| format!("{mesh_name}.{i}")))
        .collect()
}

/// Adds the morph targets of corners `before..` of a mesh (`new`, one
/// position offset per such corner, normal offsets optional) to its
/// targets `into` (each with `before` offsets), matched by name; then pads
/// every target with zero offsets to `corners`, so corners without a
/// target don't move.
fn merge_targets(into: &mut Vec<MorphTarget>, before: usize, new: Vec<MorphTarget>, corners: usize) {
    for target in new {
        let at = match into.iter().position(|t| t.name == target.name) {
            Some(at) => at,
            None => {
                into.push(
                    MorphTarget::new(target.name.clone(), vec![Vec3::ZERO; before])
                        .with_normals(vec![Vec3::ZERO; before]),
                );
                into.len() - 1
            }
        };
        let t = &mut into[at];
        t.positions.extend(target.positions);
        if target.normals.is_empty() {
            t.normals.resize(t.positions.len(), Vec3::ZERO);
        } else {
            t.normals.extend(target.normals);
        }
    }
    for t in into {
        t.positions.resize(corners, Vec3::ZERO);
        t.normals.resize(corners, Vec3::ZERO);
    }
}

fn read_clips(
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    rig: &Rig,
    data: &MeshData,
) -> Result<Vec<super::AnimationClip>, String> {
    use super::{AnimationClip, AnimationTrack, Interpolation, JointAnimation, Skeleton};
    use gltf::animation::util::ReadOutputs;
    let source = Skeleton::try_new(rig.joints.clone())?;
    document
        .animations()
        .map(|animation| {
            let mut tracks: Vec<JointAnimation> = Vec::new();
            let mut morphs = Vec::new();
            let extras = animation
                .extras()
                .as_ref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw.get()).ok());
            let animated = extras
                .as_ref()
                .and_then(|e| e.get("ponkoAnimatedTargets"))
                .and_then(|v| v.as_array());
            for channel in animation.channels() {
                // gltf-json leaves channel targets unchecked, and the gltf
                // crate unwraps them: check the node, and skip paths of
                // extensions this loader doesn't read (such as "pointer").
                let target = &document.as_json().animations[animation.index()].channels[channel.index()].target;
                if target.node.value() >= document.nodes().len() {
                    return Err(format!(
                        "animation {}: a channel targets node {}, which doesn't exist",
                        animation.index(),
                        target.node.value()
                    ));
                }
                if !matches!(target.path, gltf::json::validation::Checked::Valid(_)) {
                    continue;
                }
                let joint = rig.of_node[channel.target().node().index()];
                if joint == u16::MAX {
                    continue;
                }
                let joint = usize::from(joint);
                let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let times: Vec<f32> = reader.read_inputs().ok_or("animation has no key times")?.collect();
                let interpolation = match channel.sampler().interpolation() {
                    gltf::animation::Interpolation::Step => Interpolation::Step,
                    gltf::animation::Interpolation::Linear => Interpolation::Linear,
                    gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
                };
                let (property, values) = match reader.read_outputs().ok_or("animation has no values")? {
                    ReadOutputs::Translations(v) => (0, v.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::Rotations(v) => (1, v.into_f32().map(glam::Vec4::from).collect()),
                    ReadOutputs::Scales(v) => (2, v.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::MorphTargetWeights(v) => {
                        let mesh = channel.target().node().mesh().ok_or("weight channel has no mesh")?;
                        let names = target_names(&mesh);
                        let values: Vec<f32> = v.into_f32().collect();
                        let multiple = if interpolation == Interpolation::CubicSpline {
                            3
                        } else {
                            1
                        };
                        if names.is_empty() || values.len() != times.len() * multiple * names.len() {
                            return Err("invalid morph animation value count".into());
                        }
                        for (k, name) in names.iter().enumerate() {
                            if animated.is_some_and(|a| !a.iter().any(|v| v.as_str() == Some(name))) {
                                continue;
                            }
                            if !data.morph_targets.iter().any(|t| t.name == *name) {
                                continue;
                            }
                            let track = AnimationTrack::try_new(
                                times.clone(),
                                values
                                    .iter()
                                    .skip(k)
                                    .step_by(names.len())
                                    .map(|v| glam::Vec4::new(*v, 0., 0., 0.))
                                    .collect(),
                                interpolation,
                            )?;
                            if let Some(at) = morphs.iter().position(|t: &super::MorphAnimation| t.name == *name) {
                                morphs.remove(at);
                            }
                            morphs.push(super::MorphAnimation::new(name.clone(), track));
                        }
                        continue;
                    }
                };
                let track = AnimationTrack::try_new(times, values, interpolation)?;
                let at = tracks.iter().position(|t| t.joint == joint).unwrap_or_else(|| {
                    tracks.push(JointAnimation::new(joint, rig.joints[joint].name.clone()));
                    tracks.len() - 1
                });
                match property {
                    0 => tracks[at].translation = Some(track),
                    1 => tracks[at].rotation = Some(track),
                    _ => tracks[at].scale = Some(track),
                }
            }
            Ok(AnimationClip::try_new(
                animation
                    .name()
                    .map_or_else(|| format!("animation {}", animation.index()), str::to_string),
                &source,
                tracks,
            )?
            .try_with_morph_tracks(morphs)?)
        })
        .collect()
}

/// A morph target's offsets: none, or one per corner.
fn offsets(v: Vec<[f32; 3]>, corners: usize, name: &str) -> Result<Vec<Vec3>, String> {
    let v: Vec<Vec3> = v.into_iter().map(Vec3::from).collect();
    if !v.is_empty() && v.len() != corners {
        return Err(format!(
            "{name}: a morph target has {} offsets for {corners} corners",
            v.len()
        ));
    }
    Ok(v)
}

/// How one mesh of the file goes into the loaded mesh.
struct Part<'a> {
    /// Its node's matrix (identity when skinned).
    matrix: Mat4,
    binding: &'a Binding,
    /// Its morph targets' names.
    targets: &'a [String],
    /// Which texture coordinates its material's textures use, and how
    /// they're moved.
    mapping: UvMapping,
}

/// Adds one primitive's triangles to `out`, moved by its part's matrix.
fn append(
    out: &mut MeshData,
    primitive: &gltf::Primitive,
    buffers: &[Vec<u8>],
    part_of: &Part,
    name: &str,
) -> Result<(), String> {
    let (matrix, binding) = (part_of.matrix, part_of.binding);
    let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
    // Positions, normals and texture coordinates may be quantized
    // (`KHR_mesh_quantization`): read whatever their component type.
    let floats3 = |semantic: &gltf::Semantic| -> Result<Option<Vec<Vec3>>, String> {
        primitive
            .get(semantic)
            .map(|a| gltf_ext::read_floats::<3>(&a, buffers, name).map(|v| v.into_iter().map(Vec3::from).collect()))
            .transpose()
    };
    let uvs = |set: u32| -> Result<Vec<Vec2>, String> {
        Ok(primitive
            .get(&gltf::Semantic::TexCoords(set))
            .map(|a| gltf_ext::read_floats::<2>(&a, buffers, name))
            .transpose()?
            .map(|v| v.into_iter().map(Vec2::from).collect())
            .unwrap_or_default())
    };
    let positions: Vec<Vec3> =
        floats3(&gltf::Semantic::Positions)?.ok_or_else(|| format!("{name}: a mesh has no positions"))?;
    let mapping = part_of.mapping;
    let count = positions.len() as u32;
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..count).collect(),
    };
    let part = MeshData {
        positions,
        // Quantized normals needn't be unit length.
        normals: floats3(&gltf::Semantic::Normals)?
            .map(|n| n.into_iter().map(|n| n.normalize_or(Vec3::Y)).collect())
            .unwrap_or_default(),
        uvs: uvs(mapping.set)?.into_iter().map(|uv| mapping.apply(uv)).collect(),
        indices,
        // glTF's colors are linear; Color is sRGB.
        colors: reader
            .read_colors(0)
            .map(|c| {
                c.into_rgba_f32()
                    .map(|[r, g, b, a]| Color::rgba(to_srgb(r), to_srgb(g), to_srgb(b), a))
                    .collect()
            })
            .unwrap_or_default(),
        uvs1: uvs(1)?,
        ..Default::default()
    };
    match binding {
        Binding::None => {}
        Binding::Rigid(slot) => {
            out.joints.extend(std::iter::repeat_n([*slot, 0, 0, 0], count as usize));
            out.weights
                .extend(std::iter::repeat_n([1.0, 0.0, 0.0, 0.0], count as usize));
        }
        Binding::Skin(slots) => {
            let joints: Vec<[u16; 4]> = reader
                .read_joints(0)
                .map(|j| j.into_u16().collect())
                .unwrap_or_default();
            let weights: Vec<[f32; 4]> = reader
                .read_weights(0)
                .map(|w| w.into_f32().collect())
                .unwrap_or_default();
            if joints.len() != count as usize || weights.len() != count as usize {
                return Err(format!(
                    "{name}: a skinned mesh needs JOINTS_0 and WEIGHTS_0 on every corner"
                ));
            }
            let slot = |j: u16| {
                slots
                    .get(usize::from(j))
                    .copied()
                    .ok_or_else(|| format!("{name}: a corner names joint {j}, past its skin's joints"))
            };
            for (j, w) in joints.iter().zip(&weights) {
                let mut mapped = [0u16; 4];
                for k in 0..4 {
                    // Unused influences may name any joint.
                    mapped[k] = if w[k] > 0.0 { slot(j[k])? } else { 0 };
                }
                out.joints.push(mapped);
                out.weights.push(super::skin::normalized(*w));
            }
        }
    }
    part.problem().map_err(|e| format!("{name}: {e}"))?;
    let normals = part.normals_or_computed().into_owned();
    // A mirroring node flips triangles inside out: swap two corners back.
    let flips = matrix.determinant() < 0.0;
    let normal_matrix = Mat3::from_mat4(matrix).inverse().transpose();
    let base = out.positions.len() as u32;
    out.positions
        .extend(part.positions.iter().map(|p| matrix.transform_point3(*p)));
    out.normals
        .extend(normals.iter().map(|n| (normal_matrix * *n).normalize_or(Vec3::Y)));
    // Morph targets: offsets turn with the node (normals' by the normal
    // matrix, unnormalized: they're differences).
    let mut targets = Vec::new();
    for (k, target) in primitive.morph_targets().enumerate() {
        let corners = count as usize;
        let read = |a: Option<gltf::Accessor>| -> Result<Vec<Vec3>, String> {
            let v = match a {
                Some(a) => gltf_ext::read_floats::<3>(&a, buffers, name)?,
                None => Vec::new(),
            };
            offsets(v, corners, name)
        };
        let (positions, normals) = (read(target.positions())?, read(target.normals())?);
        targets.push(
            MorphTarget::new(
                part_of.targets.get(k).cloned().unwrap_or_else(|| format!("{name}.{k}")),
                if positions.is_empty() {
                    vec![Vec3::ZERO; corners]
                } else {
                    positions.iter().map(|d| matrix.transform_vector3(*d)).collect()
                },
            )
            .with_normals(normals.iter().map(|d| normal_matrix * *d).collect()),
        );
    }
    merge_targets(&mut out.morph_targets, base as usize, targets, out.positions.len());
    if part.uvs.is_empty() {
        out.uvs.extend(std::iter::repeat_n(Vec2::ZERO, count as usize));
    } else {
        out.uvs.extend(part.uvs);
    }
    if part.uvs1.is_empty() {
        out.uvs1.extend(std::iter::repeat_n(Vec2::ZERO, count as usize));
    } else {
        out.uvs1.extend(part.uvs1);
    }
    if part.colors.is_empty() {
        out.colors.extend(std::iter::repeat_n(Color::WHITE, count as usize));
    } else {
        out.colors.extend(part.colors);
    }
    for tri in part.indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] + base, tri[1] + base, tri[2] + base];
        out.indices.extend(if flips { [a, c, b] } else { [a, b, c] });
    }
    Ok(())
}

/// The matrix that takes a primitive's positions back to the size they had
/// before `ponko cook` quantized them (see
/// [`gltf_ext::fold_quantized_positions`]).
fn dequantization(dequantize: &[Vec<Mat4>], mesh: &gltf::Mesh, primitive: &gltf::Primitive) -> Mat4 {
    dequantize
        .get(mesh.index())
        .and_then(|list| list.get(primitive.index()))
        .copied()
        .unwrap_or(Mat4::IDENTITY)
}

/// The file's images, decoded on first use.
struct Images<'a> {
    decoded: Vec<Image>,
    /// Images kept in their block format (`decoded` holds only their size).
    blocks: Vec<Option<Blocks>>,
    /// The block formats images may stay in.
    gpu: GpuFormats,
    /// What each of `decoded` is, so each is decoded or merged once.
    keys: Vec<Key>,
    /// Which files each of `decoded` came from (see `Loaded::image_sources`).
    sources: Vec<Option<String>>,
    have: &'a dyn Fn(&str) -> bool,
    document: &'a gltf::Document,
    buffers: &'a [Vec<u8>],
    folder: &'a str,
    name: &'a str,
}

#[derive(PartialEq)]
enum Key {
    /// An image of the file.
    Image(usize),
    /// An ORM image made from glTF's metallic-roughness and occlusion images.
    Orm(Option<usize>, Option<usize>),
}

type Read<'r> = &'r mut dyn FnMut(&str) -> Result<Vec<u8>, String>;

/// What [`Images::cooked_orm`] found: the cooked ORM image, or the file it
/// read to find out (its name and bytes), if it read one.
#[cfg(feature = "compressed-textures")]
type Orm = Result<(Image, Option<Blocks>), Option<(String, Vec<u8>)>>;

impl Images<'_> {
    /// Texture `texture`'s image, decoded: its index in `decoded`. An image
    /// it has through an extension the build reads (KTX2, WebP) comes
    /// first, its core image if that fails.
    fn texture(
        &mut self,
        texture: &gltf::Texture,
        preferred: &[Option<usize>],
        warnings: &mut Vec<String>,
        read: Read,
    ) -> Result<usize, String> {
        if let Some(Some(image)) = preferred.get(texture.index()) {
            match self.get(*image, read) {
                Ok(at) => return Ok(at),
                Err(e) => warnings.push(format!("{e}; its fallback image loads instead")),
            }
        }
        self.get(texture.source().index(), read)
    }

    /// Image `index`, decoded: its index in `decoded`.
    fn get(&mut self, index: usize, read: Read) -> Result<usize, String> {
        if let Some(at) = self.keys.iter().position(|k| *k == Key::Image(index)) {
            return Ok(at);
        }
        let source = self.source(index);
        let (image, blocks) = match &source {
            Some(s) if (self.have)(s) => ((0, 0, Vec::new()), None),
            _ => self.decode(index, self.gpu, read)?,
        };
        Ok(self.add(Key::Image(index), image, blocks, source))
    }

    /// The asset name of image `index`, if it's a file of its own.
    fn source(&self, index: usize) -> Option<String> {
        match self.document.images().nth(index)?.source() {
            gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
                Some(format!("{}{}", self.folder, percent_decode(uri)))
            }
            _ => None,
        }
    }

    /// An ORM image from glTF's metallic-roughness image (roughness in
    /// green, metalness in blue) and occlusion image (red), either of
    /// which can be missing: what's missing is 1.
    fn orm(&mut self, metal_rough: Option<usize>, occlusion: Option<usize>, read: Read) -> Result<usize, String> {
        let key = Key::Orm(metal_rough, occlusion);
        if let Some(at) = self.keys.iter().position(|k| *k == key) {
            return Ok(at);
        }
        let file = |i: Option<usize>| i.map_or(Some(String::new()), |i| self.source(i));
        let source = file(metal_rough)
            .zip(file(occlusion))
            .map(|(m, o)| format!("orm:{m}|{o}"));
        if let Some(s) = &source
            && (self.have)(s)
        {
            return Ok(self.add(key, (0, 0, Vec::new()), None, source));
        }
        // An image file `ponko cook` merged them into: as it is. Otherwise
        // the file it read is decoded below without reading it again.
        #[cfg(feature = "compressed-textures")]
        let mut spare = match self.cooked_orm(metal_rough, occlusion, read)? {
            Ok((image, blocks)) => return Ok(self.add(key, image, blocks, source)),
            Err(read_file) => read_file,
        };
        #[cfg(not(feature = "compressed-textures"))]
        let mut spare: Option<(String, Vec<u8>)> = None;
        let read: Read = &mut |file: &str| match spare.take_if(|(f, _)| f == file) {
            Some((_, bytes)) => Ok(bytes),
            None => read(file),
        };
        // Merged on the CPU: both as RGBA8.
        let rgba = GpuFormats::default();
        let occlusion = occlusion.map(|o| self.decode(o, rgba, read).map(|d| d.0)).transpose()?;
        let mut orm = match metal_rough {
            Some(m) => self.decode(m, rgba, read)?.0,
            None => {
                let (w, h, _) = occlusion.as_ref().expect("one of the two is there");
                (*w, *h, vec![255; (*w * *h * 4) as usize])
            }
        };
        let pixels = orm.2.as_chunks_mut::<4>().0;
        match occlusion {
            // Red from the occlusion image, when it lines up texel for texel.
            Some((w, h, ao)) if (w, h) == (orm.0, orm.1) => {
                for (px, o) in pixels.iter_mut().zip(ao.as_chunks::<4>().0) {
                    px[0] = o[0];
                }
            }
            _ => pixels.iter_mut().for_each(|px| px[0] = 255),
        }
        Ok(self.add(key, orm, None, source))
    }

    /// The ORM image of a metallic-roughness and an occlusion image, when
    /// `ponko cook` already merged them into the first's file (the second's
    /// without one): a KTX2 file whose [`ORM_KEY`](crate::compressed::ORM_KEY)
    /// names these two files. Kept in its block format, as any image; so a
    /// model that isn't cooked itself keeps its merged occlusion compressed.
    /// When it isn't one: the file it read (its name and bytes), if any.
    #[cfg(feature = "compressed-textures")]
    fn cooked_orm(&self, metal_rough: Option<usize>, occlusion: Option<usize>, read: Read) -> Result<Orm, String> {
        let Some(holder) = metal_rough.or(occlusion) else {
            return Ok(Err(None));
        };
        let name = |i: Option<usize>| match i {
            Some(i) => self.source(i).map(|s| normalize(&s)),
            None => Some(String::new()),
        };
        let (Some(file), Some(m), Some(o)) = (self.source(holder), name(metal_rough), name(occlusion)) else {
            return Ok(Err(None));
        };
        let Ok(bytes) = read(&file) else {
            return Ok(Err(None));
        };
        let Some(note) = crate::compressed::orm_note(&bytes) else {
            return Ok(Err(Some((file, bytes))));
        };
        let folder = folder_of(&file);
        let resolve = |part: &str| {
            if part.is_empty() {
                String::new()
            } else {
                normalize(&format!("{folder}{part}"))
            }
        };
        match note.split_once('|') {
            Some((a, b)) if resolve(a) == m && resolve(b) == o => {
                let what = format!("image {holder} of {}", self.name);
                decode_bytes(bytes, self.name, &what, self.gpu).map(Ok)
            }
            _ => Ok(Err(Some((file, bytes)))),
        }
    }

    fn add(&mut self, key: Key, image: Image, blocks: Option<Blocks>, source: Option<String>) -> usize {
        self.keys.push(key);
        self.sources.push(source);
        self.decoded.push(image);
        self.blocks.push(blocks);
        self.decoded.len() - 1
    }

    /// Decodes image `index` of the file to RGBA8, or keeps it in a block
    /// format of `gpu`.
    fn decode(&self, index: usize, gpu: GpuFormats, read: Read) -> Result<(Image, Option<Blocks>), String> {
        let image = self
            .document
            .images()
            .nth(index)
            .ok_or_else(|| format!("{}: image {index} isn't in the file", self.name))?;
        decode(&image, self.buffers, self.folder, read, self.name, gpu)
    }
}

/// Decodes an image of the file to RGBA8; a KTX2 image in a block format
/// `gpu` samples stays as it is (its size and blocks).
fn decode(
    image: &gltf::Image,
    buffers: &[Vec<u8>],
    folder: &str,
    read: Read,
    name: &str,
    gpu: GpuFormats,
) -> Result<(Image, Option<Blocks>), String> {
    if let Some(decoded) = DECODED.with(|d| d.borrow_mut().remove(&image.index())) {
        return Ok((decoded, None));
    }
    let bytes = match image.source() {
        gltf::image::Source::View { view, .. } => {
            let buffer = &buffers[view.buffer().index()];
            buffer
                .get(view.offset()..view.offset() + view.length())
                .ok_or_else(|| format!("{name}: an image is past the end of its buffer"))?
                .to_vec()
        }
        gltf::image::Source::Uri { uri, .. } => fetch(uri, folder, read)?,
    };
    decode_bytes(bytes, name, &format!("image {} of {name}", image.index()), gpu)
}

/// [`decode`] of an image's bytes.
fn decode_bytes(bytes: Vec<u8>, name: &str, what: &str, gpu: GpuFormats) -> Result<(Image, Option<Blocks>), String> {
    // Any format the build reads: KTX2 (`KHR_texture_basisu`) and WebP
    // (`EXT_texture_webp`) too, with their features.
    let broken = |e: crate::error::Error| format!("{name}: an image can't be read: {e}");
    #[cfg(feature = "compressed-textures")]
    if let Some(decoded) = crate::compressed::decode(&bytes, name, what, gpu) {
        return Ok(match decoded.map_err(broken)? {
            crate::compressed::Decoded::Rgba(i) => ((i.width, i.height, i.rgba), None),
            crate::compressed::Decoded::Blocks(b) => ((b.width, b.height, Vec::new()), Some(b)),
        });
    }
    #[cfg(not(feature = "compressed-textures"))]
    let _ = gpu;
    let decoded = crate::assets::decode_image(&bytes, name, what).map_err(broken)?;
    Ok(((decoded.width, decoded.height, decoded.rgba), None))
}

/// The bytes a URI names: inline base64 data, or a file next to the glTF.
pub(super) fn fetch(
    uri: &str,
    folder: &str,
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, data) = rest
            .split_once(";base64,")
            .ok_or("only base64 data URIs are supported")?;
        return base64(data);
    }
    let file = percent_decode(uri);
    read(&format!("{folder}{file}"))
}

/// The folder part of asset name `name`, with its trailing separator: what
/// relative URIs in it are joined to. `\\` counts too, for a file loaded by an
/// absolute Windows path.
pub(super) fn folder_of(name: &str) -> &str {
    name.rfind(['/', '\\']).map_or("", |i| &name[..=i])
}

/// `a/./b/../c` to `a/c`: asset names compared as the same file.
#[cfg(feature = "compressed-textures")]
fn normalize(name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// `%20` and friends in file names.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The most elements an accessor without data (all zeros) may have where
/// `read_floats` reads it: 16 Mi, far more than a mesh of zeros needs.
const MAX_ZERO_ELEMENTS: usize = 1 << 24;

/// Refuses a primitive whose `POSITION` names a missing accessor, before
/// gltf-json's validation, which looks that accessor up without checking
/// (a panic in every build). `gltf_ext::prepare` runs it on the JSON
/// before `Document::from_json`.
pub(super) fn check_position_indices(json: &gltf::json::Root) -> Result<(), String> {
    use gltf::json::validation::Checked;
    for (m, mesh) in json.meshes.iter().enumerate() {
        for primitive in &mesh.primitives {
            if let Some(index) = primitive.attributes.get(&Checked::Valid(gltf::Semantic::Positions))
                && index.value() >= json.accessors.len()
            {
                return Err(format!(
                    "mesh {m}: POSITION names accessor {}, which doesn't exist",
                    index.value()
                ));
            }
        }
    }
    Ok(())
}

/// Checks every accessor this loader reads before anything reads it:
///
/// - What the gltf crate's readers read (colors, joints, weights, indices,
///   tangents, inverse binds, key times and values) has a type glTF allows
///   for its use, at least one element, a stride of at least one element
///   and an end a `usize` holds. The readers assume all of these: another
///   type reaches an `unreachable!` (a panic in every build) or a debug
///   assertion, and the others overflow their size arithmetic. (Such an
///   accessor past the end of its view is fine: they find no data there.)
/// - What `gltf_ext::read_floats` reads (positions, normals, texture
///   coordinates, morph offsets, of any component type for
///   `KHR_mesh_quantization`) fits its view, since it sets aside room for
///   every element first; one without a view (zeros) has at most
///   [`MAX_ZERO_ELEMENTS`].
/// - A primitive's attributes and morph offsets all have `POSITION`'s
///   count, as glTF requires.
fn check_accessors(document: &gltf::Document) -> Result<(), String> {
    use gltf::Semantic;
    use gltf::accessor::{DataType::*, Dimensions, Dimensions::*};
    use gltf::json::validation::Checked;

    /// Integer types a normalized rotation or weight may use, and floats.
    const NORMALIZED: &[gltf::accessor::DataType] = &[I8, U8, I16, U16, F32];
    /// Any component type: `read_floats` reads them all.
    const ANY: &[gltf::accessor::DataType] = &[I8, U8, I16, U16, U32, F32];
    // Where the last element ends, as the readers compute it, if a `usize`
    // holds it.
    let end = |view: &gltf::buffer::View, offset: usize, count: usize, size: usize| {
        let stride = view.stride().unwrap_or(size);
        if stride < size || count == 0 {
            return None;
        }
        stride.checked_mul(count - 1)?.checked_add(offset)?.checked_add(size)
    };
    // `whole`: the accessor must fit its view (`read_floats`).
    let check =
        |a: &gltf::Accessor, used_as: &str, dims: &[Dimensions], types: &[gltf::accessor::DataType], whole: bool| {
            let bad = |why: String| format!("accessor {} ({used_as}) {why}", a.index());
            if !dims.contains(&a.dimensions()) || !types.contains(&a.data_type()) {
                return Err(bad(format!(
                    "is {:?} {:?}, which glTF doesn't allow there",
                    a.dimensions(),
                    a.data_type()
                )));
            }
            match a.view() {
                Some(view) => match end(&view, a.offset(), a.count(), a.size()) {
                    None => return Err(bad("has no elements, a stride under one, or an end past 2^64".into())),
                    Some(end) if whole && end > view.length() => {
                        return Err(bad("runs past the end of its buffer view".into()));
                    }
                    Some(_) => {}
                },
                None if whole && a.count() > MAX_ZERO_ELEMENTS => {
                    return Err(bad(format!(
                        "has {} elements and no data; at most {MAX_ZERO_ELEMENTS} may be zeros",
                        a.count()
                    )));
                }
                None => {}
            }
            if let Some(sparse) = a.sparse() {
                let (indices, values) = (sparse.indices(), sparse.values());
                if end(
                    &indices.view(),
                    indices.offset(),
                    sparse.count(),
                    indices.index_type().size(),
                )
                .is_none()
                    || end(&values.view(), values.offset(), sparse.count(), a.size()).is_none()
                {
                    return Err(bad(
                        "has sparse values with no elements, a stride under one, or an end past 2^64".into(),
                    ));
                }
            }
            Ok(())
        };

    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let corners = primitive.get(&Semantic::Positions).map(|a| a.count());
            let same_count = |a: &gltf::Accessor, used_as: &str| match corners {
                Some(corners) if a.count() != corners => Err(format!(
                    "accessor {} ({used_as}) has {} elements, but POSITION has {corners}",
                    a.index(),
                    a.count()
                )),
                _ => Ok(()),
            };
            for (semantic, a) in primitive.attributes() {
                let used_as = match semantic {
                    Semantic::Positions => "POSITION",
                    Semantic::Normals => "NORMAL",
                    Semantic::Tangents => "TANGENT",
                    Semantic::Colors(_) => "COLOR",
                    Semantic::TexCoords(_) => "TEXCOORD",
                    Semantic::Joints(_) => "JOINTS",
                    Semantic::Weights(_) => "WEIGHTS",
                    _ => continue,
                };
                same_count(&a, used_as)?;
                match semantic {
                    Semantic::Positions | Semantic::Normals => check(&a, used_as, &[Vec3], ANY, true)?,
                    Semantic::TexCoords(_) => check(&a, used_as, &[Vec2], ANY, true)?,
                    Semantic::Tangents => check(&a, used_as, &[Vec4], &[F32], false)?,
                    Semantic::Colors(_) => check(&a, used_as, &[Vec3, Vec4], &[U8, U16, F32], false)?,
                    Semantic::Joints(_) => check(&a, used_as, &[Vec4], &[U8, U16], false)?,
                    _ => check(&a, used_as, &[Vec4], &[U8, U16, F32], false)?,
                }
            }
            if let Some(a) = primitive.indices() {
                check(&a, "indices", &[Scalar], &[U8, U16, U32], false)?;
            }
            for target in primitive.morph_targets() {
                for a in [target.positions(), target.normals(), target.tangents()]
                    .into_iter()
                    .flatten()
                {
                    same_count(&a, "morph target")?;
                    check(&a, "morph target", &[Vec3], ANY, true)?;
                }
            }
        }
    }
    for skin in document.skins() {
        if let Some(a) = skin.inverse_bind_matrices() {
            check(&a, "inverse bind matrices", &[Mat4], &[F32], false)?;
        }
    }
    let json = document.as_json();
    for animation in document.animations() {
        for channel in animation.channels() {
            let sampler = channel.sampler();
            check(&sampler.input(), "key times", &[Scalar], &[F32], false)?;
            let output = sampler.output();
            match json.animations[animation.index()].channels[channel.index()].target.path {
                Checked::Valid(gltf::animation::Property::Translation | gltf::animation::Property::Scale) => {
                    check(&output, "key values", &[Vec3], &[F32], false)?;
                }
                Checked::Valid(gltf::animation::Property::Rotation) => {
                    check(&output, "key values", &[Vec4], NORMALIZED, false)?;
                }
                Checked::Valid(gltf::animation::Property::MorphTargetWeights) => {
                    check(&output, "key values", &[Scalar], NORMALIZED, false)?;
                }
                // Skipped by `read_clips`.
                Checked::Invalid => {}
            }
        }
    }
    Ok(())
}

/// Checks that the nodes form trees, as glTF requires: each node has at
/// most one parent and isn't its own ancestor, and a scene lists only roots,
/// each once. Every walk of the scenes below relies on it: a cycle would
/// never end, and a node with two parents (or a root listed twice) is
/// walked again for each, which grows exponentially with the depth.
fn check_node_tree(document: &gltf::Document) -> Result<(), String> {
    let mut parent: Vec<Option<usize>> = vec![None; document.nodes().len()];
    for node in document.nodes() {
        for child in node.children() {
            if let Some(other) = parent[child.index()].replace(node.index()) {
                return Err(format!(
                    "node {} is a child of both node {other} and node {}",
                    child.index(),
                    node.index()
                ));
            }
        }
    }
    // Up from each node until a root or a node already known to reach one.
    const UNSEEN: u8 = 0;
    const ON_PATH: u8 = 1;
    const REACHES_ROOT: u8 = 2;
    let mut state = vec![UNSEEN; parent.len()];
    let mut path = Vec::new();
    for start in 0..parent.len() {
        let mut at = Some(start);
        while let Some(n) = at.filter(|&n| state[n] != REACHES_ROOT) {
            if state[n] == ON_PATH {
                return Err(format!("node {n} is its own ancestor"));
            }
            state[n] = ON_PATH;
            path.push(n);
            at = parent[n];
        }
        for n in path.drain(..) {
            state[n] = REACHES_ROOT;
        }
    }
    for scene in document.scenes() {
        let mut listed = vec![false; parent.len()];
        for node in scene.nodes() {
            let n = node.index();
            if let Some(p) = parent[n] {
                return Err(format!("scene {} lists node {n}, a child of node {p}", scene.index()));
            }
            if std::mem::replace(&mut listed[n], true) {
                return Err(format!("scene {} lists node {n} twice", scene.index()));
            }
        }
    }
    Ok(())
}

/// Refuses a binary glTF whose header gives a length shorter than the
/// header itself: the gltf crate subtracts without checking (a panic in
/// builds with overflow checks).
fn check_glb_length(name: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(length) = bytes.strip_prefix(b"glTF").and_then(|rest| rest.get(4..8))
        && u32::from_le_bytes(length.try_into().expect("4 bytes")) < 12
    {
        return Err(format!(
            "{name}: not a glTF file: its header gives a length under 12 bytes"
        ));
    }
    Ok(())
}

fn base64(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = value(c).ok_or("bad base64 in a data URI")?;
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

pub(super) fn to_srgb(linear: f32) -> f32 {
    // Exact for white, which the formula misses by a rounding step.
    if linear == 1.0 {
        1.0
    } else if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_percent_decoding() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("AAEC").unwrap(), [0, 1, 2]);
        assert!(base64("a$").is_err());
        assert_eq!(percent_decode("my%20tree.bin"), "my tree.bin");
    }

    #[test]
    fn buffers_are_looked_up_next_to_the_file_with_either_separator() {
        assert_eq!(folder_of("models/tree.gltf"), "models/");
        assert_eq!(folder_of(r"C:\Temp\x\tree.gltf"), r"C:\Temp\x\");
        assert_eq!(folder_of(r"D:\a\crate/../tree.gltf"), r"D:\a\crate/../");
        assert_eq!(folder_of("tree.gltf"), "");
    }

    fn to_base64(bytes: &[u8]) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut b64 = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                b64.push(alphabet[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        while !b64.len().is_multiple_of(4) {
            b64.push('=');
        }
        b64
    }

    /// A PNG of two `pixel`s side by side, as a data URI.
    fn png(pixel: [u8; 4]) -> String {
        let image = image::RgbaImage::from_fn(2, 1, |_, _| image::Rgba(pixel));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        format!("data:image/png;base64,{}", to_base64(&bytes.into_inner()))
    }

    #[test]
    fn masked_materials_load_as_cut_outs() {
        let gltf = triangle_gltf(None).replace(
            r#""materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1], "roughnessFactor": 0.25 } }],"#,
            r#""materials": [{ "alphaMode": "MASK" }, {}],"#,
        );
        let loaded = load("mask.gltf", gltf.as_bytes(), &mut |f| Err(f.to_string()), &|_| false).unwrap();
        assert_eq!(loaded.materials[0].blend, Blend::Cutout);
        assert_eq!(loaded.materials[1].blend, Blend::Opaque);
    }

    #[test]
    fn material_maps_load_with_occlusion_packed_into_red() {
        let gltf = triangle_gltf(None);
        let materials = format!(
            r#""materials": [
                {{ "pbrMetallicRoughness": {{ "metallicRoughnessTexture": {{ "index": 0 }} }},
                   "occlusionTexture": {{ "index": 1 }},
                   "normalTexture": {{ "index": 2, "scale": 0.5 }},
                   "emissiveTexture": {{ "index": 2 }},
                   "alphaMode": "BLEND", "doubleSided": true }},
                {{ "occlusionTexture": {{ "index": 1 }} }}
            ],
            "textures": [{{ "source": 0 }}, {{ "source": 1 }}, {{ "source": 2 }}],
            "images": [{{ "uri": "{}" }}, {{ "uri": "{}" }}, {{ "uri": "{}" }}],"#,
            png([9, 100, 200, 255]),
            png([50, 50, 50, 255]),
            png([128, 128, 255, 255]),
        );
        let gltf = gltf.replace(
            r#""materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1], "roughnessFactor": 0.25 } }],"#,
            &materials,
        );
        let loaded = load("maps.gltf", gltf.as_bytes(), &mut |f| Err(f.to_string()), &|_| false).unwrap();
        let m = &loaded.materials[0];
        assert!(m.blend == Blend::Transparent && m.double_sided);
        assert_eq!(m.normal_strength, 0.5);
        assert_eq!(m.normal_map, m.emissive_map, "one image, decoded once");
        let orm = &loaded.images[m.orm_map.unwrap()];
        assert_eq!(
            &orm.2[..4],
            &[50, 100, 200, 255],
            "occlusion in red, roughness and metalness as they were"
        );
        let only_occlusion = &loaded.images[loaded.materials[1].orm_map.unwrap()];
        assert_eq!(
            &only_occlusion.2[..4],
            &[50, 255, 255, 255],
            "no roughness or metalness map: 1"
        );
        assert_eq!(loaded.images.len(), 3, "normal, ORM and occlusion-only ORM");
    }

    #[test]
    fn image_files_say_where_they_came_from_and_known_ones_are_not_read() {
        let gltf = triangle_gltf(None).replace(
            r#""materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1], "roughnessFactor": 0.25 } }],"#,
            r#""materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                              "normalTexture": { "index": 1 } }],
            "textures": [{ "source": 0 }, { "source": 1 }],
            "images": [{ "uri": "textures/bark%20a.png" }, { "uri": "textures/bark_n.png" }],"#,
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut reads = Vec::new();
        let loaded = load(
            "models/oak.gltf",
            gltf.as_bytes(),
            &mut |f| {
                reads.push(f.to_string());
                Ok(png.get_ref().clone())
            },
            &|source| source == "models/textures/bark a.png",
        )
        .unwrap();
        assert_eq!(reads, ["models/textures/bark_n.png"], "the known one isn't read");
        let m = &loaded.materials[0];
        assert_eq!(
            loaded.image_sources[m.image.unwrap()].as_deref(),
            Some("models/textures/bark a.png")
        );
        assert_eq!(loaded.images[m.image.unwrap()].0, 0, "left empty");
        assert_eq!(loaded.images[m.normal_map.unwrap()].2, [1, 2, 3, 4]);
    }

    /// A model's metallic-roughness and occlusion images in files of their
    /// own, which `ponko cook` merged into the first's file (a KTX2 file
    /// whose `ponkoOrm` names both): the model, uncooked itself, gets that
    /// file in its block format, and the occlusion file isn't read. A note
    /// naming other files is ignored: the merge happens as before.
    #[cfg(feature = "compressed-textures")]
    #[test]
    fn a_cooked_orm_file_stays_compressed_for_an_uncooked_model() {
        use crate::compressed::tests::ktx2_file_with;
        let gltf = triangle_gltf(None).replace(
            r#""materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1], "roughnessFactor": 0.25 } }],"#,
            r#""materials": [{ "pbrMetallicRoughness": { "metallicRoughnessTexture": { "index": 0 } },
                              "occlusionTexture": { "index": 1 } }],
            "textures": [{ "source": 0 }, { "source": 1 }],
            "images": [{ "uri": "textures/rock_mr.png" }, { "uri": "./textures/rock_ao.png" }],"#,
        );
        let ao = {
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(4, 4, image::Rgba([77, 0, 0, 255]))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            png.into_inner()
        };
        let load_with = |note: &str| {
            let block = vec![0u8; 16];
            let mr = ktx2_file_with(
                Some(ktx2::Format::BC7_UNORM_BLOCK),
                4,
                4,
                &[block],
                false,
                &[("ponkoOrm", note)],
            );
            let mut reads = Vec::new();
            let loaded = load_for(
                "models/rock.gltf",
                gltf.as_bytes(),
                &mut |f| {
                    reads.push(f.to_string());
                    match f {
                        "models/textures/rock_mr.png" => Ok(mr.clone()),
                        "models/./textures/rock_ao.png" => Ok(ao.clone()),
                        other => Err(format!("no {other}")),
                    }
                },
                &|_| false,
                GpuFormats::ALL,
            )
            .unwrap();
            let orm = loaded.materials[0].orm_map.unwrap();
            (loaded.blocks[orm].is_some(), reads)
        };
        let (compressed, reads) = load_with("rock_mr.png|rock_ao.png");
        assert!(compressed, "the cooked file, in its blocks");
        assert_eq!(reads, ["models/textures/rock_mr.png"], "the occlusion isn't read");
        let (compressed, reads) = load_with("rock_mr.png|other_ao.png");
        assert!(!compressed, "merged on the CPU");
        assert_eq!(
            reads,
            ["models/textures/rock_mr.png", "models/./textures/rock_ao.png"],
            "each file read once"
        );
    }

    #[test]
    fn inline_images_have_no_source() {
        let loaded = load(
            "t.gltf",
            triangle_gltf(None).as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| true,
        )
        .unwrap();
        assert!(loaded.image_sources.is_empty());
    }

    /// A one-triangle .gltf under a node that moves it up by 2 and mirrors
    /// x; its buffer inline, or in `file` next to it.
    fn triangle_gltf(file: Option<&str>) -> String {
        let mut bytes = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                bytes.extend(c.to_le_bytes());
            }
        }
        let b64 = to_base64(&bytes);
        let uri = file.map_or(format!("data:application/octet-stream;base64,{b64}"), str::to_string);
        format!(
            r#"{{
            "asset": {{ "version": "2.0" }},
            "scene": 0,
            "scenes": [{{ "nodes": [0] }}],
            "nodes": [{{ "mesh": 0, "translation": [0, 2, 0], "scale": [-1, 1, 1] }}],
            "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "material": 0 }}] }}],
            "materials": [{{ "pbrMetallicRoughness": {{ "baseColorFactor": [1, 0, 0, 1], "roughnessFactor": 0.25 }} }}],
            "buffers": [{{ "byteLength": 36, "uri": "{uri}" }}],
            "bufferViews": [{{ "buffer": 0, "byteLength": 36 }}],
            "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                            "min": [0, 0, 0], "max": [1, 1, 0] }}]
        }}"#
        )
    }

    #[test]
    fn a_gltf_file_loads_placed_by_its_node_with_its_material() {
        let loaded = load(
            "tri.gltf",
            triangle_gltf(None).as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| false,
        )
        .unwrap();
        let data = &loaded.data;
        assert_eq!(
            data.positions,
            vec![
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(-1.0, 2.0, 0.0),
                Vec3::new(0.0, 3.0, 0.0)
            ]
        );
        // Mirrored: the winding is swapped so the computed normal (+Z) faces front.
        assert_eq!(data.indices, vec![0, 2, 1]);
        let [a, b, c] = [0, 2, 1].map(|i| data.positions[i]);
        assert!((b - a).cross(c - a).dot(data.normals[0]) > 0.0);
        assert_eq!(loaded.parts, vec![(0..3, 0)]);
        let m = &loaded.materials[0];
        assert!((m.color.r - 1.0).abs() < 1e-5 && m.color.g == 0.0);
        assert_eq!((m.roughness, m.metallic), (0.25, 1.0), "metallic defaults to 1 in glTF");
    }

    /// Two meshes on two nodes: "face" has targets "smile" and "blink"
    /// (named in its extras, with default weights), "lid" (scaled 2x, its
    /// own material) only "blink".
    fn morph_gltf() -> String {
        let mut bytes = Vec::new();
        let floats = |bytes: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|f| bytes.extend(f.to_le_bytes()));
        floats(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // corners
        floats(&mut bytes, &[0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]); // all up 1
        floats(&mut bytes, &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]); // corner 0 +x
        let accessor = |at: usize| {
            format!(
                r#"{{ "bufferView": 0, "byteOffset": {}, "componentType": 5126, "count": 3, "type": "VEC3",
                      "min": [0, 0, 0], "max": [1, 1, 0] }}"#,
                at * 36
            )
        };
        format!(
            r#"{{
            "asset": {{ "version": "2.0" }},
            "scene": 0,
            "scenes": [{{ "nodes": [0, 1] }}],
            "nodes": [{{ "mesh": 0 }}, {{ "mesh": 1, "scale": [2, 2, 2] }}],
            "meshes": [
                {{ "name": "face", "weights": [0.5, 0.0], "extras": {{ "targetNames": ["smile", "blink"] }},
                   "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "material": 0,
                                    "targets": [{{ "POSITION": 1 }}, {{ "POSITION": 2 }}] }}] }},
                {{ "name": "lid", "extras": {{ "targetNames": ["blink"] }},
                   "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "material": 1,
                                    "targets": [{{ "POSITION": 2 }}] }}] }}
            ],
            "materials": [{{}}, {{}}],
            "buffers": [{{ "byteLength": 108, "uri": "data:application/octet-stream;base64,{}" }}],
            "bufferViews": [{{ "buffer": 0, "byteLength": 108 }}],
            "accessors": [{}, {}, {}]
        }}"#,
            to_base64(&bytes),
            accessor(0),
            accessor(1),
            accessor(2),
        )
    }

    #[test]
    fn morph_only_gltf_channels_sample_all_interpolations_and_frame_metadata() {
        use crate::anim::{MorphWeights, Skeleton};
        for (frames_key, targets_key) in [("ponkoMorphFrames", "ponkoAnimatedTargets")] {
            for (mode, values, expected) in [
                ("LINEAR", vec![0., 0.2, 1., 0.6], [0.5, 0.4]),
                ("STEP", vec![0., 0.2, 1., 0.6], [0., 0.2]),
                // key-major in/value/out triples, with each tuple's target values interleaved.
                (
                    "CUBICSPLINE",
                    vec![0., 0., 0., 0.2, 4., 0., 0., 0., 0., 0.6, 0., 0.],
                    [1., 0.4],
                ),
            ] {
                let mut doc: serde_json::Value = serde_json::from_str(&morph_gltf()).unwrap();
                let mut bytes = Vec::new();
                for v in [0f32, 2.].into_iter().chain(values) {
                    bytes.extend(v.to_le_bytes());
                }
                doc["buffers"].as_array_mut().unwrap().push(serde_json::json!({"byteLength":bytes.len(),"uri":format!("data:application/octet-stream;base64,{}",to_base64(&bytes))}));
                doc["bufferViews"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({"buffer":1,"byteLength":bytes.len()}));
                doc["accessors"].as_array_mut().unwrap().extend([
                serde_json::json!({"bufferView":1,"componentType":5126,"count":2,"type":"SCALAR","min":[0.],"max":[2.]}),
                serde_json::json!({"bufferView":1,"byteOffset":8,"componentType":5126,"count":(bytes.len()-8)/4,"type":"SCALAR"})]);
                doc["animations"] = serde_json::json!([{"name":"face","samplers":[{"input":3,"output":4,"interpolation":mode}],"channels":[{"sampler":0,"target":{"node":0,"path":"weights"}}]}]);
                doc["meshes"][0]["extras"][frames_key] = serde_json::json!([
                {"channel":0,"target":1,"weight":0.5},{"channel":0,"target":0,"weight":1.}]);
                doc["animations"][0]["extras"][targets_key] = serde_json::json!(["smile"]);
                let loaded = load("face.gltf", doc.to_string().as_bytes(), &mut |f| Err(f.into()), &|_| {
                    false
                })
                .unwrap();
                assert_eq!(loaded.data.morph_frames.len(), 2);
                let mut skeleton = Skeleton::from_shared(loaded.skin.unwrap().joints);
                loaded.clips[0].sample(1., &mut skeleton);
                let mut weights = MorphWeights(vec![0., 0.]);
                let names: Vec<_> = loaded.data.morph_targets.iter().map(|t| t.name.clone()).collect();
                skeleton.write_morph_weights(&names, &mut weights);
                for (name, value) in [("smile", expected[0]), ("blink", 0.)] {
                    assert!(
                        (weights.get(names.iter().position(|n| n == name).unwrap()) - value).abs() < 1e-6,
                        "{mode} {name}"
                    );
                }
            }
        }
    }

    #[test]
    fn morph_targets_merge_by_name_and_follow_their_node() {
        let loaded = load(
            "face.gltf",
            morph_gltf().as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| false,
        )
        .unwrap();
        let data = &loaded.data;
        assert!(data.check().is_ok());
        assert_eq!(data.positions.len(), 6);
        let names: Vec<&str> = data.morph_targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 2, "one \"blink\" for both meshes: {names:?}");
        let target = |name: &str| data.morph_targets.iter().position(|t| t.name == name).unwrap();
        let (smile, blink) = (target("smile"), target("blink"));
        assert_eq!(loaded.morph_defaults[smile], 0.5);
        assert_eq!(loaded.morph_defaults[blink], 0.0);
        // The face's corners: those its material's part draws.
        let (range, _) = loaded.parts.iter().find(|(_, m)| *m == 0).unwrap();
        let face: Vec<usize> = data.indices[range.start as usize..range.end as usize]
            .iter()
            .map(|&i| i as usize)
            .collect();
        for &i in &face {
            assert_eq!(
                data.morph_targets[smile].positions[i],
                Vec3::Y,
                "the face's smile lifts it"
            );
        }
        // Corner 0 of each mesh (at the origin) moves +x in "blink": by 2
        // on the scaled lid.
        let blinks: Vec<Vec3> = data.morph_targets[blink]
            .positions
            .iter()
            .copied()
            .filter(|d| *d != Vec3::ZERO)
            .collect();
        assert_eq!(blinks.len(), 2);
        assert!(blinks.contains(&Vec3::X) && blinks.contains(&(2.0 * Vec3::X)));
        // The lid has no smile.
        let smiling = data.morph_targets[smile]
            .positions
            .iter()
            .filter(|d| **d != Vec3::ZERO)
            .count();
        assert_eq!(smiling, 3);
        assert_eq!(data.morph_targets[smile].normals.len(), 6, "normals padded with zeros");
    }

    #[test]
    fn unnamed_morph_targets_are_named_after_their_mesh() {
        let gltf = morph_gltf().replace(r#""extras": { "targetNames": ["smile", "blink"] },"#, "");
        let loaded = load("face.gltf", gltf.as_bytes(), &mut |f| Err(f.to_string()), &|_| false).unwrap();
        let mut names: Vec<&str> = loaded.data.morph_targets.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["blink", "face.0", "face.1"]);
    }

    #[test]
    fn a_second_uv_set_loads_for_shaders() {
        let mut bytes = Vec::new();
        for c in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend(c.to_le_bytes());
        }
        for c in [0.5f32, 7.0, 1.5, 7.0, 2.5, 7.0] {
            bytes.extend(c.to_le_bytes());
        }
        let b64 = to_base64(&bytes);
        let text = format!(
            r#"{{
            "asset": {{ "version": "2.0" }},
            "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "TEXCOORD_1": 1 }} }}] }}],
            "nodes": [{{ "mesh": 0 }}], "scenes": [{{ "nodes": [0] }}], "scene": 0,
            "buffers": [{{ "byteLength": 60, "uri": "data:application/octet-stream;base64,{b64}" }}],
            "bufferViews": [{{ "buffer": 0, "byteLength": 36 }}, {{ "buffer": 0, "byteOffset": 36, "byteLength": 24 }}],
            "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }},
                          {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2" }}]
        }}"#
        );
        let loaded = load("uv1.gltf", text.as_bytes(), &mut |f| Err(f.to_string()), &|_| false).unwrap();
        assert_eq!(
            loaded.data.uvs1,
            vec![Vec2::new(0.5, 7.0), Vec2::new(1.5, 7.0), Vec2::new(2.5, 7.0)]
        );
        assert!(
            loaded.data.uvs.iter().all(|uv| *uv == Vec2::ZERO),
            "no first set: zeros"
        );
        let plain = load(
            "tri.gltf",
            triangle_gltf(None).as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| false,
        )
        .unwrap();
        assert_eq!(plain.data.uvs1, vec![Vec2::ZERO; 3], "none: zeros");
    }

    /// A skinned .gltf: an "Armature" node over two joints ("root", and
    /// "tip" one up), a triangle skinned to them, and a plain triangle
    /// under "tip".
    fn skinned_gltf() -> String {
        let mut bytes = Vec::new();
        let floats = |bytes: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|c| bytes.extend(c.to_le_bytes()));
        // 0: positions (36 bytes)
        floats(&mut bytes, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        // 36: joints, u8 x 4 per corner (12 bytes)
        bytes.extend([0u8, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0]);
        // 48: weights (48 bytes): unnormalized on the first corner
        floats(
            &mut bytes,
            &[1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        );
        // 96: inverse binds (128 bytes): root at the origin, tip at y 1
        let identity = Mat4::IDENTITY.to_cols_array();
        let tip = Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)).to_cols_array();
        floats(&mut bytes, &identity);
        floats(&mut bytes, &tip);
        let b64 = to_base64(&bytes);
        format!(
            r#"{{
            "asset": {{ "version": "2.0" }},
            "scene": 0,
            "scenes": [{{ "nodes": [0, 3] }}],
            "nodes": [
                {{ "name": "Armature", "children": [1], "scale": [2, 2, 2] }},
                {{ "name": "root", "children": [2] }},
                {{ "name": "tip", "translation": [0, 1, 0], "children": [4] }},
                {{ "name": "body", "mesh": 0, "skin": 0, "translation": [50, 0, 0] }},
                {{ "name": "cup", "mesh": 1 }}
            ],
            "skins": [{{ "joints": [1, 2], "inverseBindMatrices": 3 }}],
            "meshes": [
                {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2 }} }}] }},
                {{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}
            ],
            "buffers": [{{ "byteLength": 224, "uri": "data:application/octet-stream;base64,{b64}" }}],
            "bufferViews": [
                {{ "buffer": 0, "byteLength": 36 }},
                {{ "buffer": 0, "byteOffset": 36, "byteLength": 12 }},
                {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
                {{ "buffer": 0, "byteOffset": 96, "byteLength": 128 }}
            ],
            "accessors": [
                {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }},
                {{ "bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC4" }},
                {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4" }},
                {{ "bufferView": 3, "componentType": 5126, "count": 2, "type": "MAT4" }}
            ]
        }}"#
        )
    }

    #[test]
    fn animations_map_node_indices_to_preorder_joints() {
        // Separate inline buffer for two times and two translation values.
        let bytes: Vec<u8> = [0.0f32, 2.0, 0.0, 1.0, 0.0, 0.0, 3.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut text = skinned_gltf();
        text = text.replace("\"buffers\": [", &format!(r#""animations": [{{ "name": "rise", "samplers": [{{ "input": 4, "output": 5, "interpolation": "LINEAR" }}], "channels": [{{ "sampler": 0, "target": {{ "node": 4, "path": "translation" }} }}] }}],
            "buffers": [{{ "byteLength": 32, "uri": "data:application/octet-stream;base64,{}" }},"#, to_base64(&bytes)));
        // Existing views refer to the old buffer, now index 1.
        text = text.replace("\"buffer\": 0", "\"buffer\": 1");
        // Append views and accessors so existing mesh indices stay unchanged.
        text = text.replace(r#"{ "buffer": 1, "byteOffset": 96, "byteLength": 128 }"#, r#"{ "buffer": 1, "byteOffset": 96, "byteLength": 128 }, { "buffer": 0, "byteLength": 8 }, { "buffer": 0, "byteOffset": 8, "byteLength": 24 }"#);
        text = text.replace(r#"{ "bufferView": 3, "componentType": 5126, "count": 2, "type": "MAT4" }"#, r#"{ "bufferView": 3, "componentType": 5126, "count": 2, "type": "MAT4" }, { "bufferView": 4, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [2] }, { "bufferView": 5, "componentType": 5126, "count": 2, "type": "VEC3" }"#);
        let loaded = load("animated.gltf", text.as_bytes(), &mut |f| Err(f.into()), &|_| false).unwrap();
        let clip = &loaded.clips[0];
        assert_eq!(clip.name(), "rise");
        assert_eq!(clip.duration(), 2.0);
        assert_eq!((clip.tracks()[0].joint, clip.tracks()[0].name.as_str()), (3, "cup"));
        assert_eq!(clip.tracks()[0].translation.as_ref().unwrap().times(), &[0.0, 2.0]);
        let mut skeleton = super::super::Skeleton::from_shared(loaded.skin.unwrap().joints);
        assert!(skeleton.shares_joints(clip.source_joints()));
        clip.sample(1.0, &mut skeleton);
        assert_eq!(skeleton.pose[3].position, Vec3::Y * 2.0);
        assert_eq!(skeleton.pose[0].scale, Vec3::splat(2.0));
    }

    #[test]
    fn skinned_files_keep_their_nodes_as_joints() {
        let loaded = load(
            "skin.gltf",
            skinned_gltf().as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| false,
        )
        .unwrap();
        let skin = loaded.skin.expect("a skinned file has a skeleton");
        let names: Vec<&str> = skin.joints.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(
            names,
            ["Armature", "root", "tip", "cup", "body"],
            "every node, parents first"
        );
        let parents: Vec<Option<usize>> = skin.joints.iter().map(|j| j.parent).collect();
        assert_eq!(parents, [None, Some(0), Some(1), Some(2), None]);
        assert_eq!(skin.joints[2].rest.position, Vec3::Y);
        // Slots: root and tip from the skin, then cup's own node for the
        // plain triangle, bound where cup sits at rest (scaled by 2, up 2).
        assert_eq!(skin.slots, [1, 2, 3]);
        assert_eq!(skin.inverse_binds[1], Mat4::from_translation(-Vec3::Y));
        let cup_rest = Mat4::from_scale(Vec3::splat(2.0)) * Mat4::from_translation(Vec3::Y);
        assert!(skin.inverse_binds[2].abs_diff_eq(cup_rest.inverse(), 1e-6));
        let data = &loaded.data;
        assert_eq!(data.joints.len(), 6);
        // The skinned triangle stays where it was bound (its node's move
        // is ignored, as glTF says); weights add up to 1.
        assert_eq!(data.positions[..3], [Vec3::ZERO, Vec3::X, Vec3::Y]);
        assert_eq!(data.joints[0], [0, 1, 0, 0]);
        assert_eq!(data.weights[0], [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(data.joints[2], [1, 0, 0, 0]);
        // The plain one is placed by its node, all on its own slot.
        assert!(data.positions[4].abs_diff_eq(Vec3::new(2.0, 2.0, 0.0), 1e-6));
        assert_eq!(data.joints[3..], [[2, 0, 0, 0]; 3]);
        assert_eq!(data.weights[3..], [[1.0, 0.0, 0.0, 0.0]; 3]);
    }

    #[test]
    fn files_without_skins_have_no_joints() {
        let loaded = load(
            "tri.gltf",
            triangle_gltf(None).as_bytes(),
            &mut |f| Err(f.to_string()),
            &|_| false,
        )
        .unwrap();
        assert!(loaded.skin.is_none() && loaded.data.joints.is_empty());
    }

    #[test]
    fn missing_files_and_junk_are_errors() {
        assert!(load("junk.glb", b"not a gltf", &mut |_| Ok(vec![]), &|_| false).is_err());
        let external = triangle_gltf(Some("my%20tri.bin"));
        let err = load(
            "models/tri.gltf",
            external.as_bytes(),
            &mut |f| Err(format!("no {f}")),
            &|_| false,
        )
        .err()
        .unwrap();
        assert_eq!(err, "no models/my tri.bin");
    }
}
