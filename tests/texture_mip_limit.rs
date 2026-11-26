//! The texture mip limit: 3D textures loaded under it drop their largest
//! mip levels, and textures for sprites and UI don't.
#![cfg(feature = "3d")]

use ponko::assets::MemorySource;
use ponko::prelude::*;
use ponko::terrain::TextureKind;

fn game() -> Game {
    Game::new("texture mip limit", 1, 1).hot_reload(false)
}

/// A one-triangle `.gltf` whose material shows image file `tex.png`, with
/// its geometry in `tri.bin`.
fn textured_gltf() -> String {
    r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
    "meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"indices":2,"material":0}]}],
    "materials":[{"name":"m","pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}],
    "textures":[{"source":0}],"images":[{"uri":"tex.png"}],
    "buffers":[{"uri":"tri.bin","byteLength":66}],
    "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":24},
        {"buffer":0,"byteOffset":60,"byteLength":6}],
    "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
        {"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"},
        {"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}]}"#
        .to_string()
}

fn triangle_bin() -> Vec<u8> {
    let mut bin = Vec::new();
    for v in [
        0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    bin
}

fn png(size: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(size, size, image::Rgba([0, 255, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn model_image_size(limit: u32, size: u32) -> (u32, u32) {
    let mut out = (0, 0);
    game().run_headless(0, |ctx| {
        ctx.gfx.set_texture_mip_limit(limit);
        ctx.assets.add_source(
            MemorySource::new("models")
                .with("prop.gltf", textured_gltf())
                .with("tex.png", png(size))
                .with("tri.bin", triangle_bin()),
        );
        ctx.gfx.load_mesh("prop.gltf");
        let materials = ctx.gfx.loaded_materials("m");
        let image = materials[0].1.texture.expect("the material has an image");
        out = (image.width(), image.height());
    });
    out
}

#[test]
fn model_images_drop_their_largest_levels() {
    assert_eq!(model_image_size(0, 256), (256, 256), "no limit, full size");
    assert_eq!(model_image_size(1, 256), (128, 128));
    assert_eq!(model_image_size(2, 256), (64, 64));
}

#[test]
fn small_model_images_keep_their_size() {
    assert_eq!(model_image_size(2, 32), (32, 32));
    assert_eq!(model_image_size(2, 64), (32, 32), "down to 32 pixels, no further");
}

#[test]
fn texture_arrays_drop_their_largest_levels() {
    game().run_headless(0, |ctx| {
        ctx.gfx.set_texture_mip_limit(1);
        let layer = [255u8, 0, 0, 255].repeat(128 * 64);
        let array = ctx
            .gfx
            .create_texture_array(128, 64, &[&layer, &layer], TextureKind::Color);
        assert_eq!((array.width(), array.height(), array.layers()), (64, 32, 2));
        // Too small to lose a level: as made.
        let mask = [255u8; 32 * 32 * 4];
        let small = ctx.gfx.create_texture_array(32, 32, &[&mask], TextureKind::Data);
        assert_eq!((small.width(), small.height()), (32, 32));
    });
}

#[test]
fn sprite_textures_and_textures_loaded_earlier_keep_their_size() {
    game().run_headless(0, |ctx| {
        let pixels = [255u8; 128 * 128 * 4];
        let before = ctx.gfx.create_texture_array(128, 128, &[&pixels], TextureKind::Color);
        ctx.gfx.set_texture_mip_limit(1);
        assert_eq!(before.width(), 128, "already made");
        let sprite = ctx.gfx.create_texture(128, 128, &pixels);
        assert_eq!(sprite.width(), 128, "2D textures never drop levels");
    });
}

#[test]
fn the_limit_is_at_most_four_levels() {
    game().run_headless(0, |ctx| {
        ctx.gfx.set_texture_mip_limit(9);
        assert_eq!(ctx.gfx.texture_mip_limit(), 4);
    });
}
