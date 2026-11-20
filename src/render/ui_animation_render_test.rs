//! UI animation drawn offscreen: a card with a square inside, at rest, then
//! moved, scaled and faded by its animations, and a screen half way through
//! going off. The engine's UI drawing makes the sprites; the window's sprite
//! shader, pipeline and pass draw them into a texture read back here (the
//! rig of `ui_render_test`).
//!
//! The checks are semantic, with no reference image: the card's pixels and
//! the square's sit where `Ui::drawn` says (the square moves and scales with
//! its parent), and a faded node is drawn darker over the black window than
//! the same node at rest, which is the negative control. Skips without a GPU
//! (a skip is not a pass).
use super::render_test_support::Image;
use super::ui_render_test::Painter;
use crate::camera::Rect;
use crate::color::Color;
use crate::ui::{UiInput, UiLayout, UiScreenTransition, UiTween};
use glam::{Vec2, vec2};

const SIZE: Vec2 = Vec2::new(200.0, 160.0);
const CARD: Color = Color::rgb(1.0, 0.5, 0.0);
const SQUARE: Color = Color::rgb(0.0, 0.3, 1.0);

/// The bounding box of the pixels `test` picks, or `None`.
fn bounds(image: &Image, test: impl Fn([u8; 4]) -> bool) -> Option<Rect> {
    let (mut min, mut max) = (vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN));
    for y in 0..image.height {
        for x in 0..image.width {
            if test(image.pixel(x, y)) {
                min = min.min(vec2(x as f32, y as f32));
                max = max.max(vec2(x as f32 + 1.0, y as f32 + 1.0));
            }
        }
    }
    (min.x <= max.x).then_some(Rect { min, max })
}

/// The card's pixels: red well above the black window.
fn card(p: [u8; 4]) -> bool {
    p[0] > 60
}

/// The square's pixels: clearly bluer than red. (Faded, the square lets
/// some of its faded parent through: each piece is faded on its own.)
fn square(p: [u8; 4]) -> bool {
    p[2] > 60 && p[2] > p[0].saturating_add(40)
}

/// `found` covers `expected` to within a pixel and a half on every side
/// (edges on fractions of a pixel are blended).
fn assert_near(what: &str, found: Option<Rect>, expected: Rect) {
    let found = found.unwrap_or_else(|| panic!("{what}: nothing drawn, expected {expected:?}"));
    let off = (found.min - expected.min).abs().max((found.max - expected.max).abs());
    assert!(
        off.max_element() <= 1.5,
        "{what}: drawn at {found:?}, expected {expected:?}"
    );
}

#[test]
fn animated_nodes_are_drawn_moved_scaled_and_faded_where_the_ui_says() {
    let Some(painter) = Painter::new() else { return };
    let mut screen = painter.screen(SIZE);
    let ctx = &mut screen.ctx;
    let at = |x: f32, y: f32, w: f32, h: f32| UiLayout::anchored(x / SIZE.x, y / SIZE.y, w / SIZE.x, h / SIZE.y);
    let root = (ctx.ui.root().layout(UiLayout::fill())).background(Color::BLACK).id();
    let card_id = (ctx.ui.panel(root))
        .layout(at(30.0, 30.0, 80.0, 40.0).padding(10.0))
        .background(CARD)
        .id();
    let square_id = (ctx.ui.panel(card_id))
        .layout(UiLayout::fixed(20.0, 20.0))
        .background(SQUARE)
        .id();
    ctx.ui.push_modeless(root);
    // The pointer is off the window: nothing is hovered.
    let still = UiInput {
        pointer: vec2(-10.0, -10.0),
        ..Default::default()
    };

    // At rest: drawn on its layout rectangle, at full strength.
    ctx.ui.frame(SIZE, &still);
    ctx.draw_retained_ui();
    let rest = painter.shot(&mut screen, SIZE);
    let ctx = &mut screen.ctx;
    let card_rect = ctx.ui.rect(card_id).unwrap();
    let square_rect = ctx.ui.rect(square_id).unwrap();
    assert_eq!(ctx.ui.drawn(card_id).unwrap().rect, card_rect);
    assert_near("card at rest", bounds(&rest, card), card_rect);
    assert_near("square at rest", bounds(&rest, square), square_rect);
    let full = rest.pixel(card_rect.min.x as u32 + 4, card_rect.min.y as u32 + 4);

    // Moved, scaled about its centre and half faded, at once.
    for tween in [
        UiTween::offset(vec2(40.0, 30.0)),
        UiTween::scale(1.5),
        UiTween::opacity(0.5),
    ] {
        ctx.ui.animate(card_id, tween.seconds(0.0));
    }
    ctx.ui.frame(SIZE, &still);
    ctx.draw_retained_ui();
    let moved = painter.shot(&mut screen, SIZE);
    let ctx = &mut screen.ctx;
    let drawn = ctx.ui.drawn(card_id).unwrap();
    assert_eq!((drawn.opacity, drawn.scale), (0.5, 1.5));
    // The layout keeps its place; the picture moves.
    assert_eq!(ctx.ui.rect(card_id).unwrap(), card_rect);
    let centre = card_rect.center() + vec2(40.0, 30.0);
    assert_eq!(drawn.rect.center(), centre);
    assert_eq!(drawn.rect.size(), card_rect.size() * 1.5);
    assert_near("moved card", bounds(&moved, card), drawn.rect);
    // The square goes with its parent.
    let square_drawn = ctx.ui.drawn(square_id).unwrap().rect;
    assert_eq!(square_drawn.size(), square_rect.size() * 1.5);
    assert_near("moved square", bounds(&moved, square), square_drawn);
    // Faded: darker over black than at rest, but clearly there.
    let half = moved.pixel(drawn.rect.min.x as u32 + 4, drawn.rect.min.y as u32 + 4);
    assert!(
        half[0] > 90 && half[0] + 30 < full[0],
        "half faded: {half:?}, at rest {full:?}"
    );
    // Nothing is left where the card was and the moved one isn't.
    let old = rest.pixel(card_rect.min.x as u32 + 2, card_rect.min.y as u32 + 2);
    assert!(card(old));
    assert!(!card(
        moved.pixel(card_rect.min.x as u32 + 2, card_rect.min.y as u32 + 2)
    ));

    // Back at rest, then the screen goes off over a second: half way, it
    // is still drawn, faded, though no longer on the stack.
    ctx.ui.stop_animations(card_id);
    for tween in [UiTween::offset(Vec2::ZERO), UiTween::scale(1.0), UiTween::opacity(1.0)] {
        ctx.ui.animate(card_id, tween.seconds(0.0));
    }
    let fade = UiScreenTransition::fade(1.0).with_ease(crate::skeleton2d::Bezier::LINEAR);
    ctx.ui.set_screen_transition(root, UiScreenTransition::NONE, fade);
    ctx.ui.frame(SIZE, &still);
    ctx.ui.pop_screen();
    assert!(!ctx.ui.is_shown(card_id));
    ctx.ui.frame(SIZE, &UiInput { dt: 0.5, ..still });
    ctx.draw_retained_ui();
    let going = painter.shot(&mut screen, SIZE);
    assert_near("card going off", bounds(&going, card), card_rect);
    let fading = going.pixel(card_rect.min.x as u32 + 4, card_rect.min.y as u32 + 4);
    assert!(
        fading[0] > 90 && fading[0] + 30 < full[0],
        "half gone: {fading:?}, at rest {full:?}"
    );
}
