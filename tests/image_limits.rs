//! `ImageLimits`: a game changes how big a picture the decoders accept.
//! A test lowers the limits and a picture just past them is refused by each
//! decoder, naming the limits in force; another raises them and a picture
//! past the defaults decodes. One picture can also be decoded under limits of
//! its own, without touching the game's.
//!
//! The limits are for the whole process, so every test that sets them takes
//! `LIMITS` first (the tests of one file run on several threads) and puts the
//! defaults back when it ends.
//!
//! Run the other formats with `cargo test -p ponko --features image-formats --test image_limits`.

use std::sync::{Mutex, MutexGuard};

use ponko::assets::{ImageLimits, MemorySource};
use ponko::error::ErrorKind;
use ponko::prelude::*;

static LIMITS: Mutex<()> = Mutex::new(());

/// Holds the lock on the process's limits; they are the defaults again when it drops.
struct Exclusive(#[allow(dead_code)] MutexGuard<'static, ()>);

fn exclusive() -> Exclusive {
    let guard = LIMITS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Game::new("limits", 1, 1).run_headless(0, |ctx| ctx.assets.set_image_limits(ImageLimits::default()));
    Exclusive(guard)
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        Game::new("limits", 1, 1).run_headless(0, |ctx| ctx.assets.set_image_limits(ImageLimits::default()));
    }
}

/// A `width` x `height` picture in `format`, as the image crate encodes it.
fn picture(format: image::ImageFormat, width: u32, height: u32) -> Vec<u8> {
    use image::{DynamicImage, ImageBuffer};
    let image = match format {
        // Float pictures for the float formats; no alpha for the ones without it.
        image::ImageFormat::Hdr => DynamicImage::ImageRgb32F(ImageBuffer::new(width, height)),
        image::ImageFormat::OpenExr => DynamicImage::ImageRgba32F(ImageBuffer::new(width, height)),
        image::ImageFormat::Jpeg | image::ImageFormat::Pnm => DynamicImage::ImageRgb8(ImageBuffer::new(width, height)),
        _ => DynamicImage::ImageRgba8(ImageBuffer::new(width, height)),
    };
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .unwrap_or_else(|e| panic!("encoding {format:?} {width} x {height}: {e}"));
    bytes
}

/// The formats this build reads, with the extension a file of each has, and
/// the most pixels a side the format's own header can say (the container is
/// what stops WebP at 16 383 and ICO at 256).
fn formats() -> Vec<(image::ImageFormat, &'static str, u32)> {
    vec![
        (image::ImageFormat::Png, "png", u32::MAX),
        #[cfg(feature = "image-jpeg")]
        (image::ImageFormat::Jpeg, "jpg", 65_535),
        #[cfg(feature = "image-webp")]
        (image::ImageFormat::WebP, "webp", 16_383),
        #[cfg(feature = "image-bmp")]
        (image::ImageFormat::Bmp, "bmp", u32::MAX >> 1),
        #[cfg(feature = "image-tga")]
        (image::ImageFormat::Tga, "tga", 65_535),
        #[cfg(feature = "image-gif")]
        (image::ImageFormat::Gif, "gif", 65_535),
        #[cfg(feature = "image-tiff")]
        (image::ImageFormat::Tiff, "tiff", u32::MAX),
        #[cfg(feature = "image-ico")]
        (image::ImageFormat::Ico, "ico", 256),
        #[cfg(feature = "image-qoi")]
        (image::ImageFormat::Qoi, "qoi", u32::MAX),
        #[cfg(feature = "image-pnm")]
        (image::ImageFormat::Pnm, "ppm", u32::MAX),
        #[cfg(feature = "image-hdr")]
        (image::ImageFormat::Hdr, "hdr", 65_535),
        #[cfg(feature = "image-exr")]
        (image::ImageFormat::OpenExr, "exr", u32::MAX),
    ]
}

/// Loads `bytes` stored under `name` the way a game does, with the game's limits in force.
fn load(name: &str, bytes: &[u8]) -> Result<Texture, ponko::error::Error> {
    let mut result = None;
    Game::new("limits", 1, 1).hot_reload(false).run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("mem").with(name, bytes.to_vec()));
        result = Some(ctx.gfx.try_load_texture(name));
    });
    result.unwrap()
}

fn set(limits: ImageLimits) {
    Game::new("limits", 1, 1).run_headless(0, |ctx| ctx.assets.set_image_limits(limits));
}

#[test]
fn the_defaults_are_the_engines_old_constants() {
    let _own = exclusive();
    let limits = ImageLimits::default();
    assert_eq!((limits.max_side, limits.max_bytes), (16_384, 512 << 20));
    assert_eq!(limits, ImageLimits::DEFAULT);
    Game::new("limits", 1, 1).run_headless(0, |ctx| assert_eq!(ctx.assets.image_limits(), limits));
}

#[test]
fn a_picture_one_pixel_past_a_lowered_side_is_refused_by_every_format() {
    let _own = exclusive();
    set(ImageLimits::new(16, 1 << 20));
    for (format, extension, _) in formats() {
        let name = format!("a.{extension}");
        assert!(
            load(&name, &picture(format, 16, 1)).is_ok(),
            "{format:?}: 16 x 1 is within"
        );
        let err = load(&name, &picture(format, 17, 1)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{format:?}: {}", err.message());
        assert!(
            err.message().contains("16 pixels a side") && err.message().contains("1 MiB"),
            "{format:?}: the message names the limits in force: {}",
            err.message()
        );
        let tall = load(&name, &picture(format, 1, 17)).unwrap_err();
        assert_eq!(tall.kind(), ErrorKind::Limit, "{format:?} tall: {}", tall.message());
    }
}

#[test]
fn a_picture_past_lowered_bytes_is_refused_by_every_format() {
    let _own = exclusive();
    // 16 x 16 RGBA8 is 1 024 bytes, within 4 096 (with room for a decoder's own buffers);
    // 33 x 32 is 4 224, just past them.
    set(ImageLimits::new(1_024, 4_096));
    for (format, extension, _) in formats() {
        let name = format!("a.{extension}");
        assert!(
            load(&name, &picture(format, 16, 16)).is_ok(),
            "{format:?}: 16 x 16 is within"
        );
        let err = load(&name, &picture(format, 33, 32)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{format:?}: {}", err.message());
        assert!(
            err.message().contains("1024 pixels a side") && err.message().contains("4096 bytes"),
            "{format:?}: the message names the limits in force: {}",
            err.message()
        );
    }
}

#[test]
fn raised_limits_decode_a_picture_past_the_defaults() {
    let _own = exclusive();
    let wide = ImageLimits::DEFAULT.max_side + 1;
    for (format, extension, widest) in formats() {
        if widest < wide {
            continue; // the format's own header can't say it
        }
        let name = format!("a.{extension}");
        let bytes = picture(format, wide, 1);
        set(ImageLimits::default());
        let err = load(&name, &bytes).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{format:?} by default: {}", err.message());
        assert!(
            err.message().contains("16384 pixels a side") && err.message().contains("512 MiB"),
            "{format:?}: {}",
            err.message()
        );
        set(ImageLimits::new(65_535, 1 << 30));
        let texture = load(&name, &bytes).unwrap_or_else(|e| panic!("{format:?} with raised limits: {e}"));
        assert_eq!((texture.width(), texture.height()), (wide, 1), "{format:?}");
    }
}

#[test]
fn the_game_builder_sets_the_limits_at_startup() {
    let _own = exclusive();
    Game::new("limits", 1, 1)
        .image_limits(ImageLimits::new(4_096, 64 << 20))
        .run_headless(0, |ctx| {
            assert_eq!(ctx.assets.image_limits(), ImageLimits::new(4_096, 64 << 20));
        });
    let err = load("a.png", &picture(image::ImageFormat::Png, 4_097, 1)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Limit);
    assert!(
        err.message().contains("4096 pixels a side, 64 MiB"),
        "{}",
        err.message()
    );
    assert!(load("a.png", &picture(image::ImageFormat::Png, 4_096, 1)).is_ok());
}

#[test]
fn a_game_that_sets_no_limits_leaves_the_current_ones() {
    let _own = exclusive();
    set(ImageLimits::new(64, 1 << 20));
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        assert_eq!(ctx.assets.image_limits(), ImageLimits::new(64, 1 << 20));
    });
}

#[test]
fn one_picture_decodes_under_tighter_limits_without_changing_the_games() {
    let _own = exclusive();
    let avatar = picture(image::ImageFormat::Png, 100, 100);
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        let tight = ImageLimits::new(64, 1 << 20);
        let err = ctx.gfx.try_decode_texture(&avatar, "avatar.png", tight).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(err.message().contains("64 pixels a side"), "{}", err.message());
        // The game's own limits are untouched: the same picture loads by name.
        assert_eq!(ctx.assets.image_limits(), ImageLimits::default());
        ctx.assets
            .add_source(MemorySource::new("mem").with("avatar.png", avatar.clone()));
        assert!(ctx.gfx.try_load_texture("avatar.png").is_ok());
        // And a picture within them decodes.
        let ok = ctx
            .gfx
            .try_decode_texture(&avatar, "avatar.png", ImageLimits::new(100, 1 << 20));
        assert_eq!(ok.map(|t| (t.width(), t.height())).unwrap(), (100, 100));
    });
}

#[test]
fn per_picture_limits_hold_for_every_format_and_for_bytes() {
    let _own = exclusive();
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        for (format, extension, _) in formats() {
            let name = format!("a.{extension}");
            let tight = ImageLimits::new(16, 1 << 20);
            assert!(
                ctx.gfx
                    .try_decode_texture(&picture(format, 16, 1), &name, tight)
                    .is_ok()
            );
            let err = ctx
                .gfx
                .try_decode_texture(&picture(format, 17, 1), &name, tight)
                .unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Limit, "{format:?}: {}", err.message());
            let few = ImageLimits::new(1_024, 4_096);
            let err = ctx
                .gfx
                .try_decode_texture(&picture(format, 33, 32), &name, few)
                .unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Limit, "{format:?}: {}", err.message());
        }
    });
}

#[test]
fn per_picture_limits_can_be_higher_than_the_games() {
    let _own = exclusive();
    set(ImageLimits::new(16, 1 << 20));
    let wide = picture(image::ImageFormat::Png, 40, 1);
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        assert!(
            ctx.gfx
                .try_decode_texture(&wide, "w.png", ImageLimits::default())
                .is_ok()
        );
        assert_eq!(ctx.assets.image_limits(), ImageLimits::new(16, 1 << 20));
    });
}

#[test]
fn per_picture_limits_end_with_the_call_even_when_it_fails() {
    let _own = exclusive();
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        let tight = ImageLimits::new(8, 1 << 20);
        assert!(ctx.gfx.try_decode_texture(b"not a picture", "x.png", tight).is_err());
        assert!(
            ctx.gfx
                .try_decode_texture(&picture(image::ImageFormat::Png, 9, 1), "x.png", tight)
                .is_err()
        );
        // Back to the game's: a 9-pixel picture loads.
        ctx.assets
            .add_source(MemorySource::new("mem").with("x.png", picture(image::ImageFormat::Png, 9, 1)));
        assert!(ctx.gfx.try_load_texture("x.png").is_ok());
    });
}

#[test]
fn bytes_that_are_not_a_picture_are_still_a_decode_error_under_any_limits() {
    let _own = exclusive();
    Game::new("limits", 1, 1).run_headless(0, |ctx| {
        let err = ctx
            .gfx
            .try_decode_texture(b"definitely not an image", "x.png", ImageLimits::new(8, 64))
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode, "{}", err.message());
    });
}

#[test]
fn a_game_lowers_its_limits_and_decodes_players_pictures_under_tighter_ones() {
    let _own = exclusive();
    let downloaded = picture(image::ImageFormat::Png, 600, 8);
    // ANCHOR: image_limits
    // Everything this game loads: at most 8 192 pixels a side and 256 MiB of pixels.
    Game::new("Avatars", 800, 600)
        .image_limits(ImageLimits::new(8_192, 256 << 20))
        .run_headless(0, |ctx| {
            // A picture from a player: much tighter, without touching the limits above.
            let tight = ImageLimits::new(512, 1 << 20);
            match ctx.gfx.try_decode_texture(&downloaded, "avatar.png", tight) {
                Ok(avatar) => println!("avatar is {} x {}", avatar.width(), avatar.height()),
                Err(e) if e.kind() == ErrorKind::Limit => println!("too big: {e}"),
                Err(e) => println!("not a picture: {e}"),
            }
            // ANCHOR_END: image_limits
            assert_eq!(ctx.assets.image_limits(), ImageLimits::new(8_192, 256 << 20));
            assert!(
                ctx.gfx
                    .try_decode_texture(&downloaded, "avatar.png", tight)
                    .is_err_and(|e| e.kind() == ErrorKind::Limit)
            );
            ctx.assets.set_image_limits(ImageLimits::default());
        });
}
