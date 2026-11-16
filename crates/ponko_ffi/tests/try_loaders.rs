//! The `try_` loaders and the error path: failures with their kind and
//! asset path, never a caught panic.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::CStr;
use std::ptr::null_mut;

use PonkoErrorKind::*;
use PonkoStatus::*;

#[test]
fn missing_files_are_not_found_with_their_path() {
    run_ctx(|ctx| unsafe {
        let mut texture = PonkoTexture {
            id: 7,
            width: 0,
            height: 0,
        };
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"no/such.png".as_ptr(), &mut texture),
            PONKO_NOT_FOUND
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_NOT_FOUND);
        assert_eq!(last_path(), "no/such.png");
        assert_eq!(texture.id, 7, "the output is unchanged on failure");

        let mut font = PonkoFont { id: 0 };
        assert_eq!(
            ponko_ctx_try_load_font(ctx, c"no/such.ttf".as_ptr(), &mut font),
            PONKO_NOT_FOUND
        );
        assert_eq!(last_path(), "no/such.ttf");
        let mut sound = PonkoSound { id: 0, seconds: 0.0 };
        assert_eq!(
            ponko_ctx_try_load_sound(ctx, c"no/such.ogg".as_ptr(), &mut sound),
            PONKO_NOT_FOUND
        );
        assert_eq!(last_path(), "no/such.ogg");
        assert_eq!(
            ponko_ctx_try_load_stream(ctx, c"no/music.ogg".as_ptr(), &mut sound),
            PONKO_NOT_FOUND
        );
        assert_eq!(last_path(), "no/music.ogg");
        let mut mesh = PonkoMesh { id: 0 };
        assert_eq!(
            ponko_ctx_try_load_mesh(ctx, c"no/such.glb".as_ptr(), &mut mesh),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            (ponko_last_error_kind(), last_path().as_str()),
            (PONKO_ERROR_NOT_FOUND, "no/such.glb")
        );
    });
}

#[test]
fn files_that_do_not_decode_are_decode_errors() {
    run_ctx(|ctx| unsafe {
        let source = ponko_memory_source_new(c"m".as_ptr());
        let junk = b"not an image";
        assert_eq!(
            ponko_memory_source_insert(source, c"bad.png".as_ptr(), junk.as_ptr(), junk.len()),
            PONKO_OK
        );
        assert_eq!(
            ponko_memory_source_insert(source, c"good.png".as_ptr(), PIXEL_PNG.as_ptr(), PIXEL_PNG.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        let mut texture = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"bad.png".as_ptr(), &mut texture),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_DECODE);
        assert_eq!(last_path(), "bad.png");
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"good.png".as_ptr(), &mut texture),
            PONKO_OK
        );
        assert_eq!((texture.width, texture.height), (1, 1));
        assert_eq!(last_path(), "bad.png", "a success leaves the last error alone");
    });
}

#[test]
fn bad_arguments_have_no_path() {
    unsafe {
        let mut texture = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(
            ponko_ctx_try_load_texture(null_mut(), c"a.png".as_ptr(), &mut texture),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_INVALID);
        assert_eq!(last_path(), "");
    }
    run_ctx(|ctx| unsafe {
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"a.png".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_try_load_texture(
                ctx,
                std::ptr::null(),
                &mut PonkoTexture {
                    id: 0,
                    width: 0,
                    height: 0
                }
            ),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn error_kinds_have_names() {
    let name = |k: u32| unsafe { CStr::from_ptr(ponko_error_kind_name(k)) }.to_str().unwrap();
    assert_eq!(name(PONKO_ERROR_NONE as u32), "none");
    assert_eq!(name(PONKO_ERROR_NOT_FOUND as u32), "not found");
    assert_eq!(name(PONKO_ERROR_SHADER as u32), "shader");
    assert_eq!(name(PONKO_ERROR_OTHER as u32), "other");
    assert_eq!(name(999), "other", "a kind this library doesn't know");
}

/// C has no panics: the data-file loaders are the `try_` forms, so a missing
/// or broken file is a failed status with its kind and path.
#[test]
fn data_file_loaders_report_a_kind_and_path_for_missing_and_broken_files() {
    run_ctx(|ctx| unsafe {
        let source = ponko_memory_source_new(c"m".as_ptr());
        for (name, body) in [
            ("broken.json", &b"{"[..]),
            ("table.json", br#"{"language":"en","strings":{"a":"A"}}"#),
        ] {
            let name = std::ffi::CString::new(name).unwrap();
            assert_eq!(
                ponko_memory_source_insert(source, name.as_ptr(), body.as_ptr(), body.len()),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);

        assert_eq!(ponko_ctx_load_strings(ctx, c"lang/none.json".as_ptr()), PONKO_NOT_FOUND);
        assert_eq!(
            (ponko_last_error_kind(), last_path().as_str()),
            (PONKO_ERROR_NOT_FOUND, "lang/none.json")
        );
        assert_ne!(ponko_ctx_load_strings(ctx, c"broken.json".as_ptr()), PONKO_OK);
        assert_eq!(
            (ponko_last_error_kind(), last_path().as_str()),
            (PONKO_ERROR_JSON, "broken.json")
        );
        assert_eq!(ponko_ctx_load_strings(ctx, c"table.json".as_ptr()), PONKO_OK);

        assert_eq!(
            ponko_ctx_load_resource_json(ctx, c"no/such.json".as_ptr(), c"Anything".as_ptr()),
            PONKO_INVALID_ARGUMENT,
            "the older reflected loaders keep their status; the kind and path say what failed"
        );
        assert_eq!(
            (ponko_last_error_kind(), last_path().as_str()),
            (PONKO_ERROR_NOT_FOUND, "no/such.json")
        );
    });
}

#[cfg(feature = "physics3d")]
#[test]
fn a_missing_collision_file_is_not_found_with_its_path() {
    run_ctx(|ctx| unsafe {
        let one = PonkoVec3 { x: 1.0, y: 1.0, z: 1.0 };
        let mut mesh = Default::default();
        assert_eq!(
            ponko_ctx_load_collision_mesh(ctx, c"no/such.glb".as_ptr(), one, false, &mut mesh),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            (ponko_last_error_kind(), last_path().as_str()),
            (PONKO_ERROR_NOT_FOUND, "no/such.glb")
        );
    });
}
