//! Asset hot reload: textures and data files loaded from files are reloaded
//! when the file changes (textures under the same handle), so edits in an
//! image or text editor show up in the running game.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use web_time::Instant;

use crate::texture::Texture;

/// Environment variable that turns hot reload on (`1`) or off (`0`),
/// overriding [`Game::hot_reload`](crate::game::Game::hot_reload).
pub(crate) const ENV_HOT_RELOAD: &str = "PONKO_HOT_RELOAD";

/// How often watched files are checked.
const CHECK_EVERY: Duration = Duration::from_millis(250);

/// What a file looked like when it was last loaded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// What a watched file was loaded into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Asset {
    Texture(Texture),
    /// A font id.
    Font(u32),
    /// A sound id.
    Sound(u32),
    /// Index into `Ctx`'s data loaders.
    Data(usize),
    /// A sprite shader id.
    SpriteShader(u32),
    /// A 3D material shader id.
    #[cfg(feature = "3d")]
    Shader(u32),
    /// A 3D screen effect id.
    #[cfg(feature = "3d")]
    Effect(u32),
    /// A 3D compute pass id.
    #[cfg(feature = "3d")]
    Compute(u32),
    /// A 3D sky shader id.
    #[cfg(feature = "3d")]
    SkyShader(u32),
    /// A file of a level (`Ctx::spawn_level`); the watched name is the level's path.
    Level,
    /// A mixer file (`Audio::load_mixer`), applied again whole.
    Mixer,
}

struct Watched {
    /// The name the game loaded it by, for messages.
    name: String,
    path: PathBuf,
    stamp: Option<Stamp>,
    asset: Asset,
}

/// Files the game loaded, and which of them changed.
pub(crate) struct Watcher {
    pub(crate) on: bool,
    files: Vec<Watched>,
    next_check: Instant,
}

impl Default for Watcher {
    fn default() -> Self {
        Watcher {
            on: false,
            files: Vec::new(),
            next_check: Instant::now(),
        }
    }
}

/// The [`Watcher`] of a game, shared by every service that loads files:
/// each registers what it loaded, and `ctx.assets` owns the reloading.
#[derive(Clone, Default)]
pub(crate) struct SharedWatcher(std::rc::Rc<std::cell::RefCell<Watcher>>);

impl SharedWatcher {
    /// Turns watching on or off; files loaded while it was off are not watched.
    pub(crate) fn set_on(&self, on: bool) {
        self.0.borrow_mut().on = on;
    }

    pub(crate) fn on(&self) -> bool {
        self.0.borrow().on
    }

    /// As [`Watcher::watched`].
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.0.borrow().watched()
    }

    /// As [`Watcher::watch`].
    pub(crate) fn watch(&self, name: &str, path: PathBuf, asset: Asset) {
        self.0.borrow_mut().watch(name, path, asset);
    }

    /// Whether `path` is watched for an asset `matches` picks.
    pub(crate) fn watching(&self, path: &Path, matches: impl Fn(&Asset) -> bool) -> bool {
        self.0
            .borrow()
            .files
            .iter()
            .any(|f| f.path == path && matches(&f.asset))
    }

    /// As [`Watcher::forget`].
    pub(crate) fn forget(&self, matches: impl Fn(&Asset) -> bool) {
        self.0.borrow_mut().forget(matches);
    }

    /// As [`Watcher::changed`].
    pub(crate) fn changed(&self, now: Instant, out: &mut Vec<(Asset, PathBuf, String)>) {
        self.0.borrow_mut().changed(now, out);
    }
}

/// `on` unless `PONKO_HOT_RELOAD` says otherwise.
pub(crate) fn enabled(on: bool) -> bool {
    // Web builds have no files to watch.
    if cfg!(target_arch = "wasm32") {
        return false;
    }
    match std::env::var(ENV_HOT_RELOAD).as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => on,
    }
}

impl Watcher {
    /// How many files are watched.
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.files.len()
    }

    /// Remembers that `asset` came from `path`.
    pub(crate) fn watch(&mut self, name: &str, path: PathBuf, asset: Asset) {
        if !self.on {
            return;
        }
        let stamp = Stamp::of(&path);
        self.files.push(Watched {
            name: name.to_string(),
            path,
            stamp,
            asset,
        });
    }

    /// Stops watching the files of assets `matches` picks (freed ones).
    pub(crate) fn forget(&mut self, matches: impl Fn(&Asset) -> bool) {
        self.files.retain(|f| !matches(&f.asset));
    }

    /// Files that changed since they were loaded (or last checked), at most
    /// every [`CHECK_EVERY`]. Each comes back once per change.
    pub(crate) fn changed(&mut self, now: Instant, out: &mut Vec<(Asset, PathBuf, String)>) {
        out.clear();
        if !self.on || self.files.is_empty() || now < self.next_check {
            return;
        }
        self.next_check = now + CHECK_EVERY;
        for f in &mut self.files {
            let stamp = Stamp::of(&f.path);
            // A file that's gone (mid-save, or deleted) keeps its old texture.
            if stamp.is_some() && stamp != f.stamp {
                f.stamp = stamp;
                out.push((f.asset, f.path.clone(), f.name.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ponko-hot-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn changes_are_reported_once_and_not_before_the_next_check() {
        let path = temp_file("a.png", b"one");
        let mut w = Watcher {
            on: true,
            ..Default::default()
        };
        let tex = Texture::new(7, 2, 2);
        w.watch("a.png", path.clone(), Asset::Texture(tex));
        let mut out = Vec::new();
        let t0 = Instant::now();
        w.changed(t0, &mut out);
        assert!(out.is_empty(), "nothing changed yet");

        std::fs::write(&path, b"three").unwrap(); // a different length is a change even within one mtime tick
        w.changed(t0 + Duration::from_millis(10), &mut out);
        assert!(out.is_empty(), "too soon after the last check");
        w.changed(t0 + CHECK_EVERY * 2, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, Asset::Texture(tex));
        assert_eq!(out[0].2, "a.png");
        w.changed(t0 + CHECK_EVERY * 4, &mut out);
        assert!(out.is_empty(), "reported once");
    }

    #[test]
    fn missing_files_keep_their_texture() {
        let path = temp_file("b.png", b"one");
        let mut w = Watcher {
            on: true,
            ..Default::default()
        };
        w.watch("b.png", path.clone(), Asset::Texture(Texture::new(1, 1, 1)));
        std::fs::remove_file(&path).unwrap();
        let mut out = Vec::new();
        w.changed(Instant::now() + CHECK_EVERY, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn nothing_is_watched_while_off() {
        let path = temp_file("c.png", b"one");
        let mut w = Watcher::default();
        w.watch("c.png", path, Asset::Data(0));
        assert!(w.files.is_empty());
    }
}
