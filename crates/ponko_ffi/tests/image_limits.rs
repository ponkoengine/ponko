//! The image limits from C: the game's setting, and one picture decoded
//! under limits of its own. The limits are for the whole process, so the
//! tests that set them take `LIMITS` and put the defaults back.
mod common;
use common::*;
use ponko_ffi::*;
use std::sync::Mutex;

use PonkoErrorKind::*;
use PonkoStatus::*;

static LIMITS: Mutex<()> = Mutex::new(());

const DEFAULT_SIDE: u32 = 16_384;
const DEFAULT_BYTES: u64 = 512 << 20;

/// A `width` x 1 PNG, written by hand: a stored (uncompressed) zlib block of
/// zero pixels, so the file is as big as its rows.
fn png(width: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    // One row: filter byte 0, then 4 bytes a pixel.
    let row = vec![0u8; 1 + width as usize * 4];
    let mut zlib = vec![0x78, 0x01];
    let mut rest = &row[..];
    while !rest.is_empty() {
        let n = rest.len().min(65_535);
        let last = n == rest.len();
        zlib.push(u8::from(last));
        zlib.extend_from_slice(&(n as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(n as u16)).to_le_bytes());
        zlib.extend_from_slice(&rest[..n]);
        rest = &rest[n..];
    }
    zlib.extend_from_slice(&adler(&row).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &zlib[..]), (b"IEND", &[][..])] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend_from_slice(data);
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&crc32(&chunk).to_be_bytes());
    }
    png
}

fn empty() -> PonkoTexture {
    PonkoTexture {
        id: 7,
        width: 0,
        height: 0,
    }
}

#[test]
fn the_defaults_are_the_old_constants_and_can_be_set_and_read() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    run_ctx(|ctx| unsafe {
        let (mut side, mut bytes) = (0u32, 0u64);
        assert_eq!(ponko_ctx_image_limits(ctx, &mut side, &mut bytes), PONKO_OK);
        assert_eq!((side, bytes), (DEFAULT_SIDE, DEFAULT_BYTES));
        assert_eq!(ponko_ctx_set_image_limits(ctx, 4_096, 64 << 20), PONKO_OK);
        assert_eq!(ponko_ctx_image_limits(ctx, &mut side, &mut bytes), PONKO_OK);
        assert_eq!((side, bytes), (4_096, 64 << 20));
        assert_eq!(ponko_ctx_set_image_limits(ctx, DEFAULT_SIDE, DEFAULT_BYTES), PONKO_OK);
    });
}

#[test]
fn the_game_setting_is_applied_at_startup() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    let game = new_game();
    unsafe {
        assert_eq!(ponko_game_image_limits(game, 2_048, 16 << 20), PONKO_OK);
    }
    let world = run_game(game, 0, |ctx| unsafe {
        let (mut side, mut bytes) = (0u32, 0u64);
        assert_eq!(ponko_ctx_image_limits(ctx, &mut side, &mut bytes), PONKO_OK);
        assert_eq!((side, bytes), (2_048, 16 << 20));
        assert_eq!(ponko_ctx_set_image_limits(ctx, DEFAULT_SIDE, DEFAULT_BYTES), PONKO_OK);
    });
    unsafe { ponko_world_free(world) };
}

#[test]
fn a_picture_past_the_global_limits_is_a_limit_error_naming_them() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    run_ctx(|ctx| unsafe {
        let source = ponko_memory_source_new(c"m".as_ptr());
        for (name, width) in [("ok.png", 16), ("wide.png", 17)] {
            let bytes = png(width);
            let name = std::ffi::CString::new(name).unwrap();
            assert_eq!(
                ponko_memory_source_insert(source, name.as_ptr(), bytes.as_ptr(), bytes.len()),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        assert_eq!(ponko_ctx_set_image_limits(ctx, 16, 1 << 20), PONKO_OK);
        let mut texture = empty();
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"ok.png".as_ptr(), &mut texture),
            PONKO_OK
        );
        assert_eq!(texture.width, 16);
        let mut texture = empty();
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"wide.png".as_ptr(), &mut texture),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_LIMIT);
        assert!(last_error().contains("16 pixels a side, 1 MiB"), "{}", last_error());
        assert_eq!(texture.id, 7, "the output is unchanged on failure");
        assert_eq!(ponko_ctx_set_image_limits(ctx, DEFAULT_SIDE, DEFAULT_BYTES), PONKO_OK);
    });
}

#[test]
fn raised_limits_load_a_picture_past_the_defaults() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    run_ctx(|ctx| unsafe {
        let bytes = png(DEFAULT_SIDE + 1);
        let mut texture = empty();
        assert_eq!(
            ponko_ctx_try_decode_texture(
                ctx,
                bytes.as_ptr(),
                bytes.len(),
                c"w.png".as_ptr(),
                DEFAULT_SIDE,
                DEFAULT_BYTES,
                &mut texture
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_LIMIT);
        assert_eq!(ponko_ctx_set_image_limits(ctx, 32_768, 1 << 30), PONKO_OK);
        let source = ponko_memory_source_new(c"m".as_ptr());
        assert_eq!(
            ponko_memory_source_insert(source, c"w.png".as_ptr(), bytes.as_ptr(), bytes.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        assert_eq!(
            ponko_ctx_try_load_texture(ctx, c"w.png".as_ptr(), &mut texture),
            PONKO_OK
        );
        assert_eq!((texture.width, texture.height), (DEFAULT_SIDE + 1, 1));
        assert_eq!(ponko_ctx_set_image_limits(ctx, DEFAULT_SIDE, DEFAULT_BYTES), PONKO_OK);
    });
}

#[test]
fn one_picture_decodes_under_its_own_limits_and_leaves_the_games() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    run_ctx(|ctx| unsafe {
        let avatar = png(100);
        let mut texture = empty();
        assert_eq!(
            ponko_ctx_try_decode_texture(
                ctx,
                avatar.as_ptr(),
                avatar.len(),
                c"avatar.png".as_ptr(),
                64,
                1 << 20,
                &mut texture
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_LIMIT);
        assert_eq!(last_path(), "avatar.png");
        assert!(last_error().contains("64 pixels a side"), "{}", last_error());
        assert_eq!(texture.id, 7, "the output is unchanged on failure");
        let (mut side, mut bytes) = (0u32, 0u64);
        assert_eq!(ponko_ctx_image_limits(ctx, &mut side, &mut bytes), PONKO_OK);
        assert_eq!(
            (side, bytes),
            (DEFAULT_SIDE, DEFAULT_BYTES),
            "the game's limits are untouched"
        );
        assert_eq!(
            ponko_ctx_try_decode_texture(
                ctx,
                avatar.as_ptr(),
                avatar.len(),
                c"avatar.png".as_ptr(),
                100,
                1 << 20,
                &mut texture
            ),
            PONKO_OK
        );
        assert_eq!((texture.width, texture.height), (100, 1));
    });
}

#[test]
fn bad_arguments_are_invalid_not_a_crash() {
    let _own = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut texture = empty();
        assert_eq!(
            ponko_ctx_try_decode_texture(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                c"a.png".as_ptr(),
                64,
                64,
                &mut texture
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_image_limits(std::ptr::null_mut(), 64, 64),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_image_limits(std::ptr::null_mut(), 64, 64),
            PONKO_INVALID_ARGUMENT
        );
    }
    run_ctx(|ctx| unsafe {
        assert_eq!(ponko_ctx_set_image_limits(ctx, 0, 64), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_set_image_limits(ctx, 64, 0), PONKO_INVALID_ARGUMENT);
        let mut texture = empty();
        let junk = b"not a picture";
        assert_eq!(
            ponko_ctx_try_decode_texture(
                ctx,
                junk.as_ptr(),
                junk.len(),
                c"x.png".as_ptr(),
                64,
                1 << 20,
                &mut texture
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_DECODE);
        assert_eq!(
            ponko_ctx_try_decode_texture(ctx, std::ptr::null(), 5, c"x.png".as_ptr(), 64, 64, &mut texture),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_try_decode_texture(ctx, junk.as_ptr(), junk.len(), std::ptr::null(), 64, 64, &mut texture),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_try_decode_texture(
                ctx,
                junk.as_ptr(),
                junk.len(),
                c"x.png".as_ptr(),
                64,
                64,
                std::ptr::null_mut()
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_image_limits(ctx, std::ptr::null_mut(), std::ptr::null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}
