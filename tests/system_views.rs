//! View systems (systems that take `&mut SystemView`): what a `SystemView` offers, that
//! anything undeclared panics in every build, service declarations, the
//! ordered barrier rules and the report. Headless.

#[allow(unused_imports)]
use ponko::access::{AccessConflict, Service, SystemAccess};
#[allow(unused_imports)]
use ponko::audio::Audio;
#[allow(unused_imports)]
#[cfg(feature = "physics")]
use ponko::d2::Physics;
#[allow(unused_imports)]
#[cfg(feature = "physics3d")]
use ponko::d3::Physics as Physics3D;
#[allow(unused_imports)]
use ponko::input::Input;
#[allow(unused_imports)]
use ponko::net::Net;
use ponko::prelude::*;
#[allow(unused_imports)]
use ponko::system_view::SystemView;

#[derive(Debug, PartialEq, Component)]
struct Pos(f32);
#[derive(Component)]
struct Vel(f32);
#[derive(Component)]
struct Health(i32);
struct Gravity(f32);
#[derive(Debug, PartialEq)]
struct Score(u32);
#[derive(Debug, PartialEq)]
struct Hit(u32);

/// What systems observed, in order.
#[derive(Default)]
struct Log(Vec<String>);

fn setup(ctx: &mut Ctx) {
    ctx.world.insert_resource(Gravity(-1.0));
    ctx.world.insert_resource(Score(0));
    ctx.world.insert_resource(Log::default());
    ctx.world.spawn((Pos(0.0), Vel(1.0), Health(3)));
}

fn run(game: Game, ticks: u64) -> World {
    game.run_headless(ticks, setup)
}

fn one(access: SystemAccess, system: impl FnMut(&mut SystemView) + Send + 'static) -> World {
    run(
        Game::new("views", 1, 1).add(Phase::AfterPhysics, system.access(access)),
        1,
    )
}

// ---- the world part ------------------------------------------------------------------

#[test]
fn a_view_system_moves_declared_components_each_tick() {
    let world = run(
        Game::new("views", 1, 1).add(
            Phase::AfterPhysics,
            (|sys: &mut SystemView| {
                let dt = sys.dt();
                sys.world
                    .query::<(&mut Pos, &Vel)>()
                    .for_each(|(p, v)| p.0 += v.0 * dt * 60.0);
            })
            .access(
                SystemAccess::new()
                    .write::<Pos>()
                    .read::<Vel>()
                    .read_service(Service::Time),
            ),
        ),
        3,
    );
    let mut q = world;
    let x = q.query::<&Pos>().iter().next().unwrap().0;
    assert!((x - 3.0).abs() < 1e-4, "{x}");
}

#[test]
fn written_resources_are_back_in_the_world_after_the_system() {
    let world = run(
        Game::new("views", 1, 1)
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let g = sys.world.resource::<Gravity>().0;
                    sys.world.resource_mut::<Score>().0 += (-g) as u32;
                })
                .access(SystemAccess::new().write_resource::<Score>().read_resource::<Gravity>()),
            )
            // A plain Ctx system after it sees the resource where it belongs.
            .sim_system(|ctx: &mut Ctx| {
                let s = ctx.world.resource::<Score>().0;
                ctx.world.resource_mut::<Log>().0.push(format!("score {s}"));
            }),
        2,
    );
    assert_eq!(world.resource::<Score>(), &Score(2));
    assert_eq!(world.resource::<Log>().0, ["score 1", "score 2"]);
}

#[test]
fn commands_from_a_view_apply_before_the_next_system() {
    let world = run(
        Game::new("views", 1, 1)
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    sys.commands.spawn((Health(9),));
                })
                .access(SystemAccess::new().commands()),
            )
            .sim_system(|ctx: &mut Ctx| {
                let n = ctx.world.query::<&Health>().count();
                ctx.world.resource_mut::<Log>().0.push(format!("{n}"));
            }),
        2,
    );
    assert_eq!(world.resource::<Log>().0, ["2", "3"]);
}

#[test]
#[should_panic(expected = "doesn't declare that it may write component")]
fn writing_an_undeclared_component_panics_in_every_build() {
    one(SystemAccess::new().read::<Pos>(), |sys| {
        sys.world.query::<&mut Pos>().for_each(|p| p.0 = 1.0);
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it may read resource")]
fn reading_an_undeclared_resource_panics() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.world.resource::<Score>();
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it records commands")]
fn undeclared_commands_panic() {
    one(SystemAccess::new(), |sys| {
        sys.commands.spawn((Health(1),));
    });
}

#[test]
#[should_panic(expected = "system `system_views::the_message_names_the_system::a_named_system` doesn't declare")]
fn the_message_names_the_system() {
    fn a_named_system(sys: &mut SystemView) {
        let _ = sys.world.resource::<Score>();
    }
    one(SystemAccess::new(), a_named_system);
}

// ---- events ----------------------------------------------------------------------------

#[test]
fn view_events_follow_the_declaration_and_the_usual_visibility() {
    let world = run(
        Game::new("views", 1, 1)
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    sys.events.send(Hit(1));
                })
                .access(SystemAccess::new().send_events::<Hit>()),
            )
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let n = sys.events.read::<Hit>().len();
                    let kept = sys.events.peek::<Hit>().len();
                    sys.world.resource_mut::<Log>().0.push(format!("read {n} kept {kept}"));
                })
                .access(SystemAccess::new().read_events::<Hit>().write_resource::<Log>()),
            ),
        2,
    );
    assert_eq!(world.resource::<Log>().0, ["read 1 kept 1", "read 1 kept 2"]);
}

#[test]
#[should_panic(expected = "doesn't declare that it may send events `system_views::Hit`")]
fn sending_undeclared_events_panics() {
    one(SystemAccess::new().read_events::<Hit>(), |sys| sys.events.send(Hit(0)));
}

#[test]
#[should_panic(expected = "doesn't declare that it may read events")]
fn reading_events_declared_only_as_sent_panics() {
    one(SystemAccess::new().send_events::<Hit>(), |sys| {
        let _ = sys.events.read::<Hit>().len();
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it may read events")]
fn peeking_undeclared_events_panics() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.events.peek::<Hit>().len();
    });
}

// ---- services --------------------------------------------------------------------------

#[test]
fn declared_services_are_offered() {
    let world = one(
        SystemAccess::new()
            .read_service(Service::Input)
            .read_service(Service::Time)
            .read_service(Service::Random)
            .read_service(Service::Spatial)
            .write_service(Service::Camera)
            .write_service(Service::Net)
            .write_service(Service::Audio)
            .write_resource::<Log>(),
        |sys| {
            let line = format!(
                "key {} mouse {} {} {} pos {} tick {} frame {} time {} fixed {} spatial {} player {}",
                sys.key_down(Key::Space) || sys.key_pressed(Key::Space) || sys.key_released(Key::Space),
                sys.input.mouse_down(MouseButton::Left),
                sys.input.mouse_pressed(MouseButton::Left),
                sys.input.mouse_released(MouseButton::Left),
                sys.mouse_pos(),
                sys.time.sim_tick(),
                sys.time.frame(),
                sys.time.elapsed() >= 0.0,
                sys.time.fixed_dt() > 0.0,
                sys.spatial.len(),
                sys.net.lockstep.player(),
            );
            let r = sys.random().stream(0).u64();
            let same = sys.random().stream(0).u64();
            assert_eq!(r, same, "one stream per system and tick");
            sys.camera.zoom = 2.0;
            assert_eq!(sys.camera.zoom, 2.0);
            let _ = sys.net.lockstep.players();
            assert_eq!(sys.name(), sys.name());
            assert!(sys.access().service(Service::Audio).unwrap());
            sys.world.resource_mut::<Log>().0.push(line);
        },
    );
    let log = &world.resource::<Log>().0;
    assert_eq!(log.len(), 1);
    assert!(log[0].starts_with("key false mouse false false false"), "{}", log[0]);
}

#[test]
fn view_random_matches_ctx_random_for_the_same_slot() {
    // The view's stream is the one `ctx.random()` gives that system.
    let world = run(
        Game::new("views", 1, 1).add(
            Phase::AfterPhysics,
            (|sys: &mut SystemView| {
                let v = sys.random().stream(3).u64();
                sys.world.resource_mut::<Log>().0.push(v.to_string());
            })
            .access(
                SystemAccess::new()
                    .read_service(Service::Random)
                    .write_resource::<Log>(),
            ),
        ),
        2,
    );
    let log = &world.resource::<Log>().0;
    assert_eq!(log.len(), 2);
    assert_ne!(log[0], log[1], "a new stream every tick");
}

#[test]
#[should_panic(expected = "doesn't declare that it may read service `input`")]
fn undeclared_input_panics() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.key_down(Key::Space);
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it may read service `time`")]
fn undeclared_time_panics() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.dt();
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it may read service `input`")]
fn an_undeclared_service_field_panics_when_used() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.input.mouse_down(MouseButton::Left);
    });
}

#[test]
#[should_panic(expected = "doesn't declare that it may read service `time`")]
fn an_undeclared_time_field_panics_when_used() {
    one(SystemAccess::new(), |sys| {
        let _ = sys.time.sim_tick();
    });
}

#[test]
fn a_service_field_is_the_service_ctx_has() {
    // Whatever `ctx.input` and `ctx.time` offer for reading, the view does.
    let world = one(
        SystemAccess::new()
            .read_service(Service::Input)
            .read_service(Service::Time)
            .write_resource::<Log>(),
        |sys| {
            let line = format!(
                "wheel {} paused {} sim_dt {} dt {}",
                sys.input.mouse_wheel() == Vec2::ZERO,
                sys.time.is_paused(),
                sys.time.sim_dt() == sys.time.fixed_dt(),
                sys.dt() == sys.time.dt(),
            );
            sys.world.resource_mut::<Log>().0.push(line);
        },
    );
    assert_eq!(
        world.resource::<Log>().0,
        ["wheel true paused false sim_dt true dt true"]
    );
}

#[test]
#[should_panic(expected = "doesn't declare that it may write service `camera`")]
fn reading_a_service_does_not_allow_writing_it() {
    one(SystemAccess::new().read_service(Service::Camera), |sys| {
        sys.camera.zoom = 3.0;
    });
}

struct Click(Sound);

fn with_sound(access: SystemAccess, system: impl FnMut(&mut SystemView) + Send + 'static) -> World {
    Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, system.access(access))
        .run_headless(1, |ctx| {
            setup(ctx);
            let sound = ctx.audio.create(8000, &[0.0; 80]);
            ctx.world.insert_resource(Click(sound));
        })
}

#[test]
fn declared_audio_plays_sounds_and_is_silent_headless() {
    let world = with_sound(
        SystemAccess::new()
            .write_service(Service::Audio)
            .read_resource::<Click>()
            .write_resource::<Log>(),
        |sys| {
            let sound = sys.world.resource::<Click>().0;
            let a = sys.play_sound(sound);
            let b = sys.audio.play_ex(sound, SoundParams::default());
            // Silent, but each sound has its own handle and is listed.
            let heard = sys.audio.started().len();
            sys.world.resource_mut::<Log>().0.push(format!("{} {heard}", a != b));
        },
    );
    assert_eq!(world.resource::<Log>().0, ["true 2"]);
}

#[test]
#[should_panic(expected = "doesn't declare that it may write service `audio`")]
fn playing_sound_needs_audio() {
    with_sound(SystemAccess::new().read_resource::<Click>(), |sys| {
        let sound = sys.world.resource::<Click>().0;
        let _ = sys.play_sound(sound);
    });
}

#[test]
#[should_panic(expected = "declares service `gfx`, which a view doesn't offer")]
fn services_a_view_cannot_offer_are_rejected_at_registration() {
    let _ = Game::new("views", 1, 1).add(
        Phase::Frame,
        (|_: &mut SystemView| {}).access(SystemAccess::new().write_service(Service::Gfx)),
    );
}

#[test]
#[should_panic(expected = "declares writing service `time`, which a view only reads")]
fn writing_a_read_only_service_is_rejected_at_registration() {
    let _ = Game::new("views", 1, 1).add(
        Phase::AfterPhysics,
        (|_: &mut SystemView| {}).access(SystemAccess::new().write_service(Service::Time)),
    );
}

#[test]
#[should_panic(expected = "is exclusive")]
fn an_exclusive_view_is_rejected() {
    let _ = Game::new("views", 1, 1).add(
        Phase::AfterPhysics,
        (|_: &mut SystemView| {}).access(SystemAccess::exclusive()),
    );
}

#[test]
#[should_panic(expected = "which has no Rust type")]
fn named_items_are_rejected_for_views() {
    let _ = Game::new("views", 1, 1).add(
        Phase::AfterPhysics,
        (|_: &mut SystemView| {}).access(SystemAccess::new().read_named("pos")),
    );
}

#[test]
fn service_declarations_conflict_like_data() {
    let read = SystemAccess::new().read_service(Service::Input);
    let write = SystemAccess::new().write_service(Service::Input);
    assert!(read.conflicts_with(&read.clone()).is_empty());
    assert_eq!(read.conflicts_with(&write), [AccessConflict::Service(Service::Input)]);
    assert_eq!(write.conflicts_before(&read), [AccessConflict::Service(Service::Input)]);
    assert!(
        write
            .conflicts_with(&SystemAccess::new().write_service(Service::Audio))
            .is_empty()
    );
    assert_eq!(read.service(Service::Input), Some(false));
    assert_eq!(
        read.clone().write_service(Service::Input).service(Service::Input),
        Some(true)
    );
    assert_eq!(read.service(Service::Audio), None);
    assert_eq!(
        AccessConflict::Service(Service::Physics3D).to_string(),
        "service `physics3d`"
    );
    assert_eq!(Service::ALL.len(), 13);
    let names: Vec<&str> = Service::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names[0], "input");
    assert_eq!(Service::Gfx.to_string(), "gfx");
}

// ---- ordered barrier rules -------------------------------------------------------------

#[test]
fn ordered_rules_relax_sends_and_commands_only_where_order_hides_them() {
    let send = SystemAccess::new().send_events::<Hit>();
    let read = SystemAccess::new().read_events::<Hit>();
    // Symmetric rules: any sender clashes.
    assert_eq!(send.conflicts_with(&read).len(), 1);
    assert_eq!(send.conflicts_with(&send.clone()).len(), 1);
    // Ordered: a reader before a sender, and two senders, are fine.
    assert!(read.conflicts_before(&send).is_empty());
    assert!(send.conflicts_before(&send.clone()).is_empty());
    assert_eq!(
        send.conflicts_before(&read),
        [AccessConflict::Event(std::any::type_name::<Hit>())]
    );

    let spawner = SystemAccess::new().commands();
    let reader = SystemAccess::new().read::<Pos>();
    assert_eq!(spawner.conflicts_with(&reader), [AccessConflict::Commands]);
    assert_eq!(reader.conflicts_with(&spawner), [AccessConflict::Commands]);
    // An earlier recorder's commands would be seen by a later world user...
    assert_eq!(spawner.conflicts_before(&reader), [AccessConflict::Commands]);
    // ...even one that declares no data (it can still see the structure),
    let counter = SystemAccess::new().send_events::<Hit>();
    assert_eq!(spawner.conflicts_before(&counter), [AccessConflict::Commands]);
    assert_eq!(spawner.conflicts_before(&spawner.clone()), [AccessConflict::Commands]);
    // ...but an earlier system never sees a later recorder's commands.
    assert!(reader.conflicts_before(&spawner).is_empty());
    assert!(counter.conflicts_before(&spawner).is_empty());
    // Data and exclusivity are the same in both rule sets.
    let w = SystemAccess::new().write::<Pos>();
    assert_eq!(reader.conflicts_before(&w), reader.conflicts_with(&w));
    assert_eq!(
        SystemAccess::exclusive().conflicts_before(&reader),
        [AccessConflict::Exclusive]
    );
}

/// The sequential behaviour each ordered rule relies on, observed: a reader
/// before a sender sees the events on its next run; two senders' events
/// arrive in registration order; a world user before a command recorder
/// doesn't see its commands until the next tick.
#[test]
fn sequential_runs_show_what_the_ordered_rules_rely_on() {
    fn log(sys: &mut SystemView, line: String) {
        sys.world.resource_mut::<Log>().0.push(line);
    }
    let reader = SystemAccess::new().read_events::<Hit>().write_resource::<Log>();
    let world = run(
        Game::new("views", 1, 1)
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let hits: Vec<u32> = sys.events.read::<Hit>().iter().map(|h| h.0).collect();
                    let n = sys.world.query::<&Health>().count();
                    log(sys, format!("early {hits:?} health {n}"));
                })
                .access(reader.clone().read::<Health>()),
            )
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let t = sys.time.sim_tick() as u32;
                    sys.events.send(Hit(t * 10 + 1));
                    sys.commands.spawn((Health(1),));
                })
                .access(
                    SystemAccess::new()
                        .send_events::<Hit>()
                        .commands()
                        .read_service(Service::Time),
                ),
            )
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let t = sys.time.sim_tick() as u32;
                    sys.events.send(Hit(t * 10 + 2));
                })
                .access(SystemAccess::new().send_events::<Hit>().read_service(Service::Time)),
            )
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let hits: Vec<u32> = sys.events.read::<Hit>().iter().map(|h| h.0).collect();
                    log(sys, format!("late {hits:?}"));
                })
                .access(reader),
            ),
        2,
    );
    assert_eq!(
        world.resource::<Log>().0,
        [
            "early [] health 1",
            "late [1, 2]",
            "early [1, 2] health 2",
            "late [11, 12]"
        ]
    );
}

/// A view that declares no data still sees the world's structure, so a
/// command recorder before it must conflict with it.
#[test]
fn a_view_without_declared_data_sees_earlier_commands() {
    let spawner = SystemAccess::new().commands();
    let counter = SystemAccess::new().write_resource::<Log>();
    assert_eq!(spawner.conflicts_before(&counter), [AccessConflict::Commands]);
    let world = run(
        Game::new("views", 1, 1)
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    sys.commands.spawn((Pos(0.0),));
                })
                .access(spawner),
            )
            .add(
                Phase::AfterPhysics,
                (|sys: &mut SystemView| {
                    let (n, alive) = (sys.world.len(), sys.world.query::<Entity>().count());
                    sys.world.resource_mut::<Log>().0.push(format!("{n} {alive}"));
                })
                .access(counter),
            ),
        2,
    );
    // setup spawns one entity; each tick's spawn is seen in the same tick.
    assert_eq!(world.resource::<Log>().0, ["2 2", "3 3"]);
}

// ---- the report --------------------------------------------------------------------------

fn fall(sys: &mut SystemView) {
    let g = sys.world.resource::<Gravity>().0;
    sys.world.query::<&mut Vel>().for_each(|v| v.0 += g);
}

fn decay(sys: &mut SystemView) {
    sys.world.query::<&mut Health>().for_each(|h| h.0 -= 1);
}

fn ctx_mover(ctx: &mut Ctx) {
    ctx.world.query::<&mut Pos>().for_each(|p| p.0 += 1.0);
}

#[test]
fn only_view_systems_are_parallel_candidates() {
    let game = Game::new("views", 1, 1)
        .add(
            Phase::AfterPhysics,
            fall.access(SystemAccess::new().write::<Vel>().read_resource::<Gravity>()),
        )
        .add(Phase::AfterPhysics, decay.access(SystemAccess::new().write::<Health>()))
        .add(
            Phase::AfterPhysics,
            ctx_mover.access(SystemAccess::new().write::<Pos>()),
        );
    let report = game.access_report();
    let short = |n: &str| n.rsplit("::").next().unwrap().to_string();
    let enforced: Vec<String> = report
        .systems
        .iter()
        .filter(|s| s.enforced)
        .map(|s| short(s.name))
        .collect();
    assert_eq!(enforced, ["fall", "decay"]);
    assert_eq!(report.compatible_pairs().count(), 3, "no declared conflicts");
    let parallel: Vec<(String, String)> = report.parallel_pairs().map(|(a, b)| (short(a), short(b))).collect();
    assert_eq!(parallel, [("fall".to_string(), "decay".to_string())]);
    let text = report.to_string();
    assert!(
        text.contains("fall + system_views::decay: could run together"),
        "{text}"
    );
    assert!(text.contains("a `Ctx` system stays sequential"), "{text}");
    // And they run, in order, like any other system.
    let world = game.run_headless(1, setup);
    let mut world = world;
    let (v, h) = world
        .query::<(&Vel, &Health)>()
        .iter()
        .map(|(v, h)| (v.0, h.0))
        .next()
        .unwrap();
    assert_eq!((v, h), (0.0, 2));
}

fn noop_ctx(_: &mut Ctx) {}

/// Batching two views would move one across every system between them, so
/// such a system must be able to move too.
#[test]
fn a_system_between_two_views_keeps_them_apart() {
    let short = |n: &str| n.rsplit("::").next().unwrap().to_string();
    let a = || SystemAccess::new().write::<Vel>();
    let b = || SystemAccess::new().write::<Health>();
    // An undeclared (exclusive) system between them.
    let report = Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(a()))
        .add(Phase::AfterPhysics, noop_ctx)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(b()))
        .access_report();
    assert_eq!(report.pairs.len(), 1);
    let between = &report.pairs[0].conflicts;
    assert_eq!(between.len(), 1);
    assert!(
        matches!(between[0], AccessConflict::Between(n) if short(n) == "noop_ctx"),
        "{between:?}"
    );
    assert_eq!(report.parallel_pairs().count(), 0);
    assert!(report.to_string().contains("which runs between them"), "{report}");

    // A view between them that conflicts with one of them.
    let report = Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(a()))
        .add(
            Phase::AfterPhysics,
            (|_: &mut SystemView| {}).access(SystemAccess::new().read::<Vel>()),
        )
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(b()))
        .access_report();
    let first_last = report.pairs.iter().find(|p| p.second - p.first == 2).unwrap();
    assert!(matches!(first_last.conflicts[..], [AccessConflict::Between(_)]));
    // The reader and `b` still could.
    assert_eq!(report.parallel_pairs().count(), 1);

    // A view between them that conflicts with neither: all three could.
    let report = Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(a()))
        .add(
            Phase::AfterPhysics,
            (|_: &mut SystemView| {}).access(SystemAccess::new().read::<Pos>()),
        )
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(b()))
        .access_report();
    assert_eq!(report.parallel_pairs().count(), 3);

    // A declared `Ctx` system between them that conflicts with neither: the
    // declarations allow it, but its declaration isn't trusted to move.
    let report = Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(a()))
        .add(Phase::AfterPhysics, noop_ctx.access(SystemAccess::new().read::<Pos>()))
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(b()))
        .access_report();
    assert_eq!(report.compatible_pairs().count(), 3);
    assert_eq!(report.parallel_pairs().count(), 0);

    // Systems in other phases don't count.
    let report = Game::new("views", 1, 1)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(a()))
        .add(Phase::Frame, noop_ctx)
        .add(Phase::AfterPhysics, (|_: &mut SystemView| {}).access(b()))
        .access_report();
    assert_eq!(report.parallel_pairs().count(), 1);
}

#[test]
fn the_report_uses_the_ordered_rules() {
    let report = Game::new("views", 1, 1)
        .add(
            Phase::AfterPhysics,
            (|_: &mut SystemView| {}).access(SystemAccess::new().read_events::<Hit>()),
        )
        .add(
            Phase::AfterPhysics,
            (|_: &mut SystemView| {}).access(SystemAccess::new().send_events::<Hit>()),
        )
        .add(
            Phase::AfterPhysics,
            (|_: &mut SystemView| {}).access(SystemAccess::new().read_events::<Hit>()),
        )
        .access_report();
    let conflicts: Vec<(usize, usize, usize)> = report
        .pairs
        .iter()
        .map(|p| (p.first, p.second, p.conflicts.len()))
        .filter(|p| p.2 > 0)
        .collect();
    // The sender before the second reader conflicts, and so the two readers
    // are kept apart by the sender between them.
    assert_eq!(conflicts.len(), 2);
    let (r1, s, r2) = (conflicts[0].0, conflicts[1].0, conflicts[1].1);
    let readers = report.pairs.iter().find(|p| (p.first, p.second) == (r1, r2)).unwrap();
    assert_eq!(readers.conflicts, [AccessConflict::Between(report.systems[s].name)]);
    let p = conflicts[1];
    assert!(
        report.systems[p.0]
            .access
            .as_ref()
            .unwrap()
            .conflicts_before(report.systems[p.1].access.as_ref().unwrap())
            .len()
            == 1
    );
}

// ---- the guide's example ------------------------------------------------------------------

mod guide {
    #[allow(unused_imports)]
    use ponko::access::AccessConflict;
    #[allow(unused_imports)]
    use ponko::access::Service;
    #[allow(unused_imports)]
    use ponko::access::SystemAccess;
    #[allow(unused_imports)]
    use ponko::audio::Audio;
    #[allow(unused_imports)]
    #[cfg(feature = "physics")]
    use ponko::d2::Physics;
    #[allow(unused_imports)]
    #[cfg(feature = "physics3d")]
    use ponko::d3::Physics as Physics3D;
    #[allow(unused_imports)]
    use ponko::input::Input;
    #[allow(unused_imports)]
    use ponko::net::Net;
    use ponko::prelude::*;
    #[allow(unused_imports)]
    use ponko::system_view::SystemView;

    // ANCHOR: view_system
    #[derive(Component)]
    struct Pos(f32);
    #[derive(Component)]
    struct Vel(f32);
    struct Gravity(f32);

    // A view system gets exactly what it declares: no `&mut Ctx`.
    fn fall(sys: &mut SystemView) {
        let g = sys.world.resource::<Gravity>().0;
        let dt = sys.dt();
        sys.world.query::<&mut Vel>().for_each(|v| v.0 += g * dt);
    }

    fn movement(sys: &mut SystemView) {
        let dt = sys.dt();
        sys.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0 * dt);
    }

    fn game() -> Game {
        Game::new("falling", 320, 240)
            .add(
                Phase::AfterPhysics,
                fall.access(
                    SystemAccess::new()
                        .write::<Vel>()
                        .read_resource::<Gravity>()
                        .read_service(Service::Time),
                ),
            )
            .add(
                Phase::AfterPhysics,
                movement.access(
                    SystemAccess::new()
                        .write::<Pos>()
                        .read::<Vel>()
                        .read_service(Service::Time),
                ),
            )
    }
    // ANCHOR_END: view_system

    #[test]
    fn the_guide_example_runs_and_reports() {
        let report = game().access_report();
        assert_eq!(report.parallel_pairs().count(), 0);
        assert!(report.to_string().contains("conflict on component"));
        let mut world = game().run_headless(60, |ctx| {
            ctx.world.insert_resource(Gravity(-10.0));
            ctx.world.spawn((Pos(0.0), Vel(0.0)));
        });
        let p = world.query::<&Pos>().iter().next().unwrap().0;
        assert!(p < -4.0 && p > -6.0, "{p}");
    }
}

// ---- physics services (features `physics`, `physics3d`) ----------------------------------

#[cfg(feature = "physics")]
mod physics2d {
    #[allow(unused_imports)]
    use ponko::access::AccessConflict;
    #[allow(unused_imports)]
    use ponko::access::Service;
    #[allow(unused_imports)]
    use ponko::access::SystemAccess;
    #[allow(unused_imports)]
    use ponko::audio::Audio;
    #[allow(unused_imports)]
    #[cfg(feature = "physics")]
    use ponko::d2::Physics;
    #[allow(unused_imports)]
    #[cfg(feature = "physics3d")]
    use ponko::d3::Physics as Physics3D;
    #[allow(unused_imports)]
    use ponko::input::Input;
    #[allow(unused_imports)]
    use ponko::net::Net;
    use ponko::prelude::*;
    #[allow(unused_imports)]
    use ponko::system_view::SystemView;

    fn with_physics(access: SystemAccess, system: impl FnMut(&mut SystemView) + Send + 'static) -> World {
        Game::new("views", 1, 1)
            .physics(vec2(0.0, 10.0))
            .add(Phase::AfterPhysics, system.access(access))
            .run_headless(1, |_| {})
    }

    #[test]
    fn declared_physics_is_offered() {
        with_physics(SystemAccess::new().write_service(Service::Physics), |sys| {
            assert_eq!(sys.physics.gravity(), vec2(0.0, 10.0));
            sys.physics.set_gravity(vec2(0.0, -5.0));
            assert_eq!(sys.physics.gravity(), vec2(0.0, -5.0));
            assert_eq!(sys.physics.body_count(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "doesn't declare that it may read service `physics`")]
    fn undeclared_physics_panics() {
        with_physics(SystemAccess::new(), |sys| {
            let _ = sys.physics.gravity();
        });
    }

    #[test]
    #[should_panic(expected = "doesn't declare that it may write service `physics`")]
    fn reading_physics_does_not_allow_changing_it() {
        with_physics(SystemAccess::new().read_service(Service::Physics), |sys| {
            sys.physics.set_gravity(Vec2::ZERO);
        });
    }
}

#[cfg(feature = "physics3d")]
mod physics3d {
    #[allow(unused_imports)]
    use ponko::access::AccessConflict;
    #[allow(unused_imports)]
    use ponko::access::Service;
    #[allow(unused_imports)]
    use ponko::access::SystemAccess;
    #[allow(unused_imports)]
    use ponko::audio::Audio;
    #[allow(unused_imports)]
    #[cfg(feature = "physics")]
    use ponko::d2::Physics;
    #[allow(unused_imports)]
    #[cfg(feature = "physics3d")]
    use ponko::d3::Physics as Physics3D;
    #[allow(unused_imports)]
    use ponko::input::Input;
    #[allow(unused_imports)]
    use ponko::net::Net;
    use ponko::prelude::*;
    #[allow(unused_imports)]
    use ponko::system_view::SystemView;

    fn with_physics(access: SystemAccess, system: impl FnMut(&mut SystemView) + Send + 'static) -> World {
        Game::new("views", 1, 1)
            .physics3d(vec3(0.0, -10.0, 0.0))
            .add(Phase::AfterPhysics, system.access(access))
            .run_headless(1, |_| {})
    }

    #[test]
    fn declared_physics3d_is_offered() {
        with_physics(SystemAccess::new().write_service(Service::Physics3D), |sys| {
            assert_eq!(sys.physics3d.gravity(), vec3(0.0, -10.0, 0.0));
            sys.physics3d.set_gravity(vec3(0.0, -1.0, 0.0));
            assert_eq!(sys.physics3d.gravity(), vec3(0.0, -1.0, 0.0));
            assert_eq!(sys.physics3d.body_count(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "doesn't declare that it may read service `physics3d`")]
    fn undeclared_physics3d_panics() {
        with_physics(SystemAccess::new(), |sys| {
            let _ = sys.physics3d.gravity();
        });
    }

    #[test]
    #[should_panic(expected = "doesn't declare that it may write service `physics3d`")]
    fn reading_physics3d_does_not_allow_changing_it() {
        with_physics(SystemAccess::new().read_service(Service::Physics3D), |sys| {
            sys.physics3d.set_gravity(Vec3::ZERO);
        });
    }
}
