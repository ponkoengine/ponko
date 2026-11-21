//! Thumbnails for the editor's asset browser, made from the files on a
//! worker thread (nothing here touches the world or the GPU):
//!
//! - a texture, scaled down;
//! - a glTF model, drawn from above at an angle by a small software
//!   rasterizer (flat shaded, depth tested) at first; then, when the game
//!   has a GPU, by the engine's own renderer on the game thread (see
//!   `editor_thumb_studio.rs`), kept on disk by the model's content so the
//!   next run shows it at once;
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

/// Bumped when rendered thumbnails look different: older cached ones are
/// then made again.
const LOOK: &str = "ponko-model-thumbnail-1";

/// The folder rendered model thumbnails are kept in: the user's cache
/// folder (`PONKO_THUMBNAIL_CACHE` names another). `None` in unit tests
/// (they name their own) and on the web.
fn default_cache() -> Option<PathBuf> {
    if cfg!(test) || cfg!(target_arch = "wasm32") {
        return None;
    }
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("PONKO_THUMBNAIL_CACHE") {
        return Some(dir);
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    base.map(|b| b.join("ponko").join("thumbnails"))
}

/// The files a glTF file names (its buffers and images, not inline data),
/// as it names them.
pub(crate) fn named_files(bytes: &[u8]) -> Vec<String> {
    #[cfg(feature = "3d")]
    if let Ok(gltf) = gltf::Gltf::from_slice(bytes) {
        let buffers = gltf.buffers().filter_map(|b| match b.source() {
            gltf::buffer::Source::Uri(uri) => Some(uri.to_string()),
            gltf::buffer::Source::Bin => None,
        });
        let images = gltf.images().filter_map(|i| match i.source() {
            gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
            gltf::image::Source::View { .. } => None,
        });
        return buffers.chain(images).filter(|u| !u.starts_with("data:")).collect();
    }
    let _ = bytes;
    Vec::new()
}

/// The name a model's rendered thumbnail is kept under: a hash of the
/// file, of the files it names, of the thumbnail size and of [`LOOK`], so
/// a changed model (or texture) is drawn again and an unchanged one never.
pub(crate) fn content_key(file: &Path, bytes: &[u8]) -> String {
    // FNV-1a, 64 bits, with the length beside it.
    fn add(mut h: u64, bytes: &[u8]) -> u64 {
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    }
    let mut h = add(0xcbf2_9ce4_8422_2325, bytes);
    let mut len = bytes.len();
    let folder = file.parent().unwrap_or(Path::new("."));
    for uri in named_files(bytes) {
        let named = std::fs::read(folder.join(&uri)).unwrap_or_default();
        h = add(add(h, uri.as_bytes()), &named);
        len += named.len();
    }
    h = add(add(h, &(SIZE as u32).to_le_bytes()), LOOK.as_bytes());
    format!("{h:016x}-{len}")
}

/// A model to draw with the renderer: `rel` in assets folder `root`, kept
/// as `png` (`None`: no cache folder). Builds without the renderer's studio
/// (no `3d`, or the web) drop these and keep the software thumbnail.
#[derive(Debug, Clone)]
#[cfg_attr(any(not(feature = "3d"), target_arch = "wasm32"), allow(dead_code))]
struct RenderJob {
    root: PathBuf,
    rel: String,
    png: Option<PathBuf>,
}

/// [`make`], with model thumbnails from the cache folder `cache` when it
/// holds this content's; else the software one, and the model to draw.
fn make_cached(
    root: &Path,
    rel: &str,
    kind: AssetKind,
    cache: Option<&Path>,
) -> (Result<Thumb, String>, Option<RenderJob>) {
    if kind != AssetKind::Model {
        return (make(root, rel, kind), None);
    }
    let file = root.join(rel);
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => return (Err(format!("{rel}: {e}")), None),
    };
    let soft = model(&file, &bytes).map_err(|e| format!("{rel}: {e}"));
    let Ok(soft) = soft else { return (soft, None) };
    let png = cache.map(|c| c.join(format!("{}.png", content_key(&file, &bytes))));
    if let Some(png) = &png
        && let Ok(image) = image::open(png)
    {
        let image = image.to_rgba8();
        let cached = Thumb {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba: image.into_raw(),
            note: soft.note,
        };
        return (Ok(cached), None);
    }
    let job = RenderJob {
        root: root.to_path_buf(),
        rel: rel.to_string(),
        png,
    };
    (Ok(soft), Some(job))
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

/// What the worker is asked for: (generation, key, assets folder, path,
/// kind, cache folder).
type Job = (u64, String, PathBuf, String, AssetKind, Option<PathBuf>);
/// What it hands back: (generation, key, thumbnail, model to draw with
/// the renderer).
type Done = (u64, String, Result<Thumb, String>, Option<RenderJob>);

/// Thumbnails made on a worker thread and kept by asset path.
pub(crate) struct Thumbnails {
    ready: HashMap<String, Result<Thumb, String>>,
    asked: HashSet<String>,
    jobs: Option<mpsc::Sender<Job>>,
    done: mpsc::Receiver<Done>,
    done_tx: mpsc::Sender<Done>,
    /// Where rendered model thumbnails are kept (see [`content_key`]).
    pub cache: Option<PathBuf>,
    /// Models waiting for the renderer, oldest first.
    renders: Vec<RenderJob>,
    /// Paths whose thumbnail the renderer replaced since the panel last looked.
    updated: Vec<String>,
    #[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
    studio: Option<super::thumb_studio::Studio>,
    /// Counts `clear`s: what was asked for before one is dropped.
    generation: u64,
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
            cache: default_cache(),
            renders: Vec::new(),
            updated: Vec::new(),
            #[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
            studio: None,
            generation: 0,
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
            let job = (
                self.generation,
                rel.to_string(),
                root.to_path_buf(),
                rel.to_string(),
                kind,
                self.cache.clone(),
            );
            let sent = self.worker().send(job);
            if let Err(mpsc::SendError((_, key, root, rel, kind, cache))) = sent {
                // The worker is gone: make it here.
                let (thumb, render) = make_cached(&root, &rel, kind, cache.as_deref());
                self.ready.insert(key, thumb);
                self.renders.extend(render);
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
                    for (generation, key, root, rel, kind, cache) in rx {
                        let (thumb, render) = make_cached(&root, &rel, kind, cache.as_deref());
                        if done.send((generation, key, thumb, render)).is_err() {
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
        while let Ok(done) = self.done.try_recv() {
            self.take(done);
        }
    }

    fn take(&mut self, (generation, key, thumb, render): Done) {
        // Asked for before a `clear`: made again when asked again.
        if generation != self.generation {
            return;
        }
        self.ready.insert(key, thumb);
        self.renders.extend(render);
    }

    /// Whether a thumbnail asked for isn't made yet.
    fn waiting(&self) -> bool {
        self.asked.iter().any(|k| !self.ready.contains_key(k))
    }

    /// Draws the oldest model waiting for the renderer on `gpu`'s device
    /// (once the worker has made its software thumbnail), keeps it on disk
    /// and in place of the software one. Without a GPU (headless) the
    /// software thumbnails stay. Returns whether one was drawn.
    ///
    /// Web builds have no studio (see `thumb_studio`): there, too, the
    /// software thumbnails stay.
    #[cfg(feature = "3d")]
    pub fn render_models(&mut self, gpu: Option<&crate::render::Renderer>) -> bool {
        self.collect();
        if self.renders.is_empty() {
            return false;
        }
        let Some(gpu) = gpu else {
            self.renders.clear();
            return false;
        };
        self.render_next(gpu)
    }

    #[cfg(all(feature = "3d", target_arch = "wasm32"))]
    fn render_next(&mut self, _: &crate::render::Renderer) -> bool {
        self.renders.clear();
        false
    }

    /// Draws the oldest model in `renders` (see [`Self::render_models`]).
    #[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
    fn render_next(&mut self, gpu: &crate::render::Renderer) -> bool {
        let job = self.renders.remove(0);
        let studio = self.studio.get_or_insert_with(|| super::thumb_studio::Studio::new(gpu));
        let out = job
            .png
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join(format!("ponko-thumbnail-{}.png", std::process::id())));
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let drawn = studio.render(&job.root, &job.rel, &out);
        if job.png.is_none() {
            let _ = std::fs::remove_file(&out);
        }
        match drawn {
            Ok(mut thumb) => {
                thumb.note = self
                    .ready
                    .get(&job.rel)
                    .and_then(|t| t.as_ref().ok())
                    .and_then(|t| t.note.clone());
                self.ready.insert(job.rel.clone(), Ok(thumb));
                self.updated.push(job.rel);
                true
            }
            Err(e) => {
                // The software thumbnail stays.
                eprintln!("ponko: editor thumbnail: {e}");
                false
            }
        }
    }

    /// The paths whose thumbnail changed since the last call.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub fn take_updated(&mut self) -> Vec<String> {
        std::mem::take(&mut self.updated)
    }

    /// Thumbnails made since the last call: the panel turns them into
    /// textures.
    pub fn is_pending(&self) -> bool {
        self.waiting() || !self.renders.is_empty()
    }

    /// Waits (at most `timeout`: a hang detector) until every thumbnail asked for is made.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        let end = web_time::Instant::now() + timeout;
        self.collect();
        while self.waiting() {
            let left = end.saturating_duration_since(web_time::Instant::now());
            match self.done.recv_timeout(left) {
                Ok(done) => self.take(done),
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
        self.renders.clear();
        self.updated.clear();
        self.generation += 1;
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

    #[cfg(feature = "3d")]
    #[test]
    fn the_cache_name_follows_the_model_and_the_files_it_names_not_where_it_is() {
        let dir = std::env::temp_dir().join(format!("ponko-thumb-key-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        let gltf = br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"cube.bin"}]}"#;
        assert_eq!(named_files(gltf), ["cube.bin"]);
        std::fs::write(dir.join("cube.bin"), [1, 2, 3, 4]).unwrap();
        std::fs::write(dir.join("a/cube.bin"), [1, 2, 3, 4]).unwrap();
        let here = content_key(&dir.join("m.gltf"), gltf);
        assert_eq!(
            here,
            content_key(&dir.join("a/other.gltf"), gltf),
            "the same content elsewhere"
        );
        std::fs::write(dir.join("a/cube.bin"), [9, 2, 3, 4]).unwrap();
        let changed = content_key(&dir.join("a/other.gltf"), gltf);
        let _ = std::fs::remove_dir_all(&dir);
        assert_ne!(here, changed, "a changed buffer is drawn again");
        assert_ne!(here, content_key(&dir.join("m.gltf"), b"{}"));
    }

    #[cfg(feature = "3d")]
    #[test]
    fn a_cached_model_thumbnail_is_used_and_an_uncached_one_waits_for_the_renderer() {
        let cache = std::env::temp_dir().join(format!("ponko-thumb-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let mut thumbs = Thumbnails {
            cache: Some(cache.clone()),
            ..Thumbnails::default()
        };
        assert!(thumbs.get(&fixtures(), "grid.glb", AssetKind::Model).is_none());
        assert!(thumbs.wait(std::time::Duration::from_secs(10)));
        let soft = make(&fixtures(), "grid.glb", AssetKind::Model).unwrap();
        assert_eq!(
            thumbs
                .get(&fixtures(), "grid.glb", AssetKind::Model)
                .unwrap()
                .as_ref()
                .unwrap(),
            &soft
        );
        assert!(thumbs.is_pending(), "waiting for the renderer");
        assert!(!thumbs.render_models(None), "no GPU: nothing drawn");
        assert!(!thumbs.is_pending(), "and the software one stays");

        // A thumbnail kept under the model's content is used as it is.
        let bytes = std::fs::read(fixtures().join("grid.glb")).unwrap();
        let png = cache.join(format!("{}.png", content_key(&fixtures().join("grid.glb"), &bytes)));
        std::fs::create_dir_all(&cache).unwrap();
        image::RgbaImage::from_pixel(SIZE as u32, SIZE as u32, image::Rgba([255, 0, 255, 255]))
            .save(&png)
            .unwrap();
        thumbs.clear();
        assert!(thumbs.get(&fixtures(), "grid.glb", AssetKind::Model).is_none());
        assert!(thumbs.wait(std::time::Duration::from_secs(10)));
        let cached = thumbs
            .get(&fixtures(), "grid.glb", AssetKind::Model)
            .unwrap()
            .clone()
            .unwrap();
        let _ = std::fs::remove_dir_all(&cache);
        assert_eq!(&cached.rgba[..4], [255, 0, 255, 255]);
        assert_eq!(cached.note, soft.note, "with its triangle count");
        assert!(!thumbs.is_pending(), "nothing to draw");
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
