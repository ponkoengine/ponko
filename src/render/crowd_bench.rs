//! Offscreen benchmark of crowds: 10,000 animated skinned characters as one
//! crowd (the GPU path, and the CPU path under WebGL2's limits) against the
//! same characters as 10,000 skinned models, with each pass's GPU time
//! (timestamps) and each frame's time. No window is opened. Run explicitly,
//! in release:
//! `cargo test --release --lib --features 3d crowd_benchmark -- --ignored --nocapture`
//! (`PONKO_CROWD_BENCH=<text>` keeps the scenarios whose name contains it).
//! The guide's "Crowds" chapter has the results and how to read them.
use super::crowd_render_test::{bend, crowd, figure_with, skinned_mesh, sway};
use super::gpu3d::StateChanges;
use super::render_test_support as support;
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::anim::Skeleton;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{CrowdAnimation, CrowdInstance, CrowdLevel, CrowdOptions, MaterialDesc, MeshData, Model};

const SIZE: (u32, u32) = (1280, 720);
const CHARACTERS: usize = 10_000;
const WARM_UP: usize = 10;
const FRAMES: usize = 60;
/// Each scenario is built and measured this many times.
const RUNS: usize = 3;
/// Face cuts of the near character: 1,296 triangles, 882 corners.
const CUTS: u32 = 6;
/// Metres between neighbouring characters.
const SPACING: f32 = 1.6;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Way {
    /// One crowd; `compute: false` takes the CPU path (WebGL2's limits).
    /// `fade`: the near level crossfades into the far one over 5 m.
    Crowd { compute: bool, levels: bool, fade: bool },
    /// One skinned model each, posed on the CPU every frame by the clips.
    Models,
    /// No characters, only the ground: the rest of the frame.
    Empty,
}

struct Scenario {
    name: &'static str,
    way: Way,
    shadows: bool,
}

/// One run's medians.
#[derive(Clone, Copy)]
struct Run {
    /// Posing the models' skeletons (CPU), in ms.
    animate: f64,
    prepare: f64,
    record: f64,
    /// From the start of preparing to the GPU being done, in ms.
    frame: f64,
    /// Preparing plus recording on one thread (no waiting for busy
    /// workers), in ms: the CPU work itself.
    serial: f64,
    /// All passes, and the crowd's cull pass, in ms.
    gpu: Option<(f64, f64)>,
    changes: StateChanges,
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn place(i: usize) -> Vec3 {
    let (x, z) = ((i % 100) as f32, (i / 100) as f32);
    vec3((x - 49.5) * SPACING, 0.0, 40.0 - z * SPACING)
}

/// Builds `s` and measures it. `None` without a GPU.
fn run(s: &Scenario) -> Option<Run> {
    let compute = !matches!(s.way, Way::Crowd { compute: false, .. });
    let mut rig = BatchRig::new(RigSetup {
        size: SIZE,
        compute,
        timestamps: true,
        ..Default::default()
    })?;
    rig.skins = s.way == Way::Models;
    let camera = &mut rig.ctx.scene3d.camera;
    camera.position = vec3(0.0, 30.0, 80.0);
    camera.look_at(vec3(0.0, 0.0, -20.0));
    camera.far = 400.0;
    rig.ctx.scene3d.lighting.shadows = s.shadows;
    // Every scenario stands on a ground, so every pass of a frame runs.
    let ground = MaterialDesc {
        color: Color::rgb(0.6, 0.6, 0.6),
        ..Default::default()
    };
    let plane = rig.mesh(&MeshData::plane(400.0), ground);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(plane)));
    let (skeleton, data) = figure_with(CUTS);
    let body = MaterialDesc {
        color: Color::rgb(0.9, 0.5, 0.2),
        ..Default::default()
    };
    let mesh = skinned_mesh(&mut rig, &data, body, &skeleton);
    let clips = [bend(&skeleton), sway(&skeleton)];
    let start = |i: usize| (i % 37) as f32 * 0.027;
    match s.way {
        Way::Crowd { levels, fade, .. } => {
            let animation = CrowdAnimation::bake(&skeleton, &[clips[0].clone().into(), clips[1].clone().into()], 30.0);
            let far = skinned_mesh(&mut rig, &figure_with(1).1, body, &skeleton);
            let levels = if levels {
                vec![
                    CrowdLevel::new(mesh, 0.0, 40.0).with_fade(if fade { 5.0 } else { 0.0 }),
                    CrowdLevel::new(far, 40.0, f32::INFINITY),
                ]
            } else {
                vec![CrowdLevel::new(mesh, 0.0, f32::INFINITY)]
            };
            let people: Vec<_> = (0..CHARACTERS)
                .map(|i| CrowdInstance::new(Mat4::from_translation(place(i))).playing((i % 2) as u16, start(i)))
                .collect();
            crowd(&mut rig, &animation, &levels, &people, CrowdOptions::default());
        }
        Way::Empty => {}
        Way::Models => {
            for i in 0..CHARACTERS {
                rig.ctx
                    .world
                    .spawn((Transform3D::at(place(i)), Model::new(mesh), skeleton.clone()));
            }
        }
    }
    let mut frames = 0usize;
    // Poses every model as the crowd's characters play (the clips loop).
    let mut animate = |rig: &mut BatchRig| {
        let now = frames as f32 / 60.0;
        frames += 1;
        if s.way != Way::Models {
            return 0.0;
        }
        let begin = std::time::Instant::now();
        let mut i = 0;
        rig.ctx.world.query::<&mut Skeleton>().for_each(|skeleton| {
            let clip = &clips[i % 2];
            clip.sample((start(i) + now).rem_euclid(clip.duration()), skeleton);
            i += 1;
        });
        begin.elapsed().as_secs_f64() * 1e3
    };
    for _ in 0..WARM_UP {
        animate(&mut rig);
        rig.frame();
    }
    let mut read = rig.timer.as_ref().map_or(0, |t| t.frames_read);
    let (mut posing, mut prepare, mut record, mut frame, mut gpu, mut cull) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut changes = StateChanges::default();
    for _ in 0..FRAMES {
        posing.push(animate(&mut rig));
        let cost = rig.frame();
        prepare.push(cost.prepare_ms);
        record.push(cost.record_ms);
        frame.push(cost.frame_ms);
        changes = cost.changes;
        if let Some(timer) = rig.timer.as_ref().filter(|t| t.frames_read != read) {
            read = timer.frames_read;
            let of = |name: Option<&str>| {
                timer
                    .last_frame
                    .iter()
                    .filter(|p| name.is_none_or(|n| p.name == n))
                    .map(|p| f64::from(p.ms))
                    .sum::<f64>()
            };
            gpu.push(of(None));
            cull.push(of(Some("crowd cull")));
        }
    }
    // The same frames on one thread: what preparing and recording cost
    // without waiting for workers on a busy machine.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().ok()?;
    let mut serial = Vec::new();
    for _ in 0..FRAMES / 2 {
        animate(&mut rig);
        let cost = rig.frame();
        serial.push(cost.prepare_ms + rig.recording_ms(&pool));
    }
    Some(Run {
        serial: median(&mut serial),
        animate: median(&mut posing),
        prepare: median(&mut prepare),
        record: median(&mut record),
        frame: median(&mut frame),
        gpu: (!gpu.is_empty()).then(|| (median(&mut gpu), median(&mut cull))),
        changes,
    })
}

/// "median (min–max)" of the runs' medians.
fn spread(values: impl Iterator<Item = f64>) -> String {
    let mut v: Vec<f64> = values.collect();
    let (min, max) = (
        v.iter().copied().fold(f64::INFINITY, f64::min),
        v.iter().copied().fold(0.0, f64::max),
    );
    format!("{:.2} ({min:.2}–{max:.2})", median(&mut v))
}

#[test]
#[ignore = "requires a GPU; run explicitly in release for numbers"]
fn crowd_benchmark() {
    let Some(probe) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    println!(
        "adapter: {}; target {}x{} Rgba8UnormSrgb, 4x MSAA, depth prepass; {CHARACTERS} characters of {} triangles \
         ({} corners, 2 joints, 2 looping clips of 1 s baked at 30 frames a second) {SPACING} m apart, seen from \
         30 m up, on a ground; {WARM_UP} warm-up + {FRAMES} measured frames, {RUNS} runs; median of the runs' medians (min–max)",
        support::describe_adapter(&probe.gpu.info),
        SIZE.0,
        SIZE.1,
        figure_with(CUTS).1.indices.len() / 3,
        figure_with(CUTS).1.positions.len(),
    );
    drop(probe);
    let gpu = |levels| Way::Crowd {
        compute: true,
        levels,
        fade: false,
    };
    let scenarios = [
        Scenario {
            name: "no characters",
            way: Way::Empty,
            shadows: false,
        },
        Scenario {
            name: "crowd, GPU path",
            way: gpu(false),
            shadows: false,
        },
        Scenario {
            name: "crowd, GPU path, sun shadows",
            way: gpu(false),
            shadows: true,
        },
        Scenario {
            name: "crowd, GPU path, 2 levels, sun shadows",
            way: gpu(true),
            shadows: true,
        },
        Scenario {
            name: "crowd, GPU path, 2 levels crossfading over 5 m, sun shadows",
            way: Way::Crowd {
                compute: true,
                levels: true,
                fade: true,
            },
            shadows: true,
        },
        Scenario {
            name: "crowd, CPU path (WebGL2 limits)",
            way: Way::Crowd {
                compute: false,
                levels: false,
                fade: false,
            },
            shadows: false,
        },
        Scenario {
            name: "skinned models",
            way: Way::Models,
            shadows: false,
        },
        Scenario {
            name: "skinned models, sun shadows",
            way: Way::Models,
            shadows: true,
        },
    ];
    println!(
        "| scenario | posing ms (CPU) | prepare ms | record ms | prepare + record ms, 1 thread | GPU ms | crowd cull GPU ms | frame ms | draws | pipelines | bind groups |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let only = std::env::var("PONKO_CROWD_BENCH").unwrap_or_default();
    for s in scenarios.iter().filter(|s| s.name.contains(&only)) {
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let Some(run) = run(s) else { return };
            runs.push(run);
        }
        let gpu = |k: usize| {
            let crowd = matches!(s.way, Way::Crowd { compute: true, .. });
            if runs.iter().all(|r| r.gpu.is_some()) && (k == 0 || crowd) {
                spread(
                    runs.iter()
                        .map(|r| r.gpu.map_or(0.0, |g| if k == 0 { g.0 } else { g.1 })),
                )
            } else {
                "n/a".into()
            }
        };
        let c = runs[0].changes;
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            s.name,
            spread(runs.iter().map(|r| r.animate)),
            spread(runs.iter().map(|r| r.prepare)),
            spread(runs.iter().map(|r| r.record)),
            spread(runs.iter().map(|r| r.serial)),
            gpu(0),
            gpu(1),
            spread(runs.iter().map(|r| r.frame)),
            c.draws,
            c.pipelines,
            c.bind_groups,
        );
    }
}
