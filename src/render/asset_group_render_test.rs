//! Asset groups on a real GPU, drawn offscreen: a group's textures are made
//! on the GPU and draw, and the group counts the time and bytes of those
//! uploads (`GroupProgress::upload_ms`, `GroupStats::upload_ms`). Skips
//! without a GPU (a skip is not a pass).
use super::render_test_support as support;
use crate::assets::{AssetGroup, GroupState, MemorySource};
use crate::ctx::Ctx;
use crate::prelude::*;

const SIZE: u32 = 64;

fn png(width: u32, height: u32, mut pixel: impl FnMut(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(pixel(x, y)));
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// A PNG of noise, which decodes and uploads as a photo would.
fn noise_png(side: u32, seed: u32) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9) | 1;
    png(side, side, |_, _| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        [s as u8, (s >> 8) as u8, (s >> 16) as u8, 255]
    })
}

/// Draws a frame and reads it back (through a screenshot, which waits for
/// the GPU to finish the frame).
fn frame(ctx: &mut Ctx, name: &str) -> image::RgbaImage {
    let path = support::output_dir().join(format!("asset-group-{name}-{}.png", std::process::id()));
    ctx.gfx.save_screenshot(&path);
    assert!(ctx.render_frame().drew());
    let image = image::open(&path).expect("the screenshot was written").to_rgba8();
    let _ = std::fs::remove_file(&path);
    image
}

fn red(p: &image::Rgba<u8>) -> bool {
    p[0] > 200 && p[1] < 40 && p[2] < 40
}

fn load(ctx: &mut Ctx, id: crate::assets::GroupId) {
    for _ in 0..10_000 {
        ctx.pump_asset_groups();
        if ctx.assets.group_state(id) != GroupState::Loading {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the group never finished");
}

#[test]
fn group_textures_are_made_on_the_gpu_draw_and_count_their_upload_cost() {
    let Some(mut ctx) = support::offscreen_ctx((SIZE, SIZE)) else {
        return;
    };
    ctx.scene3d.sky.atmosphere = false;
    ctx.gfx.clear(Color::BLACK);
    let mut source = MemorySource::new("mem").with("red.png", png(SIZE, SIZE, |_, _| [255, 0, 0, 255]));
    for i in 0..3 {
        source.insert(format!("noise{i}.png"), noise_png(256, i));
    }
    ctx.assets.add_background_source(source);
    let group = AssetGroup::new("art")
        .texture("red.png")
        .texture("noise0.png")
        .texture("noise1.png")
        .texture("noise2.png");
    let id = ctx.assets.start_group(group);

    // The negative control: nothing red before the group's texture draws.
    let before = frame(&mut ctx, "before");
    assert!(
        !red(before.get_pixel(SIZE / 2, SIZE / 2)),
        "{:?}",
        before.get_pixel(SIZE / 2, SIZE / 2)
    );

    load(&mut ctx, id);
    assert_eq!(
        ctx.assets.group_state(id),
        GroupState::Ready,
        "{:?}",
        ctx.assets.group_error(id)
    );
    let texture = ctx.gfx.load_texture("red.png");
    ctx.gfx.draw(texture, vec2(SIZE as f32 / 2.0, SIZE as f32 / 2.0));
    let after = frame(&mut ctx, "after");
    let covered = after.pixels().filter(|p| red(p)).count() as f32 / (SIZE * SIZE) as f32;
    assert!(covered > 0.95, "the group's texture covers the frame: {covered}");

    let progress = ctx.assets.group_progress(id);
    let bytes = u64::from(SIZE * SIZE * 4) + 3 * 256 * 256 * 4;
    assert_eq!(progress.bytes_uploaded, bytes);
    assert!(progress.upload_ms > 0.0, "making textures on a GPU takes time");
    let stats = ctx.assets.group_stats();
    assert_eq!(stats.total_upload_bytes, bytes);
    assert!(stats.max_upload_ms > 0.0 && stats.max_upload_ms <= stats.max_step_ms);
    eprintln!(
        "uploaded {bytes} bytes in {:.3} ms of game-thread time (longest step's share {:.3} ms)",
        progress.upload_ms, stats.max_upload_ms
    );
}

/// An offscreen game whose device has timestamp queries where the adapter
/// does, as the production renderer asks for them.
fn timed_ctx() -> Option<Ctx> {
    let gpu = support::gpu_with(|adapter| {
        Ok(wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })
    })?;
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(SIZE as f32, SIZE as f32));
    ctx.audio.headless = true;
    ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, (SIZE, SIZE)));
    Some(ctx)
}

#[test]
fn uploads_report_the_gpus_own_copy_time_where_it_has_timestamps_and_still_draw() {
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.scene3d.sky.atmosphere = false;
    ctx.gfx.clear(Color::BLACK);
    let blue = |_, _| [0, 0, 255, 255];
    let mut source = MemorySource::new("mem")
        .with("red.png", png(SIZE, SIZE, |_, _| [255, 0, 0, 255]))
        .with("blue.png", png(SIZE, SIZE, blue))
        .with("noise.png", noise_png(512, 7));
    // More uploads for the collecting below: Metal now and then leaves a
    // timestamp unwritten, and a step whose copies all lost one has no time.
    const MORE: u32 = 16;
    for i in 0..MORE {
        source.insert(format!("more{i}.png"), noise_png(64, i + 1));
    }
    ctx.assets.add_background_source(source);
    let id = ctx
        .assets
        .start_group(AssetGroup::new("art").texture("red.png").texture("noise.png"));
    load(&mut ctx, id);
    assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
    // A streamed texture, through the streaming step's uploads.
    assert_eq!(ctx.gfx.stream_texture("blue.png"), None);
    let mut streamed = None;
    for _ in 0..1000 {
        ctx.pump_streaming();
        streamed = ctx.gfx.stream_texture("blue.png");
        let state = ctx
            .gfx
            .asset_state(crate::streaming::ResidencyKind::Texture, "blue.png");
        if state == crate::streaming::AssetState::Resident {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let streamed = streamed.expect("the streamed texture loaded");

    // The timed uploads still carry the pixels (the negative control: the
    // frame before them was black).
    let texture = ctx.gfx.load_texture("red.png");
    let half = SIZE as f32 / 2.0;
    ctx.gfx.draw(texture, vec2(half, half));
    let red_frame = frame(&mut ctx, "timed-red");
    let covered = red_frame.pixels().filter(|p| red(p)).count() as f32 / (SIZE * SIZE) as f32;
    assert!(covered > 0.95, "the group's texture covers the frame: {covered}");
    ctx.gfx.draw(streamed, vec2(half, half));
    let blue_frame = frame(&mut ctx, "timed-blue");
    let p = blue_frame.get_pixel(SIZE / 2, SIZE / 2);
    assert!(p[2] > 200 && p[0] < 40, "the streamed texture draws: {p:?}");

    // The times come back a few frames later. A step whose timestamps the
    // GPU didn't write has none, so while one is missing, upload a little
    // more through that step (a bounded count, not a wait on the clock).
    let mut groups = ctx.assets.group_stats();
    let mut streams = ctx.gfx.upload_stats();
    let (mut more_groups, mut more_streams) = (0, 0);
    for i in 0..400 {
        if groups.gpu_upload_ms.is_some() && streams.gpu_upload_ms.is_some() {
            break;
        }
        if i % 25 == 24 {
            if groups.gpu_upload_ms.is_none() && more_groups < MORE / 2 {
                let more = AssetGroup::new(format!("more{more_groups}"))
                    .texture(&format!("more{more_groups}.png"));
                more_groups += 1;
                let id = ctx.assets.start_group(more);
                load(&mut ctx, id);
            }
            if streams.gpu_upload_ms.is_none() && more_streams < MORE / 2 {
                let _ = ctx.gfx.stream_texture(&format!("more{}.png", MORE / 2 + more_streams));
                more_streams += 1;
            }
        }
        ctx.pump_asset_groups();
        frame(&mut ctx, &format!("collect{i}"));
        ctx.pump_streaming();
        groups = ctx.assets.group_stats();
        streams = ctx.gfx.upload_stats();
    }
    let adapter = &ctx.gfx.renderer.as_ref().unwrap().capabilities().adapter;
    assert_eq!(groups.gpu_upload_timing, streams.gpu_upload_timing);
    if !groups.gpu_upload_timing {
        // Said so, and nothing made up.
        assert_eq!((groups.gpu_upload_ms, streams.gpu_upload_ms), (None, None));
        eprintln!(
            "{} ({}) has no timestamp queries: uploads not timed on the GPU",
            adapter.name, adapter.backend
        );
        return;
    }
    let (group_ms, stream_ms) = (groups.gpu_upload_ms.unwrap(), streams.gpu_upload_ms.unwrap());
    assert!(group_ms > 0.0 && stream_ms > 0.0, "{group_ms} {stream_ms}");
    assert!(groups.total_gpu_upload_ms >= f64::from(group_ms));
    eprintln!(
        "{} ({}): the GPU copied the group's {} bytes in {group_ms:.3} ms (game thread {:.3} ms); \
         the last streamed upload in {stream_ms:.3} ms",
        adapter.name, adapter.backend, groups.total_upload_bytes, groups.total_upload_ms
    );
}

/// What a group's uploads cost on a real GPU: the game thread's time per
/// step (`GroupStats::upload_ms`), and how much longer a frame takes to
/// finish on the GPU when it carries a step's uploads. Run on purpose, on a
/// quiet machine:
/// `cargo test -p ponko --lib --features 3d group_upload_cost_on_the_gpu -- --ignored --nocapture`.
#[test]
#[ignore = "a timing: run on purpose, on a quiet machine"]
fn group_upload_cost_on_the_gpu() {
    const TEXTURES: u32 = 16;
    const SIDE: u32 = 1024;
    let Some(mut ctx) = timed_ctx() else {
        return;
    };
    ctx.scene3d.sky.atmosphere = false;
    let mut source = MemorySource::new("mem");
    let mut group = AssetGroup::new("big");
    for i in 0..TEXTURES {
        let name = format!("t{i}.png");
        source.insert(name.clone(), noise_png(SIDE, i + 1));
        group = group.texture(&name);
    }
    ctx.assets.add_background_source(source);
    // Frames with nothing to upload, for the baseline.
    let mut empty = Vec::new();
    for i in 0..30 {
        let started = std::time::Instant::now();
        frame(&mut ctx, &format!("empty{i}"));
        empty.push(started.elapsed().as_secs_f32() * 1000.0);
    }
    let id = ctx.assets.start_group(group);
    let mut loaded = Vec::new();
    while ctx.assets.group_state(id) == GroupState::Loading {
        ctx.pump_asset_groups();
        // As every frame does: takes the GPU's times that came back.
        ctx.pump_streaming();
        let stats = ctx.assets.group_stats();
        let started = std::time::Instant::now();
        frame(&mut ctx, "loading");
        let frame_ms = started.elapsed().as_secs_f32() * 1000.0;
        if stats.frame_bytes > 0 {
            loaded.push((stats.upload_ms, stats.step_ms, stats.frame_bytes, frame_ms));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
    // The GPU's own copy times come back a few frames later.
    for i in 0..10 {
        frame(&mut ctx, &format!("after{i}"));
        ctx.pump_streaming();
    }
    empty.sort_by(f32::total_cmp);
    let base = empty[empty.len() / 2];
    let stats = ctx.assets.group_stats();
    let mib = stats.total_upload_bytes as f64 / (1024.0 * 1024.0);
    let adapter = &ctx.gfx.renderer.as_ref().unwrap().capabilities().adapter;
    println!(
        "{TEXTURES} textures of {SIDE} x {SIDE}, {mib:.0} MiB on {} ({}, {})",
        adapter.name, adapter.backend, adapter.driver
    );
    println!(
        "game thread: {:.2} ms uploading in all, {:.3} ms per MiB, longest step's share {:.2} ms",
        stats.total_upload_ms,
        stats.total_upload_ms / mib,
        stats.max_upload_ms
    );
    if stats.gpu_upload_timing {
        println!(
            "GPU's own copy time (timestamps): {:.2} ms in all, {:.3} ms per MiB, latest step {:.3} ms",
            stats.total_gpu_upload_ms,
            stats.total_gpu_upload_ms / mib,
            stats.gpu_upload_ms.unwrap_or(0.0)
        );
    } else {
        println!("this GPU has no timestamp queries: copies not timed");
    }
    println!("frame drawn and read back with nothing uploaded: median {base:.2} ms");
    for (upload_ms, step_ms, bytes, frame_ms) in &loaded {
        println!(
            "step uploading {:.0} MiB: step {step_ms:.2} ms (uploads {upload_ms:.2} ms), then its frame {frame_ms:.2} ms \
             (+{:.2} ms on the GPU's side)",
            *bytes as f64 / (1024.0 * 1024.0),
            frame_ms - base
        );
    }
}
