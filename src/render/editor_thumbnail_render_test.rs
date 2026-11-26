//! The editor's model thumbnails drawn by the production renderer,
//! offscreen: a model's thumbnail shows the model (lit, its own colours)
//! on the background, differs from the software thumbnail it replaces, is
//! kept on disk under its content and read from there by the next editor
//! run without drawing again. Skips without a GPU (a skip is not a pass).
use std::path::{Path, PathBuf};

use crate::editor::cook::AssetKind;
use crate::editor::thumbs::{SIZE, Thumbnails, content_key, make};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cook/source")
}

/// How far apart two pixels' colours are (largest channel difference).
fn apart(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).take(3).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

#[test]
fn a_model_thumbnail_is_drawn_by_the_renderer_and_kept_on_disk_by_its_content() {
    let Some(ctx) = super::render_test_support::offscreen_ctx((64, 64)) else {
        return;
    };
    let cache = std::env::temp_dir().join(format!("ponko-thumb-render-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let mut thumbs = Thumbnails::default();
    thumbs.cache = Some(cache.clone());
    assert!(thumbs.get(&fixtures(), "grid.glb", AssetKind::Model).is_none());
    assert!(thumbs.wait(std::time::Duration::from_secs(20)));
    assert!(thumbs.render_models(ctx.gfx.renderer.as_ref()), "drawn");
    assert_eq!(thumbs.take_updated(), ["grid.glb"]);
    let drawn = thumbs
        .get(&fixtures(), "grid.glb", AssetKind::Model)
        .unwrap()
        .clone()
        .unwrap();
    assert_eq!((drawn.width, drawn.height), (SIZE, SIZE));
    super::render_test_support::keep_image(&super::render_test_support::Image {
        width: drawn.width as u32,
        height: drawn.height as u32,
        rgba: drawn.rgba.clone(),
    });

    // The model in the middle, the background in the corners.
    let px = |x: usize, y: usize| &drawn.rgba[(y * SIZE + x) * 4..(y * SIZE + x) * 4 + 4];
    let corner = px(0, 0).to_vec();
    assert!(
        [px(0, SIZE - 1), px(SIZE - 1, 0), px(SIZE - 1, SIZE - 1)]
            .iter()
            .all(|p| apart(p, &corner) < 8),
        "the corners are background"
    );
    let covered = (0..SIZE * SIZE)
        .filter(|i| apart(&drawn.rgba[i * 4..i * 4 + 4], &corner) > 24)
        .count();
    assert!(covered > SIZE * SIZE / 10, "the model covers part of it: {covered}");
    assert!(covered < SIZE * SIZE * 9 / 10, "framed with a margin: {covered}");
    // Negative control: not the software thumbnail it replaces.
    let soft = make(&fixtures(), "grid.glb", AssetKind::Model).unwrap();
    assert_ne!(drawn.rgba, soft.rgba);
    assert_eq!(drawn.note, soft.note, "the triangle count stays");

    // Kept under its content; the next run reads it and draws nothing.
    let bytes = std::fs::read(fixtures().join("grid.glb")).unwrap();
    let png = cache.join(format!("{}.png", content_key(&fixtures().join("grid.glb"), &bytes)));
    assert!(png.is_file(), "{}", png.display());
    let mut next = Thumbnails::default();
    next.cache = Some(cache.clone());
    assert!(next.get(&fixtures(), "grid.glb", AssetKind::Model).is_none());
    assert!(next.wait(std::time::Duration::from_secs(20)));
    assert!(!next.render_models(ctx.gfx.renderer.as_ref()), "nothing to draw");
    let again = next
        .get(&fixtures(), "grid.glb", AssetKind::Model)
        .unwrap()
        .clone()
        .unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    assert_eq!(again, drawn);
}

/// The studio draws a model in one frame: the thumbnail from a fresh
/// studio's first frame is the one a later, settled frame of the same
/// studio makes (nothing is built once per view and missing from the first
/// frame), and the studio draws one frame for each model it is asked for.
#[test]
fn a_model_thumbnail_is_right_after_a_single_frame() {
    use crate::editor::thumb_studio::Studio;
    let Some(ctx) = super::render_test_support::offscreen_ctx((64, 64)) else {
        return;
    };
    let gpu = ctx.gfx.renderer.as_ref().unwrap();
    let dir = std::env::temp_dir().join(format!("ponko-thumb-frames-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut studio = Studio::new(gpu);
    let draw = |studio: &mut Studio, rel: &str, name: &str| {
        let before = studio.frames_drawn();
        let thumb = studio
            .render(&fixtures(), rel, &dir.join(format!("{name}.png")))
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(studio.frames_drawn() - before, 1, "{rel}: one frame");
        thumb
    };
    let covered = |t: &crate::editor::thumbs::Thumb| {
        let corner = t.rgba[..4].to_vec();
        (0..SIZE * SIZE)
            .filter(|i| apart(&t.rgba[i * 4..i * 4 + 4], &corner) > 24)
            .count()
    };
    for rel in ["grid.glb", "level.glb", "panel.glb", "models/crate.glb"] {
        let first = draw(&mut studio, rel, "first");
        let covers = covered(&first);
        assert!(
            covers > SIZE * SIZE / 20 && covers < SIZE * SIZE * 9 / 10,
            "{rel}: the model is on the background after one frame: {covers}"
        );
        // Settled: the same studio draws it again, and again the same.
        let again = draw(&mut studio, rel, "again");
        assert_eq!(first.rgba, again.rgba, "{rel}: the first frame is the settled picture");
        // A fresh studio's first frame too (new targets, nothing warm).
        let fresh = draw(&mut Studio::new(gpu), rel, "fresh");
        assert_eq!(first.rgba, fresh.rgba, "{rel}: a fresh studio draws the same");
    }
    // Negative control: different models make different thumbnails.
    let grid = draw(&mut studio, "grid.glb", "grid");
    let level = draw(&mut studio, "level.glb", "level");
    assert_ne!(grid.rgba, level.rgba);
    let _ = std::fs::remove_dir_all(&dir);
}
