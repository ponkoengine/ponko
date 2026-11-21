//! The terrain window: a terrain file (`"format": "ponko-terrain"`, read by
//! `Ctx::spawn_terrain`) shaped and painted in the 3D view. Brushes raise,
//! lower and smooth the ground, and paint a ground layer, under a circle
//! that follows the mouse over the tile; each stroke is one step in the
//! editor's history. Save writes the file, its heights (a 16-bit greyscale
//! PNG) and its weight maps (RGBA PNGs) beside it. A name that isn't there
//! yet makes a new flat tile.
//!
//! The tile shown is the one `spawn_terrain` made from the file, or, with
//! **Show in the view**, one the editor puts in the middle of the world
//! (not saved with the scene). Heights stay between the file's `low` and
//! `high`.

use egui::{Color32, RichText, Ui};
use glam::{Vec2, Vec3};
use ponko_ecs::Entity;
use serde_json::{Value, json};

use super::Panels;
use super::author::{beside, near_first, read_text, relative};
use crate::ctx::Ctx;
use crate::terrain::TextureArray;
use crate::three::{TERRAIN_FORMAT, TerrainBuilt, TerrainData, TerrainJson, TerrainLayerJson, TerrainSource};

/// What the brush does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Brush {
    #[default]
    Raise,
    Lower,
    Smooth,
    Paint,
}

/// The terrain window's state.
#[derive(Default)]
pub(crate) struct TerrainState {
    pub path: Option<String>,
    pub brush: Brush,
    /// The brush's radius, in world units (0: an eighth of the tile).
    pub radius: f32,
    /// How fast it works: height units a second for raise and lower, a
    /// share a second for smooth and paint (0: the default).
    pub strength: f32,
    /// The layer painted.
    pub layer: usize,
    applied: u64,
    /// What the editor last made for the tile, to free when it makes it again.
    built: Option<TerrainBuilt>,
    /// The layer textures, for the layer list they were loaded for.
    layers: Option<(Vec<String>, Option<TextureArray>)>,
    pub error: Option<String>,
}

/// Whether the file at `path` (in `assets/`) is a terrain file.
pub(crate) fn is_terrain_file(path: &str) -> bool {
    crate::assets::resolve(path)
        .ok()
        .and_then(|p| crate::assets::read_to_string(&p).ok())
        .is_some_and(|t| t.contains(&format!("\"{TERRAIN_FORMAT}\"")))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 15)] as char);
    }
    out
}

fn unhex(text: &str) -> Vec<u8> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => 0,
    };
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[hi, lo]| digit(hi) << 4 | digit(lo))
        .collect()
}

/// A terrain file as a document: its JSON, with the heights and weights
/// alongside (`_n`, `_heights`, `_side`, `_weights`, as hex) so a brush
/// stroke is an edit of the document like any other.
fn to_doc(data: &TerrainData) -> Value {
    let mut doc = serde_json::to_value(&data.json).unwrap_or_default();
    let bytes: Vec<u8> = data.heights.iter().flat_map(|h| h.to_be_bytes()).collect();
    doc["_n"] = json!(data.n);
    doc["_heights"] = json!(hex(&bytes));
    doc["_side"] = json!(data.side);
    doc["_weights"] = json!(data.weights.iter().map(|w| hex(w)).collect::<Vec<_>>());
    doc
}

fn from_doc(doc: &Value) -> Result<TerrainData, String> {
    let mut file = doc.clone();
    if let Some(o) = file.as_object_mut() {
        o.retain(|k, _| !k.starts_with('_'));
    }
    let json: TerrainJson = serde_json::from_value(file).map_err(|e| e.to_string())?;
    json.check()?;
    let n = doc["_n"].as_u64().unwrap_or(0) as usize;
    let heights: Vec<u16> = unhex(doc["_heights"].as_str().unwrap_or(""))
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&p| u16::from_be_bytes(p))
        .collect();
    if n < 2 || heights.len() != n * n {
        return Err("the heights aren't a square grid".into());
    }
    let side = doc["_side"].as_u64().unwrap_or(0) as u32;
    let weights: Vec<Vec<u8>> = doc["_weights"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| unhex(w.as_str().unwrap_or("")))
        .collect();
    if weights.iter().any(|w| w.len() != (side * side * 4) as usize) || weights.len() != json.weights.len() {
        return Err("the weight maps don't match the layers".into());
    }
    Ok(TerrainData {
        json,
        n,
        heights,
        side,
        weights,
    })
}

/// Writes a terrain document: the file, its heights and its weight maps.
fn write_terrain(path: &str, doc: &Value) -> Result<(), crate::error::Error> {
    let invalid = |e: String| crate::error::Error::new(crate::error::ErrorKind::Invalid, e).with_path(path);
    let data = from_doc(doc).map_err(invalid)?;
    let png = crate::three::encode_grey16(data.n as u32, &data.heights).map_err(invalid)?;
    crate::assets::write_asset(&beside(path, &data.json.heights), &png)?;
    for (name, rgba) in data.json.weights.iter().zip(&data.weights) {
        let png = crate::three::encode_rgba(data.side, rgba).map_err(invalid)?;
        crate::assets::write_asset(&beside(path, name), &png)?;
    }
    let mut text = serde_json::to_string_pretty(&data.json).map_err(|e| invalid(e.to_string()))?;
    text.push('\n');
    crate::assets::write_asset(path, text.as_bytes())
}

/// A new flat tile, 64 units wide, 65 × 65 heights, from -8 to 24 high.
fn new_terrain(path: &str) -> TerrainData {
    let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".json");
    let (low, high) = (-8.0f32, 24.0f32);
    let level = ((0.0 - low) / (high - low) * 65535.0).round() as u16;
    TerrainData {
        json: TerrainJson {
            format: TERRAIN_FORMAT.into(),
            version: 1,
            size: 64.0,
            heights: format!("{stem}-heights.png"),
            low,
            high,
            layers: Vec::new(),
            weights: Vec::new(),
            roughness: 0.9,
        },
        n: 65,
        heights: vec![level; 65 * 65],
        side: 0,
        weights: Vec::new(),
    }
}

/// The terrain entity of `path` in the world, and its corner.
fn tile_of(ctx: &mut Ctx, path: &str) -> Option<(Entity, Vec3)> {
    ctx.world
        .query::<(Entity, &crate::d3::Transform, &TerrainSource)>()
        .iter()
        .find(|(_, _, s)| s.path == path)
        .map(|(e, t, _)| (e, t.position))
}

impl TerrainData {
    fn to_stored(&self, h: f32) -> u16 {
        let (low, high) = (self.json.low, self.json.high);
        ((h.clamp(low, high) - low) / (high - low) * 65535.0).round() as u16
    }

    fn cell(&self) -> f32 {
        self.json.size / (self.n - 1) as f32
    }

    /// Where `ray` first meets the ground, from the tile's corner (x, z).
    fn hit(&self, corner: Vec3, from: Vec3, dir: Vec3) -> Option<Vec2> {
        let field = crate::terrain::Heightfield::new(&self.world_heights(), self.json.size);
        let below = |t: f32| {
            let p = from + dir * t - corner;
            field.height_at(p.x, p.z).map(|h| p.y <= h)
        };
        let step = self.cell() * 0.5;
        let far = (from - corner).length() + self.json.size * 3.0 + (self.json.high - self.json.low).abs();
        let mut last = 0.0;
        let mut t = 0.0;
        while t < far {
            if below(t) == Some(true) {
                // Narrow it down between the last point above and this one.
                let (mut lo, mut hi) = (last, t);
                for _ in 0..16 {
                    let mid = (lo + hi) / 2.0;
                    if below(mid) == Some(true) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let p = from + dir * hi - corner;
                return Some(Vec2::new(p.x, p.z));
            }
            last = t;
            t += step;
        }
        None
    }

    /// One frame of `brush` at `at` (from the tile's corner).
    fn stroke(&mut self, brush: Brush, layer: usize, at: Vec2, radius: f32, strength: f32, dt: f32) {
        let falloff = |p: Vec2| {
            let d = (p - at).length() / radius;
            if d >= 1.0 { 0.0 } else { 1.0 - d * d * (3.0 - 2.0 * d) }
        };
        if brush == Brush::Paint {
            let side = self.side as usize;
            if side == 0 || layer >= self.json.layers.len() {
                return;
            }
            let texel = self.json.size / side as f32;
            for y in 0..side {
                for x in 0..side {
                    let f = falloff(Vec2::new((x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel));
                    if f <= 0.0 {
                        continue;
                    }
                    let k = (strength * dt * f).min(1.0);
                    for (image, w) in self.weights.iter_mut().enumerate() {
                        for c in 0..4 {
                            let at = (y * side + x) * 4 + c;
                            let goal = if image * 4 + c == layer { 255.0 } else { 0.0 };
                            let now = f32::from(w[at]);
                            w[at] = (now + (goal - now) * k).round() as u8;
                        }
                    }
                }
            }
            return;
        }
        let n = self.n;
        let cell = self.cell();
        let heights = self.world_heights();
        let mut changed = heights.clone();
        for z in 0..n {
            for x in 0..n {
                let f = falloff(Vec2::new(x as f32 * cell, z as f32 * cell));
                if f <= 0.0 {
                    continue;
                }
                let i = z * n + x;
                changed[i] = match brush {
                    Brush::Raise => heights[i] + strength * dt * f,
                    Brush::Lower => heights[i] - strength * dt * f,
                    Brush::Smooth => {
                        let mut sum = 0.0;
                        let mut count = 0.0;
                        for (dx, dz) in [
                            (-1, -1),
                            (0, -1),
                            (1, -1),
                            (-1, 0),
                            (0, 0),
                            (1, 0),
                            (-1, 1),
                            (0, 1),
                            (1, 1),
                        ] {
                            let (nx, nz) = (x as i64 + dx, z as i64 + dz);
                            if (0..n as i64).contains(&nx) && (0..n as i64).contains(&nz) {
                                sum += heights[nz as usize * n + nx as usize];
                                count += 1.0;
                            }
                        }
                        let k = (strength * dt * f).min(1.0);
                        heights[i] + (sum / count - heights[i]) * k
                    }
                    Brush::Paint => heights[i],
                };
            }
        }
        self.heights = changed.iter().map(|&h| self.to_stored(h)).collect();
    }
}

impl Panels {
    fn terrain(&mut self) -> Option<&mut TerrainState> {
        self.app.as_mut().map(|a| &mut a.author.terrain)
    }

    /// Opens the terrain file at `path` in the terrain window, or starts a
    /// new flat tile there if there's no file.
    pub(super) fn open_terrain(&mut self, ctx: &mut Ctx, path: &str) -> Result<(), String> {
        if !path.ends_with(".json") {
            return Err(format!(
                "{path}: a terrain file is .json (its images are PNGs beside it)"
            ));
        }
        match read_text(ctx, path) {
            Ok(text) => {
                let assets = &ctx.assets;
                let data = TerrainData::read(path, &text, &mut |name| {
                    assets
                        .asset_sources
                        .find(name)?
                        .into_bytes()
                        .map_err(|e| e.with_path(name))
                })
                .map_err(|e| e.to_string())?;
                ctx.editor.docs.open(path, to_doc(&data), write_terrain);
            }
            Err(_) if crate::assets::resolve(path).is_err() => {
                ctx.editor
                    .docs
                    .open_new(path, to_doc(&new_terrain(path)), write_terrain);
            }
            Err(e) => return Err(e),
        }
        let state = self.terrain().ok_or("not the editor")?;
        *state = TerrainState {
            path: Some(path.to_string()),
            applied: u64::MAX,
            ..TerrainState::default()
        };
        Ok(())
    }

    /// Makes the tile in the world again from the document.
    fn rebuild_terrain(&mut self, ctx: &mut Ctx, path: &str, data: &TerrainData) -> Result<(), String> {
        let Some((entity, corner)) = tile_of(ctx, path) else {
            return Ok(());
        };
        let Some(state) = self.terrain() else { return Ok(()) };
        let names: Vec<String> = data.json.layers.iter().map(|l| l.texture.clone()).collect();
        let cached = state.layers.as_ref().filter(|(n, _)| *n == names).map(|(_, a)| *a);
        let layers = match cached {
            Some(a) => a,
            None => {
                let loaded = crate::three::load_terrain_layers(ctx, path, data).map_err(|e| e.to_string())?;
                if let Some(state) = self.terrain() {
                    if let Some((_, Some(old))) = state.layers.take() {
                        ctx.free(old);
                    }
                    state.layers = Some((names, loaded));
                }
                loaded
            }
        };
        let built =
            crate::three::build_terrain(ctx, data, Vec2::new(corner.x, corner.z), layers).map_err(|e| e.to_string())?;
        // The tile made before (the editor's, or `spawn_terrain`'s) goes.
        if let Some(old) = ctx.world.get::<crate::terrain::TerrainTile>(entity).copied() {
            ctx.free(old);
        }
        if let Some(Some((material, weights))) = self.terrain().and_then(|s| s.built.take()).map(|b| b.material) {
            ctx.free(material);
            ctx.free(weights);
        }
        ctx.world.insert(entity, built.tile);
        if let Some(state) = self.terrain() {
            state.built = Some(built);
        }
        Ok(())
    }

    /// The terrain window, one frame, and the tile following the file.
    pub(super) fn terrain_window(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(state) = self.terrain() else { return };
        let Some(path) = state.path.clone() else { return };
        let Some(doc) = ctx.editor.docs.get(&path).cloned() else {
            state.path = None;
            return;
        };
        let version = ctx.editor.docs.version();
        let data = from_doc(&doc);
        if state.applied != version {
            state.applied = version;
            let rebuilt = match &data {
                Ok(d) => self.rebuild_terrain(ctx, &path, d),
                Err(e) => Err(e.clone()),
            };
            if let Some(state) = self.terrain() {
                state.error = rebuilt.err();
            }
        }
        let shown = tile_of(ctx, &path).is_some();
        let unsaved = ctx.editor.docs.unsaved(&path);
        let pngs = near_first(self.asset_files("png"), &path);
        let mut open = true;
        let mut save = false;
        let mut show = false;
        let mut edited = None;
        egui::Window::new(format!("Terrain — {path}{}", if unsaved { " *" } else { "" }))
            .id(egui::Id::unique("ponko-terrain-window"))
            .open(&mut open)
            .default_pos(egui::pos2(320.0, 80.0))
            .show(ui.ctx(), |ui| {
                let Some(state) = self.app.as_mut().map(|a| &mut a.author.terrain) else {
                    return;
                };
                let Ok(data) = &data else {
                    if let Some(e) = &state.error {
                        ui.colored_label(Color32::LIGHT_RED, e);
                    }
                    return;
                };
                let mut d = data.clone();
                ui.horizontal(|ui| {
                    save = ui.add_enabled(unsaved, egui::Button::new("Save")).clicked();
                    if !shown {
                        show = ui
                            .button("Show in the view")
                            .on_hover_text(
                                "Put the tile in the middle of the world to shape it (not saved with the scene)",
                            )
                            .clicked();
                    }
                });
                ui.horizontal(|ui| {
                    for (brush, label) in [
                        (Brush::Raise, "Raise"),
                        (Brush::Lower, "Lower"),
                        (Brush::Smooth, "Smooth"),
                        (Brush::Paint, "Paint"),
                    ] {
                        ui.selectable_value(&mut state.brush, brush, label);
                    }
                });
                if state.radius <= 0.0 {
                    state.radius = d.json.size / 8.0;
                }
                if state.strength <= 0.0 {
                    state.strength = default_strength(state.brush, &d);
                }
                egui::Grid::new("ponko-terrain-brush").num_columns(2).show(ui, |ui| {
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(&mut state.radius)
                            .speed(0.1)
                            .range(0.1..=d.json.size),
                    );
                    ui.end_row();
                    ui.label("Strength");
                    ui.add(
                        egui::DragValue::new(&mut state.strength)
                            .speed(0.05)
                            .range(0.01..=1000.0),
                    );
                    ui.end_row();
                    if state.brush == Brush::Paint {
                        ui.label("Layer");
                        let name = |i: usize| {
                            d.json
                                .layers
                                .get(i)
                                .map_or("(no layers)".into(), |l| format!("{i}: {}", l.texture))
                        };
                        egui::ComboBox::from_id_salt("ponko-terrain-layer")
                            .selected_text(name(state.layer))
                            .show_ui(ui, |ui| {
                                for i in 0..d.json.layers.len() {
                                    ui.selectable_value(&mut state.layer, i, name(i));
                                }
                            });
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new(if shown {
                        "Drag over the tile in the view to use the brush"
                    } else {
                        "The tile isn't in the world: Show in the view, or spawn it with spawn_terrain"
                    })
                    .weak(),
                );
                ui.separator();
                egui::Grid::new("ponko-terrain-settings").num_columns(2).show(ui, |ui| {
                    ui.label("Size");
                    ui.add(egui::DragValue::new(&mut d.json.size).speed(0.5).range(1.0..=100_000.0));
                    ui.end_row();
                    for (label, which) in [("Low", 0), ("High", 1)] {
                        ui.label(label);
                        let mut v = if which == 0 { d.json.low } else { d.json.high };
                        if ui.add(egui::DragValue::new(&mut v).speed(0.1)).changed() {
                            // The ground stays where it is; only the range changes.
                            let world = d.world_heights();
                            if which == 0 {
                                d.json.low = v.min(d.json.high - 0.01);
                            } else {
                                d.json.high = v.max(d.json.low + 0.01);
                            }
                            d.heights = world.iter().map(|&h| d.to_stored(h)).collect();
                        }
                        ui.end_row();
                    }
                    ui.label("Roughness");
                    ui.add(egui::DragValue::new(&mut d.json.roughness).speed(0.01).range(0.0..=1.0));
                    ui.end_row();
                });
                ui.label(RichText::new("Layers").strong());
                let mut remove = None;
                for (i, layer) in d.json.layers.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{i}: {}", layer.texture));
                        ui.label("size");
                        ui.add(egui::DragValue::new(&mut layer.size).speed(0.1).range(0.1..=10_000.0));
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    remove_layer(&mut d, i);
                }
                egui::ComboBox::from_id_salt("ponko-terrain-add-layer")
                    .selected_text("+ layer")
                    .show_ui(ui, |ui| {
                        for png in &pngs {
                            if ui.selectable_label(false, png).clicked() {
                                add_layer(&mut d, &path, png);
                            }
                        }
                    });
                if let Some(e) = &state.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                if d != *data {
                    edited = Some(to_doc(&d));
                }
            });
        if let Some(doc) = edited {
            ctx.editor.set_doc_merged(&path, doc);
        }
        if show && let Ok(d) = &data {
            let at = Vec3::new(-d.json.size / 2.0, 0.0, -d.json.size / 2.0);
            ctx.world.spawn((
                crate::d3::Transform::at(at),
                TerrainSource { path: path.clone() },
                crate::reflect::NotSaved,
            ));
            if let Some(state) = self.terrain() {
                state.applied = u64::MAX;
            }
        }
        if save {
            let saved = ctx.editor.docs.save(&path).map_err(|e| e.to_string());
            if self.report(saved).is_some() {
                self.status = Some(format!("saved {path}"));
            }
        }
        if !open {
            ctx.editor.docs.close(&path);
            if let Some(state) = self.terrain() {
                state.path = None;
            }
        }
    }

    /// The brush in the 3D view: `true` when it took the press.
    pub(super) fn terrain_brush(&mut self, ui: &egui::Ui, ctx: &mut Ctx, pos: Vec2) -> bool {
        let Some(state) = self.app.as_ref().map(|a| &a.author.terrain) else {
            return false;
        };
        let Some(path) = state.path.clone() else { return false };
        let (brush, layer, radius, strength) = (state.brush, state.layer, state.radius, state.strength);
        let (pressed, down, dt) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.stable_dt.min(0.1),
            )
        });
        if !(pressed || down) {
            return false;
        }
        let Some((_, corner)) = tile_of(ctx, &path) else {
            return false;
        };
        let Some(doc) = ctx.editor.docs.get(&path) else {
            return false;
        };
        let Ok(mut data) = from_doc(doc) else { return false };
        let ray = crate::edit3d::Ray::from_screen(&ctx.scene3d.camera, pos);
        let Some(at) = data.hit(corner, ray.from, ray.dir) else {
            return false;
        };
        let radius = if radius > 0.0 { radius } else { data.json.size / 8.0 };
        let strength = if strength > 0.0 {
            strength
        } else {
            default_strength(brush, &data)
        };
        data.stroke(brush, layer, at, radius, strength, dt);
        ctx.editor.set_doc_merged(&path, to_doc(&data));
        true
    }
}

/// The brush's strength until it's set: a quarter of the height range a
/// second for raise and lower, half the way a second for smooth and paint.
fn default_strength(brush: Brush, data: &TerrainData) -> f32 {
    match brush {
        Brush::Raise | Brush::Lower => (data.json.high - data.json.low) * 0.25,
        Brush::Smooth | Brush::Paint => 2.0,
    }
}

/// Adds ground layer `png` (an asset name) to the terrain file `path`, with
/// a weight map when it needs one more: the first layer covers everything.
fn add_layer(d: &mut TerrainData, path: &str, png: &str) {
    if d.json.layers.len() >= crate::terrain::TerrainTile::LAYERS {
        return;
    }
    let texture = relative(path, png);
    let first = d.json.layers.is_empty();
    d.json.layers.push(TerrainLayerJson { texture, size: 4.0 });
    if d.json.weights.len() < d.json.layers.len().div_ceil(4) {
        if d.side == 0 {
            d.side = 64;
        }
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".json");
        let number = d.json.weights.len();
        d.json.weights.push(if number == 0 {
            format!("{stem}-weights.png")
        } else {
            format!("{stem}-weights{}.png", number + 1)
        });
        let texels = (d.side * d.side) as usize;
        let mut image = vec![0u8; texels * 4];
        if first {
            for t in 0..texels {
                image[t * 4] = 255;
            }
        }
        d.weights.push(image);
    }
}

/// Takes ground layer `i` out, with its weights (the later layers move up).
fn remove_layer(d: &mut TerrainData, i: usize) {
    if i >= d.json.layers.len() {
        return;
    }
    let count = d.json.layers.len();
    let texels = (d.side * d.side) as usize;
    let get = |w: &[Vec<u8>], layer: usize, t: usize| w[layer / 4][t * 4 + layer % 4];
    let old = d.weights.clone();
    d.json.layers.remove(i);
    let images = d.json.layers.len().div_ceil(4);
    d.json.weights.truncate(images);
    d.weights = vec![vec![0u8; texels * 4]; images];
    for layer in 0..count - 1 {
        let from = if layer < i { layer } else { layer + 1 };
        for t in 0..texels {
            d.weights[layer / 4][t * 4 + layer % 4] = get(&old, from, t);
        }
    }
    if d.json.layers.is_empty() {
        d.side = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::super::author_harness::*;
    use super::*;

    /// A terrain file name in `assets/`; the files saved for it go on drop.
    struct Fixture {
        path: String,
        dir: std::path::PathBuf,
    }

    impl Fixture {
        fn new(what: &str) -> Self {
            let folder = format!("ponko-test-terrain-{what}-{}", std::process::id());
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&folder);
            std::fs::create_dir_all(&dir).unwrap();
            let mut png = Vec::new();
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&[90u8; 16]).unwrap();
            std::fs::write(dir.join("grass.png"), &png).unwrap();
            std::fs::write(dir.join("rock.png"), &png).unwrap();
            Self {
                path: format!("{folder}/hill.json"),
                dir,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A new terrain shown in the view, with the camera above its middle
    /// looking down.
    fn terrain_editor(f: &Fixture) -> Harness {
        let mut h = Harness::new();
        h.click_text(|t| t == "Author");
        h.click_text(|t| t == "Terrain…");
        h.frame(vec![]);
        h.type_text(&f.path);
        assert_eq!(
            state(&h).path.as_deref(),
            Some(f.path.as_str()),
            "{:?}",
            h.panels.app.as_ref().unwrap().author.error
        );
        h.click_text(|t| t == "Show in the view");
        h.frame(vec![]);
        let camera = &mut h.ctx.scene3d.camera;
        camera.position = Vec3::new(0.0, 60.0, 0.001);
        camera.look_at(Vec3::ZERO);
        h
    }

    fn state(h: &Harness) -> &TerrainState {
        &h.panels.app.as_ref().unwrap().author.terrain
    }

    fn data(h: &Harness, f: &Fixture) -> TerrainData {
        from_doc(h.ctx.editor.docs.get(&f.path).unwrap()).unwrap()
    }

    /// Where the world's origin (the tile's middle) is on the screen.
    fn middle(h: &Harness) -> glam::Vec2 {
        let camera = &h.ctx.scene3d.camera;
        let clip = camera.view_proj() * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        camera.origin() + glam::Vec2::new((ndc.x + 1.0) / 2.0, (1.0 - ndc.y) / 2.0) * camera.viewport()
    }

    /// Holds the button down at `at` for `frames` frames.
    fn hold(h: &mut Harness, at: glam::Vec2, frames: usize) {
        h.press(at);
        for _ in 0..frames {
            h.frame(vec![]);
        }
        h.release(at);
    }

    #[test]
    fn holding_the_raise_brush_lifts_the_ground_under_it_in_one_undo_step() {
        let f = Fixture::new("raise");
        let mut h = terrain_editor(&f);
        let flat = data(&h, &f).world_heights();
        assert!(flat.iter().all(|y| y.abs() < 1e-3), "a new tile is flat at 0");
        let at = middle(&h);
        hold(&mut h, at, 30);
        let d = data(&h, &f);
        let raised = d.world_heights();
        let centre = raised[32 * 65 + 32];
        assert!(centre > 1.0, "the middle went up: {centre}");
        assert!(raised[0].abs() < 1e-3, "a corner, outside the brush, stays");
        // The tile in the world follows: its ground is higher in the middle.
        h.frame(vec![]);
        let tile = h.ctx.world.query::<&crate::terrain::TerrainTile>().iter().count();
        assert_eq!(tile, 1, "one tile, made again");
        h.undo();
        assert!(
            data(&h, &f).world_heights().iter().all(|y| y.abs() < 1e-3),
            "the stroke was one step"
        );
    }

    #[test]
    fn smoothing_flattens_a_spike() {
        let mut d = new_terrain("t.json");
        let i = 32 * 65 + 32;
        d.heights[i] = d.to_stored(10.0);
        d.stroke(Brush::Smooth, 0, Vec2::splat(32.0), 4.0, 2.0, 0.5);
        let after = d.world_heights()[i];
        assert!(after < 5.0 && after > 0.0, "{after}");
    }

    #[test]
    fn a_layer_added_and_painted_is_saved_as_png_weights_that_spawn_terrain_loads() {
        let f = Fixture::new("paint");
        let mut h = terrain_editor(&f);
        // Two layers from the "+ layer" menu.
        for png in ["grass.png", "rock.png"] {
            h.click_text(|t| t == "+ layer");
            let name = f.path.replace("hill.json", png);
            h.click_last_text(|t| t == name);
        }
        let d = data(&h, &f);
        assert_eq!(d.json.layers.len(), 2);
        assert_eq!(d.json.weights, vec!["hill-weights.png".to_string()]);
        assert_eq!(d.weights[0][0..4], [255, 0, 0, 0], "the first layer covers it");
        // Paint layer 1 in the middle.
        h.click_text(|t| t == "Paint");
        let field = field_right_of(&h.frame(vec![]), "Layer");
        h.click(field);
        h.click_last_text(|t| t.starts_with("1: "));
        assert_eq!(state(&h).layer, 1);
        let at = middle(&h);
        hold(&mut h, at, 30);
        let d = data(&h, &f);
        let centre = (32 * 64 + 32) * 4;
        assert!(
            d.weights[0][centre + 1] > 200,
            "layer 1 shows in the middle: {:?}",
            &d.weights[0][centre..centre + 4]
        );
        assert_eq!(d.weights[0][0..4], [255, 0, 0, 0], "the corner keeps layer 0");
        h.save();
        assert!(!h.ctx.editor.docs.unsaved(&f.path));
        // The runtime reads what was saved.
        let e = h.ctx.spawn_terrain(&f.path, Vec3::ZERO).unwrap();
        assert!(h.ctx.world.get::<crate::terrain::TerrainTile>(e).is_some());
        let text = std::fs::read_to_string(f.dir.join("hill.json")).unwrap();
        assert!(text.contains("\"ponko-terrain\""), "{text}");
        assert!(f.dir.join("hill-heights.png").exists() && f.dir.join("hill-weights.png").exists());
    }
}
