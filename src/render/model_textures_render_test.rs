//! Offscreen GPU test of a model's own textures after `ponko cook`: the
//! cooked GLB's KTX2 images (`KHR_texture_basisu`) load as GPU blocks,
//! never decoded to RGBA8, and take a quarter of the video memory the
//! source model's PNGs do once decoded (with the mipmaps 3D materials
//! get), counted the way the engine's GPU memory report counts textures.
//! The fixture is `tests/fixtures/cook/`'s `panel.glb`: a base color, a
//! normal map, and separate metallic-roughness and occlusion images (which
//! the engine merges into one ORM image), 32 x 32 each.
//!
//! The same holds for a model `ponko cook` keeps as it is when its images
//! are files of their own: the fixture's `models/crate.glb`, which reads
//! `crate.png`, `crate_n.png` and `../maps/crate_mr.png` and
//! `../maps/crate_ao.png`. The cook compresses each file for its use and
//! merges the occlusion into the metallic-roughness file, which the engine
//! then uploads as it is. Skips without a GPU that samples BC formats.
use super::memory::{MemoryClass, MemoryTally};
use super::render_test_support::{TestGpu, gpu_with};
use crate::compressed::BlockFormat;
use crate::three::gltf::{self, GpuFormats, Loaded};

const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/cook/source/panel.glb");
const COOKED: &[u8] = include_bytes!("../../tests/fixtures/cook/cooked/panel.glb");

/// The crate model and its image files, from `folder` of the fixture.
macro_rules! crate_files {
    ($folder:literal) => {
        [
            (
                "crate.png",
                &include_bytes!(concat!("../../tests/fixtures/cook/", $folder, "/models/crate.png"))[..],
            ),
            (
                "crate_n.png",
                &include_bytes!(concat!("../../tests/fixtures/cook/", $folder, "/models/crate_n.png"))[..],
            ),
            (
                "crate_mr.png",
                &include_bytes!(concat!("../../tests/fixtures/cook/", $folder, "/maps/crate_mr.png"))[..],
            ),
            (
                "crate_ao.png",
                &include_bytes!(concat!("../../tests/fixtures/cook/", $folder, "/maps/crate_ao.png"))[..],
            ),
        ]
    };
}
const CRATE: &[u8] = include_bytes!("../../tests/fixtures/cook/source/models/crate.glb");

/// The uncooked crate model with the image files `files`, and the image
/// files it read.
fn load_crate(files: &[(&str, &[u8])], gpu: GpuFormats) -> (Loaded, Vec<String>) {
    let mut reads = Vec::new();
    let loaded = gltf::load_for(
        "models/crate.glb",
        CRATE,
        &mut |f| {
            reads.push(f.to_string());
            let name = f.rsplit('/').next().unwrap_or(f);
            match files.iter().find(|(n, _)| *n == name) {
                Some((_, bytes)) => Ok(bytes.to_vec()),
                None => Err(format!("no {f}")),
            }
        },
        &|_| false,
        gpu,
    )
    .unwrap();
    (loaded, reads)
}

fn load(name: &str, bytes: &[u8], gpu: GpuFormats) -> Loaded {
    gltf::load_for(name, bytes, &mut |f| Err(format!("no {f}")), &|_| false, gpu).unwrap()
}

/// Uploads every image of `loaded` as the renderer does (blocks as they
/// are; RGBA8 with the mip chain 3D materials get) and counts them as the
/// GPU memory report does.
fn video_memory(gpu: &TestGpu, loaded: &Loaded) -> u64 {
    let mips = super::mips::MipMaker::new(&gpu.device);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let mut tally = MemoryTally::default();
    for (image, blocks) in loaded.images.iter().zip(&loaded.blocks) {
        let texture = match blocks {
            Some(blocks) => super::blocks::upload(&gpu.device, &gpu.queue, "model texture", &[blocks]),
            None => {
                let (width, height, rgba) = image;
                let size = wgpu::Extent3d {
                    width: *width,
                    height: *height,
                    depth_or_array_layers: 1,
                };
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("model texture"),
                    size,
                    mip_level_count: super::mips::levels(*width, *height),
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
                });
                gpu.queue.write_texture(
                    texture.as_image_copy(),
                    rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * width),
                        rows_per_image: None,
                    },
                    size,
                );
                mips.generate(&gpu.device, &mut encoder, &texture, false);
                texture
            }
        };
        tally.texture(MemoryClass::Textures, "model texture", &texture);
    }
    gpu.queue.submit([encoder.finish()]);
    tally.finish().textures
}

/// The largest difference in channels `channels` between two RGBA8 images.
fn largest_difference(a: &gltf::Image, b: &gltf::Image, channels: std::ops::Range<usize>) -> u8 {
    assert_eq!((a.0, a.1), (b.0, b.1));
    a.2.as_chunks::<4>()
        .0
        .iter()
        .zip(b.2.as_chunks::<4>().0)
        .map(|(p, q)| channels.clone().map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
}

/// A device that samples BC formats, or `None` (the test skips).
fn bc_gpu() -> Option<TestGpu> {
    gpu_with(|adapter| {
        if adapter.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            Ok(wgpu::DeviceDescriptor {
                required_features: wgpu::Features::TEXTURE_COMPRESSION_BC,
                ..Default::default()
            })
        } else {
            Err("needs BC texture compression".into())
        }
    })
}

#[test]
fn a_cooked_models_textures_stay_blocks_and_take_a_quarter_of_the_video_memory() {
    let Some(gpu) = bc_gpu() else {
        return;
    };
    let formats = super::blocks::gpu_formats(gpu.device.features());
    let source = load("source/panel.glb", SOURCE, formats);
    let cooked = load("cooked/panel.glb", COOKED, formats);

    // The source: three RGBA8 images (color, normal, the merged ORM).
    assert_eq!(source.images.len(), 3);
    assert!(source.blocks.iter().all(Option::is_none));
    // The cooked file: the same three, as blocks only.
    assert_eq!(cooked.images.len(), 3);
    let kinds: Vec<(BlockFormat, bool)> = cooked
        .blocks
        .iter()
        .map(|b| {
            let b = b.as_ref().expect("every image stays blocks");
            (b.format, b.srgb)
        })
        .collect();
    let m = &cooked.materials[0];
    let kind = |i: Option<usize>| kinds[i.expect("the material has it")];
    assert_eq!(kind(m.image), (BlockFormat::Bc7, true), "the color: BC7 sRGB");
    assert_eq!(kind(m.normal_map), (BlockFormat::Bc5, false), "the normal map: BC5");
    assert_eq!(kind(m.orm_map), (BlockFormat::Bc7, false), "ORM: BC7, linear");
    for (image, _) in cooked.images.iter().zip(&cooked.blocks) {
        assert!(image.2.is_empty(), "no RGBA8 decoded");
    }

    let before = video_memory(&gpu, &source);
    let after = video_memory(&gpu, &cooked);
    eprintln!(
        "panel.glb on {} ({:?}): textures {before} bytes of video memory from the source, {after} cooked",
        gpu.info.name, gpu.info.backend
    );
    // 32 x 32 RGBA8 with six levels: 5460 bytes each; BC with six levels:
    // 64 + 16 + 4 + 1 + 1 + 1 blocks of 16 bytes.
    assert_eq!(before, 3 * 5460);
    assert_eq!(after, 3 * 87 * 16);

    // The same textures: the cooked images decoded on the CPU are their
    // sources within BC7's and BC5's error on these smooth images.
    let decoded = load("cooked/panel.glb", COOKED, GpuFormats::default());
    let (s, c) = (&source.materials[0], &decoded.materials[0]);
    let pair = |a: Option<usize>, b: Option<usize>| (&source.images[a.unwrap()], &decoded.images[b.unwrap()]);
    let (color, color_cooked) = pair(s.image, c.image);
    let (orm, orm_cooked) = pair(s.orm_map, c.orm_map);
    let (normal, normal_cooked) = pair(s.normal_map, c.normal_map);
    let differences = [
        largest_difference(color, color_cooked, 0..4),
        largest_difference(orm, orm_cooked, 0..1),
        largest_difference(orm, orm_cooked, 1..3),
        largest_difference(normal, normal_cooked, 0..2),
    ];
    eprintln!("largest differences: color, occlusion, roughness and metalness, normal {differences:?}");
    // BC7's bound in `compressed_render_test` (measured here: 1, 12, 13, 2;
    // the ORM image's occlusion has sharp steps).
    assert!(differences.iter().all(|d| *d <= 16), "{differences:?}");
    // Negative controls: the comparison tells images apart, and without
    // the merge red would be 255 (no occlusion), far from the source's.
    assert!(largest_difference(color, orm_cooked, 0..3) > 64);
    let unmerged = (
        orm.0,
        orm.1,
        orm.2
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [255, p[1], p[2], p[3]])
            .collect(),
    );
    assert!(largest_difference(&unmerged, orm_cooked, 0..1) > 64);
}

#[test]
fn an_uncooked_models_cooked_image_files_stay_blocks_with_the_occlusion_merged() {
    let Some(gpu) = bc_gpu() else {
        return;
    };
    let formats = super::blocks::gpu_formats(gpu.device.features());
    let (source, source_reads) = load_crate(&crate_files!("source"), formats);
    let (cooked, cooked_reads) = load_crate(&crate_files!("cooked"), formats);
    assert_eq!(source_reads.len(), 4, "{source_reads:?}");
    assert_eq!(cooked_reads.len(), 3, "the occlusion file isn't read: {cooked_reads:?}");

    assert!(source.blocks.iter().all(Option::is_none));
    let m = &cooked.materials[0];
    let kind = |i: Option<usize>| {
        let b = cooked.blocks[i.expect("the material has it")]
            .as_ref()
            .expect("every image stays blocks");
        (b.format, b.srgb)
    };
    assert_eq!(kind(m.image), (BlockFormat::Bc7, true), "the color: BC7 sRGB");
    assert_eq!(kind(m.normal_map), (BlockFormat::Bc5, false), "the normal map: BC5");
    assert_eq!(kind(m.orm_map), (BlockFormat::Bc7, false), "ORM: BC7, linear");

    let before = video_memory(&gpu, &source);
    let after = video_memory(&gpu, &cooked);
    eprintln!(
        "crate.glb on {} ({:?}): textures {before} bytes of video memory from the source images, {after} cooked",
        gpu.info.name, gpu.info.backend
    );
    assert_eq!(before, 3 * 5460);
    assert_eq!(after, 3 * 87 * 16);

    // The ORM the engine merges from the source files is what the cook
    // merged, within BC7's error (the images are solid colors).
    let decoded = load_crate(&crate_files!("cooked"), GpuFormats::default()).0;
    let (s, c) = (&source.materials[0], &decoded.materials[0]);
    let (orm, orm_cooked) = (&source.images[s.orm_map.unwrap()], &decoded.images[c.orm_map.unwrap()]);
    let difference = largest_difference(orm, orm_cooked, 0..3);
    eprintln!("largest ORM difference: {difference}");
    assert!(difference <= 2, "{difference}");
    // A negative control: when the model names another occlusion file than
    // the note, the cooked file is not trusted and the engine merges the
    // files itself (the same-length name keeps the GLB's offsets).
    let mut renamed = CRATE.to_vec();
    let at = renamed
        .windows(12)
        .position(|w| w == b"crate_ao.png")
        .expect("the model names its occlusion file");
    renamed[at..at + 12].copy_from_slice(b"crate_ax.png");
    let mut files = crate_files!("cooked");
    files[3] = ("crate_ax.png", crate_files!("source")[3].1);
    let mut reads = Vec::new();
    let other = gltf::load_for(
        "models/crate.glb",
        &renamed,
        &mut |f| {
            reads.push(f.to_string());
            let name = f.rsplit('/').next().unwrap_or(f);
            files
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| b.to_vec())
                .ok_or(format!("no {f}"))
        },
        &|_| false,
        formats,
    )
    .unwrap();
    assert!(
        other.blocks[other.materials[0].orm_map.unwrap()].is_none(),
        "merged on the CPU"
    );
    assert!(reads.iter().any(|r| r.ends_with("crate_ax.png")), "{reads:?}");
}

/// A model whose geometry is meshopt-compressed keeps that geometry in the
/// cook, and its own images still become KTX2: `tests/fixtures/cook-kept/`.
#[cfg(feature = "gltf-meshopt")]
#[test]
fn a_meshopt_models_cooked_images_stay_blocks_and_its_geometry_is_unchanged() {
    let Some(gpu) = bc_gpu() else {
        return;
    };
    let formats = super::blocks::gpu_formats(gpu.device.features());
    let source = load(
        "source/packed.glb",
        include_bytes!("../../tests/fixtures/cook-kept/source/packed.glb"),
        formats,
    );
    let cooked = load(
        "cooked/packed.glb",
        include_bytes!("../../tests/fixtures/cook-kept/cooked/packed.glb"),
        formats,
    );
    // The same decompressed triangles.
    assert!(!source.data.indices.is_empty());
    assert_eq!(cooked.data.positions, source.data.positions);
    assert_eq!(cooked.data.normals, source.data.normals);
    assert_eq!(cooked.data.uvs, source.data.uvs);
    assert_eq!(cooked.data.indices, source.data.indices);

    assert!(source.blocks.iter().all(Option::is_none), "the source's PNGs decode");
    let m = &cooked.materials[0];
    let kind = |i: Option<usize>| {
        let b = cooked.blocks[i.expect("the material has it")]
            .as_ref()
            .expect("every image stays blocks");
        (b.format, b.srgb)
    };
    assert_eq!(kind(m.image), (BlockFormat::Bc7, true), "the color: BC7 sRGB");
    assert_eq!(kind(m.normal_map), (BlockFormat::Bc5, false), "the normal map: BC5");
    assert_eq!(kind(m.orm_map), (BlockFormat::Bc7, false), "ORM: BC7, linear");

    let before = video_memory(&gpu, &source);
    let after = video_memory(&gpu, &cooked);
    eprintln!(
        "packed.glb on {} ({:?}): textures {before} bytes of video memory from the source, {after} cooked",
        gpu.info.name, gpu.info.backend
    );
    assert_eq!(before, 3 * 5460);
    assert_eq!(after, 3 * 87 * 16);
}
