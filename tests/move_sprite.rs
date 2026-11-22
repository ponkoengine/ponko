//! The `move_sprite` example (Bevy's `move_sprite`, ported), played
//! headlessly: the sprite moves at its speed, turns at both ends and stays
//! between them.
#[path = "../examples/move_sprite/game.rs"]
mod game;

use game::{Direction, SPEED, TURN_AT, game, sprite_movement};
use ponko::d2::Transform;
use ponko::prelude::*;

fn sprite(world: &mut World) -> (f32, Direction) {
    let mut query = world.query::<(&Transform, &Direction)>();
    let (t, d) = query.iter().next().expect("the sprite");
    (t.position.x, *d)
}

#[test]
fn the_sprite_moves_right_at_its_speed() {
    let mut world = game().run_headless(60); // one second at 60 ticks a second
    let (x, direction) = sprite(&mut world);
    assert!((x - SPEED).abs() < 1e-2, "x = {x}");
    assert_eq!(direction, Direction::Right);
}

#[test]
fn the_sprite_turns_at_both_ends_and_stays_between_them() {
    #[derive(Default)]
    struct Seen {
        min: f32,
        max: f32,
        turns: u32,
        last: Option<Direction>,
    }
    let world = game()
        .setup(|ctx| ctx.world.insert_resource(Seen::default()))
        .sim_system(|ctx: &mut Ctx| {
            let (x, d) = sprite(&mut ctx.world);
            let seen = ctx.world.resource_mut::<Seen>();
            seen.min = seen.min.min(x);
            seen.max = seen.max.max(x);
            if seen.last.is_some_and(|last| last != d) {
                seen.turns += 1;
            }
            seen.last = Some(d);
        })
        .run_headless(600); // ten seconds: 1,500 units of travel
    let seen = world.resource::<Seen>();
    let step = SPEED / 60.0;
    assert!(seen.max > TURN_AT && seen.max <= TURN_AT + step, "max = {}", seen.max);
    assert!(seen.min < -TURN_AT && seen.min >= -TURN_AT - step, "min = {}", seen.min);
    // 200 out to the first turn, then a turn every 400: at 200, 600, 1,000
    // and 1,400 units.
    assert_eq!(seen.turns, 4);
}

#[test]
fn the_movement_system_moves_only_entities_with_a_direction() {
    let mut world = Game::new("move_sprite", 800, 600)
        .sim_system(sprite_movement)
        .setup(|ctx| {
            ctx.world.spawn((Transform::at(Vec2::ZERO), Direction::Left));
            ctx.world.spawn((Transform::at(vec2(5.0, 0.0)),));
        })
        .run_headless(30);
    let mut xs: Vec<(f32, bool)> = world
        .query::<(&Transform, Option<&Direction>)>()
        .iter()
        .map(|(t, d)| (t.position.x, d.is_some()))
        .collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!((xs[0].0 + SPEED / 2.0).abs() < 1e-2 && xs[0].1);
    assert_eq!(xs[1], (5.0, false));
}
