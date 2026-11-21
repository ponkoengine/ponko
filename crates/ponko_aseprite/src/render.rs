//! Render the layer tree around aseprite-loader's pixel blender. Its raw
//! parser keeps tileset chunks and the original layer indices.

use std::collections::{BTreeMap, BTreeSet};

use aseprite_loader::{
    binary::{
        blend_mode::BlendMode,
        chunk::Chunk,
        chunks::{
            cel::{CelChunk, CelContent},
            layer::{LayerChunk, LayerFlags, LayerType},
            tileset::TilesetChunk,
        },
        color_depth::ColorDepth,
        file::File,
        header::Header,
        image::Image,
        palette::{Palette, create_palette},
        raw_file::RawFile,
        raw_file::parse_raw_file,
    },
    loader::{AsepriteFile, Frame, FrameCel, Layer, LayerSelection},
};

use crate::{Load, MAX_BYTES, rgba_len, tiles::Tileset};

/// How many files deep a tileset may link to another file's tileset.
const MAX_LINKS: usize = 8;

struct Node<'a> {
    layer: &'a LayerChunk<'a>,
    visible: bool,
    children: Vec<usize>,
    end: usize,
}

pub(super) struct Renderer<'a> {
    header: Header,
    palette: Option<Palette>,
    nodes: Vec<Node<'a>>,
    roots: Vec<usize>,
    cels: Vec<BTreeMap<usize, &'a CelChunk<'a>>>,
    tilesets: BTreeMap<u32, &'a TilesetChunk<'a>>,
    files: BTreeMap<u32, &'a str>,
    decoded_tilesets: BTreeMap<u32, Tileset>,
    tileset_budget: usize,
}

impl<'a> Renderer<'a> {
    pub(super) fn new(file: &'a RawFile<'a>) -> Result<Self, String> {
        let mut nodes = Vec::<Node<'_>>::new();
        let mut roots = Vec::new();
        let mut groups: Vec<usize> = Vec::new();
        let mut tilesets = BTreeMap::new();
        let files = external_files(file);
        for chunk in file.frames.iter().flat_map(|f| &f.chunks) {
            match chunk {
                Chunk::Layer(layer) => {
                    let level = usize::from(layer.child_level);
                    let index = nodes.len();
                    if level > groups.len() || level > 64 {
                        return Err(format!("invalid nesting of layer {:?}", layer.name));
                    }
                    for group in groups.drain(level..) {
                        nodes[group].end = index;
                    }
                    let mut visible = layer.flags.contains(LayerFlags::VISIBLE);
                    if let Some(&parent) = groups.last() {
                        nodes[parent].children.push(index);
                        visible &= nodes[parent].visible;
                    } else {
                        roots.push(index);
                    }
                    if layer.layer_type == LayerType::Group {
                        groups.push(index);
                    }
                    nodes.push(Node {
                        layer,
                        visible,
                        children: Vec::new(),
                        end: index + 1,
                    });
                }
                Chunk::Tileset(tileset) => {
                    tilesets.insert(tileset.id, tileset);
                }
                _ => {}
            }
        }
        for group in groups {
            nodes[group].end = nodes.len();
        }
        let cels = file
            .frames
            .iter()
            .map(|frame| {
                frame
                    .cels()
                    .map(|cel| {
                        let i = usize::from(cel.layer_index);
                        if i >= nodes.len() {
                            return Err(format!("cel references missing layer {i}"));
                        }
                        Ok((i, cel))
                    })
                    .collect()
            })
            .collect::<Result<_, String>>()?;
        let palette = if file.header.color_depth == ColorDepth::Indexed {
            Some(create_palette(&file.header, &file.frames).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Self {
            header: file.header,
            palette,
            nodes,
            roots,
            cels,
            tilesets,
            files,
            decoded_tilesets: BTreeMap::new(),
            tileset_budget: MAX_BYTES,
        })
    }

    /// Decodes the tilesets the tilemap layers use that link to a tileset
    /// in another file instead of holding their tiles; `path` is this
    /// file's asset path, which the links are relative to.
    pub(super) fn link_tilesets(&mut self, path: &str, load: &mut Load<'_>) -> Result<(), String> {
        let used: BTreeSet<u32> = self
            .nodes
            .iter()
            .filter(|node| node.layer.layer_type == LayerType::Tilemap)
            .filter_map(|node| node.layer.tileset_index)
            .collect();
        for id in used {
            // A missing tileset or one without tiles or link is reported
            // where a layer draws it.
            let Some(&chunk) = self.tilesets.get(&id) else { continue };
            let (None, Some(link)) = (&chunk.tiles, chunk.external_file) else {
                continue;
            };
            let name = self
                .files
                .get(&link.external_file_id)
                .ok_or_else(|| format!("tileset {:?} links to a file the file doesn't list", chunk.name))?;
            let tiles = linked_tileset(
                path,
                name,
                link.tileset_id,
                (chunk.width, chunk.height),
                load,
                self.tileset_budget,
                0,
            )
            .map_err(|e| format!("tileset {:?}: {e}", chunk.name))?;
            self.tileset_budget -= tiles.byte_len();
            self.decoded_tilesets.insert(id, tiles);
        }
        Ok(())
    }

    pub(super) fn split_layers(&self) -> Result<Vec<(usize, &'a str)>, String> {
        let mut names = BTreeSet::new();
        let mut layers = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if !node.visible || !matches!(node.layer.layer_type, LayerType::Normal | LayerType::Tilemap) {
                continue;
            }
            let name = node.layer.name;
            if !names.insert(name) {
                return Err(format!(
                    "two visible layers are named \"{name}\": split layers need names of their own"
                ));
            }
            layers.push((i, name));
        }
        if layers.is_empty() {
            return Err("it has no visible layer to split".into());
        }
        Ok(layers)
    }

    pub(super) fn render(&mut self, frame: usize, selected: Option<usize>, canvas: &mut [u8]) -> Result<(), String> {
        self.draw_layers(frame, self.roots.clone(), selected, canvas, 1)
    }

    fn draw_layers(
        &mut self,
        frame: usize,
        layers: Vec<usize>,
        selected: Option<usize>,
        canvas: &mut [u8],
        surfaces: usize,
    ) -> Result<(), String> {
        for i in layers {
            let node = &self.nodes[i];
            let layer = node.layer;
            if !node.visible || selected.is_some_and(|selection| !(i..node.end).contains(&selection)) {
                continue;
            }
            match layer.layer_type {
                LayerType::Group => {
                    let children = node.children.clone();
                    if self.header.flags & 2 == 0 {
                        self.draw_layers(frame, children, selected, canvas, surfaces)?;
                    } else if layer.opacity > 0 && !children.is_empty() {
                        if surfaces >= MAX_BYTES / canvas.len() {
                            return Err("isolated groups exceed the 256 MiB compositing limit".into());
                        }
                        let mut group = vec![0; canvas.len()];
                        self.draw_layers(frame, children, selected, &mut group, surfaces + 1)?;
                        self.blend(
                            canvas,
                            (0, 0),
                            (self.header.width, self.header.height),
                            &group,
                            layer.opacity,
                            layer.blend_mode,
                        )?;
                    }
                }
                LayerType::Normal | LayerType::Tilemap => {
                    let Some(&cel) = self.cels[frame].get(&i) else { continue };
                    let content = self.linked_content(frame, i)?;
                    let opacity = if self.header.flags & 1 != 0 {
                        ((u16::from(layer.opacity) * u16::from(cel.opacity) + 127) / 255) as u8
                    } else {
                        cel.opacity
                    };
                    let origin = (cel.x, cel.y);
                    match content {
                        CelContent::Image(image) if layer.layer_type == LayerType::Normal => {
                            let pixel_count = rgba_len(image.width.into(), image.height.into())? / 4;
                            let bytes =
                                decode_pixels(image.data, image.compressed, pixel_count, self.header.color_depth)?;
                            let rgba = to_rgba(&bytes, self.header.color_depth, self.palette.as_ref())?;
                            self.blend(
                                canvas,
                                origin,
                                (image.width, image.height),
                                rgba.as_ref(),
                                opacity,
                                layer.blend_mode,
                            )?;
                        }
                        CelContent::CompressedTilemap { .. } if layer.layer_type == LayerType::Tilemap => {
                            let id = layer.tileset_index.ok_or("tilemap layer does not name a tileset")?;
                            let tileset = self
                                .tilesets
                                .get(&id)
                                .ok_or_else(|| format!("missing tileset {id} used by layer {:?}", layer.name))?;
                            if let std::collections::btree_map::Entry::Vacant(entry) = self.decoded_tilesets.entry(id) {
                                let tiles = Tileset::decode(
                                    tileset,
                                    self.header.color_depth,
                                    self.palette.as_ref(),
                                    self.tileset_budget,
                                )?;
                                self.tileset_budget -= tiles.byte_len();
                                entry.insert(tiles);
                            }
                            let rendered = self.decoded_tilesets[&id].render(content, origin, &self.header)?;
                            if let Some(raster) = rendered {
                                self.blend(
                                    canvas,
                                    raster.origin,
                                    raster.size,
                                    &raster.rgba,
                                    opacity,
                                    layer.blend_mode,
                                )?;
                            }
                        }
                        _ => return Err(format!("invalid cel for layer {:?}", layer.name)),
                    }
                }
                LayerType::Unknown(_) => {}
            }
        }
        Ok(())
    }

    fn linked_content(&self, mut frame: usize, layer: usize) -> Result<&'a CelContent<'a>, String> {
        for _ in 0..self.cels.len() {
            let cel = self
                .cels
                .get(frame)
                .and_then(|f| f.get(&layer))
                .ok_or_else(|| format!("linked cel points to missing frame {frame}, layer {layer}"))?;
            if let CelContent::LinkedCel { frame_position } = cel.content {
                frame = usize::from(frame_position);
            } else {
                return Ok(&cel.content);
            }
        }
        Err(format!("cyclic linked cel in layer {layer}"))
    }

    // The layer/group raster is already validated straight-alpha RGBA8.
    // Use the existing blender on it so all blend modes match the old cook.
    fn blend(
        &self,
        canvas: &mut [u8],
        origin: (i16, i16),
        size: (u16, u16),
        rgba: &[u8],
        opacity: u8,
        blend_mode: BlendMode,
    ) -> Result<(), String> {
        let file = AsepriteFile {
            file: File {
                header: Header {
                    color_depth: ColorDepth::Rgba,
                    ..self.header
                },
                palette: None,
                layers: Vec::new(),
                frames: Vec::new(),
                tags: Vec::new(),
                slices: Vec::new(),
            },
            layers: vec![Layer {
                name: String::new(),
                opacity,
                blend_mode,
                visible: true,
                layer_type: LayerType::Normal,
            }],
            frames: vec![Frame {
                duration: 0,
                origin: (0, 0),
                cels: vec![FrameCel {
                    origin,
                    size,
                    layer_index: 0,
                    image_index: 0,
                }],
            }],
            tags: Vec::new(),
            images: vec![Image {
                width: size.0,
                height: size.1,
                data: rgba,
                compressed: false,
            }],
        };
        file.render_frame(0, canvas, &LayerSelection::All)
            .map_err(|err| err.to_string())
    }
}

/// The external files a file lists, by entry ID.
fn external_files<'a>(file: &'a RawFile<'a>) -> BTreeMap<u32, &'a str> {
    file.frames
        .iter()
        .flat_map(|f| &f.chunks)
        .filter_map(|chunk| match chunk {
            Chunk::ExternalFiles(list) => Some(list.files.iter().map(|f| (f.entry_id, f.file_name))),
            _ => None,
        })
        .flatten()
        .collect()
}

/// The asset paths of the files `file`'s tilesets link to instead of
/// holding their tiles, resolved against `path`.
pub(super) fn linked_files(path: &str, file: &RawFile<'_>) -> Result<Vec<String>, String> {
    let files = external_files(file);
    let mut out = BTreeSet::new();
    for chunk in file.frames.iter().flat_map(|f| &f.chunks) {
        if let Chunk::Tileset(tileset) = chunk
            && tileset.tiles.is_none()
            && let Some(link) = tileset.external_file
            && let Some(name) = files.get(&link.external_file_id)
        {
            out.insert(resolve(path, name)?);
        }
    }
    Ok(out.into_iter().collect())
}

/// Tileset `id` of the file `name` links to (relative to `base`), whose
/// tiles must be `size`. That tileset may link on, up to [`MAX_LINKS`]
/// files deep.
fn linked_tileset(
    base: &str,
    name: &str,
    id: u32,
    size: (u16, u16),
    load: &mut Load<'_>,
    budget: usize,
    depth: usize,
) -> Result<Tileset, String> {
    if depth >= MAX_LINKS {
        return Err(format!(
            "its links go more than {MAX_LINKS} files deep; is there a loop?"
        ));
    }
    let path = resolve(base, name)?;
    let bytes = load(&path).map_err(|e| format!("its file {path} can't be read: {e}"))?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("its file {path} exceeds the 256 MiB byte limit"));
    }
    let file = parse_raw_file(&bytes).map_err(|e| format!("its file {path} is not a readable Aseprite file: {e}"))?;
    let chunk = file
        .frames
        .iter()
        .flat_map(|f| &f.chunks)
        .find_map(|chunk| match chunk {
            Chunk::Tileset(tileset) if tileset.id == id => Some(tileset),
            _ => None,
        })
        .ok_or_else(|| format!("its file {path} has no tileset {id}"))?;
    if (chunk.width, chunk.height) != size {
        return Err(format!(
            "tileset {:?} of {path} has {} x {} tiles, but the link expects {} x {}",
            chunk.name, chunk.width, chunk.height, size.0, size.1
        ));
    }
    if let (None, Some(link)) = (&chunk.tiles, chunk.external_file) {
        let files = external_files(&file);
        let next = files
            .get(&link.external_file_id)
            .ok_or_else(|| format!("tileset {:?} of {path} links to a file {path} doesn't list", chunk.name))?;
        return linked_tileset(&path, next, link.tileset_id, size, load, budget, depth + 1);
    }
    let palette = if file.header.color_depth == ColorDepth::Indexed {
        Some(create_palette(&file.header, &file.frames).map_err(|e| format!("{path}: {e}"))?)
    } else {
        None
    };
    Tileset::decode(chunk, file.header.color_depth, palette.as_ref(), budget).map_err(|e| format!("{path}: {e}"))
}

/// Resolves `name`, a path relative to the file at asset path `base`, to
/// an asset path. Absolute paths and paths above the root are refused.
pub(super) fn resolve(base: &str, name: &str) -> Result<String, String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return Err(format!(
            "it links to {name:?}, an absolute path: link the tileset file by a path relative to this one"
        ));
    }
    let base = base.replace('\\', "/");
    let root = if base.starts_with('/') { "/" } else { "" };
    let mut parts: Vec<&str> = match base.rsplit_once('/') {
        Some((folder, _)) => folder.split('/').filter(|p| !p.is_empty() && *p != ".").collect(),
        None => Vec::new(),
    };
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => match parts.last() {
                Some(p) if *p != ".." && !p.ends_with(':') => {
                    parts.pop();
                }
                _ => return Err(format!("it links to {name:?}, outside the folders it can read")),
            },
            p => parts.push(p),
        }
    }
    Ok(format!("{root}{}", parts.join("/")))
}

pub(super) fn inflate(bytes: &[u8], len: usize) -> Result<Vec<u8>, String> {
    let mut output = vec![0; len];
    let mut decoder = flate2::Decompress::new(true);
    let status = decoder
        .decompress(bytes, &mut output, flate2::FlushDecompress::Finish)
        .map_err(|e| format!("invalid zlib data: {e}"))?;
    if status != flate2::Status::StreamEnd || decoder.total_out() != len as u64 {
        return Err("zlib data does not match the expected size".into());
    }
    Ok(output)
}

pub(super) fn decode_pixels(
    bytes: &[u8],
    compressed: bool,
    pixels: usize,
    depth: ColorDepth,
) -> Result<std::borrow::Cow<'_, [u8]>, String> {
    let len = depth.pixel_size().ok_or("unsupported Aseprite color depth")? * pixels;
    if compressed {
        Ok(inflate(bytes, len)?.into())
    } else if bytes.len() == len {
        Ok(bytes.into())
    } else {
        Err("raw pixel data does not match the cel size".into())
    }
}

pub(super) fn to_rgba<'a>(
    data: &'a [u8],
    depth: ColorDepth,
    palette: Option<&Palette>,
) -> Result<std::borrow::Cow<'a, [u8]>, String> {
    match depth {
        ColorDepth::Rgba => Ok(data.into()),
        ColorDepth::Grayscale => Ok(data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[v, a]| [v, v, v, a])
            .collect::<Vec<_>>()
            .into()),
        ColorDepth::Indexed => {
            let colors = &palette.ok_or("missing indexed palette")?.colors;
            Ok(data
                .iter()
                .flat_map(|&i| {
                    let c = colors[usize::from(i)];
                    [c.red, c.green, c.blue, c.alpha]
                })
                .collect::<Vec<_>>()
                .into())
        }
        ColorDepth::Unknown(_) => Err("unsupported Aseprite color depth".into()),
    }
}
