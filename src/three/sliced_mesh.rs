//! Streamed meshes decoded a slice at a time, where there are no worker
//! threads (a browser build without threads): the streaming step reads the
//! file, decodes its PNG images a few rows at a time with the asset
//! groups' PNG slicer, and then parses the file a piece at a time
//! ([`gltf::Parse`]): its JSON, each buffer (an inline one a slice of
//! bytes at a time), the document, its checks, its materials (with those
//! images already decoded), a few thousand corners of its primitives at a
//! time, and the end.
//!
//! The mesh is exactly what the one-piece parse makes: `load_for` runs the
//! same steps, the slicer gives the same pixels as `decode_image`, and an
//! image it can't split (another format, an interlaced PNG, a broken one)
//! is left to the parse, which decodes it as always (and fails with the
//! usual message).

use std::collections::HashMap;
use std::sync::Arc;

use super::gltf;
use super::streamed::LoadedFile;
use crate::assets::group::sliced::{PngSlices, SLICE_BYTES};
use crate::assets::{Origin, SharedSources};
use crate::error::Error;

/// Reads the file, on the game thread.
pub(crate) type Read = Box<dyn FnOnce() -> Result<(Origin, Vec<u8>), Error> + Send>;

/// A glTF file decoded a slice at a time.
pub(crate) struct MeshSlices {
    name: String,
    gpu: gltf::GpuFormats,
    phase: Phase,
}

enum Phase {
    /// Not read yet.
    Read(Read),
    /// Read: its images are looked for next.
    Scan(Origin, Vec<u8>),
    /// Its PNG images being decoded, then the parse.
    Images(Box<Images>),
    /// Being parsed, a piece a slice.
    Parse(Origin, Box<gltf::Parse>),
    /// Done (or failed).
    Taken,
}

struct Images {
    origin: Origin,
    bytes: Vec<u8>,
    /// PNG and KTX2 images not started, by image index, last first.
    todo: Vec<(usize, Arc<Vec<u8>>)>,
    current: Option<(usize, ImageSlices)>,
    done: HashMap<usize, gltf::Predecoded>,
}

/// One image being decoded a slice at a time.
enum ImageSlices {
    Png(Box<PngSlices>),
    /// A KTX2 file (`KHR_texture_basisu`): its bytes and its decode.
    #[cfg(feature = "compressed-textures")]
    Ktx2(Arc<Vec<u8>>, Box<crate::compressed::DecodeSlices>),
}

impl ImageSlices {
    /// Starts decoding image `index`, `bytes`, for `gpu`, if it is one the
    /// slicers split.
    fn new(name: &str, index: usize, bytes: Arc<Vec<u8>>, gpu: gltf::GpuFormats) -> Option<Self> {
        #[cfg(feature = "compressed-textures")]
        if crate::compressed::is_ktx2(&bytes) {
            let what = format!("image {index} of {name}");
            let slices = crate::compressed::DecodeSlices::new(&bytes, name, &what, gpu)?.ok()?;
            return Some(ImageSlices::Ktx2(bytes, Box::new(slices)));
        }
        let _ = (name, index, gpu);
        PngSlices::new(&bytes).map(|png| ImageSlices::Png(Box::new(png)))
    }

    /// The next slice: the image once it is decoded (as the parse decodes
    /// it for `gpu`), else the rest to do; `None` if it failed (the parse
    /// decodes it again, failing with its message).
    fn step(self, gpu: gltf::GpuFormats) -> Option<Result<gltf::Predecoded, Self>> {
        #[cfg(not(feature = "compressed-textures"))]
        let _ = gpu;
        match self {
            ImageSlices::Png(mut png) => match png.step() {
                Ok(false) => Some(Err(ImageSlices::Png(png))),
                Ok(true) => {
                    let image = png.finish();
                    Some(Ok(((image.width, image.height, image.rgba), None, None)))
                }
                Err(_) => None,
            },
            #[cfg(feature = "compressed-textures")]
            ImageSlices::Ktx2(bytes, mut slices) => match slices.step(&bytes).ok()? {
                None => Some(Err(ImageSlices::Ktx2(bytes, slices))),
                Some(crate::compressed::Decoded::Rgba(i)) => Some(Ok(((i.width, i.height, i.rgba), None, Some(gpu)))),
                Some(crate::compressed::Decoded::Blocks(b)) => {
                    Some(Ok(((b.width, b.height, Vec::new()), Some(b), Some(gpu))))
                }
            },
        }
    }
}

impl MeshSlices {
    /// Decodes what `read` gives.
    pub fn new(name: &str, gpu: gltf::GpuFormats, read: Read) -> Self {
        Self {
            name: name.to_string(),
            gpu,
            phase: Phase::Read(read),
        }
    }

    /// Decodes `bytes`, read already from `origin`.
    pub fn read(name: &str, gpu: gltf::GpuFormats, origin: Origin, bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            gpu,
            phase: Phase::Scan(origin, bytes),
        }
    }

    /// PNG images decoded so far, for tests.
    #[cfg(test)]
    pub fn images_done(&self) -> usize {
        match &self.phase {
            Phase::Images(images) => images.done.len(),
            _ => 0,
        }
    }
}

impl crate::streaming::StreamSlices<(Origin, LoadedFile)> for MeshSlices {
    fn step(&mut self, sources: &SharedSources) -> Result<Option<(Origin, LoadedFile)>, Error> {
        let mut read = |file: &str| sources.read(file);
        match std::mem::replace(&mut self.phase, Phase::Taken) {
            Phase::Read(read) => {
                let (origin, bytes) = read().map_err(|e| e.with_path(&self.name))?;
                self.phase = Phase::Scan(origin, bytes);
                Ok(None)
            }
            Phase::Scan(origin, bytes) => {
                let mut todo = png_images(&self.name, &bytes, &mut read);
                todo.reverse();
                self.phase = Phase::Images(Box::new(Images {
                    origin,
                    bytes,
                    todo,
                    current: None,
                    done: HashMap::new(),
                }));
                Ok(None)
            }
            Phase::Images(mut images) => {
                if images.current.is_none()
                    && let Some((index, bytes)) = images.todo.pop()
                {
                    images.current = ImageSlices::new(&self.name, index, bytes, self.gpu).map(|i| (index, i));
                }
                if let Some((index, image)) = images.current.take() {
                    match image.step(self.gpu) {
                        Some(Err(rest)) => images.current = Some((index, rest)),
                        Some(Ok(decoded)) => {
                            images.done.insert(index, decoded);
                        }
                        None => {}
                    }
                    self.phase = Phase::Images(images);
                    return Ok(None);
                }
                if !images.todo.is_empty() {
                    self.phase = Phase::Images(images);
                    return Ok(None);
                }
                let Images {
                    origin, bytes, done, ..
                } = *images;
                let parse = gltf::Parse::new(&self.name, &bytes, self.gpu)
                    .map_err(|e| Error::decode(&self.name, e))?
                    .with_images(done);
                self.phase = Phase::Parse(origin, Box::new(parse));
                Ok(None)
            }
            Phase::Parse(origin, mut parse) => {
                match parse
                    .step(&mut read, &|_| false, SLICE_BYTES)
                    .map_err(|e| Error::decode(&self.name, e))?
                {
                    Some(loaded) => Ok(Some((origin, LoadedFile::Parsed(Box::new(loaded))))),
                    None => {
                        self.phase = Phase::Parse(origin, parse);
                        Ok(None)
                    }
                }
            }
            Phase::Taken => Err(Error::new(
                crate::error::ErrorKind::Invalid,
                format!("{} was decoded already", self.name),
            )),
        }
    }
}

/// The PNG and KTX2 images that file `name`'s textures use (their own
/// image, and a `KHR_texture_basisu` one with the `compressed-textures`
/// feature), by image index, in order: their bytes, from the file's binary
/// chunk, its buffers or files of their own. What can't be found here (a
/// broken file, a data URI that isn't base64) is left out: the parse finds
/// it, or says what is wrong.
fn png_images(
    name: &str,
    bytes: &[u8],
    read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Vec<(usize, Arc<Vec<u8>>)> {
    use serde_json::Value;
    let (json, bin) = if bytes.starts_with(b"glTF") {
        match ::gltf::Glb::from_slice(bytes) {
            Ok(glb) => (glb.json, glb.bin),
            Err(_) => return Vec::new(),
        }
    } else {
        (std::borrow::Cow::Borrowed(bytes), None)
    };
    let Ok(root) = serde_json::from_slice::<Value>(&json) else {
        return Vec::new();
    };
    let list = |key: &str| root[key].as_array().map_or(&[][..], Vec::as_slice);
    let index = |v: &Value| v.as_u64().and_then(|i| usize::try_from(i).ok());
    let mut used: Vec<usize> = Vec::new();
    for texture in list("textures") {
        used.extend(index(&texture["source"]));
        if cfg!(feature = "compressed-textures") {
            used.extend(index(&texture["extensions"]["KHR_texture_basisu"]["source"]));
        }
    }
    used.sort_unstable();
    used.dedup();
    let folder = gltf::folder_of(name);
    let mut buffers: HashMap<usize, Option<Vec<u8>>> = HashMap::new();
    let mut out = Vec::new();
    for at in used {
        let Some(image) = list("images").get(at) else {
            continue;
        };
        let data = if let Some(view) = index(&image["bufferView"]) {
            let Some(view) = list("bufferViews").get(view) else {
                continue;
            };
            let (Some(buffer), Some(length)) = (index(&view["buffer"]), index(&view["byteLength"])) else {
                continue;
            };
            let start = index(&view["byteOffset"]).unwrap_or(0);
            let range = start..start.saturating_add(length);
            match list("buffers").get(buffer).and_then(|b| b["uri"].as_str()) {
                None => bin.as_deref().and_then(|bin| bin.get(range)).map(<[u8]>::to_vec),
                Some(uri) => buffers
                    .entry(buffer)
                    .or_insert_with(|| gltf::fetch(uri, folder, read).ok())
                    .as_deref()
                    .and_then(|b| b.get(range))
                    .map(<[u8]>::to_vec),
            }
        } else if let Some(uri) = image["uri"].as_str() {
            gltf::fetch(uri, folder, read).ok()
        } else {
            None
        };
        if let Some(data) = data
            && (data.starts_with(b"\x89PNG\r\n\x1a\n") || is_ktx2(&data))
        {
            out.push((at, Arc::new(data)));
        }
    }
    out
}

/// Whether `bytes` are a KTX2 file the build reads.
fn is_ktx2(bytes: &[u8]) -> bool {
    #[cfg(feature = "compressed-textures")]
    return crate::compressed::is_ktx2(bytes);
    #[cfg(not(feature = "compressed-textures"))]
    {
        let _ = bytes;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::MemorySource;
    use crate::prelude::*;
    use crate::streaming::{AssetState, ResidencyKind, StreamSlices};

    fn noise_png(side: u32, seed: u32) -> Vec<u8> {
        let mut s = seed.wrapping_mul(0x9E37_79B9) | 1;
        let image = image::RgbaImage::from_fn(side, side, |_, _| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            image::Rgba([s as u8, (s >> 8) as u8, (s >> 16) as u8, 255])
        });
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// A GLB of one textured triangle: its base color image is `embedded`,
    /// in the file's binary chunk, and its emissive image the file
    /// `glow.png` next to it.
    fn glb(embedded: &[u8]) -> Vec<u8> {
        glb_with(
            embedded,
            r#"[{"source":0},{"source":1}]"#,
            r#"[{"bufferView":3,"mimeType":"image/png"},{"uri":"glow.png"}]"#,
            "",
        )
    }

    /// [`glb`] with its own `textures` and `images`, and `extras` JSON
    /// members.
    fn glb_with(embedded: &[u8], textures: &str, images: &str, extras: &str) -> Vec<u8> {
        let mut bin = Vec::new();
        for v in [
            0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
        ] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2, 0] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let image_at = bin.len();
        bin.extend_from_slice(embedded);
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}},"emissiveTexture":{{"index":1}}}}],
            "textures":{textures},
            "images":{images},{extras}
            "buffers":[{{"byteLength":{}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},
                {{"buffer":0,"byteOffset":60,"byteLength":6}},{{"buffer":0,"byteOffset":{image_at},"byteLength":{}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                {{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}},
                {{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
            bin.len(),
            embedded.len()
        );
        let mut json = json.into_bytes();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn sliced_meshes_decode_their_png_images_a_few_rows_a_slice_into_what_the_parse_makes() {
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            let (base, glow) = (noise_png(256, 1), noise_png(128, 2));
            ctx.assets.add_source(MemorySource::new("mem").with("m/glow.png", glow));
            let file = glb(&base);
            let sources = ctx.gfx.asset_sources.clone();
            let mut read = |f: &str| sources.read(f);
            let whole = gltf::load("m/tri.glb", &file, &mut read, &|_| false).unwrap();
            assert_eq!(whole.images.len(), 2);

            let origin = Origin::File("m/tri.glb".into());
            let mut slices = MeshSlices::read("m/tri.glb", gltf::GpuFormats::default(), origin.clone(), file);
            let mut steps = 0;
            let mut most_images = 0;
            let (got_origin, loaded) = loop {
                steps += 1;
                most_images = most_images.max(slices.images_done());
                if let Some(done) = slices.step(&sources).unwrap() {
                    break done;
                }
            };
            assert_eq!(got_origin, origin);
            let LoadedFile::Parsed(loaded) = loaded else {
                panic!("parsed on the game thread")
            };
            // Both images were decoded by the slicer before the parse.
            assert_eq!(most_images, 2);
            // The scan; 64 KiB of RGBA a slice: 64 rows of the 256-pixel
            // image (4 slices) and all 128 rows of the other (1); the parse:
            // its JSON, the binary chunk, the document, its checks, the
            // materials, and the one triangle with the end.
            assert_eq!(steps, 1 + 4 + 1 + 6);
            assert!(loaded.images == whole.images, "the slices made other pixels");
            assert_eq!(loaded.data.positions.len(), whole.data.positions.len());
            assert_eq!(loaded.parts, whole.parts);
        });
    }

    #[test]
    fn a_mesh_image_the_slicer_cannot_decode_is_left_to_the_parse() {
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            let mut cut = noise_png(256, 1);
            cut.truncate(cut.len() / 2);
            ctx.assets
                .add_source(MemorySource::new("mem").with("m/glow.png", noise_png(8, 2)));
            let sources = ctx.gfx.asset_sources.clone();
            let mut slices = MeshSlices::read(
                "m/tri.glb",
                gltf::GpuFormats::default(),
                Origin::File("m/tri.glb".into()),
                glb(&cut),
            );
            let error = loop {
                match slices.step(&sources) {
                    Ok(None) => {}
                    Ok(Some(_)) => panic!("a broken image loaded"),
                    Err(e) => break e,
                }
            };
            assert_eq!(
                (error.kind(), error.path()),
                (crate::error::ErrorKind::Decode, Some("m/tri.glb"))
            );
            assert!(error.to_string().contains("an image can't be read"), "{error}");
        });
    }

    #[test]
    fn without_workers_a_streamed_mesh_decodes_within_the_frame_budget_and_uploads() {
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            ctx.gfx.streaming.sliced = Some(true);
            ctx.assets.groups.piece_cost = Some(0.125 / 1000.0);
            ctx.assets.set_group_frame_budget(0.25);
            ctx.assets.add_source(
                MemorySource::new("mem")
                    .with("m/tri.glb", glb(&noise_png(256, 1)))
                    .with("m/glow.png", noise_png(128, 2)),
            );
            assert_eq!(ctx.gfx.stream_mesh("m/tri.glb"), None);
            let mut frames = Vec::new();
            while ctx.gfx.asset_state(ResidencyKind::Mesh, "m/tri.glb") != AssetState::Resident {
                ctx.pump_asset_groups();
                ctx.pump_streaming();
                frames.push(ctx.gfx.upload_stats().frame_slices);
                assert!(frames.len() < 100, "never loaded: {frames:?}");
            }
            // Two pieces a frame: the scan, five image slices, the JSON, the
            // binary chunk, the document, its checks, the materials, the
            // triangle.
            assert!(frames.iter().all(|&n| n <= 2), "{frames:?}");
            assert_eq!(frames.iter().sum::<u32>(), 12, "{frames:?}");
            assert!(ctx.gfx.stream_mesh("m/tri.glb").is_some());
        });
    }

    /// A glTF file of one mesh of eight primitives, of 150 to 1200
    /// triangles each (deterministic noise), on two nodes (the second a
    /// child of the first, moved), its buffer `uri`: the buffer's bytes.
    fn grid_gltf(uri: &str) -> (String, Vec<u8>) {
        let mut bin = Vec::new();
        let mut s = 0x2545_F491u32;
        let mut primitives = Vec::new();
        let (mut views, mut accessors) = (Vec::new(), Vec::new());
        for k in 0..8 {
            let corners = (k + 1) * 150 * 3;
            let at = bin.len();
            for _ in 0..corners * 3 {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                bin.extend_from_slice(&((s % 2000) as f32 / 100.0 - 10.0).to_le_bytes());
            }
            views.push(format!(
                r#"{{"buffer":0,"byteOffset":{at},"byteLength":{}}}"#,
                corners * 12
            ));
            accessors.push(format!(
                r#"{{"bufferView":{},"componentType":5126,"count":{corners},"type":"VEC3","min":[-10,-10,-10],"max":[10,10,10]}}"#,
                views.len() - 1
            ));
            let at = bin.len();
            for i in 0..corners as u32 {
                bin.extend_from_slice(&(corners as u32 - 1 - i).to_le_bytes());
            }
            views.push(format!(
                r#"{{"buffer":0,"byteOffset":{at},"byteLength":{}}}"#,
                corners * 4
            ));
            accessors.push(format!(
                r#"{{"bufferView":{},"componentType":5125,"count":{corners},"type":"SCALAR"}}"#,
                views.len() - 1
            ));
            primitives.push(format!(
                r#"{{"attributes":{{"POSITION":{}}},"indices":{}}}"#,
                accessors.len() - 2,
                accessors.len() - 1
            ));
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"children":[1]}},{{"mesh":0,"translation":[3,0,0]}}],
            "meshes":[{{"primitives":[{}]}}],
            "buffers":[{{"byteLength":{},"uri":"{uri}"}}],
            "bufferViews":[{}],"accessors":[{}]}}"#,
            primitives.join(","),
            bin.len(),
            views.join(","),
            accessors.join(",")
        );
        (json, bin)
    }

    /// Base64 of `bytes`.
    fn base64(bytes: &[u8]) -> String {
        const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// Parses `json` a piece at a time as the streaming step does: the
    /// loaded file, and the pieces each phase took (the walk's with the
    /// corners each added).
    fn parse_in_slices(
        name: &str,
        json: &str,
        read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> (gltf::Loaded, HashMap<&'static str, usize>, Vec<usize>) {
        let mut parse = gltf::Parse::new(name, json.as_bytes(), gltf::GpuFormats::default()).unwrap();
        let (mut pieces, mut walk) = (HashMap::new(), Vec::new());
        loop {
            let phase = parse.phase();
            *pieces.entry(phase).or_insert(0) += 1;
            let before = parse.corners();
            match parse.step(read, &|_| false, SLICE_BYTES).unwrap() {
                Some(loaded) => return (loaded, pieces, walk),
                None if phase == "walk" => walk.push(parse.corners() - before),
                None => {}
            }
        }
    }

    #[test]
    fn a_sliced_parse_decodes_an_inline_buffer_a_slice_of_bytes_at_a_time_into_the_same_mesh() {
        let (json, bin) = grid_gltf("");
        let uri = format!("data:application/octet-stream;base64,{}", base64(&bin));
        let json = json.replace(r#""uri":"""#, &format!(r#""uri":"{uri}""#));
        let mut nothing = |f: &str| Err(format!("{f} isn't there"));
        let whole = gltf::load("m/grid.gltf", json.as_bytes(), &mut nothing, &|_| false).unwrap();
        let (sliced, pieces, walk) = parse_in_slices("m/grid.gltf", &json, &mut nothing);
        assert!(sliced.data == whole.data, "the slices made another mesh");
        assert_eq!(sliced.parts, whole.parts);
        // 64 KiB of bytes a slice: as many slices as that takes.
        assert_eq!(pieces["buffers"], bin.len().div_ceil(SLICE_BYTES));
        assert!(pieces["buffers"] > 1);
        // Each walk slice adds at most a slice's worth of corners, plus
        // the primitive that crossed it (the largest is 3600 corners).
        let per_slice = SLICE_BYTES / gltf::CORNER_BYTES;
        let corners: usize = 2 * (1..=8).map(|k| k * 450).sum::<usize>();
        assert_eq!(whole.data.positions.len(), corners);
        assert!(walk.iter().all(|&n| n < per_slice + 3600), "{walk:?}");
        assert!(walk.len() + 1 >= corners / (per_slice + 3600), "{walk:?}");
        assert!(walk.len() > 4, "{walk:?}");
    }

    #[test]
    fn a_sliced_parse_reads_a_buffer_file_in_its_own_slice_into_the_same_mesh() {
        let (json, bin) = grid_gltf("grid.bin");
        let mut reads = 0;
        let mut read = |f: &str| {
            reads += 1;
            if f == "m/grid.bin" {
                Ok(bin.clone())
            } else {
                Err(format!("{f} isn't there"))
            }
        };
        let whole = gltf::load("m/grid.gltf", json.as_bytes(), &mut read, &|_| false).unwrap();
        let (sliced, pieces, _) = parse_in_slices("m/grid.gltf", &json, &mut read);
        assert!(sliced.data == whole.data, "the slices made another mesh");
        assert_eq!(sliced.parts, whole.parts);
        assert_eq!(pieces["buffers"], 1);
        assert_eq!(reads, 2, "read once by each parse");
    }

    /// The images and blocks of `a` and `b` are the same.
    #[cfg(feature = "compressed-textures")]
    fn same_images(a: &gltf::Loaded, b: &gltf::Loaded) -> bool {
        a.images == b.images && a.blocks == b.blocks
    }

    #[cfg(feature = "compressed-textures")]
    #[test]
    fn sliced_meshes_decode_their_ktx2_images_in_slices_into_what_the_parse_makes() {
        use crate::compressed::{BlockFormat, GpuFormats, tests};
        Game::new("t", 1, 1).hot_reload(false).run_headless(0, |ctx| {
            let levels = tests::noise_levels(BlockFormat::Bc7, 128, 128);
            let ktx2 = tests::ktx2_file(Some(ktx2::Format::BC7_UNORM_BLOCK), 128, 128, &levels, true);
            ctx.assets
                .add_source(MemorySource::new("mem").with("m/glow.png", noise_png(64, 2)));
            let sources = ctx.gfx.asset_sources.clone();
            // As the base color image, and as the `KHR_texture_basisu`
            // image of a texture whose own image is a PNG.
            let files = [
                glb(&ktx2),
                glb_with(
                    &ktx2,
                    r#"[{"source":1,"extensions":{"KHR_texture_basisu":{"source":0}}},{"source":1}]"#,
                    r#"[{"bufferView":3,"mimeType":"image/ktx2"},{"uri":"glow.png"}]"#,
                    r#""extensionsUsed":["KHR_texture_basisu"],"#,
                ),
            ];
            for file in files {
                // Kept as BC7 blocks, and decoded to RGBA8 on the CPU.
                for gpu in [GpuFormats::ALL, GpuFormats::default()] {
                    let mut read = |f: &str| sources.read(f);
                    let whole = gltf::load_for("m/tri.glb", &file, &mut read, &|_| false, gpu).unwrap();
                    let origin = Origin::File("m/tri.glb".into());
                    let mut slices = MeshSlices::read("m/tri.glb", gpu, origin, file.clone());
                    let (mut steps, mut most_images) = (0, 0);
                    let loaded = loop {
                        steps += 1;
                        most_images = most_images.max(slices.images_done());
                        if let Some((_, loaded)) = slices.step(&sources).unwrap() {
                            break loaded;
                        }
                    };
                    let LoadedFile::Parsed(loaded) = loaded else {
                        panic!("parsed on the game thread")
                    };
                    assert_eq!(most_images, 2, "{gpu:?}: both decoded before the parse");
                    assert!(same_images(&loaded, &whole), "{gpu:?}: the slices made other images");
                    assert_eq!(loaded.blocks.iter().flatten().count(), usize::from(gpu.bc));
                    assert_eq!(loaded.data, whole.data);
                    // The KTX2 file: its eight levels, the choice, and for
                    // RGBA8 the 64 KiB of the decode in one band.
                    let ktx2_slices = 8 + 1 + usize::from(!gpu.bc);
                    // The scan, the images, the parse's six.
                    assert_eq!(steps, 1 + ktx2_slices + 1 + 6, "{gpu:?}");
                }
            }
        });
    }
}
