//! Frames with little or nothing to draw, through a game's own frame
//! (`Ctx::render_frame` with the offscreen renderer): a target camera
//! still clears its render target when the frame has nothing at all to
//! draw, instead of leaving its last picture there; it draws what it sees
//! while the main camera sees nothing; and first-person held items are
//! drawn in a frame that has nothing else. Each case has its negative
//! control (the same camera with something to draw). Skips without a GPU.
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::prelude::*;

/// The window and the render target.
const SIZE: (u32, u32) = (64, 64);

/// A game with a black background and no sky, a red box at the origin and
/// a target camera looking at it from the front.
struct Scene {
    ctx: crate::ctx::Ctx,
    target: crate::render::RenderTarget,
    red: crate::render::Mesh,
    boxed: Option<Entity>,
}

impl Scene {
    fn new() -> Option<Self> {
        let mut ctx = offscreen_ctx(SIZE)?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        ctx.gfx.clear_color = Color::BLACK;
        let material = ctx
            .gfx
            .create_material(crate::render::MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(1.0, 0.0, 0.0)));
        let red = ctx.gfx.create_mesh(&crate::render::MeshData::cube(1.0), material);
        let target = ctx.gfx.create_render_target(SIZE.0, SIZE.1);
        let mut camera = ctx.scene3d.camera;
        camera.position = vec3(0.0, 0.0, 4.0);
        camera.look_at(Vec3::ZERO);
        ctx.scene3d
            .passes
            .target_cameras
            .push(crate::render::TargetCamera::new(camera, target));
        // The main camera looks at the box too.
        ctx.scene3d.camera = camera;
        let mut scene = Self {
            ctx,
            target,
            red,
            boxed: None,
        };
        scene.put_box();
        // The meshes reach the GPU with the first frame drawn.
        scene.ctx.render_frame();
        Some(scene)
    }

    fn put_box(&mut self) {
        self.boxed = Some(
            self.ctx
                .world
                .spawn((crate::d3::Transform::default(), crate::render::Model::new(self.red))),
        );
    }

    /// Draws a frame; returns the window and the render target, as RGBA8
    /// (the target's linear HDR light clamped and encoded as sRGB).
    fn frame(&mut self) -> (Image, Image) {
        let path = std::env::temp_dir().join(format!(
            "ponko-target-frame-{}-{:?}.png",
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
        let renderer = self.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let bytes = renderer
            .read_texture(self.target.texture().id)
            .expect("the render target");
        let encode = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        let rgba = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .flat_map(|p| {
                let c = |i: usize| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32();
                [encode(c(0)), encode(c(1)), encode(c(2)), 255]
            })
            .collect();
        let target = Image {
            width: SIZE.0,
            height: SIZE.1,
            rgba,
        };
        keep_image(&target);
        (window, target)
    }
}

/// How many pixels of `image` are clearly red.
fn reds(image: &Image) -> usize {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 150 && p[1] < 60 && p[2] < 60)
        .count()
}

#[test]
fn a_3d_frame_with_nothing_to_draw_still_clears_its_render_target() {
    let Some(mut scene) = Scene::new() else { return };
    // Negative control: the box is drawn into the target (and the window).
    let (window, target) = scene.frame();
    let (on_screen, in_target) = (reds(&window), reds(&target));
    assert!(on_screen > 200, "the main camera shows the box: {on_screen} red pixels");
    assert!(
        in_target > 200,
        "the target camera draws the box: {in_target} red pixels"
    );
    // Nothing left to draw anywhere: the target is cleared, not kept.
    scene.ctx.world.despawn(scene.boxed.take().unwrap());
    let (window, target) = scene.frame();
    assert_eq!(reds(&window), 0, "the window is cleared");
    assert_eq!(
        reds(&target),
        0,
        "a frame with nothing to draw clears the render target instead of keeping the last picture"
    );
    // Back again: drawn again.
    scene.put_box();
    let (_, target) = scene.frame();
    assert!(reds(&target) > 200, "the box is back in the target");
}

#[test]
fn a_target_camera_draws_what_it_sees_while_the_main_camera_sees_nothing() {
    let Some(mut scene) = Scene::new() else { return };
    // The main camera turned away from the box: it sees nothing.
    scene.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, 8.0));
    let (window, target) = scene.frame();
    assert_eq!(reds(&window), 0, "the main camera sees nothing");
    assert!(
        reds(&target) > 200,
        "the target camera still draws the box it sees: {} red pixels",
        reds(&target)
    );
    // Negative control: the target camera turned away too, the target is
    // cleared.
    scene.ctx.scene3d.passes.target_cameras[0]
        .camera
        .look_at(vec3(0.0, 0.0, 8.0));
    let (_, target) = scene.frame();
    assert_eq!(reds(&target), 0, "nothing in sight, nothing in the target");
}

#[test]
fn held_items_are_drawn_in_a_frame_with_nothing_else() {
    let Some(mut scene) = Scene::new() else { return };
    scene.ctx.scene3d.passes.target_cameras.clear();
    let held = scene.boxed.unwrap();
    let mut item = crate::d3::HeldItem::at(vec3(0.0, 0.0, -0.5));
    item.offset.scale = Vec3::splat(0.2);
    scene.ctx.world.insert(held, item);
    // Negative control: without a view model the held item is a model in
    // the world, drawn by the main camera.
    let (window, _) = scene.frame();
    assert!(reds(&window) > 200, "the box is drawn: {} red pixels", reds(&window));
    // With one, it is the only thing to draw: still drawn.
    scene.ctx.scene3d.view_model = Some(crate::d3::ViewModel::default());
    let (window, _) = scene.frame();
    assert!(
        reds(&window) > 200,
        "a held item alone is drawn by the view model: {} red pixels",
        reds(&window)
    );
}
