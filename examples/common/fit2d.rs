//! Laying a 2D game with a fixed play area out for any window: a desktop
//! window, a phone held upright or turned, a notch or a home bar. Shared by
//! Coin Dash and Ember Hop (each includes it next to its `game.rs`).
//!
//! - [`fit_camera`]: the camera shows the play area whole, as large as it
//!   fits and centred, in the window's safe area; what is left over is
//!   the clear color (letterboxed). In a window the play area's size it is
//!   the camera the game always had: zoom 1, the area's corner at the
//!   window's.
//! - [`stack_lines`]: a centred block of text lines, each made smaller only
//!   if it would not fit the safe area's width, and all of them if the block
//!   would not fit its height. In a window where everything fits, the text
//!   is where and as large as it always was.
//!
//! Both are pure, so a test checks them at a phone's size without a window,
//! and both are worked out every frame from the window's current size and
//! safe area: turning the phone needs no restart.

use ponko::d2::Camera;
use ponko::prelude::*;

// ANCHOR: fit_camera
/// Where `area` shows in `room` when it is whole, as large as fits and
/// centred: what [`fit_camera`] makes of it. `None` for an empty `room` or
/// `area`.
pub fn fitted(area: Rect, room: Rect) -> Option<Rect> {
    let zoom = (room.size() / area.size()).min_element();
    if !(zoom.is_finite() && zoom > 0.0) {
        return None;
    }
    let size = area.size() * zoom;
    let min = room.center() - size / 2.0;
    Some(Rect { min, max: min + size })
}

/// Points the camera so `area` (world units) is whole, as large as fits
/// and centred in `room` (window pixels, such as `ctx.window.safe_area()`).
/// Nothing changes for an empty `room` or `area`.
pub fn fit_camera(camera: &mut Camera, area: Rect, room: Rect) {
    let Some(shown) = fitted(area, room) else {
        return;
    };
    camera.zoom = shown.size().x / area.size().x;
    camera.position = area.min - (shown.min - camera.screen_rect().min) / camera.zoom;
}

// ANCHOR_END: fit_camera

/// One line of text where it is drawn: its top left corner (window
/// pixels), its size and its width at that size.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub text: String,
    pub at: Vec2,
    pub size: f32,
    pub width: f32,
    pub color: Color,
}

impl Placed {
    /// The window pixels it covers (its height is its size).
    pub fn rect(&self) -> Rect {
        Rect::new(self.at.x, self.at.y, self.width, self.size)
    }
}

/// The size a line `width` wide at `size` is drawn at to fit `room`
/// pixels: `size`, or smaller when it is too wide.
pub fn fit_size(size: f32, width: f32, room: f32) -> f32 {
    if width > room && width > 0.0 {
        size * (room.max(0.0) / width)
    } else {
        size
    }
}

/// Lines of text, each `(text, size, width at that size, color)`, stacked
/// and centred in `room`, `spacing` times their size apart, `margin`
/// pixels clear of its sides. A line too wide is made smaller; a block too
/// tall, all of its lines alike. `top` places the block's top edge (it is
/// kept inside `room`); `None` centres it.
pub fn stack_lines(
    lines: impl IntoIterator<Item = (String, f32, f32, Color)>,
    spacing: f32,
    room: Rect,
    margin: f32,
    top: Option<f32>,
) -> Vec<Placed> {
    let inner = (room.size().x - 2.0 * margin).max(0.0);
    let mut placed: Vec<Placed> = lines
        .into_iter()
        .map(|(text, size, width, color)| {
            let fitted = fit_size(size, width, inner);
            Placed {
                text,
                at: Vec2::ZERO,
                size: fitted,
                width: width * fitted / size.max(f32::MIN_POSITIVE),
                color,
            }
        })
        .collect();
    let height: f32 = placed.iter().map(|p| p.size * spacing).sum();
    let tall = (room.size().y - 2.0 * margin).max(0.0);
    let shrink = if height > tall && height > 0.0 {
        tall / height
    } else {
        1.0
    };
    let height = height * shrink;
    let lowest = room.min.y + margin;
    let highest = (room.max.y - margin - height).max(lowest);
    let mut y = top
        .unwrap_or(room.min.y + (room.size().y - height) / 2.0)
        .clamp(lowest, highest);
    for p in &mut placed {
        p.size *= shrink;
        p.width *= shrink;
        p.at = vec2(room.center().x - p.width / 2.0, y);
        y += p.size * spacing;
    }
    placed
}
