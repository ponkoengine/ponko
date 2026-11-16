//! Errors can be matched on: a kind, the file, the line and the bare message,
//! with the `std::io::Error` kept as the source.

use ponko_unity::*;
use std::error::Error as _;
use std::path::Path;

/// A path no test creates.
fn missing() -> std::path::PathBuf {
    std::env::temp_dir().join("ponko_unity_errors_test_no_such_file.unity")
}

#[test]
fn a_missing_file_is_an_io_error_naming_the_file_and_keeping_its_source() {
    let path = missing();
    let err = UnityFile::read(&path).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert_eq!(err.path(), Some(path.as_path()));
    assert_eq!(err.line(), None);
    let source = err.source().expect("the I/O error is the source");
    let io = source.downcast_ref::<std::io::Error>().expect("a std::io::Error");
    assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
    // The text names the file once, and `message` is only the cause.
    assert!(err.to_string().starts_with(&format!("{}: ", path.display())), "{err}");
    assert!(!err.message().contains(path.to_str().unwrap()), "{}", err.message());
}

#[test]
fn a_missing_clip_file_is_an_io_error_too() {
    let err = AnimationClip::read(missing()).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert_eq!(err.path(), Some(missing().as_path()));
    assert!(err.source().is_some());
}

#[test]
fn a_directory_that_is_not_a_project_is_invalid_and_names_the_directory() {
    let err = Project::open(missing()).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Invalid);
    assert_eq!(err.path(), Some(missing().as_path()));
    assert!(err.source().is_none());
}

#[test]
fn bad_yaml_is_a_decode_error_with_the_line() {
    let err = UnityFile::parse("--- !u!1 &100\nGameObject:\n  a: [1, 2\n").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert!(err.line().is_some_and(|l| l >= 3), "{err}");
    assert_eq!(err.path(), None);
    assert!(err.source().is_none());
    assert!(!err.message().starts_with("line "), "{}", err.message());
    assert!(
        err.to_string().starts_with(&format!("line {}: ", err.line().unwrap())),
        "{err}"
    );
}

#[test]
fn a_bad_document_header_is_a_decode_error_on_its_own_line() {
    let err = UnityFile::parse("--- !u!1 &100\nGameObject: {}\n--- nonsense\nX: 1\n").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert_eq!(err.line(), Some(3));
}

#[test]
fn text_that_is_not_a_unity_file_is_a_decode_error() {
    let err = UnityFile::parse("hello\n").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert_eq!(err.line(), Some(1));
}

#[test]
fn a_file_with_bad_yaml_names_the_file_and_the_line() {
    let dir = std::env::temp_dir().join("ponko_unity_errors_test_bad_yaml");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.asset");
    std::fs::write(&path, "--- !u!1 &100\nGameObject:\n  a: [1, 2\n").unwrap();
    let err = UnityFile::read(&path).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert_eq!(err.path(), Some(path.as_path()));
    assert!(err.line().is_some());
    assert!(err.source().is_none());
}

/// The start of a binary serialized file with this format version.
fn binary_header(version: u32) -> Vec<u8> {
    let mut data = vec![0u8; 64];
    data[8..12].copy_from_slice(&version.to_be_bytes());
    data
}

#[test]
fn an_old_binary_version_is_unsupported() {
    let err = UnityFile::parse_binary(&binary_header(21)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert!(err.message().contains("older than Unity 2022"), "{err}");
    assert_eq!(err.path(), None);
    assert_eq!(err.line(), None);
}

#[test]
fn a_truncated_binary_asset_is_a_decode_error() {
    let err = UnityFile::parse_binary(&binary_header(22)[..20]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
    let err = AnimationClip::parse_binary(&[1, 2, 3]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
}

#[test]
fn a_compressed_texture_is_unsupported_not_invalid() {
    let file = UnityFile::parse(
        "--- !u!28 &1\nTexture2D:\n  m_Name: t\n  m_TextureFormat: 10\n  m_Width: 4\n  m_Height: 4\n  image data: 8\n",
    )
    .unwrap();
    let err = UnityTexture::from_object(&file.objects[0]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported);
}

#[test]
fn an_object_of_the_wrong_class_is_invalid() {
    let file = UnityFile::parse("--- !u!1 &1\nGameObject:\n  m_Name: a\n").unwrap();
    let err = UnityMaterial::from_object(&file.objects[0]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Invalid);
}

#[test]
fn a_prefab_instance_without_a_source_is_invalid() {
    let file = UnityFile::parse(
        "--- !u!1001 &9\nPrefabInstance:\n  m_Modification:\n    m_TransformParent: {fileID: 0}\n    m_Modifications: []\n",
    )
    .unwrap();
    let dir = std::env::temp_dir().join("ponko_unity_errors_test_project");
    std::fs::create_dir_all(dir.join("Assets")).unwrap();
    let project = Project::open(&dir).unwrap();
    let result = Hierarchy::build(&file, &project);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(result.unwrap_err().kind(), ErrorKind::Invalid);
}

#[test]
fn errors_compare_and_clone_without_the_io_source() {
    let a = UnityFile::read(missing()).unwrap_err();
    let b = a.clone();
    assert_eq!(a, b);
    assert!(b.source().is_some());
    assert_ne!(a, UnityFile::parse("x\n").unwrap_err());
}

#[test]
fn error_kinds_can_be_matched_with_a_wildcard() {
    let kind = UnityFile::parse("x\n").unwrap_err().kind();
    let what = match kind {
        ErrorKind::Io => "io",
        ErrorKind::Decode => "decode",
        _ => "other",
    };
    assert_eq!(what, "decode");
    let _: &Path = missing().as_path();
}
