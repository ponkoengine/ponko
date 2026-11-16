//! Asset sources for 3D files: meshes, shaders, screen effects and compute
//! passes load by name from memory, with the same caching and messages as
//! files from `assets/`.
#![cfg(feature = "3d")]

use std::cell::Cell;
use std::rc::Rc;

use ponko::assets::{AssetSource, MemorySource};
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::{ReflectionProbe as ReflectionProbe3D, Transform as Transform3D};
use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::Model;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::terrain::TextureKind;

const HOUSE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/house.glb"));
const SURFACE: &str = "fn surface(s: Surface) -> Surface { return s; }";
const EFFECT: &str = "fn effect(p: Pixel) -> vec3<f32> { return p.color; }";
const COMPUTE: &str = "fn compute(c: Cell) -> vec4<f32> { return c.value; }";

fn game() -> Game {
    Game::new("asset sources 3d", 1, 1).hot_reload(false)
}

/// Counts reads of every name it has, to see what the caches do.
struct Counting {
    id: &'static str,
    reads: Rc<Cell<u32>>,
    name: &'static str,
    bytes: Vec<u8>,
}

impl AssetSource for Counting {
    fn id(&self) -> &str {
        self.id
    }
    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        if name != self.name {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        self.reads.set(self.reads.get() + 1);
        Ok(self.bytes.clone())
    }
    fn contains(&self, name: &str) -> bool {
        name == self.name
    }
}

#[test]
fn a_mesh_loads_by_name_from_a_memory_source() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("pack").with("models/home.glb", HOUSE));
        let home = ctx.gfx.load_mesh("models/home.glb");
        assert_eq!(
            ctx.gfx.load_mesh("models/home.glb"),
            home,
            "loading again returns the same mesh"
        );
        let data = ctx.gfx.load_mesh_data("models/home.glb");
        assert!(!data.indices.is_empty());
        ctx.world.spawn((Transform3D::default(), Model::new(home)));
    });
}

#[test]
fn a_mesh_in_a_source_is_found_by_the_names_a_scene_gives() {
    // A saved scene names its mesh; it is found in the source too.
    let mut saved = String::new();
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("pack").with("models/home.glb", HOUSE));
        let home = ctx.gfx.load_mesh("models/home.glb");
        ctx.world.spawn((Transform3D::default(), Model::new(home)));
        saved = serde_json::to_string(&ctx.world.save()).unwrap();
    });
    let pack = MemorySource::new("pack")
        .with("models/home.glb", HOUSE)
        .with("level.json", saved);
    let world = game().run_headless(0, |ctx| {
        ctx.assets.add_source(pack);
        ctx.load_scene("level.json").unwrap();
        assert_eq!(ctx.world.query::<&Model>().iter().count(), 1);
    });
    assert_eq!(world.len(), 1);
}

#[test]
fn repeated_mesh_loads_read_a_source_once() {
    let reads = Rc::new(Cell::new(0));
    let counted = reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_source(Counting {
            id: "counting",
            reads: counted.clone(),
            name: "home.glb",
            bytes: HOUSE.to_vec(),
        });
        let first = ctx.gfx.load_mesh("home.glb");
        for _ in 0..3 {
            assert_eq!(ctx.gfx.load_mesh("home.glb"), first);
        }
        assert!(ctx.gfx.try_load_clips("home.glb").is_err(), "no skins in this file");
    });
    assert_eq!(reads.get(), 1, "meshes, skeletons and clips share one read");
}

#[test]
fn the_same_mesh_name_in_two_sources_is_two_meshes_and_the_first_wins() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(MemorySource::new("first").with("m.glb", HOUSE));
        ctx.assets.add_source(MemorySource::new("second").with("m.glb", HOUSE));
        let from_first = ctx.gfx.load_mesh("m.glb");
        assert_eq!(ctx.gfx.load_mesh("m.glb"), from_first);
    });
    // The folder's copy and a source's copy are separate assets.
    game().run_headless(0, |ctx| {
        let from_folder = ctx.gfx.load_mesh("models/house.glb");
        ctx.assets
            .add_source(MemorySource::new("pack").with("models/house.glb", HOUSE));
        let from_source = ctx.gfx.load_mesh("models/house.glb");
        assert_ne!(from_source, from_folder, "same name, other source: another mesh");
        assert_eq!(ctx.gfx.load_mesh("models/house.glb"), from_source);
    });
}

#[test]
fn a_missing_mesh_reports_the_sources_and_folders_searched() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(MemorySource::new("pack"));
        let err = ctx.gfx.try_load_mesh_data("models/nowhere.glb").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.message().contains("\"models/nowhere.glb\" not found in assets/"),
            "{err}"
        );
        assert!(
            err.message().contains("Asset sources searched first: \"pack\""),
            "{err}"
        );
    });
}

#[test]
#[should_panic(expected = "models/broken.glb")]
fn corrupt_mesh_bytes_are_an_error_naming_the_file() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("pack").with("models/broken.glb", b"nope".to_vec()));
        ctx.gfx.load_mesh("models/broken.glb");
    });
}

#[test]
fn shaders_effects_and_compute_load_by_name_from_a_memory_source() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("pack")
                .with("shaders/plain.wgsl", SURFACE)
                .with("effects/plain.wgsl", EFFECT)
                .with("compute/plain.wgsl", COMPUTE),
        );
        let shader = ctx.gfx.load_shader("shaders/plain.wgsl");
        let effect = ctx.gfx.load_effect("effects/plain.wgsl");
        let compute = ctx.gfx.load_compute("compute/plain.wgsl");
        assert_eq!(ctx.gfx.load_shader("shaders/plain.wgsl"), shader);
        assert_eq!(ctx.gfx.load_effect("effects/plain.wgsl"), effect);
        assert_eq!(ctx.gfx.load_compute("compute/plain.wgsl"), compute);
    });
}

#[test]
fn repeated_shader_loads_read_a_source_once() {
    let reads = Rc::new(Cell::new(0));
    let counted = reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_source(Counting {
            id: "counting",
            reads: counted.clone(),
            name: "plain.wgsl",
            bytes: SURFACE.as_bytes().to_vec(),
        });
        let first = ctx.gfx.load_shader("plain.wgsl");
        for _ in 0..3 {
            assert_eq!(ctx.gfx.load_shader("plain.wgsl"), first);
        }
    });
    assert_eq!(reads.get(), 1);
}

#[test]
fn the_same_shader_name_in_two_sources_is_two_shaders() {
    game().run_headless(0, |ctx| {
        // The assets folder has shaders/water.wgsl; a source's copy is another shader.
        let from_folder = ctx.gfx.load_shader("shaders/water.wgsl");
        ctx.assets
            .add_source(MemorySource::new("pack").with("shaders/water.wgsl", SURFACE));
        let from_source = ctx.gfx.load_shader("shaders/water.wgsl");
        assert_ne!(from_source, from_folder);
        assert_eq!(ctx.gfx.load_shader("shaders/water.wgsl"), from_source);
    });
}

#[test]
fn shader_errors_name_the_source() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("pack").with("bad.wgsl", vec![0xff, 0xfe, 0xfd]));
        let err = ctx.gfx.try_load_shader("bad.wgsl").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert_eq!(err.path(), Some("bad.wgsl"));
        assert!(
            err.message()
                .contains("\"bad.wgsl\" (from asset source \"pack\") is not UTF-8 text"),
            "{err}"
        );
        let err = ctx.gfx.try_load_effect("missing.wgsl").unwrap_err();
        assert!(err.message().contains("\"missing.wgsl\" not found in assets/"), "{err}");
        assert!(
            err.message().contains("Asset sources searched first: \"pack\""),
            "{err}"
        );
        let err = ctx.gfx.try_load_compute("missing.wgsl").unwrap_err();
        assert!(
            err.message().contains("Asset sources searched first: \"pack\""),
            "{err}"
        );
    });
}

#[test]
fn assets_without_sources_still_say_where_they_looked() {
    game().run_headless(0, |ctx| {
        let err = ctx.gfx.try_load_shader("nowhere.wgsl").unwrap_err();
        assert!(
            err.message()
                .contains("\"nowhere.wgsl\" not found in assets/. Looked in:"),
            "{err}"
        );
        assert!(!err.message().contains("Asset sources searched first"), "{err}");
    });
}

/// A one-triangle `.gltf` whose material `material` shows image file
/// `tex.png` and whose geometry is in `tri.bin`, both next to it.
fn textured_gltf(material: &str) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
        "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
        "materials":[{{"name":"{material}","pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}}}}],
        "textures":[{{"source":0}}],"images":[{{"uri":"tex.png"}}],
        "buffers":[{{"uri":"tri.bin","byteLength":66}}],
        "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},
            {{"buffer":0,"byteOffset":60,"byteLength":6}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
            {{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}},
            {{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#
    )
}

/// `tri.bin` for [`textured_gltf`]: three corners, their UVs, three indices.
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

/// The image material `name` of a loaded glTF file shows.
fn image_of(ctx: &Ctx, name: &str) -> Texture {
    let materials = ctx.gfx.loaded_materials(name);
    assert_eq!(materials.len(), 1, "{name}");
    materials[0].1.texture.expect("the material has an image")
}

#[test]
fn gltf_images_are_shared_within_a_source_and_read_once() {
    let reads = Rc::new(Cell::new(0));
    let counted = reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_source(Counting {
            id: "images",
            reads: counted.clone(),
            name: "props/tex.png",
            bytes: png(2),
        });
        ctx.assets.add_source(
            MemorySource::new("models")
                .with("props/a.gltf", textured_gltf("a"))
                .with("props/b.gltf", textured_gltf("b"))
                .with("props/tri.bin", triangle_bin()),
        );
        ctx.gfx.load_mesh("props/a.gltf");
        ctx.gfx.load_mesh("props/b.gltf");
        let image = image_of(ctx, "a");
        assert_eq!(image_of(ctx, "b"), image, "both models show one texture");
        assert_eq!((image.width(), image.height()), (2, 2));
    });
    assert_eq!(reads.get(), 1, "the shared image file is read once");
}

#[test]
fn the_same_gltf_image_name_in_two_sources_is_two_images() {
    // A model and its image in the default source (a temporary folder,
    // named by absolute path)...
    let dir = std::env::temp_dir().join(format!("ponko-gltf-images-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.gltf"), textured_gltf("from-folder")).unwrap();
    std::fs::write(dir.join("tri.bin"), triangle_bin()).unwrap();
    std::fs::write(dir.join("tex.png"), png(1)).unwrap();
    let folder = format!("{}/", dir.to_str().unwrap());
    let in_folder = |file: &str| format!("{folder}{file}");
    game().run_headless(0, |ctx| {
        ctx.gfx.load_mesh(&in_folder("one.gltf"));
        let from_folder = image_of(ctx, "from-folder");
        assert_eq!(from_folder.width(), 1);
        // ...then a source with another image under the same name, and a
        // model naming it: it shows the source's image, not the folder's.
        ctx.assets.add_source(
            MemorySource::new("pack")
                .with(in_folder("two.gltf"), textured_gltf("from-pack"))
                .with(in_folder("tex.png"), png(4)),
        );
        ctx.gfx.load_mesh(&in_folder("two.gltf"));
        let from_pack = image_of(ctx, "from-pack");
        assert_ne!(from_pack, from_folder, "same name, other source: another image");
        assert_eq!(from_pack.width(), 4);
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn texture_arrays_load_their_layers_from_sources() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("pack")
                .with("layers/a.png", png(4))
                .with("layers/b.png", png(4))
                .with("layers/small.png", png(2))
                .with("layers/bad.png", b"nope".to_vec()),
        );
        ctx.gfx
            .load_texture_array(&["layers/a.png", "layers/b.png"], TextureKind::Color);
        let err = ctx
            .gfx
            .try_load_texture_array(&["layers/a.png", "layers/small.png"], TextureKind::Color)
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(err.message().contains("every layer must be the same size"), "{err}");
        let err = ctx
            .gfx
            .try_load_texture_array(&["layers/bad.png"], TextureKind::Color)
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert_eq!(err.path(), Some("layers/bad.png"));
        assert!(
            err.message()
                .contains("\"layers/bad.png\" (from asset source \"pack\") is not a valid image"),
            "{err}"
        );
        let err = ctx
            .gfx
            .try_load_texture_array(&["layers/none.png"], TextureKind::Color)
            .unwrap_err();
        assert!(
            err.message().contains("Asset sources searched first: \"pack\""),
            "{err}"
        );
    });
}

fn exr(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_pixel(width, height, image::Rgb([1.0; 3])))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::OpenExr)
        .unwrap();
    bytes
}

#[test]
fn reflection_probe_images_load_from_sources() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("pack")
                .with("sky/room.exr", exr(64, 32))
                .with("sky/bad.exr", b"nope".to_vec()),
        );
        let probe = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
        let err = ctx
            .gfx
            .try_load_reflection_probe(&mut ctx.world, probe, "sky/bad.exr")
            .unwrap_err();
        assert!(
            err.message().contains("\"sky/bad.exr\" (from asset source \"pack\")"),
            "{err}"
        );
        let err = ctx
            .gfx
            .try_load_reflection_probe(&mut ctx.world, probe, "sky/none.exr")
            .unwrap_err();
        assert!(
            err.message().contains("Asset sources searched first: \"pack\""),
            "{err}"
        );
        ctx.gfx.load_reflection_probe(&mut ctx.world, probe, "sky/room.exr");
        assert_ne!(ctx.world.get::<ReflectionProbe3D>(probe).unwrap().content, 0);
    });
}
