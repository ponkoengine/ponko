//! Which image formats `load_texture` reads: PNG always, JPEG by default (no
//! `3d` feature needed), the rest behind `image-*` cargo features. A format
//! the build leaves out fails with an error that names the feature.
//!
//! Run the other feature sets with
//! `cargo test -p ponko --no-default-features --test image_formats` (JPEG off)
//! and `cargo test -p ponko --features image-formats --test image_formats`.

use ponko::assets::MemorySource;
use ponko::error::ErrorKind;
use ponko::prelude::*;

/// A 16 x 8 JPEG, red on the left half and blue on the right (`cjpeg`, 641 bytes).
#[allow(dead_code)]
const RED_BLUE_JPEG: &[u8] = include_bytes!("fixtures/images/red-blue.jpg");

fn game() -> Game {
    Game::new("image formats", 1, 1).hot_reload(false)
}

/// Loads `bytes` stored under `name` in a memory source.
fn load(name: &str, bytes: &[u8]) -> Result<Texture, ponko::error::Error> {
    let mut result = None;
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("mem").with(name, bytes.to_vec()));
        result = Some(ctx.gfx.try_load_texture(name));
    });
    result.unwrap()
}

/// The bytes the image crate encodes `image` as, to build fixtures in the formats under test.
#[allow(dead_code)]
fn encoded(format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(5, 3, image::Rgba([10, 200, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}

#[cfg(feature = "image-jpeg")]
#[test]
fn a_2d_build_loads_a_jpeg() {
    let texture = load("backgrounds/sky.jpg", RED_BLUE_JPEG).expect("a JPEG loads without the 3d feature");
    assert_eq!((texture.width(), texture.height()), (16, 8));
}

#[cfg(feature = "image-jpeg")]
#[test]
fn a_jpeg_loads_whatever_its_extension_says() {
    // The format comes from the file's first bytes.
    let texture = load("backgrounds/sky.png", RED_BLUE_JPEG).expect("a JPEG named .png loads");
    assert_eq!((texture.width(), texture.height()), (16, 8));
    assert!(load("sky", RED_BLUE_JPEG).is_ok(), "and so does one with no extension");
}

#[cfg(not(feature = "image-jpeg"))]
#[test]
fn a_jpeg_without_the_feature_names_it() {
    if image::ImageFormat::Jpeg.reading_enabled() {
        return; // another crate in this build turned the decoder on
    }
    for name in ["sky.jpg", "sky.png"] {
        let err = load(name, RED_BLUE_JPEG).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported, "{name}");
        assert_eq!(err.path(), Some(name));
        assert!(err.message().contains("image-jpeg"), "{}", err.message());
        assert!(err.message().contains("JPEG"), "{}", err.message());
    }
}

#[test]
fn png_always_loads() {
    let texture = load("a.png", &encoded(image::ImageFormat::Png)).unwrap();
    assert_eq!((texture.width(), texture.height()), (5, 3));
}

#[test]
fn garbage_is_a_decode_error_not_a_missing_feature() {
    let err = load("a.png", b"this is not an image").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert!(err.message().contains("not a valid image"), "{}", err.message());
}

#[test]
fn a_truncated_jpeg_is_a_decode_error() {
    if !image::ImageFormat::Jpeg.reading_enabled() {
        return;
    }
    let err = load("cut.jpg", &RED_BLUE_JPEG[..40]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Decode);
}

#[cfg(not(feature = "image-webp"))]
#[test]
fn webp_without_the_feature_names_it() {
    if image::ImageFormat::WebP.reading_enabled() {
        return;
    }
    // A RIFF container tagged WEBP: only the first bytes matter to the check.
    let webp = b"RIFF\x0c\0\0\0WEBPVP8 \0\0\0\0";
    for name in ["tile.webp", "tile.png"] {
        let err = load(name, webp).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported, "{name}");
        assert!(err.message().contains("`image-webp`"), "{}", err.message());
    }
}

#[cfg(not(feature = "image-tga"))]
#[test]
fn tga_without_the_feature_is_found_by_extension() {
    if image::ImageFormat::Tga.reading_enabled() {
        return;
    }
    // TGA has no magic bytes, so the extension is all there is.
    let err = load("sprite.tga", b"\0\0\x02\0\0\0\0\0\0\0\0\0\x01\0\x01\0\x18\0\0\0\0").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert!(err.message().contains("`image-tga`"), "{}", err.message());
}

#[cfg(feature = "image-bmp")]
#[test]
fn bmp_loads_with_its_feature() {
    let texture = load("a.bmp", &encoded(image::ImageFormat::Bmp)).unwrap();
    assert_eq!((texture.width(), texture.height()), (5, 3));
}

#[cfg(feature = "image-tga")]
#[test]
fn tga_loads_with_its_feature() {
    let texture = load("a.tga", &encoded(image::ImageFormat::Tga)).unwrap();
    assert_eq!((texture.width(), texture.height()), (5, 3));
}

#[cfg(feature = "image-qoi")]
#[test]
fn qoi_loads_with_its_feature() {
    let texture = load("a.qoi", &encoded(image::ImageFormat::Qoi)).unwrap();
    assert_eq!((texture.width(), texture.height()), (5, 3));
}

#[cfg(feature = "image-gif")]
#[test]
fn gif_loads_with_its_feature() {
    let texture = load("a.gif", &encoded(image::ImageFormat::Gif)).unwrap();
    assert_eq!((texture.width(), texture.height()), (5, 3));
}
