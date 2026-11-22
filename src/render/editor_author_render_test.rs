//! The editor's authoring previews drawn by the production renderer,
//! offscreen: a glTF material edit recolours its model on screen, a shader
//! edit shows and a broken one keeps the last good look, a painted terrain
//! layer shows where it was painted, the particle section's live preview
//! draws particles while the game is paused, and the animation graph's
//! preview poses its skeleton by the test values. Each checks a picture
//! against one made without the edit. Skips without a GPU (a skip is not a
//! pass).
use glam::{Vec2, Vec3, vec3};
use serde_json::json;

use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::assets::MemorySource;
use crate::ctx::Ctx;
use crate::render::FrameStatus;

const SIZE: (u32, u32) = (96, 96);

/// One frame of `ctx`, captured.
fn shot(ctx: &mut Ctx, name: &str) -> Image {
    let path = std::env::temp_dir().join(format!(
        "ponko-author-{name}-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let picture = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    let image = Image {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    };
    keep_image(&image);
    image
}

/// The mean colour of the middle quarter of `image`.
fn middle(image: &Image) -> [f32; 3] {
    let (w, h) = (image.width, image.height);
    let mut sum = [0.0f32; 3];
    let mut n = 0.0;
    for y in h * 3 / 8..h * 5 / 8 {
        for x in w * 3 / 8..w * 5 / 8 {
            let p = image.pixel(x, y);
            for c in 0..3 {
                sum[c] += f32::from(p[c]);
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

/// A 2 × 2 quad facing +Z at the origin, with one material; its buffer is
/// `quad_bin`.
fn quad_gltf(material: serde_json::Value) -> Vec<u8> {
    let gltf = json!({
        "asset": {"version": "2.0"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": 0}]}],
        "materials": [material],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"}
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 48},
            {"buffer": 0, "byteOffset": 48, "byteLength": 48},
            {"buffer": 0, "byteOffset": 96, "byteLength": 12}
        ],
        "buffers": [{"byteLength": 108, "uri": "quad.bin"}]
    });
    serde_json::to_vec(&gltf).unwrap_or_default()
}

fn quad_bin() -> Vec<u8> {
    let positions = [-1.0f32, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0, 0.0];
    let normals = [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let mut bin: Vec<u8> = positions.iter().chain(&normals).flat_map(|f| f.to_le_bytes()).collect();
    bin.extend([0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
    bin
}

/// A context drawing offscreen, with the camera 3 units in front of the
/// origin and no sky.
fn stage() -> Option<Ctx> {
    let mut ctx = offscreen_ctx(SIZE)?;
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 3.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some(ctx)
}

#[test]
fn a_gltf_material_edit_recolours_its_model_on_screen() {
    let Some(mut ctx) = stage() else { return };
    let white = json!({"name": "Paint", "emissiveFactor": [1.0, 1.0, 1.0],
        "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "metallicFactor": 0.0}});
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("quad.gltf", quad_gltf(white.clone()))
            .with("quad.bin", quad_bin()),
    );
    let mesh = ctx.gfx.try_load_mesh("quad.gltf").unwrap();
    ctx.world
        .spawn((crate::d3::Transform::default(), crate::render::Model::new(mesh)));
    shot(&mut ctx, "material-warmup");
    let before = middle(&shot(&mut ctx, "material-before"));
    // Negative control: as loaded, it's white, not red.
    assert!(
        before[0] > 150.0 && (before[0] - before[1]).abs() < 30.0,
        "white before: {before:?}"
    );
    let mut red = white;
    red["emissiveFactor"] = json!([1.0, 0.0, 0.0]);
    red["pbrMetallicRoughness"]["baseColorFactor"] = json!([1.0, 0.0, 0.0, 1.0]);
    let applied = ctx
        .gfx
        .apply_file_materials("quad.gltf", &quad_gltf(red), true)
        .unwrap();
    assert!(applied, "the file is loaded");
    let after = middle(&shot(&mut ctx, "material-after"));
    assert!(
        after[0] > 150.0 && after[1] < 60.0 && after[2] < 60.0,
        "red after the edit: {after:?}"
    );
}

#[test]
fn a_shader_edit_shows_on_screen_and_a_broken_one_keeps_the_last_good_look() {
    let Some(mut ctx) = stage() else { return };
    let unlit = |rgb: &str| {
        format!(
            "fn surface(s: Surface) -> Surface {{\n    var out = s;\n    out.unlit = true;\n    out.albedo = vec3<f32>({rgb});\n    return out;\n}}\n"
        )
    };
    ctx.assets
        .add_source(MemorySource::new("mem").with("tint.wgsl", unlit("0.0, 1.0, 0.0")));
    let shader = ctx.gfx.try_load_shader("tint.wgsl").unwrap();
    let material = ctx
        .gfx
        .create_material(crate::render::MaterialDesc::default().with_shader(Some(shader)));
    let quad = crate::render::MeshData::cube(2.0);
    let mesh = ctx.gfx.create_mesh(&quad, material);
    ctx.world
        .spawn((crate::d3::Transform::default(), crate::render::Model::new(mesh)));
    shot(&mut ctx, "shader-warmup");
    let green = middle(&shot(&mut ctx, "shader-before"));
    assert!(green[1] > 150.0 && green[0] < 60.0, "green before: {green:?}");
    assert_eq!(ctx.gfx.loaded_shader("tint.wgsl"), Some(shader));
    ctx.gfx
        .replace_shader_source(shader, "tint.wgsl", &unlit("1.0, 0.0, 0.0"))
        .unwrap();
    let red = middle(&shot(&mut ctx, "shader-after"));
    assert!(red[0] > 150.0 && red[1] < 60.0, "red after the edit: {red:?}");
    // A broken edit: an error, and the picture stays.
    let broken = unlit("0.0, 0.0, 1.0").replace("return out;", "return out");
    assert!(crate::gfx::Graphics::check_material_shader("tint.wgsl", &broken).is_err());
    assert!(ctx.gfx.replace_shader_source(shader, "tint.wgsl", &broken).is_err());
    let kept = middle(&shot(&mut ctx, "shader-kept"));
    assert!(kept[0] > 150.0 && kept[2] < 60.0, "the last good look stays: {kept:?}");
}

fn solid_png(rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 4, 4);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = (0..16).flat_map(|_| rgba).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&pixels))
        .unwrap();
    out
}

#[test]
fn a_painted_terrain_layer_shows_where_it_was_painted() {
    let Some(mut ctx) = stage() else { return };
    // A flat 16-unit tile seen from above, red ground and a green layer.
    ctx.scene3d.camera.position = vec3(0.0, 14.0, 0.001);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let file = json!({"format": "ponko-terrain", "size": 16.0, "heights": "t-heights.png", "low": -1.0, "high": 1.0,
        "layers": [{"texture": "red.png"}, {"texture": "green.png"}], "weights": ["t-weights.png"], "roughness": 1.0});
    let side = 16usize;
    let weights: Vec<u8> = (0..side * side).flat_map(|_| [255u8, 0, 0, 0]).collect();
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("t.json", serde_json::to_vec(&file).unwrap())
            .with("t-heights.png", crate::three::encode_grey16(9, &[32768; 81]).unwrap())
            .with(
                "t-weights.png",
                crate::three::encode_rgba(side as u32, &weights).unwrap(),
            )
            .with("red.png", solid_png([220, 30, 30, 255]))
            .with("green.png", solid_png([30, 220, 30, 255])),
    );
    let e = crate::three::TerrainTile::spawn_from_file(&mut ctx, "t.json", vec3(-8.0, 0.0, -8.0)).unwrap();
    shot(&mut ctx, "terrain-warmup");
    let before = middle(&shot(&mut ctx, "terrain-before"));
    assert!(before[0] > before[1] + 40.0, "red ground before: {before:?}");
    // Paint the middle green, as the brush does, and make the tile again.
    let (_, text) = ctx.assets.read_text("t.json").unwrap();
    let sources = ctx.assets.asset_sources.clone();
    let mut data = crate::three::TerrainData::read("t.json", &text, &mut |n| {
        sources.find(n)?.into_bytes().map_err(|e| e.with_path(n))
    })
    .unwrap();
    for y in 4..12 {
        for x in 4..12 {
            data.weights[0][(y * side + x) * 4..(y * side + x) * 4 + 4].copy_from_slice(&[0, 255, 0, 0]);
        }
    }
    let layers = crate::three::load_terrain_layers(&mut ctx, "t.json", &data).unwrap();
    let built = crate::three::build_terrain(&mut ctx, &data, Vec2::new(-8.0, -8.0), layers).unwrap();
    let old = *ctx.world.get::<crate::terrain::TerrainTile>(e).unwrap();
    ctx.world.insert(e, built.tile);
    ctx.free(old);
    let after = shot(&mut ctx, "terrain-after");
    let centre = middle(&after);
    assert!(centre[1] > centre[0] + 40.0, "green where painted: {centre:?}");
    let corner = after.pixel(after.width / 16, after.height / 16);
    assert!(corner[0] > corner[1], "red where not painted: {corner:?}");
}

#[test]
fn a_moved_terrain_tile_keeps_its_painted_layer_under_it() {
    let Some(mut ctx) = stage() else { return };
    // A flat 16-unit tile, red ground, its middle painted green.
    let file = json!({"format": "ponko-terrain", "size": 16.0, "heights": "t-heights.png", "low": -1.0, "high": 1.0,
        "layers": [{"texture": "red.png"}, {"texture": "green.png"}], "weights": ["t-weights.png"], "roughness": 1.0});
    let side = 16usize;
    let weights: Vec<u8> = (0..side * side)
        .flat_map(|i| {
            let (x, y) = (i % side, i / side);
            if (4..12).contains(&x) && (4..12).contains(&y) {
                [0u8, 255, 0, 0]
            } else {
                [255, 0, 0, 0]
            }
        })
        .collect();
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("t.json", serde_json::to_vec(&file).unwrap())
            .with("t-heights.png", crate::three::encode_grey16(9, &[32768; 81]).unwrap())
            .with(
                "t-weights.png",
                crate::three::encode_rgba(side as u32, &weights).unwrap(),
            )
            .with("red.png", solid_png([220, 30, 30, 255]))
            .with("green.png", solid_png([30, 220, 30, 255])),
    );
    let e = crate::three::TerrainTile::spawn_from_file(&mut ctx, "t.json", vec3(-8.0, 0.0, -8.0)).unwrap();
    let tile = *ctx.world.get::<crate::terrain::TerrainTile>(e).unwrap();
    // The camera looks down on where the tile goes: 40 units along +X.
    ctx.scene3d.camera.position = vec3(40.0, 14.0, 0.001);
    ctx.scene3d.camera.look_at(vec3(40.0, 0.0, 0.0));
    ctx.world.get_mut::<crate::d3::Transform>(e).unwrap().position = vec3(32.0, 0.0, -8.0);
    // Negative control: while a drag holds it, the paint stays at the old place.
    ctx.gfx.assets3d.terrain_files.held = Some(ctx.gfx.frame);
    shot(&mut ctx, "moved-warmup");
    let held = middle(&shot(&mut ctx, "moved-held"));
    assert!(
        held[0] > held[1] + 40.0,
        "the paint is left behind while held: {held:?}"
    );
    // The drag ends: the painted layer follows the tile, the same tile.
    ctx.gfx.assets3d.terrain_files.held = None;
    let after = shot(&mut ctx, "moved-after");
    let centre = middle(&after);
    assert!(
        centre[1] > centre[0] + 40.0,
        "green under the moved tile's middle: {centre:?}"
    );
    let corner = after.pixel(after.width / 16, after.height / 16);
    assert!(corner[0] > corner[1], "red where not painted: {corner:?}");
    assert_eq!(
        *ctx.world.get::<crate::terrain::TerrainTile>(e).unwrap(),
        tile,
        "re-placed, not rebuilt"
    );
}

#[test]
fn the_particle_sections_live_preview_draws_particles_while_paused() {
    use crate::inspector::author_harness::{Harness, SCREEN};
    let frame = |select: bool| -> Option<Image> {
        // The editor lays the view out on its own screen: draw at its size.
        let mut ctx = offscreen_ctx((SCREEN.x as u32, SCREEN.y as u32))?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.camera.position = vec3(0.0, 1.0, 6.0);
        ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
        let settings = crate::d3::ParticleSettings::default()
            .with_rate(400.0)
            .with_size(0.25, 0.25)
            .with_gravity(Vec3::ZERO);
        let e = ctx.world.spawn((
            crate::d3::Transform::at(vec3(0.0, 1.0, 0.0)),
            crate::d3::ParticleEmitter::new(settings),
        ));
        // As the game's frame does for a scene of particles alone: the
        // first 3D resource starts the 3D renderer.
        let _ = ctx.gfx.default_material();
        let mut h = Harness::with_ctx(ctx);
        if select {
            h.select(e);
        }
        for _ in 0..30 {
            h.frame(vec![]);
        }
        let alive = h
            .ctx
            .world
            .get::<crate::d3::ParticleEmitter>(e)
            .map_or(0, |p| p.live_count());
        assert_eq!(alive > 0, select, "particles only with the preview: {alive}");
        Some(shot(
            &mut h.ctx,
            if select { "particles-preview" } else { "particles-none" },
        ))
    };
    let Some(none) = frame(false) else { return };
    let Some(preview) = frame(true) else { return };
    // Negative control: unselected, nothing simulates, so nothing shows.
    let changed = (0..preview.height)
        .flat_map(|y| (0..preview.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (a, b) = (none.pixel(x, y), preview.pixel(x, y));
            (0..3).any(|c| a[c].abs_diff(b[c]) > 40)
        })
        .count();
    assert!(changed > 200, "the preview's particles show: {changed} pixels changed");
}

/// Paints the editor's frame (`shapes`, with every texture egui asked for
/// so far) into `target`, over a dark clear.
fn paint_editor(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::Texture,
    h: &crate::inspector::author_harness::Harness,
    shapes: Vec<egui::epaint::ClippedShape>,
) {
    let mut renderer = egui_wgpu::Renderer::new(
        device,
        target.format(),
        egui_wgpu::RendererOptions {
            msaa_samples: 1,
            depth_stencil_format: None,
            dithering: false,
            predictable_texture_filtering: true,
        },
    );
    for (id, deltas) in &h.textures.set {
        for delta in deltas {
            renderer.update_texture(device, queue, *id, delta);
        }
    }
    let primitives = h.egui.tessellate(shapes, 1.0);
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [target.width(), target.height()],
        pixels_per_point: 1.0,
    };
    let view = target.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut buffers = renderer.update_buffers(device, queue, &mut encoder, &primitives, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
    }
    buffers.push(encoder.finish());
    queue.submit(buffers);
}

#[test]
fn the_animation_graph_preview_poses_the_skeleton_by_the_test_values() {
    use crate::inspector::author_harness::{Harness, SCREEN, field_right_of};
    use crate::inspector::graph_panel::preview_id;
    let Some(gpu) = super::render_test_support::gpu() else {
        return;
    };
    // An arm along +X in "idle", turned 90° in "wave"; wave when speed > 0.5.
    let name = format!("ponko-test-graph-gpu-{}.json", std::process::id());
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(&name);
    let skeleton = json!({
        "format": "ponko-skeleton2d", "version": 1,
        "bones": [{"name": "root"}, {"name": "arm", "parent": "root", "length": 40}],
        "clips": [
            {"name": "idle", "bones": [{"bone": "arm", "rotate": [{"time": 0, "value": 0}, {"time": 1, "value": 0}]}]},
            {"name": "wave", "bones": [{"bone": "arm", "rotate": [{"time": 0, "value": 90}, {"time": 1, "value": 90}]}]}
        ],
        "graph": {"parameters": [{"name": "speed", "float": 0.0}],
            "states": [{"name": "idle", "clip": "idle"}, {"name": "wave", "clip": "wave"}],
            "transitions": [{"from": "idle", "to": "wave", "when": [{"greater": ["speed", 0.5]}]}]}
    });
    std::fs::write(&file, serde_json::to_string(&skeleton).unwrap()).unwrap();
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _remove = Remove(file);
    let mut h = Harness::new();
    h.click_text(|t| t == "Author");
    h.click_text(|t| t == "Animation graph…");
    h.frame(vec![]);
    h.type_text(&name);
    let target = super::render_test_support::target(
        &gpu.device,
        (SCREEN.x as u32, SCREEN.y as u32),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    // The bones' extent in the preview, in pixels: (wide, tall).
    let pose = |h: &mut Harness, name: &str| {
        let shapes = h.frame(vec![]);
        paint_editor(&gpu.device, &gpu.queue, &target, h, shapes);
        let image = Image::read(&gpu.device, &gpu.queue, &target);
        keep_image(&image);
        let r = h.rect(preview_id());
        let bone = |p: [u8; 4]| p[2] > 200 && p[1] > 180 && p[0] < 170;
        let (mut lo, mut hi) = ((u32::MAX, u32::MAX), (0, 0));
        for y in r.top() as u32..r.bottom() as u32 {
            for x in r.left() as u32..r.right() as u32 {
                if bone(image.pixel(x, y)) {
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                }
            }
        }
        assert!(hi.0 >= lo.0, "{name}: the bones are drawn in the preview");
        (hi.0 - lo.0, hi.1 - lo.1)
    };
    let (wide, tall) = pose(&mut h, "idle");
    assert!(wide > tall * 2, "idle: the arm lies along X ({wide} x {tall})");
    // Drag the test value of speed past 0.5: the graph goes to wave.
    let field = field_right_of(&h.frame(vec![]), "test");
    h.drag(field, field + glam::vec2(60.0, 0.0), 4);
    for _ in 0..3 {
        h.frame(vec![]);
    }
    let (wide, tall) = pose(&mut h, "wave");
    assert!(tall > wide * 2, "wave: the arm points along Y ({wide} x {tall})");
}
