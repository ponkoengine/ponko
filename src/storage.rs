//! [`Storage`] (`ctx.storage`): the player's own files, such as saves and
//! settings, kept in the per-user data folder on the desktop and in the
//! browser's storage on the web.
//!
//! **Maturity: proven**, from the guide chapter `storage.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows (their saves and progress files, from the
//! package). Tests: `tests/storage.rs`. Platforms: Linux, macOS and Windows, headless (the
//! workspace tests in the list of checks). Graphics: not involved. Missing: browser storage is not
//! part of a recorded browser run.
//!
//! The folder comes from the game's app id ([`Game::app_id`](crate::game::Game::app_id)):
//!
//! | System | Folder |
//! |---|---|
//! | Windows | `%APPDATA%\<app id>` |
//! | macOS | `~/Library/Application Support/<app id>` |
//! | iOS | `Library/Application Support/<app id>` in the app's own container (backed up with the device) |
//! | Linux and other Unix | `$XDG_DATA_HOME/<app id>`, else `~/.local/share/<app id>` |
//! | Web | The page's IndexedDB database `ponko:<app id>` (see below) |
//!
//! `PONKO_DATA_DIR` replaces the system's base folder (the app id is still
//! added), and [`Game::data_dir`](crate::game::Game::data_dir) names the exact
//! folder (tests, portable installs).
//!
//! # Safe writes
//!
//! [`write`](Storage::write) never leaves a half-written file under the
//! name: the bytes go to `<name>.tmp` first and are flushed to the disk,
//! the old file is kept as `<name>.bak`, and only then does a rename put the
//! new file in its place. A crash or power cut at any point leaves either
//! the old file or the new one, and [`read_backup`](Storage::read_backup)
//! has the one before. Names ending in `.tmp` or `.bak` are reserved for this.
//!
//! # The web
//!
//! Browser storage can only be reached asynchronously, so the web build reads
//! every stored file into memory while it loads the page's assets, before
//! setup runs. After that the same calls work the same way: reads and lists
//! answer from memory, and each write or delete changes memory at once and is
//! stored in one IndexedDB transaction (the file and its backup together, so
//! a closed tab never leaves half of it). Browsers may clear a site's storage
//! when space runs low; [`is_persistent`](Storage::is_persistent) is `false`
//! when the page couldn't open its database (some private windows), and then
//! files last only until the page closes.
//!
//! # Large worlds
//!
//! [`save_world_in_background`](Storage::save_world_in_background) takes the
//! world's save on the game's thread and encodes and writes it on a
//! [`Job`] thread, and [`set_save_format`](Storage::set_save_format) with
//! [`SaveFormat::Binary`] writes slots in a compact binary encoding of the
//! same save. Loading reads either format.
#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::error::{Error, ErrorKind};
use crate::reflect::SaveFormat;
use crate::streaming::Job;

/// The environment variable that replaces the system's data folder (the
/// app id is still added below it).
pub const ENV_DATA_DIR: &str = "PONKO_DATA_DIR";

/// Suffix of the file a write goes to before it is renamed into place.
const TEMP: &str = ".tmp";
/// Suffix of the previous version of a file.
const BACKUP: &str = ".bak";
/// Where [`Storage::save_world`] keeps its slots.
const SLOTS: &str = "saves/";

/// The player's own files: saves, settings, anything the game keeps between
/// runs. `ctx.storage`; see the [module](self) for where they live.
///
/// Names are relative, with `/` between folders (`"saves/slot1.json"`); each
/// part uses letters, digits, spaces, `-`, `_` and `.`, and doesn't start
/// with a `.`.
///
/// ```
/// use ponko::prelude::*;
/// # let dir = std::env::temp_dir().join(format!("ponko-doc-storage-{}", std::process::id()));
/// Game::new("Crabs", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
///     ctx.storage.write("settings.json", br#"{"volume":0.5}"#).unwrap();
///     assert_eq!(ctx.storage.read("settings.json").unwrap(), br#"{"volume":0.5}"#);
///     assert_eq!(ctx.storage.list().unwrap(), ["settings.json"]);
/// });
/// # let _ = std::fs::remove_dir_all(dir);
/// ```
#[derive(Debug, Default)]
pub struct Storage {
    backend: Backend,
    /// The format [`save_world`](Self::save_world) writes slots in.
    format: SaveFormat,
    /// Background saves not written yet.
    queue: SaveQueue,
}

#[derive(Debug, Default, Clone)]
enum Backend {
    /// No app id and no folder: every call fails with `Unsupported`.
    #[default]
    Off,
    /// A folder on the disk.
    Folder(PathBuf),
    /// The browser's storage, preloaded into memory.
    #[cfg(target_arch = "wasm32")]
    Browser,
}

/// Whether `app_id` can name a game's folder ([`Game::app_id`](crate::game::Game::app_id)):
/// 1 to 128 letters, digits, `-`, `_` and `.`, not starting with `.`.
///
/// ```
/// assert!(ponko::storage::is_app_id("com.example.crabs"));
/// assert!(!ponko::storage::is_app_id("../crabs"));
/// ```
#[must_use]
pub fn is_app_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && !app_id.starts_with('.')
        && app_id.len() <= 128
        && app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Where a write stops in tests that simulate a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Crash {
    Never,
    /// The new bytes are in `<name>.tmp`; nothing else changed.
    AfterTemp,
    /// The old file is also `<name>.bak`; the rename hasn't happened.
    AfterBackup,
}

impl Storage {
    /// The storage a game gets: `dir` if given ([`Game::data_dir`](crate::game::Game::data_dir)),
    /// else the system's data folder (or `PONKO_DATA_DIR`) with `app_id`
    /// below it, else off.
    pub(crate) fn for_game(app_id: Option<&str>, dir: Option<PathBuf>) -> Self {
        if let Some(dir) = dir {
            return Self::in_folder(dir);
        }
        let Some(app_id) = app_id else {
            return Self::default();
        };
        #[cfg(target_arch = "wasm32")]
        {
            web::open(app_id);
            Self::with_backend(Backend::Browser)
        }
        #[cfg(not(target_arch = "wasm32"))]
        match base_dir(&|name| std::env::var_os(name)) {
            Some(base) => Self::in_folder(base.join(app_id)),
            None => Self::default(),
        }
    }

    /// Storage in the folder `dir`, created at the first write. For tools and
    /// tests that look at a game's files from outside it; a game gets its
    /// own as `ctx.storage`.
    ///
    /// ```
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-in-folder-{}", std::process::id()));
    /// let mut files = ponko::storage::Storage::in_folder(&dir);
    /// files.write("a.txt", b"hello").unwrap();
    /// assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"hello");
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    pub fn in_folder(dir: impl Into<PathBuf>) -> Self {
        Self::with_backend(Backend::Folder(dir.into()))
    }

    /// Another handle to the same files, with its own background-save
    /// queue (crash reports write through one).
    pub(crate) fn reopen(&self) -> Self {
        Self::with_backend(self.backend.clone())
    }

    fn with_backend(backend: Backend) -> Self {
        Self {
            backend,
            format: SaveFormat::default(),
            queue: SaveQueue::default(),
        }
    }

    /// The folder the files are in: `None` on the web, and when the game has
    /// no app id.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-dir-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// assert_eq!(files.dir(), Some(dir.as_path()));
    /// # let _ = &mut files;
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        match &self.backend {
            Backend::Folder(dir) => Some(dir),
            _ => None,
        }
    }

    /// Whether the game can keep files at all: `false` without an app id
    /// ([`Game::app_id`](crate::game::Game::app_id)), or on a desktop with no
    /// home folder. Every call fails with
    /// [`ErrorKind::Unsupported`] then.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-avail-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// assert!(files.is_available());
    /// # let _ = &mut files;
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    #[must_use]
    pub fn is_available(&self) -> bool {
        !matches!(self.backend, Backend::Off)
    }

    /// Whether files outlive the run: `true` for a folder, and on the web
    /// when the page's database opened (some private windows refuse it;
    /// files then last only until the page closes).
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-persist-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// assert!(files.is_persistent()); // a folder outlives the run
    /// # let _ = &mut files;
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    #[must_use]
    pub fn is_persistent(&self) -> bool {
        match self.backend {
            Backend::Off => false,
            Backend::Folder(_) => true,
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::is_persistent(),
        }
    }

    /// Whether writes are still on their way to the disk: while a
    /// [`save_world_in_background`](Self::save_world_in_background) hasn't
    /// written its file yet, and on the web while the browser hasn't
    /// confirmed every stored change (a tab closed then may lose the last
    /// one, never half of it). [`write`](Self::write) itself returns once the
    /// file is there on the desktop. A game can show "saving..." while it is
    /// `true`.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-saving-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// assert!(!files.is_saving()); // nothing is queued
    /// files.write("a.txt", b"x").unwrap();
    /// assert!(!files.is_saving()); // `write` returns once the file is there
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    #[must_use]
    pub fn is_saving(&self) -> bool {
        if self.queue.is_busy() {
            return true;
        }
        match self.backend {
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::is_saving(),
            _ => false,
        }
    }

    /// Writes `bytes` as the file `name`, creating its folders. Safe against
    /// crashes: the old file stays whole until the new one is complete, and
    /// is then kept as the backup ([`read_backup`](Self::read_backup)).
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-write-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write("saves/slot1.bin", b"hello").unwrap();
    /// assert_eq!(std::fs::read(dir.join("saves/slot1.bin")).unwrap(), b"hello");
    /// assert!(files.write("../outside", b"no").is_err()); // names stay inside the folder
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] for a bad name, [`ErrorKind::Unsupported`]
    /// when storage is off, [`ErrorKind::Io`] when the disk refuses (read-only,
    /// full). The file named `name` is unchanged after an error.
    pub fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        self.write_until(name, bytes, Crash::Never)
    }

    fn write_until(&mut self, name: &str, bytes: &[u8], crash: Crash) -> Result<(), Error> {
        check_name(name)?;
        self.queue.wait_for(|n| n == name);
        self.backend.write(name, bytes, crash)
    }

    /// The bytes of the file `name`.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-read-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write("a.txt", b"hello").unwrap();
    /// assert_eq!(files.read("a.txt").unwrap(), b"hello");
    /// assert_eq!(files.read("missing.txt").unwrap_err().kind(), ponko::error::ErrorKind::NotFound);
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] if there is no such file, [`ErrorKind::Io`] if
    /// it can't be read, [`ErrorKind::Invalid`] for a bad name,
    /// [`ErrorKind::Unsupported`] when storage is off.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        check_name(name)?;
        self.queue.wait_for(|n| n == name);
        self.read_raw(name)
    }

    /// The file `name` as it was before its last [`write`](Self::write): for
    /// a game that finds its save damaged (by hand, by another program) and
    /// wants the one before.
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read); [`ErrorKind::NotFound`] if `name` was never
    /// written over.
    pub fn read_backup(&self, name: &str) -> Result<Vec<u8>, Error> {
        check_name(name)?;
        self.queue.wait_for(|n| n == name);
        self.read_raw(&format!("{name}{BACKUP}"))
    }

    fn read_raw(&self, name: &str) -> Result<Vec<u8>, Error> {
        match &self.backend {
            Backend::Off => Err(off(name)),
            Backend::Folder(dir) => std::fs::read(dir.join(name)).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    // Where it looked, so a wrong app id or folder shows.
                    Error::new(
                        ErrorKind::NotFound,
                        format!("no stored file {name:?} in {}", dir.display()),
                    )
                    .with_path(name)
                } else {
                    io(name, &e)
                }
            }),
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::read(name).ok_or_else(|| missing(name)),
        }
    }

    /// Whether the file `name` exists (`false` for a bad name or when
    /// storage is off).
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-exists-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// assert!(!files.exists("a.txt"));
    /// files.write("a.txt", b"x").unwrap();
    /// assert!(files.exists("a.txt"));
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        if check_name(name).is_err() {
            return false;
        }
        self.queue.wait_for(|n| n == name);
        match &self.backend {
            Backend::Off => false,
            Backend::Folder(dir) => dir.join(name).is_file(),
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::read(name).is_some(),
        }
    }

    /// When the file `name` was last written: `None` when it isn't there,
    /// and on the web (the browser keeps no times). For syncing with a
    /// cloud copy ([`Steam::sync_cloud`](crate::steam::Steam::sync_cloud)).
    #[cfg_attr(not(feature = "steam"), allow(dead_code))]
    pub(crate) fn modified(&self, name: &str) -> Option<std::time::SystemTime> {
        check_name(name).ok()?;
        self.queue.wait_for(|n| n == name);
        match &self.backend {
            Backend::Folder(dir) => std::fs::metadata(dir.join(name)).and_then(|m| m.modified()).ok(),
            _ => None,
        }
    }

    /// Every file's name, in sorted order, including those in folders
    /// (`"saves/slot1.json"`). Backups and unfinished writes aren't listed.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-list-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write("b.txt", b"x").unwrap();
    /// files.write("saves/a.txt", b"x").unwrap();
    /// assert_eq!(files.list().unwrap(), ["b.txt", "saves/a.txt"]);
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Io`] if the folder can't be read,
    /// [`ErrorKind::Unsupported`] when storage is off. A folder that doesn't
    /// exist yet has no files.
    pub fn list(&self) -> Result<Vec<String>, Error> {
        self.queue.wait_for(|_| true);
        let mut names = match &self.backend {
            Backend::Off => return Err(off("")),
            Backend::Folder(dir) => {
                let mut names = Vec::new();
                list_folder(dir, "", &mut names).map_err(|e| io("", &e))?;
                names
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::names(),
        };
        names.retain(|n| is_listed(n));
        names.sort();
        Ok(names)
    }

    /// Deletes the file `name` and its backup. A file that isn't there is
    /// already deleted.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-delete-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write("a.txt", b"x").unwrap();
    /// files.delete("a.txt").unwrap();
    /// assert!(!files.exists("a.txt"));
    /// files.delete("a.txt").unwrap(); // already gone is fine
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Io`] if the disk refuses, [`ErrorKind::Invalid`] for a bad
    /// name, [`ErrorKind::Unsupported`] when storage is off.
    pub fn delete(&mut self, name: &str) -> Result<(), Error> {
        check_name(name)?;
        self.queue.wait_for(|n| n == name);
        self.backend.delete(name)
    }

    /// Writes `value` as pretty-printed JSON, safely as [`write`](Self::write).
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-wjson-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write_json("settings.json", &[1, 2, 3]).unwrap();
    /// let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    /// assert!(text.contains('2'));
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// As `write`, and [`ErrorKind::Json`] if `value` can't be written as JSON.
    pub fn write_json<T: serde::Serialize + ?Sized>(&mut self, name: &str, value: &T) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::from(e).with_path(name))?;
        self.write(name, &bytes)
    }

    /// Reads the JSON file `name` as a `T`.
    ///
    /// ```
    /// use ponko::storage::Storage;
    /// let dir = std::env::temp_dir().join(format!("ponko-doc-rjson-{}", std::process::id()));
    /// let mut files = Storage::in_folder(&dir);
    /// files.write_json("settings.json", &vec![1, 2, 3]).unwrap();
    /// let numbers: Vec<i32> = files.read_json("settings.json").unwrap();
    /// assert_eq!(numbers, [1, 2, 3]);
    /// assert!(files.read_json::<String>("settings.json").is_err()); // not a string
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// As [`read`](Self::read), and [`ErrorKind::Json`] if the file isn't JSON
    /// for a `T`.
    pub fn read_json<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<T, Error> {
        let bytes = self.read(name)?;
        serde_json::from_slice(&bytes).map_err(|e| {
            Error::from(e)
                .context(format!("the file doesn't fit `{}`", std::any::type_name::<T>()))
                .context(name)
                .with_path(name)
        })
    }

    /// Saves the world's reflected components and resources (as
    /// [`WorldReflect::save`](crate::reflect::WorldReflect::save) and
    /// [`save_scene`](crate::assets::Assets::save_scene) do) in the save slot
    /// `slot`, the file `saves/<slot>.json`; [`Ctx::load_world`](crate::ctx::Ctx::load_world)
    /// reads it back. Safe against crashes, like [`write`](Self::write).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-slots-{}", std::process::id()));
    /// Game::new("Crabs", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(1.0, 2.0)),));
    ///     ctx.storage.save_world(&ctx.world, "slot1").unwrap();
    ///     assert_eq!(ctx.storage.world_slots().unwrap(), ["slot1"]);
    ///     ctx.load_world("slot1").unwrap();
    /// });
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// As `write`, and [`ErrorKind::Invalid`] for a slot name that isn't one
    /// part of a file name (no `/`).
    pub fn save_world(&mut self, world: &ponko_ecs::World, slot: &str) -> Result<(), Error> {
        let (file, other) = self.slot_files(slot)?;
        let save = crate::reflect::WorldReflect::save(world);
        self.queue.wait_for(|n| n == file || n == other);
        self.backend.write_slot(&file, &other, &self.format.encode(&save))
    }

    /// [`save_world`](Self::save_world), with only the world's save itself
    /// ([`WorldReflect::save`](crate::reflect::WorldReflect::save)) taken on
    /// this thread: encoding it and writing the file happen on a [`Job`]
    /// thread, so a large world doesn't stop the game for the whole save. The
    /// save holds the world as it is at the call; the game can change it
    /// right away.
    ///
    /// Storage behaves as if the save were written at the call: background
    /// saves are written in the order they were made, and a call that touches
    /// a slot still being written ([`read`](Self::read), [`write`](Self::write),
    /// [`Ctx::load_world`](crate::ctx::Ctx::load_world), [`list`](Self::list),
    /// ...) waits for it first. [`is_saving`](Self::is_saving) is `true`
    /// until the file is written, [`finish_saves`](Self::finish_saves) waits
    /// for every one, and so does dropping the storage when the game ends.
    /// The returned job says how the write went (keep it to find out; the
    /// save is written either way). On the web the save is written before
    /// this returns, on the page's thread, which the browser's storage
    /// belongs to (in a build with threads too).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-bg-save-{}", std::process::id()));
    /// Game::new("Crabs", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(1.0, 2.0)),));
    ///     let mut job = ctx.storage.save_world_in_background(&ctx.world, "slot1").unwrap();
    ///     // Each frame, until it's written:
    ///     while !job.is_done() {}
    ///     job.take().unwrap().expect("written");
    ///     ctx.load_world("slot1").unwrap();
    /// });
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// Before anything is saved: [`ErrorKind::Invalid`] for a bad slot
    /// name, [`ErrorKind::Unsupported`] when storage is off. The job's result
    /// has the errors of [`write`](Self::write).
    pub fn save_world_in_background(
        &mut self,
        world: &ponko_ecs::World,
        slot: &str,
    ) -> Result<Job<Result<(), Error>>, Error> {
        let (file, other) = self.slot_files(slot)?;
        if !self.is_available() {
            return Err(off(&file));
        }
        let save = crate::reflect::WorldReflect::save(world);
        let (format, backend) = (self.format, self.backend.clone());
        let ticket = self.queue.ticket(vec![file.clone(), other.clone()]);
        // In a browser the database belongs to the page's thread, so the
        // save runs there, at once, even when the game has threads.
        Ok(Job::spawn_on_web_page(move || {
            let bytes = format.encode(&save);
            // The save is freed here too, not on the game's thread.
            drop(save);
            ticket.wait_turn();
            backend.write_slot(&file, &other, &bytes)
        }))
    }

    /// Waits until every [`save_world_in_background`](Self::save_world_in_background)
    /// is written: before quitting to the desktop, or in tests. Returns at
    /// once when none is on its way.
    pub fn finish_saves(&self) {
        self.queue.wait_for(|_| true);
    }

    /// Writes save slots in `format` from now on: [`SaveFormat::Json`] (the
    /// default) as `saves/<slot>.json`, [`SaveFormat::Binary`] as
    /// `saves/<slot>.save`, much smaller and quicker for large worlds.
    /// Saving a slot removes its file in the other format, so each slot has
    /// one file; [`Ctx::load_world`](crate::ctx::Ctx::load_world) reads
    /// either, so slots saved before the change still load.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::reflect::SaveFormat;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-save-format-{}", std::process::id()));
    /// Game::new("Crabs", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
    ///     ctx.storage.set_save_format(SaveFormat::Binary);
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(1.0, 2.0)),));
    ///     ctx.storage.save_world(&ctx.world, "slot1").unwrap();
    ///     assert!(ctx.storage.exists("saves/slot1.save"));
    ///     ctx.load_world("slot1").unwrap();
    /// });
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    pub fn set_save_format(&mut self, format: SaveFormat) {
        self.format = format;
    }

    /// The format save slots are written in ([`set_save_format`](Self::set_save_format)).
    #[must_use]
    pub fn save_format(&self) -> SaveFormat {
        self.format
    }

    /// Slot `slot`'s file in the current format, and in the other one.
    fn slot_files(&self, slot: &str) -> Result<(String, String), Error> {
        let other = match self.format {
            SaveFormat::Binary => SaveFormat::Json,
            _ => SaveFormat::Binary,
        };
        Ok((slot_file(slot, self.format)?, slot_file(slot, other)?))
    }

    /// The save slots [`save_world`](Self::save_world) wrote (in either
    /// format), sorted.
    ///
    /// # Errors
    ///
    /// As [`list`](Self::list).
    pub fn world_slots(&self) -> Result<Vec<String>, Error> {
        let mut slots: Vec<String> = self
            .list()?
            .into_iter()
            .filter_map(|n| {
                let n = n.strip_prefix(SLOTS)?;
                [SaveFormat::Json, SaveFormat::Binary]
                    .into_iter()
                    .find_map(|f| n.strip_suffix(f.extension())?.strip_suffix('.'))
                    .filter(|n| !n.contains('/'))
                    .map(str::to_string)
            })
            .collect();
        // A slot in both formats (a save cut off between writing one and
        // removing the other) is one slot.
        slots.sort();
        slots.dedup();
        Ok(slots)
    }

    /// Deletes the save slot `slot` (in either format) and its backup.
    ///
    /// # Errors
    ///
    /// As [`delete`](Self::delete).
    pub fn delete_world(&mut self, slot: &str) -> Result<(), Error> {
        let (file, other) = self.slot_files(slot)?;
        self.delete(&file)?;
        self.delete(&other)
    }
}

impl crate::ctx::Ctx {
    /// Replaces the world with the save slot `slot` written by
    /// [`Storage::save_world`]: the same load as
    /// [`load_scene`](Self::load_scene) (its table says what is replaced and
    /// what is kept), from the player's storage instead of the assets.
    ///
    /// A save is this player's own, so loading one is refused while a
    /// lockstep session is connecting or running: the other peers don't have
    /// the file.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] if there is no such slot, [`ErrorKind::Json`]
    /// if it isn't a world save, the error of an asset it names,
    /// [`ErrorKind::Unsupported`] when storage is off or a session runs. The
    /// world is unchanged then.
    pub fn load_world(&mut self, slot: &str) -> Result<(), Error> {
        let (file, other) = self.storage.slot_files(slot)?;
        // The current format's file, else one saved before a format change.
        let name = if !self.storage.exists(&file) && self.storage.exists(&other) {
            other
        } else {
            file
        };
        let which = format!("save slot {slot:?} ({name})");
        let bytes = self.storage.read(&name).map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                no_such_slot(slot, &name, &self.storage)
            } else {
                e
            }
        })?;
        let save = SaveFormat::decode(&bytes).map_err(|e| damaged_slot(&which, &name, &bytes, &e))?;
        self.replace_world(&save).map_err(|e| {
            if e.kind() == ErrorKind::Unsupported {
                Error::new(
                    ErrorKind::Unsupported,
                    "a lockstep session is running: a save slot is this player's own, the other peers can't load it",
                )
                .with_path(&name)
            } else {
                e.context(&which).with_path(&name)
            }
        })
    }
}

/// The error of loading a slot nobody saved: the slots there are, and the
/// call that makes one.
fn no_such_slot(slot: &str, file: &str, storage: &Storage) -> Error {
    let slots = storage.world_slots().unwrap_or_default();
    let there = if slots.is_empty() {
        "no slot has been saved yet; save one with `Storage::save_world`".to_string()
    } else {
        format!("saved slots: {}", slots.join(", "))
    };
    Error::new(
        ErrorKind::NotFound,
        format!("there is no save in slot {slot:?} (looked for {file}); {there}"),
    )
    .with_path(file)
}

/// The error of a slot file that doesn't read as a save: what is wrong with
/// it (`why` is the decoder's) and how to get on.
fn damaged_slot(which: &str, file: &str, bytes: &[u8], why: &Error) -> Error {
    let problem = if bytes.is_empty() {
        "is empty (0 bytes)".to_string()
    } else {
        format!("is damaged or isn't a ponko save: {}", why.message())
    };
    Error::new(
        why.kind(),
        format!(
            "{which} {problem}; restore its previous copy if there is one (`Storage::read_backup({file:?})`) \
             or delete the slot with `Storage::delete_world`"
        ),
    )
    .with_path(file)
}

/// The name of slot `slot`'s file in `format`.
fn slot_file(slot: &str, format: SaveFormat) -> Result<String, Error> {
    if slot.contains('/') {
        return Err(Error::new(ErrorKind::Invalid, format!("save slot {slot:?} has a '/'")).with_path(slot));
    }
    let name = format!("{SLOTS}{slot}.{}", format.extension());
    check_name(&name)?;
    Ok(name)
}

impl Backend {
    fn write(&self, name: &str, bytes: &[u8], crash: Crash) -> Result<(), Error> {
        match self {
            Backend::Off => Err(off(name)),
            Backend::Folder(dir) => write_file(&dir.join(name), bytes, crash).map_err(|e| io(name, &e)),
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => {
                let _ = crash;
                web::write(name, bytes)
            }
        }
    }

    fn delete(&self, name: &str) -> Result<(), Error> {
        match self {
            Backend::Off => Err(off(name)),
            Backend::Folder(dir) => {
                for file in [name.to_string(), format!("{name}{BACKUP}")] {
                    match std::fs::remove_file(dir.join(&file)) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(name, &e)),
                        _ => {}
                    }
                }
                Ok(())
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Browser => web::delete(name),
        }
    }

    /// Writes a save slot's `file`, then removes the slot's file in the
    /// other format (`other`), which is older now: each slot has one file.
    fn write_slot(&self, file: &str, other: &str, bytes: &[u8]) -> Result<(), Error> {
        self.write(file, bytes, Crash::Never)?;
        self.delete(other)
    }
}

/// The background saves of one storage that aren't written yet. Each takes
/// a [`Ticket`] when it is made, and they are written in ticket order, so a
/// later save of a slot never ends up under an earlier one.
#[derive(Debug, Default)]
struct SaveQueue(Arc<Shared>);

/// The queue's state, shared with its tickets' jobs.
type Shared = (Mutex<Queued>, Condvar);

/// Waits for the background saves still being written when the storage
/// goes (the game ends), so none is cut off.
impl Drop for SaveQueue {
    fn drop(&mut self) {
        self.wait_for(|_| true);
    }
}

#[derive(Debug, Default)]
struct Queued {
    /// Tickets handed out.
    issued: u64,
    /// Tickets finished, written or not: the next one to write.
    finished: u64,
    /// The files each unfinished ticket writes or removes.
    files: Vec<(u64, Vec<String>)>,
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Queued> {
    shared.0.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SaveQueue {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queued> {
        lock(&self.0)
    }

    /// A ticket for a save that writes or removes `files`.
    fn ticket(&self, files: Vec<String>) -> Ticket {
        let mut q = self.lock();
        let number = q.issued;
        q.issued += 1;
        q.files.push((number, files));
        Ticket {
            shared: Arc::clone(&self.0),
            number,
        }
    }

    fn is_busy(&self) -> bool {
        let q = self.lock();
        q.finished < q.issued
    }

    /// Blocks until no unfinished save has a file that `touches`. Returns at
    /// once when none has (always on the web, where saves finish before
    /// they return, so this never waits there).
    fn wait_for(&self, touches: impl Fn(&str) -> bool) {
        let mut q = self.lock();
        while q.files.iter().any(|(_, files)| files.iter().any(|f| touches(f))) {
            q = self.0.1.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// One background save's place in its [`SaveQueue`]. Dropping it (after
/// the write, or when the work panics) finishes it, in order: it waits for
/// the tickets before it first.
struct Ticket {
    shared: Arc<Shared>,
    number: u64,
}

impl Ticket {
    /// Blocks until every earlier ticket is finished.
    fn wait_turn(&self) {
        let mut q = lock(&self.shared);
        while q.finished < self.number {
            q = self.shared.1.wait(q).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.wait_turn();
        let mut q = lock(&self.shared);
        q.finished = self.number + 1;
        q.files.retain(|(n, _)| *n != self.number);
        drop(q);
        self.shared.1.notify_all();
    }
}

/// Checks a file name: relative parts with safe characters, no reserved
/// suffix.
pub(crate) fn check_name(name: &str) -> Result<(), Error> {
    let bad = |why: &str| Err(Error::new(ErrorKind::Invalid, format!("file name {name:?} {why}")).with_path(name));
    if name.is_empty() {
        return bad("is empty");
    }
    if name.len() > 255 {
        return bad("is longer than 255 bytes");
    }
    for part in name.split('/') {
        if part.is_empty() {
            return bad("has an empty part (a leading, trailing or double '/')");
        }
        if part.starts_with('.') {
            return bad("has a part starting with '.'");
        }
        if part.ends_with(' ') {
            return bad("has a part ending with a space");
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if device {
            return bad("has a part Windows keeps for devices (CON, NUL, COM1, ...)");
        }
        if let Some(c) = part
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')))
        {
            return bad(&format!(
                "has {c:?}: use letters, digits, spaces, '-', '_', '.' and '/'"
            ));
        }
    }
    if name.ends_with(TEMP) || name.ends_with(BACKUP) {
        return bad("ends with .tmp or .bak, which storage keeps for itself");
    }
    Ok(())
}

/// A name `list` shows: not a backup or an unfinished write.
fn is_listed(name: &str) -> bool {
    !name.ends_with(TEMP) && !name.ends_with(BACKUP)
}

fn off(name: &str) -> Error {
    let e = Error::new(
        ErrorKind::Unsupported,
        "this game keeps no files: give it an app id with Game::app_id (or a folder with Game::data_dir)",
    );
    if name.is_empty() { e } else { e.with_path(name) }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn missing(name: &str) -> Error {
    Error::new(ErrorKind::NotFound, format!("no stored file {name:?}")).with_path(name)
}

fn io(name: &str, e: &std::io::Error) -> Error {
    let kind = if e.kind() == std::io::ErrorKind::NotFound {
        ErrorKind::NotFound
    } else {
        ErrorKind::Io
    };
    let what = if name.is_empty() {
        "the storage folder".to_string()
    } else {
        format!("stored file {name:?}")
    };
    let e = Error::new(kind, format!("{what}: {e}"));
    if name.is_empty() { e } else { e.with_path(name) }
}

/// The system's per-user data folder (before the app id), read through `var`
/// so tests can give their own environment.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn base_dir(var: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let var = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var(ENV_DATA_DIR) {
        return Some(dir);
    }
    if cfg!(windows) {
        var("APPDATA").or_else(|| var("USERPROFILE").map(|h| h.join("AppData").join("Roaming")))
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        // On iOS, HOME is the app's own container.
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        // XDG says a relative value is invalid and must be ignored.
        var("XDG_DATA_HOME")
            .filter(|d| d.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    }
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The safe write: temporary file (flushed), backup of the old file, rename.
fn write_file(path: &Path, bytes: &[u8], crash: Crash) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let temp = with_suffix(path, TEMP);
    {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if crash == Crash::AfterTemp {
        return Err(std::io::Error::other("simulated crash after the temporary file"));
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        keep_backup(path)?;
    }
    if crash == Crash::AfterBackup {
        return Err(std::io::Error::other("simulated crash before the rename"));
    }
    // Atomic on every desktop system: the name is the old file or the new one.
    // (On Windows, std's rename replaces an existing file.)
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    sync_dir(dir);
    Ok(())
}

/// Makes `<path>.bak` the current `path`: a second name for the same file
/// where the file system has them (the rename then only moves `path`), else a
/// copy, made whole before it gets the backup's name.
fn keep_backup(path: &Path) -> std::io::Result<()> {
    let backup = with_suffix(path, BACKUP);
    match std::fs::remove_file(&backup) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    if std::fs::hard_link(path, &backup).is_ok() {
        return Ok(());
    }
    let temp = with_suffix(&backup, TEMP);
    std::fs::copy(path, &temp)?;
    std::fs::rename(&temp, &backup)
}

/// Flushes the folder's entries, so the rename itself survives a power cut
/// (Unix; Windows has no such call for folders). Best effort.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Adds the files below `dir` to `names`, as `prefix<name>`.
fn list_folder(dir: &Path, prefix: &str, names: &mut Vec<String>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && prefix.is_empty() => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        // Names storage couldn't have written (not UTF-8) aren't its files.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            list_folder(&entry.path(), &format!("{prefix}{name}/"), names)?;
        } else if file_type.is_file() {
            names.push(format!("{prefix}{name}"));
        }
    }
    Ok(())
}

/// Files kept in memory with the same rules as the folder: a write keeps the
/// old bytes as the backup. The web build's copy of its browser storage.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct MemoryFiles {
    files: std::collections::BTreeMap<String, Vec<u8>>,
}

/// One change to store: a file's new bytes, or its removal.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    Put(String, Vec<u8>),
    Remove(String),
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl MemoryFiles {
    pub(crate) fn insert(&mut self, name: String, bytes: Vec<u8>) {
        self.files.insert(name, bytes);
    }

    pub(crate) fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(name).cloned()
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    /// Writes `name`, keeping the old bytes as its backup; returns the
    /// changes to store, all in one transaction.
    pub(crate) fn write(&mut self, name: &str, bytes: &[u8]) -> Vec<Change> {
        let mut changes = Vec::with_capacity(2);
        if let Some(old) = self.files.insert(name.to_string(), bytes.to_vec()) {
            let backup = format!("{name}{BACKUP}");
            changes.push(Change::Put(backup.clone(), old.clone()));
            self.files.insert(backup, old);
        }
        changes.push(Change::Put(name.to_string(), bytes.to_vec()));
        changes
    }

    /// Removes `name` and its backup; returns the changes to store.
    pub(crate) fn delete(&mut self, name: &str) -> Vec<Change> {
        let backup = format!("{name}{BACKUP}");
        self.files.remove(name);
        self.files.remove(&backup);
        vec![Change::Remove(name.to_string()), Change::Remove(backup)]
    }
}

#[cfg(target_arch = "wasm32")]
mod web;

/// Reads the game's browser storage into memory (web builds, before setup).
#[cfg(target_arch = "wasm32")]
pub(crate) async fn preload_web() {
    web::preload().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ponko-storage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_crash_between_the_temporary_file_and_the_rename_never_leaves_a_corrupt_save() {
        for crash in [Crash::AfterTemp, Crash::AfterBackup] {
            let dir = temp_dir(&format!("crash-{crash:?}"));
            let mut storage = Storage::in_folder(&dir);
            storage.write("saves/slot.json", b"{\"level\":1}").unwrap();

            // The game dies while writing level 2.
            let err = storage
                .write_until("saves/slot.json", b"{\"level\":2}", crash)
                .unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Io, "{crash:?}");

            // A new run (a new process, in effect) finds the whole old save.
            let storage = Storage::in_folder(&dir);
            assert_eq!(storage.read("saves/slot.json").unwrap(), b"{\"level\":1}", "{crash:?}");
            assert_eq!(
                storage.list().unwrap(),
                ["saves/slot.json"],
                "the leftover isn't listed"
            );

            // And the next write goes through, keeping level 1 as the backup.
            let mut storage = storage;
            storage.write("saves/slot.json", b"{\"level\":3}").unwrap();
            assert_eq!(storage.read("saves/slot.json").unwrap(), b"{\"level\":3}");
            assert_eq!(storage.read_backup("saves/slot.json").unwrap(), b"{\"level\":1}");
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_half_written_temporary_file_from_a_killed_process_is_ignored_and_replaced() {
        let dir = temp_dir("torn");
        let mut storage = Storage::in_folder(&dir);
        storage.write("progress.json", b"complete").unwrap();
        // What a process killed in the middle of writing the bytes leaves.
        std::fs::write(dir.join("progress.json.tmp"), b"comp").unwrap();
        assert_eq!(storage.read("progress.json").unwrap(), b"complete");
        assert_eq!(storage.list().unwrap(), ["progress.json"]);
        storage.write("progress.json", b"newer").unwrap();
        assert_eq!(storage.read("progress.json").unwrap(), b"newer");
        assert!(!dir.join("progress.json.tmp").exists(), "the rename used it up");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_background_save_that_panics_still_lets_the_saves_after_it_through() {
        let queue = SaveQueue::default();
        let first = queue.ticket(vec!["saves/a.save".into()]);
        let second = queue.ticket(vec!["saves/a.save".into()]);
        assert!(queue.is_busy());
        let later = std::thread::spawn(move || {
            second.wait_turn();
            drop(second);
        });
        let panicked = std::thread::spawn(move || {
            let _first = first;
            panic!("encoding failed");
        })
        .join();
        assert!(panicked.is_err());
        later.join().unwrap();
        assert!(!queue.is_busy());
        queue.wait_for(|_| true); // returns: nothing left
    }

    #[test]
    fn the_first_write_has_no_backup_and_each_later_one_keeps_the_previous_file() {
        let dir = temp_dir("backup");
        let mut storage = Storage::in_folder(&dir);
        storage.write("a.txt", b"1").unwrap();
        assert_eq!(storage.read_backup("a.txt").unwrap_err().kind(), ErrorKind::NotFound);
        storage.write("a.txt", b"2").unwrap();
        storage.write("a.txt", b"3").unwrap();
        assert_eq!(storage.read("a.txt").unwrap(), b"3");
        assert_eq!(storage.read_backup("a.txt").unwrap(), b"2");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unsafe_and_reserved_names_are_refused() {
        for name in [
            "",
            "/abs",
            "a//b",
            "../up",
            "a/../b",
            ".hidden",
            "back\\slash",
            "c:drive",
            "x.tmp",
            "x.bak",
            "trailing ",
            "new\nline",
            "nul.json",
            "saves/COM1",
        ] {
            assert_eq!(check_name(name).unwrap_err().kind(), ErrorKind::Invalid, "{name:?}");
        }
        for name in ["a", "saves/slot 1.json", "x.tmp.json", "A-b_c.d"] {
            assert!(check_name(name).is_ok(), "{name:?}");
        }
        assert_eq!(
            slot_file("a/b", SaveFormat::Json).unwrap_err().kind(),
            ErrorKind::Invalid
        );
    }

    #[test]
    fn the_data_folder_follows_each_system_and_the_override() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| std::ffi::OsString::from(v))
            }
        };
        let home = env(&[("HOME", "/home/p"), ("APPDATA", "C:\\Users\\p\\AppData\\Roaming")]);
        let expected = if cfg!(windows) {
            PathBuf::from("C:\\Users\\p\\AppData\\Roaming")
        } else if cfg!(any(target_os = "macos", target_os = "ios")) {
            PathBuf::from("/home/p/Library/Application Support")
        } else {
            PathBuf::from("/home/p/.local/share")
        };
        assert_eq!(base_dir(&home), Some(expected));
        let over = env(&[(ENV_DATA_DIR, "/elsewhere"), ("HOME", "/home/p")]);
        assert_eq!(base_dir(&over), Some(PathBuf::from("/elsewhere")));
        assert_eq!(base_dir(&env(&[])), None, "no home: no folder");
        if !cfg!(windows) && !cfg!(any(target_os = "macos", target_os = "ios")) {
            let xdg = env(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/p")]);
            assert_eq!(base_dir(&xdg), Some(PathBuf::from("/xdg")));
            let relative = env(&[("XDG_DATA_HOME", "rel"), ("HOME", "/home/p")]);
            assert_eq!(base_dir(&relative), Some(PathBuf::from("/home/p/.local/share")));
        }
    }

    #[test]
    fn memory_files_keep_the_backup_and_store_the_file_and_backup_together() {
        let mut files = MemoryFiles::default();
        assert_eq!(files.write("a", b"1"), [Change::Put("a".into(), b"1".to_vec())]);
        assert_eq!(
            files.write("a", b"2"),
            [
                Change::Put("a.bak".into(), b"1".to_vec()),
                Change::Put("a".into(), b"2".to_vec())
            ]
        );
        assert_eq!(files.get("a").unwrap(), b"2");
        assert_eq!(files.get("a.bak").unwrap(), b"1");
        assert_eq!(files.names(), ["a", "a.bak"]);
        files.delete("a");
        assert!(files.names().is_empty());
    }
}
