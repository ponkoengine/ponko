//! Ponko's side of the cross-engine scoreboard (`compare/...`): the
//! workloads `bench/compare` runs again with Bevy, written as ordinary Ponko
//! games and timed the same way on both sides: the time between the starts
//! of two consecutive frames (or ticks), so a frame is everything the engine
//! does for it. The GPU workloads (feature `render`) draw every frame with
//! the window's renderer into an offscreen 1280 x 720 target and wait for
//! the GPU before the next ([`Game::bench_offscreen`]); the others are
//! headless ticks. `bench/compare/README.md` has the method and the other
//! engine's side; `book/src/performance.md` the results.
//!
//! - `compare/sim-1m`: one headless tick of 1,000,000 entities moved by one
//!   simulation system (`pos += vel * dt`).
//! - `compare/physics-10k`: one tick of 10,000 dynamic 10 px boxes falling
//!   into a pile between walls (`Game::physics`, rapier underneath).
//! - `compare/sprites-100k`, `compare/sprites-1m`: one frame of 100,000 or
//!   1,000,000 bouncing 32 x 32 white sprites of one texture (the `bunnymark`
//!   movement), drawn. `compare/sprites-100k-tinted`: the 100,000 with a
//!   different tint each.
//! - `compare/ui-10k-still`: one frame of a retained UI of 10,000 colored boxes in
//!   a 100 x 100 grid that doesn't change, drawn. `compare/ui-10k-recolor`:
//!   every box gets a new color every frame.

use std::time::{Duration, Instant};

use ponko::prelude::*;

use crate::{Bench, report};

const WARMUP: usize = 60;

/// Starts of the frames (or ticks) so far.
#[derive(Default)]
struct Stamps(Vec<Instant>);

/// The first system of every frame: when it started.
fn stamp(ctx: &mut Ctx) {
    ctx.world.resource_mut::<Stamps>().0.push(Instant::now());
}

/// The frame times after the warm-up: the time between two starts.
fn frame_times(world: &mut World) -> Vec<Duration> {
    let stamps = world.remove_resource::<Stamps>().expect("the frame stamps").0;
    stamps.windows(2).skip(WARMUP).map(|w| w[1] - w[0]).collect()
}

fn record(b: &mut Bench, name: &str, mut times: Vec<Duration>) {
    b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
}

#[derive(Component, Clone, Copy)]
struct Pos(Vec2);
#[derive(Component, Clone, Copy)]
struct Vel(Vec2);

fn sim_move(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0 * dt);
}

/// The world of `compare/sim-1m`: `n` entities, each moving its own way.
fn sim_world(ctx: &mut Ctx, n: usize) {
    ctx.world.spawn_batch((0..n).map(|i| {
        let f = i as f32;
        (Pos(vec2(f, 0.0)), Vel(vec2(1.0 + (f * 0.37).sin(), (f * 0.11).cos())))
    }));
}

fn sim(b: &mut Bench) {
    const NAME: &str = "compare/sim-1m";
    if b.skips(NAME) {
        return;
    }
    let mut world = Game::new("sim", 1, 1)
        .sim_system(stamp)
        .sim_system(sim_move)
        .run_headless((WARMUP + 300 + 1) as u64, |ctx| {
            ctx.world.insert_resource(Stamps::default());
            sim_world(ctx, 1_000_000);
        });
    record(b, NAME, frame_times(&mut world));
}

fn physics(b: &mut Bench) {
    use ponko::d2::{Collider, RigidBody, Transform as Transform2D};
    const NAME: &str = "compare/physics-10k";
    if b.skips(NAME) {
        return;
    }
    let (boxes, width) = crate::physics::scene(10_000);
    let mut world = Game::new("physics", 1, 1)
        .physics(vec2(0.0, 980.0))
        .sim_system(stamp)
        .run_headless((WARMUP + 300 + 1) as u64, move |ctx| {
            ctx.world.insert_resource(Stamps::default());
            let wall = |x: f32, y: f32, w: f32, h: f32| {
                (
                    Transform2D::at(vec2(x, y)),
                    RigidBody::fixed(),
                    Collider::rect(vec2(w, h)),
                )
            };
            ctx.world.spawn(wall(0.0, 100.0, width + 40.0, 20.0));
            ctx.world.spawn(wall(-width / 2.0 - 10.0, -500.0, 20.0, 1200.0));
            ctx.world.spawn(wall(width / 2.0 + 10.0, -500.0, 20.0, 1200.0));
            for &(x, y) in &boxes {
                ctx.world.spawn((
                    Transform2D::at(vec2(x, y)),
                    RigidBody::dynamic(),
                    Collider::rect(vec2(10.0, 10.0)),
                ));
            }
        });
    record(b, NAME, frame_times(&mut world));
}

#[cfg(feature = "render")]
mod gpu {
    use super::*;
    use ponko::d2::{Sprite, Transform as Transform2D};
    use ponko::ui::UiLayout;

    pub const SIZE: (u32, u32) = (1280, 720);

    /// The `bunnymark` sprites' velocity.
    #[derive(Component)]
    struct Bounce(Vec2);

    fn rand(seed: &mut u64) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        (*seed >> 40) as f32 / (1u64 << 24) as f32
    }

    /// `n` sprites of one 32 x 32 white texture at random places with
    /// random velocities, each tinted if `tinted` (white otherwise): the same
    /// sequence as the Bevy side.
    pub fn spawn_sprites(ctx: &mut Ctx, n: usize, tinted: bool) {
        let texture = ctx.gfx.create_texture(32, 32, &[255; 32 * 32 * 4]);
        let mut seed = 0x9E37_79B9_7F4A_7C15;
        let sprites: Vec<_> = (0..n)
            .map(|_| {
                let at = vec2(rand(&mut seed) * SIZE.0 as f32, rand(&mut seed) * SIZE.1 as f32);
                let vel = vec2(rand(&mut seed) * 400.0 - 200.0, rand(&mut seed) * 400.0 - 300.0);
                let color = Color::rgb(
                    0.6 + rand(&mut seed) * 0.4,
                    0.6 + rand(&mut seed) * 0.4,
                    0.6 + rand(&mut seed) * 0.4,
                );
                let color = if tinted { color } else { Color::WHITE };
                (Transform2D::at(at), Sprite::new(texture).with_color(color), Bounce(vel))
            })
            .collect();
        ctx.world.spawn_batch(sprites);
    }

    /// `bunnymark`'s movement, a fixed 1/60 s a frame (y grows downward).
    fn bounce(ctx: &mut Ctx) {
        let dt = 1.0 / 60.0;
        let bounds = vec2(SIZE.0 as f32, SIZE.1 as f32);
        ctx.world.query::<(&mut Transform2D, &mut Bounce)>().for_each(|(t, v)| {
            let (p, v) = (&mut t.position, &mut v.0);
            v.y += 600.0 * dt;
            *p += *v * dt;
            if p.x < 0.0 || p.x > bounds.x {
                p.x = p.x.clamp(0.0, bounds.x);
                v.x = -v.x;
            }
            if p.y > bounds.y {
                p.y = bounds.y;
                v.y = -v.y.abs() * 0.85;
            }
            if p.y < 0.0 {
                p.y = 0.0;
                v.y = 0.0;
            }
        });
    }

    /// What a run drew in its last frame, to check the workload.
    #[derive(Default)]
    pub struct Drawn {
        pub sprites: usize,
        pub adapter: String,
    }

    fn note_drawn(ctx: &mut Ctx) {
        let sprites = ctx.perf.render_stats().sprites;
        let adapter = ctx.bench_adapter().unwrap_or_default();
        let drawn = ctx.world.resource_mut::<Drawn>();
        drawn.sprites = sprites;
        drawn.adapter = adapter;
    }

    pub fn sprites_world(n: usize, tinted: bool, frames: usize) -> World {
        Game::new("sprites", SIZE.0, SIZE.1)
            .bench_offscreen()
            .sim_system(stamp)
            .sim_system(note_drawn)
            .system(bounce)
            .run_headless((WARMUP + frames + 1) as u64, move |ctx| {
                ctx.world.insert_resource(Stamps::default());
                ctx.world.insert_resource(Drawn::default());
                spawn_sprites(ctx, n, tinted);
            })
    }

    pub fn sprites(b: &mut Bench) {
        for (name, n, tinted, frames) in [
            ("compare/sprites-100k", 100_000, false, 300),
            ("compare/sprites-100k-tinted", 100_000, true, 300),
            ("compare/sprites-1m", 1_000_000, false, 120),
        ] {
            if b.skips(name) {
                continue;
            }
            let mut world = sprites_world(n, tinted, frames);
            let drawn = world.remove_resource::<Drawn>().expect("what was drawn");
            eprintln!("{name}: {} sprites drawn a frame on {}", drawn.sprites, drawn.adapter);
            record(b, name, frame_times(&mut world));
        }
    }

    /// The grid's boxes, row by row.
    #[derive(Default)]
    pub struct Boxes(pub Vec<ponko::ui::UiId>);

    fn box_color(i: usize, frame: u64) -> Color {
        let t = (i as u64 + frame) % 256;
        Color::rgb(t as f32 / 255.0, 0.4, 1.0 - t as f32 / 255.0)
    }

    /// A 100 x 100 grid of colored boxes filling the screen.
    pub fn spawn_ui(ctx: &mut Ctx, side: usize) {
        let root = ctx
            .ui
            .root()
            .layout(UiLayout::grid(side, SIZE.1 as f32 / side as f32).width(SIZE.0 as f32))
            .id();
        let boxes = (0..side * side)
            .map(|i| ctx.ui.panel(root).background(box_color(i, 0)).id())
            .collect();
        ctx.ui.push_modeless(root);
        ctx.world.insert_resource(Boxes(boxes));
    }

    fn recolor(ctx: &mut Ctx) {
        let frame = ctx.time.frame();
        let boxes = std::mem::take(&mut ctx.world.resource_mut::<Boxes>().0);
        for (i, &id) in boxes.iter().enumerate() {
            if let Some(mut paint) = ctx.ui.paint(id) {
                paint.background = box_color(i, frame);
                ctx.ui.set_paint(id, paint);
            }
        }
        ctx.world.resource_mut::<Boxes>().0 = boxes;
    }

    pub fn ui_world(recolors: bool, frames: usize) -> World {
        let mut game = Game::new("ui", SIZE.0, SIZE.1)
            .bench_offscreen()
            .sim_system(stamp)
            .sim_system(note_drawn);
        if recolors {
            game = game.system(recolor);
        }
        game.run_headless((WARMUP + frames + 1) as u64, |ctx| {
            ctx.world.insert_resource(Stamps::default());
            ctx.world.insert_resource(Drawn::default());
            spawn_ui(ctx, 100);
        })
    }

    pub fn ui(b: &mut Bench) {
        for (name, recolors) in [("compare/ui-10k-still", false), ("compare/ui-10k-recolor", true)] {
            if b.skips(name) {
                continue;
            }
            let mut world = ui_world(recolors, 300);
            record(b, name, frame_times(&mut world));
        }
    }
}

pub fn run(b: &mut Bench) {
    sim(b);
    physics(b);
    #[cfg(feature = "render")]
    {
        gpu::sprites(b);
        gpu::ui(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sim_workload_moves_every_entity_every_tick() {
        let mut world = Game::new("sim", 1, 1).sim_system(sim_move).run_headless(3, |ctx| {
            sim_world(ctx, 1000);
        });
        let mut moved = 0;
        world.query::<&Pos>().iter().enumerate().for_each(|(_, p)| {
            if p.0.x != 0.0 || p.0.y != 0.0 {
                moved += 1;
            }
        });
        assert_eq!(moved, 1000);
    }

    #[test]
    fn frame_times_skip_the_warm_up() {
        let mut world = World::new();
        let start = Instant::now();
        let stamps = (0..WARMUP + 4)
            .map(|i| start + Duration::from_millis(i as u64 * 2))
            .collect();
        world.insert_resource(Stamps(stamps));
        let times = frame_times(&mut world);
        assert_eq!(times.len(), 3);
        assert!(times.iter().all(|t| *t == Duration::from_millis(2)));
    }

    /// Skips without a GPU (a skip is not a pass).
    #[cfg(feature = "render")]
    #[test]
    fn offscreen_frames_draw_every_sprite() {
        let mut world = match std::panic::catch_unwind(|| gpu::sprites_world(500, true, 2)) {
            Ok(world) => world,
            Err(_) if std::env::var_os("PONKO_REQUIRE_GPU").is_none() => {
                eprintln!("no GPU: offscreen sprite frames not checked");
                return;
            }
            Err(e) => std::panic::resume_unwind(e),
        };
        assert_eq!(world.resource::<gpu::Drawn>().sprites, 500);
        assert!(!world.resource::<gpu::Drawn>().adapter.is_empty());
        assert_eq!(frame_times(&mut world).len(), 2);
    }
}
