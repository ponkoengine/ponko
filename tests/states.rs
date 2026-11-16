//! Game states and run conditions (`ponko::state`): when changes apply,
//! what on-enter and on-exit see, the stack, conditions with the parallel
//! scheduler, saves and lockstep.
use std::thread;

use ponko::access::SystemAccess;
use ponko::net::Net;
use ponko::prelude::*;
use ponko::schedule::Alone;
use ponko::state::{Condition, Next, State};
use ponko::system_view::SystemView;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Screen {
    Title,
    Loading,
    Playing,
    Paused,
    Options,
}

/// What happened, in order: `("enter", state, tick)`, `("exit", ...)`,
/// `("run", ...)`.
#[derive(Default)]
struct Log(Vec<(&'static str, Screen, u64)>);

fn log(ctx: &mut Ctx, what: &'static str) {
    let entry = (what, *ctx.state::<Screen>(), ctx.time.sim_tick());
    if ctx.world.get_resource::<Log>().is_none() {
        ctx.world.insert_resource(Log::default());
    }
    ctx.world.resource_mut::<Log>().0.push(entry);
}

/// A game that logs every enter and exit of every screen.
fn logged(initial: Screen) -> Game {
    let mut game = Game::new("states", 1, 1).state(initial);
    for s in [
        Screen::Title,
        Screen::Loading,
        Screen::Playing,
        Screen::Paused,
        Screen::Options,
    ] {
        game = game
            .on_enter(s, |ctx: &mut Ctx| log(ctx, "enter"))
            .on_exit(s, |ctx: &mut Ctx| log(ctx, "exit"));
    }
    game
}

/// A sim system that asks for `change` on tick `at`.
fn at_tick(at: u64, change: fn(&mut Ctx)) -> impl FnMut(&mut Ctx) {
    move |ctx: &mut Ctx| {
        if ctx.time.sim_tick() == at {
            change(ctx);
        }
    }
}

fn entries(world: &World) -> Vec<(&'static str, Screen, u64)> {
    world.get_resource::<Log>().map(|l| l.0.clone()).unwrap_or_default()
}

#[test]
fn the_first_state_is_entered_at_the_start_of_the_first_tick() {
    let world = logged(Screen::Title).run_headless(0, |_| {});
    assert!(entries(&world).is_empty(), "no tick, no on-enter");
    let world = logged(Screen::Title).run_headless(2, |_| {});
    assert_eq!(entries(&world), [("enter", Screen::Title, 0)]);
}

#[test]
fn a_change_asked_for_in_a_tick_applies_at_its_end_for_every_system() {
    #[derive(Default)]
    struct Seen(Vec<(u64, Screen, Screen)>);
    let world = logged(Screen::Title)
        .add(Phase::Input, |ctx: &mut Ctx| {
            let s = *ctx.state::<Screen>();
            ctx.world.resource_mut::<Seen>().0.push((ctx.time.sim_tick(), s, s));
        })
        .add(Phase::AfterPhysics, at_tick(2, |ctx| ctx.set_state(Screen::Playing)))
        .add(Phase::SimLast, |ctx: &mut Ctx| {
            let s = *ctx.state::<Screen>();
            ctx.world.resource_mut::<Seen>().0.last_mut().unwrap().2 = s;
        })
        .setup(|ctx| ctx.world.insert_resource(Seen::default()))
        .run_headless(5);
    let seen = &world.resource::<Seen>().0;
    // Asked for in tick 2: all of tick 2 stays on the title, all of tick 3 plays.
    assert_eq!(seen[2], (2, Screen::Title, Screen::Title));
    assert_eq!(seen[3], (3, Screen::Playing, Screen::Playing));
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Title, 0),
            ("exit", Screen::Title, 2),
            ("enter", Screen::Playing, 2)
        ]
    );
}

#[test]
fn a_change_asked_for_in_setup_applies_after_the_first_state_is_entered() {
    let world = logged(Screen::Title)
        .setup(|ctx| ctx.set_state(Screen::Loading))
        .run_headless(1);
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Title, 0),
            ("exit", Screen::Title, 0),
            ("enter", Screen::Loading, 0)
        ]
    );
}

#[test]
fn the_last_request_of_a_tick_wins() {
    let world = logged(Screen::Title)
        .sim_system(at_tick(0, |ctx| {
            ctx.set_state(Screen::Options);
            ctx.push_state(Screen::Paused);
            ctx.set_state(Screen::Playing);
        }))
        .run_headless(2, |_| {});
    assert_eq!(world.resource::<State<Screen>>().get(), &Screen::Playing);
    assert_eq!(entries(&world).last(), Some(&("enter", Screen::Playing, 0)));
}

#[test]
fn a_pushed_state_covers_the_one_below_without_leaving_it() {
    let world = logged(Screen::Playing)
        .sim_system(at_tick(1, |ctx| ctx.push_state(Screen::Paused)))
        .sim_system(at_tick(2, |ctx| ctx.push_state(Screen::Options)))
        .sim_system(at_tick(3, |ctx| ctx.pop_state::<Screen>()))
        .sim_system(at_tick(4, |ctx| ctx.pop_state::<Screen>()))
        .sim_system(at_tick(5, |ctx| {
            assert_eq!(ctx.world.resource::<State<Screen>>().below(), [] as [Screen; 0]);
        }))
        .sim_system(at_tick(2, |ctx| {
            let state = ctx.world.resource::<State<Screen>>();
            assert_eq!((state.get(), state.below()), (&Screen::Paused, &[Screen::Playing][..]));
            assert!(state.in_stack(&Screen::Playing) && !state.is(&Screen::Playing));
        }))
        .run_headless(6, |_| {});
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Playing, 0),
            ("enter", Screen::Paused, 1),
            ("enter", Screen::Options, 2),
            ("exit", Screen::Options, 3),
            ("exit", Screen::Paused, 4),
        ],
        "popping goes back without entering the state below again"
    );
    assert_eq!(world.resource::<State<Screen>>().get(), &Screen::Playing);
}

#[test]
fn setting_a_state_leaves_every_state_on_the_stack_top_first() {
    let world = logged(Screen::Playing)
        .sim_system(at_tick(0, |ctx| ctx.push_state(Screen::Paused)))
        .sim_system(at_tick(1, |ctx| ctx.push_state(Screen::Options)))
        .sim_system(at_tick(2, |ctx| ctx.set_state(Screen::Title)))
        .run_headless(4, |_| {});
    assert_eq!(
        entries(&world)[3..],
        [
            ("exit", Screen::Options, 2),
            ("exit", Screen::Paused, 2),
            ("exit", Screen::Playing, 2),
            ("enter", Screen::Title, 2),
        ]
    );
    assert!(world.resource::<State<Screen>>().below().is_empty());
}

#[test]
fn setting_the_current_state_leaves_and_enters_it_again() {
    let world = logged(Screen::Playing)
        .sim_system(at_tick(1, |ctx| ctx.set_state(Screen::Playing)))
        .run_headless(3, |_| {});
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Playing, 0),
            ("exit", Screen::Playing, 1),
            ("enter", Screen::Playing, 1)
        ]
    );
}

#[test]
fn ticks_count_the_ticks_on_top_and_a_pushed_state_stops_the_count_below() {
    let world = Game::new("t", 1, 1)
        .state(Screen::Playing)
        .sim_system(|ctx| {
            let seen = (*ctx.state::<Screen>(), ctx.world.resource::<State<Screen>>().ticks());
            ctx.world.resource_mut::<Vec<(Screen, u64)>>().push(seen);
        })
        .sim_system(at_tick(2, |ctx| ctx.push_state(Screen::Paused)))
        .sim_system(at_tick(4, |ctx| ctx.pop_state::<Screen>()))
        .run_headless(7, |ctx| ctx.world.insert_resource(Vec::<(Screen, u64)>::new()));
    assert_eq!(
        world.resource::<Vec<(Screen, u64)>>(),
        &[
            (Screen::Playing, 1),
            (Screen::Playing, 2),
            (Screen::Playing, 3),
            (Screen::Paused, 1),
            (Screen::Paused, 2),
            (Screen::Playing, 4),
            (Screen::Playing, 5),
        ],
        "the pause didn't count for the state below"
    );
}

#[test]
fn every_set_is_a_change_even_to_the_current_state_and_restarts_its_tick_count() {
    // The rule: a set always leaves and enters, so a timer kept in the
    // state by setting it every tick would run on-exit and on-enter every
    // tick. Timed states read `ticks` instead.
    let world = logged(Screen::Loading)
        .sim_system(at_tick(3, |ctx| ctx.set_state(Screen::Loading)))
        .run_headless(6, |_| {});
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Loading, 0),
            ("exit", Screen::Loading, 3),
            ("enter", Screen::Loading, 3)
        ]
    );
    assert_eq!(world.resource::<State<Screen>>().ticks(), 2, "ticks 4 and 5");
}

#[test]
fn a_timed_state_changes_once_after_its_time_without_reentering() {
    let world = logged(Screen::Loading)
        .fixed_hz(10)
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| {
                if ctx.seconds_in_state::<Screen>() >= 0.3 {
                    ctx.set_state(Screen::Playing);
                }
            })
            .run_if(in_state(Screen::Loading)),
        )
        .run_headless(8, |_| {});
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Loading, 0),
            ("exit", Screen::Loading, 2),
            ("enter", Screen::Playing, 2)
        ]
    );
}

#[test]
fn popping_with_nothing_below_is_dropped() {
    let world = logged(Screen::Title)
        .sim_system(at_tick(0, |ctx| ctx.pop_state::<Screen>()))
        .run_headless(3, |_| {});
    assert_eq!(entries(&world), [("enter", Screen::Title, 0)]);
    let state = world.resource::<State<Screen>>();
    assert_eq!((state.get(), state.next()), (&Screen::Title, None));
}

#[test]
fn on_enter_work_runs_in_order_with_its_commands_applied_after_each() {
    #[derive(Component)]
    struct Coin;
    let world = Game::new("t", 1, 1)
        .state(Screen::Playing)
        .on_enter(Screen::Playing, |ctx: &mut Ctx| {
            ctx.commands.spawn((Coin,));
        })
        .on_enter(Screen::Playing, |ctx: &mut Ctx| {
            let coins = ctx.world.query::<&Coin>().iter().count();
            ctx.world.insert_resource(coins);
        })
        .run_headless(1, |_| {});
    assert_eq!(*world.resource::<usize>(), 1);
}

#[test]
fn a_change_asked_for_while_entering_waits_for_the_next_change_point() {
    // Loading has nothing to load: entered at the start of tick 0, it moves
    // on at its end, so tick 0 runs in Loading.
    let world = logged(Screen::Loading)
        .on_enter(Screen::Loading, |ctx: &mut Ctx| ctx.set_state(Screen::Playing))
        .run_headless(2, |_| {});
    assert_eq!(
        entries(&world),
        [
            ("enter", Screen::Loading, 0),
            ("exit", Screen::Loading, 0),
            ("enter", Screen::Playing, 0)
        ]
    );
    // Entered at the end of a tick, its request waits for the start of the next.
    let world = logged(Screen::Title)
        .on_enter(Screen::Loading, |ctx: &mut Ctx| ctx.set_state(Screen::Playing))
        .sim_system(at_tick(1, |ctx| ctx.set_state(Screen::Loading)))
        .run_headless(3, |_| {});
    assert_eq!(
        entries(&world)[1..],
        [
            ("exit", Screen::Title, 1),
            ("enter", Screen::Loading, 1),
            ("exit", Screen::Loading, 2),
            ("enter", Screen::Playing, 2)
        ]
    );
}

#[test]
fn a_change_asked_for_between_ticks_applies_before_the_next_tick() {
    // A frame system asks; the next tick runs in the new state from its start.
    let world = logged(Screen::Title)
        .headless_frames(true)
        .add(Phase::Frame, |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                ctx.set_state(Screen::Playing);
            }
        })
        .run_headless(4, |_| {});
    assert_eq!(
        entries(&world)[1..],
        [("exit", Screen::Title, 2), ("enter", Screen::Playing, 2)]
    );
}

#[test]
fn systems_run_only_while_their_condition_holds() {
    #[derive(Default)]
    struct Counts {
        playing: u32,
        under_pause: u32,
        not_title: u32,
        late: u32,
        both: u32,
    }
    let world = Game::new("t", 1, 1)
        .state(Screen::Title)
        .sim_system(at_tick(2, |ctx| ctx.set_state(Screen::Playing)))
        .sim_system(at_tick(6, |ctx| ctx.push_state(Screen::Paused)))
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| ctx.world.resource_mut::<Counts>().playing += 1).run_if(in_state(Screen::Playing)),
        )
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| ctx.world.resource_mut::<Counts>().under_pause += 1).run_if(in_stack(Screen::Playing)),
        )
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| ctx.world.resource_mut::<Counts>().not_title += 1).run_if(in_state(Screen::Title).not()),
        )
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| ctx.world.resource_mut::<Counts>().late += 1).run_if(|ctx: &Ctx| ctx.time.sim_tick() >= 8),
        )
        .add(
            Phase::AfterPhysics,
            (|ctx: &mut Ctx| ctx.world.resource_mut::<Counts>().both += 1)
                .run_if(in_stack(Screen::Playing))
                .run_if(Condition::new(|ctx: &Ctx| ctx.time.sim_tick().is_multiple_of(2)).or(in_state(Screen::Paused))),
        )
        .setup(|ctx| ctx.world.insert_resource(Counts::default()))
        .run_headless(10);
    let c = world.resource::<Counts>();
    // Title for ticks 0-2, playing 3-6, paused over play 7-9.
    assert_eq!(c.playing, 4);
    assert_eq!(c.under_pause, 7);
    assert_eq!(c.not_title, 7);
    assert_eq!(c.late, 2);
    // Ticks 4 and 6 (even, playing), then 7, 8, 9 (paused).
    assert_eq!(c.both, 5);
}

#[test]
fn a_skipped_system_sees_every_change_since_it_last_ran() {
    #[derive(Component)]
    struct Coin;
    struct Fresh(usize);
    let world = Game::new("t", 1, 1)
        .state(Screen::Playing)
        .sim_system(at_tick(1, |ctx| ctx.push_state(Screen::Paused)))
        .sim_system(at_tick(4, |ctx| ctx.pop_state::<Screen>()))
        .sim_system(|ctx: &mut Ctx| {
            ctx.world.spawn((Coin,));
        })
        .add(
            Phase::SimLast,
            (|ctx: &mut Ctx| {
                let n = ctx.world.query::<&Coin>().filter::<Added<Coin>>().iter().count();
                ctx.world.resource_mut::<Fresh>().0 = n;
            })
            .run_if(in_state(Screen::Playing)),
        )
        .setup(|ctx| {
            ctx.world.track_changes::<Coin>();
            ctx.world.insert_resource(Fresh(0));
        })
        .run_headless(6);
    // Last ran in tick 1; skipped in 2-4; in tick 5 it sees the coins of 2-5.
    assert_eq!(world.resource::<Fresh>().0, 4);
}

#[derive(Component, Clone, Copy, Serialize, Deserialize)]
struct Pos(f32);
#[derive(Component, Clone, Copy, Serialize, Deserialize)]
struct Vel(f32);

fn drift(sys: &mut SystemView) {
    sys.world.query::<&mut Pos>().for_each(|p| p.0 += 1.0);
}
fn spin(sys: &mut SystemView) {
    sys.world.query::<&mut Vel>().for_each(|v| v.0 += 2.0);
}

/// Two view systems that share a batch, one only while playing; a `Ctx`
/// system that changes the state now and then.
fn batched(mode: &str) -> Game {
    let game = Game::new("t", 1, 1).parallel_systems_env(false);
    let game = match mode {
        "off" => game.parallel_systems(false),
        "on" => game.parallel_systems(true),
        _ => game
            .parallel_systems(true)
            .parallel_threshold(std::time::Duration::ZERO),
    };
    game.state(Screen::Title)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() % 7 {
            2 => ctx.set_state(Screen::Playing),
            5 => ctx.push_state(Screen::Paused),
            6 => ctx.set_state(Screen::Title),
            _ => {}
        })
        .add(
            Phase::SimLast,
            drift
                .access(SystemAccess::new().write::<Pos>())
                .run_if(in_state(Screen::Playing)),
        )
        .add(Phase::SimLast, spin.access(SystemAccess::new().write::<Vel>()))
}

#[test]
fn state_conditions_keep_view_systems_in_one_batch() {
    let plan = batched("on").schedule_plan();
    let batch = plan.batches().next().expect("a batch");
    assert_eq!((plan.batches().count(), batch.len), (1, 2), "{plan}");
}

#[test]
fn a_closure_condition_makes_a_view_system_run_alone_and_says_why() {
    let plan = Game::new("t", 1, 1)
        .add(
            Phase::SimLast,
            drift
                .access(SystemAccess::new().write::<Pos>())
                .run_if(|ctx: &Ctx| ctx.time.sim_tick() > 3),
        )
        .add(Phase::SimLast, spin.access(SystemAccess::new().write::<Vel>()))
        .schedule_plan();
    assert_eq!(plan.batches().count(), 0, "{plan}");
    let alone = plan.systems.iter().find(|s| s.name.contains("drift")).unwrap().alone;
    assert_eq!(alone, Some(Alone::Condition));
    assert!(plan.to_string().contains("has a run condition"), "{plan}");
}

#[test]
fn a_state_condition_declares_a_read_of_the_state() {
    // A view system that writes the state conflicts with one that runs in it.
    fn pause(sys: &mut SystemView) {
        sys.world.resource_mut::<State<Screen>>().push(Screen::Paused);
    }
    let plan = Game::new("t", 1, 1)
        .state(Screen::Playing)
        .add(
            Phase::SimLast,
            pause.access(SystemAccess::new().write_resource::<State<Screen>>()),
        )
        .add(
            Phase::SimLast,
            drift
                .access(SystemAccess::new().write::<Pos>())
                .run_if(in_state(Screen::Playing)),
        )
        .schedule_plan();
    assert_eq!(plan.batches().count(), 0, "{plan}");
}

#[test]
fn the_scheduler_on_or_off_gives_the_same_world_with_conditions() {
    let run = |mode| {
        let mut world = batched(mode).run_headless(40, |ctx| {
            ctx.world.reflect::<Pos>();
            ctx.world.reflect::<Vel>();
            for _ in 0..50 {
                ctx.world.spawn((Pos(0.0), Vel(0.0)));
            }
        });
        let tick = world.change_tick();
        (world.save(), tick, world.query::<&Pos>().iter().next().unwrap().0)
    };
    let off = run("off");
    assert!(off.2 > 0.0 && off.2 < 40.0, "drift ran only while playing: {}", off.2);
    for mode in ["on", "threads"] {
        let other = run(mode);
        assert_eq!(other.0, off.0, "{mode}: the same world");
        assert_eq!(other.1, off.1, "{mode}: the same change tick");
    }
}

#[test]
fn the_state_is_saved_and_loaded_with_the_world() {
    let dir = std::env::temp_dir().join(format!("ponko-states-{}", std::process::id()));
    let world = Game::new("t", 1, 1)
        .data_dir(&dir)
        .state(Screen::Playing)
        .sim_system(at_tick(0, |ctx| ctx.push_state(Screen::Paused)))
        .sim_system(at_tick(1, |ctx| {
            ctx.set_state(Screen::Title); // asked for, not applied yet
            ctx.storage.save_world(&ctx.world, "slot").unwrap();
        }))
        .run_headless(3, |_| {});
    assert_eq!(world.resource::<State<Screen>>().get(), &Screen::Title);
    let world = Game::new("t", 1, 1)
        .data_dir(&dir)
        .state(Screen::Title)
        .run_headless(0, |ctx| ctx.load_world("slot").unwrap());
    let state = world.resource::<State<Screen>>();
    assert_eq!(state.get(), &Screen::Paused);
    assert_eq!(state.below(), [Screen::Playing]);
    assert_eq!(state.next(), Some(&Next::Set(Screen::Title)));
    assert_eq!(state.ticks(), 1, "the tick count came back too");
    let _ = std::fs::remove_dir_all(dir);
}

/// What each player asks for: pause on some ticks.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Pause(bool);

/// Two lockstep peers whose player 1 pauses and resumes the game; `tweak`
/// runs on player 0 only. Returns each peer's world and its desync, if any.
fn lockstep(tweak: fn(&mut Ctx)) -> Vec<(ponko::reflect::json::Value, Option<ponko::net::Desync>)> {
    let runs: Vec<_> = Net::loopback(2)
        .into_iter()
        .map(|net| {
            thread::spawn(move || {
                let net = net.checksum_every(5);
                let mut world = Game::new("net", 1, 1)
                    .lockstep(net)
                    .state(Screen::Playing)
                    .add(Phase::Input, |ctx: &mut Ctx| {
                        let pause = ctx.net.player() == 1 && ctx.time.sim_tick() % 20 == 3;
                        ctx.net.set_input(&Pause(pause));
                    })
                    .sim_system(|ctx: &mut Ctx| {
                        if ctx.net.input::<Pause>(1).is_some_and(|p| p.0) {
                            if *ctx.state::<Screen>() == Screen::Paused {
                                ctx.pop_state::<Screen>();
                            } else {
                                ctx.push_state(Screen::Paused);
                            }
                        }
                    })
                    .add(
                        Phase::AfterPhysics,
                        (|ctx: &mut Ctx| ctx.world.query::<&mut Pos>().for_each(|p| p.0 += 1.0))
                            .run_if(in_state(Screen::Playing)),
                    )
                    .sim_system(move |ctx: &mut Ctx| {
                        if ctx.net.player() == 0 {
                            tweak(ctx);
                        }
                        if ctx.time.sim_tick() == 199 {
                            let desync = ctx.net.desync().cloned();
                            ctx.world.insert_resource(desync);
                        }
                    })
                    .run_headless(200, |ctx| {
                        ctx.world.reflect::<Pos>();
                        ctx.world.spawn((Pos(0.0),));
                    });
                let desync = world
                    .remove_resource::<Option<ponko::net::Desync>>()
                    .expect("ran every tick");
                (world.save(), desync)
            })
        })
        .collect();
    runs.into_iter().map(|r| r.join().unwrap()).collect()
}

#[test]
fn lockstep_peers_change_state_on_the_same_tick() {
    let results = lockstep(|_| {});
    for (save, desync) in &results {
        assert!(desync.is_none(), "{desync:?}");
        assert_eq!(*save, results[0].0);
    }
    let resources = results[0].0["resources"].to_string();
    assert!(
        resources.contains("State<"),
        "the state is in the checked world: {resources}"
    );
}

#[test]
fn lockstep_finds_a_peer_whose_state_differs() {
    // The negative control: player 0 alone pushes a state.
    let results = lockstep(|ctx| {
        if ctx.time.sim_tick() == 50 {
            ctx.push_state(Screen::Options);
        }
    });
    let desync = results[0].1.as_ref().or(results[1].1.as_ref()).expect("a desync");
    assert!(
        desync.components.iter().any(|c| c.starts_with("resource:State<")),
        "{desync}"
    );
}

#[test]
#[should_panic(expected = "Game::on_enter: the game has no state of type")]
fn on_enter_needs_the_state_first() {
    let _ = Game::new("t", 1, 1).on_enter(Screen::Title, |_: &mut Ctx| {});
}

#[test]
#[should_panic(expected = "Game::state: the game already has a state of type")]
fn a_state_type_is_added_once() {
    let _ = Game::new("t", 1, 1).state(Screen::Title).state(Screen::Playing);
}

#[test]
#[should_panic(expected = "the game has no state of type")]
fn asking_for_a_state_the_game_lacks_panics_with_a_hint() {
    Game::new("t", 1, 1).run_headless(0, |ctx| ctx.set_state(Screen::Title));
}

#[test]
fn several_state_types_apply_in_registration_order_first_in_the_tick() {
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    enum Music {
        Calm,
        Loud,
    }
    let game = Game::new("t", 1, 1)
        .add(Phase::Input, |_: &mut Ctx| {})
        .state(Screen::Title)
        .state(Music::Calm);
    let plan = game.schedule_plan();
    let names: Vec<&str> = plan.systems.iter().map(|s| s.name).take(2).collect();
    assert!(
        names[0].contains("apply_state") && names[0].contains("Screen"),
        "{names:?}"
    );
    assert!(
        names[1].contains("apply_state") && names[1].contains("Music"),
        "{names:?}"
    );
    let world = game
        .sim_system(at_tick(0, |ctx| ctx.set_state(Music::Loud)))
        .run_headless(2, |_| {});
    assert_eq!(world.resource::<State<Music>>().get(), &Music::Loud);
}

// ---- Screens tied to a state (`Game::state_screen`) -------------------------

/// What the UI showed after each frame: the names of the screens on its stack, bottom first.
#[derive(Default)]
struct Shown(Vec<(u64, Vec<String>)>);

fn named_screen(name: &'static str) -> impl FnMut(&mut Ctx) -> UiId {
    move |ctx: &mut Ctx| {
        let root = ctx.ui.root().name(name).id();
        ctx.ui.button(root, "Back").name(format!("{name}.back")).closes_screen();
        root
    }
}

fn record_screens(ctx: &mut Ctx) {
    let names = ["title", "paused", "options"]
        .into_iter()
        .filter(|n| ctx.ui.find(n).is_some_and(|id| ctx.ui.is_screen(id)))
        .map(String::from)
        .collect();
    let tick = ctx.time.sim_tick();
    ctx.world.resource_mut::<Shown>().0.push((tick, names));
}

fn screens_game(initial: Screen) -> Game {
    use ponko::ui::ScreenMode;
    Game::new("screens", 800, 600)
        .state(initial)
        .state_screen(Screen::Title, ScreenMode::Modeless, named_screen("title"))
        .state_screen(Screen::Paused, ScreenMode::Modal, named_screen("paused"))
        .state_screen(Screen::Options, ScreenMode::Modal, named_screen("options"))
        .system(record_screens)
        .headless_frames(true)
}

fn shown_at(world: &World, tick: u64) -> Vec<String> {
    let shown = &world.resource::<Shown>().0;
    shown
        .iter()
        .rev()
        .find(|(t, _)| *t == tick)
        .map(|(_, s)| s.clone())
        .unwrap_or_default()
}

#[test]
fn a_state_screen_is_up_exactly_while_its_state_lasts_and_its_nodes_go_with_it() {
    let world = screens_game(Screen::Playing)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            2 => ctx.push_state(Screen::Paused),
            6 => ctx.pop_state::<Screen>(),
            _ => {}
        })
        .run_headless(10, |ctx| ctx.world.insert_resource(Shown::default()));
    assert!(shown_at(&world, 2).is_empty(), "the change applies after tick 2");
    assert_eq!(shown_at(&world, 3), ["paused"]);
    assert_eq!(shown_at(&world, 6), ["paused"]);
    assert!(shown_at(&world, 7).is_empty(), "taken down on exit");
}

#[test]
fn closing_a_pushed_state_screen_pops_the_state() {
    use ponko::actions::InputEvent;
    use ponko::scenario::InputScenario;
    // Paused over Playing, Options over Paused. Escape closes Options, the
    // Back button then closes Paused.
    let scenario = InputScenario::new()
        .at(8, InputEvent::Key(Key::Escape, true))
        .at(9, InputEvent::Key(Key::Escape, false))
        .click(14, "paused.back")
        .duration(20);
    let (world, report) = screens_game(Screen::Playing)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            1 => ctx.push_state(Screen::Paused),
            3 => ctx.push_state(Screen::Options),
            _ => {}
        })
        .run_headless_scenario(scenario, |ctx| ctx.world.insert_resource(Shown::default()));
    report.assert_passed();
    assert_eq!(
        shown_at(&world, 5),
        ["paused", "options"],
        "Paused stays up below Options"
    );
    assert_eq!(shown_at(&world, 11), ["paused"], "Escape closed Options and popped it");
    assert!(shown_at(&world, 17).is_empty(), "the Back button closed Paused");
    let state = world.resource::<State<Screen>>();
    assert_eq!((state.get(), state.below()), (&Screen::Playing, &[][..]));
}

#[test]
fn a_set_state_screen_comes_back_when_closed_since_there_is_nothing_to_go_back_to() {
    use ponko::actions::InputEvent;
    use ponko::scenario::InputScenario;
    let scenario = InputScenario::new()
        .at(4, InputEvent::Key(Key::Escape, true))
        .at(5, InputEvent::Key(Key::Escape, false))
        .duration(10);
    let (world, report) =
        screens_game(Screen::Title).run_headless_scenario(scenario, |ctx| ctx.world.insert_resource(Shown::default()));
    report.assert_passed();
    for tick in [1, 4, 5, 9] {
        assert_eq!(shown_at(&world, tick), ["title"], "tick {tick}");
    }
    assert_eq!(world.resource::<State<Screen>>().get(), &Screen::Title);
}

#[test]
fn entering_a_state_screen_again_builds_it_afresh() {
    let world = screens_game(Screen::Playing)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            1 | 5 => ctx.push_state(Screen::Paused),
            3 | 7 => ctx.pop_state::<Screen>(),
            _ => {}
        })
        .sim_system(|ctx: &mut Ctx| {
            if let Some(id) = ctx.ui.find("paused") {
                ctx.world.insert_resource(id); // the last root seen
            }
            if ctx.time.sim_tick() == 2 {
                let first = ctx.ui.find("paused").unwrap();
                ctx.world.insert_resource((first,));
            }
        })
        .run_headless(10, |ctx| ctx.world.insert_resource(Shown::default()));
    let (first,) = *world.resource::<(UiId,)>();
    assert_ne!(*world.resource::<UiId>(), first, "the second pause built a new tree");
    assert!(shown_at(&world, 9).is_empty());
}

#[test]
fn a_state_screen_leaves_the_ui_events_to_the_game() {
    use ponko::scenario::InputScenario;
    #[derive(Default)]
    struct Heard(Vec<UiEvent>);
    let scenario = InputScenario::new().click(4, "paused.back").duration(8);
    let (world, report) = screens_game(Screen::Playing)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                ctx.push_state(Screen::Paused);
            }
        })
        .system(|ctx: &mut Ctx| {
            let events: Vec<UiEvent> = ctx.ui.events().collect();
            ctx.world.resource_mut::<Heard>().0.extend(events);
        })
        .run_headless_scenario(scenario, |ctx| {
            ctx.world.insert_resource(Shown::default());
            ctx.world.insert_resource(Heard::default());
        });
    report.assert_passed();
    let heard = &world.resource::<Heard>().0;
    assert!(
        matches!(heard[..], [UiEvent::Clicked(_), UiEvent::ScreenPopped(_)]),
        "{heard:?}"
    );
    assert_eq!(world.resource::<State<Screen>>().get(), &Screen::Playing);
}
