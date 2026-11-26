//! Native runner shared by the reference examples. scripts/soak.py builds and
//! starts these binaries and evaluates their post-warm-up trends.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ponko::perf::{CountingAllocator, alloc_counts};
use ponko::prelude::*;
use serde_json::{Value, json};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Default)]
pub struct Work {
    // One completed walk-through (or one batch of terrain uploads/evictions).
    pub cycles: u64,
    pub saves: u64,
    pub loads: u64,
    pub deaths: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Headless,
    Offscreen,
    Windowed,
}

struct Run {
    output: BufWriter<File>,
    mode: Mode,
    duration: Duration,
    interval: Duration,
    next_sample: Duration,
    started: Option<Instant>,
    frame_times: Vec<f64>,
    allocations: u64,
    allocation_mark: u64,
    resize_phase: Option<u64>,
    resizes: u64,
    restores: u64,
    original_size: Vec2,
    adapter: Option<String>,
}

pub fn run(
    args: &[String],
    game: impl FnOnce(Game) -> ConfiguredGame,
    setup: impl FnOnce(&mut Ctx) + 'static,
    work: impl Fn(&Ctx) -> Work + 'static,
    title: &str,
    size: (u32, u32),
) {
    if let Err(error) = run_inner(args, game, setup, work, title, size) {
        eprintln!("soak: {error}");
        std::process::exit(1);
    }
}

fn run_inner(
    args: &[String],
    game: impl FnOnce(Game) -> ConfiguredGame,
    setup: impl FnOnce(&mut Ctx) + 'static,
    work: impl Fn(&Ctx) -> Work + 'static,
    title: &str,
    size: (u32, u32),
) -> Result<(), Box<dyn std::error::Error>> {
    let [mode, duration, interval, file, data] = args else {
        return Err("usage: --soak headless|offscreen|windowed seconds interval output.jsonl data-dir".into());
    };
    let mode = match mode.as_str() {
        "headless" => Mode::Headless,
        "offscreen" => Mode::Offscreen,
        "windowed" => Mode::Windowed,
        _ => return Err("invalid soak mode".into()),
    };
    let duration = Duration::from_secs(duration.parse()?);
    let interval = Duration::from_secs(interval.parse()?);
    if interval.is_zero() || duration < interval {
        return Err("require 0 < interval <= duration".into());
    }
    let original_size = vec2(size.0 as f32, size.1 as f32);
    let mut run = Run {
        output: BufWriter::new(File::create(file)?),
        mode,
        duration,
        interval,
        next_sample: interval,
        started: None,
        frame_times: Vec::new(),
        allocations: 0,
        allocation_mark: 0,
        resize_phase: None,
        resizes: 0,
        restores: 0,
        original_size,
        adapter: None,
    };

    let base = Game::new(title, size.0, size.1)
        .data_dir(PathBuf::from(data))
        .plugin(SamplerSetup(mode))
        .bench_after_frame(move |ctx, elapsed| {
            if let Err(error) = run.frame(ctx, elapsed, &work) {
                eprintln!("soak: {error}");
                std::process::exit(1);
            }
        });
    let game = game(base).setup(setup);

    match mode {
        Mode::Windowed => game.try_run()?,
        Mode::Offscreen => {
            game.bench_offscreen().try_run_headless(u64::MAX)?;
        }
        Mode::Headless => {
            game.headless_frames(true).try_run_headless(u64::MAX)?;
        }
    }
    Ok(())
}

struct SamplerSetup(Mode);

impl Plugin for SamplerSetup {
    fn build(&self, game: Game) -> Game {
        game
    }

    fn setup(&self, ctx: &mut Ctx) {
        if self.0 != Mode::Windowed {
            ctx.audio.open_offline(48_000);
        }
        ctx.perf.set_profiling(true);
    }
}

impl Run {
    fn frame(&mut self, ctx: &mut Ctx, elapsed: Duration, work: &impl Fn(&Ctx) -> Work) -> std::io::Result<()> {
        let frame_end = Instant::now();
        let started = *self.started.get_or_insert_with(|| {
            self.adapter = ctx.bench_adapter();
            self.allocation_mark = alloc_counts().count;
            frame_end - elapsed
        });
        let ran = frame_end.duration_since(started);
        if self.mode != Mode::Windowed {
            // Advance actual mixer voices by one frame without opening speakers.
            drop(ctx.audio.render_offline(ctx.time.frame_dt()));
        }
        self.allocations += alloc_counts().count.saturating_sub(self.allocation_mark);
        let frame_work = elapsed + frame_end.elapsed();
        self.frame_times.push(frame_work.as_secs_f64() * 1000.0);

        let finished = ran >= self.duration;
        if ran >= self.next_sample || finished {
            let sample = self.sample(ctx, ran, work(ctx));
            writeln!(self.output, "{sample}")?;
            self.output.flush()?;
            self.frame_times.clear();
            self.allocations = 0;
            while ran >= self.next_sample {
                self.next_sample += self.interval;
            }
        }
        if finished {
            ctx.quit();
            return Ok(());
        }
        self.resize(ctx, ran)?;
        self.allocation_mark = alloc_counts().count;
        if self.mode != Mode::Windowed {
            // Pace the fixed-tick game at its native rate (50 or 60 Hz).
            let tick = Duration::from_secs_f32(ctx.time.fixed_dt());
            std::thread::sleep(tick.saturating_sub(elapsed + frame_end.elapsed()));
        }
        Ok(())
    }

    fn resize(&mut self, ctx: &mut Ctx, elapsed: Duration) -> std::io::Result<()> {
        // Away from the sample boundaries, shrink, minimize and restore.
        // Start in the normal phase so setup gets the original dimensions.
        let shifted = elapsed.as_secs() + 15;
        let phase = shifted / 20 * 3
            + match shifted % 20 {
                0..=1 => 0,
                2..=3 => 1,
                _ => 2,
            };
        let previous = self.resize_phase.replace(phase);
        if previous.is_none() || previous == Some(phase) {
            return Ok(());
        }
        let size = match phase % 3 {
            0 => self.original_size * 0.75,
            1 => {
                if self.mode == Mode::Windowed {
                    ctx.bench_minimize_for(Duration::from_secs(2));
                } else {
                    ctx.bench_surface(None);
                }
                return Ok(());
            }
            _ => self.original_size,
        };
        if self.mode == Mode::Windowed {
            ctx.window.set_size(size).map_err(std::io::Error::other)?;
        } else {
            ctx.bench_surface(Some((size.x as u32, size.y as u32)));
        }
        if previous.is_some_and(|phase| phase % 3 == 1) {
            self.restores += 1;
        }
        self.resizes += 1;
        Ok(())
    }

    fn sample(&mut self, ctx: &Ctx, elapsed: Duration, progress: Work) -> Value {
        let handles = ctx.live_counts();
        let render = ctx.perf.render_stats();
        let voices = ctx.audio.voice_count();
        let memory = ctx.bench_gpu_memory();
        let frames = self.frame_times.len();
        self.frame_times.sort_unstable_by(f64::total_cmp);
        let percentile = |p: usize| self.frame_times[(frames * p).div_ceil(100) - 1];
        json!({
            "seconds": elapsed.as_secs_f64(),
            "frames_in_interval": frames,
            "draw_calls": render.draw_calls,
            "triangles": render.triangles,
            "frame_ms_p50": percentile(50),
            "frame_ms_p95": percentile(95),
            "frame_ms_p99": percentile(99),
            "allocations_per_frame": self.allocations as f64 / frames as f64,
            "audio_real": voices.real,
            "audio_virtual": voices.virtualized,
            "rss_bytes": rss_bytes(),
            "gpu_bytes": memory.as_ref().map(|m| m.total),
            "wgpu_bytes": memory.as_ref().and_then(|m| m.adapter_bytes),
            "adapter_memory_source": memory.map(|m| m.adapter_note),
            "handles_textures": handles.textures,
            "handles_render_targets": handles.render_targets,
            "handles_meshes": handles.meshes,
            "handles_materials": handles.materials,
            "handles_shaders": handles.shaders,
            "handles_computes": handles.computes,
            "handles_effects": handles.effects,
            "handles_gpu_buffers": handles.gpu_buffers,
            "handles_instance_batches": handles.instance_batches,
            "handles_instance_cells": handles.instance_cells,
            "handles_crowds": handles.crowds,
            "handles_texture_arrays": handles.texture_arrays,
            "handles_sounds": handles.sounds,
            "handles_fonts": handles.fonts,
            "handles_collision_meshes": handles.collision_meshes,
            "handles_sprite_shaders": handles.sprite_shaders,
            "handles_sprite_materials": handles.sprite_materials,
            "handles_sky_shaders": handles.sky_shaders,
            "adapter": self.adapter,
            "cycles": progress.cycles,
            "saves": progress.saves,
            "loads": progress.loads,
            "deaths": progress.deaths,
            "resizes": self.resizes,
            "restores": self.restores,
        })
    }
}

#[cfg(target_os = "linux")]
fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let kb: u64 = line.strip_prefix("VmRSS:")?.split_whitespace().next()?.parse().ok()?;
        Some(kb * 1024)
    })
}

#[cfg(target_os = "macos")]
fn rss_bytes() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kb: u64 = std::str::from_utf8(&output.stdout).ok()?.trim().parse().ok()?;
    output.status.success().then_some(kb * 1024)
}

#[cfg(target_os = "windows")]
fn rss_bytes() -> Option<u64> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let expression = format!("(Get-Process -Id {}).WorkingSet64", std::process::id());
    let output = std::process::Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &expression])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    std::str::from_utf8(&output.stdout).ok()?.trim().parse().ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn rss_bytes() -> Option<u64> {
    None
}
