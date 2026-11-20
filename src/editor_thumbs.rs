//! Thumbnails for the editor's asset browser, made from the files on a
//! worker thread (nothing here touches the world or the GPU):
//!
//! - a texture, scaled down;
//! - a glTF model, drawn from above at an angle by a small software
//!   rasterizer (flat shaded, depth tested);
//! - a prefab, as its first sprite's texture or its model, else a map of
//!   where its entities are;
//! - a scene, as a map of where its entities are (2D positions, or 3D ones
//!   seen from above);
//! - a sound, as its waveform (with the `audio` feature) and its length.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde_json::Value;

use super::cook::AssetKind;

/// Thumbnails are this many pixels on their long side.
pub(crate) const SIZE: usize = 64;

/// A thumbnail: RGBA8 pixels, and a line about the file (a sound's length).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Thumb {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub note: Option<String>,
}

impl Thumb {
    fn blank(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width * height * 4],
            note: None,
        }
    }

    fn put(&mut self, x: usize, y: usize, c: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = (y * self.width + x) * 4;
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }

    /// Whether any pixel isn't fully transparent.
    #[cfg(test)]
    pub fn is_drawn(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
    }
}

/// Makes the thumbnail of asset `rel` (relative to `root`, the assets
/// folder) of kind `kind`.
pub(crate) fn make(root: &Path, rel: &str, kind: AssetKind) -> Result<Thumb, String> {
    let file = root.join(rel);
    let bytes = std::fs::read(&file).map_err(|e| format!("{rel}: {e}"))?;
    match kind {
        AssetKind::Texture => texture(&bytes).map_err(|e| format!("{rel}: {e}")),
        AssetKind::Model => model(&file, &bytes).map_err(|e| format!("{rel}: {e}")),
        AssetKind::Sound => sound(&bytes).map_err(|e| format!("{rel}: {e}")),
        AssetKind::Prefab | AssetKind::Scene => {
            let json: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{rel}: {e}"))?;
            if kind == AssetKind::Prefab
                && let Some(thumb) = prefab_look(root, &json)
            {
                return Ok(thumb);
            }
            Ok(map(&json, kind == AssetKind::Prefab))
        }
        AssetKind::Shader => Err(format!("{rel}: no thumbnail for this kind")),
    }
}

fn texture(bytes: &[u8]) -> Result<Thumb, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let small = image.thumbnail(SIZE as u32, SIZE as u32).to_rgba8();
    Ok(Thumb {
        width: small.width() as usize,
        height: small.height() as usize,
        rgba: small.into_raw(),
        note: Some(format!("{}×{}", image.width(), image.height())),
    })
}

/// A prefab's first sprite texture, else its first model.
fn prefab_look(root: &Path, json: &Value) -> Option<Thumb> {
    let mut entities: Vec<&Value> = json["entities"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if let Some(c) = json.get("components") {
        entities.push(c);
    }
    let components = entities.iter().map(|e| e.get("components").unwrap_or(e));
    for c in components {
        if let Some(t) = c["Sprite"]["texture"].as_str() {
            return std::fs::read(root.join(t)).ok().and_then(|b| texture(&b).ok());
        }
        if let Some(m) = c["Model"]["mesh"].as_str() {
            let file = root.join(m);
            return std::fs::read(&file).ok().and_then(|b| model(&file, &b).ok());
        }
    }
    None
}

/// Where a scene's (or prefab's) entities are, as dots fitted to the
/// thumbnail.
fn map(json: &Value, prefab: bool) -> Thumb {
    let mut points = Vec::new();
    let list = json["entities"].as_array().cloned().unwrap_or_default();
    for e in &list {
        let c = &e["components"];
        let p2 = &c["Transform2D"]["position"];
        let p3 = &c["Transform3D"]["position"];
        let xy = |v: &Value, i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
        if p3.is_array() {
            points.push((xy(p3, 0), xy(p3, 2)));
        } else if p2.is_array() {
            // Up is +y in 2D; the image's rows go down.
            points.push((xy(p2, 0), -xy(p2, 1)));
        } else if !c.is_null() {
            points.push((0.0, 0.0));
        }
    }
    let mut t = Thumb::blank(SIZE, SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            t.put(x, y, [32, 36, 44, 255]);
        }
    }
    let (min, max) = points
        .iter()
        .fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), &(x, y)| {
            ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)))
        });
    let span = (max.0 - min.0).max(max.1 - min.1).max(1e-3);
    let margin = 6.0;
    let scale = (SIZE as f32 - 2.0 * margin) / span;
    let color = if prefab {
        [255, 200, 90, 255]
    } else {
        [120, 200, 255, 255]
    };
    for &(x, y) in &points {
        let cx = margin + (x - min.0) * scale + (span - (max.0 - min.0)) * scale / 2.0;
        let cy = margin + (y - min.1) * scale + (span - (max.1 - min.1)) * scale / 2.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                t.put((cx as i32 + dx) as usize, (cy as i32 + dy) as usize, color);
            }
        }
    }
    t.note = Some(format!(
        "{} entities",
        list.len().max(usize::from(json.get("components").is_some()))
    ));
    t
}

#[cfg(feature = "audio")]
fn sound(bytes: &[u8]) -> Result<Thumb, String> {
    let data = kira::sound::static_sound::StaticSoundData::from_cursor(std::io::Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("can't decode it: {e}"))?;
    let frames = &data.frames;
    let (w, h) = (SIZE, SIZE / 2);
    let mut t = Thumb::blank(w, h);
    let per = frames.len().div_ceil(w).max(1);
    for x in 0..w {
        let chunk = frames.iter().skip(x * per).take(per);
        let peak = chunk
            .fold(0.0f32, |m, f| m.max(f.left.abs()).max(f.right.abs()))
            .min(1.0);
        let half = (peak * (h as f32 / 2.0 - 1.0)).round() as usize;
        for y in h / 2 - half..=h / 2 + half {
            t.put(x, y, [140, 220, 140, 255]);
        }
    }
    let seconds = frames.len() as f64 / f64::from(data.sample_rate.max(1));
    t.note = Some(format!("{seconds:.2} s"));
    Ok(t)
}

#[cfg(not(feature = "audio"))]
fn sound(_: &[u8]) -> Result<Thumb, String> {
    Err("sounds need the `audio` feature".into())
}

#[cfg(feature = "3d")]
fn model(file: &Path, bytes: &[u8]) -> Result<Thumb, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a glTF file: {e}"))?;
    let folder = file.parent().unwrap_or(Path::new("."));
    let mut blob = gltf.blob.clone();
    let mut buffers = Vec::new();
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => blob.take().ok_or("missing its binary chunk")?,
            gltf::buffer::Source::Uri(uri) if !uri.starts_with("data:") => {
                std::fs::read(folder.join(uri)).map_err(|e| format!("{uri}: {e}"))?
            }
            gltf::buffer::Source::Uri(_) => return Err("embedded (data:) buffers have no thumbnail".into()),
        };
        buffers.push(data);
    }
    let mut triangles: Vec<[glam::Vec3; 3]> = Vec::new();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or("no scene")?;
    let mut stack: Vec<(gltf::Node, glam::Mat4)> = scene.nodes().map(|n| (n, glam::Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * glam::Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives() {
                let reader = prim.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let p: Vec<glam::Vec3> = positions.map(|v| world.transform_point3(v.into())).collect();
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..p.len() as u32).collect(),
                };
                for t in indices.as_chunks::<3>().0 {
                    if let (Some(&a), Some(&b), Some(&c)) =
                        (p.get(t[0] as usize), p.get(t[1] as usize), p.get(t[2] as usize))
                    {
                        triangles.push([a, b, c]);
                    }
                }
                if triangles.len() > 200_000 {
                    break;
                }
            }
        }
        stack.extend(node.children().map(|c| (c, world)));
    }
    if triangles.is_empty() {
        return Err("no triangles".into());
    }
    let mut t = raster(&triangles);
    t.note = Some(format!("{} triangles", triangles.len()));
    Ok(t)
}

#[cfg(not(feature = "3d"))]
fn model(_: &Path, _: &[u8]) -> Result<Thumb, String> {
    Err("models need the `3d` feature".into())
}

/// Draws `triangles` seen from the front-right, above, fitted to the
/// thumbnail; flat shaded with a light from the upper left.
#[cfg(feature = "3d")]
fn raster(triangles: &[[glam::Vec3; 3]]) -> Thumb {
    use glam::{Mat3, Vec3};
    let view = Mat3::from_rotation_x(-0.5) * Mat3::from_rotation_y(-0.6);
    let tri: Vec<[Vec3; 3]> = triangles.iter().map(|t| t.map(|v| view * v)).collect();
    let (lo, hi) = tri
        .iter()
        .flatten()
        .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let span = (hi.x - lo.x).max(hi.y - lo.y).max(1e-6);
    let margin = 4.0;
    let scale = (SIZE as f32 - 2.0 * margin) / span;
    let off = glam::vec2(
        margin + (span - (hi.x - lo.x)) * scale / 2.0,
        margin + (span - (hi.y - lo.y)) * scale / 2.0,
    );
    let to_px = |v: Vec3| {
        // Image rows go down; +y is up.
        Vec3::new(off.x + (v.x - lo.x) * scale, off.y + (hi.y - v.y) * scale, v.z)
    };
    let light = Vec3::new(-0.4, 0.8, 0.45).normalize();
    let mut t = Thumb::blank(SIZE, SIZE);
    let mut depth = vec![f32::MIN; SIZE * SIZE];
    for [a, b, c] in &tri {
        let n = (*b - *a).cross(*c - *a);
        if n.length_squared() == 0.0 {
            continue;
        }
        let shade = 0.35 + 0.65 * n.normalize().dot(light).abs();
        let color = [(110.0 * shade) as u8, (150.0 * shade) as u8, (200.0 * shade) as u8, 255];
        let (a, b, c) = (to_px(*a), to_px(*b), to_px(*c));
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
        let x1 = (a.x.max(b.x).max(c.x).ceil() as usize).min(SIZE - 1);
        let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
        let y1 = (a.y.max(b.y).max(c.y).ceil() as usize).min(SIZE - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a.z + w1 * b.z + w2 * c.z;
                let i = y * SIZE + x;
                // The camera looks down -z: nearer is larger z.
                if z > depth[i] {
                    depth[i] = z;
                    t.put(x, y, color);
                }
            }
        }
    }
    t
}

/// What the worker is asked for: (key, assets folder, path, kind).
type Job = (String, PathBuf, String, AssetKind);

/// Thumbnails made on a worker thread and kept by asset path.
pub(crate) struct Thumbnails {
    ready: HashMap<String, Result<Thumb, String>>,
    asked: HashSet<String>,
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Receiver<(String, Result<Thumb, String>)>,
    done_tx: mpsc::Sender<(String, Result<Thumb, String>)>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        let (done_tx, done) = mpsc::channel();
        Self {
            ready: HashMap::new(),
            asked: HashSet::new(),
            jobs: None,
            done,
            done_tx,
        }
    }
}

impl std::fmt::Debug for Thumbnails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbnails")
            .field("ready", &self.ready.len())
            .field("asked", &self.asked.len())
            .finish()
    }
}

impl Thumbnails {
    /// The thumbnail of `rel`, if made; else it's asked for (once).
    pub fn get(&mut self, root: &Path, rel: &str, kind: AssetKind) -> Option<&Result<Thumb, String>> {
        self.collect();
        if !self.ready.contains_key(rel) && self.asked.insert(rel.to_string()) {
            let job = (rel.to_string(), root.to_path_buf(), rel.to_string(), kind);
            let sent = self.worker().send(job);
            if let Err(mpsc::SendError((key, root, rel, kind))) = sent {
                // The worker is gone: make it here.
                self.ready.insert(key, make(&root, &rel, kind));
            }
        }
        self.ready.get(rel)
    }

    fn worker(&mut self) -> &mpsc::Sender<Job> {
        self.jobs.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let done = self.done_tx.clone();
            let spawned = std::thread::Builder::new()
                .name("ponko-thumbnails".into())
                .spawn(move || {
                    for (key, root, rel, kind) in rx {
                        if done.send((key, make(&root, &rel, kind))).is_err() {
                            break;
                        }
                    }
                });
            if spawned.is_err() {
                // No thread: `get` makes them itself when sending fails.
                let (dead, _) = mpsc::channel();
                return dead;
            }
            tx
        })
    }

    /// Takes the thumbnails the worker finished.
    fn collect(&mut self) {
        while let Ok((key, thumb)) = self.done.try_recv() {
            self.ready.insert(key, thumb);
        }
    }

    /// Thumbnails made since the last call: the panel turns them into
    /// textures.
    pub fn is_pending(&self) -> bool {
        self.asked.len() > self.ready.len()
    }

    /// Waits (at most `timeout`) until every thumbnail asked for is made.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        let end = web_time::Instant::now() + timeout;
        self.collect();
        while self.is_pending() {
            let left = end.saturating_duration_since(web_time::Instant::now());
            match self.done.recv_timeout(left) {
                Ok((key, thumb)) => drop(self.ready.insert(key, thumb)),
                Err(_) => return false,
            }
        }
        true
    }

    /// Forgets every thumbnail (files changed).
    pub fn clear(&mut self) {
        self.collect();
        self.ready.clear();
        self.asked.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cook/source")
    }

    #[test]
    fn a_texture_thumbnail_is_scaled_down_with_its_size_noted() {
        let t = make(&fixtures(), "tile.png", AssetKind::Texture).unwrap();
        assert!(t.width <= SIZE && t.height <= SIZE && t.is_drawn());
        assert_eq!(t.rgba.len(), t.width * t.height * 4);
        assert!(t.note.unwrap().contains('×'));
    }

    #[cfg(feature = "3d")]
    #[test]
    fn a_model_thumbnail_draws_its_triangles_and_leaves_the_corners_empty() {
        let t = make(&fixtures(), "grid.glb", AssetKind::Model).unwrap();
        assert_eq!((t.width, t.height), (SIZE, SIZE));
        let drawn = t.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
        assert!(drawn > SIZE * SIZE / 10, "the model covers part of it: {drawn}");
        assert_eq!(t.rgba[3], 0, "the corner stays empty");
        assert!(t.note.unwrap().contains("triangles"));
    }

    #[cfg(feature = "audio")]
    #[test]
    fn a_sound_thumbnail_is_its_waveform_and_length() {
        let t = make(&fixtures(), "beep.wav", AssetKind::Sound).unwrap();
        assert!(t.is_drawn());
        assert!(t.note.unwrap().ends_with(" s"));
    }

    #[test]
    fn a_scene_thumbnail_maps_its_entities() {
        let json = serde_json::json!({ "format": "ponko-world", "entities": [
            { "entity": 0, "components": { "Transform2D": { "position": [0.0, 0.0] } } },
            { "entity": 1, "components": { "Transform2D": { "position": [100.0, 50.0] } } }
        ]});
        let t = map(&json, false);
        let dots = t
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..3] == [120, 200, 255])
            .count();
        assert_eq!(dots, 18, "two 3×3 dots");
        assert_eq!(t.note.as_deref(), Some("2 entities"));
    }

    #[test]
    fn a_prefab_with_a_sprite_shows_its_texture() {
        let dir = std::env::temp_dir().join(format!("ponko-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(fixtures().join("tile.png"), dir.join("tile.png")).unwrap();
        let prefab = serde_json::json!({ "format": "ponko-prefab", "version": 1,
            "components": { "Sprite": { "texture": "tile.png" } } });
        std::fs::write(dir.join("p.json"), prefab.to_string()).unwrap();
        let t = make(&dir, "p.json", AssetKind::Prefab);
        let tile = make(&fixtures(), "tile.png", AssetKind::Texture).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(t.unwrap(), tile);
    }

    #[test]
    fn thumbnails_are_made_on_the_worker_and_kept() {
        let mut thumbs = Thumbnails::default();
        assert!(
            thumbs.get(&fixtures(), "tile.png", AssetKind::Texture).is_none(),
            "asked for"
        );
        assert!(thumbs.wait(std::time::Duration::from_secs(10)));
        assert!(thumbs.get(&fixtures(), "tile.png", AssetKind::Texture).unwrap().is_ok());
        assert!(thumbs.get(&fixtures(), "nope.png", AssetKind::Texture).is_none());
        assert!(thumbs.wait(std::time::Duration::from_secs(10)));
        assert!(
            thumbs
                .get(&fixtures(), "nope.png", AssetKind::Texture)
                .unwrap()
                .is_err()
        );
    }
}
