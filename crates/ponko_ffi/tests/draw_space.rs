//! The named coordinate space of immediate sprite draws from C.
use ponko_ffi::*;

#[test]
fn draw_space_constants_keep_their_values() {
    assert_eq!(PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD as i32, 0);
    assert_eq!(PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN as i32, 1);
}

#[test]
fn with_space_changes_only_the_space_and_round_trips() {
    let mut params = ponko_draw_params_default();
    params.z = 7.0;
    params.rotation = 0.4;
    assert!(!params.screen, "the default is world units");

    let screen = ponko_draw_params_with_space(params, PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN);
    assert!(screen.screen);
    assert_eq!((screen.z, screen.rotation), (7.0, 0.4));

    let world = ponko_draw_params_with_space(screen, PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD);
    assert!(!world.screen);
    assert_eq!((world.z, world.rotation), (7.0, 0.4));
}

#[test]
fn debug_styles_take_the_same_draw_space_as_sprites() {
    let mut style = ponko_debug_style_default();
    style.width = 3.0;
    style.on_top = true;
    let screen = ponko_debug_style_with_space(style, PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN);
    assert!(screen.screen);
    assert_eq!((screen.width, screen.on_top), (3.0, true), "nothing else changes");
    let world = ponko_debug_style_with_space(screen, PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD);
    assert_eq!(world, style);
}
