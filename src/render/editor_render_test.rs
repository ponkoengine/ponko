//! The editor app's layout, drawn offscreen with egui's wgpu renderer (the
//! one the window draws the inspector with): menu bar, hierarchy, inspector
//! column, status bar, and the world between them with the gizmo on the
//! selected entity. In 2D the world is a clear colour; in 3D it is a frame
//! of the 3D renderer (the regression stage), with the editor drawn over it.

use super::regression_stage::{Setup, Stage, mean_color};
use super::render_test_support::*;
use crate::d2::Transform as T2;
use crate::inspector::Panels;
use crate::inspector::app::App;
use crate::prelude::*;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Tall enough for the view between the panels to hold the gizmo.
const SIZE: (u32, u32) = (960, 720);
/// The world's clear colour, a blue-green no egui theme uses.
const WORLD: [u8; 4] = [20, 90, 80, 255];

#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Copy, Component)]
#[serde(default)]
struct Health(f32);

fn near(a: [u8; 4], b: [u8; 4], by: u8) -> bool {
    a.iter().zip(b).take(3).all(|(x, y)| x.abs_diff(y) <= by)
}

/// The gizmo's x arrow (red) and y arrow (green), as `draw_gizmo2` paints them.
fn red(p: [u8; 4]) -> bool {
    p[0] > 180 && p[1] < 120 && p[2] < 120
}
fn green(p: [u8; 4]) -> bool {
    p[1] > 160 && p[0] < 140 && p[2] < 140
}

/// What egui drew for one editor frame, ready for its wgpu renderer.
struct Drawn {
    primitives: Vec<egui::ClippedPrimitive>,
    textures: egui::TexturesDelta,
    pixels_per_point: f32,
}

/// The asset panel's fixture: the cooker's test sources (a texture, a
/// model and a sound, with thumbnails) and a scene and a prefab that have
/// none, instead of whatever the game's assets folder holds.
fn fixture_assets(panels: &mut Panels, selected: Option<&str>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cook/source");
    panels.show_assets_from(root, panels_list(), selected);
}

/// Two frames of the panels over `ctx` (the first starts edit mode and lays
/// out, the second draws the selection's gizmo where the first put the
/// panels): as the editor app (`app`), or as the game with the inspector
/// closed, which draws nothing. `select` is selected and renamed before the
/// second: the hierarchy shows the name, the status bar that it's unsaved.
fn editor_frame(ctx: &mut Ctx, app: bool, select: Option<Entity>) -> Drawn {
    editor_frames(ctx, SIZE, app, |frame, panels, ctx| match frame {
        0 => fixture_assets(panels, None),
        _ => {
            if let Some(e) = select {
                panels.select(Some(e), false);
                ctx.edit(|ed, ctx| ed.rename(ctx, e, "Crab")).unwrap();
            }
        }
    })
}

/// Two frames of the panels over `ctx` at `size`, `before` called before
/// each (with 0, then 1) to set them up. The asset panel's thumbnails asked
/// for in the first are made before the second.
fn editor_frames(
    ctx: &mut Ctx,
    size: (u32, u32),
    app: bool,
    mut before: impl FnMut(u32, &mut Panels, &mut Ctx),
) -> Drawn {
    let size = glam::vec2(size.0 as f32, size.1 as f32);
    let mut panels = Panels::default();
    if app {
        panels.app = Some(App::new(None));
    }
    let egui = egui::Context::default();
    // Windows drawn whole, not fading in.
    egui.all_styles_mut(|s| s.animation_time = 0.0);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.x, size.y))),
        ..Default::default()
    };
    let frame = |panels: &mut Panels, ctx: &mut Ctx| {
        egui.run_ui(input(), |ui| {
            if app {
                panels.show(ui, ctx);
            }
        })
    };
    // The first frame's textures (the fonts) are the second's too.
    if app {
        before(0, &mut panels, ctx);
    }
    let mut textures = frame(&mut panels, ctx).textures_delta;
    if app {
        assert!(panels.wait_for_asset_thumbnails(), "the thumbnails are made");
        before(1, &mut panels, ctx);
    }
    let mut out = frame(&mut panels, ctx);
    textures.append(std::mem::take(&mut out.textures_delta));
    Drawn {
        primitives: egui.tessellate(out.shapes, out.pixels_per_point),
        textures,
        pixels_per_point: out.pixels_per_point,
    }
}

/// Draws `drawn` into `target` with egui's wgpu renderer, over `clear` or
/// (`None`) over what the target holds.
fn paint(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture, mut drawn: Drawn, clear: Option<[u8; 4]>) {
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
    for (id, deltas) in &drawn.textures.set {
        for delta in deltas {
            renderer.update_texture(device, queue, *id, delta);
        }
    }
    drawn.textures.clear();
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [target.width(), target.height()],
        pixels_per_point: drawn.pixels_per_point,
    };
    let view = target.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut buffers = renderer.update_buffers(device, queue, &mut encoder, &drawn.primitives, &screen);
    {
        let load = clear.map_or(wgpu::LoadOp::Load, |c| {
            let [r, g, b, _] = c.map(|c| f64::from(c) / 255.0);
            wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 })
        });
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &drawn.primitives, &screen);
    }
    buffers.push(encoder.finish());
    queue.submit(buffers);
}

/// Where `draw` spawns its entity: under this window point of the camera
/// before the editor's panels take their place.
const ENTITY_AT: (u32, u32) = (SIZE.0 / 2, 170);

/// One editor frame over a 2D world with an entity at `ENTITY_AT` (`select`:
/// selected and renamed), as the editor app (`app`) or as the game with the
/// inspector closed, drawn into an offscreen target over the world colour.
/// Also returns where the entity is drawn: the editor's camera shows the
/// free view between the panels.
fn draw(gpu: &TestGpu, app: bool, select: bool) -> (Image, (u32, u32)) {
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(SIZE.0 as f32, SIZE.1 as f32));
    ctx.world.reflect::<Health>();
    let at = ctx
        .camera
        .screen_to_world(glam::vec2(ENTITY_AT.0 as f32, ENTITY_AT.1 as f32));
    let e = ctx.world.spawn((T2::at(at), Health(3.0)));
    ctx.world.spawn((T2::at(at + glam::vec2(-200.0, 80.0)),));
    let drawn = editor_frame(&mut ctx, app, select.then_some(e));
    let target = target(&gpu.device, SIZE, FORMAT);
    paint(&gpu.device, &gpu.queue, &target, drawn, Some(WORLD));
    let image = Image::read(&gpu.device, &gpu.queue, &target);
    keep_image(&image);
    let on_screen = ctx.camera.world_to_screen(at).round();
    (image, (on_screen.x as u32, on_screen.y as u32))
}

#[test]
fn the_editor_app_frames_the_world_with_its_panels_and_draws_the_gizmo_on_the_selection() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = SIZE;
    let world = |p: [u8; 4]| near(p, WORLD, 6);
    let panel = |p: [u8; 4]| !near(p, WORLD, 30);

    let (image, (cx, cy)) = draw(&gpu, true, true);
    // The menu bar along the top and the status bar along the bottom.
    assert_region(&image, 0..w, 2..14, 0.95, "menu bar", panel);
    assert_region(&image, 0..w, h - 12..h - 2, 0.95, "status bar", panel);
    // The hierarchy on the left, the inspector on the right.
    assert_region(&image, 4..200, 60..h - 60, 0.95, "hierarchy", panel);
    assert_region(&image, w - 200..w - 4, 60..h - 60, 0.95, "inspector", panel);
    // The asset panel along the bottom, above the status bar.
    assert_region(&image, 0..w, h - 200..h - 40, 0.9, "asset panel", panel);
    // The world between them, around the gizmo.
    assert_region(&image, cx - 120..cx - 90, cy + 50..cy + 100, 0.9, "world", world);
    // The gizmo: the x arrow to the right of the entity, the y arrow below
    // it (screen y grows down).
    assert_region(&image, cx + 20..cx + 60, cy - 3..cy + 4, 0.2, "x arrow", red);
    assert!(
        image.fraction(cx - 4..cx + 5, cy + 20..cy + 60, green) > 0.2
            || image.fraction(cx - 4..cx + 5, cy - 60..cy - 20, green) > 0.2,
        "a y arrow"
    );

    // Negative controls: nothing selected draws no gizmo; the game without
    // edit mode (inspector closed) is all world.
    let (unselected, (ux, uy)) = draw(&gpu, true, false);
    assert!(
        unselected.fraction(ux + 20..ux + 60, uy - 3..uy + 4, red) < 0.01,
        "no gizmo"
    );
    let (game, _) = draw(&gpu, false, false);
    assert_region(&game, 0..w, 0..h, 0.999, "the game alone", world);
}

/// The 3D gizmo's arrows, as `highlight_3d` paints them.
fn blue(p: [u8; 4]) -> bool {
    p[2] > 180 && p[0] < 140 && p[1] < 170
}

/// The editor over a frame of the 3D renderer: a floor and a cream block
/// at the origin (`app`: in the editor with the block selected; else the
/// game alone).
fn draw3d(app: bool) -> Option<(Image, Stage, Entity)> {
    let mut stage = Stage::with(Setup {
        size: SIZE,
        ..Setup::default()
    })?;
    stage.floor();
    let cream = stage.cube(Color::rgb(0.95, 0.85, 0.6));
    let block = stage.block(cream, glam::vec3(-0.5, 0.0, -0.5), glam::vec3(0.5, 1.0, 0.5));
    let drawn = editor_frame(&mut stage.ctx, app, app.then_some(block));
    stage.render();
    paint(&stage.device, &stage.queue, &stage.output, drawn, None);
    let image = Image::read(&stage.device, &stage.queue, &stage.output);
    keep_image(&image);
    Some((image, stage, block))
}

#[test]
fn the_editor_app_over_a_3d_scene_shows_the_renderers_frame_between_its_panels_and_the_gizmo_on_the_selection() {
    let Some((image, stage, block)) = draw3d(true) else {
        return;
    };
    let Some((game, game_stage, _)) = draw3d(false) else {
        return;
    };
    let (w, h) = SIZE;
    // The panels cover the sides and the bottom of the 3D frame: the same
    // flat fill where the hierarchy and the asset panel have no rows.
    let fill = image.pixel(150, 250);
    let panel = |p: [u8; 4]| near(p, fill, 4);
    assert_region(&image, 10..280, 180..285, 0.95, "hierarchy", panel);
    assert_region(&image, w - 280..w - 10, h - 170..h - 40, 0.95, "asset panel", panel);
    for (x, y) in [(10..280, 180..285), (w - 280..w - 10, h - 170..h - 40)] {
        assert!(game.fraction(x, y, panel) < 0.2, "the game alone shows its world there");
    }

    // Between them the game's own frame, unchanged: the block's front,
    // inside the selection ring and clear of the gizmo.
    let front = glam::vec3(-0.3, 0.8, 0.5);
    let (x, y) = stage.pixel(front);
    let (in_editor, in_game) = (mean_color(&image, x, y, 1), mean_color(&game, x, y, 1));
    assert!(
        in_editor.iter().zip(in_game).all(|(a, b)| (a - b).abs() < 0.02),
        "the 3D frame shows through: {in_editor:?} vs {in_game:?}"
    );
    assert!(
        in_game[0] > 0.3 && in_game[0] > in_game[2],
        "a lit cream block: {in_game:?}"
    );

    // The move gizmo at the block's position: x red, y green, z blue.
    let camera = &stage.ctx.scene3d.camera;
    let pivot = stage.ctx.world.get::<crate::d3::Transform>(block).unwrap().position;
    let l = crate::edit3d::layout(
        camera,
        crate::edit3d::Mode::Translate,
        pivot,
        crate::edit3d::axes(crate::edit3d::Space::World, glam::Quat::IDENTITY),
    )
    .unwrap();
    for (i, (what, color)) in [("x", red as fn([u8; 4]) -> bool), ("y", green), ("z", blue)]
        .into_iter()
        .enumerate()
    {
        let at = l.center.lerp(l.tips[i].unwrap(), 0.6);
        let (x, y) = (at.x as u32, at.y as u32);
        let around = (x - 4..x + 5, y - 4..y + 5);
        assert!(
            image.fraction(around.0.clone(), around.1.clone(), color) > 0.1,
            "the {what} arrow at {at}"
        );
        assert!(
            game.fraction(around.0, around.1, color) < 0.01,
            "no {what} arrow in the game alone"
        );
    }
    stage.finish();
    game_stage.finish();
}

/// One editor-app frame at `size` over a 2D world with an entity in the
/// middle (passed to `before`, which sets the panels up before each of the
/// two frames), drawn over the world colour.
fn draw_app(gpu: &TestGpu, size: (u32, u32), mut before: impl FnMut(u32, &mut Panels, &mut Ctx, Entity)) -> Image {
    let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(size.0 as f32, size.1 as f32));
    ctx.world.reflect::<Health>();
    let e = ctx.world.spawn((T2::at(ctx.camera.center()), Health(3.0)));
    let drawn = editor_frames(&mut ctx, size, true, |n, panels, ctx| before(n, panels, ctx, e));
    let target = target(&gpu.device, size, FORMAT);
    paint(&gpu.device, &gpu.queue, &target, drawn, Some(WORLD));
    let image = Image::read(&gpu.device, &gpu.queue, &target);
    keep_image(&image);
    image
}

/// How many different colours `image` has in a region.
fn distinct(image: &Image, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> usize {
    let mut seen = std::collections::HashSet::new();
    for y in ys {
        for x in xs.clone() {
            seen.insert(image.pixel(x, y));
        }
    }
    seen.len()
}

#[test]
fn the_asset_panel_draws_a_thumbnail_per_asset_and_the_selected_assets_import_settings() {
    let Some(gpu) = gpu() else { return };
    let with = |selected: Option<&'static str>| {
        draw_app(&gpu, SIZE, move |n, panels, _, _| {
            if n == 0 {
                fixture_assets(panels, selected);
            }
        })
    };
    let texture = with(Some("tile.png"));
    let sound = with(Some("beep.wav"));
    // The control: the same list in a folder that doesn't have the files,
    // so no thumbnails, and nothing selected.
    let missing = draw_app(&gpu, SIZE, |n, panels, _, _| {
        if n == 0 {
            fixture_assets(panels, None);
            panels.show_assets_from(std::path::PathBuf::from("no-such-folder"), panels_list(), None);
        }
    });

    // Tiles in path order: beep.wav, grid.glb, levels/start.json,
    // props/crate.json, tile.png. The texture's thumbnail is its image (a
    // gradient: many colours), the sound's a light green waveform.
    let (tile_x, wave_x, row) = (540..588, 200..256, 512..560);
    assert!(
        distinct(&texture, tile_x.clone(), row.clone()) > 500,
        "the texture's thumbnail"
    );
    assert!(
        distinct(&missing, tile_x, row.clone()) < 100,
        "no thumbnail without the file"
    );
    let wave = |p: [u8; 4]| p[1] > 200 && p[0] > 110 && p[0] < 200 && p[2] > 100 && p[2] < 200;
    assert!(
        texture.fraction(wave_x.clone(), row.clone(), wave) > 0.2,
        "the sound's waveform"
    );
    assert!(missing.fraction(wave_x, row, wave) < 0.01);

    // The details column on the right: name, kind and size, then the
    // import settings (a sound's has one setting, a texture's three), and
    // a few words without a selection.
    let fill = texture.pixel(700, 680);
    let ink = |image: &Image| image.fraction(670..950, 500..690, |p| !near(p, fill, 12));
    assert!(ink(&texture) > 2.0 * ink(&missing), "the texture's details");
    assert!(ink(&texture) > ink(&sound), "a texture has more settings than a sound");
}

/// The list `fixture_assets` shows.
fn panels_list() -> crate::editor::AssetList {
    crate::editor::AssetList {
        textures: vec!["tile.png".into()],
        meshes: vec!["grid.glb".into()],
        sounds: vec!["beep.wav".into()],
        scenes: vec!["levels/start.json".into()],
        prefabs: vec!["props/crate.json".into()],
        ..Default::default()
    }
}
#[test]
fn a_prefab_instance_shows_its_overrides_in_the_inspector() {
    let Some(gpu) = gpu() else { return };
    let name = format!("ponko-test-render-prefab-{}.json", std::process::id());
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(&name);
    let prefab = serde_json::json!({
        "format": "ponko-prefab",
        "version": 2,
        "entities": [
            { "entity": 1, "components": {
                "Transform2D": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                "Health": 10.0 } },
            { "entity": 2, "components": {
                "Transform2D": { "position": [40.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                "Attached": { "to": 1, "offset": [40.0, 0.0], "rotation": 0.0 },
                "Health": 1.0 } }
        ]
    });
    std::fs::write(&file, serde_json::to_string_pretty(&prefab).unwrap()).unwrap();
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _remove = Remove(file);
    let instance = |health: Option<f32>| {
        draw_app(&gpu, (1280, 720), |n, panels, ctx, _| {
            if n == 0 {
                fixture_assets(panels, None);
                let at = ctx.camera.center() + glam::vec2(-100.0, 0.0);
                let root = ctx.edit(|ed, ctx| ed.spawn_prefab(ctx, &name, at)).unwrap();
                if let Some(h) = health {
                    ctx.edit(|ed, ctx| ed.set(ctx, root, "Health", serde_json::json!(h)))
                        .unwrap();
                }
                panels.select(Some(root), false);
            }
        })
    };
    let changed = instance(Some(25.0));
    // The control: the same instance, as the prefab has it.
    let clean = instance(None);
    // The inspector column starts with the prefab: its path, then the
    // override (a Revert button, the component, "changed") and Revert all
    // and Apply to prefab, where the clean one says it's the same and the
    // entity's own rows begin.
    let differ = |a: [u8; 4], b: [u8; 4]| !near(a, b, 30);
    let mut changed_rows = 0;
    for y in 120..200 {
        if (810..1270).any(|x| differ(changed.pixel(x, y), clean.pixel(x, y))) {
            changed_rows += 1;
        }
    }
    assert!(changed_rows > 40, "the overrides list: {changed_rows} rows differ");
    // Above it, the prefab's header and path are the same in both.
    for y in 74..115 {
        assert!(
            (810..1270).all(|x| !differ(changed.pixel(x, y), clean.pixel(x, y))),
            "row {y} of the prefab header"
        );
    }
}

#[test]
fn the_shortcuts_window_and_the_play_bar_draw_over_the_editor() {
    let Some(gpu) = gpu() else { return };
    let image = draw_app(&gpu, SIZE, |n, panels, ctx, _| {
        if n == 0 {
            fixture_assets(panels, None);
            panels.toggle_shortcuts();
            ctx.edit(|ed, ctx| ed.play(ctx));
        } else {
            // Running (the first frame paused it, starting edit mode).
            ctx.time.resume();
        }
    });
    // The control: editing, no window.
    let editing = draw_app(&gpu, SIZE, |n, panels, _, _| {
        if n == 0 {
            fixture_assets(panels, None);
        }
    });
    let (w, h) = SIZE;
    let world = |p: [u8; 4]| near(p, WORLD, 6);
    // The window in the middle, over the view and the panels.
    let middle = (w / 2 - 60..w / 2 + 60, h / 2 - 60..h / 2 + 60);
    assert!(
        image.fraction(middle.0.clone(), middle.1.clone(), world) < 0.05,
        "the window"
    );
    assert!(
        editing.fraction(middle.0, middle.1, world) > 0.95,
        "the view without it"
    );
    // The play bar says "playing" in blue; editing it doesn't.
    let blue = |p: [u8; 4]| near(p, [120, 200, 255, 255], 40);
    let bar = (600..720, 2..22);
    assert!(image.fraction(bar.0.clone(), bar.1.clone(), blue) > 0.02, "playing");
    assert!(editing.fraction(bar.0, bar.1, blue) < 0.001, "editing");
}
