//! Guards with behaviour trees: they walk their rounds, chase the intruder
//! they see, and go home to rest when tired or hurt. What they do comes
//! from `assets/ai/guard.json`: edit it while the game runs and they
//! follow the new tree. Open the inspector (`) and click a guard to see its
//! tree, what each node did at the last think, and its blackboard.
//!
//!   `cargo run --example ai_guards`
//!   `GUARDS=40 cargo run --example ai_guards`
//!
//! The guards' systems are in `game.rs`, played headlessly by
//! `tests/ai_guards.rs`.
mod game;

use game::*;
use ponko::ai::{Blackboard, Brain};
use ponko::camera::Rect;
use ponko::d2::Transform;
use ponko::prelude::*;

fn color(brain: &Brain) -> Color {
    match brain.action().map(|k| k.name()) {
        Some("chase") => Color::hex(0xFF6B5B),
        Some("go_home") => Color::hex(0xF2C14E),
        Some("rest") => Color::hex(0x7BD389),
        Some("patrol") => Color::hex(0x5BB8FF),
        _ => Color::hex(0x9AA0A6),
    }
}

fn draw(ctx: &mut Ctx) {
    ctx.gfx
        .draw_rect(Rect::new(0.0, 0.0, YARD.x, YARD.y), Color::hex(0x1E2A22));
    let mut guards = Vec::new();
    ctx.world
        .query::<(&Transform, &Guard, &Brain, &Blackboard)>()
        .for_each(|(t, g, brain, bb)| guards.push((t.position, g.home, color(brain), brain.action(), bb.num(HEALTH))));
    for (at, home, color, action, health) in guards {
        ctx.gfx.draw_circle(home, 6.0, Color::hex(0x3A4A40));
        ctx.gfx.draw_circle(at, SIGHT, color.with_alpha(0.06));
        ctx.gfx.draw_circle(at, 10.0, color);
        let bar = 24.0 * health.clamp(0.0, 1.0);
        ctx.gfx
            .draw_rect(Rect::new(at.x - 12.0, at.y - 20.0, bar, 3.0), Color::hex(0x7BD389));
        if let Some(action) = action {
            ctx.gfx
                .draw_text(action.name(), at + vec2(14.0, -6.0), 14.0, Color::WHITE);
        }
    }
    let intruder = ctx
        .world
        .query::<(&Transform, &Intruder)>()
        .iter()
        .next()
        .map(|(t, _)| t.position);
    if let Some(at) = intruder {
        ctx.gfx.draw_circle(at, 8.0, Color::WHITE);
    }
    let tally = ctx.world.resource::<Tally>().clone();
    let line = format!(
        "catches {}   rests {}   (` opens the inspector)",
        tally.catches, tally.rests
    );
    ctx.gfx.draw_text(&line, vec2(12.0, YARD.y + 20.0), 18.0, Color::WHITE);
}

fn main() {
    let guards = std::env::var("GUARDS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    game()
        .hot_reload(true)
        .system(draw)
        .setup(move |ctx| {
            setup(ctx, guards);
            ctx.camera.look_at(YARD / 2.0);
        })
        .run();
}
