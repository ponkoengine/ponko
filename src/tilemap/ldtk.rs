//! LDtk projects (`.ldtk`), with their external level files (`.ldtkl`).

use std::collections::BTreeMap;

use glam::{UVec2, Vec2};
use serde_json::Value;

use super::TileMap;
use super::level::{
    Layer, LayerKind, Level, LevelObject, ObjectShape, PlacedTile, Properties, Reader, TileInfo, TileLayer, Tileset,
    decode_error, flag, num, num_or, parse_color, resolve, text,
};
use crate::color::Color;
use crate::error::{Error, ErrorKind};

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// LDtk fields (`fieldInstances`) as properties: identifier to value.
fn fields(v: &Value) -> Properties {
    Properties(
        list(v, "fieldInstances")
            .iter()
            .map(|f| {
                (
                    text(f, "__identifier").to_string(),
                    f.get("__value").cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    )
}

/// The project's tilesets that have an image, and their uids.
fn tilesets(defs: &Value, path: &str) -> (Vec<Tileset>, Vec<i64>) {
    let mut sets = Vec::new();
    let mut uids = Vec::new();
    for t in list(defs, "tilesets") {
        let Some(rel) = t.get("relPath").and_then(Value::as_str) else {
            continue; // LDtk's built-in icons have no image file
        };
        let grid = num_or(t, "tileGridSize", 0.0) as u32;
        let columns = num_or(t, "__cWid", 0.0) as u32;
        let rows = num_or(t, "__cHei", 0.0) as u32;
        let mut tiles: BTreeMap<u32, TileInfo> = BTreeMap::new();
        for d in list(t, "customData") {
            let id = num_or(d, "tileId", 0.0) as u32;
            let info = tiles.entry(id).or_default();
            info.properties
                .0
                .insert("data".into(), Value::String(text(d, "data").to_string()));
        }
        for tag in list(t, "enumTags") {
            let name = text(tag, "enumValueId");
            for id in list(tag, "tileIds") {
                let id = id.as_u64().unwrap_or(0) as u32;
                tiles.entry(id).or_default().tags.push(name.to_string());
            }
        }
        sets.push(Tileset {
            name: text(t, "identifier").to_string(),
            image: resolve(path, rel),
            tile_size: UVec2::splat(grid),
            columns,
            count: columns * rows,
            margin: num_or(t, "padding", 0.0) as u32,
            spacing: num_or(t, "spacing", 0.0) as u32,
            tiles,
        });
        uids.push(num_or(t, "uid", -1.0) as i64);
    }
    (sets, uids)
}

/// An LDtk world's layout as saved by the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LdtkLayout {
    /// Levels use their explicit pixel positions.
    Free,
    /// Levels use explicit positions aligned to a world grid.
    GridVania,
    /// Levels are laid out left to right in project order.
    LinearHorizontal,
    /// Levels are laid out top to bottom in project order.
    LinearVertical,
}

/// A level's placement in an LDtk world.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LdtkLevel {
    /// Name shown in the editor.
    pub identifier: String,
    /// Stable instance id, accepted by [`super::LevelOptions::level`].
    pub iid: String,
    /// Top-left position in world pixels.
    pub position: Vec2,
    /// Width and height in pixels.
    pub size: UVec2,
    /// LDtk world depth, for games that organize levels in floors.
    pub depth: i32,
}

/// An LDtk project's world and the placements of its levels.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LdtkWorld {
    /// Name shown in the editor. Empty for legacy single-world projects.
    pub identifier: String,
    /// Stable instance id, if saved by the editor.
    pub iid: String,
    /// How levels are placed in this world.
    pub layout: LdtkLayout,
    /// World grid cell dimensions in pixels, when provided.
    pub grid_size: UVec2,
    /// Level placements in editor order.
    pub levels: Vec<LdtkLevel>,
}

fn project(input: &str) -> Result<Value, Error> {
    let project: Value = serde_json::from_str(input)?;
    if project.get("jsonVersion").is_none() {
        return Err(decode_error("expected an LDtk project with jsonVersion"));
    }
    Ok(project)
}

fn pixels(v: &Value, w: &str, h: &str) -> UVec2 {
    UVec2::new(num_or(v, w, 0.0) as u32, num_or(v, h, 0.0) as u32)
}

fn world(world: &Value) -> LdtkWorld {
    let layout = match text(world, "worldLayout") {
        "GridVania" => LdtkLayout::GridVania,
        "LinearHorizontal" => LdtkLayout::LinearHorizontal,
        "LinearVertical" => LdtkLayout::LinearVertical,
        _ => LdtkLayout::Free,
    };
    let mut next = Vec2::ZERO;
    let levels = list(world, "levels")
        .iter()
        .map(|level| {
            let size = pixels(level, "pxWid", "pxHei");
            let position = match layout {
                LdtkLayout::LinearHorizontal => {
                    let at = next;
                    next.x += size.x as f32;
                    at
                }
                LdtkLayout::LinearVertical => {
                    let at = next;
                    next.y += size.y as f32;
                    at
                }
                _ => Vec2::new(num_or(level, "worldX", 0.0) as f32, num_or(level, "worldY", 0.0) as f32),
            };
            LdtkLevel {
                identifier: text(level, "identifier").to_owned(),
                iid: text(level, "iid").to_owned(),
                position,
                size,
                depth: num_or(level, "worldDepth", 0.0) as i32,
            }
        })
        .collect();
    let iid = world.get("iid").or_else(|| world.get("dummyWorldIid"));
    LdtkWorld {
        identifier: text(world, "identifier").to_owned(),
        iid: iid.and_then(Value::as_str).unwrap_or_default().to_owned(),
        layout,
        grid_size: pixels(world, "worldGridWidth", "worldGridHeight"),
        levels,
    }
}

fn worlds(project: &Value) -> impl Iterator<Item = &Value> {
    let nested = list(project, "worlds");
    std::iter::once(project)
        .filter(move |p| !list(p, "levels").is_empty() || nested.is_empty())
        .chain(nested)
}

pub(super) fn read_worlds(input: &str) -> Result<Vec<LdtkWorld>, Error> {
    Ok(worlds(&project(input)?).map(world).collect())
}

pub(super) fn read(text_: &str, path: &str, wanted: Option<&str>, reader: &mut Reader) -> Result<Level, Error> {
    let project = project(text_)?;
    let defs = project.get("defs").unwrap_or(&Value::Null);
    let (tilesets, uids) = tilesets(defs, path);
    let set_of = |uid: f64| uids.iter().position(|&u| u == uid as i64).map(|i| i as u16);
    let all: Vec<_> = worlds(&project)
        .flat_map(|w| list(w, "levels").iter().zip(world(w).levels))
        .collect();
    let (level, placement) = match wanted {
        Some(name) => all
            .iter()
            .find(|(_, l)| l.iid == name)
            .or_else(|| all.iter().find(|(_, l)| l.identifier == name))
            .ok_or_else(|| {
                let names: Vec<_> = all.iter().map(|(_, l)| l.identifier.as_str()).collect();
                Error::new(
                    ErrorKind::NotFound,
                    format!("{path} has no level {name:?}; it has {}", names.join(", ")),
                )
            })?,
        None => all
            .first()
            .ok_or_else(|| decode_error(format!("{path} has no levels")))?,
    };
    // Levels saved in their own files.
    let external;
    let level = match level.get("externalRelPath").and_then(Value::as_str) {
        Some(rel) => {
            let name = resolve(path, rel);
            let text = reader.text(&name)?;
            external = serde_json::from_str::<Value>(&text).map_err(|e| Error::from(e).context(&name))?;
            &external
        }
        None => *level,
    };
    let layer_defs = list(defs, "layers");
    let def_of = |uid: f64| layer_defs.iter().find(|d| num(d, "uid") == Some(uid));
    let mut layers = Vec::new();
    // LDtk lists the top layer first.
    for l in list(level, "layerInstances").iter().rev() {
        let grid = num_or(l, "__gridSize", 1.0) as f32;
        let size = UVec2::new(num_or(l, "__cWid", 0.0) as u32, num_or(l, "__cHei", 0.0) as u32);
        let def = num(l, "layerDefUid").and_then(def_of);
        let factor = |key: &str| def.map_or(0.0, |d| num_or(d, key, 0.0)) as f32;
        let name = text(l, "__identifier").to_string();
        let kind = match text(l, "__type") {
            "Entities" => {
                let objects = list(l, "entityInstances")
                    .iter()
                    .enumerate()
                    .map(|(i, e)| entity(e, i, &name, &tilesets, &set_of))
                    .collect();
                LayerKind::Objects(objects)
            }
            _ => {
                let set = num(l, "overrideTilesetUid")
                    .or_else(|| num(l, "__tilesetDefUid"))
                    .and_then(set_of);
                let mut tiles = Vec::new();
                for t in list(l, "gridTiles").iter().chain(list(l, "autoLayerTiles")) {
                    let (Some(set), Some(px)) = (set, t.get("px").and_then(Value::as_array)) else {
                        continue;
                    };
                    let at = |i: usize| px.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    let cell = (Vec2::new(at(0), at(1)) / grid).floor();
                    if cell.x < 0.0 || cell.y < 0.0 {
                        continue;
                    }
                    let f = num_or(t, "f", 0.0) as u8;
                    let mut flip = 0;
                    if f & 1 != 0 {
                        flip |= TileMap::FLIP_X;
                    }
                    if f & 2 != 0 {
                        flip |= TileMap::FLIP_Y;
                    }
                    tiles.push(PlacedTile {
                        cell: cell.as_uvec2(),
                        tileset: set,
                        id: num_or(t, "t", 0.0) as u32,
                        flip,
                    });
                }
                let int_grid: Vec<u32> = list(l, "intGridCsv")
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u32)
                    .collect();
                let int_grid_names = def
                    .map(|d| {
                        list(d, "intGridValues")
                            .iter()
                            .filter_map(|v| {
                                let name = v.get("identifier")?.as_str()?;
                                Some((num(v, "value")? as u32, name.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                LayerKind::Tiles(TileLayer {
                    size,
                    cell_size: Vec2::splat(grid),
                    tiles,
                    int_grid,
                    int_grid_names,
                })
            }
        };
        layers.push(Layer {
            name,
            class: String::new(),
            offset: Vec2::new(
                num_or(l, "__pxTotalOffsetX", 0.0) as f32,
                num_or(l, "__pxTotalOffsetY", 0.0) as f32,
            ),
            parallax: Vec2::ONE - Vec2::new(factor("parallaxFactorX"), factor("parallaxFactorY")),
            opacity: num_or(l, "__opacity", 1.0) as f32,
            tint: Color::WHITE,
            visible: flag(l, "visible", true),
            properties: Properties::default(),
            kind,
        });
    }
    let px = Vec2::new(num_or(level, "pxWid", 0.0) as f32, num_or(level, "pxHei", 0.0) as f32);
    let cell = num_or(&project, "defaultGridSize", 16.0).max(1.0) as f32;
    Ok(Level {
        name: placement.identifier.clone(),
        ldtk: Some(placement.clone()),
        size: (px / cell).ceil().as_uvec2(),
        cell_size: Vec2::splat(cell),
        background: level
            .get("__bgColor")
            .or_else(|| level.get("bgColor"))
            .and_then(Value::as_str)
            .and_then(parse_color),
        parallax_origin: px * 0.5,
        properties: fields(level),
        tilesets,
        layers,
        files: Vec::new(),
    })
}

/// An entity instance as an object: its center, size, fields and tile.
fn entity(
    e: &Value,
    index: usize,
    layer: &str,
    tilesets: &[Tileset],
    set_of: &impl Fn(f64) -> Option<u16>,
) -> LevelObject {
    let pair = |key: &str, default: f64| {
        let v = e.get(key).and_then(Value::as_array);
        let at = |i: usize| v.and_then(|v| v.get(i)).and_then(Value::as_f64).unwrap_or(default) as f32;
        Vec2::new(at(0), at(1))
    };
    let size = Vec2::new(num_or(e, "width", 0.0) as f32, num_or(e, "height", 0.0) as f32);
    let pivot = pair("__pivot", 0.5);
    let position = pair("px", 0.0) - pivot * size + size * 0.5;
    let tile = e.get("__tile").and_then(|t| {
        let set = set_of(num(t, "tilesetUid")?)?;
        let ts = &tilesets[usize::from(set)];
        let grid = ts.tile_size.x.max(1);
        let step = grid + ts.spacing;
        let x = (num_or(t, "x", 0.0) as u32).saturating_sub(ts.margin) / step;
        let y = (num_or(t, "y", 0.0) as u32).saturating_sub(ts.margin) / step;
        Some((set, y * ts.columns + x, 0))
    });
    let mut properties = fields(e);
    properties
        .0
        .insert("iid".into(), Value::String(text(e, "iid").to_string()));
    properties.0.insert(
        "tags".into(),
        e.get("__tags").cloned().unwrap_or(Value::Array(Vec::new())),
    );
    LevelObject {
        id: index as u64,
        name: String::new(),
        class: text(e, "__identifier").to_string(),
        layer: layer.to_string(),
        position,
        size,
        rotation: 0.0,
        shape: ObjectShape::Rect,
        tile,
        visible: true,
        properties,
    }
}
