//! Code mods: the frost drip's crawl (turn at walls and ledges, two ray
//! casts ahead and down) for 1000 drips per tick, as the game's own Rust
//! rule and as a code mod run by the WebAssembly interpreter (`mods-wasm`,
//! one `with_code` per tick, one call per drip). The gap is what running a
//! mod's code costs per call; divide a tick by 1000 for one drip.
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ponko::d2::{Collider, QueryFilter, RigidBody, Transform as Transform2D};
use ponko::mods::{ModPack, ModSet, ModValue, with_code};
use ponko::prelude::{Ctx, Game, Vec2, vec2};

use crate::{Bench, report};

const DRIPS: usize = 1000;
const WARMUP: usize = 60;
const RUNS: usize = 300;

/// The crawl as WebAssembly text: `crawl(x, y, heading) -> heading`.
const CRAWL: &str = r#"(module
  (import "ponko" "cast_ray" (func $ray (param f32 f32 f32 f32 f32) (result f32)))
  (func (export "crawl") (param $x f32) (param $y f32) (param $h f32) (result f32)
    (local $wall i32) (local $floor i32)
    (local.set $wall (f32.ge (call $ray (local.get $x) (f32.add (local.get $y) (f32.const 4))
                                        (local.get $h) (f32.const 0) (f32.const 8)) (f32.const 0)))
    (local.set $floor (f32.ge (call $ray (f32.add (local.get $x) (f32.mul (local.get $h) (f32.const 7)))
                                         (f32.add (local.get $y) (f32.const 4))
                                         (f32.const 0) (f32.const 1) (f32.const 8)) (f32.const 0)))
    (if (result f32) (i32.or (local.get $wall) (i32.eqz (local.get $floor)))
      (then (f32.neg (local.get $h)))
      (else (local.get $h)))))"#;

/// The same rule in Rust, against the game's physics.
fn crawl_rust(ctx: &Ctx, at: Vec2, heading: f32) -> f32 {
    let filter = QueryFilter::default();
    let wall = ctx
        .physics
        .cast_ray(at + vec2(0.0, 4.0), vec2(heading, 0.0), 8.0, filter)
        .is_some();
    let floor = ctx
        .physics
        .cast_ray(at + vec2(heading * 7.0, 4.0), vec2(0.0, 1.0), 8.0, filter)
        .is_some();
    if wall || !floor { -heading } else { heading }
}

pub fn run(b: &mut Bench) {
    for (name, code) in [("mods/crawl-1k/rust", false), ("mods/crawl-1k/wasmi", true)] {
        if b.skips(name) {
            continue;
        }
        let times = Rc::new(RefCell::new(Vec::<Duration>::with_capacity(RUNS)));
        let timed = times.clone();
        // 40 ledges 200 px wide, 25 drips on each.
        let mut drips: Vec<(Vec2, f32)> = (0..DRIPS)
            .map(|i| {
                let ledge = (i / 25) as f32;
                (vec2(ledge * 300.0 + 10.0 + (i % 25) as f32 * 7.0, -4.5), 1.0)
            })
            .collect();
        let mut mods = code.then(|| {
            let manifest = r#"{"format":"ponko-mod","version":1,"id":"frost","code":"crawl.wasm"}"#;
            let wasm = wat::parse_str(CRAWL).expect("valid WebAssembly text");
            let pack = ModPack::from_files(
                "frost",
                [("mod.json", manifest.as_bytes().to_vec()), ("crawl.wasm", wasm)],
            )
            .expect("a valid manifest");
            ModSet::new().with(pack)
        });
        Game::new("bench", 1, 1)
            .physics(Vec2::ZERO)
            .sim_system(move |ctx: &mut Ctx| {
                // Queries see the world from the first step on.
                if ctx.time.sim_tick() < 2 {
                    return;
                }
                let t = Instant::now();
                if code {
                    with_code(ctx, |mods| {
                        let crawl = mods.function("frost", "crawl").expect("exported");
                        for (at, heading) in &mut drips {
                            let args = [ModValue::F32(at.x), ModValue::F32(at.y), ModValue::F32(*heading)];
                            *heading = mods
                                .call(crawl, &args)
                                .ok()
                                .flatten()
                                .and_then(ModValue::as_f32)
                                .unwrap_or(*heading);
                            at.x += *heading * 0.5;
                        }
                    });
                } else {
                    for (at, heading) in &mut drips {
                        *heading = crawl_rust(ctx, *at, *heading);
                        at.x += *heading * 0.5;
                    }
                }
                timed.borrow_mut().push(t.elapsed());
            })
            .run_headless((1 + WARMUP + RUNS) as u64, |ctx| {
                for ledge in 0..40 {
                    ctx.world.spawn((
                        Transform2D::at(vec2(ledge as f32 * 300.0 + 100.0, 5.0)),
                        RigidBody::fixed(),
                        Collider::rect(vec2(200.0, 10.0)),
                    ));
                }
                if let Some(set) = mods.take() {
                    set.mount(ctx).expect("the mod mounts");
                }
            });
        let mut times = times.borrow_mut();
        let mut measured: Vec<Duration> = times.drain(..).skip(WARMUP).collect();
        let record = report::Record::new(name, report::Kind::Steady, WARMUP, &mut measured);
        b.report(record);
    }
}
