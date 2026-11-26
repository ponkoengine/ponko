//! Steam's store-side services (feature `steam`): achievements and stats,
//! cloud saves kept in step with [`Storage`], rich presence, the overlay's
//! state, and the text input of Steam Deck and Big Picture.
//!
//! **Maturity: tested**, from the guide chapter `steam.md`. Game: none. Tests: `tests/steam_services.rs` and
//! the module's own tests, against the fake backend; the real Steam calls are a check a person runs
//! with two accounts. Platforms: Linux, macOS and Windows, headless (the Steam job in the list of
//! checks). Graphics: not involved. Missing: a recorded run against a real Steam client and a
//! released game that uses it.
//!
//! One [`Steam`] value is the way in. It works the same whether Steam is
//! there or not: [`Steam::start`] returns a working one when the Steam client
//! runs and an absent one when it doesn't, and every call on an absent one
//! does nothing (says so once, in the log) and returns `false` or `None`.
//! So one build runs from Steam and from any other store.
//!
//! ```no_run
//! use ponko::prelude::*;
//! use ponko::steam::Steam;
//!
//! let steam = Steam::start(480); // 480: Valve's test app; absent without Steam
//! let in_game = steam.clone();
//! Game::new("Crabs", 1280, 720)
//!     .app_id("com.example.crabs")
//!     .setup(move |ctx| {
//!         let _ = in_game.sync_cloud(&mut ctx.storage, &["saves/"]); // newest copy wins
//!     })
//!     .system(move |ctx| {
//!         steam.update(); // the overlay's state and text input arrive here
//!         if steam.overlay_open() {
//!             ctx.time.pause();
//!         }
//!         if ctx.input.key_pressed(Key::Enter) {
//!             steam.unlock_achievement("FIRST_WIN");
//!             steam.store_stats();
//!         }
//!     })
//!     .run();
//! ```
//!
//! [`Steam::fake`] gives one backed by an in-memory Steam ([`FakeSteam`]),
//! so a game's own Steam logic is tested headless.
#![deny(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, ErrorKind};
use crate::net::SteamClient;
use crate::storage::Storage;

/// What the text input shows and accepts
/// ([`Steam::show_text_input`], [`Steam::show_floating_keyboard`]).
///
/// ```
/// let mode = ponko::steam::SteamTextMode::SingleLine;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SteamTextMode {
    /// One line of text.
    SingleLine,
    /// Several lines.
    MultiLine,
    /// One line, shown as dots. The floating keyboard shows it as one line.
    Password,
    /// An email address: the floating keyboard's email layout; one line in
    /// the full-screen dialog.
    Email,
    /// Digits: the floating keyboard's number pad; one line in the
    /// full-screen dialog.
    Numeric,
}

/// How the player closed the text input that [`Steam::show_text_input`]
/// opened ([`Steam::take_text_input`]).
///
/// ```
/// use ponko::steam::SteamText;
/// let done = SteamText::Submitted("Crab".into());
/// assert_ne!(done, SteamText::Cancelled);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamText {
    /// The player confirmed this text.
    Submitted(String),
    /// The player closed the input without confirming.
    Cancelled,
}

/// What [`Steam::sync_cloud`] did: the files it copied each way, in sorted
/// order. Both are empty when the two sides already matched, and when
/// Steam or its cloud is off.
///
/// ```
/// let nothing = ponko::steam::CloudSync::default();
/// assert!(nothing.downloaded.is_empty() && nothing.uploaded.is_empty());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudSync {
    /// Files copied from Steam Cloud into the [`Storage`].
    pub downloaded: Vec<String>,
    /// Files copied from the [`Storage`] up to Steam Cloud.
    pub uploaded: Vec<String>,
}

/// What Steam's callbacks reported, read by [`Steam::update`].
#[derive(Default)]
struct Reports {
    /// The overlay opened (`true`) or closed (`false`), oldest first.
    overlay: Vec<bool>,
    /// The full-screen text input closed.
    text: Option<SteamText>,
}
type Shared = Arc<Mutex<Reports>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The Steam calls the services make. The real one talks to the Steam
/// client; [`FakeSteam`]'s keeps everything in memory.
trait Backend: Send + Sync {
    /// Runs Steam's callbacks, which put what they report into `reports`.
    fn pump(&self);
    fn achievement(&self, name: &str) -> Option<bool>;
    fn set_achievement(&self, name: &str, achieved: bool) -> bool;
    fn stat_i32(&self, name: &str) -> Option<i32>;
    fn stat_f32(&self, name: &str) -> Option<f32>;
    fn set_stat_i32(&self, name: &str, value: i32) -> bool;
    fn set_stat_f32(&self, name: &str, value: f32) -> bool;
    fn store_stats(&self) -> bool;
    fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool;
    fn clear_rich_presence(&self);
    fn is_steam_deck(&self) -> bool;
    fn is_big_picture(&self) -> bool;
    fn show_text_input(&self, mode: SteamTextMode, description: &str, existing: &str, max_chars: u32) -> bool;
    fn show_floating_keyboard(&self, mode: SteamTextMode, field: [i32; 4]) -> bool;
    fn cloud_enabled(&self) -> bool;
    fn cloud_files(&self) -> Vec<String>;
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>>;
    fn cloud_write(&self, name: &str, bytes: &[u8]) -> bool;
    /// When the file was last written, in seconds since 1970.
    fn cloud_time(&self, name: &str) -> i64;
    fn cloud_delete(&self, name: &str) -> bool;
}

struct Inner {
    backend: Option<Box<dyn Backend>>,
    client: Option<SteamClient>,
    reports: Shared,
    /// Why Steam is absent, for the one log line.
    absent_because: String,
    /// The state [`Steam::update`] last read.
    overlay_open: Mutex<bool>,
    text: Mutex<Option<SteamText>>,
    /// What was already said in the log: `""` for "Steam is absent", else
    /// one entry per refused name.
    told: Mutex<BTreeSet<String>>,
    /// Every line said, for the tests.
    log: Mutex<Vec<String>>,
}

/// Steam's achievements, stats, cloud saves, rich presence, overlay state
/// and Deck text input (feature `steam`). Clones share the same Steam.
///
/// Every call works without Steam: on an absent Steam ([`Steam::absent`],
/// or [`Steam::start`] when the client isn't running) it does nothing,
/// returns `false` or `None`, and the first such call says so once in the
/// log. A name Steam refuses (an achievement or stat that isn't set up for
/// the app on the Steamworks site) is also said once, per name.
///
/// See the [module](self) for a whole game.
///
/// ```
/// use ponko::steam::Steam;
/// let steam = Steam::absent(); // a build for another store
/// assert!(!steam.is_running());
/// assert!(!steam.unlock_achievement("FIRST_WIN")); // does nothing
/// assert_eq!(steam.achievement("FIRST_WIN"), None);
/// ```
#[derive(Clone)]
pub struct Steam {
    inner: Arc<Inner>,
}

impl fmt::Debug for Steam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Steam")
            .field("running", &self.is_running())
            .field("client", &self.inner.client)
            .finish_non_exhaustive()
    }
}

impl Steam {
    fn with(backend: Option<Box<dyn Backend>>, client: Option<SteamClient>, reports: Shared, why: String) -> Steam {
        Steam {
            inner: Arc::new(Inner {
                backend,
                client,
                reports,
                absent_because: why,
                overlay_open: Mutex::new(false),
                text: Mutex::new(None),
                told: Mutex::default(),
                log: Mutex::default(),
            }),
        }
    }

    /// Starts Steam for `app_id` (480 is Valve's test app), or returns an
    /// absent Steam, whose calls do nothing, when the Steam client isn't
    /// running or refuses (the reason goes to the log once). Never fails.
    ///
    /// The games of the networking chapter that also start a
    /// [`SteamClient`] pass it to [`with_client`](Self::with_client) instead.
    ///
    /// ```no_run
    /// let steam = ponko::steam::Steam::start(480);
    /// if !steam.is_running() {
    ///     println!("playing without Steam");
    /// }
    /// ```
    #[must_use]
    pub fn start(app_id: u32) -> Steam {
        Self::from_client(SteamClient::init_app(app_id))
    }

    /// Starts Steam for the app Steam launched this game as (or the id in a
    /// `steam_appid.txt` beside the program, during development); absent
    /// otherwise, as [`start`](Self::start).
    ///
    /// ```no_run
    /// let steam = ponko::steam::Steam::start_launched();
    /// ```
    #[must_use]
    pub fn start_launched() -> Steam {
        Self::from_client(SteamClient::init())
    }

    fn from_client(client: std::io::Result<SteamClient>) -> Steam {
        match client {
            Ok(client) => Self::with_client(&client),
            Err(e) => {
                let steam = Self::absent_because(e.to_string());
                steam.say_absent();
                steam
            }
        }
    }

    /// The services of a [`SteamClient`] the game already started (for
    /// lobbies and networking).
    ///
    /// ```no_run
    /// let client = ponko::net::SteamClient::init_app(480)?;
    /// let steam = ponko::steam::Steam::with_client(&client);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[must_use]
    pub fn with_client(client: &SteamClient) -> Steam {
        let reports = Shared::default();
        let real = Real::new(client.clone(), reports.clone());
        Self::with(Some(Box::new(real)), Some(client.clone()), reports, String::new())
    }

    /// A Steam that isn't there: every call does nothing. For a build that
    /// ships to another store, or a player who turned Steam off in the
    /// game's options.
    ///
    /// ```
    /// let steam = ponko::steam::Steam::absent();
    /// assert!(!steam.is_running());
    /// ```
    #[must_use]
    pub fn absent() -> Steam {
        Self::absent_because("this game started without it".into())
    }

    fn absent_because(why: String) -> Steam {
        Self::with(None, None, Shared::default(), why)
    }

    /// A Steam kept in memory, for tests, and the [`FakeSteam`] that
    /// controls it and reads back what the game did. Nothing reaches Steam.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_achievements(&["FIRST_WIN"]);
    /// assert!(steam.unlock_achievement("FIRST_WIN"));
    /// assert!(fake.achieved("FIRST_WIN"));
    /// ```
    #[must_use]
    pub fn fake() -> (Steam, FakeSteam) {
        let reports = Shared::default();
        let fake = FakeSteam {
            state: Arc::new(Mutex::new(FakeState {
                cloud_enabled: true,
                ..FakeState::default()
            })),
            reports: reports.clone(),
        };
        let steam = Self::with(Some(Box::new(fake.clone())), None, reports, String::new());
        (steam, fake)
    }

    /// Whether Steam is there: `false` for an absent Steam.
    ///
    /// ```
    /// assert!(ponko::steam::Steam::fake().0.is_running());
    /// assert!(!ponko::steam::Steam::absent().is_running());
    /// ```
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner.backend.is_some()
    }

    /// The [`SteamClient`] underneath, for lobbies and networking: `None`
    /// when Steam is absent, and for a fake.
    ///
    /// ```
    /// assert!(ponko::steam::Steam::absent().client().is_none());
    /// ```
    #[must_use]
    pub fn client(&self) -> Option<&SteamClient> {
        self.inner.client.as_ref()
    }

    /// The backend, or `None` after saying once that Steam is absent.
    fn backend(&self) -> Option<&dyn Backend> {
        let b = self.inner.backend.as_deref();
        if b.is_none() {
            self.say_absent();
        }
        b
    }

    fn say_absent(&self) {
        self.say_once(
            "",
            &format!(
                "ponko: Steam is not running ({}): achievements, stats, cloud saves, rich presence and the Steam text input do nothing in this run",
                self.inner.absent_because
            ),
        );
    }

    fn say_once(&self, key: &str, line: &str) {
        if lock(&self.inner.told).insert(key.to_string()) {
            eprintln!("{line}");
            lock(&self.inner.log).push(line.to_string());
        }
    }

    /// `ok`, saying once per `name` when Steam refused it.
    fn refused(&self, ok: bool, what: &str, name: &str) -> bool {
        if !ok {
            self.say_once(
                &format!("{what} {name}"),
                &format!("ponko: Steam refused the {what} {name:?}: is it set up for this app on the Steamworks site?"),
            );
        }
        ok
    }

    /// The lines this Steam said in the log (tests).
    #[cfg(test)]
    fn said(&self) -> Vec<String> {
        lock(&self.inner.log).clone()
    }

    /// Runs Steam's callbacks and takes in what they reported: the
    /// overlay's state and a closed text input. Call it once a frame. The
    /// Steam transports and lobbies run the same callbacks; calling both is
    /// fine.
    ///
    /// ```
    /// let steam = ponko::steam::Steam::absent();
    /// steam.update(); // nothing to do without Steam
    /// ```
    pub fn update(&self) {
        let Some(b) = self.inner.backend.as_deref() else {
            return; // quietly: this runs every frame
        };
        b.pump();
        let mut reports = lock(&self.inner.reports);
        if let Some(&open) = reports.overlay.last() {
            *lock(&self.inner.overlay_open) = open;
        }
        reports.overlay.clear();
        if let Some(text) = reports.text.take() {
            *lock(&self.inner.text) = Some(text);
        }
    }

    /// Unlocks the achievement `name` (its API name on the Steamworks
    /// site). Steam shows it to the player at once and keeps it after
    /// [`store_stats`](Self::store_stats). `false` when Steam is absent or
    /// refuses the name.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_achievements(&["FIRST_WIN"]);
    /// assert!(steam.unlock_achievement("FIRST_WIN"));
    /// assert!(!steam.unlock_achievement("FIRST_WNI")); // not set up: refused, said once
    /// ```
    pub fn unlock_achievement(&self, name: &str) -> bool {
        self.backend()
            .is_some_and(|b| self.refused(b.set_achievement(name, true), "achievement", name))
    }

    /// Locks the achievement `name` again (for testing a game; players
    /// rarely lose one). `false` when Steam is absent or refuses the name.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_achievements(&["FIRST_WIN"]);
    /// steam.unlock_achievement("FIRST_WIN");
    /// assert!(steam.clear_achievement("FIRST_WIN"));
    /// assert_eq!(steam.achievement("FIRST_WIN"), Some(false));
    /// ```
    pub fn clear_achievement(&self, name: &str) -> bool {
        self.backend()
            .is_some_and(|b| self.refused(b.set_achievement(name, false), "achievement", name))
    }

    /// Whether the player has the achievement `name`: `None` when Steam is
    /// absent or doesn't know the name.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_achievements(&["FIRST_WIN"]);
    /// assert_eq!(steam.achievement("FIRST_WIN"), Some(false));
    /// assert_eq!(steam.achievement("NOPE"), None);
    /// ```
    #[must_use]
    pub fn achievement(&self, name: &str) -> Option<bool> {
        self.backend()?.achievement(name)
    }

    /// Sets the integer stat `name`. Steam keeps it after
    /// [`store_stats`](Self::store_stats). `false` when Steam is absent or
    /// refuses (an unknown name, or a stat that is a float).
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_stats(&["wins"]);
    /// assert!(steam.set_stat_i32("wins", 3));
    /// assert_eq!(steam.stat_i32("wins"), Some(3));
    /// ```
    pub fn set_stat_i32(&self, name: &str, value: i32) -> bool {
        self.backend()
            .is_some_and(|b| self.refused(b.set_stat_i32(name, value), "stat", name))
    }

    /// Sets the float stat `name`, as [`set_stat_i32`](Self::set_stat_i32).
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_stats(&["distance"]);
    /// assert!(steam.set_stat_f32("distance", 12.5));
    /// assert_eq!(steam.stat_f32("distance"), Some(12.5));
    /// ```
    pub fn set_stat_f32(&self, name: &str, value: f32) -> bool {
        self.backend()
            .is_some_and(|b| self.refused(b.set_stat_f32(name, value), "stat", name))
    }

    /// The integer stat `name`: `None` when Steam is absent or doesn't know it.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::absent().stat_i32("wins"), None);
    /// ```
    #[must_use]
    pub fn stat_i32(&self, name: &str) -> Option<i32> {
        self.backend()?.stat_i32(name)
    }

    /// The float stat `name`: `None` when Steam is absent or doesn't know it.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::absent().stat_f32("distance"), None);
    /// ```
    #[must_use]
    pub fn stat_f32(&self, name: &str) -> Option<f32> {
        self.backend()?.stat_f32(name)
    }

    /// Sends the achievements and stats changed since the last call to
    /// Steam's servers. Call it at moments that matter (a level done, the
    /// game quitting), not every frame: Steam limits how often. `false`
    /// when Steam is absent or refuses.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// assert!(steam.store_stats());
    /// assert_eq!(fake.stores(), 1);
    /// ```
    pub fn store_stats(&self) -> bool {
        self.backend().is_some_and(Backend::store_stats)
    }

    /// Sets the rich presence `key` to `value`: what friends see the
    /// player doing. `"status"` is the text in the friends list;
    /// `"steam_display"` names a localized string set up on the Steamworks
    /// site, which may use other keys as `%key%`. An empty `value` removes
    /// the key. `false` when Steam is absent or refuses (a key or value too
    /// long, or too many keys).
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// assert!(steam.set_rich_presence("status", "Level 3"));
    /// assert_eq!(fake.rich_presence("status").as_deref(), Some("Level 3"));
    /// ```
    pub fn set_rich_presence(&self, key: &str, value: &str) -> bool {
        if key.contains('\0') || value.contains('\0') {
            return self.refused(false, "rich presence key", key);
        }
        self.backend().is_some_and(|b| {
            let ok = b.set_rich_presence(key, (!value.is_empty()).then_some(value));
            self.refused(ok, "rich presence key", key)
        })
    }

    /// Removes every rich presence key.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// steam.set_rich_presence("status", "Menu");
    /// steam.clear_rich_presence();
    /// assert_eq!(fake.rich_presence("status"), None);
    /// ```
    pub fn clear_rich_presence(&self) {
        if let Some(b) = self.backend() {
            b.clear_rich_presence();
        }
    }

    /// Whether the Steam overlay is open (shift+tab, or the Steam button),
    /// as of the last [`update`](Self::update). Pause the game while it is.
    /// `false` when Steam is absent.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_overlay(true);
    /// assert!(!steam.overlay_open()); // not until update
    /// steam.update();
    /// assert!(steam.overlay_open());
    /// ```
    #[must_use]
    pub fn overlay_open(&self) -> bool {
        *lock(&self.inner.overlay_open)
    }

    /// Whether the game runs on a Steam Deck: show the Deck's button
    /// glyphs, and use [`show_floating_keyboard`](Self::show_floating_keyboard)
    /// for text. `false` when Steam is absent.
    ///
    /// ```
    /// assert!(!ponko::steam::Steam::absent().is_steam_deck());
    /// ```
    #[must_use]
    pub fn is_steam_deck(&self) -> bool {
        self.inner.backend.as_deref().is_some_and(Backend::is_steam_deck)
    }

    /// Whether Steam runs in Big Picture mode (a TV and a gamepad), where
    /// [`show_text_input`](Self::show_text_input) is the way to type.
    /// `false` when Steam is absent.
    ///
    /// ```
    /// assert!(!ponko::steam::Steam::absent().is_big_picture());
    /// ```
    #[must_use]
    pub fn is_big_picture(&self) -> bool {
        self.inner.backend.as_deref().is_some_and(Backend::is_big_picture)
    }

    /// Opens Steam's full-screen text input for a gamepad, over the game,
    /// with `description` above it and `existing` already typed, for at
    /// most `max_chars` characters. The text arrives when the player closes
    /// it: [`take_text_input`](Self::take_text_input) after
    /// [`update`](Self::update).
    ///
    /// `false` when it didn't open: Steam absent, or not in Big Picture
    /// mode or on a Deck (it opens only there). Show the game's own
    /// keyboard then.
    ///
    /// ```
    /// use ponko::steam::{Steam, SteamText, SteamTextMode};
    /// let (steam, fake) = Steam::fake();
    /// fake.set_big_picture(true);
    /// assert!(steam.show_text_input(SteamTextMode::SingleLine, "Your name", "", 16));
    /// fake.close_text_input(Some("Crab"));
    /// steam.update();
    /// assert_eq!(steam.take_text_input(), Some(SteamText::Submitted("Crab".into())));
    /// ```
    pub fn show_text_input(&self, mode: SteamTextMode, description: &str, existing: &str, max_chars: u32) -> bool {
        if description.contains('\0') || existing.contains('\0') {
            return false;
        }
        self.backend()
            .is_some_and(|b| b.show_text_input(mode, description, existing, max_chars.max(1)))
    }

    /// How the player closed the text input of
    /// [`show_text_input`](Self::show_text_input), once, after the
    /// [`update`](Self::update) that saw it; `None` while it is open, and
    /// after it was taken.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::absent().take_text_input(), None);
    /// ```
    pub fn take_text_input(&self) -> Option<SteamText> {
        lock(&self.inner.text).take()
    }

    /// Opens the Steam Deck's floating keyboard, placed so it doesn't cover
    /// the text field at `x, y` (pixels from the window's top left) of
    /// `width` by `height`. The keys it sends arrive as ordinary typed text
    /// (`ctx.input`), so the game's own text field works as with a real
    /// keyboard. `false` when it didn't open (Steam absent, not on a Deck).
    ///
    /// ```
    /// use ponko::steam::{Steam, SteamTextMode};
    /// let (steam, fake) = Steam::fake();
    /// fake.set_steam_deck(true);
    /// assert!(steam.show_floating_keyboard(SteamTextMode::Numeric, 100, 40, 300, 32));
    /// ```
    pub fn show_floating_keyboard(&self, mode: SteamTextMode, x: i32, y: i32, width: i32, height: i32) -> bool {
        self.backend()
            .is_some_and(|b| b.show_floating_keyboard(mode, [x, y, width, height]))
    }

    /// Whether Steam Cloud keeps this player's files: Steam is there, and
    /// neither the player (Steam's settings) nor the app turned it off.
    ///
    /// ```
    /// assert!(ponko::steam::Steam::fake().0.cloud_enabled());
    /// assert!(!ponko::steam::Steam::absent().cloud_enabled());
    /// ```
    #[must_use]
    pub fn cloud_enabled(&self) -> bool {
        self.backend().is_some_and(Backend::cloud_enabled)
    }

    /// Brings Steam Cloud and `storage` (`ctx.storage`) in step for `files`:
    /// file names, and folders ending in `/` for every file below them
    /// (`&["settings.json", "saves/"]`).
    ///
    /// A file on one side only is copied to the other. A file that differs
    /// is copied from the side written last, so the newest copy wins: call
    /// it at launch (another computer may have played since) and after
    /// saving (or at quit). The [`Storage`] folder stays the game's own
    /// copy: the game reads and writes there as always, and without Steam
    /// (or with its cloud off) this does nothing and the folder is all
    /// there is.
    ///
    /// ```
    /// use ponko::steam::Steam;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-cloud-{}", std::process::id()));
    /// let mut storage = ponko::storage::Storage::in_folder(&dir);
    /// let (steam, fake) = Steam::fake();
    /// fake.put_cloud_file("saves/slot1.json", b"{\"level\":4}", std::time::SystemTime::now());
    /// let done = steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    /// assert_eq!(done.downloaded, ["saves/slot1.json"]);
    /// assert_eq!(storage.read("saves/slot1.json").unwrap(), b"{\"level\":4}");
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] for an empty entry in `files`, the
    /// [`Storage`]'s errors (storage off, a disk that refuses), and
    /// [`ErrorKind::Io`] naming the file when Steam Cloud refuses one (the
    /// player's cloud space is full). Files done before the failure stay
    /// done.
    pub fn sync_cloud(&self, storage: &mut Storage, files: &[&str]) -> Result<CloudSync, Error> {
        if let Some(bad) = files.iter().find(|f| f.is_empty() || **f == "/") {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!("sync_cloud: {bad:?} names no file; give file names, or folders ending in /"),
            ));
        }
        let mut done = CloudSync::default();
        let Some(b) = self.backend() else {
            return Ok(done);
        };
        if !b.cloud_enabled() {
            self.say_once(
                "cloud off",
                "ponko: Steam Cloud is off for this player or app: saves stay in the game's folder",
            );
            return Ok(done);
        }
        let covered = |name: &str| {
            files
                .iter()
                .any(|f| name == *f || (f.ends_with('/') && name.starts_with(f)))
        };
        let local: BTreeSet<String> = storage.list()?.into_iter().filter(|n| covered(n)).collect();
        // Cloud files the storage couldn't name are left alone.
        let cloud: BTreeSet<String> = b
            .cloud_files()
            .into_iter()
            .filter(|n| covered(n) && crate::storage::check_name(n).is_ok())
            .collect();
        for name in local.union(&cloud) {
            let up = match (local.contains(name), cloud.contains(name)) {
                (true, false) => true,
                (false, _) => false,
                (true, true) => {
                    let mine = storage.read(name)?;
                    let theirs = b.cloud_read(name);
                    if theirs.as_deref() == Some(mine.as_slice()) {
                        continue;
                    }
                    // The newer side wins; a tie keeps what this computer has.
                    let mine_at = storage
                        .modified(name)
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
                    theirs.is_none() || mine_at >= b.cloud_time(name)
                }
            };
            if up {
                let bytes = storage.read(name)?;
                if !b.cloud_write(name, &bytes) {
                    return Err(Error::new(
                        ErrorKind::Io,
                        format!("Steam Cloud refused {name:?} (is the player's cloud space full?)"),
                    )
                    .with_path(name.clone()));
                }
                done.uploaded.push(name.clone());
            } else {
                let Some(bytes) = b.cloud_read(name) else {
                    return Err(
                        Error::new(ErrorKind::Io, format!("could not read {name:?} from Steam Cloud"))
                            .with_path(name.clone()),
                    );
                };
                storage.write(name, &bytes)?;
                done.downloaded.push(name.clone());
            }
        }
        Ok(done)
    }

    /// Deletes the file `name` from `storage` and from Steam Cloud, so a
    /// later [`sync_cloud`](Self::sync_cloud) doesn't bring it back. Without
    /// Steam it deletes the local file only.
    ///
    /// ```
    /// use ponko::steam::Steam;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-cloud-del-{}", std::process::id()));
    /// let mut storage = ponko::storage::Storage::in_folder(&dir);
    /// let (steam, fake) = Steam::fake();
    /// storage.write("saves/old.json", b"{}").unwrap();
    /// steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    /// steam.delete_cloud_file(&mut storage, "saves/old.json").unwrap();
    /// assert!(!storage.exists("saves/old.json"));
    /// assert_eq!(fake.cloud_file("saves/old.json"), None);
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    ///
    /// # Errors
    ///
    /// As [`Storage::delete`]. Steam refusing is not an error: the file
    /// may not have been in the cloud.
    pub fn delete_cloud_file(&self, storage: &mut Storage, name: &str) -> Result<(), Error> {
        storage.delete(name)?;
        if let Some(b) = self.backend()
            && b.cloud_enabled()
        {
            b.cloud_delete(name);
        }
        Ok(())
    }
}

/// The real Steam client.
struct Real {
    client: SteamClient,
    reports: Shared,
    _overlay: steamworks::CallbackHandle,
    /// Registered again after each `show_gamepad_text_input`: the crate
    /// keeps one callback per kind and drops the one it registers.
    text: Mutex<Option<steamworks::CallbackHandle>>,
}

impl Real {
    fn new(client: SteamClient, reports: Shared) -> Real {
        let seen = reports.clone();
        let overlay = client
            .inner
            .register_callback(move |o: steamworks::GameOverlayActivated| {
                lock(&seen).overlay.push(o.active);
            });
        let real = Real {
            client,
            reports,
            _overlay: overlay,
            text: Mutex::new(None),
        };
        real.listen_for_text();
        real
    }

    fn listen_for_text(&self) {
        let seen = self.reports.clone();
        let client = self.client.inner.clone();
        let handle = self
            .client
            .inner
            .register_callback(move |d: steamworks::GamepadTextInputDismissed| {
                let text = match client.utils().get_entered_gamepad_text_input(&d) {
                    Some(text) => SteamText::Submitted(text),
                    None => SteamText::Cancelled,
                };
                lock(&seen).text = Some(text);
            });
        *lock(&self.text) = Some(handle);
    }

    fn stats(&self) -> steamworks::UserStats {
        self.client.inner.user_stats()
    }

    fn file(&self, name: &str) -> Option<steamworks::SteamFile> {
        // The crate panics on a NUL; Steam's own names never have one.
        (!name.contains('\0')).then(|| self.client.inner.remote_storage().file(name))
    }
}

impl Backend for Real {
    fn pump(&self) {
        self.client.run_callbacks();
    }
    fn achievement(&self, name: &str) -> Option<bool> {
        if name.contains('\0') {
            return None;
        }
        self.stats().achievement(name).get().ok()
    }
    fn set_achievement(&self, name: &str, achieved: bool) -> bool {
        if name.contains('\0') {
            return false;
        }
        let stats = self.stats();
        let a = stats.achievement(name);
        if achieved { a.set() } else { a.clear() }.is_ok()
    }
    fn stat_i32(&self, name: &str) -> Option<i32> {
        (!name.contains('\0')).then(|| self.stats().get_stat_i32(name).ok())?
    }
    fn stat_f32(&self, name: &str) -> Option<f32> {
        (!name.contains('\0')).then(|| self.stats().get_stat_f32(name).ok())?
    }
    fn set_stat_i32(&self, name: &str, value: i32) -> bool {
        !name.contains('\0') && self.stats().set_stat_i32(name, value).is_ok()
    }
    fn set_stat_f32(&self, name: &str, value: f32) -> bool {
        !name.contains('\0') && self.stats().set_stat_f32(name, value).is_ok()
    }
    fn store_stats(&self) -> bool {
        self.stats().store_stats().is_ok()
    }
    fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool {
        self.client.inner.friends().set_rich_presence(key, value)
    }
    fn clear_rich_presence(&self) {
        self.client.inner.friends().clear_rich_presence();
    }
    fn is_steam_deck(&self) -> bool {
        self.client.inner.utils().is_steam_running_on_steam_deck()
    }
    fn is_big_picture(&self) -> bool {
        self.client.inner.utils().is_steam_in_big_picture_mode()
    }
    fn show_text_input(&self, mode: SteamTextMode, description: &str, existing: &str, max_chars: u32) -> bool {
        use steamworks::{GamepadTextInputLineMode as Line, GamepadTextInputMode as Input};
        let input = if mode == SteamTextMode::Password {
            Input::Password
        } else {
            Input::Normal
        };
        let line = if mode == SteamTextMode::MultiLine {
            Line::MultipleLines
        } else {
            Line::SingleLine
        };
        let existing = (!existing.is_empty()).then_some(existing);
        let shown =
            self.client
                .inner
                .utils()
                .show_gamepad_text_input(input, line, description, max_chars, existing, |_| {});
        self.listen_for_text();
        shown
    }
    fn show_floating_keyboard(&self, mode: SteamTextMode, [x, y, w, h]: [i32; 4]) -> bool {
        use steamworks::FloatingGamepadTextInputMode as Keys;
        let keys = match mode {
            SteamTextMode::SingleLine | SteamTextMode::Password => Keys::SingleLine,
            SteamTextMode::MultiLine => Keys::MultipleLines,
            SteamTextMode::Email => Keys::Email,
            SteamTextMode::Numeric => Keys::Numeric,
        };
        self.client
            .inner
            .utils()
            .show_floating_gamepad_text_input(keys, x, y, w, h, || {})
    }
    fn cloud_enabled(&self) -> bool {
        let rs = self.client.inner.remote_storage();
        rs.is_cloud_enabled_for_account() && rs.is_cloud_enabled_for_app()
    }
    fn cloud_files(&self) -> Vec<String> {
        self.client
            .inner
            .remote_storage()
            .files()
            .into_iter()
            .map(|f| f.name)
            .collect()
    }
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>> {
        use std::io::Read;
        let file = self.file(name)?;
        if !file.exists() {
            return None;
        }
        let mut bytes = Vec::new();
        file.read().read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
    fn cloud_write(&self, name: &str, bytes: &[u8]) -> bool {
        use std::io::Write;
        let Some(file) = self.file(name) else { return false };
        let written = file.write().write_all(bytes).is_ok(); // the stream closes here
        written && self.cloud_read(name).as_deref() == Some(bytes)
    }
    fn cloud_time(&self, name: &str) -> i64 {
        self.file(name).map_or(0, |f| f.timestamp())
    }
    fn cloud_delete(&self, name: &str) -> bool {
        self.file(name).is_some_and(|f| f.delete())
    }
}

#[derive(Default)]
struct FakeState {
    achievements: BTreeMap<String, bool>,
    stats: BTreeMap<String, f64>,
    stores: u32,
    presence: BTreeMap<String, String>,
    deck: bool,
    big_picture: bool,
    text_open: bool,
    keyboard_open: bool,
    cloud_enabled: bool,
    cloud_full: bool,
    cloud: BTreeMap<String, (Vec<u8>, i64)>,
}

/// The controls of a fake Steam ([`Steam::fake`]): what Steam knows
/// (achievements, stats, cloud files), what the player does (open the
/// overlay, type text), and what the game did. Clones share the same fake.
///
/// ```
/// use ponko::steam::Steam;
/// let (steam, fake) = Steam::fake();
/// fake.define_stats(&["wins"]);
/// steam.set_stat_i32("wins", 2);
/// steam.store_stats();
/// assert_eq!(fake.stat("wins"), Some(2.0));
/// assert_eq!(fake.stores(), 1);
/// ```
#[derive(Clone)]
pub struct FakeSteam {
    state: Arc<Mutex<FakeState>>,
    reports: Shared,
}

impl fmt::Debug for FakeSteam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeSteam").finish_non_exhaustive()
    }
}

impl FakeSteam {
    fn state(&self) -> MutexGuard<'_, FakeState> {
        lock(&self.state)
    }

    /// Sets up achievements, as on the Steamworks site: locked at first.
    /// Steam refuses any other name.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_achievements(&["A", "B"]);
    /// assert_eq!(steam.achievement("B"), Some(false));
    /// ```
    pub fn define_achievements(&self, names: &[&str]) {
        let mut s = self.state();
        for n in names {
            s.achievements.entry((*n).to_string()).or_insert(false);
        }
    }

    /// Sets up stats, as on the Steamworks site: zero at first. A stat
    /// holds an integer or a float, as the game sets it.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.define_stats(&["wins"]);
    /// assert_eq!(steam.stat_i32("wins"), Some(0));
    /// ```
    pub fn define_stats(&self, names: &[&str]) {
        let mut s = self.state();
        for n in names {
            s.stats.entry((*n).to_string()).or_insert(0.0);
        }
    }

    /// Whether the achievement `name` is unlocked.
    ///
    /// ```
    /// assert!(!ponko::steam::Steam::fake().1.achieved("A"));
    /// ```
    #[must_use]
    pub fn achieved(&self, name: &str) -> bool {
        self.state().achievements.get(name).copied().unwrap_or(false)
    }

    /// The stat `name`, integer or float: `None` if it isn't set up.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::fake().1.stat("wins"), None);
    /// ```
    #[must_use]
    pub fn stat(&self, name: &str) -> Option<f64> {
        self.state().stats.get(name).copied()
    }

    /// How many times the game called [`Steam::store_stats`].
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::fake().1.stores(), 0);
    /// ```
    #[must_use]
    pub fn stores(&self) -> u32 {
        self.state().stores
    }

    /// The rich presence `key` the game set.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::fake().1.rich_presence("status"), None);
    /// ```
    #[must_use]
    pub fn rich_presence(&self, key: &str) -> Option<String> {
        self.state().presence.get(key).cloned()
    }

    /// The player opens (`true`) or closes the overlay; the game sees it at
    /// its next [`Steam::update`].
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_overlay(true);
    /// steam.update();
    /// assert!(steam.overlay_open());
    /// ```
    pub fn set_overlay(&self, open: bool) {
        lock(&self.reports).overlay.push(open);
    }

    /// Plays on a Steam Deck (`true`): the floating keyboard and the
    /// full-screen text input open.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_steam_deck(true);
    /// assert!(steam.is_steam_deck());
    /// ```
    pub fn set_steam_deck(&self, deck: bool) {
        self.state().deck = deck;
    }

    /// Steam runs in Big Picture mode (`true`): the full-screen text input opens.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_big_picture(true);
    /// assert!(steam.is_big_picture());
    /// ```
    pub fn set_big_picture(&self, on: bool) {
        self.state().big_picture = on;
    }

    /// Whether the full-screen text input is open.
    ///
    /// ```
    /// assert!(!ponko::steam::Steam::fake().1.text_input_open());
    /// ```
    #[must_use]
    pub fn text_input_open(&self) -> bool {
        self.state().text_open
    }

    /// Whether the game opened the floating keyboard (it stays open in the
    /// fake).
    ///
    /// ```
    /// assert!(!ponko::steam::Steam::fake().1.keyboard_open());
    /// ```
    #[must_use]
    pub fn keyboard_open(&self) -> bool {
        self.state().keyboard_open
    }

    /// The player closes the full-screen text input: `Some(text)` confirms
    /// it, `None` cancels. The game sees it at its next [`Steam::update`].
    /// Does nothing if it isn't open.
    ///
    /// ```
    /// use ponko::steam::{Steam, SteamText, SteamTextMode};
    /// let (steam, fake) = Steam::fake();
    /// fake.set_steam_deck(true);
    /// steam.show_text_input(SteamTextMode::Password, "Password", "", 32);
    /// fake.close_text_input(None);
    /// steam.update();
    /// assert_eq!(steam.take_text_input(), Some(SteamText::Cancelled));
    /// ```
    pub fn close_text_input(&self, text: Option<&str>) {
        let mut s = self.state();
        if !s.text_open {
            return;
        }
        s.text_open = false;
        lock(&self.reports).text = Some(text.map_or(SteamText::Cancelled, |t| SteamText::Submitted(t.into())));
    }

    /// The player turns Steam Cloud off (`false`) or on, in Steam's settings.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_cloud_enabled(false);
    /// assert!(!steam.cloud_enabled());
    /// ```
    pub fn set_cloud_enabled(&self, on: bool) {
        self.state().cloud_enabled = on;
    }

    /// The player's cloud space fills up (`true`): Steam refuses every
    /// write, as when the app's quota is used.
    ///
    /// ```
    /// let (steam, fake) = ponko::steam::Steam::fake();
    /// fake.set_cloud_full(true);
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-cloud-full-{}", std::process::id()));
    /// let mut storage = ponko::storage::Storage::in_folder(&dir);
    /// storage.write("a.txt", b"x").unwrap();
    /// assert!(steam.sync_cloud(&mut storage, &["a.txt"]).is_err());
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    pub fn set_cloud_full(&self, full: bool) {
        self.state().cloud_full = full;
    }

    /// Puts a file in the cloud as another computer would have, written at
    /// `at`.
    ///
    /// ```
    /// let (_, fake) = ponko::steam::Steam::fake();
    /// fake.put_cloud_file("a.txt", b"x", std::time::SystemTime::now());
    /// assert_eq!(fake.cloud_file("a.txt").as_deref(), Some(&b"x"[..]));
    /// ```
    pub fn put_cloud_file(&self, name: &str, bytes: &[u8], at: SystemTime) {
        let secs = at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        self.state().cloud.insert(name.to_string(), (bytes.to_vec(), secs));
    }

    /// The bytes of the cloud file `name`.
    ///
    /// ```
    /// assert_eq!(ponko::steam::Steam::fake().1.cloud_file("a.txt"), None);
    /// ```
    #[must_use]
    pub fn cloud_file(&self, name: &str) -> Option<Vec<u8>> {
        self.state().cloud.get(name).map(|f| f.0.clone())
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Backend for FakeSteam {
    fn pump(&self) {}
    fn achievement(&self, name: &str) -> Option<bool> {
        self.state().achievements.get(name).copied()
    }
    fn set_achievement(&self, name: &str, achieved: bool) -> bool {
        self.state().achievements.get_mut(name).map(|a| *a = achieved).is_some()
    }
    fn stat_i32(&self, name: &str) -> Option<i32> {
        #[allow(clippy::cast_possible_truncation)]
        self.state().stats.get(name).map(|v| *v as i32)
    }
    fn stat_f32(&self, name: &str) -> Option<f32> {
        #[allow(clippy::cast_possible_truncation)]
        self.state().stats.get(name).map(|v| *v as f32)
    }
    fn set_stat_i32(&self, name: &str, value: i32) -> bool {
        self.state()
            .stats
            .get_mut(name)
            .map(|v| *v = f64::from(value))
            .is_some()
    }
    fn set_stat_f32(&self, name: &str, value: f32) -> bool {
        self.state()
            .stats
            .get_mut(name)
            .map(|v| *v = f64::from(value))
            .is_some()
    }
    fn store_stats(&self) -> bool {
        self.state().stores += 1;
        true
    }
    fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool {
        // Steam's limits: keys up to 64 bytes, values up to 256, 30 keys.
        let mut s = self.state();
        if key.is_empty() || key.len() > 64 || value.is_some_and(|v| v.len() > 256) {
            return false;
        }
        match value {
            Some(v) if s.presence.len() < 30 || s.presence.contains_key(key) => {
                s.presence.insert(key.into(), v.into());
                true
            }
            Some(_) => false,
            None => {
                s.presence.remove(key);
                true
            }
        }
    }
    fn clear_rich_presence(&self) {
        self.state().presence.clear();
    }
    fn is_steam_deck(&self) -> bool {
        self.state().deck
    }
    fn is_big_picture(&self) -> bool {
        self.state().big_picture
    }
    fn show_text_input(&self, _: SteamTextMode, _: &str, _: &str, _: u32) -> bool {
        let mut s = self.state();
        if !(s.deck || s.big_picture) || s.text_open {
            return false;
        }
        s.text_open = true;
        true
    }
    fn show_floating_keyboard(&self, _: SteamTextMode, _: [i32; 4]) -> bool {
        let mut s = self.state();
        s.keyboard_open |= s.deck;
        s.deck
    }
    fn cloud_enabled(&self) -> bool {
        self.state().cloud_enabled
    }
    fn cloud_files(&self) -> Vec<String> {
        self.state().cloud.keys().cloned().collect()
    }
    fn cloud_read(&self, name: &str) -> Option<Vec<u8>> {
        self.cloud_file(name)
    }
    fn cloud_write(&self, name: &str, bytes: &[u8]) -> bool {
        let mut s = self.state();
        if s.cloud_full {
            return false;
        }
        s.cloud.insert(name.into(), (bytes.to_vec(), now_secs()));
        true
    }
    fn cloud_time(&self, name: &str) -> i64 {
        self.state().cloud.get(name).map_or(0, |f| f.1)
    }
    fn cloud_delete(&self, name: &str) -> bool {
        self.state().cloud.remove(name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_steam_says_so_once_however_many_calls() {
        let steam = Steam::absent();
        for _ in 0..3 {
            assert!(!steam.unlock_achievement("A"));
            assert!(!steam.set_stat_i32("s", 1));
            assert!(!steam.store_stats());
            assert!(!steam.set_rich_presence("status", "x"));
            assert!(!steam.show_text_input(SteamTextMode::SingleLine, "", "", 8));
            steam.update();
        }
        let said = steam.said();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("Steam is not running"), "{said:?}");
    }

    #[test]
    fn a_refused_name_is_said_once_per_name() {
        let (steam, fake) = Steam::fake();
        fake.define_achievements(&["A"]);
        for _ in 0..3 {
            assert!(!steam.unlock_achievement("TYPO"));
            assert!(!steam.unlock_achievement("OTHER"));
            assert!(steam.unlock_achievement("A"));
        }
        let said = steam.said();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("\"TYPO\""), "{said:?}");
    }

    #[test]
    fn a_failed_start_is_absent_and_names_the_reason() {
        let steam = Steam::from_client(Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "could not start Steam (is it running and logged in?)",
        )));
        assert!(!steam.is_running());
        assert!(steam.client().is_none());
        assert!(steam.said()[0].contains("is it running"), "{:?}", steam.said());
        assert!(!steam.unlock_achievement("A"));
        assert_eq!(steam.said().len(), 1, "said once at start, not again");
    }

    #[test]
    fn steam_is_a_resource_systems_can_share() {
        fn shareable<T: Send + Sync + 'static>() {}
        shareable::<Steam>();
        shareable::<FakeSteam>();
    }
}
