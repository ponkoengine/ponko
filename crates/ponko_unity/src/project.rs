//! A Unity project's assets by GUID, from their `.meta` files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{Error, UnityFile};

/// A Unity project's assets: which file each GUID belongs to, read from
/// the `.meta` file beside every asset under `Assets/`, `Packages/` and
/// `Library/PackageCache/` (where installed packages such as the render
/// pipeline live).
///
/// ```
/// let dir = std::env::temp_dir().join("ponko_unity_doc_project");
/// std::fs::create_dir_all(dir.join("Assets")).unwrap();
/// std::fs::write(dir.join("Assets/Rock.mat"), "").unwrap();
/// std::fs::write(dir.join("Assets/Rock.mat.meta"), "fileFormatVersion: 2\nguid: 00ff\n").unwrap();
/// let project = ponko_unity::Project::open(&dir).unwrap();
/// assert_eq!(project.path_of("00ff"), Some(dir.join("Assets/Rock.mat").as_path()));
/// assert_eq!(project.guid_of(&dir.join("Assets/Rock.mat")), Some("00ff"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct Project {
    root: PathBuf,
    paths: HashMap<String, PathBuf>,
    guids: HashMap<PathBuf, String>,
}

impl Project {
    /// Reads every `.meta` file of the project at `root`.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`](crate::ErrorKind::Invalid) if `root` has no
    /// `Assets` folder; [`ErrorKind::Io`](crate::ErrorKind::Io) if a folder
    /// can't be read.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, Error> {
        let root = root.as_ref().to_path_buf();
        if !root.join("Assets").is_dir() {
            return Err(Error::invalid("not a Unity project: it has no Assets folder").at_path(&root));
        }
        let mut project = Self {
            root: root.clone(),
            ..Self::default()
        };
        for top in ["Assets", "Packages", "Library/PackageCache"] {
            let dir = root.join(top);
            if dir.is_dir() {
                project.scan(&dir)?;
            }
        }
        Ok(project)
    }

    fn scan(&mut self, dir: &Path) -> Result<(), Error> {
        let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                self.scan(&path)?;
            } else if path.extension().is_some_and(|e| e == "meta")
                && let Some(guid) = meta_guid(&path)
            {
                let asset = path.with_extension("");
                self.guids.insert(asset.clone(), guid.clone());
                self.paths.insert(guid, asset);
            }
        }
        Ok(())
    }

    /// The project's folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// How many assets have GUIDs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Whether no asset has a GUID.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The asset file with this GUID.
    #[must_use]
    pub fn path_of(&self, guid: &str) -> Option<&Path> {
        self.paths.get(guid).map(PathBuf::as_path)
    }

    /// The GUID of the asset at `path`.
    #[must_use]
    pub fn guid_of(&self, path: &Path) -> Option<&str> {
        self.guids.get(path).map(String::as_str)
    }

    /// The GUID of an asset whose file is named `name` (any case), anywhere
    /// in the project; the first by path when several are.
    pub(crate) fn find_by_file_name(&self, name: &str) -> Option<&str> {
        self.guids
            .iter()
            .filter(|(p, _)| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .min_by(|a, b| a.0.cmp(b.0))
            .map(|(_, g)| g.as_str())
    }

    /// Reads and parses the asset with this GUID (Unity text or binary).
    ///
    /// # Errors
    ///
    /// If no asset has the GUID, it can't be read, or it isn't a Unity
    /// asset (model files and shaders aren't).
    pub fn load(&self, guid: &str) -> Result<UnityFile, Error> {
        let path = self
            .path_of(guid)
            .ok_or_else(|| Error::invalid(format!("no asset has GUID {guid}")))?;
        UnityFile::read(path)
    }
}

/// The `guid:` line of a `.meta` file.
fn meta_guid(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("guid:"))
        .map(|g| g.trim().to_owned())
        .filter(|g| !g.is_empty())
}
