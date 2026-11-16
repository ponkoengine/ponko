//! Mesh assets (class 43): vertex streams and index buffers stored as hex.

use crate::{Error, UnityObject};

/// How one vertex attribute is stored (Unity's `VertexAttributeFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    /// 32-bit float.
    Float32,
    /// 16-bit float.
    Float16,
    /// 0..255 as 0..1.
    UNorm8,
    /// -127..127 as -1..1.
    SNorm8,
    /// 0..65535 as 0..1.
    UNorm16,
    /// -32767..32767 as -1..1.
    SNorm16,
    /// Whole numbers (bone indices).
    UInt8,
    /// Whole numbers.
    SInt8,
    /// Whole numbers.
    UInt16,
    /// Whole numbers.
    SInt16,
    /// Whole numbers.
    UInt32,
    /// Whole numbers.
    SInt32,
}

impl VertexFormat {
    fn from_unity(code: i64) -> Option<Self> {
        use VertexFormat::*;
        Some(match code {
            0 => Float32,
            1 => Float16,
            2 => UNorm8,
            3 => SNorm8,
            4 => UNorm16,
            5 => SNorm16,
            6 => UInt8,
            7 => SInt8,
            8 => UInt16,
            9 => SInt16,
            10 => UInt32,
            11 => SInt32,
            _ => return None,
        })
    }

    /// Bytes per component.
    #[must_use]
    pub fn size(self) -> usize {
        use VertexFormat::*;
        match self {
            UNorm8 | SNorm8 | UInt8 | SInt8 => 1,
            Float16 | UNorm16 | SNorm16 | UInt16 | SInt16 => 2,
            Float32 | UInt32 | SInt32 => 4,
        }
    }

    fn read(self, b: &[u8]) -> f32 {
        use VertexFormat::*;
        match self {
            Float32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Float16 => half(u16::from_le_bytes([b[0], b[1]])),
            UNorm8 => f32::from(b[0]) / 255.0,
            SNorm8 => (f32::from(b[0] as i8) / 127.0).max(-1.0),
            UNorm16 => f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0,
            SNorm16 => (f32::from(i16::from_le_bytes([b[0], b[1]])) / 32767.0).max(-1.0),
            UInt8 => f32::from(b[0]),
            SInt8 => f32::from(b[0] as i8),
            UInt16 => f32::from(u16::from_le_bytes([b[0], b[1]])),
            SInt16 => f32::from(i16::from_le_bytes([b[0], b[1]])),
            UInt32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            SInt32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
        }
    }
}

/// A 16-bit float's value.
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let frac = f32::from(bits & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 if frac == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

/// Where one attribute is in the vertex data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    /// Which stream (each stream's vertices are stored one after another).
    pub stream: usize,
    /// Bytes from the start of a vertex in its stream.
    pub offset: usize,
    /// How each component is stored.
    pub format: VertexFormat,
    /// How many components (0: the mesh has no such attribute).
    pub dimension: usize,
}

/// A mesh asset's triangles in Unity's own space (left-handed, Y up): the
/// importer turns them into Ponko's.
///
/// ```
/// # let text = include_str!("../tests/fixtures/quad.asset");
/// let file = ponko_unity::UnityFile::parse(text).unwrap();
/// let mesh = ponko_unity::UnityMesh::from_object(&file.objects[0]).unwrap();
/// assert_eq!(mesh.positions.len(), 5);
/// assert_eq!(mesh.submeshes[0].len(), 6);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnityMesh {
    /// Skeleton and vertex influences, when the mesh is skinned.
    pub skin: Option<MeshSkin>,
    /// `m_Name`.
    pub name: String,
    /// Every vertex's position.
    pub positions: Vec<[f32; 3]>,
    /// Normals, one per vertex, or none.
    pub normals: Vec<[f32; 3]>,
    /// Tangents (w is the bitangent's sign), or none.
    pub tangents: Vec<[f32; 4]>,
    /// Vertex colors, or none.
    pub colors: Vec<[f32; 4]>,
    /// Texture coordinate sets 0 to 7 (the first two components), each
    /// one per vertex or empty.
    pub uvs: [Vec<[f32; 2]>; 8],
    /// Each submesh's triangle list, as vertex indices (its base vertex
    /// added). Submesh i uses the renderer's material i.
    pub submeshes: Vec<Vec<u32>>,
    /// Blend shapes (morph targets), in Unity's space.
    pub blend_shapes: Vec<BlendShape>,
}

/// One authored blend-shape frame: how far each vertex moves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlendShape {
    /// Its name (the model file's channel name).
    pub name: String,
    /// One position offset per vertex.
    pub positions: Vec<[f32; 3]>,
    /// One normal offset per vertex, or none.
    pub normals: Vec<[f32; 3]>,
    /// The weight it starts at, 0 to 1.
    pub weight: f32,
    /// Weight of this frame, in units where 1 means 100 percent (0 means 1
    /// for older callers). Frames of a channel share `channel`.
    pub frame_weight: f32,
    /// Original channel name; empty for an ordinary single-frame shape.
    pub channel: String,
}

/// A skeleton in Unity space; matrices are column-major.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshSkin {
    /// Explicit skeleton root, when supplied by a renderer.
    pub root: Option<usize>,
    /// Sampled animation clips targeting skeleton node indices.
    pub animations: Vec<SkinAnimation>,
    /// Skeleton nodes, including non-bone ancestors.
    pub nodes: Vec<SkinNode>,
    /// Node indices in the order used by vertex influences.
    pub joints: Vec<usize>,
    /// Inverse bind matrices in joint order.
    pub inverse_binds: Vec<[f32; 16]>,
    /// Four joint indices per vertex.
    pub indices: Vec<[u16; 4]>,
    /// Four normalized weights per vertex.
    pub weights: Vec<[f32; 4]>,
}

/// A sampled local-pose clip in Unity coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkinAnimation {
    /// Clip name.
    pub name: String,
    /// Unity's loop setting (preserved as glTF extras).
    pub looping: bool,
    /// Seconds from the start of the clip.
    pub times: Vec<f32>,
    /// STEP interpolation for translation, rotation and scale, per node.
    pub step: Vec<[bool; 3]>,
    /// One local matrix per sample for each skeleton node.
    pub nodes: Vec<Vec<[f32; 16]>>,
    /// Named blend-shape curves sampled at `times`, in 0..1 weight units.
    /// The bool selects STEP interpolation.
    pub morphs: Vec<(String, Vec<f32>, bool)>,
}

/// One skeleton node, relative to its parent (or the mesh for roots).
#[derive(Debug, Clone, PartialEq)]
pub struct SkinNode {
    /// Node name.
    pub name: String,
    /// Parent node index.
    pub parent: Option<usize>,
    /// Local column-major matrix.
    pub matrix: [f32; 16],
}

/// The most blend shape work a mesh may take: a position or normal per
/// vertex per frame (each frame is stored sparse, but expanded to every
/// vertex), at least one per frame, and one per sparse offset read.
const MAX_SHAPE_ENTRIES: usize = 1 << 25;

/// All authored frames from a mesh asset, with sparse offsets expanded.
fn blend_shapes(shapes: &crate::Value, count: usize) -> Result<Vec<BlendShape>, Error> {
    let vertices = shapes["vertices"].as_seq().unwrap_or_default();
    let frames = shapes["shapes"].as_seq().unwrap_or_default();
    let weights = shapes["fullWeights"].as_seq().unwrap_or_default();
    let xyz = |v: &crate::Value| ["x", "y", "z"].map(|k| v[k].as_f32().unwrap_or(0.0));
    let channels = shapes["channels"].as_seq().unwrap_or_default();
    // Each channel owns its own range of frames, as Unity writes them: no
    // two overlap, so there are at most as many shapes as frames.
    let index = |v: &crate::Value| usize::try_from(v.as_i64().unwrap_or(0)).unwrap_or(0);
    let mut ranges: Vec<(usize, usize)> = channels
        .iter()
        .map(|c| {
            let first = index(&c["frameIndex"]).min(frames.len());
            (first, first + index(&c["frameCount"]).min(frames.len() - first))
        })
        .filter(|(first, end)| first < end)
        .collect();
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[1].0 < w[0].1) {
        return Err(Error::invalid("blend shapes: two channels share frames"));
    }
    let mut out = Vec::new();
    let mut entries = 0usize;
    for c in channels {
        let first_frame = index(&c["frameIndex"]);
        let nframes = index(&c["frameCount"]);
        let channel = c["name"].as_str().unwrap_or_default();
        for (k, frame) in frames.iter().enumerate().skip(first_frame).take(nframes) {
            let first = index(&frame["firstVertex"]);
            let n = index(&frame["vertexCount"]);
            let normals = frame["hasNormals"].as_i64().unwrap_or(0) != 0;
            let weight = weights.get(k).and_then(crate::Value::as_f32).unwrap_or(100.) / 100.;
            let expanded = if normals { 2 * count } else { count };
            let read = n.min(vertices.len().saturating_sub(first));
            entries = entries.saturating_add(expanded.max(1)).saturating_add(read);
            if entries > MAX_SHAPE_ENTRIES {
                return Err(Error::invalid(format!(
                    "blend shapes: more than {MAX_SHAPE_ENTRIES} vertex offsets to expand or read"
                )));
            }
            let mut shape = BlendShape {
                name: if k + 1 == first_frame.saturating_add(nframes) {
                    channel.into()
                } else {
                    format!("{channel}@{weight}")
                },
                channel: channel.into(),
                frame_weight: weight,
                positions: vec![[0.; 3]; count],
                normals: if normals { vec![[0.; 3]; count] } else { Vec::new() },
                weight: 0.,
            };
            for v in vertices.iter().skip(first).take(n) {
                let Some(i) = v["index"]
                    .as_i64()
                    .and_then(|i| usize::try_from(i).ok())
                    .filter(|&i| i < count)
                else {
                    continue;
                };
                shape.positions[i] = xyz(&v["vertex"]);
                if normals {
                    shape.normals[i] = xyz(&v["normal"]);
                }
            }
            out.push(shape);
        }
    }
    Ok(out)
}

/// Unity's four-influence import policy.
pub(crate) fn influences(values: impl IntoIterator<Item = (u16, f32)>) -> ([u16; 4], [f32; 4]) {
    let mut values: Vec<_> = values
        .into_iter()
        .filter(|(_, w)| w.is_finite() && *w >= 0.001)
        .collect();
    values.sort_by(|a, b| b.1.total_cmp(&a.1));
    values.truncate(4);
    let sum: f32 = values.iter().map(|v| v.1).sum();
    let (mut indices, mut weights) = ([0; 4], [0.0; 4]);
    for (i, (joint, weight)) in values.into_iter().enumerate() {
        indices[i] = joint;
        weights[i] = weight / sum;
    }
    if sum == 0.0 {
        weights[0] = 1.0;
    }
    (indices, weights)
}

/// Unity's fixed channel order.
const POSITION: usize = 0;
const NORMAL: usize = 1;
const TANGENT: usize = 2;
const COLOR: usize = 3;
const UV0: usize = 4;

impl UnityMesh {
    /// Reads a `Mesh` object (class 43) saved uncompressed.
    ///
    /// # Errors
    ///
    /// If it isn't a mesh, it's compressed, it isn't made of triangles, or
    /// its data is shorter than its layout says.
    pub fn from_object(object: &UnityObject) -> Result<Self, Error> {
        if object.class_id != 43 {
            return Err(Error::invalid(format!(
                "{} isn't a Mesh (class {})",
                object.type_name, object.class_id
            )));
        }
        let m = &object.body;
        let name = m["m_Name"].as_str().unwrap_or_default().to_owned();
        let fail = |what: &str| Error::invalid(format!("mesh {name}: {what}"));
        let unsupported = |what: &str| Error::unsupported(format!("mesh {name}: {what}"));
        if m["m_MeshCompression"].as_i64().unwrap_or(0) != 0 {
            return Err(unsupported(
                "compressed meshes aren't supported (turn Mesh Compression off)",
            ));
        }
        let vd = &m["m_VertexData"];
        let count = vd["m_VertexCount"].as_i64().ok_or_else(|| fail("no m_VertexCount"))?;
        let count = usize::try_from(count).map_err(|_| fail("a negative m_VertexCount"))?;
        let channels: Vec<Channel> = vd["m_Channels"]
            .as_seq()
            .ok_or_else(|| fail("no m_Channels"))?
            .iter()
            .map(|c| {
                let small = |key: &str, max: usize| {
                    usize::try_from(c[key].as_i64().unwrap_or(0))
                        .ok()
                        .filter(|&v| v <= max)
                        .ok_or_else(|| fail(&format!("vertex channel {key} out of range")))
                };
                Ok(Channel {
                    // Unity has at most 4 streams, and vertices of at most
                    // a few hundred bytes.
                    stream: small("stream", 31)?,
                    offset: small("offset", 4096)?,
                    format: VertexFormat::from_unity(c["format"].as_i64().unwrap_or(0))
                        .ok_or_else(|| unsupported("unknown vertex format"))?,
                    dimension: (c["dimension"].as_i64().unwrap_or(0) & 0xf) as usize,
                })
            })
            .collect::<Result<_, Error>>()?;
        let data = hex(vd["_typelessdata"].as_str().unwrap_or_default());
        // Each stream: its vertices back to back, starting 16-byte aligned.
        let streams = channels
            .iter()
            .filter(|c| c.dimension > 0)
            .map(|c| c.stream + 1)
            .max()
            .unwrap_or(0);
        let mut strides = vec![0usize; streams];
        for c in channels.iter().filter(|c| c.dimension > 0) {
            strides[c.stream] = strides[c.stream].max(c.offset + c.format.size() * c.dimension);
        }
        let mut starts = vec![0usize; streams];
        let mut at = 0usize;
        for s in 0..streams {
            starts[s] = at;
            at = strides[s]
                .checked_mul(count)
                .and_then(|len| at.checked_add(len))
                .and_then(|end| end.checked_next_multiple_of(16))
                .ok_or_else(|| fail("vertex data too short"))?;
        }
        let read = |ch: usize, n: usize| -> Result<Vec<[f32; 4]>, Error> {
            let Some(c) = channels.get(ch).filter(|c| c.dimension > 0) else {
                return Ok(Vec::new());
            };
            let size = c.format.size();
            (0..count)
                .map(|v| {
                    let base = starts[c.stream] + v * strides[c.stream] + c.offset;
                    let mut out = [0.0, 0.0, 0.0, if ch == 12 || ch == 13 { 0.0 } else { 1.0 }];
                    for (k, slot) in out.iter_mut().enumerate().take(c.dimension.min(n)) {
                        let b = data
                            .get(base + k * size..base + (k + 1) * size)
                            .ok_or_else(|| fail("vertex data too short"))?;
                        *slot = c.format.read(b);
                    }
                    Ok(out)
                })
                .collect()
        };
        let xyz = |v: Vec<[f32; 4]>| v.into_iter().map(|p| [p[0], p[1], p[2]]).collect();
        let uvs: [Result<Vec<[f32; 2]>, Error>; 8] =
            std::array::from_fn(|i| read(UV0 + i, 2).map(|v| v.into_iter().map(|p| [p[0], p[1]]).collect()));
        let mut mesh = Self {
            positions: xyz(read(POSITION, 3)?),
            normals: xyz(read(NORMAL, 3)?),
            tangents: read(TANGENT, 4)?,
            colors: read(COLOR, 4)?,
            uvs: Default::default(),
            submeshes: Vec::new(),
            name: name.clone(),
            skin: None,
            blend_shapes: Vec::new(),
        };
        // As many vertices as the data holds positions for.
        mesh.blend_shapes =
            blend_shapes(&m["m_Shapes"], mesh.positions.len()).map_err(|e| e.with_context(&format!("mesh {name}")))?;
        for (slot, uv) in mesh.uvs.iter_mut().zip(uvs) {
            *slot = uv?;
        }
        let binds = m["m_BindPose"].as_seq().unwrap_or_default();
        if !binds.is_empty() {
            if binds.len() > u16::MAX as usize {
                return Err(unsupported("too many skin joints"));
            }
            let inverse_binds: Vec<[f32; 16]> = binds
                .iter()
                .map(|b| std::array::from_fn(|i| b[&format!("e{}{}", i % 4, i / 4)[..]].as_f32().unwrap_or(0.0)))
                .collect();
            let nodes = inverse_binds
                .iter()
                .enumerate()
                .map(|(i, bind)| SkinNode {
                    name: format!("Bone{i}"),
                    parent: None,
                    matrix: glam::Mat4::from_cols_array(bind).inverse().to_cols_array(),
                })
                .collect();
            let legacy = m["m_Skin"]
                .as_seq()
                .filter(|s| !s.is_empty())
                .or_else(|| m["m_BoneWeights"].as_seq().filter(|s| !s.is_empty()));
            let (channel_weights, channel_indices) = (read(12, 4)?, read(13, 4)?);
            let mut skin = MeshSkin {
                nodes,
                joints: (0..binds.len()).collect(),
                inverse_binds,
                ..Default::default()
            };
            for v in 0..count {
                let values: Vec<_> = if let Some(legacy) = legacy {
                    let b = legacy.get(v).ok_or_else(|| fail("bone weights too short"))?;
                    (0..4)
                        .map(|k| {
                            (
                                b[&format!("boneIndex{k}")[..]].as_i64().unwrap_or(0) as u16,
                                b[&format!("weight{k}")[..]].as_f32().unwrap_or(0.0),
                            )
                        })
                        .collect()
                } else {
                    let weights = channel_weights.get(v).ok_or_else(|| fail("missing blend weights"))?;
                    let indices = channel_indices.get(v).ok_or_else(|| fail("missing blend indices"))?;
                    (0..4).map(|k| (indices[k] as u16, weights[k])).collect()
                };
                if values.iter().any(|&(j, w)| w >= 0.001 && j as usize >= binds.len()) {
                    return Err(fail("bone index out of range"));
                }
                let (indices, weights) = influences(values);
                skin.indices.push(indices);
                skin.weights.push(weights);
            }
            mesh.skin = Some(skin);
        }
        // Indices: 16 or 32 bits each.
        let wide = m["m_IndexFormat"].as_i64().unwrap_or(0) == 1;
        let indices = hex(m["m_IndexBuffer"].as_str().unwrap_or_default());
        let index = |i: usize| -> Option<u32> {
            if wide {
                let at = i.checked_mul(4)?;
                indices
                    .get(at..at.checked_add(4)?)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            } else {
                let at = i.checked_mul(2)?;
                indices
                    .get(at..at.checked_add(2)?)
                    .map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])))
            }
        };
        // Submeshes may share indices, but not read the buffer over and
        // over: at most four times its size in all.
        let mut budget = indices.len() / if wide { 4 } else { 2 } * 4;
        let short = || fail("index buffer too short or out of range");
        for sub in m["m_SubMeshes"].as_seq().unwrap_or_default() {
            if sub["topology"].as_i64().unwrap_or(0) != 0 {
                return Err(unsupported("only triangle submeshes are supported"));
            }
            let first = usize::try_from(sub["firstByte"].as_i64().unwrap_or(0)).map_err(|_| short())?
                / if wide { 4 } else { 2 };
            let n = usize::try_from(sub["indexCount"].as_i64().unwrap_or(0)).map_err(|_| short())?;
            let base = u32::try_from(sub["baseVertex"].as_i64().unwrap_or(0)).map_err(|_| short())?;
            budget = budget.checked_sub(n).ok_or_else(short)?;
            let end = first.checked_add(n).ok_or_else(short)?;
            let list = (first..end)
                .map(|i| {
                    index(i)
                        .and_then(|v| v.checked_add(base))
                        .filter(|&v| (v as usize) < count)
                })
                .collect::<Option<Vec<u32>>>()
                .ok_or_else(short)?;
            mesh.submeshes.push(list);
        }
        Ok(mesh)
    }
}

/// Bytes from hex text: its leading whole hex pairs. Some files carry a
/// stray character after the data, which Unity ignores too; a buffer that
/// comes out short for its layout is still an error where it's read.
pub(crate) fn hex(text: &str) -> Vec<u8> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    text.trim()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map_while(|&[hi, lo]| Some(digit(hi)? << 4 | digit(lo)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_assets_keep_all_blend_shape_frames() {
        let file = crate::UnityFile::parse("--- !u!43 &1\nMesh:\n  m_Shapes:\n    channels: [{name: Raise, frameIndex: 0, frameCount: 2}]\n    fullWeights: [50, 100]\n    shapes: [{firstVertex: 0, vertexCount: 1, hasNormals: 1}, {firstVertex: 1, vertexCount: 1, hasNormals: 0}]\n    vertices:\n    - {index: 0, vertex: {x: 1, y: 2, z: 3}, normal: {x: 0, y: 1, z: 0}}\n    - {index: 0, vertex: {x: 4, y: 5, z: 6}}\n").unwrap();
        let mesh = file.of_class(43).next().unwrap();
        let frames = blend_shapes(&mesh.body["m_Shapes"], 2).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].frame_weight, frames[1].frame_weight), (0.5, 1.));
        assert_eq!(frames[0].channel, "Raise");
        assert_eq!(frames[1].name, "Raise");
        assert_eq!(frames[0].positions, [[1., 2., 3.], [0.; 3]]);
        assert_eq!(frames[1].positions[0], [4., 5., 6.]);
        assert_eq!(frames[0].normals[0], [0., 1., 0.]);
    }
    #[test]
    fn vertex_data_skin_channels_decode_and_normalize() {
        let mut bytes = Vec::new();
        for n in [0.0f32, 0.0, 0.0, 0.2, 0.6, 0.0001, 0.0] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend([0, 1, 0, 0]);
        let data: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let mut channels = String::from("    - {stream: 0, offset: 0, format: 0, dimension: 3}\n");
        for _ in 1..12 {
            channels.push_str("    - {dimension: 0}\n");
        }
        channels.push_str("    - {stream: 0, offset: 12, format: 0, dimension: 4}\n    - {stream: 0, offset: 28, format: 6, dimension: 4}\n");
        let text = format!(
            "--- !u!43 &1\nMesh:\n  m_VertexData:\n    m_VertexCount: 1\n    m_Channels:\n{channels}    _typelessdata: {data}\n  m_BindPose:\n  - {{e00: 1, e11: 1, e22: 1, e33: 1}}\n  - {{e00: 1, e11: 1, e22: 1, e33: 1}}\n"
        );
        let file = crate::UnityFile::parse(&text).unwrap();
        let mesh = UnityMesh::from_object(&file.objects[0]).unwrap();
        let skin = mesh.skin.unwrap();
        assert_eq!(skin.indices, [[1, 0, 0, 0]]);
        assert_eq!(skin.weights, [[0.75, 0.25, 0.0, 0.0]]);
    }

    #[test]
    fn halves_decode() {
        assert_eq!(half(0x3c00), 1.0);
        assert_eq!(half(0xc000), -2.0);
        assert_eq!(half(0x3800), 0.5);
        assert_eq!(half(0), 0.0);
        assert!(half(0x7c00).is_infinite());
    }

    #[test]
    fn hex_decodes_its_leading_whole_pairs() {
        assert_eq!(hex("00ff10"), vec![0, 255, 16]);
        assert_eq!(hex("00ff1"), vec![0, 255], "a stray digit is left off");
        assert_eq!(hex("00ffy"), vec![0, 255], "so is a stray letter");
        assert_eq!(hex("00zz11"), vec![0], "decoding stops at the first non-hex pair");
    }
}
