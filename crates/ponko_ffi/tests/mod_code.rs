//! Code mods from C (`ponko_ctx_mod_function`, `ponko_ctx_mod_call` and the
//! host functions' expose calls): with the `mods-wasm` feature, calls with
//! numbers under the sandbox's fuel limit and the host functions reaching
//! only what the game exposed; without it, every call says code mods are
//! not compiled in.
mod common;
use common::*;
use ponko_ffi::*;
use std::ptr::{null, null_mut};

use PonkoModValueKind::*;
use PonkoStatus::*;

/// A module (from WebAssembly text, in the guide's "Code mods") exporting
/// `add(i32, i32) -> i32`, `half(f64) -> f64`, `spin()` (loops forever),
/// `turn(entity: i64, radians: f64) -> i32` (`set_field` of
/// `"Transform2D.rotation"`), `spawn(x, y: f32) -> i64` (`spawn_prefab` of
/// `"prefabs/bat.json"`) and `random() -> f32`.
pub const MODULE: [u8; LEN] = [
    0, 97, 115, 109, 1, 0, 0, 0, 1, 47, 8, 96, 4, 126, 127, 127, 124, 1, 127, 96, 4, 127, 127, 125, 125, 1, 126, 96, 0,
    1, 125, 96, 2, 127, 127, 1, 127, 96, 1, 124, 1, 124, 96, 0, 0, 96, 2, 126, 124, 1, 127, 96, 2, 125, 125, 1, 126, 2,
    55, 3, 5, 112, 111, 110, 107, 111, 9, 115, 101, 116, 95, 102, 105, 101, 108, 100, 0, 0, 5, 112, 111, 110, 107, 111,
    12, 115, 112, 97, 119, 110, 95, 112, 114, 101, 102, 97, 98, 0, 1, 5, 112, 111, 110, 107, 111, 6, 114, 97, 110, 100,
    111, 109, 0, 2, 3, 7, 6, 3, 4, 5, 6, 7, 2, 5, 3, 1, 0, 1, 7, 54, 7, 6, 109, 101, 109, 111, 114, 121, 2, 0, 3, 97,
    100, 100, 0, 3, 4, 104, 97, 108, 102, 0, 4, 4, 115, 112, 105, 110, 0, 5, 4, 116, 117, 114, 110, 0, 6, 5, 115, 112,
    97, 119, 110, 0, 7, 6, 114, 97, 110, 100, 111, 109, 0, 8, 10, 63, 6, 7, 0, 32, 0, 32, 1, 106, 11, 14, 0, 32, 0, 68,
    0, 0, 0, 0, 0, 0, 224, 63, 162, 11, 7, 0, 3, 64, 12, 0, 11, 11, 12, 0, 32, 0, 65, 0, 65, 20, 32, 1, 16, 0, 11, 12,
    0, 65, 32, 65, 16, 32, 0, 32, 1, 16, 1, 11, 4, 0, 16, 2, 11, 11, 47, 2, 0, 65, 0, 11, 20, 84, 114, 97, 110, 115,
    102, 111, 114, 109, 50, 68, 46, 114, 111, 116, 97, 116, 105, 111, 110, 0, 65, 32, 11, 16, 112, 114, 101, 102, 97,
    98, 115, 47, 98, 97, 116, 46, 106, 115, 111, 110, 0, 36, 4, 110, 97, 109, 101, 1, 21, 3, 0, 3, 115, 101, 116, 1, 5,
    115, 112, 97, 119, 110, 2, 6, 114, 97, 110, 100, 111, 109, 3, 6, 1, 5, 1, 0, 1, 108,
];
const LEN: usize = 336;

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
fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
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
        index.extend_from_slice(&crc32(bytes).to_le_bytes());
        data.extend_from_slice(bytes);
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

const BAT: &str = r#"{"format":"ponko-prefab","version":1,"components":{
    "Transform2D":{"position":[0.0,0.0],"rotation":0.0,"scale":[1.0,1.0]}}}"#;

/// The "crawler" mod: [`MODULE`] and a prefab.
fn crawler() -> Vec<u8> {
    let manifest = r#"{"format":"ponko-mod","version":1,"id":"crawler","code":"code.wasm"}"#;
    pack(&[
        ("mod.json", manifest.as_bytes()),
        ("code.wasm", &MODULE),
        ("prefabs/bat.json", BAT.as_bytes()),
    ])
}

/// Mounts the crawler from memory; the status.
unsafe fn mount(ctx: *mut PonkoCtx) -> PonkoStatus {
    let bytes = crawler();
    let entry = PonkoModBytes {
        name: c"crawler.pack".as_ptr(),
        bytes: bytes.as_ptr(),
        len: bytes.len(),
    };
    unsafe { ponko_ctx_mount_mods_from_memory(ctx, &entry, 1) }
}

#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
fn i32v(i: i32) -> PonkoModValue {
    PonkoModValue {
        kind: PONKO_MOD_I32,
        i: i.into(),
        f: 0.0,
    }
}
#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
fn i64v(i: i64) -> PonkoModValue {
    PonkoModValue {
        kind: PONKO_MOD_I64,
        i,
        f: 0.0,
    }
}
#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
fn f32v(f: f32) -> PonkoModValue {
    PonkoModValue {
        kind: PONKO_MOD_F32,
        i: 0,
        f: f.into(),
    }
}
#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
fn f64v(f: f64) -> PonkoModValue {
    PonkoModValue {
        kind: PONKO_MOD_F64,
        i: 0,
        f,
    }
}

/// Calls `f` with `args`; its result, if any.
#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
unsafe fn call(
    ctx: *mut PonkoCtx,
    f: PonkoModFn,
    args: &[PonkoModValue],
) -> Result<Option<PonkoModValue>, PonkoStatus> {
    let mut out = f32v(0.0);
    let mut has = false;
    match unsafe { ponko_ctx_mod_call(ctx, f, args.as_ptr(), args.len(), &mut out, &mut has) } {
        PONKO_OK => Ok(has.then_some(out)),
        status => Err(status),
    }
}

#[cfg_attr(not(feature = "mods-wasm"), allow(dead_code))]
unsafe fn function(ctx: *mut PonkoCtx, name: &std::ffi::CStr) -> PonkoModFn {
    let mut f = PonkoModFn::default();
    assert_eq!(
        unsafe { ponko_ctx_mod_function(ctx, c"crawler".as_ptr(), name.as_ptr(), &mut f) },
        PONKO_OK,
        "{}",
        last_error()
    );
    f
}

#[cfg(feature = "mods-wasm")]
#[test]
fn c_calls_a_mod_function_by_name_with_numbers_and_gets_its_result() {
    run_ctx(|ctx| unsafe {
        let mut f = PonkoModFn::default();
        assert_eq!(
            ponko_ctx_mod_function(ctx, c"crawler".as_ptr(), c"add".as_ptr(), &mut f),
            PONKO_NOT_FOUND,
            "nothing mounted"
        );
        assert_eq!(mount(ctx), PONKO_OK, "{}", last_error());
        let add = function(ctx, c"add");
        assert_eq!(function(ctx, c"add"), add, "looked up again: the same handle");
        assert_eq!(call(ctx, add, &[i32v(40), i32v(2)]), Ok(Some(i32v(42))));
        let half = function(ctx, c"half");
        assert_eq!(call(ctx, half, &[f64v(5.0)]), Ok(Some(f64v(2.5))));
        // Arguments that don't fit, a missing function, a handle this game didn't find.
        assert_eq!(call(ctx, add, &[f64v(1.0)]), Err(PONKO_INVALID_ARGUMENT));
        assert!(last_error().contains("crawler"), "{}", last_error());
        assert_eq!(
            call(ctx, add, &[i64v(1 << 40), i32v(0)]).map(|_| ()),
            Err(PONKO_INVALID_ARGUMENT)
        );
        assert_eq!(
            ponko_ctx_mod_function(ctx, c"crawler".as_ptr(), c"fly".as_ptr(), &mut f),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_mod_function(ctx, c"nobody".as_ptr(), c"add".as_ptr(), &mut f),
            PONKO_NOT_FOUND
        );
        assert_eq!(call(ctx, PonkoModFn { id: 99 }, &[]), Err(PONKO_NOT_FOUND));
        assert_eq!(
            ponko_ctx_mod_call(ctx, add, null(), 2, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[cfg(feature = "mods-wasm")]
#[test]
fn a_mod_function_called_from_c_is_stopped_by_the_same_fuel_limit_and_stays_callable() {
    run_ctx(|ctx| unsafe {
        assert_eq!(mount(ctx), PONKO_OK, "{}", last_error());
        let spin = function(ctx, c"spin");
        assert_eq!(call(ctx, spin, &[]), Err(PONKO_INVALID_ARGUMENT));
        assert!(last_error().contains("fuel"), "{}", last_error());
        let add = function(ctx, c"add");
        assert_eq!(call(ctx, add, &[i32v(1), i32v(2)]), Ok(Some(i32v(3))));
    });
}

#[cfg(feature = "mods-wasm")]
#[test]
fn from_c_the_host_functions_reach_only_the_entities_and_prefabs_the_game_exposed() {
    run_ctx(|ctx| unsafe {
        assert_eq!(mount(ctx), PONKO_OK, "{}", last_error());
        let w = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        let t = PonkoTransform2D::default();
        let v = PonkoValue {
            component: transform,
            data: (&raw const t).cast(),
            size: size_of::<PonkoTransform2D>(),
        };
        let mut e = 0;
        assert_eq!(ponko_spawn(w, &v, 1, &mut e), PONKO_OK);
        let rotation = |e: u64| {
            let mut t = PonkoTransform2D::default();
            assert_eq!(
                ponko_get(w, e, transform, (&raw mut t).cast(), size_of::<PonkoTransform2D>()),
                PONKO_OK
            );
            t.rotation
        };
        let turn = function(ctx, c"turn");
        assert_eq!(
            call(ctx, turn, &[i64v(e as i64), f64v(0.5)]),
            Ok(Some(i32v(-1))),
            "not exposed"
        );
        assert_eq!(rotation(e), 0.0);
        assert_eq!(ponko_ctx_mod_expose_entity(ctx, e), PONKO_OK);
        assert_eq!(call(ctx, turn, &[i64v(e as i64), f64v(0.5)]), Ok(Some(i32v(0))));
        assert_eq!(rotation(e), 0.5);
        let mut was = false;
        assert_eq!(ponko_ctx_mod_hide_entity(ctx, e, &mut was), PONKO_OK);
        assert!(was);
        assert_eq!(call(ctx, turn, &[i64v(e as i64), f64v(1.0)]), Ok(Some(i32v(-1))));
        assert_eq!(rotation(e), 0.5);

        let spawn = function(ctx, c"spawn");
        assert_eq!(
            call(ctx, spawn, &[f32v(3.0), f32v(4.0)]),
            Ok(Some(i64v(-1))),
            "prefab not exposed"
        );
        assert_eq!(ponko_ctx_mod_expose_prefab(ctx, c"prefabs/bat.json".as_ptr()), PONKO_OK);
        let bat = call(ctx, spawn, &[f32v(3.0), f32v(4.0)]).unwrap().unwrap().i;
        assert!(bat >= 0);
        assert_eq!(
            call(ctx, turn, &[i64v(bat), f64v(2.0)]),
            Ok(Some(i32v(0))),
            "its own spawn is exposed"
        );
        assert_eq!(rotation(bat as u64), 2.0);
    });
}

#[cfg(feature = "mods-wasm")]
#[test]
fn from_c_the_mods_random_numbers_follow_the_seed() {
    run_ctx(|ctx| unsafe {
        assert_eq!(mount(ctx), PONKO_OK, "{}", last_error());
        let random = function(ctx, c"random");
        let draw = || {
            (0..16)
                .map(|_| call(ctx, random, &[]).unwrap().unwrap().f)
                .collect::<Vec<_>>()
        };
        assert_eq!(ponko_ctx_mod_seed_random(ctx, 9), PONKO_OK);
        let first = draw();
        assert!(first.iter().all(|v| (0.0..1.0).contains(v)));
        assert_eq!(ponko_ctx_mod_seed_random(ctx, 9), PONKO_OK);
        assert_eq!(draw(), first);
    });
}

#[cfg(not(feature = "mods-wasm"))]
#[test]
fn without_the_feature_every_code_mod_call_says_code_mods_are_not_compiled_in() {
    run_ctx(|ctx| unsafe {
        let unsupported = |status: PonkoStatus| {
            assert_eq!(status, PONKO_UNSUPPORTED);
            assert!(last_error().contains("not compiled in"), "{}", last_error());
            assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_UNSUPPORTED);
        };
        let mut f = PonkoModFn::default();
        unsupported(ponko_ctx_mod_function(
            ctx,
            c"crawler".as_ptr(),
            c"add".as_ptr(),
            &mut f,
        ));
        unsupported(ponko_ctx_mod_call(ctx, f, null(), 0, null_mut(), null_mut()));
        unsupported(ponko_ctx_mod_expose_entity(ctx, 0));
        unsupported(ponko_ctx_mod_hide_entity(ctx, 0, null_mut()));
        unsupported(ponko_ctx_mod_expose_prefab(ctx, c"prefabs/bat.json".as_ptr()));
        unsupported(ponko_ctx_mod_seed_random(ctx, 1));
        // A mod with code is refused like any bad mod.
        assert_eq!(mount(ctx), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("mods-wasm"), "{}", last_error());
    });
}

#[test]
fn code_mod_calls_check_their_arguments() {
    unsafe {
        let mut f = PonkoModFn::default();
        assert_eq!(
            ponko_ctx_mod_function(null_mut(), c"m".as_ptr(), c"f".as_ptr(), &mut f),
            PONKO_INVALID_ARGUMENT
        );
        run_ctx(|ctx| {
            assert_eq!(
                ponko_ctx_mod_function(ctx, null(), c"f".as_ptr(), &mut f),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_mod_function(ctx, c"m".as_ptr(), c"f".as_ptr(), null_mut()),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(ponko_ctx_mod_expose_prefab(ctx, null()), PONKO_INVALID_ARGUMENT);
        });
    }
}
