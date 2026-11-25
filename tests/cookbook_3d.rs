//! Cookbook, "In 3D": clicking a 3D object and walking a character with a
//! third-person camera. Every recipe between `ANCHOR` lines is what
//! `book/src/cookbook/three-d.md` shows, and each test runs it headless for a
//! few ticks and checks that it did its job.
#![cfg(feature = "physics3d")]
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData};
use ponko::scenario::InputScenario;

/// A cube you can see (a model) and click (a collider) at `at`.
fn crate_at(ctx: &mut Ctx, at: Vec3) -> Entity {
    let paint = ctx.gfx.create_material(MaterialDesc::color(Color::hex(0xc08040)));
    let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), paint);
    ctx.world.spawn((
        Transform::at(at),
        Model::new(mesh),
        RigidBody::fixed(),
        Collider::cuboid(Vec3::ONE),
    ))
}

// ANCHOR: click_3d
#[derive(Component)]
struct Picked;

/// What is under a point of the window: a ray from the camera through it,
/// into the 3D physics world.
fn pick_at(ctx: &Ctx, pointer: Vec2) -> Option<Entity> {
    let (origin, direction) = ctx.scene3d.camera.screen_ray(pointer);
    ctx.physics3d
        .cast_ray(origin, direction, 100.0, QueryFilter::default())
        .map(|hit| hit.entity)
}

fn click_to_pick(ctx: &mut Ctx) {
    if ctx.input.mouse_pressed(MouseButton::Left)
        && !ctx.ui.wants_pointer()
        && let Some(clicked) = pick_at(ctx, ctx.input.mouse_pos())
    {
        ctx.commands.insert(clicked, Picked);
    }
}
// ANCHOR_END: click_3d

#[test]
fn a_ray_through_the_cursor_finds_the_cube_under_it_and_nothing_in_the_sky() {
    #[derive(Default)]
    struct Found(Vec<Option<Entity>>);
    let world = Game::new("pick", 800, 600)
        .physics3d(Vec3::ZERO)
        .setup(|ctx| {
            ctx.scene3d.camera.position = vec3(0.0, 2.0, 10.0);
            ctx.scene3d.camera.look_at(Vec3::ZERO);
            let left = crate_at(ctx, vec3(-3.0, 0.0, 0.0));
            let right = crate_at(ctx, vec3(3.0, 0.0, 0.0));
            ctx.world.insert_resource((left, right));
            ctx.world.insert_resource(Found::default());
        })
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                // Where the cubes appear on the window, and the top corner (sky).
                let (left, right) = *ctx.world.resource::<(Entity, Entity)>();
                let at = |ctx: &Ctx, e: Entity| {
                    let p = ctx.world.get::<Transform>(e).unwrap().position;
                    ctx.scene3d.camera.world_to_screen(p).unwrap()
                };
                let (l, r) = (at(ctx, left), at(ctx, right));
                let found = vec![pick_at(ctx, l), pick_at(ctx, r), pick_at(ctx, Vec2::new(5.0, 5.0))];
                ctx.world.resource_mut::<Found>().0 = found;
            }
        })
        .run_headless(4);
    let (left, right) = *world.resource::<(Entity, Entity)>();
    assert_eq!(world.resource::<Found>().0, [Some(left), Some(right), None]);
}

#[test]
fn clicking_marks_what_the_pointer_is_over() {
    // Two cubes; the test puts the pointer on the right one and clicks.
    let mut world = Game::new("click", 800, 600)
        .physics3d(Vec3::ZERO)
        .setup(|ctx| {
            ctx.scene3d.camera.position = vec3(0.0, 2.0, 10.0);
            ctx.scene3d.camera.look_at(Vec3::ZERO);
            crate_at(ctx, vec3(-3.0, 0.0, 0.0));
            let right = crate_at(ctx, vec3(3.0, 0.0, 0.0));
            let on_screen = ctx.scene3d.camera.world_to_screen(vec3(3.0, 0.0, 0.0)).unwrap();
            ctx.input.feed(InputEvent::MouseMove(on_screen));
            ctx.world.insert_resource(right);
        })
        .sim_system(click_to_pick)
        .input_scenario(
            InputScenario::new()
                .at(3, InputEvent::Mouse(MouseButton::Left, true))
                .at(4, InputEvent::Mouse(MouseButton::Left, false)),
        )
        .run_headless(8);
    let right = *world.resource::<Entity>();
    let picked: Vec<Entity> = world.query::<(Entity, &Picked)>().iter().map(|(e, _)| e).collect();
    assert_eq!(picked, [right], "only the cube under the pointer");
}

// ANCHOR: walk_3d
#[derive(Component)]
struct Hero;

/// The `move` action, resolved once when it is bound.
#[derive(Clone, Copy)]
struct Move(ActionId);

fn bind_walk(ctx: &mut Ctx) {
    ctx.input.set_action(
        "move",
        Action::Axis2(vec![Axis2Binding::Keys {
            left: Key::A,
            right: Key::D,
            up: Key::W,
            down: Key::S,
        }]),
    );
    let movement = ctx.input.action_id("move");
    ctx.world.insert_resource(Move(movement));
}

/// A frame system: walk relative to the camera, which follows from behind
/// and comes in front of walls.
fn walk_and_follow(ctx: &mut Ctx) {
    let dt = ctx.time.frame_dt();
    let Move(movement) = *ctx.world.resource::<Move>();
    let stick = ctx.input.action_axis2(movement); // y is down: W is -1
    ctx.world.resource_scope(|world, rig: &mut ThirdPersonCamera| {
        world.query::<(Entity, &Hero, &mut Transform)>().for_each(|(me, _, t)| {
            t.position += (rig.flat_forward() * -stick.y + rig.flat_right() * stick.x) * 5.0 * dt;
            let not_me = QueryFilter::default().exclude(me);
            rig.update_with_physics(&mut ctx.scene3d.camera, t.position, &ctx.physics3d, not_me, dt);
        });
    });
}
// ANCHOR_END: walk_3d

#[test]
fn the_hero_walks_where_the_camera_looks_and_the_camera_stays_behind() {
    let scenario = InputScenario::new()
        .at(0, InputEvent::Key(Key::W, true))
        .at(60, InputEvent::Key(Key::W, false))
        .duration(120);
    let (mut world, report) = Game::new("walk3d", 800, 600)
        .physics3d(Vec3::ZERO)
        .setup(|ctx| {
            bind_walk(ctx);
            ctx.world.insert_resource(ThirdPersonCamera::default());
            ctx.world.spawn((Hero, Transform::at(Vec3::ZERO)));
        })
        .system(walk_and_follow)
        .system(|ctx: &mut Ctx| {
            let camera = ctx.scene3d.camera.position;
            ctx.world.insert_resource(camera);
        })
        .headless_frames(true)
        .run_headless_scenario(scenario);
    report.assert_passed();
    // About a second at 5 units a second along the camera's flat forward: -Z by default.
    let at = world.query::<(&Hero, &Transform)>().iter().next().unwrap().1.position;
    assert!(
        (-7.0..-4.5).contains(&at.z) && at.x.abs() < 0.01,
        "walked forward: {at}"
    );
    let camera = *world.resource::<Vec3>();
    assert!(
        camera.z > at.z + 2.0 && camera.y > 0.5,
        "behind and above the hero: {camera} (hero {at})"
    );
}
