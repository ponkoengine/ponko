//! Item icons (`Ctx::render_icon`) through a game's own frame
//! (`Ctx::render_frame` with the offscreen renderer): the model alone,
//! framed, tone mapped, on its background, drawn once, and untouched by
//! the game's world (its models and its shadows). Each test has its
//! negative control. Skips without a GPU.
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::prelude::*;
use crate::render::{Icon, MaterialDesc, Mesh, MeshData, Model, RenderTarget};

/// The window.
const SIZE: (u32, u32) = (64, 64);
/// The icons.
const ICON: u32 = 32;

/// A game with a black background and no sky, a green glowing box at the
/// origin and the camera looking at it from the front; `red` is a red
/// glowing box mesh nothing in the world uses.
struct Scene {
    ctx: crate::ctx::Ctx,
    red: Mesh,
}

impl Scene {
    fn new() -> Option<Self> {
        let mut ctx = offscreen_ctx(SIZE)?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        ctx.gfx.clear_color = Color::BLACK;
        let green = glowing(&mut ctx, Color::rgb(0.0, 1.0, 0.0));
        let red = glowing(&mut ctx, Color::rgb(1.0, 0.0, 0.0));
        ctx.world.spawn((crate::d3::Transform::default(), Model::new(green)));
        ctx.scene3d.camera.position = vec3(0.0, 0.0, 5.0);
        ctx.scene3d.camera.look_at(Vec3::ZERO);
        // No frame drawn yet: icons asked for in a game's setup.
        Some(Self { ctx, red })
    }

    /// Draws a frame; returns the window as RGBA8.
    fn frame(&mut self) -> Image {
        let path = std::env::temp_dir().join(format!(
            "ponko-icon-{}-{:?}.png",
            std::process::id(),
            std::thread::current().id()
        ));
        self.ctx.gfx.queue_screenshot(path.clone(), 0, None);
        assert_eq!(self.ctx.render_frame(), crate::render::FrameStatus::Offscreen);
        let shot = image::open(&path).expect("the frame was captured").to_rgba8();
        let _ = std::fs::remove_file(&path);
        let window = Image {
            width: shot.width(),
            height: shot.height(),
            rgba: shot.into_raw(),
        };
        keep_image(&window);
        window
    }

    /// `target`'s texels, linear RGBA.
    fn read(&self, target: RenderTarget) -> Vec<[f32; 4]> {
        let renderer = self.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let (bytes, width, height) = renderer
            .read_texture_level(target.texture().id, 0)
            .expect("the icon's target is on the GPU");
        let texels: Vec<[f32; 4]> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
            .collect();
        let encode = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        keep_image(&Image {
            width,
            height,
            rgba: texels
                .iter()
                .flat_map(|c| [encode(c[0]), encode(c[1]), encode(c[2]), encode(c[3])])
                .collect(),
        });
        texels
    }
}

fn glowing(ctx: &mut crate::ctx::Ctx, color: Color) -> Mesh {
    let material = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::BLACK).with_emissive(color));
    ctx.gfx.create_mesh(&MeshData::cube(1.0), material)
}

/// Texels whose light is mostly `channel` (0 red, 1 green, 2 blue).
fn count(texels: &[[f32; 4]], channel: usize) -> usize {
    texels
        .iter()
        .filter(|c| c[channel] > 0.4 && (0..3).all(|i| i == channel || c[i] < 0.2))
        .count()
}

/// Pixels of an RGBA8 image that are clearly `channel`.
fn count8(image: &Image, channel: usize) -> usize {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[channel] > 150 && (0..3).all(|i| i == channel || p[i] < 60))
        .count()
}

#[test]
fn an_icon_shows_the_model_alone_on_a_see_through_background() {
    let Some(mut scene) = Scene::new() else { return };
    let entities = scene.ctx.world.len();
    let icon = scene.ctx.render_icon(scene.red, &Icon::new(ICON));
    assert_eq!(scene.ctx.world.len(), entities, "nothing spawned in the game's world");
    // The game's first frame draws no 3D picture: an icon asked for in
    // setup is drawn again in the second.
    scene.frame();
    let window = scene.frame();
    let texels = scene.read(icon);
    assert_eq!(texels.len(), (ICON * ICON) as usize);
    // The red box, framed to fill the icon: from the front-right and above,
    // a good part of it.
    let red = count(&texels, 0);
    assert!(red > texels.len() / 4, "the box fills the icon: {red} red texels");
    let center = texels[(ICON / 2 * ICON + ICON / 2) as usize];
    assert!(
        center[3] > 0.99 && center[0] > 0.5,
        "the box is opaque at the center: {center:?}"
    );
    // Alone: the green box at the same place in the game's world isn't in it.
    assert_eq!(count(&texels, 1), 0, "nothing of the game's world");
    // See-through around it.
    for corner in [0, ICON - 1, ICON * (ICON - 1), ICON * ICON - 1] {
        let c = texels[corner as usize];
        assert!(c.iter().all(|&v| v.abs() < 1e-3), "corner {corner} see-through: {c:?}");
    }
    // Negative control: the window shows the world's green box, and no red.
    assert!(count8(&window, 1) > 50, "the game's world is drawn");
    assert_eq!(count8(&window, 0), 0, "the icon's model isn't in the window");
    // With a background of its own: opaque blue around the box.
    let blue = scene
        .ctx
        .render_icon(scene.red, &Icon::new(ICON).with_background(Color::rgb(0.0, 0.0, 1.0)));
    scene.frame();
    let texels = scene.read(blue);
    let corner = texels[0];
    assert!(corner[2] > 0.5 && corner[3] > 0.99, "a blue corner: {corner:?}");
    assert!(count(&texels, 0) > texels.len() / 4, "the box over it");
}

#[test]
fn an_icon_is_drawn_in_two_frames_and_kept() {
    let Some(mut scene) = Scene::new() else { return };
    let materials = scene.ctx.gfx.assets3d.materials_live.ids().count();
    // The game's first frame, which draws no 3D picture, behind it.
    scene.frame();
    let icon = scene.ctx.render_icon(scene.red, &Icon::new(ICON));
    scene.frame();
    let first = scene.read(icon);
    assert!(count(&first, 0) > first.len() / 4, "drawn with the next frame");
    assert_eq!(scene.ctx.gfx.frame3d.views.len(), 1, "one view: the icon");
    // Drawn again in the second frame (see ICON_FRAMES), then no more: the
    // next frames draw no view and keep the picture.
    scene.frame();
    assert_eq!(scene.ctx.gfx.frame3d.views.len(), 1, "drawn in its second frame");
    let second = scene.read(icon);
    assert_eq!(second, first, "the same picture");
    scene.frame();
    scene.frame();
    assert!(scene.ctx.gfx.icons.is_empty(), "nothing left to draw");
    assert!(scene.ctx.gfx.frame3d.views.is_empty(), "no view in later frames");
    assert_eq!(scene.read(icon), second, "the picture is kept");
    assert_eq!(
        scene.ctx.gfx.assets3d.materials_live.ids().count(),
        materials,
        "its tone mapping material is freed"
    );
    // Negative control: a target camera drawing into it every frame
    // replaces the picture (here with the game's green box).
    let camera = crate::render::TargetCamera::new(scene.ctx.scene3d.camera, icon);
    scene.ctx.scene3d.passes.target_cameras.push(camera);
    scene.frame();
    assert!(count(&scene.read(icon), 1) > 10, "a target camera draws over it");
    // A target freed before its frame: the icon is dropped, nothing drawn.
    scene.ctx.scene3d.passes.target_cameras.clear();
    let gone = scene.ctx.render_icon(scene.red, &Icon::new(ICON));
    scene.ctx.free(gone);
    scene.frame();
    assert!(scene.ctx.gfx.icons.is_empty() && scene.ctx.gfx.frame3d.views.is_empty());
    assert_eq!(scene.ctx.gfx.assets3d.materials_live.ids().count(), materials);
}

#[test]
fn an_icons_light_is_tone_mapped() {
    let Some(mut scene) = Scene::new() else { return };
    let bright = glowing(&mut scene.ctx, Color::rgb(20.0, 2.0, 0.0));
    let icon = scene.ctx.render_icon(bright, &Icon::new(ICON));
    let dim = scene.ctx.render_icon(bright, &Icon::new(ICON).with_exposure(0.02));
    scene.frame();
    scene.frame();
    let texels = scene.read(icon);
    let center = texels[(ICON / 2 * ICON + ICON / 2) as usize];
    // Twenty times white comes out just under white, not 20.
    assert!(
        center[0] > 0.9 && center[0] <= 1.0 && center[1] < center[0],
        "tone mapped: {center:?}"
    );
    assert!(
        texels.iter().all(|c| c[..3].iter().all(|&v| v <= 1.0)),
        "nothing above 1"
    );
    // Exposure scales the light before the curve.
    let low = scene.read(dim)[(ICON / 2 * ICON + ICON / 2) as usize];
    assert!(
        low[1] > 0.01 && low[1] < center[1] * 0.5,
        "darker: {low:?} vs {center:?}"
    );
}

#[test]
fn an_icon_takes_no_shadow_from_the_games_world() {
    let Some(mut scene) = Scene::new() else { return };
    scene.ctx.world.clear();
    scene.ctx.scene3d.lighting.shadows = true;
    scene.ctx.scene3d.lighting.sun_direction = vec3(0.0, -1.0, 0.0);
    let white = scene.ctx.gfx.create_material(MaterialDesc::color(Color::WHITE));
    let cube = scene.ctx.gfx.create_mesh(&MeshData::cube(1.0), white);
    // The same cube in the game's world, at the origin, and a target camera
    // looking down at its top (the sun's face), as the icon does.
    scene
        .ctx
        .world
        .spawn((crate::d3::Transform::default(), Model::new(cube)));
    let seen = scene.ctx.gfx.create_render_target(ICON, ICON);
    let mut eye = scene.ctx.scene3d.camera;
    eye.position = vec3(0.0, 2.0, 0.4);
    eye.look_at(Vec3::ZERO);
    scene
        .ctx
        .scene3d
        .passes
        .target_cameras
        .push(crate::render::TargetCamera::new(eye, seen));
    let from_above = Icon::new(ICON).with_from(vec3(0.0, 1.0, 0.2));
    let top = |t: &[[f32; 4]]| t[(ICON / 2 * ICON + ICON / 2) as usize][0];
    let open = scene.ctx.render_icon(cube, &from_above);
    scene.frame();
    scene.frame();
    let (icon_open, world_open) = (top(&scene.read(open)), top(&scene.read(seen)));
    assert!(
        icon_open > 0.1 && world_open > 0.1,
        "lit by the sun: {icon_open}, {world_open}"
    );
    // A roof over the origin in the game's world.
    let roof = scene.ctx.gfx.create_mesh(&MeshData::cube(1.0), white);
    scene.ctx.world.spawn((
        crate::d3::Transform {
            scale: vec3(8.0, 0.2, 8.0),
            ..crate::d3::Transform::at(vec3(0.0, 3.0, 0.0))
        },
        Model::new(roof),
    ));
    let roofed = scene.ctx.render_icon(cube, &from_above);
    scene.frame();
    scene.frame();
    // Negative control: the world's cube is in the roof's shadow.
    let world_roofed = top(&scene.read(seen));
    assert!(
        world_roofed < world_open * 0.5,
        "the world's cube is shaded: {world_roofed} vs {world_open}"
    );
    // The icon's isn't: it has a world of its own.
    let icon_roofed = top(&scene.read(roofed));
    assert!(
        (icon_roofed - icon_open).abs() < 0.02,
        "the same light under the roof: {icon_roofed} vs {icon_open}"
    );
}
