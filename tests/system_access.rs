//! Declared system access: conflict rules, the report, and the debug check
//! that a system stays within its declaration. Headless.

#[allow(unused_imports)]
use ponko::access::{AccessConflict, SystemAccess};
use ponko::prelude::*;

#[derive(Component)]
struct Pos(f32);
#[derive(Component)]
struct Vel(f32);
#[derive(Component)]
struct Health(i32);
struct Gravity(f32);
struct Score(u32);
#[derive(Component)]
struct Hit;

fn names(a: &SystemAccess, b: &SystemAccess) -> Vec<String> {
    a.conflicts_with(b).iter().map(ToString::to_string).collect()
}

// ---- conflict rules -----------------------------------------------------------

#[test]
fn readers_of_the_same_data_do_not_conflict() {
    let a = SystemAccess::new()
        .read::<Pos>()
        .read_resource::<Gravity>()
        .read_events::<Hit>();
    let b = SystemAccess::new()
        .read::<Pos>()
        .read_resource::<Gravity>()
        .read_events::<Hit>();
    assert!(a.conflicts_with(&b).is_empty());
}

#[test]
fn disjoint_writers_do_not_conflict() {
    let a = SystemAccess::new().write::<Pos>().write_resource::<Gravity>();
    let b = SystemAccess::new().write::<Health>().write_resource::<Score>();
    assert!(a.conflicts_with(&b).is_empty());
}

#[test]
fn a_writer_conflicts_with_a_reader_of_the_same_component() {
    let a = SystemAccess::new().write::<Vel>();
    let b = SystemAccess::new().read::<Vel>().write::<Pos>();
    assert_eq!(
        a.conflicts_with(&b),
        [AccessConflict::Component(std::any::type_name::<Vel>())]
    );
    assert_eq!(b.conflicts_with(&a), a.conflicts_with(&b), "symmetric");
}

#[test]
fn two_writers_of_a_resource_conflict() {
    let a = SystemAccess::new().write_resource::<Score>();
    let b = SystemAccess::new().write_resource::<Score>();
    assert_eq!(names(&a, &b), ["resource `system_access::Score`"]);
}

#[test]
fn senders_conflict_with_readers_and_with_other_senders() {
    let send = SystemAccess::new().send_events::<Hit>();
    let read = SystemAccess::new().read_events::<Hit>();
    assert_eq!(
        send.conflicts_with(&read),
        [AccessConflict::Event(std::any::type_name::<Hit>())]
    );
    assert_eq!(read.conflicts_with(&send).len(), 1);
    assert_eq!(send.conflicts_with(&send.clone()).len(), 1);
}

#[test]
fn commands_conflict_with_every_other_system() {
    let spawner = SystemAccess::new().commands();
    assert_eq!(
        spawner.conflicts_with(&SystemAccess::new().read::<Pos>()),
        [AccessConflict::Commands]
    );
    assert_eq!(
        spawner.conflicts_with(&SystemAccess::new().read_resource::<Score>()),
        [AccessConflict::Commands]
    );
    assert_eq!(spawner.conflicts_with(&spawner.clone()), [AccessConflict::Commands]);
    // Even one that declares no data: it can still see the world's
    // structure (`len`, `is_alive`, `has`), which commands change.
    assert_eq!(
        spawner.conflicts_with(&SystemAccess::new().send_events::<Hit>()),
        [AccessConflict::Commands]
    );
    assert_eq!(spawner.conflicts_with(&SystemAccess::new()), [AccessConflict::Commands]);
}

#[test]
fn exclusive_conflicts_with_everything() {
    let all = SystemAccess::exclusive();
    assert!(all.is_exclusive());
    assert_eq!(all.conflicts_with(&SystemAccess::new()), [AccessConflict::Exclusive]);
    assert_eq!(SystemAccess::new().conflicts_with(&all), [AccessConflict::Exclusive]);
}

// ---- the report ----------------------------------------------------------------

fn movement(ctx: &mut Ctx) {
    ctx.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0);
}

fn fall(ctx: &mut Ctx) {
    let g = ctx.world.resource::<Gravity>().0;
    ctx.world.query::<&mut Vel>().for_each(|v| v.0 += g);
}

fn decay(ctx: &mut Ctx) {
    ctx.world.query::<&mut Health>().for_each(|h| h.0 -= 1);
}

fn undeclared(_: &mut Ctx) {}

fn game() -> Game {
    Game::new("access", 320, 240)
        .add(
            Phase::AfterPhysics,
            fall.access(SystemAccess::new().write::<Vel>().read_resource::<Gravity>()),
        )
        .add(
            Phase::AfterPhysics,
            movement.access(SystemAccess::new().write::<Pos>().read::<Vel>()),
        )
        .add(Phase::AfterPhysics, decay.access(SystemAccess::new().write::<Health>()))
        .sim_system(undeclared)
        .add(
            Phase::Frame,
            (|_: &mut Ctx| {}).access(SystemAccess::new().read::<Pos>()),
        )
}

#[test]
fn the_report_lists_which_declared_systems_could_run_together() {
    let report = game().access_report();
    // Ours plus the built-in drawing systems, which stay undeclared.
    assert_eq!(report.systems.iter().filter(|s| s.access.is_some()).count(), 4);
    let plain = report.systems.iter().find(|s| s.name.ends_with("undeclared")).unwrap();
    assert!(plain.access.is_none(), "plain sim_system stays undeclared (exclusive)");
    assert_eq!(plain.phase, Phase::AfterPhysics);
    // Three declared AfterPhysics systems: three pairs. The frame system is in another phase.
    assert_eq!(report.pairs.len(), 3);
    let short = |n: &str| n.rsplit("::").next().unwrap().to_string();
    let mut together: Vec<(String, String)> = report.compatible_pairs().map(|(a, b)| (short(a), short(b))).collect();
    together.sort();
    // fall + decay don't conflict, but movement, between them, conflicts
    // with fall: batching them would move decay before movement.
    assert_eq!(together, [("movement".to_string(), "decay".to_string())]);
    let text = report.to_string();
    assert!(
        text.contains("fall + system_access::decay: conflict on `system_access::movement`, which runs between them"),
        "{text}"
    );
    assert!(
        text.contains("fall + system_access::movement: conflict on component `system_access::Vel`"),
        "{text}"
    );
}

#[test]
fn declared_systems_run_in_order_like_any_other() {
    let world = game().run_headless(2, |ctx| {
        ctx.world.insert_resource(Gravity(1.0));
        ctx.world.spawn((Pos(0.0), Vel(0.0), Health(10)));
    });
    let (p, v, h) = world.query_one();
    // fall then movement each tick: v = 1, 2; p = 1, 3.
    assert_eq!((p, v, h), (3.0, 2.0, 8));
}

trait QueryOne {
    fn query_one(self) -> (f32, f32, i32);
}

impl QueryOne for World {
    fn query_one(mut self) -> (f32, f32, i32) {
        let mut out = (0.0, 0.0, 0);
        self.query::<(&Pos, &Vel, &Health)>()
            .for_each(|(p, v, h)| out = (p.0, v.0, h.0));
        out
    }
}

// ---- debug validation ------------------------------------------------------------

fn run_one(access: SystemAccess, system: impl FnMut(&mut Ctx) + 'static) -> World {
    Game::new("access", 320, 240)
        .add(Phase::AfterPhysics, system.access(access))
        .run_headless(1, |ctx| {
            ctx.world.insert_resource(Score(0));
            ctx.world.spawn((Pos(0.0), Vel(1.0)));
        })
}

#[test]
fn a_system_within_its_declaration_runs() {
    let world = run_one(
        SystemAccess::new()
            .write::<Pos>()
            .read::<Vel>()
            .write_resource::<Score>()
            .send_events::<Hit>()
            .read_events::<Hit>()
            .commands(),
        |ctx| {
            ctx.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0);
            ctx.world.resource_mut::<Score>().0 += 1;
            ctx.events.send(Hit);
            let _ = ctx.events.read::<Hit>().len();
            ctx.commands.spawn((Health(1),));
        },
    );
    assert_eq!(world.resource::<Score>().0, 1);
    assert_eq!(world.len(), 2, "the command was applied");
}

#[test]
fn exclusive_access_is_not_checked() {
    run_one(SystemAccess::exclusive(), |ctx| {
        ctx.world.spawn((Health(1),));
    });
}

#[cfg(debug_assertions)]
mod violations {
    use super::*;

    #[test]
    #[should_panic(expected = "writes component `system_access::Pos`")]
    fn writing_a_component_declared_as_read_panics() {
        run_one(SystemAccess::new().read::<Pos>(), |ctx| {
            ctx.world.query::<&mut Pos>().for_each(|p| p.0 += 1.0);
        });
    }

    #[test]
    #[should_panic(expected = "reads component `system_access::Vel`")]
    fn reading_an_undeclared_component_panics() {
        run_one(SystemAccess::new().write::<Pos>(), |ctx| {
            ctx.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0);
        });
    }

    #[test]
    #[should_panic(expected = "writes resource `system_access::Score`")]
    fn writing_an_undeclared_resource_panics() {
        run_one(SystemAccess::new().read_resource::<Score>(), |ctx| {
            ctx.world.resource_mut::<Score>().0 += 1;
        });
    }

    #[test]
    #[should_panic(expected = "sends events `system_access::Hit`")]
    fn sending_undeclared_events_panics() {
        run_one(SystemAccess::new().read_events::<Hit>(), |ctx| ctx.events.send(Hit));
    }

    #[test]
    #[should_panic(expected = "reads events `system_access::Hit`")]
    fn reading_events_declared_only_as_sent_panics() {
        run_one(SystemAccess::new().send_events::<Hit>(), |ctx| {
            let _ = ctx.events.read::<Hit>().len();
        });
    }

    #[test]
    #[should_panic(expected = "records commands")]
    fn undeclared_commands_panic() {
        run_one(SystemAccess::new(), |ctx| {
            ctx.commands.spawn((Health(1),));
        });
    }

    #[test]
    #[should_panic(expected = "changes the world's structure directly")]
    fn spawning_directly_needs_exclusive_access() {
        run_one(SystemAccess::new().commands(), |ctx| {
            ctx.world.spawn((Health(1),));
        });
    }

    #[test]
    #[should_panic(expected = "system `system_access::violations::")]
    fn the_message_names_the_system() {
        fn sneaky(ctx: &mut Ctx) {
            ctx.world.resource_mut::<Score>().0 += 1;
        }
        run_one(SystemAccess::new(), sneaky);
    }
}

// ---- declarations by name (the C and C++ path) ---------------------------------------

mod named {
    use super::*;
    use std::alloc::Layout;

    fn store(ctx: &mut Ctx) {
        let value = 5u32;
        // SAFETY: four readable bytes for a 4-byte layout.
        unsafe {
            ctx.world
                .raw()
                .insert_resource_dynamic("lives", Layout::new::<u32>(), (&raw const value).cast());
        }
    }

    #[test]
    fn named_components_resources_and_raw_events_cover_their_use() {
        let world = Game::new("access", 320, 240)
            .add(
                Phase::AfterPhysics,
                (|ctx: &mut Ctx| {
                    ctx.world.query::<&mut Pos>().for_each(|p| p.0 += 1.0);
                    let lives = ctx.world.raw_ref().get_resource_dynamic("lives").unwrap();
                    // SAFETY: stored as a u32 in setup.
                    let n = unsafe { *lives.cast::<u32>() };
                    let hit = ctx.world.register::<Hit>().index() as u32;
                    ctx.events.send_raw(hit, &n.to_ne_bytes()).unwrap();
                    let (_, count) = ctx.events.read_raw(hit, 4).unwrap();
                    assert_eq!(count, 1);
                })
                .access(
                    SystemAccess::new()
                        .write_named(std::any::type_name::<Pos>())
                        .read_resource_named("lives")
                        .send_events_named(std::any::type_name::<Hit>())
                        .read_events_named(std::any::type_name::<Hit>()),
                ),
            )
            .run_headless(1, |ctx| {
                store(ctx);
                ctx.world.spawn((Pos(0.0),));
            });
        let mut world = world;
        assert_eq!(world.query::<&Pos>().iter().next().unwrap().0, 1.0);
    }

    #[test]
    fn named_declarations_conflict_by_name() {
        let a = SystemAccess::new().write_named("pos");
        let b = SystemAccess::new().read_named("pos");
        assert_eq!(a.conflicts_with(&b), [AccessConflict::Component("pos")]);
        assert!(a.conflicts_with(&SystemAccess::new().read_named("vel")).is_empty());
        let r = SystemAccess::new().write_resource_named("lives");
        assert_eq!(
            r.conflicts_with(&SystemAccess::new().read_resource_named("lives")),
            [AccessConflict::Resource("lives")]
        );
        let send = SystemAccess::new().send_events_named("hit");
        let read = SystemAccess::new().read_events_named("hit");
        assert_eq!(send.conflicts_before(&read), [AccessConflict::Event("hit")]);
        assert!(read.conflicts_before(&send).is_empty());
        assert!(
            send.conflicts_with(&SystemAccess::new().read_events_named("miss"))
                .is_empty()
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "uses resource `lives`, which has no Rust type")]
    fn an_undeclared_dynamic_resource_panics() {
        Game::new("access", 320, 240)
            .add(
                Phase::AfterPhysics,
                (|ctx: &mut Ctx| {
                    let _ = ctx.world.raw_ref().get_resource_dynamic("lives");
                })
                .access(SystemAccess::new()),
            )
            .run_headless(1, store);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "sends events `system_access::Hit`")]
    fn undeclared_raw_events_panic() {
        Game::new("access", 320, 240)
            .add(
                Phase::AfterPhysics,
                (|ctx: &mut Ctx| {
                    let hit = ctx.world.register::<Hit>().index() as u32;
                    ctx.events.send_raw(hit, &[1, 2, 3, 4]).unwrap();
                })
                .access(SystemAccess::new().read_events_named(std::any::type_name::<Hit>())),
            )
            .run_headless(1, |_| {});
    }

    #[test]
    fn a_named_declaration_covers_any_use_because_raw_access_cannot_tell() {
        let mut world = Game::new("access", 320, 240)
            .add(
                Phase::AfterPhysics,
                (|ctx: &mut Ctx| {
                    let id = ctx.world.component_id::<Pos>().unwrap();
                    ctx.world.raw().for_each_chunk(&[id], |_| {});
                })
                .access(SystemAccess::new().read_named(std::any::type_name::<Pos>())),
            )
            .run_headless(1, |ctx| {
                ctx.world.spawn((Pos(0.0),));
            });
        assert_eq!(world.query::<&Pos>().count(), 1);
    }
}
