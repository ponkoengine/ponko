//! Reads Unity projects saved as text (Force Text serialization), for
//! importing them into Ponko: scenes, prefabs, materials and mesh
//! assets as [`UnityFile`]s of [`UnityObject`]s, the GUIDs in `.meta`
//! files as a [`Project`], and mesh vertex data as [`UnityMesh`].
//!
//! The guide's [Unity chapter](https://ponko.dev/guide/unity.html) shows the
//! whole import, from a project folder to files the engine loads, through
//! [`Importer`].
//!
//! ```
//! let text = "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &100\nGameObject:\n  m_Name: Lamp\n";
//! let file = ponko_unity::UnityFile::parse(text).unwrap();
//! assert_eq!(file.objects[0].type_name, "GameObject");
//! assert_eq!(file.objects[0].body["m_Name"].as_str(), Some("Lamp"));
//! ```

#![deny(missing_docs)]

mod animation;
mod baked;
mod binary;
mod builtin;
mod fbx;
mod glb;
mod hierarchy;
mod import;
mod material;
mod mesh;
mod project;
mod terrain;
mod texture;
mod value;

pub use animation::AnimationClip;
pub use hierarchy::{GameObject, Hierarchy};
pub use import::{Importer, Report};
pub use material::{Surface, TextureProperty, UnityMaterial};
pub use mesh::{BlendShape, Channel, MeshSkin, SkinAnimation, SkinNode, UnityMesh, VertexFormat};
pub use project::Project;
pub use terrain::UnityTerrain;
pub use texture::UnityTexture;
pub use value::{ObjRef, Value};

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What went wrong reading or importing a file, and where.
///
/// Match on [`kind`](Error::kind) to tell a missing file from damaged
/// data from a Unity feature the importer doesn't handle, without reading
/// the text. [`path`](Error::path) and [`line`](Error::line) say where,
/// when that is known, and [`message`](Error::message) says what. Printing
/// the error gives all three in one line.
///
/// ```
/// use ponko_unity::{ErrorKind, UnityFile};
///
/// let err = UnityFile::read("no/such/file.unity").unwrap_err();
/// assert_eq!(err.kind(), ErrorKind::Io);
/// assert!(err.path().is_some());
/// // The underlying `std::io::Error` stays reachable.
/// assert!(std::error::Error::source(&err).is_some());
///
/// let err = UnityFile::parse("--- !u!1 &1\nGameObject: [").unwrap_err();
/// assert_eq!(err.kind(), ErrorKind::Decode);
/// assert!(err.line().is_some());
/// ```
#[derive(Debug, Clone)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    path: Option<PathBuf>,
    line: Option<usize>,
    source: Option<Arc<std::io::Error>>,
}

/// The broad class of an [`Error`]: what a caller can act on.
///
/// New classes may be added in a minor release, so a `match` needs a
/// wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A file or folder couldn't be read, listed or written (it is missing,
    /// not allowed, full, ...). The [`std::io::Error`] is the error's
    /// source.
    Io,
    /// The data isn't well formed: bad YAML, a damaged or truncated binary
    /// asset, a header that doesn't parse, counts that don't fit the data.
    Decode,
    /// The data is well formed but uses something the importer doesn't
    /// handle: an older binary asset version, assets saved without type
    /// trees, compressed meshes or textures, a feature of a format.
    Unsupported,
    /// The data parses but doesn't make sense: a field missing or out of
    /// range, a reference that leads nowhere, prefabs that nest in a loop,
    /// a size beyond the importer's safety limits, an object of the wrong
    /// class.
    Invalid,
    /// Anything else.
    Other,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            path: None,
            line: None,
            source: None,
        }
    }

    pub(crate) fn decode(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Decode, message)
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }

    /// A failed file operation at `path`; the I/O error is kept as the source.
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: source.to_string(),
            path: Some(path.to_path_buf()),
            line: None,
            source: Some(Arc::new(source)),
        }
    }

    /// Says which file it was (kept if the error already has one).
    pub(crate) fn at_path(mut self, path: &Path) -> Self {
        self.path.get_or_insert_with(|| path.to_path_buf());
        self
    }

    pub(crate) fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Puts `context` in front of the message (`context: message`).
    pub(crate) fn with_context(mut self, context: &str) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }

    /// The class of failure to match on.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The file it happened in, when one is known.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The 1-based line of a text file it happened at, when known.
    #[must_use]
    pub fn line(&self) -> Option<usize> {
        self.line
    }

    /// What went wrong, without the path or line (printing the error has
    /// them).
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl PartialEq for Error {
    /// Errors are equal when their kind, message, path and line are; the
    /// I/O source (which can't be compared) is not looked at.
    fn eq(&self, other: &Self) -> bool {
        (self.kind, &self.message, &self.path, self.line) == (other.kind, &other.message, &other.path, other.line)
    }
}

impl Eq for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(f, "{}: ", path.display())?;
        }
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|e| e as &(dyn std::error::Error + 'static))
    }
}

/// One object of a Unity file: a `--- !u!<class> &<file id>` document.
#[derive(Debug, Clone, PartialEq)]
pub struct UnityObject {
    /// Unity's class id: 1 GameObject, 4 Transform, 21 Material, 43 Mesh,
    /// 114 MonoBehaviour, 1001 PrefabInstance, ...
    pub class_id: i32,
    /// Its id within the file, which references to it use.
    pub file_id: i64,
    /// A stand-in for an object of a nested prefab (`stripped`): only
    /// its links to the prefab instance are here.
    pub stripped: bool,
    /// The type's name: the document's one top-level key.
    pub type_name: String,
    /// Its fields.
    pub body: Value,
}

/// A whole Unity text file: its objects in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnityFile {
    /// Every object, in the order the file lists them.
    pub objects: Vec<UnityObject>,
}

impl UnityFile {
    /// Parses a Unity text file (a scene, prefab, material, `.asset`).
    ///
    /// ```
    /// let file = ponko_unity::UnityFile::parse("--- !u!4 &7 stripped\nTransform:\n  m_PrefabInstance: {fileID: 9}\n").unwrap();
    /// assert!(file.objects[0].stripped);
    /// assert_eq!(file.objects[0].body["m_PrefabInstance"].obj_ref().unwrap().file_id, 9);
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`] if a document header or its YAML is malformed
    /// (the error has the line).
    pub fn parse(text: &str) -> Result<Self, Error> {
        let mut objects = Vec::new();
        // Documents start at lines `--- !u!<class> &<id>[ stripped]`.
        let mut header: Option<(usize, &str)> = None;
        let mut start = 0;
        let mut offset = 0;
        let mut line_no = 0;
        let mut finish = |header: Option<(usize, &str)>, body: &str| -> Result<(), Error> {
            if let Some((line, h)) = header {
                objects.push(object(line, h, body)?);
            }
            Ok(())
        };
        for line in text.split_inclusive('\n') {
            line_no += 1;
            if header.is_none() && !line.starts_with("--- ") && !line.starts_with('%') && !line.trim().is_empty() {
                return Err(
                    Error::decode("not a Unity text file (expected `%YAML` or `--- !u!` documents)").at_line(line_no),
                );
            }
            if line.starts_with("--- ") {
                finish(header, &text[start..offset])?;
                header = Some((line_no, line.trim_end()));
                start = offset + line.len();
            }
            offset += line.len();
        }
        finish(header, &text[start..])?;
        Ok(Self { objects })
    }

    /// Parses a binary serialized asset saved by the Unity editor (Unity
    /// 2022 or later, with type trees), such as terrain data. Objects'
    /// fields are named as in the text format; references to other
    /// files carry their GUID.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Unsupported`] if it's older than Unity 2022 or has no
    /// type trees; [`ErrorKind::Decode`] if it isn't one or its data doesn't
    /// match its type trees.
    pub fn parse_binary(data: &[u8]) -> Result<Self, Error> {
        binary::parse(data)
    }

    /// Reads a Unity file, text or binary.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Io`] if it can't be read (with the `std::io::Error` as
    /// the source); otherwise as [`parse`](Self::parse) and
    /// [`parse_binary`](Self::parse_binary). The error names the file.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let data = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        let parsed = if binary::is_binary(&data) {
            Self::parse_binary(&data)
        } else {
            let text = std::str::from_utf8(&data)
                .map_err(|_| Error::decode("neither Unity text nor a binary asset").at_path(path))?;
            Self::parse(text)
        };
        parsed.map_err(|e| e.at_path(path))
    }

    /// The object with this file id.
    ///
    /// ```
    /// let file = ponko_unity::UnityFile::parse("--- !u!1 &5\nGameObject:\n  m_Name: A\n").unwrap();
    /// assert_eq!(file.get(5).unwrap().type_name, "GameObject");
    /// assert!(file.get(6).is_none());
    /// ```
    #[must_use]
    pub fn get(&self, file_id: i64) -> Option<&UnityObject> {
        self.objects.iter().find(|o| o.file_id == file_id)
    }

    /// Every object of one class (see [`UnityObject::class_id`]).
    ///
    /// ```
    /// let file = ponko_unity::UnityFile::parse("--- !u!1 &5\nGameObject: {}\n--- !u!4 &6\nTransform: {}\n").unwrap();
    /// assert_eq!(file.of_class(4).count(), 1);
    /// ```
    pub fn of_class(&self, class_id: i32) -> impl Iterator<Item = &UnityObject> {
        self.objects.iter().filter(move |o| o.class_id == class_id)
    }
}

/// One document from its header line and body text.
fn object(line: usize, header: &str, body: &str) -> Result<UnityObject, Error> {
    let bad = || Error::decode(format!("expected `--- !u!<class> &<id>`, got `{header}`")).at_line(line);
    let mut parts = header.split_whitespace().skip(1);
    let class_id = parts
        .next()
        .and_then(|t| t.strip_prefix("!u!"))
        .and_then(|t| t.parse().ok())
        .ok_or_else(bad)?;
    let file_id = parts
        .next()
        .and_then(|t| t.strip_prefix('&'))
        .and_then(|t| t.parse().ok())
        .ok_or_else(bad)?;
    let stripped = parts.next() == Some("stripped");
    let value = value::parse(body).map_err(|e| Error::decode(e.message).at_line(line + e.line))?;
    let Value::Map(mut entries) = value else {
        return Err(Error::decode("an object's body must be `TypeName:` and its fields").at_line(line));
    };
    if entries.len() != 1 {
        return Err(Error::decode(format!(
            "an object has one type name, got {} top-level keys",
            entries.len()
        ))
        .at_line(line));
    }
    let (type_name, body) = entries.pop().expect("one entry");
    Ok(UnityObject {
        class_id,
        file_id,
        stripped,
        type_name,
        body,
    })
}
