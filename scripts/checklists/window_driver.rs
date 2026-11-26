//! A driver for the window checks (hardware-*.md, W1 to W7): copy it to
//! `examples/window_driver.rs`, run it with `cargo run --example window_driver`,
//! and delete the copy afterwards. Do not commit it.
//!
//! F1 mode (windowed, borderless, exclusive, windowed ...)   F2 title bar on/off
//! F3 always on top on/off   F4 cursor grab (none, confined, locked)
//! F5 cursor shown/hidden    F6 next system cursor   F7 save the settings
//! F8 print what the OS reports   Esc quit (the first Esc only asks to close:
//! check W6 with the close request below)
use ponko::prelude::*;
use ponko::window::{Cursor, CursorGrab, CursorIcon, WindowMode};

const ICONS: [CursorIcon; 6] = [
    CursorIcon::Default,
    CursorIcon::Pointer,
    CursorIcon::Crosshair,
    CursorIcon::Text,
    CursorIcon::Move,
    CursorIcon::NotAllowed,
];

fn main() {
    let mut decorations = true;
    let mut on_top = false;
    let mut grab = 0_usize;
    let mut shown = true;
    let mut icon = 0_usize;
    Game::new("Ponko: window checks", 800, 600)
        .restore_window_settings(true)
        .run(move |ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::F1) {
                let next = match ctx.window.mode() {
                    WindowMode::Windowed => WindowMode::Borderless(None),
                    WindowMode::Borderless(_) => {
                        let exclusive = ctx.window.current_monitor().and_then(|monitor| {
                            let best = ctx.window.exclusive_modes(monitor).ok()?.last().cloned()?;
                            Some(WindowMode::Exclusive(monitor, best))
                        });
                        exclusive.unwrap_or(WindowMode::Windowed)
                    }
                    WindowMode::Exclusive(..) => WindowMode::Windowed,
                    _ => WindowMode::Windowed,
                };
                let shown_mode = format!("{next:?}");
                println!("set_mode({shown_mode}) -> {:?}", ctx.window.set_mode(next));
            }
            if ctx.input.key_pressed(Key::F2) {
                decorations = !decorations;
                println!("set_decorations({decorations}) -> {:?}", ctx.window.set_decorations(decorations));
            }
            if ctx.input.key_pressed(Key::F3) {
                on_top = !on_top;
                println!("set_always_on_top({on_top}) -> {:?}", ctx.window.set_always_on_top(on_top));
            }
            if ctx.input.key_pressed(Key::F4) {
                grab = (grab + 1) % 3;
                let mode = [CursorGrab::None, CursorGrab::Confined, CursorGrab::Locked][grab];
                println!("set_cursor_grab({mode:?}) -> {:?}", ctx.window.set_cursor_grab(mode));
            }
            if ctx.input.key_pressed(Key::F5) {
                shown = !shown;
                println!("set_cursor_visible({shown}) -> {:?}", ctx.window.set_cursor_visible(shown));
            }
            if ctx.input.key_pressed(Key::F6) {
                icon = (icon + 1) % ICONS.len();
                println!("set_cursor({:?}) -> {:?}", ICONS[icon], ctx.window.set_cursor(Cursor::System(ICONS[icon])));
            }
            if ctx.input.key_pressed(Key::F7) {
                println!("save_settings -> {:?}", ctx.window.save_settings(&mut ctx.storage));
            }
            if ctx.input.key_pressed(Key::F8) {
                println!("{:#?}", ctx.window.state());
            }
            if ctx.window.close_requested() {
                println!("close requested; F8 prints the state, Esc again quits");
                ctx.window.cancel_close();
            }
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
        });
}
