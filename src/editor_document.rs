//! The file the editor edits: a scene or a prefab in `assets/`. Opening,
//! saving, saving as another file or kind, whether there are unsaved
//! changes (the dirty marker), and noticing when something else changed the
//! file on disk.
//!
//! A scene document is the whole world. A prefab document is a world that
//! holds the prefab alone: one root entity and what hangs off it, saved
//! back with [`Assets::save_prefab`](crate::assets::Assets::save_prefab).
//!
//! The editor knows a change is unsaved by its undo history: every change
//! moves the world to a new state, undo and redo move it back and forth,
//! and the document remembers the state it was saved in. Undoing back to
//! that state makes the document clean again.

use serde_json::Value;
use web_time::{Duration, Instant};

use super::Editor;
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::reflect::WorldReflect;

/// How often the file on disk is looked at for changes.
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(crate) const CHECK_EVERY: Duration = Duration::from_millis(500);

/// What a document's file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Kind {
    /// A scene (`"format": "ponko-world"`): the whole world.
    #[default]
    Scene,
    /// A prefab (`"format": "ponko-prefab"`): one entity and its tree.
    Prefab,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Scene => "scene",
            Kind::Prefab => "prefab",
        }
    }
}

/// What the file looked like when it was opened or saved last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
}

/// The stamp of asset file `path` now; `None` when there's no such file
/// (or no files: web builds).
fn stamp(path: &str) -> Option<Stamp> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let file = crate::assets::resolve(path).ok()?;
        let meta = std::fs::metadata(file).ok()?;
        Some(Stamp {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// The document: its file and kind, the state it was saved in, and how the
/// file looked then.
#[derive(Debug, Default)]
pub(crate) struct Document {
    /// The file in `assets/`; `None` for a world never saved (untitled).
    pub path: Option<String>,
    pub kind: Kind,
    /// The editor's state when it was opened or saved (see [`Editor::dirty`]).
    pub(super) saved: u64,
    stamp: Option<Stamp>,
    next_check: Option<Instant>,
    /// The file changed on disk while there were unsaved changes: the
    /// person decides between [`Editor::reload`] and [`Editor::keep_mine`].
    pub conflict: bool,
    /// The glTF scenes the scene names that didn't load when it was
    /// opened, or that didn't spawn again at Stop (their files are
    /// missing or broken): the editor's panel lists them, and its status
    /// line names them.
    pub unloaded: Vec<Unloaded>,
}

/// A glTF scene that a scene file names and that didn't load when the
/// scene was opened: it stays out of the world, and its record (with its
/// overrides) stays in the scene.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(crate) struct Unloaded {
    /// Its file in `assets/`.
    pub file: String,
    /// Why it didn't load.
    pub why: String,
}

impl Document {
    /// The world is now `path` as it is on disk, in editor state `state`.
    pub(super) fn opened(&mut self, path: Option<String>, kind: Kind, state: u64) {
        self.stamp = path.as_deref().and_then(stamp);
        self.path = path;
        self.kind = kind;
        self.saved = state;
        self.conflict = false;
        self.next_check = None;
        self.unloaded.clear();
    }

    /// The file's name for a title: its path, or "untitled scene".
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn name(&self) -> String {
        match &self.path {
            Some(path) => path.clone(),
            None => format!("untitled {}", self.kind.name()),
        }
    }
}

/// What [`Editor::check_disk`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "editor"), allow(dead_code))]
pub(crate) enum DiskChange {
    /// The file changed and there were no unsaved changes: it was opened again.
    Reloaded,
    /// The file changed while there were unsaved changes: nothing was
    /// done, [`Document::conflict`] is set.
    Conflict,
}

/// The format of a scene or prefab file's JSON.
fn kind_of(json: &Value) -> Option<Kind> {
    match json["format"].as_str()? {
        crate::scene::PREFAB_FORMAT => Some(Kind::Prefab),
        "ponko-world" => Some(Kind::Scene),
        _ => None,
    }
}

impl Editor {
    /// Replaces the world with an empty one (reflected resources stay) as
    /// an untitled scene.
    pub fn new_scene(&mut self, ctx: &mut Ctx) -> Result<(), Error> {
        self.clear_world(ctx)?;
        ctx.scene_file = None;
        self.reset_document(ctx, None, Kind::Scene);
        Ok(())
    }

    /// Opens scene or prefab file `path` (in `assets/`): the world becomes
    /// what it holds, the history is cleared, and the document is clean. A
    /// prefab is spawned alone, at the origin, in an empty world.
    ///
    /// # Errors
    ///
    /// `NotFound` and `Json` as [`Ctx::load_scene`]; `Invalid` for a JSON
    /// file that is neither a scene nor a prefab. The world is unchanged
    /// on an error.
    pub fn open(&mut self, ctx: &mut Ctx, path: &str) -> Result<Kind, Error> {
        let (_, text) = ctx.assets.read_text(path)?;
        let json: Value = serde_json::from_str(&text).map_err(|e| Error::from(e).context(path).with_path(path))?;
        let kind = kind_of(&json).ok_or_else(|| {
            Error::new(
                ErrorKind::Invalid,
                format!("{path} is neither a scene (\"format\": \"ponko-world\") nor a prefab (\"ponko-prefab\")"),
            )
            .with_path(path)
        })?;
        #[cfg_attr(not(feature = "3d"), allow(unused_mut))]
        let mut unloaded = Vec::new();
        match kind {
            Kind::Scene => {
                ctx.load_scene(path)?;
                // Its glTF scenes, as the game spawns them after a load
                // (with their overrides). One that doesn't load stays out,
                // and the status line names it.
                #[cfg(feature = "3d")]
                for (file, e) in ctx.respawn_saved_gltf() {
                    eprintln!("ponko: {path}: {e}");
                    unloaded.push(Unloaded {
                        file,
                        why: e.message().to_string(),
                    });
                }
            }
            Kind::Prefab => {
                // Read from the file, not a copy cached before it changed.
                ctx.assets.prefabs.remove(path);
                let before = ctx.world.save();
                self.clear_world(ctx)?;
                if let Err(e) = ctx.spawn_prefab(path, glam::Vec2::ZERO) {
                    ctx.replace_world(&before)?;
                    self.loads = ctx.scene_loads;
                    return Err(e);
                }
                settle(ctx);
                ctx.scene_file = None;
            }
        }
        self.reset_document(ctx, Some(path.to_string()), kind);
        self.doc.unloaded = unloaded;
        Ok(kind)
    }

    /// Tries again to spawn glTF scene file `file`, which didn't load when
    /// the document was opened (see [`Document::unloaded`]): every record
    /// of it in the scene that has no scene spawned, where it was saved,
    /// with its overrides. Nodes of other scenes saved on its nodes hang
    /// off them again. On success its entries leave
    /// [`Document::unloaded`]; the document stays as clean or changed as
    /// it was, as its file names the scene either way.
    ///
    /// # Errors
    ///
    /// The scene's error when it still doesn't load (its entry then says
    /// why); `NotFound` for a file that isn't waiting to load.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn retry_unloaded(&mut self, ctx: &mut Ctx, file: &str) -> Result<(), Error> {
        if !self.doc.unloaded.iter().any(|u| u.file == file) {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("{file} isn't a glTF scene of this scene that didn't load"),
            )
            .with_path(file));
        }
        #[cfg(feature = "3d")]
        {
            if let Some((_, e)) = ctx.respawn_saved_gltf_of(Some(file)).into_iter().next() {
                for u in self.doc.unloaded.iter_mut().filter(|u| u.file == file) {
                    u.why = e.message().to_string();
                }
                return Err(e);
            }
            self.doc.unloaded.retain(|u| u.file != file);
            Ok(())
        }
        #[cfg(not(feature = "3d"))]
        {
            let _ = ctx;
            Err(Error::new(ErrorKind::Unsupported, "glTF scenes need the `3d` feature").with_path(file))
        }
    }

    /// Opens the document's file again, dropping unsaved changes.
    pub fn reload(&mut self, ctx: &mut Ctx) -> Result<(), Error> {
        let Some(path) = self.doc.path.clone() else {
            return Err(Error::new(
                ErrorKind::NotFound,
                "the scene was never saved: nothing to reload",
            ));
        };
        self.open(ctx, &path).map(drop)
    }

    /// Saves the world to the document's file. Returns the path.
    ///
    /// # Errors
    ///
    /// `NotFound` for an untitled document (use [`save_as`](Self::save_as)),
    /// `Invalid` for a prefab document that doesn't have exactly one root
    /// entity, `Io` if the file can't be written.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn save(&mut self, ctx: &mut Ctx) -> Result<String, Error> {
        let Some(path) = self.doc.path.clone() else {
            return Err(Error::new(
                ErrorKind::NotFound,
                "this scene has no file yet: use Save as to name one",
            ));
        };
        let kind = self.doc.kind;
        self.save_as(ctx, &path, kind)?;
        Ok(path)
    }

    /// Saves the world as `kind` to `path` (in `assets/`), which becomes the
    /// document's file. Saving a scene as a prefab saves its one root
    /// entity (with what hangs off it); the world stays as it is.
    ///
    /// # Errors
    ///
    /// As [`save`](Self::save); the document is unchanged on an error.
    pub fn save_as(&mut self, ctx: &mut Ctx, path: &str, kind: Kind) -> Result<(), Error> {
        match kind {
            Kind::Scene => {
                // How its glTF scenes were edited goes in their records.
                #[cfg(feature = "3d")]
                ctx.record_gltf_overrides(None);
                ctx.assets.save_scene(&ctx.world, path)?;
                ctx.scene_file = Some(path.to_string());
            }
            Kind::Prefab => {
                let root = single_root(ctx)?;
                ctx.assets.save_prefab(&ctx.world, root, path)?;
                if ctx.scene_file.as_deref() == Some(path) {
                    ctx.scene_file = None;
                }
            }
        }
        let state = self.state;
        self.doc.opened(Some(path.to_string()), kind, state);
        Ok(())
    }

    /// Looks at the document's file on disk, at most every [`CHECK_EVERY`]
    /// (`now` is the current time). If something else changed it: with no
    /// unsaved changes it is opened again; with unsaved changes nothing is
    /// done and [`Document::conflict`] is set, for the person to choose
    /// [`reload`](Self::reload) or [`keep_mine`](Self::keep_mine). A file
    /// that was deleted is left alone: saving writes it again.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn check_disk(&mut self, ctx: &mut Ctx, now: Instant) -> Option<DiskChange> {
        if self.doc.next_check.is_some_and(|at| now < at) || self.doc.conflict {
            return None;
        }
        self.doc.next_check = Some(now + CHECK_EVERY);
        let path = self.doc.path.clone()?;
        let on_disk = stamp(&path)?;
        if self.doc.stamp == Some(on_disk) {
            return None;
        }
        if self.dirty() || self.playing() {
            self.doc.conflict = true;
            return Some(DiskChange::Conflict);
        }
        match self.open(ctx, &path) {
            Ok(_) => Some(DiskChange::Reloaded),
            Err(_) => {
                // Half written, or broken: look again later, keep the world.
                self.doc.stamp = Some(on_disk);
                None
            }
        }
    }

    /// Keeps the unsaved world over a file that changed on disk: the next
    /// save writes over it.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn keep_mine(&mut self) {
        self.doc.conflict = false;
        self.doc.stamp = self.doc.path.as_deref().and_then(stamp);
    }

    /// Empties the world, keeping reflected resources.
    fn clear_world(&mut self, ctx: &mut Ctx) -> Result<(), Error> {
        let mut empty = ctx.world.save();
        empty["entities"] = Value::Array(Vec::new());
        ctx.replace_world(&empty)
    }

    /// The world was replaced by the editor itself: a fresh history, and
    /// the document is `path` and clean.
    fn reset_document(&mut self, ctx: &Ctx, path: Option<String>, kind: Kind) {
        self.loads = ctx.scene_loads;
        self.played_from = None;
        self.forget_history();
        let state = self.state;
        self.doc.opened(path, kind, state);
    }
}

/// Places attachments and children lists after a spawn.
fn settle(ctx: &mut Ctx) {
    super::settle(&mut ctx.world);
}

/// The one entity nothing else hangs off, for a prefab document.
fn single_root(ctx: &Ctx) -> Result<ponko_ecs::Entity, Error> {
    let roots: Vec<_> = super::hierarchy(&ctx.world)
        .into_iter()
        .filter(|n| n.parent.is_none())
        .map(|n| n.entity)
        .collect();
    match roots[..] {
        [root] => Ok(root),
        [] => Err(Error::new(
            ErrorKind::Invalid,
            "a prefab needs an entity: the world is empty",
        )),
        _ => Err(Error::new(
            ErrorKind::Invalid,
            format!(
                "a prefab has one root entity, and this world has {}: drag the others onto one in the hierarchy, or save the selection as a prefab",
                roots.len()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Transform as T2;
    use crate::reflect::WorldReflect;
    use glam::vec2;
    use ponko_ecs::{Component, Entity};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Hp(f32);

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
    #[serde(default)]
    struct Rules {
        speed: f32,
    }

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(800.0, 600.0));
        ctx.world.reflect::<Hp>();
        ctx
    }

    /// An asset file name unique to this test run, removed when dropped.
    struct TempAsset {
        name: String,
        file: std::path::PathBuf,
    }

    impl TempAsset {
        fn new(what: &str) -> Self {
            let name = format!("ponko-test-doc-{what}-{}.json", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&name);
            Self { name, file }
        }
    }

    impl Drop for TempAsset {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
        }
    }

    fn hp_of(ctx: &mut Ctx) -> Vec<f32> {
        let mut out: Vec<f32> = ctx.world.query::<&Hp>().iter().map(|h| h.0).collect();
        out.sort_by(f32::total_cmp);
        out
    }

    #[test]
    fn a_saved_scene_opens_again_as_the_same_world_and_clean() {
        let file = TempAsset::new("scene");
        let mut ctx = ctx();
        let a = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(1.0, 2.0)));
        ctx.edit(|ed, ctx| ed.set(ctx, a, "Hp", serde_json::json!(4.0)))
            .unwrap();
        ctx.edit(|ed, ctx| ed.rename(ctx, a, "Gate")).unwrap();
        assert!(ctx.editor.dirty());
        assert!(ctx.edit(|ed, ctx| ed.save(ctx)).is_err(), "untitled: save as first");
        ctx.edit(|ed, ctx| ed.save_as(ctx, &file.name, Kind::Scene)).unwrap();
        assert!(!ctx.editor.dirty(), "saved");
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
        assert_eq!(ctx.scene_file.as_deref(), Some(file.name.as_str()));
        let before = ctx.world.save();

        // Change it, then open the file again: the change is gone.
        ctx.edit(|ed, ctx| ed.delete(ctx, a)).unwrap();
        assert!(ctx.editor.dirty());
        let kind = ctx.edit(|ed, ctx| ed.open(ctx, &file.name)).unwrap();
        assert_eq!(kind, Kind::Scene);
        assert!(
            !ctx.editor.dirty() && !ctx.editor.can_undo(),
            "opening starts a fresh history"
        );
        assert_eq!(ctx.world.save()["entities"], before["entities"]);
        assert_eq!(hp_of(&mut ctx), [4.0]);
    }

    #[test]
    fn saving_after_undoing_to_the_saved_state_and_redoing_dirties_again() {
        let file = TempAsset::new("undo-clean");
        let mut ctx = ctx();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(1.0, 2.0)));
        ctx.edit(|ed, ctx| ed.save_as(ctx, &file.name, Kind::Scene)).unwrap();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(3.0, 2.0)));
        assert!(ctx.editor.dirty());
        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert!(!ctx.editor.dirty(), "undone back to what was saved");
        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert!(ctx.editor.dirty(), "undone past it");
        ctx.edit(|ed, ctx| ed.redo(ctx)).unwrap();
        assert!(!ctx.editor.dirty());
    }

    #[test]
    fn a_prefab_document_opens_alone_and_saves_back_its_tree() {
        let file = TempAsset::new("prefab");
        let mut ctx = ctx();
        let root = ctx.world.spawn((T2::at(vec2(50.0, 0.0)), Hp(1.0)));
        let part = ctx.world.spawn((T2::at(vec2(54.0, 0.0)), Hp(2.0)));
        ctx.edit(|ed, ctx| ed.set_parent(ctx, part, Some(root))).unwrap();
        ctx.world.spawn((T2::default(), Hp(99.0))); // another root: not in the prefab
        ctx.assets.save_prefab(&ctx.world, root, &file.name).unwrap();

        let kind = ctx.edit(|ed, ctx| ed.open(ctx, &file.name)).unwrap();
        assert_eq!(kind, Kind::Prefab);
        assert_eq!(hp_of(&mut ctx), [1.0, 2.0], "the prefab alone, in an empty world");
        assert_eq!(ctx.editor.doc.kind, Kind::Prefab);
        assert!(ctx.scene_file.is_none(), "a scene Save mustn't write over the prefab");
        assert!(!ctx.editor.dirty());

        // Edit the part and save: the file has the change, still one tree.
        let part = ctx
            .world
            .query::<(Entity, &Hp)>()
            .iter()
            .find(|(_, h)| h.0 == 2.0)
            .map(|(e, _)| e)
            .unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, part, "Hp", serde_json::json!(7.0)))
            .unwrap();
        ctx.edit(|ed, ctx| ed.save(ctx)).unwrap();
        assert!(!ctx.editor.dirty());
        ctx.edit(|ed, ctx| ed.new_scene(ctx)).unwrap();
        let spawned = ctx.spawn_prefab(&file.name, vec2(0.0, 0.0)).unwrap();
        assert_eq!(hp_of(&mut ctx), [1.0, 7.0]);
        assert_eq!(super::super::tree(&ctx.world, &[spawned]).len(), 2);
    }

    #[test]
    fn a_prefab_needs_exactly_one_root() {
        let file = TempAsset::new("two-roots");
        let mut ctx = ctx();
        let err = ctx
            .edit(|ed, ctx| ed.save_as(ctx, &file.name, Kind::Prefab))
            .unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
        ctx.world.spawn((T2::default(),));
        ctx.world.spawn((T2::default(),));
        let err = ctx
            .edit(|ed, ctx| ed.save_as(ctx, &file.name, Kind::Prefab))
            .unwrap_err();
        assert!(err.to_string().contains("one root"), "{err}");
        assert!(!file.file.exists());
        assert!(
            ctx.editor.doc.path.is_none(),
            "a failed save leaves the document as it was"
        );
    }

    #[test]
    fn a_file_that_is_neither_a_scene_nor_a_prefab_is_refused_and_the_world_stays() {
        let file = TempAsset::new("other");
        std::fs::write(&file.file, br#"{"speed": 3}"#).unwrap();
        let mut ctx = ctx();
        ctx.world.spawn((Hp(5.0),));
        let err = ctx.edit(|ed, ctx| ed.open(ctx, &file.name)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid, "{err}");
        assert_eq!(hp_of(&mut ctx), [5.0]);
    }

    #[test]
    fn a_new_scene_is_empty_untitled_and_keeps_resources() {
        let mut ctx = ctx();
        ctx.world.reflect_resource::<Rules>();
        ctx.world.insert_resource(Rules { speed: 2.0 });
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(1.0, 1.0)));
        ctx.edit(|ed, ctx| ed.new_scene(ctx)).unwrap();
        assert_eq!(ctx.world.len(), 0);
        assert_eq!(ctx.world.resource::<Rules>().speed, 2.0);
        assert!(ctx.editor.doc.path.is_none() && !ctx.editor.dirty() && !ctx.editor.can_undo());
        assert_eq!(ctx.editor.doc.name(), "untitled scene");
    }

    #[test]
    fn a_scene_another_program_changed_opens_again_when_clean_and_asks_when_dirty() {
        let file = TempAsset::new("watched");
        let mut ctx = ctx();
        ctx.world.spawn((Hp(1.0),));
        ctx.edit(|ed, ctx| ed.save_as(ctx, &file.name, Kind::Scene)).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            ctx.edit(|ed, ctx| ed.check_disk(ctx, t0)),
            None,
            "our own save is no change"
        );

        // Another program writes the file: with nothing unsaved, it's reopened.
        let mut other = ctx.world.save();
        other["entities"][0]["components"]["Hp"] = serde_json::json!(2.5);
        std::fs::write(&file.file, serde_json::to_string_pretty(&other).unwrap() + "\n\n").unwrap();
        let soon = t0 + CHECK_EVERY / 2;
        assert_eq!(
            ctx.edit(|ed, ctx| ed.check_disk(ctx, soon)),
            None,
            "looked at only now and then"
        );
        let t1 = t0 + CHECK_EVERY * 2;
        assert_eq!(ctx.edit(|ed, ctx| ed.check_disk(ctx, t1)), Some(DiskChange::Reloaded));
        assert_eq!(hp_of(&mut ctx), [2.5]);
        assert!(!ctx.editor.dirty());

        // With unsaved changes, nothing is thrown away: the person decides.
        let e = ctx
            .world
            .query::<(Entity, &Hp)>()
            .iter()
            .map(|(e, _)| e)
            .next()
            .unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, e, "Hp", serde_json::json!(9.0)))
            .unwrap();
        other["entities"][0]["components"]["Hp"] = serde_json::json!(3.5);
        std::fs::write(&file.file, serde_json::to_string(&other).unwrap()).unwrap();
        let t2 = t1 + CHECK_EVERY * 2;
        assert_eq!(ctx.edit(|ed, ctx| ed.check_disk(ctx, t2)), Some(DiskChange::Conflict));
        assert!(ctx.editor.doc.conflict);
        assert_eq!(hp_of(&mut ctx), [9.0], "the unsaved change stays");
        let t3 = t2 + CHECK_EVERY * 2;
        assert_eq!(ctx.edit(|ed, ctx| ed.check_disk(ctx, t3)), None, "asked once");

        // Keep mine: no more questions, and saving writes over the file.
        ctx.edit(|ed, _| ed.keep_mine());
        assert!(!ctx.editor.doc.conflict);
        let t4 = t3 + CHECK_EVERY * 2;
        assert_eq!(ctx.edit(|ed, ctx| ed.check_disk(ctx, t4)), None);
        ctx.edit(|ed, ctx| ed.save(ctx)).unwrap();
        let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&file.file).unwrap()).unwrap();
        assert_eq!(on_disk["entities"][0]["components"]["Hp"], 9.0);

        // Or reload: the file's world, the change dropped.
        std::fs::write(&file.file, serde_json::to_string(&other).unwrap()).unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, e, "Hp", serde_json::json!(11.0)))
            .unwrap();
        let t5 = t4 + CHECK_EVERY * 2;
        assert_eq!(ctx.edit(|ed, ctx| ed.check_disk(ctx, t5)), Some(DiskChange::Conflict));
        ctx.edit(|ed, ctx| ed.reload(ctx)).unwrap();
        assert_eq!(hp_of(&mut ctx), [3.5]);
        assert!(!ctx.editor.doc.conflict && !ctx.editor.dirty());
    }

    #[test]
    fn a_load_by_something_else_makes_that_file_the_document() {
        let file = TempAsset::new("loaded");
        let mut ctx = ctx();
        ctx.world.spawn((Hp(1.0),));
        ctx.assets.save_scene(&ctx.world, &file.name).unwrap();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, vec2(0.0, 0.0)));
        ctx.load_scene(&file.name).unwrap(); // the game, or the control endpoint
        ctx.edit(|_, _| ());
        assert_eq!(ctx.editor.doc.path.as_deref(), Some(file.name.as_str()));
        assert!(!ctx.editor.dirty());
    }
}
