//! The editor's settings: one `editor.json` per user, shared by every
//! project. It keeps the layout (panel sizes, the asset panel), the recent
//! projects with each one's recent files, and the shortcuts that were
//! rebound. `ponko editor --recent` reads the same file to list the
//! projects.
//!
//! The file is read again just before each write and only this editor's
//! parts are replaced, so two editors open at once don't undo each other's
//! recent lists, and keys a newer version adds are kept.
//!
//! Where it lives: `$PONKO_CONFIG_DIR/editor.json` if that is set, else
//! `%APPDATA%\ponko` (Windows), `~/Library/Application Support/ponko`
//! (macOS), or `$XDG_CONFIG_HOME/ponko` / `~/.config/ponko` (elsewhere).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::keys::Keymap;

/// Environment variable naming the folder the settings file is kept in.
pub(crate) const ENV_CONFIG_DIR: &str = "PONKO_CONFIG_DIR";
/// The settings file's name in that folder.
pub(crate) const FILE: &str = "editor.json";
/// How many recent projects, and recent files per project, are kept.
pub(crate) const RECENT: usize = 10;

/// The folder the settings file is kept in (see the module docs); `None`
/// where there is no home folder (or on the web).
pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        if let Some(dir) = var(ENV_CONFIG_DIR) {
            return Some(dir);
        }
        if cfg!(windows) {
            return var("APPDATA").map(|d| d.join("ponko"));
        }
        if cfg!(target_os = "macos") {
            return var("HOME").map(|h| h.join("Library/Application Support/ponko"));
        }
        var("XDG_CONFIG_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".config")))
            .map(|d| d.join("ponko"))
    }
}

/// Where the editor keeps its settings. Unit tests never touch the real
/// file: they pass their own path.
pub(crate) fn default_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    config_dir().map(|d| d.join(FILE))
}

/// Sizes of the editor's panels, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
    /// The hierarchy, on the left.
    pub left: f32,
    /// The components, on the right.
    pub right: f32,
    /// The asset panel, at the bottom.
    pub bottom: f32,
    /// Whether the asset panel is shown.
    pub assets: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            left: 300.0,
            right: 300.0,
            bottom: 220.0,
            assets: true,
        }
    }
}

impl Layout {
    fn to_json(self) -> Value {
        json!({ "left": self.left, "right": self.right, "bottom": self.bottom, "assets": self.assets })
    }

    fn from_json(v: &Value) -> Self {
        let d = Self::default();
        let size = |key: &str, default: f32| {
            v.get(key)
                .and_then(Value::as_f64)
                .map_or(default, |x| (x as f32).clamp(80.0, 2000.0))
        };
        Self {
            left: size("left", d.left),
            right: size("right", d.right),
            bottom: size("bottom", d.bottom),
            assets: v.get("assets").and_then(Value::as_bool).unwrap_or(d.assets),
        }
    }
}

/// A project opened in the editor and its recent files.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Project {
    /// The game's folder.
    pub path: String,
    /// Scene and prefab files in its assets, most recent first.
    pub files: Vec<String>,
}

/// What the settings file holds (see the module docs).
#[derive(Debug, Clone, Default)]
pub(crate) struct Settings {
    /// The file; `None`: nothing is read or written.
    pub path: Option<PathBuf>,
    pub layout: Layout,
    /// Most recent first; the first is this one once it's opened.
    pub projects: Vec<Project>,
    pub keys: Keymap,
    /// What was wrong in the file (it's used as far as it's readable).
    pub warnings: Vec<String>,
}

impl Settings {
    /// Reads `path`; a missing file is the defaults.
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut s = Self {
            path,
            ..Self::default()
        };
        let file = match s.read() {
            Ok(file) => file,
            Err(e) => {
                s.warnings.push(e);
                Map::new()
            }
        };
        if let Some(layout) = file.get("layout") {
            s.layout = Layout::from_json(layout);
        }
        s.projects = projects(&file);
        if let Some(keys) = file.get("keys").and_then(Value::as_object) {
            let (keys, warnings) = Keymap::with_changes(keys);
            s.keys = keys;
            s.warnings.extend(warnings.into_iter().map(|w| format!("{FILE}: {w}")));
        }
        s
    }

    /// The file as it is on disk now.
    fn read(&self) -> Result<Map<String, Value>, String> {
        let Some(path) = &self.path else {
            return Ok(Map::new());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(map)) => Ok(map),
                Ok(_) => Err(format!("{}: not a JSON object; using the defaults", path.display())),
                Err(e) => Err(format!("{}: {e}; using the defaults", path.display())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Puts `project` (with `file`, a scene or prefab in it, if given) first
    /// in the recent lists.
    pub fn opened(&mut self, project: &str, file: Option<&str>) {
        let mut entry = match self.projects.iter().position(|p| p.path == project) {
            Some(i) => self.projects.remove(i),
            None => Project {
                path: project.to_string(),
                files: Vec::new(),
            },
        };
        if let Some(file) = file {
            entry.files.retain(|f| f != file);
            entry.files.insert(0, file.to_string());
            entry.files.truncate(RECENT);
        }
        self.projects.insert(0, entry);
        self.projects.truncate(RECENT);
    }

    /// The recent files of `project`, most recent first.
    pub fn recent_files(&self, project: &str) -> &[String] {
        self.projects
            .iter()
            .find(|p| p.path == project)
            .map_or(&[], |p| p.files.as_slice())
    }

    /// Writes the layout, the shortcuts and `project`'s recent files into
    /// the file as it is now, keeping everything else in it.
    pub fn save(&mut self, project: &str) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        // A file that can't be read isn't overwritten with less.
        let mut file = self.read()?;
        let mut on_disk = projects(&file);
        if let Some(mine) = self.projects.iter().find(|p| p.path == project) {
            on_disk.retain(|p| p.path != project);
            on_disk.insert(0, mine.clone());
            on_disk.truncate(RECENT);
        }
        self.projects = on_disk;
        file.insert("layout".into(), self.layout.to_json());
        file.insert("keys".into(), Value::Object(self.keys.changes()));
        file.insert(
            "projects".into(),
            self.projects
                .iter()
                .map(|p| json!({ "path": p.path, "files": p.files }))
                .collect(),
        );
        write(&path, &Value::Object(file))
    }
}

/// The `projects` list of a settings file.
fn projects(file: &Map<String, Value>) -> Vec<Project> {
    let list = file.get("projects").and_then(Value::as_array);
    list.into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Project {
                path: p.get("path")?.as_str()?.to_string(),
                files: p
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect(),
            })
        })
        .collect()
}

/// Writes `value` to `path` through a file beside it, so a crash never
/// leaves half a file.
fn write(path: &Path, value: &Value) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text + "\n").map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(fail)
}

/// The project the editor runs in: the game's folder (its working
/// directory, which `ponko editor` sets).
pub(crate) fn current_project() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        String::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::inspector::keys::{Action, parse};

    /// A settings file in a folder of its own, removed when dropped.
    pub(crate) struct TempSettings(pub PathBuf);

    impl TempSettings {
        pub fn new(what: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ponko-settings-{what}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir.join(FILE))
        }
    }

    impl Drop for TempSettings {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    #[test]
    fn layout_recent_files_and_rebound_keys_are_remembered() {
        let file = TempSettings::new("remember");
        let mut s = Settings::load(Some(file.0.clone()));
        assert!(s.warnings.is_empty() && s.projects.is_empty());
        s.layout.left = 420.0;
        s.layout.assets = false;
        s.keys.set(Action::Duplicate, vec![parse("Ctrl+J").unwrap()]);
        s.opened("/games/a", Some("levels/one.json"));
        s.opened("/games/a", Some("levels/two.json"));
        s.opened("/games/a", Some("levels/one.json"));
        s.save("/games/a").unwrap();

        let back = Settings::load(Some(file.0.clone()));
        assert_eq!(back.layout, s.layout);
        assert_eq!(back.keys, s.keys);
        assert_eq!(back.recent_files("/games/a"), ["levels/one.json", "levels/two.json"]);
    }

    #[test]
    fn two_editors_keep_each_others_recent_projects_and_unknown_keys() {
        let file = TempSettings::new("merge");
        std::fs::create_dir_all(file.0.parent().unwrap()).unwrap();
        std::fs::write(
            &file.0,
            r#"{"theme": "dark", "projects": [{"path": "/games/b", "files": ["x.json"]}]}"#,
        )
        .unwrap();
        let mut a = Settings::load(Some(file.0.clone()));
        let mut c = Settings::load(Some(file.0.clone()));
        a.opened("/games/a", Some("a.json"));
        a.save("/games/a").unwrap();
        c.opened("/games/c", None);
        c.save("/games/c").unwrap();

        let back = Settings::load(Some(file.0.clone()));
        let paths: Vec<_> = back.projects.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["/games/c", "/games/a", "/games/b"]);
        assert_eq!(back.recent_files("/games/a"), ["a.json"]);
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&file.0).unwrap()).unwrap();
        assert_eq!(raw["theme"], "dark", "keys the editor doesn't know are kept");
    }

    #[test]
    fn a_broken_file_gives_the_defaults_with_a_warning_and_is_not_overwritten() {
        let file = TempSettings::new("broken");
        std::fs::create_dir_all(file.0.parent().unwrap()).unwrap();
        std::fs::write(&file.0, "{ not json").unwrap();
        let mut s = Settings::load(Some(file.0.clone()));
        assert_eq!(s.layout, Layout::default());
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        s.opened("/games/a", None);
        assert!(s.save("/games/a").is_err());
        assert_eq!(std::fs::read_to_string(&file.0).unwrap(), "{ not json");
    }

    #[test]
    fn recent_lists_stop_at_ten() {
        let mut s = Settings::default();
        for i in 0..15 {
            s.opened(&format!("/games/{i}"), Some(&format!("f{i}.json")));
            s.opened("/games/main", Some(&format!("f{i}.json")));
        }
        assert_eq!(s.projects.len(), RECENT);
        assert_eq!(s.recent_files("/games/main").len(), RECENT);
        assert_eq!(s.recent_files("/games/main")[0], "f14.json");
    }

    /// The CLI reads this file too (`ponko editor --recent`); both sides
    /// check the same fixture.
    #[test]
    fn the_shared_fixture_reads_as_the_cli_expects() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/editor-settings.json");
        let s = Settings::load(Some(fixture));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let paths: Vec<_> = s.projects.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["/home/me/games/space", "/home/me/games/farm"]);
        assert_eq!(
            s.recent_files("/home/me/games/space"),
            ["levels/one.json", "prefabs/ship.json"]
        );
        assert_eq!(s.keys.keys(Action::Duplicate), [parse("Ctrl+J").unwrap()]);
        assert_eq!(s.layout.left, 360.0);
    }
}
