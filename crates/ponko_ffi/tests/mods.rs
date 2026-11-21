//! Data-only mods from C: mounting packs in a load order, the refusal of a
//! bad set (status, message, path), and reading what is mounted.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use PonkoErrorKind::*;
use PonkoStatus::*;

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(c & 1));
        }
    }
    !c
}

/// A pack as `ponko pack` writes it (version 1, stored).
fn pack(files: &[(&str, &str)]) -> Vec<u8> {
    let mut files = files.to_vec();
    files.sort();
    let (mut index, mut data) = (Vec::new(), Vec::new());
    for (name, bytes) in &files {
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(name.as_bytes());
        index.push(0);
        index.extend_from_slice(&(data.len() as u64).to_le_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&crc32(bytes.as_bytes()).to_le_bytes());
        data.extend_from_slice(bytes.as_bytes());
    }
    let mut out = b"PONKOPAK".to_vec();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}

fn manifest(id: &str, requires: &str) -> String {
    format!(r#"{{"format":"ponko-mod","version":1,"id":"{id}","requires":[{requires}]}}"#)
}

fn write(dir: &std::path::Path, name: &str, files: &[(&str, &str)]) -> CString {
    let path = dir.join(name);
    std::fs::write(&path, pack(files)).unwrap();
    CString::new(path.to_str().unwrap()).unwrap()
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-mods-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads text through a `(buffer, capacity, out_len)` call: length first, then the text.
fn read(f: impl Fn(*mut c_char, usize, *mut usize) -> PonkoStatus) -> Result<String, PonkoStatus> {
    let mut len = 0;
    let status = f(null_mut(), 0, &mut len);
    if status != PONKO_OK {
        return Err(status);
    }
    let mut buf = vec![0u8; len + 1];
    assert_eq!(f(buf.as_mut_ptr().cast(), buf.len(), &mut len), PONKO_OK);
    buf.truncate(len);
    Ok(String::from_utf8(buf).unwrap())
}

unsafe extern "C" fn keep(user: *mut c_void, data: *const u8, len: usize) -> u32 {
    // SAFETY: `user` is the test's Vec, `data` holds `len` bytes.
    unsafe { *user.cast::<Vec<u8>>() = std::slice::from_raw_parts(data, len).to_vec() };
    PONKO_OK as u32
}

#[test]
fn mods_mount_in_load_order_and_the_later_mod_wins_by_path() {
    let dir = temp("order");
    let base = write(
        &dir,
        "base.pack",
        &[
            ("mod.json", &manifest("base", "")),
            ("rules.txt", "base"),
            ("enemies/bat.json", "{}"),
        ],
    );
    let strong = write(
        &dir,
        "strong.pack",
        &[
            ("mod.json", &manifest("strong", r#""base""#)),
            ("rules.txt", "strong"),
            ("enemies/rat.json", "{}"),
        ],
    );
    run_ctx(|ctx| unsafe {
        let mut n = 7;
        assert_eq!(ponko_ctx_mod_count(ctx, &mut n), PONKO_OK);
        assert_eq!(n, 0, "nothing mounted yet");
        let mut f = 7;
        assert_eq!(ponko_ctx_mods_fingerprint(ctx, &mut f), PONKO_OK);
        assert_eq!(f, 0);
        // Added "strong" first, but it requires "base".
        let paths = [strong.as_ptr(), base.as_ptr()];
        assert_eq!(
            ponko_ctx_mount_mods(ctx, paths.as_ptr(), 2),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(ponko_ctx_mod_count(ctx, &mut n), PONKO_OK);
        assert_eq!(n, 2);
        let ids: Vec<String> = (0..2)
            .map(|i| read(|b, c, l| ponko_ctx_mod_id(ctx, i, b, c, l)).unwrap())
            .collect();
        assert_eq!(ids, ["base", "strong"]);
        assert_eq!(read(|b, c, l| ponko_ctx_mod_id(ctx, 2, b, c, l)), Err(PONKO_NOT_FOUND));
        assert_eq!(
            read(|b, c, l| ponko_ctx_mod_provider(ctx, c"rules.txt".as_ptr(), b, c, l)).unwrap(),
            "strong"
        );
        assert_eq!(
            read(|b, c, l| ponko_ctx_mod_provider(ctx, c"crab.png".as_ptr(), b, c, l)),
            Err(PONKO_NOT_FOUND)
        );
        let under: Vec<String> = (0..)
            .map_while(|i| read(|b, c, l| ponko_ctx_mod_file_under(ctx, c"enemies/".as_ptr(), i, b, c, l)).ok())
            .collect();
        assert_eq!(under, ["enemies/bat.json", "enemies/rat.json"]);
        let mut bytes: Vec<u8> = Vec::new();
        assert_eq!(
            ponko_ctx_load_asset_with(ctx, c"rules.txt".as_ptr(), Some(keep), (&raw mut bytes).cast()),
            PONKO_OK
        );
        assert_eq!(bytes, b"strong");
        assert_eq!(ponko_ctx_mods_fingerprint(ctx, &mut f), PONKO_OK);
        assert_ne!(f, 0);
        // Once per game.
        assert_eq!(ponko_ctx_mount_mods(ctx, paths.as_ptr(), 1), PONKO_INVALID_ARGUMENT);
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bad_mod_is_refused_whole_naming_the_mod_the_file_and_the_reason() {
    let dir = temp("refused");
    let good = write(
        &dir,
        "good.pack",
        &[("mod.json", &manifest("good", "")), ("rules.txt", "good")],
    );
    let ghost = write(
        &dir,
        "ghost.pack",
        &[
            ("mod.json", &manifest("ghost", "")),
            (
                "prefabs/ghost.json",
                r#"{"format":"ponko-prefab","version":1,"components":{"Ghost":{}}}"#,
            ),
        ],
    );
    let lonely = write(&dir, "lonely.pack", &[("mod.json", &manifest("lonely", r#""nobody""#))]);
    let sneaky = write(
        &dir,
        "sneaky.pack",
        &[("mod.json", &manifest("sneaky", "")), ("../escape.txt", "x")],
    );
    let missing = CString::new(dir.join("missing.pack").to_str().unwrap()).unwrap();
    run_ctx(|ctx| unsafe {
        let paths = [good.as_ptr(), ghost.as_ptr()];
        assert_eq!(ponko_ctx_mount_mods(ctx, paths.as_ptr(), 2), PONKO_NOT_FOUND);
        assert_eq!(
            last_error(),
            "mod \"ghost\", file \"prefabs/ghost.json\": component \"Ghost\" isn't one this game has (not reflected)"
        );
        assert_eq!(last_path(), "prefabs/ghost.json");
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_NOT_FOUND);
        let mut n = 7;
        assert_eq!(ponko_ctx_mod_count(ctx, &mut n), PONKO_OK);
        assert_eq!(n, 0, "the good mod isn't mounted either");

        assert_eq!(
            ponko_ctx_mount_mods(ctx, [lonely.as_ptr()].as_ptr(), 1),
            PONKO_NOT_FOUND
        );
        assert!(last_error().contains("requires mod \"nobody\""), "{}", last_error());

        assert_eq!(
            ponko_ctx_mount_mods(ctx, [sneaky.as_ptr()].as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert!(
            last_error().starts_with("mod \"sneaky\", file \"../escape.txt\": bad file name"),
            "{}",
            last_error()
        );

        assert_eq!(
            ponko_ctx_mount_mods(ctx, [missing.as_ptr()].as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert!(
            last_error().starts_with("mod \"missing.pack\": can't be read"),
            "{}",
            last_error()
        );

        assert_eq!(ponko_ctx_mount_mods(ctx, null(), 1), PONKO_INVALID_ARGUMENT);
        // The game still mounts a good set afterwards.
        assert_eq!(ponko_ctx_mount_mods(ctx, [good.as_ptr()].as_ptr(), 1), PONKO_OK);
        assert_eq!(ponko_ctx_mod_count(ctx, &mut n), PONKO_OK);
        assert_eq!(n, 1);
    });
    let _ = std::fs::remove_dir_all(dir);
}
