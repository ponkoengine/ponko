//! Where the engine's own service steps sit in a windowed frame, driven
//! without a window: `begin_frame` then `run_systems`, as `window.rs` does.
//!
//! The defaults pinned here are part of the documented frame order in
//! ARCHITECTURE.md: retained UI updates before any simulation or frame
//! system, built-in animation and particles run between the frame systems
//! and the late systems (the built-in systems that end `Phase::Animate`),
//! positional audio follows the late and drawing systems (the built-in
//! system that ends `Phase::Draw`). Headless runs skip all of these
//! (`tests/service_phases.rs`).

use super::*;
use ponko_ecs::Component;

#[derive(Default)]
struct Log(Vec<String>);

fn ctx() -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
    // No device: positional audio hands out silent voices.
    ctx.set_headless(true);
    ctx.world.insert_resource(Log::default());
    ctx
}

fn note(ctx: &mut Ctx, line: impl Into<String>) {
    ctx.world.resource_mut::<Log>().0.push(line.into());
}

fn take(ctx: &mut Ctx) -> Vec<String> {
    std::mem::take(&mut ctx.world.resource_mut::<Log>().0)
}

/// A game without the built-in drawing systems (animation, particles and
/// positional audio stay).
fn bare(game: Game) -> Game {
    game.remove(Systems::ANIMATE_SPRITES)
        .remove(Systems::DRAW_TILEMAPS)
        .remove(Systems::DRAW_SPRITES)
        .remove(Systems::DRAW_PARTICLES)
}

/// One windowed frame running `ticks` simulation ticks, then the end-of-frame
/// input reset, as the window loop does.
fn frame(game: &mut Game, ctx: &mut Ctx, ticks: u32) {
    ctx.begin_frame(FrameTiming {
        dt: 1.0 / 60.0,
        seconds: 1.0 / 60.0,
        fixed_steps: ticks,
        alpha: 0.0,
    });
    game.run_systems(ctx, ticks);
    ctx.input.end_frame();
}

#[test]
fn retained_ui_updates_before_simulation_and_frame_systems() {
    use crate::ui::unstable_taffy::{Dimension, Size, Style};
    let mut ctx = ctx();
    let root = ctx.ui.add(
        crate::ui::UiWidget::Panel,
        crate::ui::UiLayout::unstable_custom(Style {
            size: Size {
                width: Dimension::length(300.0),
                height: Dimension::length(200.0),
            },
            ..Default::default()
        }),
    );
    let button = ctx.ui.add_child(
        root,
        crate::ui::UiWidget::Button,
        crate::ui::UiLayout::unstable_custom(Style {
            size: Size {
                width: Dimension::length(70.0),
                height: Dimension::length(30.0),
            },
            flex_shrink: 0.0,
            ..Default::default()
        }),
    );
    ctx.ui.push_modeless(root);
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| {
            let line = format!("sim focus={}", ctx.ui.focus().is_some());
            note(ctx, line);
        })
        .system(|ctx: &mut Ctx| {
            let line = format!("frame focus={}", ctx.ui.focus().is_some());
            note(ctx, line);
        });
    assert_eq!(ctx.ui.focus(), None);
    // Tab moves the focus to the button when the UI updates. The systems of
    // the same frame already see it: the UI went first.
    ctx.input
        .feed(crate::actions::InputEvent::Key(crate::input::Key::Tab, true));
    frame(&mut game, &mut ctx, 1);
    assert_eq!(take(&mut ctx), ["sim focus=true", "frame focus=true"]);
    assert_eq!(ctx.ui.focus(), Some(button));
}

#[test]
fn ticks_per_frame_follow_the_requested_count() {
    let mut ctx = ctx();
    let mut game = bare(Game::new("t", 1, 1)).sim_system(|ctx: &mut Ctx| {
        let tick = ctx.time.sim_tick();
        note(ctx, format!("tick {tick}"));
    });
    // Zero ticks (a frame between ticks) and three ticks.
    frame(&mut game, &mut ctx, 0);
    frame(&mut game, &mut ctx, 3);
    assert_eq!(take(&mut ctx), ["tick 0", "tick 1", "tick 2"]);
}

#[cfg(feature = "3d")]
mod positional_audio {
    use super::*;
    use crate::{d3::AudioSettings as AudioSettings3D, d3::AudioSource as AudioSource3D, d3::Transform as Transform3D};
    use glam::Vec3;

    #[test]
    fn positional_audio_follows_late_and_draw_systems() {
        let mut ctx = ctx();
        let sound = ctx.audio.create(100, &[0.0; 100]);
        let moved = ctx.world.spawn((
            Transform3D::default(),
            AudioSource3D::new(sound, AudioSettings3D::default()),
        ));
        // The draw system is added last, after the built-in audio system was
        // installed: audio still runs after it.
        let mut game = bare(Game::new("t", 1, 1))
            .system(|ctx: &mut Ctx| {
                let silent = ctx.audio.backend.spatial.voice_positions().len();
                note(ctx, format!("frame voices={silent}"));
            })
            .add(Phase::Late, move |ctx: &mut Ctx| {
                let started = ctx.world.get::<AudioSource3D>(moved).unwrap().started();
                note(ctx, format!("late started={started}"));
                ctx.world.get_mut::<Transform3D>(moved).unwrap().position = Vec3::X * 4.0;
            })
            .add(Phase::Draw, move |ctx: &mut Ctx| {
                let started = ctx.world.get::<AudioSource3D>(moved).unwrap().started();
                note(ctx, format!("draw started={started}"));
                ctx.world.get_mut::<Transform3D>(moved).unwrap().position = Vec3::X * 9.0;
            });
        frame(&mut game, &mut ctx, 1);
        assert_eq!(
            take(&mut ctx),
            ["frame voices=0", "late started=false", "draw started=false"],
            "no system sees a voice before the audio step"
        );
        assert_eq!(
            ctx.audio.backend.spatial.voice_positions(),
            [Vec3::X * 9.0],
            "the emitter started once the frame's systems were done, at its final place"
        );
    }

    /// The positional-audio step is a system: removed, emitters never start;
    /// added again in `Late`, it runs there, before the drawing systems.
    #[test]
    fn positional_audio_can_be_removed_or_moved_like_any_system() {
        let mut ctx = ctx();
        let sound = ctx.audio.create(100, &[0.0; 100]);
        let moved = ctx.world.spawn((
            Transform3D::default(),
            AudioSource3D::new(sound, AudioSettings3D::default()),
        ));
        let mut game = bare(Game::new("t", 1, 1)).remove(Systems::UPDATE_AUDIO3D);
        frame(&mut game, &mut ctx, 0);
        assert!(ctx.audio.backend.spatial.voice_positions().is_empty(), "removed");
        assert!(!ctx.world.get::<AudioSource3D>(moved).unwrap().started());

        let mut game = game
            .add(Phase::Late, crate::systems::update_audio3d)
            .add(Phase::Draw, move |ctx: &mut Ctx| {
                let started = ctx.world.get::<AudioSource3D>(moved).unwrap().started();
                note(ctx, format!("draw started={started}"));
                ctx.world.get_mut::<Transform3D>(moved).unwrap().position = Vec3::X * 9.0;
            });
        frame(&mut game, &mut ctx, 0);
        assert_eq!(take(&mut ctx), ["draw started=true"], "audio ran in Late");
        assert_eq!(
            ctx.audio.backend.spatial.voice_positions(),
            [Vec3::ZERO],
            "placed before the draw system moved it"
        );
    }

    /// Positional audio is the end of a frame; a headless run has ticks and
    /// no frames, so its emitters never start, however many ticks run.
    #[test]
    fn headless_runs_do_not_start_positional_audio() {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s = seen.clone();
        let mut world = Game::new("t", 1, 1)
            .sim_system(move |ctx: &mut Ctx| {
                let started = ctx.world.query::<&AudioSource3D>().iter().any(AudioSource3D::started);
                let voices = ctx.audio.backend.spatial.voice_positions().len();
                s.borrow_mut().push((started, voices));
            })
            .run_headless(3, |ctx| {
                let sound = ctx.audio.create(100, &[0.0; 100]);
                ctx.world.spawn((
                    Transform3D::default(),
                    AudioSource3D::new(sound, AudioSettings3D::default()),
                ));
            });
        assert_eq!(*seen.borrow(), [(false, 0); 3]);
        assert!(world.query::<&AudioSource3D>().iter().all(|s| !s.started()));
    }

    #[test]
    fn emitters_spawned_by_late_systems_start_in_the_same_frame() {
        let mut ctx = ctx();
        let sound = ctx.audio.create(100, &[0.0; 100]);
        let mut game = bare(Game::new("t", 1, 1)).add(Phase::Late, move |ctx: &mut Ctx| {
            ctx.commands.spawn((
                Transform3D::default(),
                AudioSource3D::new(sound, AudioSettings3D::default()),
            ));
        });
        frame(&mut game, &mut ctx, 0);
        assert_eq!(ctx.audio.backend.spatial.voice_positions().len(), 1);
        assert!(ctx.world.query::<&AudioSource3D>().iter().all(|s| s.started()));
    }

    #[test]
    fn the_listener_is_the_cameras_final_place_of_the_frame() {
        let mut ctx = ctx();
        let mut game = bare(Game::new("t", 1, 1)).add(Phase::Late, |ctx: &mut Ctx| {
            ctx.scene3d.camera.position = Vec3::new(1.0, 2.0, 3.0);
        });
        frame(&mut game, &mut ctx, 0);
        assert_eq!(ctx.audio.listener().position, Vec3::new(1.0, 2.0, 3.0));
    }
}

#[test]
fn a_frame_runs_ticks_then_frame_then_late_then_draw_and_applies_commands_between() {
    #[derive(Component)]
    struct Spawned;
    let mut ctx = ctx();
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| note(ctx, "sim"))
        .add(Phase::Frame, |ctx: &mut Ctx| {
            note(ctx, "frame");
            ctx.commands.spawn((Spawned,));
            // The recording system never sees its own commands.
            let n = ctx.world.query::<&Spawned>().count();
            note(ctx, format!("frame sees {n}"));
        })
        .add(Phase::Animate, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Spawned>().count();
            note(ctx, format!("animate sees {n}"));
        })
        .add(Phase::Late, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Spawned>().count();
            note(ctx, format!("late sees {n}"));
        })
        .add(Phase::Draw, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Spawned>().count();
            note(ctx, format!("draw sees {n}"));
        });
    frame(&mut game, &mut ctx, 2);
    assert_eq!(
        take(&mut ctx),
        [
            "sim",
            "sim",
            "frame",
            "frame sees 0",
            "animate sees 1",
            "late sees 1",
            "draw sees 1"
        ],
        "commands apply right after the recording system, before the next one"
    );
}

/// The built-in animation and particle step sits between the frame systems
/// and the late systems, so late systems read the final pose and the
/// particles of this frame.
#[cfg(feature = "3d")]
#[test]
fn builtin_animation_and_particles_run_between_frame_and_late_systems() {
    use crate::{
        anim::AnimationClip, anim::AnimationGraph, anim::AnimationState, anim::Animator, anim::Joint, anim::Motion,
        anim::Skeleton, d3::ParticleEmitter as ParticleEmitter3D, d3::ParticleSettings as ParticleSettings3D,
        d3::Transform as Transform3D,
    };
    use glam::Vec3;
    let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
    let clip = AnimationClip::new("rest", &skeleton, vec![]);
    let animator = Animator::new(
        AnimationGraph::new(
            vec![],
            vec![AnimationState::new(Motion::Clip(clip))],
            vec![],
            crate::anim::StateId(0),
        )
        .expect("a valid graph"),
    );
    let mut ctx = ctx();
    let rig = ctx.world.spawn((skeleton, animator));
    let emitter = ctx.world.spawn((
        Transform3D::default(),
        ParticleEmitter3D::new(ParticleSettings3D {
            rate: 0.0,
            gravity: Vec3::ZERO,
            ..Default::default()
        }),
    ));
    let mut game = bare(Game::new("t", 1, 1))
        .system(move |ctx: &mut Ctx| {
            // A pose the frame system forces and a burst it asks for.
            ctx.world.get_mut::<Skeleton>(rig).unwrap().pose[0].position.x = 7.0;
            ctx.world.get_mut::<ParticleEmitter3D>(emitter).unwrap().burst(3);
            let live = ctx.world.get::<ParticleEmitter3D>(emitter).unwrap().live_count();
            note(ctx, format!("frame live={live}"));
        })
        .add(Phase::Late, move |ctx: &mut Ctx| {
            let x = ctx.world.get::<Skeleton>(rig).unwrap().pose[0].position.x;
            let live = ctx.world.get::<ParticleEmitter3D>(emitter).unwrap().live_count();
            note(ctx, format!("late pose={x} live={live}"));
        });
    frame(&mut game, &mut ctx, 0);
    assert_eq!(
        take(&mut ctx),
        ["frame live=0", "late pose=0 live=3"],
        "animation posed the rig and the particles spawned before the late systems"
    );

    // The same holds for a system added to `Animate` after the built-ins
    // were installed: they stay at the end of the phase.
    let mut game = game.add(Phase::Animate, move |ctx: &mut Ctx| {
        ctx.world.get_mut::<Skeleton>(rig).unwrap().pose[0].position.x = 5.0;
        let live = ctx.world.get::<ParticleEmitter3D>(emitter).unwrap().live_count();
        ctx.world.get_mut::<ParticleEmitter3D>(emitter).unwrap().burst(2);
        note(ctx, format!("animate live={live}"));
    });
    frame(&mut game, &mut ctx, 0);
    let log = take(&mut ctx);
    assert_eq!(log[1], "animate live=3", "{log:?}");
    assert_eq!(log[2], "late pose=0 live=8", "{log:?}");
}

#[test]
fn both_loops_build_their_context_from_the_same_configuration() {
    let configured = || Game::new("t", 64, 48).seed(7).fixed_hz(50).spatial_grid(32.0);
    let windowed = configured().build_ctx(false);
    let headless = configured().build_ctx(true);
    assert!(!windowed.headless && headless.headless, "the one difference");
    assert_eq!(windowed.seed, 7);
    assert_eq!(headless.seed, windowed.seed);
    assert_eq!(headless.spatial.cell_size(), 32.0);
    assert_eq!(headless.spatial.cell_size(), windowed.spatial.cell_size());
    assert_eq!(headless.time.fixed_dt(), windowed.time.fixed_dt());
    assert_eq!(headless.window_size(), windowed.window_size());
    assert_eq!(headless.time.sim_tick(), 0, "nothing has run before setup");
}
