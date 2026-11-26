//! The `coin_dash` reference game laid out for phones: a window the size of
//! a phone held upright and turned, with the safe area insets an iPhone and
//! an Android phone report, a touch screen. Played headlessly with frames
//! (`Game::headless_frames`), no window or GPU. On every screen the whole
//! arena, the player and the coins are in the safe area, every line of text
//! is inside it and not cut off, the menus are centred in it, the status line
//! keeps clear of the on-screen pause button, and the hints name taps.
//! A desktop window the arena's size is laid out as it always was.

#[path = "../examples/coin_dash/game.rs"]
mod game;

use game::fit2d::Placed;
use game::*;
use ponko::d2::Transform;
use ponko::prelude::*;
use ponko::scenario::InputScenario;
use ponko::window::SafeInsets;
use std::cell::RefCell;
use std::rc::Rc;

/// A phone's window and the insets its system reports.
#[derive(Debug, Clone, Copy)]
struct Phone {
    name: &'static str,
    size: Vec2,
    insets: SafeInsets,
}

/// iPhone (the Dynamic Island, the home bar) and Android (the camera
/// cutout, the navigation bar), upright and turned, as the mobile checks
/// measured them.
const PHONES: [Phone; 4] = [
    Phone {
        name: "iPhone upright",
        size: vec2(402.0, 874.0),
        insets: SafeInsets {
            top: 62.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        },
    },
    Phone {
        name: "iPhone turned",
        size: vec2(874.0, 402.0),
        insets: SafeInsets {
            top: 0.0,
            right: 62.0,
            bottom: 21.0,
            left: 62.0,
        },
    },
    Phone {
        name: "Android upright",
        size: vec2(411.0, 914.0),
        insets: SafeInsets {
            top: 52.0,
            right: 0.0,
            bottom: 24.0,
            left: 0.0,
        },
    },
    Phone {
        name: "Android turned",
        size: vec2(914.0, 411.0),
        insets: SafeInsets {
            top: 0.0,
            right: 0.0,
            bottom: 24.0,
            left: 52.0,
        },
    },
];

/// What the player sees in one frame.
#[derive(Debug, Clone)]
struct Seen {
    screen: Screen,
    safe: Rect,
    /// The arena's corners in window pixels.
    arena: Rect,
    /// The player and the coins in window pixels.
    things: Vec<Vec2>,
    text: Vec<Placed>,
    /// The on-screen pause button, in window pixels, while it is up.
    pause: Option<Rect>,
}

fn look(ctx: &mut Ctx) -> Seen {
    let text = hud_text(ctx);
    let camera = ctx.camera;
    let mut things: Vec<Vec2> = Vec::new();
    for (t, _) in ctx.world.query::<(&Transform, &Player)>().iter() {
        things.push(camera.world_to_screen(t.position));
    }
    for (t, _) in ctx.world.query::<(&Transform, &Coin)>().iter() {
        things.push(camera.world_to_screen(t.position));
    }
    let pause = ctx.input.touch_controls_shown().then(|| {
        let id = ctx.ui.find("touch.pause").expect("the pause button");
        ctx.ui.rect(id).expect("laid out")
    });
    Seen {
        screen: screen(&ctx.world),
        safe: ctx.window.safe_area(),
        arena: Rect {
            min: camera.world_to_screen(Vec2::ZERO),
            max: camera.world_to_screen(ARENA),
        },
        things,
        text,
        pause,
    }
}

/// The ticks the script below shows each screen on: the title, a round, the
/// pause and a lost round's result, each after its lines came in.
const TITLE: u64 = 40;
const PLAYING: u64 = 100;
const PAUSED: u64 = 140;
const RESULT: u64 = 200;

/// Plays title → round → pause → round → result in a window `size` large
/// with `insets`, on a touch screen if `touch`, and looks at each screen.
fn play(size: Vec2, insets: SafeInsets, touch: bool, scenario: InputScenario) -> Vec<Seen> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    // A data folder of its own: the tests run at the same time.
    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ponko-coin-dash-layout-{run}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (_, report) = Game::new("coin dash layout", size.x as u32, size.y as u32)
        .seed(SEED)
        .data_dir(&dir)
        .headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let intent = ctx.world.resource_mut::<Intent>();
            match tick {
                60 => intent.confirm = true,
                101 | 141 => intent.pause = true,
                150 => ctx.world.resource_mut::<Round>().time_left = 0.01,
                _ => {}
            }
        })
        .plugin(CoinDash)
        .add(Phase::Late, move |ctx: &mut Ctx| {
            if [TITLE, PLAYING, PAUSED, RESULT].contains(&ctx.time.sim_tick()) {
                let seen = look(ctx);
                log.borrow_mut().push(seen);
            }
        })
        .setup(move |ctx: &mut Ctx| {
            ctx.window.feed_safe_insets(insets);
            ctx.input.set_touch_device(touch);
        })
        .run_headless_scenario(scenario.duration(RESULT + 1));
    report.assert_passed();
    let _ = std::fs::remove_dir_all(&dir);
    seen.take()
}

fn inside(outer: Rect, inner: Rect) -> bool {
    let slack = 0.5;
    inner.min.x >= outer.min.x - slack
        && inner.min.y >= outer.min.y - slack
        && inner.max.x <= outer.max.x + slack
        && inner.max.y <= outer.max.y + slack
}

/// Everything a phone shows is where it must be.
fn check(phone: &Phone, seen: &[Seen]) {
    let screens: Vec<Screen> = seen.iter().map(|s| s.screen).collect();
    assert_eq!(
        screens,
        [
            Screen::Title,
            Screen::Playing,
            Screen::Paused,
            Screen::Result(Outcome::Lost)
        ],
        "{}",
        phone.name
    );
    for s in seen {
        let what = format!("{} {:?}", phone.name, s.screen);
        assert_eq!(s.safe, phone.insets_area(), "{what}");
        // The whole arena, as large as fits: it spans the safe area one way.
        assert!(inside(s.safe, s.arena), "{what}: arena {:?} in {:?}", s.arena, s.safe);
        let spans = (s.arena.size() - s.safe.size()).abs().min_element();
        assert!(spans < 0.5, "{what}: arena {:?} fills {:?} one way", s.arena, s.safe);
        assert!((s.arena.center() - s.safe.center()).length() < 0.5, "{what}: centred");
        for at in &s.things {
            assert!(s.safe.contains(*at), "{what}: {at} off screen");
        }
        if s.screen != Screen::Title {
            assert!(s.things.len() > 1, "{what}: the player and the coins");
        }
        assert!(!s.text.is_empty(), "{what}");
        for line in &s.text {
            assert!(line.size >= 8.0, "{what}: {:?} is readable", line.text);
            assert!(
                inside(s.safe, line.rect()),
                "{what}: {:?} at {:?}",
                line.text,
                line.rect()
            );
            if let Some(pause) = s.pause {
                assert!(
                    line.rect().max.x <= pause.min.x || line.rect().min.y >= pause.max.y,
                    "{what}: {:?} under the pause button {pause:?}",
                    line.text
                );
            }
            // Desktop keys aren't offered on a touch screen.
            assert!(
                !line.text.contains("Enter") && !line.text.contains("Esc"),
                "{what}: {:?}",
                line.text
            );
        }
        // The menu's lines are centred in the safe area (the status line,
        // while a round is on, sits in its corner).
        let menu = if s.screen == Screen::Title {
            &s.text[..]
        } else {
            &s.text[1..]
        };
        for line in menu {
            let middle = line.rect().center().x;
            assert!(
                (middle - s.safe.center().x).abs() < 0.5,
                "{what}: {:?} centred",
                line.text
            );
        }
        if matches!(s.screen, Screen::Playing | Screen::Paused) {
            assert!(s.pause.is_some(), "{what}: the pause button is up");
            assert!(s.text[0].text.starts_with("Round 1"), "{what}");
        }
    }
    let hint = |i: usize| seen[i].text.last().unwrap().text.clone();
    assert_eq!(hint(0), "Tap: play    Set: settings", "{}", phone.name);
    assert_eq!(hint(2), "Tap or II: resume", "{}", phone.name);
    assert_eq!(hint(3), "Tap: play again", "{}", phone.name);
}

impl Phone {
    fn insets_area(&self) -> Rect {
        let i = self.insets;
        Rect::new(
            i.left,
            i.top,
            self.size.x - i.left - i.right,
            self.size.y - i.top - i.bottom,
        )
    }
}

#[test]
fn on_a_phone_upright_and_turned_everything_is_in_the_safe_area() {
    for phone in PHONES {
        let seen = play(phone.size, phone.insets, true, InputScenario::new());
        check(&phone, &seen);
    }
}

#[test]
fn the_layout_follows_the_safe_area_when_it_changes_without_a_restart() {
    // Upright, then the notch is on the left from the round on, as when the
    // phone turns: the layout is worked out every frame from the window's
    // safe area, nothing is kept from startup.
    let upright = PHONES[0];
    let left = SafeInsets::new(0.0, 0.0, 34.0, 62.0);
    let scenario = InputScenario::new().safe_insets(80, left);
    let seen = play(upright.size, upright.insets, true, scenario);
    assert_eq!(seen[0].safe, upright.insets_area());
    let turned = Rect::new(62.0, 0.0, upright.size.x - 62.0, upright.size.y - 34.0);
    for s in &seen[1..] {
        assert_eq!(s.safe, turned);
        assert!(inside(turned, s.arena), "{:?}", s.arena);
        for line in &s.text {
            assert!(inside(turned, line.rect()), "{:?} at {:?}", line.text, line.rect());
        }
    }
    // The status line moved right with the notch.
    assert!(seen[1].text[0].at.x >= 62.0 + 12.0 - 0.5, "{:?}", seen[1].text[0]);
    assert!(seen[0].arena.min.x < 1.0, "upright the arena spans the width");
}

#[test]
fn a_desktop_window_the_arenas_size_is_laid_out_as_it_always_was() {
    let seen = play(ARENA, SafeInsets::NONE, false, InputScenario::new());
    for s in &seen {
        // The camera the game always had: zoom 1, the arena's corner at the window's.
        assert_eq!(s.arena, Rect::new(0.0, 0.0, ARENA.x, ARENA.y), "{:?}", s.screen);
    }
    // The status line in its corner, at its size.
    let status = &seen[1].text[0];
    assert_eq!((status.at, status.size), (vec2(12.0, 8.0), 20.0));
    // The title's lines at their sizes, centred on the arena, the block in
    // its middle; the hints name keys.
    let title = &seen[0].text;
    let sizes: Vec<f32> = title.iter().map(|l| l.size).collect();
    assert_eq!(sizes, [56.0, 20.0, 20.0, 20.0]);
    let height: f32 = sizes.iter().map(|s| s * 1.5).sum();
    assert_eq!(title[0].at.y, (ARENA.y - height) / 2.0);
    for line in title {
        assert!((line.at.x - (ARENA.x - line.width) / 2.0).abs() < 1e-3, "{:?}", line);
    }
    assert!(title[3].text.starts_with("Enter: play"), "{:?}", title[3].text);
}
