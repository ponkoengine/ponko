//! Reading GPU buffers and render targets back on the CPU
//! (`Graphics::read_gpu_buffer`, `read_render_target`, `readback`), through
//! a game's own frames on the offscreen renderer: a compute pass's known
//! values come back a few frames later, while an untouched buffer and an
//! untouched render target read back what they were made with (the
//! negative controls). On a device with WebGL2's limits, a buffer read
//! fails and a render target read still works. Skips without a GPU.
use super::render_test_support::{TestGpu, gpu, gpu_with, needs};
use crate::prelude::*;
use crate::render::{ComputePass, GpuFeature, Readback};
use crate::time::FrameTiming;

/// The most frames a read may take to arrive.
const MOST_FRAMES: u32 = 30;

/// A game on `gpu`'s offscreen renderer (as `render_test_support::offscreen_ctx`).
fn game_on(gpu: TestGpu) -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(32.0, 32.0));
    ctx.audio.headless = true;
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, (32, 32)));
    ctx
}

/// Draws the frame the reads were asked in, then runs frames until `read`
/// is answered: the answer and how many frames later it came.
fn wait(ctx: &mut Ctx, read: Readback) -> (Result<Vec<Vec4>, ErrorKind>, u32) {
    ctx.render_frame();
    for frames in 1..=MOST_FRAMES {
        ctx.begin_frame(FrameTiming {
            dt: 1.0 / 60.0,
            seconds: 1.0 / 60.0,
            fixed_steps: 0,
            alpha: 0.0,
        });
        if let Some(answer) = ctx.gfx.readback(read) {
            return (answer.map(<[Vec4]>::to_vec).map_err(|e| e.kind()), frames);
        }
        ctx.render_frame();
    }
    panic!("no answer in {MOST_FRAMES} frames");
}

fn close(a: &[Vec4], b: &[Vec4], tolerance: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.abs_diff_eq(*b, tolerance))
}

#[test]
fn a_buffer_reads_back_what_its_compute_pass_wrote() {
    let Some(gpu) = gpu() else { return };
    if !needs(&gpu.device, GpuFeature::Compute) || !needs(&gpu.device, GpuFeature::StorageBuffers) {
        return;
    }
    let mut ctx = game_on(gpu);
    let start: Vec<Vec4> = (0..8).map(|i| Vec4::splat(i as f32 + 0.5)).collect();
    let written = ctx.gfx.create_gpu_buffer(8, &start);
    let untouched = ctx.gfx.create_gpu_buffer(8, &start);
    let write = ctx.gfx.create_compute(
        "write",
        "fn update(i: u32) { items[i] = vec4<f32>(f32(i) * 2.0, 7.0, -1.5, 0.25); }",
    );
    ctx.scene3d.passes.compute.push(ComputePass::for_buffer(write, written));
    let read = ctx.gfx.read_gpu_buffer(written, 2..6);
    let control = ctx.gfx.read_gpu_buffer(untouched, 2..6);
    assert!(
        ctx.gfx.readback(read).is_none(),
        "never answered on the frame it was asked"
    );
    let (values, frames) = wait(&mut ctx, read);
    let values = values.expect("the read arrives");
    eprintln!("buffer read arrived {frames} frame(s) later: {values:?}");
    let expected: Vec<Vec4> = (2..6).map(|i| Vec4::new(i as f32 * 2.0, 7.0, -1.5, 0.25)).collect();
    assert!(close(&values, &expected, 1e-6), "{values:?}");
    // The negative control: an untouched buffer reads back its initial data.
    let control = ctx.gfx.readback(control).expect("asked together, answered together");
    assert!(close(control.unwrap(), &start[2..6], 1e-6));
    // The answer stays for the rest of the frame, then the handle is used up.
    assert!(ctx.gfx.readback(read).is_some_and(|a| a.is_ok()));
    ctx.render_frame();
    ctx.begin_frame(FrameTiming {
        dt: 1.0 / 60.0,
        seconds: 1.0 / 60.0,
        fixed_steps: 0,
        alpha: 0.0,
    });
    let used = ctx.gfx.readback(read).expect("answered").unwrap_err();
    assert_eq!(used.kind(), ErrorKind::BadHandle);

    // Reading again each frame reuses the staging buffers.
    let staging = ctx.gfx.renderer.as_ref().unwrap().readback_staging_buffers();
    for _ in 0..4 {
        let again = ctx.gfx.read_gpu_buffer(written, 2..6);
        let (values, _) = wait(&mut ctx, again);
        assert!(close(&values.unwrap(), &expected, 1e-6));
    }
    assert_eq!(ctx.gfx.renderer.as_ref().unwrap().readback_staging_buffers(), staging);
}

#[test]
fn a_render_target_reads_back_a_rectangle_of_its_texels() {
    let Some(gpu) = gpu() else { return };
    if !needs(&gpu.device, GpuFeature::Compute) {
        return;
    }
    let mut ctx = game_on(gpu);
    let target = ctx.gfx.create_render_target(300, 8);
    let untouched = ctx.gfx.create_render_target(4, 4);
    let coordinates = ctx.gfx.create_compute(
        "coordinates",
        "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(f32(c.pixel.x), f32(c.pixel.y), 0.5, 1.0); }",
    );
    ctx.scene3d.passes.compute.push(ComputePass::new(coordinates, target));
    // A rectangle whose rows (3 texels, 24 bytes) are padded in the copy,
    // and one as wide as a target whose rows aren't a multiple of 256 bytes.
    let read = ctx.gfx.read_render_target(target, uvec2(1, 2), uvec2(3, 2));
    let row = ctx.gfx.read_render_target(target, uvec2(0, 7), uvec2(300, 1));
    let control = ctx.gfx.read_render_target(untouched, uvec2(0, 0), uvec2(4, 4));
    let (values, frames) = wait(&mut ctx, read);
    let values = values.expect("the read arrives");
    eprintln!("render target read arrived {frames} frame(s) later: {values:?}");
    let expected: Vec<Vec4> = (2..4)
        .flat_map(|y| (1..4).map(move |x| Vec4::new(x as f32, y as f32, 0.5, 1.0)))
        .collect();
    assert!(close(&values, &expected, 1e-3), "{values:?}");
    let row = ctx.gfx.readback(row).expect("answered with the first").unwrap();
    assert_eq!(row.len(), 300);
    assert!(
        row.iter()
            .enumerate()
            .all(|(x, v)| (v.x - x as f32).abs() < 1e-3 && v.y == 7.0)
    );
    // The negative control: an untouched target reads back zeros.
    let control = ctx.gfx.readback(control).expect("answered with the first").unwrap();
    assert_eq!(control, [Vec4::ZERO; 16]);
}

#[test]
fn with_webgl2_limits_a_buffer_read_fails_and_a_target_read_works() {
    let Some(gpu) = gpu_with(|_| {
        Ok(wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            ..Default::default()
        })
    }) else {
        return;
    };
    let mut ctx = game_on(gpu);
    let start = [Vec4::new(1.0, 2.0, 3.0, 4.0); 4];
    let buffer = ctx.gfx.create_gpu_buffer(4, &start);
    let target = ctx.gfx.create_render_target(4, 4);
    let read = ctx.gfx.read_gpu_buffer(buffer, 0..4);
    let texels = ctx.gfx.read_render_target(target, uvec2(1, 1), uvec2(2, 2));
    let (answer, _) = wait(&mut ctx, read);
    assert_eq!(answer.unwrap_err(), ErrorKind::Unsupported);
    let (texels, _) = wait(&mut ctx, texels);
    assert_eq!(texels.unwrap(), [Vec4::ZERO; 4]);
}

#[test]
fn a_grid_reduced_on_the_gpu_reads_back_as_a_few_counts() {
    let Some(gpu) = gpu() else { return };
    if !needs(&gpu.device, GpuFeature::Compute) {
        return;
    }
    let mut ctx = game_on(gpu);
    // The guide's pattern ("Reading compute results on the CPU"): a 64 x 64
    // fire grid, burning where x < 20, adds up into 4 x 4 regions.
    let fire = ctx.gfx.create_render_target(64, 64);
    let burn = ctx.gfx.create_compute(
        "burn",
        "fn compute(c: Cell) -> vec4<f32> { return vec4<f32>(select(0.0, 1.0, c.pixel.x < 20), 0.0, 0.0, 1.0); }",
    );
    let regions = ctx.gfx.create_render_target(4, 4);
    let count = ctx.gfx.create_compute(
        "count fire",
        "fn compute(c: Cell) -> vec4<f32> {
            var burning = 0.0;
            for (var y = 0; y < 16; y++) {
                for (var x = 0; x < 16; x++) {
                    burning += textureLoad(base_texture, c.pixel * 16 + vec2<i32>(x, y), 0).x;
                }
            }
            return vec4<f32>(burning, 0.0, 0.0, 0.0);
        }",
    );
    let grid = ctx
        .gfx
        .create_material(crate::render::MaterialDesc::default().with_texture(fire.texture()));
    ctx.scene3d.passes.compute.push(ComputePass::new(burn, fire));
    ctx.scene3d
        .passes
        .compute
        .push(ComputePass::new(count, regions).with_material(grid));
    let read = ctx.gfx.read_render_target(regions, uvec2(0, 0), uvec2(4, 4));
    let (counts, frames) = wait(&mut ctx, read);
    let counts: Vec<f32> = counts.expect("the read arrives").iter().map(|t| t.x).collect();
    eprintln!("burning per region, {frames} frame(s) later: {counts:?}");
    // Columns 0..16 burn whole (256 cells), 16..20 a quarter (64); the
    // regions right of the fire (the negative control) count none.
    for row in counts.chunks(4) {
        assert_eq!(row, [256.0, 64.0, 0.0, 0.0], "{counts:?}");
    }
}
