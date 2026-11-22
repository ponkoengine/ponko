//! Terrain files: a tile of terrain kept in `assets/` rather than made in
//! code, as the editor's terrain window saves it. The file is JSON:
//!
//! ```json
//! {
//!   "format": "ponko-terrain", "version": 1,
//!   "size": 64.0,
//!   "heights": "hill-heights.png", "low": -8.0, "high": 24.0,
//!   "layers": [{ "texture": "grass.png", "size": 4.0 }, { "texture": "rock.png", "size": 6.0 }],
//!   "weights": ["hill-weights.png"],
//!   "roughness": 0.9
//! }
//! ```
//!
//! `heights` is a square greyscale PNG (16 bits a pixel for smooth slopes;
//! 8 work too) of n × n heights, n at least 2, its rows along +Z: black is
//! `low`, white is `high`. `layers` (up to 8, optional) are the ground
//! textures, each repeating every `size` world units, and `weights` are
//! RGBA PNGs over the whole tile saying how much of each layer shows where
//! (red, green, blue and alpha of the first image are layers 0 to 3, of the
//! second 4 to 7). Without layers the tile is drawn with the default
//! material. Names are relative to the file.

use glam::{Vec2, Vec3};
use ponko_ecs::{Component, Entity};
use serde::{Deserialize, Serialize};

use super::{Material, TerrainMaterialDesc, TerrainTile, TextureArray, TextureKind};
use crate::ctx::Ctx;
use crate::error::Error;

/// The format name a terrain file has.
pub(crate) const TERRAIN_FORMAT: &str = "ponko-terrain";

fn version() -> u32 {
    1
}

fn layer_size() -> f32 {
    4.0
}

fn roughness() -> f32 {
    0.9
}

/// A terrain file's JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerrainJson {
    pub format: String,
    #[serde(default = "version")]
    pub version: u32,
    pub size: f32,
    pub heights: String,
    #[serde(default)]
    pub low: f32,
    pub high: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<String>,
    #[serde(default = "roughness")]
    pub roughness: f32,
}

/// One ground layer of a terrain file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayerJson {
    pub texture: String,
    #[serde(default = "layer_size")]
    pub size: f32,
}

/// A terrain file read: its JSON, its heights as stored (0 is `low`,
/// 65535 `high`), n × n, and its weight images (RGBA, `side` × `side`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerrainData {
    pub json: TerrainJson,
    pub n: usize,
    pub heights: Vec<u16>,
    pub side: u32,
    pub weights: Vec<Vec<u8>>,
}

/// The terrain file an entity's [`TerrainTile`] is made from (`"format":
/// "ponko-terrain"`, see [`TerrainTile::spawn_from_file`], which adds it).
///
/// Scenes save it in place of the tile, which holds GPU meshes: the
/// entity's [`d3::Transform`](crate::d3::Transform) places the tile's
/// corner, and loading the scene makes the tile from the file again (an
/// entity whose tile is still live at the same corner keeps it, so the
/// editor's Stop and undo don't remake it). It is reflected as
/// `"TerrainFile"`:
///
/// ```json
/// { "Transform3D": { "position": [-32, 0, -32] }, "TerrainFile": { "path": "terrain/hill.json" } }
/// ```
///
/// The tile itself isn't reflected, so a save counts it under
/// `"not_saved"`.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # use ponko::terrain::{TerrainFile, TerrainTile};
/// # fn f(ctx: &mut Ctx) -> Result<(), ponko::error::Error> {
/// let hill = TerrainTile::spawn_from_file(ctx, "terrain/hill.json", vec3(-32.0, 0.0, -32.0))?;
/// assert_eq!(ctx.world.get::<TerrainFile>(hill).unwrap().path, "terrain/hill.json");
/// ctx.assets.save_scene(&ctx.world, "levels/one.json")?; // the hill is in it
/// # Ok(()) }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Component, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TerrainFile {
    /// The terrain file, an asset name.
    pub path: String,
}

impl TerrainFile {
    /// The tile of terrain file `path`.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// A tile made from a terrain file: the file, where its corner is now,
/// and what moving it changes.
struct FileTile {
    path: String,
    corner: Vec3,
    /// Its terrain material and what that was made from (`None`: drawn
    /// with the default material, which doesn't depend on the corner).
    ground: Option<(Material, TerrainMaterialDesc)>,
    /// Its heights in world units and its size, for a collider.
    #[cfg(feature = "physics3d")]
    heights: std::sync::Arc<[f32]>,
    #[cfg(feature = "physics3d")]
    size: f32,
}

/// The tiles made from terrain files, so a scene loaded again gets the
/// tiles it had, and a tile whose entity moved is re-placed.
#[derive(Default)]
pub(crate) struct FileTiles {
    tiles: std::collections::HashMap<TerrainTile, FileTile>,
    /// The last frame a drag in the editor moved entities: tiles are
    /// re-placed once a frame passes without one, when the drag ended.
    pub(crate) held: Option<u64>,
}

/// Remembers that `built` is terrain file `path`'s tile (made from `data`)
/// at `corner`.
pub(crate) fn note_tile(ctx: &mut Ctx, path: &str, corner: Vec3, built: &Built, data: &TerrainData) {
    // Tiles freed since (the editor's terrain window makes them again).
    let dead: Vec<TerrainTile> = ctx
        .gfx
        .assets3d
        .terrain_files
        .tiles
        .keys()
        .filter(|t| !ctx.gfx.is_live(**t))
        .copied()
        .collect();
    let files = &mut ctx.gfx.assets3d.terrain_files;
    for t in dead {
        files.tiles.remove(&t);
    }
    #[cfg(not(feature = "physics3d"))]
    let _ = data;
    files.tiles.insert(
        built.tile,
        FileTile {
            path: path.to_string(),
            corner,
            ground: built.material.map(|(m, _)| m).zip(built.ground),
            #[cfg(feature = "physics3d")]
            heights: data.world_heights().into(),
            #[cfg(feature = "physics3d")]
            size: data.json.size,
        },
    );
}

/// The terrain files a scene names that no live tile was made from yet,
/// read with their layer textures before the scene replaces the world, so
/// a file that doesn't load stops the load with the world unchanged.
pub(crate) fn prepare_scene(ctx: &mut Ctx, save: &serde_json::Value) -> Result<Prepared, Error> {
    let mut names = Vec::new();
    crate::reflect::asset_names(save, "TerrainFile", "path", &mut names);
    names.sort_unstable();
    names.dedup();
    let mut out = Prepared::default();
    for path in names {
        let made = ctx
            .gfx
            .assets3d
            .terrain_files
            .tiles
            .iter()
            .any(|(t, f)| f.path == path && ctx.gfx.is_live(*t));
        if !made {
            let read = read_file(ctx, &path)?;
            out.files.insert(path, read);
        }
    }
    Ok(out)
}

/// Terrain files read ahead of a scene load: data and layer textures, by path.
#[derive(Default)]
pub(crate) struct Prepared {
    files: std::collections::HashMap<String, (TerrainData, Option<TextureArray>)>,
}

/// Reads terrain file `path` and its weight pictures, which follow the image
/// limits of the source the file came from.
fn read_data(ctx: &Ctx, path: &str) -> Result<TerrainData, Error> {
    let (origin, text) = ctx.assets.read_text(path)?;
    let sources = &ctx.assets.asset_sources;
    sources.image_limits().scope(&origin, || {
        TerrainData::read(path, &text, &mut |name| {
            sources.find(name)?.into_bytes().map_err(|e| e.with_path(name))
        })
    })
}

fn read_file(ctx: &mut Ctx, path: &str) -> Result<(TerrainData, Option<TextureArray>), Error> {
    let data = read_data(ctx, path)?;
    let layers = load_layers(ctx, path, &data)?;
    Ok((data, layers))
}

/// The entities with a [`TerrainFile`] and a 3D transform but no
/// [`TerrainTile`], with their corners and files.
fn wanting(ctx: &mut Ctx) -> Vec<(Entity, Vec3, String)> {
    ctx.world
        .query::<(Entity, &crate::d3::Transform, &TerrainFile)>()
        .filter::<ponko_ecs::Without<TerrainTile>>()
        .iter()
        .map(|(e, t, f)| (e, t.position, f.path.clone()))
        .collect()
}

/// Gives every entity with a [`TerrainFile`] and a 3D transform but no
/// [`TerrainTile`] its tile (see [`give`]), from `prepared` for the files
/// read ahead.
///
/// # Errors
/// The first file that doesn't load; the entities before it have tiles.
pub(crate) fn restore_tiles(ctx: &mut Ctx, prepared: Prepared) -> Result<(), Error> {
    let wanting = wanting(ctx);
    give(ctx, wanting, prepared).map_err(|(_, e)| e)
}

/// Gives those of `entities` that have a [`TerrainFile`] and a 3D
/// transform but no [`TerrainTile`] their tiles: what a prefab spawn, an
/// editor duplicate or undo does after spawning them.
///
/// # Errors
/// The first file that doesn't load, naming it; the caller despawns what
/// it spawned, so no entity is left without its tile.
pub(crate) fn give_tiles(ctx: &mut Ctx, entities: &[Entity]) -> Result<(), Error> {
    let mut wanting = Vec::new();
    for &e in entities {
        if ctx.world.get::<TerrainTile>(e).is_some() {
            continue;
        }
        if let (Some(t), Some(f)) = (
            ctx.world.get::<crate::d3::Transform>(e),
            ctx.world.get::<TerrainFile>(e),
        ) {
            wanting.push((e, t.position, f.path.clone()));
        }
    }
    give(ctx, wanting, Prepared::default()).map_err(|(_, e)| e)
}

/// Gives each of `wanting` (entity, corner, file) its tile: a live tile
/// made from the same file that no other entity has (the one at the same
/// corner first, else one re-placed there), else one made from the file
/// (from `prepared` if it was read ahead). With `physics3d`, a
/// [`TerrainCollider`](crate::d3::TerrainCollider) on the entity without a
/// live mesh (as a scene or prefab loads it) gets the file's heightfield.
///
/// # Errors
/// The entity whose file doesn't load and why; the ones before it have tiles.
fn give(ctx: &mut Ctx, wanting: Vec<(Entity, Vec3, String)>, mut prepared: Prepared) -> Result<(), (Entity, Error)> {
    if wanting.is_empty() {
        return Ok(());
    }
    let mut taken: std::collections::HashSet<TerrainTile> = ctx.world.query::<&TerrainTile>().iter().copied().collect();
    for (e, corner, path) in wanting {
        let free = |t: &TerrainTile, f: &FileTile| f.path == path && ctx.gfx.is_live(*t) && !taken.contains(t);
        let files = &ctx.gfx.assets3d.terrain_files.tiles;
        let kept = files
            .iter()
            .find(|(t, f)| free(t, f) && f.corner == corner)
            .or_else(|| files.iter().find(|(t, f)| free(t, f)))
            .map(|(t, _)| *t);
        let tile = match kept {
            Some(tile) => {
                re_place(ctx, tile, corner);
                tile
            }
            None => {
                let (data, layers) = match prepared.files.get(&path) {
                    Some((data, layers)) => (data.clone(), *layers),
                    None => {
                        let read = read_file(ctx, &path)
                            .map_err(|err| (e, err.context(format_args!("terrain file \"{path}\""))))?;
                        prepared.files.insert(path.clone(), read.clone());
                        read
                    }
                };
                let built = build(ctx, &data, Vec2::new(corner.x, corner.z), layers).map_err(|err| (e, err))?;
                note_tile(ctx, &path, corner, &built, &data);
                built.tile
            }
        };
        ctx.world.insert(e, tile);
        taken.insert(tile);
        #[cfg(feature = "physics3d")]
        give_collider(ctx, e, tile);
    }
    Ok(())
}

/// Fills `e`'s [`TerrainCollider`](crate::d3::TerrainCollider) with the
/// heightfield of `tile`'s file if it has none that is live.
#[cfg(feature = "physics3d")]
fn give_collider(ctx: &mut Ctx, e: Entity, tile: TerrainTile) {
    let Some(collider) = ctx.world.get::<crate::d3::TerrainCollider>(e).copied() else {
        return;
    };
    if ctx.physics3d.is_live(collider.mesh) {
        return;
    }
    let Some(file) = ctx.gfx.assets3d.terrain_files.tiles.get(&tile) else {
        return;
    };
    let (heights, size) = (file.heights.clone(), file.size);
    let mesh = ctx.physics3d.heightfield(&heights, size);
    if let Some(c) = ctx.world.get_mut::<crate::d3::TerrainCollider>(e) {
        c.mesh = mesh;
    }
}

/// Where `tile`'s painted layers lie now (its corner), if it was made
/// from a terrain file.
#[cfg(all(test, feature = "editor"))]
pub(crate) fn placed_corner(ctx: &Ctx, tile: TerrainTile) -> Option<Vec3> {
    ctx.gfx.assets3d.terrain_files.tiles.get(&tile).map(|f| f.corner)
}

/// Moves `tile`'s painted layers to `corner`, where its entity is now: its
/// terrain material's weight map follows. The meshes and a collider are
/// placed by the entity's transform already.
fn re_place(ctx: &mut Ctx, tile: TerrainTile, corner: Vec3) {
    let Some(file) = ctx.gfx.assets3d.terrain_files.tiles.get_mut(&tile) else {
        return;
    };
    if file.corner == corner {
        return;
    }
    file.corner = corner;
    let Some((material, mut desc)) = file.ground else {
        return;
    };
    desc.weights_corner = Vec2::new(corner.x, corner.z);
    if let Some((_, d)) = file.ground.as_mut() {
        *d = desc;
    }
    // The description was valid when the material was made, and only its
    // corner changed; a freed material stays freed.
    if ctx.gfx.is_live(material)
        && let Ok(updated) = ctx.gfx.terrain_material_desc(&desc)
    {
        ctx.gfx.update_material(material, updated);
    }
}

/// The step that keeps terrain-file tiles with their entities, run at the
/// start of every frame and before it is drawn: an entity with a
/// [`TerrainFile`] and no tile yet (spawned from JSON, by a mod, by a
/// system) gets one, and a tile whose entity moved has its painted layers
/// re-placed (not while a drag in the editor is moving it: once, when it
/// ends). An entity whose file doesn't load is despawned, with an error
/// naming the file, rather than left without its ground.
pub(crate) fn follow_files(ctx: &mut Ctx) {
    let files = &ctx.gfx.assets3d.terrain_files;
    let held = files.held.is_some_and(|f| f.saturating_add(1) >= ctx.gfx.frame);
    let mut wants = false;
    let mut moved = false;
    ctx.world
        .query::<(&crate::d3::Transform, &TerrainFile, Option<&TerrainTile>)>()
        .for_each(|(t, _, tile)| match tile {
            None => wants = true,
            Some(tile) => {
                moved |= !held && files.tiles.get(tile).is_some_and(|f| f.corner != t.position);
            }
        });
    if moved {
        let moved: Vec<(TerrainTile, Vec3)> = ctx
            .world
            .query::<(&crate::d3::Transform, &TerrainTile)>()
            .filter::<ponko_ecs::With<TerrainFile>>()
            .iter()
            .map(|(t, tile)| (*tile, t.position))
            .collect();
        for (tile, corner) in moved {
            re_place(ctx, tile, corner);
        }
    }
    if wants {
        for one in wanting(ctx) {
            if let Err((e, err)) = give(ctx, vec![one], Prepared::default()) {
                eprintln!("{err} (its entity {} is despawned)", e.to_bits());
                ctx.world.despawn(e);
            }
        }
    }
}

impl TerrainJson {
    /// Says what's wrong, if anything.
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.format != TERRAIN_FORMAT {
            return Err(format!(
                "\"format\" must be \"{TERRAIN_FORMAT}\" (got \"{}\")",
                self.format
            ));
        }
        if self.version != 1 {
            return Err(format!(
                "terrain file version {} is newer than this engine reads (1)",
                self.version
            ));
        }
        if !(self.size > 0.0 && self.size.is_finite()) {
            return Err(format!("\"size\" must be above 0 (got {})", self.size));
        }
        if !(self.low.is_finite() && self.high.is_finite() && self.high > self.low) {
            return Err(format!(
                "\"high\" must be above \"low\" (got {} and {})",
                self.high, self.low
            ));
        }
        if self.layers.len() > super::terrain::TERRAIN_LAYERS {
            return Err(format!(
                "at most {} layers (got {})",
                super::terrain::TERRAIN_LAYERS,
                self.layers.len()
            ));
        }
        if let Some(bad) = self.layers.iter().find(|l| !(l.size > 0.0 && l.size.is_finite())) {
            return Err(format!(
                "layer \"{}\": \"size\" must be above 0 (got {})",
                bad.texture, bad.size
            ));
        }
        let needed = self.layers.len().div_ceil(4);
        if self.weights.len() != needed {
            return Err(format!(
                "{} layers need {needed} weight image(s), 4 layers each (got {})",
                self.layers.len(),
                self.weights.len()
            ));
        }
        if !(0.0..=1.0).contains(&self.roughness) {
            return Err(format!("\"roughness\" goes from 0 to 1 (got {})", self.roughness));
        }
        Ok(())
    }
}

impl TerrainData {
    /// The heights in world units, as `create_terrain_tile` takes them.
    pub(crate) fn world_heights(&self) -> Vec<f32> {
        let (low, span) = (self.json.low, self.json.high - self.json.low);
        self.heights
            .iter()
            .map(|&h| low + f32::from(h) / 65535.0 * span)
            .collect()
    }

    /// Reads the terrain file `path` and the images it names, through
    /// `read` (asset name to bytes).
    pub(crate) fn read(
        path: &str,
        text: &str,
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, Error>,
    ) -> Result<Self, Error> {
        let json: TerrainJson = serde_json::from_str(text).map_err(|e| Error::decode(path, e.to_string()))?;
        json.check().map_err(|e| Error::decode(path, e))?;
        let beside = |name: &str| crate::tilemap::resolve_relative_asset(path, name);
        let heights_name = beside(&json.heights);
        let (w, h, heights) = decode_grey16(&read(&heights_name)?).map_err(|e| Error::decode(&heights_name, e))?;
        if w != h || w < 2 {
            return Err(Error::decode(
                &heights_name,
                format!("terrain heights must be a square of at least 2 x 2 pixels (got {w} x {h})"),
            ));
        }
        let mut side = 0;
        let mut weights = Vec::new();
        for name in &json.weights {
            let name = beside(name);
            let bytes = read(&name)?;
            let image = crate::assets::decode_image(&bytes, &name, &name)?;
            if image.width != image.height || (side != 0 && image.width != side) {
                return Err(Error::decode(
                    &name,
                    format!(
                        "terrain weights must be square and all the same size (got {} x {})",
                        image.width, image.height
                    ),
                ));
            }
            side = image.width;
            weights.push(image.rgba);
        }
        Ok(Self {
            json,
            n: w as usize,
            heights,
            side,
            weights,
        })
    }
}

/// A greyscale PNG's pixels as 16-bit values (8-bit ones scaled up).
pub(crate) fn decode_grey16(bytes: &[u8]) -> Result<(u32, u32, Vec<u16>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let size = reader.output_buffer_size().ok_or("the PNG is too large")?;
    let mut buf = vec![0; size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("the PNG doesn't decode: {e}"))?;
    let (color, depth) = (frame.color_type, frame.bit_depth);
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return Err(format!("terrain heights must be a greyscale PNG (got {color:?})")),
    };
    let pixels = (frame.width * frame.height) as usize;
    let values = match depth {
        png::BitDepth::Sixteen => (0..pixels)
            .map(|i| u16::from_be_bytes([buf[i * 2 * channels], buf[i * 2 * channels + 1]]))
            .collect(),
        png::BitDepth::Eight => (0..pixels).map(|i| u16::from(buf[i * channels]) * 257).collect(),
        other => return Err(format!("terrain heights must be 8 or 16 bits a pixel (got {other:?})")),
    };
    Ok((frame.width, frame.height, values))
}

/// A 16-bit greyscale PNG of `n` × `n` heights.
#[cfg_attr(not(any(feature = "editor", test)), allow(dead_code))]
pub(crate) fn encode_grey16(n: u32, heights: &[u16]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, n, n);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let bytes: Vec<u8> = heights.iter().flat_map(|h| h.to_be_bytes()).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&bytes))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// An RGBA PNG, `side` × `side`.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(crate) fn encode_rgba(side: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// What [`build`] made, to free when it's made again.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(crate) struct Built {
    pub tile: TerrainTile,
    /// The terrain material and its weight array; `None` without layers
    /// (the default material).
    pub material: Option<(Material, TextureArray)>,
    /// What the terrain material was made from, to re-place it.
    pub ground: Option<TerrainMaterialDesc>,
}

/// Makes `data`'s tile with its corner at `corner` (x, z in the world, for
/// the weights), drawn with `layers` (the layer textures, as an array) if
/// it has layers.
pub(crate) fn build(
    ctx: &mut Ctx,
    data: &TerrainData,
    corner: Vec2,
    layers: Option<TextureArray>,
) -> Result<Built, Error> {
    let material = match layers {
        Some(layers) if !data.json.layers.is_empty() => {
            let images: Vec<&[u8]> = data.weights.iter().map(Vec::as_slice).collect();
            let weights = ctx
                .gfx
                .try_create_texture_array(data.side, data.side, &images, TextureKind::Data)?;
            let mut layer_size = [4.0; super::terrain::TERRAIN_LAYERS];
            for (to, layer) in layer_size.iter_mut().zip(&data.json.layers) {
                *to = layer.size;
            }
            let desc = TerrainMaterialDesc::new(layers, weights, corner, Vec2::splat(data.json.size))
                .with_layer_size(layer_size)
                .with_roughness(data.json.roughness);
            Some((ctx.gfx.try_create_terrain_material(&desc)?, weights, desc))
        }
        _ => None,
    };
    let ground = match material {
        Some((m, _, _)) => m,
        None => ctx.gfx.default_material(),
    };
    let tile = ctx
        .gfx
        .create_terrain_tile(&data.world_heights(), data.json.size, ground);
    Ok(Built {
        tile,
        material: material.map(|(m, w, _)| (m, w)),
        ground: material.map(|(_, _, d)| d),
    })
}

/// The layer textures of `data` as one array.
pub(crate) fn load_layers(ctx: &mut Ctx, path: &str, data: &TerrainData) -> Result<Option<TextureArray>, Error> {
    if data.json.layers.is_empty() {
        return Ok(None);
    }
    let names: Vec<String> = data
        .json
        .layers
        .iter()
        .map(|l| crate::tilemap::resolve_relative_asset(path, &l.texture))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Ok(Some(ctx.gfx.try_load_texture_array(&names, TextureKind::Color)?))
}

impl TerrainTile {
    /// Spawns the terrain tile of terrain file `path` (`"format":
    /// "ponko-terrain"`, as the editor's terrain window saves it: heights
    /// in a greyscale PNG, ground layers and their weight maps) with its
    /// corner at `at`: an entity with a [`d3::Transform`](crate::d3::Transform)
    /// and a [`TerrainTile`], with the [`TerrainFile`] that saves it in a
    /// scene. See the book's editor chapter for the format.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # use ponko::terrain::TerrainTile;
    /// # fn f(ctx: &mut Ctx) -> Result<(), ponko::error::Error> {
    /// let hill = TerrainTile::spawn_from_file(ctx, "terrain/hill.json", vec3(-32.0, 0.0, -32.0))?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// [`NotFound`](crate::error::ErrorKind::NotFound) for a file that isn't
    /// there (the terrain file or an image it names), and
    /// [`Decode`](crate::error::ErrorKind::Decode) for one that doesn't read
    /// (the message says what's wrong). Nothing is spawned then.
    pub fn spawn_from_file(ctx: &mut Ctx, path: &str, at: Vec3) -> Result<Entity, Error> {
        let data = read_data(ctx, path)?;
        let layers = load_layers(ctx, path, &data)?;
        let built = build(ctx, &data, Vec2::new(at.x, at.z), layers)?;
        note_tile(ctx, path, at, &built, &data);
        Ok(ctx
            .world
            .spawn((crate::d3::Transform::at(at), built.tile, TerrainFile::new(path))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_survive_a_round_trip_through_a_16_bit_png() {
        let heights: Vec<u16> = (0..9).map(|i| i * 8000).collect();
        let png = encode_grey16(3, &heights).unwrap();
        assert_eq!(decode_grey16(&png).unwrap(), (3, 3, heights));
    }

    #[test]
    fn a_terrain_file_says_what_is_wrong_with_it() {
        let json =
            |extra: &str| format!(r#"{{"format": "ponko-terrain", "size": 8, "heights": "h.png", "high": 1{extra}}}"#);
        let png = encode_grey16(2, &[0, 0, 0, 65535]).unwrap();
        let mut read = |_: &str| Ok(png.clone());
        let ok = TerrainData::read("t.json", &json(""), &mut read).unwrap();
        assert_eq!(ok.world_heights(), vec![0.0, 0.0, 0.0, 1.0]);
        let bad = TerrainData::read("t.json", &json(r#", "layers": [{"texture": "g.png"}]"#), &mut read).unwrap_err();
        assert!(bad.message().contains("need 1 weight image"), "{bad}");
        let bad = TerrainData::read("t.json", &json(r#", "low": 2"#), &mut read).unwrap_err();
        assert!(bad.message().contains("above \"low\""), "{bad}");
    }
}
