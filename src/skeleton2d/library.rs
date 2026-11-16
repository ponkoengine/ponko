//! Loading skeleton files through the asset system, with hot reload.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Skeleton, SkeletonData, SpineAtlas};
use crate::ctx::Ctx;

/// The skeleton files loaded so far, by asset name, and the ones parsed
/// (on first load or a hot reload) whose images still need loading.
#[derive(Default)]
pub(crate) struct SkeletonLibrary {
    pub(crate) loaded: HashMap<String, Arc<SkeletonData>>,
    pending: Vec<(String, SkeletonData)>,
    /// Each file's text and the atlas it reads (Spine exports), to parse
    /// it again when its atlas changes.
    sources: HashMap<String, (String, Option<String>)>,
    /// Spine atlases read so far, by asset name.
    atlases: HashMap<String, Arc<SpineAtlas>>,
}

/// Says `message` where a developer sees it: stderr, or the browser's
/// console on the web.
fn warn(message: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!("ponko: {message}")));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("ponko: {message}");
}

/// Parses skeleton file `name` from `text` (with its atlas, if it has
/// one) and queues it for its images.
fn parse(world: &mut ponko_ecs::World, name: &str, text: &str, atlas: Option<&str>) -> Result<(), crate::error::Error> {
    let library = library(world);
    let atlas_data = atlas.and_then(|a| library.atlases.get(a).cloned());
    let data = SkeletonData::from_json_with(text, name, atlas_data.as_deref())?;
    library
        .sources
        .insert(name.to_string(), (text.to_string(), atlas.map(String::from)));
    library.pending.push((name.to_string(), data));
    Ok(())
}

/// The atlas a Spine export at `path` reads: the same name with `.atlas`.
fn atlas_name(path: &str) -> String {
    let stem = path.strip_suffix(".json").unwrap_or(path);
    format!("{stem}.atlas")
}

impl SkeletonLibrary {
    /// The loaded skeleton file `name`.
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<SkeletonData>> {
        self.loaded.get(name)
    }
}

impl Ctx {
    /// Loads a skeleton file (Ponko's format or a Spine JSON export, see
    /// [`SkeletonData::from_json`]) and the images it names, found next to
    /// it, once: later loads of the same name return the same data. A
    /// Spine export reads the texture atlas beside it (`hero.json` reads
    /// `hero.atlas`, see [`SpineAtlas`](super::SpineAtlas)) and its page
    /// images, or loose images without one; the Spine features it can't
    /// read are printed as [warnings](SkeletonData::warnings). With
    /// [hot reload](crate::game::Game::hot_reload) on, editing the file
    /// (or its atlas) reloads it, and every [`Skeleton`] playing it switches to the new
    /// data at the next update, keeping its clip or state, clock and
    /// parameters by name. A file that doesn't parse keeps the old data
    /// and prints why.
    ///
    /// ```
    /// use ponko::assets::MemorySource;
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::Skeleton;
    /// Game::new("bones", 64, 64).run_headless(0, |ctx| {
    ///     ctx.assets.add_source(MemorySource::new("built-in").with(
    ///         "crab.json",
    ///         r#"{ "format": "ponko-skeleton2d", "version": 1, "bones": [{ "name": "root" }] }"#,
    ///     ));
    ///     let data = ctx.load_skeleton2d("crab.json");
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(32.0, 32.0)), Skeleton::new(data)));
    /// });
    /// ```
    ///
    /// # Panics
    /// If the file or an image is missing or the file isn't a skeleton,
    /// with what is wrong. [`try_load_skeleton2d`](Self::try_load_skeleton2d)
    /// returns the error instead.
    pub fn load_skeleton2d(&mut self, path: &str) -> Arc<SkeletonData> {
        self.try_load_skeleton2d(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_skeleton2d`](Self::load_skeleton2d) that returns what went wrong.
    ///
    /// # Errors
    /// [`NotFound`](crate::error::ErrorKind::NotFound) for a missing file or
    /// image, [`Decode`](crate::error::ErrorKind::Decode) for a file that
    /// isn't a skeleton or an image that doesn't decode.
    pub fn try_load_skeleton2d(&mut self, path: &str) -> Result<Arc<SkeletonData>, crate::error::Error> {
        if let Some(data) = library(&mut self.world).get(path) {
            return Ok(data.clone());
        }
        // A Spine export reads the atlas next to it, if there is one.
        let (_, text) = self.assets.read_text(path)?;
        let spine = serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| super::json::is_spine(&v));
        let mut atlas = None;
        if spine {
            let name = atlas_name(path);
            if library(&mut self.world).atlases.contains_key(&name) {
                atlas = Some(name);
            } else {
                let key = name.clone();
                let read = self.assets.load_data(
                    &mut self.world,
                    &name,
                    Box::new(move |world, text| {
                        let parsed = Arc::new(SpineAtlas::parse(text, &key)?);
                        let library = library(world);
                        library.atlases.insert(key.clone(), parsed);
                        // An edited atlas: parse the files that read it again.
                        let readers: Vec<(String, String)> = library
                            .sources
                            .iter()
                            .filter(|(_, (_, a))| a.as_deref() == Some(key.as_str()))
                            .map(|(n, (t, _))| (n.clone(), t.clone()))
                            .collect();
                        for (n, t) in readers {
                            parse(world, &n, &t, Some(&key))?;
                        }
                        Ok(())
                    }),
                );
                match read {
                    Ok(()) => atlas = Some(name),
                    Err(e) if e.kind() == crate::error::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let name = path.to_string();
        self.assets.load_data(
            &mut self.world,
            path,
            Box::new(move |world, text| parse(world, &name, text, atlas.as_deref())),
        )?;
        self.finish_skeleton_loads()?;
        Ok(library(&mut self.world).get(path).expect("just loaded").clone())
    }

    /// Loads the images of parsed skeleton files and makes them the loaded
    /// data; returns the names that replaced older data (hot reloads).
    fn finish_skeleton_loads(&mut self) -> Result<Vec<String>, crate::error::Error> {
        let pending = std::mem::take(&mut library(&mut self.world).pending);
        let mut replaced = Vec::new();
        for (name, mut data) in pending {
            for a in &mut data.attachments {
                let (image, texture) = a.attachment.texture_mut();
                if !image.is_empty() {
                    *texture = self.gfx.try_load_texture(image)?;
                }
            }
            for image in &data.nearest {
                if let Some(a) = data.attachments.iter().find(|a| a.attachment.image() == &**image) {
                    self.gfx
                        .set_texture_filter(a.attachment.texture(), crate::texture::TextureFilter::Nearest);
                }
            }
            for w in &data.warnings {
                warn(w);
            }
            data.name = Some(name.as_str().into());
            if library(&mut self.world)
                .loaded
                .insert(name.clone(), Arc::new(data))
                .is_some()
            {
                replaced.push(name);
            }
        }
        Ok(replaced)
    }

    /// The built-in update: applies hot reloads, then advances every
    /// skeleton that isn't manual.
    pub(crate) fn update_skeletons2d(&mut self) {
        let reloads = self
            .world
            .get_resource::<SkeletonLibrary>()
            .is_some_and(|l| !l.pending.is_empty());
        if reloads {
            match self.finish_skeleton_loads() {
                Ok(names) if !names.is_empty() => {
                    let library = self.world.resource::<SkeletonLibrary>();
                    let fresh: Vec<Arc<SkeletonData>> = names.iter().map(|n| library.loaded[n].clone()).collect();
                    self.world.query::<&mut Skeleton>().for_each(|s| {
                        let name = s.data().name.clone();
                        if let Some(new) = fresh.iter().find(|d| d.name.is_some() && d.name == name)
                            && !Arc::ptr_eq(new, s.data())
                        {
                            s.rebind(new.clone());
                        }
                    });
                }
                Ok(_) => {}
                Err(e) => eprintln!("ponko: reloading a skeleton failed: {e}"),
            }
        }
        let dt = self.time.dt();
        self.world.query::<&mut Skeleton>().par_for_each(|s| {
            if !s.manual {
                s.update(dt);
            }
        });
    }

    /// One frame of 2D skeletons without a GPU, for `ponko_bench`: advances
    /// every skeleton by `dt` (on all cores), draws them as the sprite pass
    /// does, orders and batches the draw list as the renderer does, and
    /// forgets it. Returns the vertices and batches. Unstable
    /// (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_skeleton2d_frame(&mut self, dt: f32) -> (usize, usize) {
        self.world.query::<&mut Skeleton>().par_for_each(|s| s.update(dt));
        super::draw_skeletons(self);
        let (vertices, batches) = self.gfx.draw_list.finish();
        let counts = (vertices.len(), batches.len());
        self.gfx.draw_list.clear();
        counts
    }
}

/// The world's skeleton library, made if it has none.
pub(crate) fn library(world: &mut ponko_ecs::World) -> &mut SkeletonLibrary {
    if world.get_resource::<SkeletonLibrary>().is_none() {
        world.insert_resource(SkeletonLibrary::default());
    }
    world.resource_mut::<SkeletonLibrary>()
}
