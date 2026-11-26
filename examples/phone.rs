//! A phone check: what the engine reads from a phone or tablet, on screen
//! and printed. The guide's iOS chapter runs it on the simulator.
//!
//! Run with: `cargo run --example phone` (a desktop shows no insets and no
//! keyboard), or package it for iOS:
//! `ponko package --example phone --target ios-sim`.
//!
//! - The safe area is outlined in green; what the on-screen keyboard
//!   covers is red.
//! - Each finger is a circle; a tap opens or closes the keyboard, and what
//!   is typed shows at the top of the safe area.
//! - Suspend, resume and memory warnings are listed and printed.
//! - `PONKO_PHONE_KEYBOARD=1` opens the keyboard at the start.

use ponko::audio::AudioSessionCategory;
use ponko::prelude::*;
use ponko::window::{SafeInsets, WindowEvent};

fn main() {
    Game::new("Phone check", 390, 844).run(|ctx| {
        // The player's own music may play on under the game's sounds.
        match ctx.audio.set_session_category(AudioSessionCategory::Ambient) {
            Ok(()) => println!("phone: audio session {:?}", ctx.audio.session_category()),
            Err(e) => println!("phone: audio session not set: {e}"),
        }
        match ctx.window.clipboard_write("ponko phone check") {
            Ok(()) => println!("phone: clipboard written"),
            Err(e) => println!("phone: clipboard not written: {e}"),
        }
        ctx.input
            .set_action("keyboard", Action::Button(vec![ButtonBinding::Gesture(Gesture::Tap)]));
        if std::env::var("PONKO_PHONE_KEYBOARD").is_ok_and(|v| v == "1") {
            ctx.input.set_ime_allowed(true);
        }

        let mut log: Vec<String> = Vec::new();
        let mut typed = String::new();
        let mut shown = (SafeInsets::NONE, -1.0_f32, Vec2::ZERO);
        let mut clipboard_read = false;

        let mut frames = 0_u64;
        let mut next_report = 5.0_f64;
        move |ctx: &mut Ctx| {
            // How many frames ran: a check that they keep coming.
            frames += 1;
            if ctx.time.elapsed() >= next_report {
                next_report += 5.0;
                println!("phone: {frames} frames in {:.1} s", ctx.time.elapsed());
            }
            if !clipboard_read {
                clipboard_read = true;
                match ctx.window.clipboard_read() {
                    Ok(text) => println!("phone: clipboard holds {text:?}"),
                    Err(e) => println!("phone: clipboard not read: {e}"),
                }
            }
            for event in ctx.events.read::<WindowEvent>() {
                let line = match event {
                    WindowEvent::Suspended => "suspended",
                    WindowEvent::Resumed => "resumed",
                    WindowEvent::LowMemory => "low memory",
                    _ => continue,
                };
                println!("phone: {line} at {:.1} s", ctx.time.elapsed());
                log.push(format!("{line} at {:.1} s", ctx.time.elapsed()));
            }
            let now = (ctx.window.safe_insets(), ctx.window.keyboard_inset(), ctx.window_size());
            if now != shown {
                shown = now;
                let i = now.0;
                println!(
                    "phone: window {:.0} x {:.0}, safe insets top {:.0} right {:.0} bottom {:.0} left {:.0}, keyboard {:.0}",
                    now.2.x, now.2.y, i.top, i.right, i.bottom, i.left, now.1
                );
            }
            if ctx.input.action_pressed("keyboard") {
                let open = !ctx.input.ime_allowed();
                ctx.input.set_ime_allowed(open);
            }
            typed.push_str(ctx.input.typed_text());
            if ctx.input.key_pressed(Key::Backspace) {
                typed.pop();
            }

            ctx.gfx.clear(Color::hex(0x1d2733));
            let size = ctx.window_size();
            let safe = ctx.window.safe_area();
            let green = Color::hex(0x7bd389);
            let (a, b) = (safe.min, safe.max);
            for (from, to) in [
                (a, vec2(b.x, a.y)),
                (vec2(b.x, a.y), b),
                (b, vec2(a.x, b.y)),
                (vec2(a.x, b.y), a),
            ] {
                ctx.gfx.draw_line(from, to, 4.0, green);
            }
            let keyboard = ctx.window.keyboard_inset();
            if keyboard > 0.0 {
                let covered = Rect::new(0.0, size.y - keyboard, size.x, size.y);
                ctx.gfx.draw_rect(covered, Color::hex(0xe05a47).with_alpha(0.5));
            }
            for touch in ctx.input.touches() {
                ctx.gfx.draw_circle(touch.position(), 44.0, Color::hex(0xf2c14e).with_alpha(0.7));
            }

            let white = Color::WHITE;
            let mut y = a.y + 12.0;
            let mut line = |ctx: &mut Ctx, text: &str| {
                ctx.gfx.draw_text(text, vec2(a.x + 12.0, y), 18.0, white);
                y += 24.0;
            };
            let i = ctx.window.safe_insets();
            line(ctx, &format!("window {:.0} x {:.0}", size.x, size.y));
            line(
                ctx,
                &format!("insets {:.0} {:.0} {:.0} {:.0}", i.top, i.right, i.bottom, i.left),
            );
            line(ctx, &format!("keyboard {keyboard:.0}"));
            line(ctx, &format!("touches {}", ctx.input.touches().len()));
            line(ctx, &format!("typed: {typed}"));
            for entry in log.iter().rev().take(6) {
                line(ctx, entry);
            }
        }
    });
}
