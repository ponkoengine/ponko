//! Aseprite sprite atlases with variable-time animations.
//!
//! Load one with [`Graphics::try_load_aseprite`]. Composited and split-layer exports
//! use the same atlas and animation APIs.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use glam::IVec2;
use ponko_ecs::Component;

use crate::assets::{Image, Origin};
use crate::camera::Rect;
use crate::ctx::Ctx;
use crate::ecs::Sprite;
use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;
use crate::hot_reload::Asset;
use crate::sheet::SpriteSheet;
use crate::texture::Texture;

mod export;

#[derive(Debug)]
struct Clip {
    /// Cumulative end times in milliseconds and their frame indices.
    frames: Vec<(u64, u32)>,
    duration: u64,
    /// Elapsed milliseconds and the frame to hold after finite playback.
    finish: Option<(f64, u32)>,
}

impl Clip {
    fn frame_at(&self, seconds: f64) -> Option<u32> {
        let elapsed = seconds.max(0.0) * 1000.0;
        let duration = self.duration as f64;
        if let Some((end, frame)) = self.finish
            && elapsed >= end
        {
            return Some(frame);
        }
        let phase = elapsed % duration;
        let i = self.frames.partition_point(|&(end, _)| end as f64 <= phase);
        self.frames.get(i).map(|&(_, frame)| frame)
    }
}

/// One named slice key in an Aseprite canvas. A key stays active until the
/// next key of the same slice.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct AsepriteSlice {
    /// First timeline frame that uses this key (zero-based).
    pub frame: u32,
    /// Rectangle in source-canvas pixels.
    pub bounds: Rect,
    /// Nine-slice center, relative to the top-left of `bounds`.
    pub center: Option<Rect>,
    /// Pivot in pixels relative to the top-left of `bounds`.
    pub pivot: Option<IVec2>,
}

#[derive(Debug)]
struct Atlas {
    sheet: SpriteSheet,
    durations: Vec<u32>,
    layers: Vec<String>,
    clips: BTreeMap<String, Clip>,
    slices: BTreeMap<String, Vec<AsepriteSlice>>,
}

impl Atlas {
    fn layer_index(&self, layer: Option<&str>) -> Option<usize> {
        match layer {
            None if self.layers.is_empty() => Some(0),
            Some(name) => self.layers.iter().position(|l| l == name),
            None => None,
        }
    }

    fn sprite(&self, frame: u32, layer: Option<&str>) -> Option<Sprite> {
        let count = self.durations.len();
        let frame = frame as usize;
        if frame >= count {
            return None;
        }
        let slot = self.layer_index(layer)? * count + frame;
        Some(self.sheet.sprite(slot as u32))
    }
}

/// Shared handle to an Aseprite sprite atlas and its timeline. Clones
/// see successful hot reloads of the export and its sheet.
#[derive(Clone, Debug)]
pub struct Aseprite(Arc<RwLock<Atlas>>);

impl Aseprite {
    fn atlas(&self) -> RwLockReadGuard<'_, Atlas> {
        self.0.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Number of timeline frames, shared by all split layers.
    #[must_use]
    pub fn frame_count(&self) -> u32 {
        self.atlas().durations.len() as u32
    }

    /// Names of split layers in bottom-to-top order, or empty for a
    /// composited export.
    #[must_use]
    pub fn layers(&self) -> Vec<String> {
        self.atlas().layers.clone()
    }

    /// Authored duration of a timeline frame, in milliseconds.
    #[must_use]
    pub fn frame_duration(&self, frame: u32) -> Option<u32> {
        self.atlas().durations.get(frame as usize).copied()
    }

    /// The sprite for a zero-based frame. Use `None` for a composite
    /// export or a layer name for split exports. Missing cels within a
    /// split layer are transparent. Returns `None` for unknown frames or
    /// layers. The sprite is a snapshot: reselect it after an atlas
    /// layout change, or use [`AsepriteAnimation`] for live playback.
    #[must_use]
    pub fn sprite(&self, frame: u32, layer: Option<&str>) -> Option<Sprite> {
        self.atlas().sprite(frame, layer)
    }

    /// The active key of a named slice at `frame`, in canvas coordinates.
    #[must_use]
    pub fn slice(&self, name: &str, frame: u32) -> Option<AsepriteSlice> {
        let atlas = self.atlas();
        atlas.durations.get(frame as usize)?;
        let keys = atlas.slices.get(name)?;
        let i = keys.partition_point(|key| key.frame <= frame);
        keys.get(i.checked_sub(1)?).copied()
    }

    /// Play a named frame tag on the composite or one named split layer.
    /// Use `""` as the tag to loop the entire timeline in forward order.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] when the requested tag or layer is absent.
    pub fn animation(&self, tag: &str, layer: Option<&str>) -> Result<AsepriteAnimation, Error> {
        let atlas = self.atlas();
        if !atlas.clips.contains_key(tag) {
            return Err(Error::new(ErrorKind::NotFound, format!("no Aseprite tag {tag:?}")));
        }
        if atlas.layer_index(layer).is_none() {
            return Err(Error::new(ErrorKind::NotFound, format!("no Aseprite layer {layer:?}")));
        }
        Ok(AsepriteAnimation {
            atlas: self.clone(),
            tag: tag.to_owned(),
            layer: layer.map(str::to_owned),
            time: 0.0,
        })
    }
}

/// Variable-duration sprite playback. Spawn it alongside a
/// [`Sprite`] and a 2D transform. The built-in
/// [`crate::systems::animate_sprites`] system advances playback and
/// refreshes the sprite's texture and region on each frame.
#[derive(Clone, Debug, Component)]
pub struct AsepriteAnimation {
    atlas: Aseprite,
    tag: String,
    layer: Option<String>,
    /// Elapsed seconds. Set this to seek or restart playback.
    pub time: f64,
}

impl AsepriteAnimation {
    /// Sample playback at `seconds`, including direction and repeat count.
    /// Returns `None` if a hot reload removed the tag or layer.
    #[must_use]
    pub fn sprite_at(&self, seconds: f64) -> Option<Sprite> {
        let atlas = self.atlas.atlas();
        let frame = atlas.clips.get(&self.tag)?.frame_at(seconds)?;
        atlas.sprite(frame, self.layer.as_deref())
    }
}

pub(crate) fn animate(ctx: &mut Ctx, dt: f32) {
    if ctx.gfx.aseprite.is_empty() {
        return;
    }
    ctx.world
        .query::<(&mut AsepriteAnimation, &mut Sprite)>()
        .par_for_each(|(animation, sprite)| {
            animation.time += f64::from(dt);
            if let Some(frame) = animation.sprite_at(animation.time) {
                sprite.texture = frame.texture;
                sprite.region = frame.region;
            } else {
                sprite.region = Some(Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        });
}

impl Graphics {
    /// Loads an Aseprite JSON Array export and packs its PNG frames into one
    /// atlas. With `split_layers`, export using `--split-layers` and
    /// `--filename-format '{frame}:{layer}'`. Otherwise the export is
    /// treated as one composite layer. Enable `aseprite-source` to read
    /// loose `.aseprite` or `.ase` files directly in development. The same
    /// source path resolves to its sibling `.json` after cooking.
    ///
    /// # Panics
    ///
    /// If [`Self::try_load_aseprite`] returns an error. Use that call for
    /// user-supplied files or optional assets.
    #[track_caller]
    pub fn load_aseprite(&mut self, path: &str, split_layers: bool) -> Aseprite {
        self.try_load_aseprite(path, split_layers)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Fallible [`Self::load_aseprite`]. The image is relative to the JSON
    /// and both files are read through the same asset sources as other
    /// assets. Each path and split mode is cached for the game's lifetime.
    /// With hot reload on, edits to the loose source or to either export
    /// file refresh the shared atlas. A failed reload keeps the last
    /// successful atlas.
    ///
    /// # Errors
    ///
    /// An asset error if the source or export is missing, malformed,
    /// unsupported, or too large for the atlas.
    pub fn try_load_aseprite(&mut self, path: &str, split_layers: bool) -> Result<Aseprite, Error> {
        let key = (path.to_owned(), split_layers);
        if let Some(asset) = self.aseprite.get(&key) {
            return Ok(asset.clone());
        }
        let atlas = self.read_aseprite(path, split_layers, None)?;
        let asset = Aseprite(Arc::new(RwLock::new(atlas)));
        self.aseprite.insert(key, asset.clone());
        Ok(asset)
    }

    pub(crate) fn reload_aseprite(&mut self, path: &str, split_layers: bool) -> Result<Option<Texture>, Error> {
        let Some(asset) = self.aseprite.get(&(path.to_owned(), split_layers)).cloned() else {
            return Ok(None);
        };
        let old = asset.atlas().sheet.texture;
        let atlas = self.read_aseprite(path, split_layers, Some(old))?;
        let texture = atlas.sheet.texture;
        *asset.0.write().unwrap_or_else(std::sync::PoisonError::into_inner) = atlas;
        if old == texture {
            Ok(Some(texture))
        } else {
            self.free(old);
            Ok(None)
        }
    }

    fn read_aseprite(&mut self, path: &str, split_layers: bool, old: Option<Texture>) -> Result<Atlas, Error> {
        let max = if self.headless {
            8192
        } else {
            self.renderer().max_texture_size()
        };
        let (export, image, origins) = self.read_aseprite_data(path, split_layers, max)?;
        let packed = export
            .pack(&image, split_layers, max)
            .map_err(|err| err.with_path(path))?;
        let w = packed.size.x;
        let h = packed.size.y;
        let texture = match old {
            Some(texture) if (texture.width(), texture.height()) == (w, h) => {
                self.update_texture(texture, &packed.rgba);
                texture
            }
            _ => self.create_texture(w, h, &packed.rgba),
        };
        let kind = Asset::Aseprite(split_layers);
        self.watcher.forget_named(path, kind);
        for origin in origins {
            if let Origin::File(file) = origin {
                self.watcher.watch(path, file, kind);
            }
        }
        Ok(Atlas {
            sheet: SpriteSheet::new(texture, packed.canvas.x, packed.canvas.y),
            durations: packed.durations,
            layers: packed.layers,
            clips: packed.clips,
            slices: packed.slices,
        })
    }

    fn read_aseprite_data(
        &self,
        path: &str,
        #[cfg_attr(
            not(feature = "aseprite-source"),
            expect(unused_variables, reason = "for loose sources")
        )]
        split_layers: bool,
        #[cfg_attr(
            not(feature = "aseprite-source"),
            expect(unused_variables, reason = "for loose sources")
        )]
        max: u32,
    ) -> Result<(export::Export, Image, Vec<Origin>), Error> {
        let Some((stem, _)) = path
            .rsplit_once('.')
            .filter(|(_, ext)| ext.eq_ignore_ascii_case("aseprite") || ext.eq_ignore_ascii_case("ase"))
        else {
            return self.read_aseprite_export(path);
        };
        // A loose source file in development; anything else (a pack, a
        // cooked build) reads the export the cook wrote beside it.
        #[cfg(feature = "aseprite-source")]
        match self.asset_sources.find(path) {
            Ok(found) if matches!(found.origin, Origin::File(_)) => {
                let mut origins = vec![found.origin.clone()];
                let bytes = found.into_bytes().map_err(|error| error.with_path(path))?;
                // A tileset linked to another file reads it through the
                // asset sources, and that file is watched as well.
                let sheet = ponko_aseprite::decode_linked(path, &bytes, split_layers, max, &mut |linked| {
                    let found = self.asset_sources.find(linked).map_err(|error| error.to_string())?;
                    origins.push(found.origin.clone());
                    found.into_bytes().map_err(|error| error.with_path(linked).to_string())
                })
                .map_err(|error| Error::decode(path, error))?;
                let export =
                    serde_json::from_value(sheet.export).map_err(|error| Error::decode(path, error.to_string()))?;
                let image = Image {
                    width: sheet.width,
                    height: sheet.height,
                    rgba: sheet.rgba,
                };
                return Ok((export, image, origins));
            }
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let json = format!("{stem}.json");
        self.read_aseprite_export(&json).map_err(|error| {
            if error.kind() != ErrorKind::NotFound || error.path() != Some(json.as_str()) {
                return error;
            }
            let how = if cfg!(feature = "aseprite-source") {
                "cook the assets (`ponko cook`) or keep the source file loose in `assets/`"
            } else {
                "cook the assets (`ponko cook`) or turn on ponko's `aseprite-source` feature to read it directly"
            };
            error.context(format_args!(
                "{path} is an Aseprite source and {json} isn't beside it: {how}"
            ))
        })
    }

    /// Reads a JSON Array export and its sheet.
    fn read_aseprite_export(&self, json_path: &str) -> Result<(export::Export, Image, Vec<Origin>), Error> {
        let found = self.asset_sources.find(json_path)?;
        let source = found.origin.clone();
        let input = found.into_text(json_path)?;
        let export = export::Export::parse(&input, json_path)?;
        let image_path = crate::tilemap::resolve_relative_asset(json_path, &export.meta.image);
        let found = self.asset_sources.find(&image_path)?;
        let image_source = found.origin.clone();
        let image_bytes = found.into_bytes().map_err(|err| err.with_path(&image_path))?;
        let image = self.asset_sources.image_limits().scope(&image_source, || {
            crate::assets::decode_image(&image_bytes, &image_path, &image_path)
        })?;
        Ok((export, image, vec![source, image_source]))
    }
}
