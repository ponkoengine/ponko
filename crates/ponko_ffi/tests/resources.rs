//! Resources set, read, removed and described from C.
use ponko_ffi::PonkoFieldType::*;
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;

use PonkoStatus::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Score {
    red: u32,
    blue: u32,
}
const SIZE: usize = size_of::<Score>();
const ALIGN: usize = align_of::<Score>();

fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}
unsafe fn set(w: *mut PonkoWorld, score: Score) -> PonkoStatus {
    unsafe { ponko_resource_set(w, c"Score".as_ptr(), (&raw const score).cast(), SIZE, ALIGN) }
}
unsafe fn get(w: *mut PonkoWorld) -> Result<Score, PonkoStatus> {
    let mut out = Score::default();
    match unsafe { ponko_resource_get(w, c"Score".as_ptr(), (&raw mut out).cast(), SIZE) } {
        PONKO_OK => Ok(out),
        status => Err(status),
    }
}
fn score_fields() -> [PonkoField; 2] {
    [
        PonkoField {
            name: c"red".as_ptr(),
            kind: PONKO_FIELD_U32 as u32,
            offset: 0,
        },
        PonkoField {
            name: c"blue".as_ptr(),
            kind: PONKO_FIELD_U32 as u32,
            offset: 4,
        },
    ]
}
fn save(w: *mut PonkoWorld) -> serde_json::Value {
    let mut len = 0;
    unsafe {
        assert_eq!(ponko_world_save(w, null_mut(), 0, &mut len), PONKO_OK);
        let mut buf = vec![0u8; len + 1];
        assert_eq!(
            ponko_world_save(w, buf.as_mut_ptr().cast(), buf.len(), &mut len),
            PONKO_OK
        );
        serde_json::from_slice(&buf[..len]).unwrap()
    }
}

#[test]
fn resources_are_set_read_replaced_and_removed_by_name() {
    let w = ponko_world_new();
    unsafe {
        assert_eq!(get(w), Err(PONKO_NOT_FOUND));
        assert_eq!(set(w, Score { red: 1, blue: 2 }), PONKO_OK);
        assert_eq!(get(w), Ok(Score { red: 1, blue: 2 }));
        assert_eq!(set(w, Score { red: 5, blue: 2 }), PONKO_OK);
        assert_eq!(get(w), Ok(Score { red: 5, blue: 2 }));
        assert_eq!(ponko_resource_remove(w, c"Score".as_ptr()), PONKO_OK);
        assert_eq!(get(w), Err(PONKO_NOT_FOUND));
        assert_eq!(ponko_resource_remove(w, c"Score".as_ptr()), PONKO_NOT_FOUND);
        ponko_world_free(w);
    }
}

#[test]
fn a_resource_keeps_its_size() {
    let w = ponko_world_new();
    unsafe {
        assert_eq!(set(w, Score::default()), PONKO_OK);
        let small: u32 = 3;
        let status = ponko_resource_set(w, c"Score".as_ptr(), (&raw const small).cast(), 4, 4);
        assert_eq!(status, PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("different size"), "{}", last_error());
        let mut out = 0u32;
        let status = ponko_resource_get(w, c"Score".as_ptr(), (&raw mut out).cast(), 4);
        assert_eq!(status, PONKO_INVALID_ARGUMENT);
        assert_eq!(get(w), Ok(Score::default()));
        ponko_world_free(w);
    }
}

#[test]
fn bad_resource_arguments_are_errors_not_crashes() {
    let w = ponko_world_new();
    let value = 0u32;
    let data: *const c_void = (&raw const value).cast();
    unsafe {
        assert_eq!(
            ponko_resource_set(w, std::ptr::null(), data, 4, 4),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_resource_set(w, c"".as_ptr(), data, 4, 4), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_resource_set(w, c"X".as_ptr(), std::ptr::null(), 4, 4),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_resource_set(w, c"X".as_ptr(), data, 4, 3), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_resource_set(w, c"X".as_ptr(), data, 2, 4), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_resource_set(null_mut(), c"X".as_ptr(), data, 4, 4),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_resource_get(w, c"X".as_ptr(), null_mut(), 4),
            PONKO_INVALID_ARGUMENT
        );
        ponko_world_free(w);
    }
}

#[test]
fn zero_size_resources_work_as_flags() {
    let w = ponko_world_new();
    let nothing = ();
    unsafe {
        let status = ponko_resource_set(w, c"Paused".as_ptr(), (&raw const nothing).cast(), 0, 1);
        assert_eq!(status, PONKO_OK);
        let mut out = ();
        assert_eq!(
            ponko_resource_get(w, c"Paused".as_ptr(), (&raw mut out).cast(), 0),
            PONKO_OK
        );
        assert_eq!(ponko_resource_remove(w, c"Paused".as_ptr()), PONKO_OK);
        ponko_world_free(w);
    }
}

#[test]
fn described_resources_are_saved_and_loaded() {
    let w = ponko_world_new();
    let fields = score_fields();
    unsafe {
        let status = ponko_resource_reflect(w, c"Score".as_ptr(), SIZE, ALIGN, fields.as_ptr(), fields.len());
        assert_eq!(status, PONKO_OK, "{}", last_error());
        assert_eq!(set(w, Score { red: 3, blue: 9 }), PONKO_OK);
        let saved = save(w);
        assert_eq!(
            saved["resources"]["Score"],
            serde_json::json!({ "red": 3, "blue": 9 }),
            "{saved}"
        );

        assert_eq!(set(w, Score::default()), PONKO_OK);
        let text = std::ffi::CString::new(saved.to_string()).unwrap();
        assert_eq!(ponko_world_load(w, text.as_ptr()), PONKO_OK, "{}", last_error());
        assert_eq!(get(w), Ok(Score { red: 3, blue: 9 }));
        ponko_world_free(w);
    }
}

#[test]
fn the_world_hash_follows_described_resources() {
    let w = ponko_world_new();
    let fields = score_fields();
    let hash = |w| {
        let mut h = 0u64;
        assert_eq!(unsafe { ponko_world_hash(w, &mut h) }, PONKO_OK);
        h
    };
    unsafe {
        assert_eq!(
            ponko_resource_reflect(w, c"Score".as_ptr(), SIZE, ALIGN, fields.as_ptr(), fields.len()),
            PONKO_OK
        );
        assert_eq!(set(w, Score { red: 3, blue: 9 }), PONKO_OK);
        let first = hash(w);
        assert_eq!(hash(w), first, "reading changes nothing");
        assert_eq!(set(w, Score { red: 4, blue: 9 }), PONKO_OK);
        assert_ne!(hash(w), first);
        assert_eq!(set(w, Score { red: 3, blue: 9 }), PONKO_OK);
        assert_eq!(hash(w), first);
        assert_eq!(ponko_world_hash(w, std::ptr::null_mut()), PONKO_INVALID_ARGUMENT);
        let mut h = 0;
        assert_eq!(ponko_world_hash(std::ptr::null_mut(), &mut h), PONKO_INVALID_ARGUMENT);
        ponko_world_free(w);
    }
}

#[test]
fn resource_descriptions_must_fit() {
    let w = ponko_world_new();
    let fields = score_fields();
    unsafe {
        let status = ponko_resource_reflect(w, c"Score".as_ptr(), 4, 4, fields.as_ptr(), fields.len());
        assert_eq!(status, PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("doesn't fit in the resource"), "{}", last_error());
        let twice = [fields[0], fields[0]];
        let status = ponko_resource_reflect(w, c"Score".as_ptr(), SIZE, ALIGN, twice.as_ptr(), 2);
        assert_eq!(status, PONKO_INVALID_ARGUMENT);
        assert_eq!(set(w, Score::default()), PONKO_OK);
        let status = ponko_resource_reflect(w, c"Score".as_ptr(), 16, 4, fields.as_ptr(), fields.len());
        assert_eq!(status, PONKO_INVALID_ARGUMENT);
        ponko_world_free(w);
    }
}
