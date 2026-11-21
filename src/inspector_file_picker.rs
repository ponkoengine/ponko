//! The editor's system file picker. The UI queues a request during layout,
//! and the windowed loop runs it after egui finishes the frame.

use std::path::{Path, PathBuf};

use super::Panels;
use crate::ctx::Ctx;
use crate::editor::DocKind;

pub(super) const ENABLED: bool = cfg!(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    all(test, not(target_arch = "wasm32"))
));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FilePicker {
    Open,
    SaveAs { kind: DocKind, then_quit: bool },
}

impl Panels {
    pub(super) fn pick_file(&mut self, request: FilePicker) {
        if let Some(app) = &mut self.app {
            app.file_picker = Some(request);
        }
    }

    /// Selects a path via the windowed backend, or supplies one in a headless test.
    /// Returns whether `choose` was asked: a system dialog had the keyboard,
    /// so the panels forget the keys held (see `Panels::input_lost`).
    pub(super) fn run_file_picker(
        &mut self,
        ctx: &mut Ctx,
        choose: impl FnOnce(FilePicker, &Path, Option<&str>) -> Option<PathBuf>,
    ) -> bool {
        let Some(request) = self.app.as_mut().and_then(|app| app.file_picker.take()) else {
            return false;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let saving = matches!(request, FilePicker::SaveAs { .. });
            let roots = crate::assets::asset_roots();
            let suggested = ctx.editor.doc.path.as_deref().unwrap_or(match request {
                FilePicker::SaveAs {
                    kind: DocKind::Prefab, ..
                } => "prefabs/prefab.json",
                _ => super::DEFAULT_SCENE,
            });
            let suggested = Path::new(suggested);
            let current = ctx
                .editor
                .doc
                .path
                .as_deref()
                .and_then(|name| crate::assets::resolve(name).ok());
            let root = roots.iter().find(|root| root.is_dir());
            let folder = current
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .or_else(|| {
                    let root = root?;
                    let folder = root.join(suggested.parent()?);
                    folder.is_dir().then_some(folder)
                })
                .or_else(|| root.cloned())
                .unwrap_or_default();
            let name = if saving {
                suggested.file_name().and_then(|name| name.to_str())
            } else {
                None
            };
            let chosen = choose(request, &folder, name);
            self.input_lost();
            let Some(chosen) = chosen else {
                return true;
            };
            let chosen = if saving { with_json_extension(chosen) } else { chosen };
            let name = asset_name(&roots, &chosen, saving).and_then(|name| {
                let sources = &ctx.assets.asset_sources;
                if let Some(crate::assets::Origin::Source { source, .. }) = sources.locate(&name)
                    && sources.has(&source)
                {
                    return Err(format!(
                        "Asset source {source} takes precedence over {name}. Choose a unique path in assets/."
                    ));
                }
                Ok(name)
            });
            let Some(name) = self.report(name) else {
                return true;
            };
            match request {
                FilePicker::Open => self.open_file(ctx, &name),
                FilePicker::SaveAs { kind, then_quit } => {
                    self.save_as(ctx, &name, kind);
                    if then_quit && self.error.is_none() {
                        ctx.quit();
                    }
                }
            }
            true
        }
        // Web builds ask for the path in a field instead (`App::system_picker`).
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (ctx, choose, request);
            false
        }
    }
}

/// A save picker may return a name typed without an extension (Linux
/// portals and Zenity don't add one); documents are JSON files.
#[cfg(not(target_arch = "wasm32"))]
fn with_json_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension("json")
    }
}

/// Translates the selected disk path to the name the document system uses.
/// Check where that name resolves so another asset root can't redirect it.
#[cfg(not(target_arch = "wasm32"))]
fn asset_name(roots: &[PathBuf], chosen: &Path, saving: bool) -> Result<String, String> {
    let chosen_file = canonical_file(chosen, saving)?;
    for root in roots {
        let Ok(root) = root.canonicalize() else { continue };
        let Ok(relative) = chosen_file.strip_prefix(root) else {
            continue;
        };
        let Some(name) = relative.to_str() else {
            return Err("The file's path in assets/ must be valid UTF-8.".into());
        };
        #[cfg(windows)]
        let name = &name.replace('\\', "/");
        // The chosen Open file exists, so the write lookup uses the same
        // first-existing-file rule as Open. It also handles untitled saves.
        let resolved = crate::assets::write_path(roots, name)?;
        if canonical_file(&resolved, saving)? == chosen_file {
            return Ok(name.to_owned());
        }
        return Err(format!(
            "The asset name {name} resolves to {}. Choose a file with a unique path in assets/.",
            resolved.display()
        ));
    }
    Err(format!("Choose a file inside assets/. Selected {}", chosen.display()))
}

/// Canonicalize existing files, including symlinks. A new save file has
/// no metadata yet, but its directory must exist and can be canonicalized.
#[cfg(not(target_arch = "wasm32"))]
fn canonical_file(path: &Path, saving: bool) -> Result<PathBuf, String> {
    use std::io::ErrorKind::NotFound;
    let error = |e: std::io::Error| format!("Could not access {}: {e}", path.display());
    match path.symlink_metadata() {
        Ok(_) => {
            let resolved = path.canonicalize().map_err(error)?;
            if resolved.is_file() {
                Ok(resolved)
            } else {
                Err(format!("Choose a regular file. Selected {}", path.display()))
            }
        }
        Err(e) if e.kind() == NotFound && saving => {
            let Some((parent, file)) = path.parent().zip(path.file_name()) else {
                return Err(format!("Choose a filename inside assets/. Selected {}", path.display()));
            };
            let parent = parent.canonicalize().map_err(error)?;
            if !parent.is_dir() {
                return Err(format!(
                    "Choose a directory inside assets/. Selected {}",
                    parent.display()
                ));
            }
            Ok(parent.join(file))
        }
        Err(e) => Err(error(e)),
    }
}

pub(super) fn system_picker(
    window: &winit::window::Window,
    request: FilePicker,
    folder: &Path,
    name: Option<&str>,
) -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let mut picker = rfd::FileDialog::new()
            .set_parent(window)
            .add_filter("Scene or prefab", &["json"]);
        if !folder.as_os_str().is_empty() {
            picker = picker.set_directory(folder);
        }
        if let Some(name) = name {
            picker = picker.set_file_name(name);
        }
        match request {
            FilePicker::Open => picker.set_title("Open scene or prefab").pick_file(),
            FilePicker::SaveAs { kind, .. } => picker.set_title(format!("Save {} as", kind.name())).save_file(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (window, request, folder, name);
        None
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(case: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ponko-file-picker-{}-{case}", std::process::id()));
            // A run killed midway leaves its folder behind; start empty.
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn new_nested_save_paths_keep_asset_relative_names() {
        let temp = TempDir::new("nested");
        let root = temp.0.join("assets");
        let levels = root.join("levels");
        fs::create_dir_all(&levels).unwrap();
        let chosen = levels.join("À bientôt.json");
        assert_eq!(asset_name(&[root], &chosen, true).unwrap(), "levels/À bientôt.json");
        assert!(!chosen.exists());
    }

    #[test]
    fn a_save_without_an_extension_becomes_a_json_file() {
        assert_eq!(
            with_json_extension("assets/level".into()),
            PathBuf::from("assets/level.json")
        );
        assert_eq!(
            with_json_extension("assets/level.json".into()),
            PathBuf::from("assets/level.json")
        );
        assert_eq!(
            with_json_extension("assets/notes.txt".into()),
            PathBuf::from("assets/notes.txt")
        );
    }

    #[test]
    fn a_save_cannot_redirect_to_another_asset_root() {
        let temp = TempDir::new("multiple-roots");
        let primary = temp.0.join("project/assets");
        let secondary = temp.0.join("build/assets");
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&secondary).unwrap();
        let roots = [primary.clone(), secondary.clone()];
        let chosen = secondary.join("level.json");

        // A new file would land in the primary root.
        assert!(asset_name(&roots, &chosen, true).unwrap_err().contains("resolves to"));
        fs::write(&chosen, []).unwrap();
        assert_eq!(asset_name(&roots, &chosen, true).unwrap(), "level.json");
        // With both files present, a save by this name would overwrite the primary one.
        fs::write(primary.join("level.json"), []).unwrap();
        assert!(asset_name(&roots, &chosen, true).unwrap_err().contains("resolves to"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_choose_an_open_or_save_path_outside_assets() {
        let temp = TempDir::new("symlinks");
        let root = temp.0.join("assets");
        fs::create_dir(&root).unwrap();
        let external = temp.0.join("other.json");
        fs::write(&external, []).unwrap();
        let link = root.join("link.json");
        std::os::unix::fs::symlink(&external, &link).unwrap();
        let directory_link = root.join("elsewhere");
        std::os::unix::fs::symlink(&temp.0, &directory_link).unwrap();

        assert!(asset_name(std::slice::from_ref(&root), &link, false).is_err());
        assert!(asset_name(std::slice::from_ref(&root), &directory_link.join("new.json"), true).is_err());
        fs::remove_file(&external).unwrap();
        assert!(
            asset_name(&[root], &link, true).is_err(),
            "a dangling symlink is not a new file"
        );
    }
}
