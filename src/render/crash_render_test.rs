//! Crash reports name the GPU the renderer opened and the graphics
//! settings of the last frame drawn (`ponko::crash`).

use crate::render::{FrameStatus, GraphicsQuality};

const SIZE: u32 = 64;

#[test]
fn a_report_names_the_gpu_and_the_graphics_settings_of_the_last_frame() {
    let Some(mut ctx) = super::render_test_support::offscreen_ctx((SIZE, SIZE)) else {
        return;
    };
    ctx.crash = Some(crate::crash::Running::for_test("gpu test", true));

    // Negative control: before a frame, the report knows neither.
    let before = ctx.crash.as_ref().unwrap().preview("before");
    assert!(before.gpu().is_none());
    assert_eq!(before.settings(), [("frames".to_string(), "none drawn yet".to_string())]);

    ctx.scene3d.set_quality(GraphicsQuality::High);
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let report = ctx.crash.as_ref().unwrap().preview("after");
    let adapter = ctx.gfx.renderer.as_ref().unwrap().capabilities().adapter.clone();
    assert!(!adapter.name.is_empty() && !adapter.backend.is_empty(), "{adapter:?}");
    assert_eq!(report.gpu(), Some(&adapter));
    let setting = |name: &str| {
        report
            .settings()
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no {name:?} in {:?}", report.settings()))
    };
    assert_eq!(setting("window"), format!("{SIZE}x{SIZE} at 1x scale, windowed"));
    assert_eq!(setting("vsync"), "on");
    assert_eq!(setting("quality preset"), "High");
    assert_eq!(setting("antialiasing"), "Smaa");
    assert_eq!(setting("ambient occlusion"), "on");
    let text = report.text();
    assert!(text.contains(&format!("gpu: {} ({}, {})", adapter.name, adapter.backend, adapter.device_type)), "{text}");
    assert!(text.contains("gpu driver: "), "{text}");

    // A changed setting shows in the next frame's report.
    ctx.scene3d.lighting.shadow_resolution = 1024;
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let report = ctx.crash.as_ref().unwrap().preview("changed");
    assert!(report.settings().contains(&("quality preset".to_string(), "custom".to_string())));
    assert!(report.gpu().is_some(), "the GPU stays known");
}
