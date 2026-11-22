//! Bevy's side of Ponko's `compare/...` rows (`crates/ponko_bench/src/scoreboard.rs`):
//! the same scenes, counts, sequences and target, timed the same way (one
//! frame is one pass of the loop: `App::update`, then, for the GPU rows,
//! waiting until the GPU has finished it), 60 warm-up frames.
//!
//! - `sim`: 1,000,000 entities moved by one system (`pos += vel * dt`), in
//!   an app of `MinimalPlugins`.
//! - `physics`: 10,000 dynamic 10 px boxes falling into a pile between walls,
//!   through `bevy_rapier2d` (rapier 0.36, as Ponko's) with a fixed 1/60 s
//!   step, 50 pixels a meter and gravity 980.
//! - `sprites`: 100,000 and 1,000,000 bouncing 32 x 32 white sprites of one
//!   texture, drawn by a `Camera2d` into a 1280 x 720 image, and the 100,000
//!   with a different tint each (`-tinted`). Bevy 0.20 draws a `Sprite` as a
//!   2D mesh with a material shared by equal sprites, so one color is its
//!   fast path and a tint each is one material each.
//! - `ui`: a 100 x 100 grid of colored `Node`s filling a 1280 x 720 image,
//!   still, or every node recolored every frame.
//! - `first-frame`: the process from start to its first frame finished on
//!   the GPU (the scoreboard times the process; this exits after it).
//!
//! Every GPU row draws with `DefaultPlugins` and no window, pipelines
//! compiled before use, and pipelined rendering off: like Ponko's run, one
//! thread simulates and renders, and each frame is finished before the next.

use std::time::{Duration, Instant};

use bevy::{
    app::SubApps,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat},
        renderer::{RenderAdapterInfo, RenderDevice},
    },
    window::ExitCondition,
};
use serde_json::{Value, json};

use crate::BEVY_VERSION;
use crate::scenes::{Dist, command_line, load_average, write};

const WARMUP: usize = 60;
const SIZE: (u32, u32) = (1280, 720);

/// The build and machine, read before anything runs.
pub fn env() -> Value {
    json!({
        "engine": "bevy",
        "engine_version": BEVY_VERSION,
        "rustc": command_line("rustc", &["--version"]),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": command_line("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "logical_cores": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "debug_assertions": cfg!(debug_assertions),
        "load_average": load_average(),
    })
}

/// Writes `records` (the `ponko_bench` record shape) to `PONKO_BENCH_JSON`.
pub fn write_results(group: &str, env: Value, records: Vec<Value>) {
    if let Some(path) = std::env::var_os("PONKO_BENCH_JSON") {
        let document = json!({ "schema_version": 1, "group": group, "env": env, "results": records });
        write(&path, "JSON", format!("{document:#}\n"));
    }
}

/// A record of frame times in milliseconds.
fn record(name: &str, mut times: Vec<f64>, adapter: Option<String>) -> Value {
    let d = Dist::of(&mut times);
    println!(
        "{name:<32} median {:>9.3} ms   p95 {:>9.3} ms   min {:>9.3} ms",
        d.p50, d.p95, d.min
    );
    json!({
        "name": name, "kind": "steady", "warmup": WARMUP, "runs": d.samples,
        "min_ms": d.min, "p50_ms": d.p50, "p95_ms": d.p95, "p99_ms": d.p99, "max_ms": d.max,
        "mean_ms": d.mean, "adapter": adapter,
    })
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

// ---------------------------------------------------------------- headless

#[derive(Component, Clone, Copy)]
struct SimPos(Vec2);
#[derive(Component, Clone, Copy)]
struct SimVel(Vec2);

fn sim_move(time: Res<Time>, mut q: Query<(&mut SimPos, &SimVel)>) {
    // The tick's fixed 1/60 s, as Ponko's headless tick (Time's delta is
    // the wall clock here, which would change the work's numbers only).
    let _ = time;
    let dt = 1.0 / 60.0;
    q.iter_mut().for_each(|(mut p, v)| p.0 += v.0 * dt);
}

fn sim() -> Value {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_systems(Update, sim_move);
    app.world_mut().spawn_batch((0..1_000_000).map(|i| {
        let f = i as f32;
        (
            SimPos(Vec2::new(f, 0.0)),
            SimVel(Vec2::new(1.0 + (f * 0.37).sin(), (f * 0.11).cos())),
        )
    }));
    app.finish();
    app.cleanup();
    let mut times = Vec::new();
    for frame in 0..WARMUP + 300 {
        let start = Instant::now();
        app.update();
        if frame >= WARMUP {
            times.push(ms(start.elapsed()));
        }
    }
    record("compare/sim-1m", times, None)
}

fn physics() -> Value {
    use bevy_rapier2d::prelude::*;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(50.0),
    ))
    .insert_resource(TimestepMode::Fixed {
        dt: 1.0 / 60.0,
        substeps: 1,
    });
    app.finish();
    app.cleanup();
    // The default context exists after the first update.
    app.update();
    for mut config in app
        .world_mut()
        .query::<&mut RapierConfiguration>()
        .iter_mut(app.world_mut())
    {
        config.gravity = Vec2::new(0.0, -980.0);
    }
    // Ponko's scene (`crates/ponko_bench/src/physics.rs`, y down), with y
    // flipped for Bevy's y up.
    let n = 10_000;
    let per_row = (n as f32).sqrt().ceil() as usize * 2;
    let width = per_row as f32 * 12.0;
    let world = app.world_mut();
    let mut wall = |x: f32, y: f32, w: f32, h: f32| {
        world.spawn((
            Transform::from_xyz(x, -y, 0.0),
            RigidBody::Fixed,
            Collider::cuboid(w / 2.0, h / 2.0),
        ));
    };
    wall(0.0, 100.0, width + 40.0, 20.0);
    wall(-width / 2.0 - 10.0, -500.0, 20.0, 1200.0);
    wall(width / 2.0 + 10.0, -500.0, 20.0, 1200.0);
    world.spawn_batch((0..n).map(move |i| {
        let x = (i % per_row) as f32 * 12.0 - width / 2.0 + 6.0;
        let y = -((i / per_row) as f32) * 12.0;
        (
            Transform::from_xyz(x, -y, 0.0),
            RigidBody::Dynamic,
            Collider::cuboid(5.0, 5.0),
        )
    }));
    let mut times = Vec::new();
    for frame in 0..WARMUP + 300 {
        let start = Instant::now();
        app.update();
        if frame >= WARMUP {
            times.push(ms(start.elapsed()));
        }
    }
    let world = app.world_mut();
    let bodies = world.query::<&RigidBody>().iter(world).count();
    assert_eq!(bodies, n + 3, "every box is a rigid body");
    record("compare/physics-10k", times, None)
}

// ---------------------------------------------------------------- on the GPU

/// An app of `DefaultPlugins` with no window, drawing on one thread, and
/// the 1280 x 720 image every GPU row draws into.
fn gpu_app() -> (SubApps, Handle<Image>) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<PipelinedRenderingPlugin>()
            // No window, so no event loop (one per process at most).
            .disable::<bevy::winit::WinitPlugin>(),
    );
    app.finish();
    app.cleanup();
    let mut apps: SubApps = std::mem::take(app.sub_apps_mut());
    let world = apps.main.world_mut();
    let mut target = Image::new_target_texture(SIZE.0, SIZE.1, TextureFormat::Rgba8UnormSrgb, None);
    target.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    (apps, target)
}

/// A 2D camera drawing into `target` as Ponko's 2D pass does: no
/// multisampling, no tone mapping, black behind.
fn camera2d(world: &mut World, target: Handle<Image>) -> Entity {
    world
        .spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::from(target),
            Msaa::Off,
            Tonemapping::None,
        ))
        .id()
}

/// Runs the warm-up and `frames` measured frames, each until the GPU has
/// finished it, and returns their times.
fn frames(apps: &mut SubApps, frames: usize) -> Vec<f64> {
    let mut times = Vec::with_capacity(frames);
    for frame in 0..WARMUP + frames {
        let start = Instant::now();
        apps.update();
        wait_for_gpu(apps);
        if frame >= WARMUP {
            times.push(ms(start.elapsed()));
        }
    }
    times
}

fn wait_for_gpu(apps: &SubApps) {
    apps.main
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU finishes the frame");
}

fn adapter(apps: &SubApps) -> String {
    let info = apps.main.world().resource::<RenderAdapterInfo>();
    format!("{} ({:?})", info.name, info.backend)
}

/// The `bunnymark` velocity, in Bevy's y-up space.
#[derive(Component)]
struct Bounce(Vec2);

fn rand(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 40) as f32 / (1u64 << 24) as f32
}

/// Ponko's `bunnymark` movement mirrored for y up: the floor at the bottom
/// edge, 1/60 s a frame.
fn bounce(mut q: Query<(&mut Transform, &mut Bounce)>) {
    let dt = 1.0 / 60.0;
    let (hw, hh) = (SIZE.0 as f32 / 2.0, SIZE.1 as f32 / 2.0);
    q.iter_mut().for_each(|(mut t, mut v)| {
        v.0.y -= 600.0 * dt;
        let step = v.0 * dt;
        let p = &mut t.translation;
        p.x += step.x;
        p.y += step.y;
        if p.x < -hw || p.x > hw {
            p.x = p.x.clamp(-hw, hw);
            v.0.x = -v.0.x;
        }
        if p.y < -hh {
            p.y = -hh;
            v.0.y = v.0.y.abs() * 0.85;
        }
        if p.y > hh {
            p.y = hh;
            v.0.y = 0.0;
        }
    });
}

fn sprites(name: &str, n: usize, tinted: bool, count: usize) -> Value {
    let (mut apps, target) = gpu_app();
    apps.main.add_systems(Update, bounce);
    let world = apps.main.world_mut();
    camera2d(world, target);
    let image = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
        Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    // The same sequence as Ponko's side, in y-up coordinates.
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let batch: Vec<_> = (0..n)
        .map(|_| {
            let (x, y) = (rand(&mut seed) * w, rand(&mut seed) * h);
            let vel = Vec2::new(rand(&mut seed) * 400.0 - 200.0, rand(&mut seed) * 400.0 - 300.0);
            let color = Color::srgb(
                0.6 + rand(&mut seed) * 0.4,
                0.6 + rand(&mut seed) * 0.4,
                0.6 + rand(&mut seed) * 0.4,
            );
            let color = if tinted { color } else { Color::WHITE };
            (
                Sprite {
                    image: image.clone(),
                    color,
                    ..default()
                },
                Transform::from_xyz(x - w / 2.0, h / 2.0 - y, 0.0),
                Bounce(Vec2::new(vel.x, -vel.y)),
            )
        })
        .collect();
    world.spawn_batch(batch);
    let times = frames(&mut apps, count);
    record(name, times, Some(adapter(&apps)))
}

#[derive(Component)]
struct Cell(usize);

fn box_color(i: usize, frame: u64) -> Color {
    let t = (i as u64 + frame) % 256;
    Color::srgb(t as f32 / 255.0, 0.4, 1.0 - t as f32 / 255.0)
}

fn recolor(mut frame: Local<u64>, mut q: Query<(&Cell, &mut BackgroundColor)>) {
    *frame += 1;
    let f = *frame;
    q.iter_mut()
        .for_each(|(cell, mut color)| color.0 = box_color(cell.0, f));
}

fn ui(name: &str, recolors: bool) -> Value {
    const SIDE: usize = 100;
    let (mut apps, target) = gpu_app();
    if recolors {
        apps.main.add_systems(Update, recolor);
    }
    let world = apps.main.world_mut();
    let camera = camera2d(world, target);
    let root = world
        .spawn((
            Node {
                width: Val::Px(SIZE.0 as f32),
                height: Val::Px(SIZE.1 as f32),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(SIDE as u16, 1.0),
                grid_template_rows: RepeatedGridTrack::flex(SIDE as u16, 1.0),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();
    let cells: Vec<_> = (0..SIDE * SIDE)
        .map(|i| {
            (
                Node::default(),
                BackgroundColor(box_color(i, 0)),
                Cell(i),
                ChildOf(root),
            )
        })
        .collect();
    world.spawn_batch(cells);
    let times = frames(&mut apps, 300);
    record(name, times, Some(adapter(&apps)))
}

/// One sprite, one frame finished on the GPU, then exit: what the process
/// takes from its start to a first frame.
fn first_frame() {
    let started = Instant::now();
    let (mut apps, target) = gpu_app();
    let world = apps.main.world_mut();
    camera2d(world, target);
    world.spawn((Sprite::from_color(Color::WHITE, Vec2::splat(32.0)), Transform::IDENTITY));
    apps.update();
    wait_for_gpu(&apps);
    println!("first frame {:.1} ms after main started", ms(started.elapsed()));
}

pub fn run(group: &str, filter: &str) {
    let env = env();
    // A full row name keeps only that row; anything else is a substring.
    let keep = |name: &str| {
        filter.split('|').any(|part| {
            if part.starts_with("compare/") {
                part == name
            } else {
                name.contains(part)
            }
        })
    };
    let mut records = Vec::new();
    match group {
        "sim" => records.push(sim()),
        "physics" => records.push(physics()),
        "sprites" => {
            for (name, n, tinted, count) in [
                ("compare/sprites-100k", 100_000, false, 300),
                ("compare/sprites-100k-tinted", 100_000, true, 300),
                ("compare/sprites-1m", 1_000_000, false, 120),
            ] {
                if keep(name) {
                    records.push(sprites(name, n, tinted, count));
                }
            }
        }
        "ui" => {
            for (name, recolors) in [("compare/ui-10k-still", false), ("compare/ui-10k-recolor", true)] {
                if keep(name) {
                    records.push(ui(name, recolors));
                }
            }
        }
        "first-frame" => {
            first_frame();
            // Exit at once, as Ponko's side does after its frame.
            std::process::exit(0);
        }
        _ => unreachable!("main checks the group"),
    }
    write_results(group, env, records);
}
