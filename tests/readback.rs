//! Reading GPU buffers and render targets back on the CPU
//! (`Graphics::read_gpu_buffer`, `read_render_target`, `readback`) where no
//! GPU runs: headless, every read fails with `Unsupported` on the next
//! frame, is used up the frame after, and misuse panics with a fix. The
//! GPU side is `src/render/readback_render_test.rs`.
#![cfg(feature = "3d")]

use ponko::prelude::*;
use ponko::render::Readback;

#[derive(Default)]
struct Seen {
    read: Option<Readback>,
    answers: Vec<(u64, Option<Result<Vec<Vec4>, ErrorKind>>)>,
}

#[test]
fn headless_reads_fail_as_unsupported_on_the_next_frame_then_are_used_up() {
    let world = Game::new("readback", 1, 1)
        .headless_frames(true)
        .setup(|ctx| {
            let buffer = ctx.gfx.create_gpu_buffer(4, &[Vec4::ONE; 4]);
            let read = ctx.gfx.read_gpu_buffer(buffer, 1..3);
            ctx.world.insert_resource(Seen {
                read: Some(read),
                ..Seen::default()
            });
        })
        .system(|ctx| {
            let read = ctx.world.resource::<Seen>().read.unwrap();
            let answer = ctx
                .gfx
                .readback(read)
                .map(|a| a.map(<[Vec4]>::to_vec).map_err(|e| e.kind()));
            let frame = ctx.time.frame();
            ctx.world.resource_mut::<Seen>().answers.push((frame, answer));
        })
        .run_headless(3);
    let answers = &world.resource::<Seen>().answers;
    assert_eq!(answers.len(), 3, "{answers:?}");
    assert_eq!(answers[0].1, Some(Err(ErrorKind::Unsupported)), "{answers:?}");
    assert_eq!(answers[1].1, Some(Err(ErrorKind::BadHandle)), "used up: {answers:?}");
    assert_eq!(answers[2].1, Some(Err(ErrorKind::BadHandle)), "{answers:?}");
}

#[test]
fn a_read_is_not_answered_on_the_frame_it_was_asked() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let target = ctx.gfx.create_render_target(8, 8);
        let read = ctx.gfx.read_render_target(target, uvec2(6, 6), uvec2(2, 2));
        assert!(ctx.gfx.readback(read).is_none());
    });
}

#[test]
#[should_panic(expected = "read_gpu_buffer range must hold at least one element and fit")]
fn a_buffer_range_past_the_end_panics_with_a_fix() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let buffer = ctx.gfx.create_gpu_buffer(4, &[]);
        ctx.gfx.read_gpu_buffer(buffer, 2..5);
    });
}

#[test]
#[should_panic(expected = "read_gpu_buffer range must hold at least one element")]
fn an_empty_buffer_range_panics() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let buffer = ctx.gfx.create_gpu_buffer(4, &[]);
        ctx.gfx.read_gpu_buffer(buffer, 2..2);
    });
}

#[test]
#[should_panic(expected = "GPU buffer was freed or belongs to another game")]
fn reading_a_freed_buffer_panics() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let buffer = ctx.gfx.create_gpu_buffer(4, &[]);
        ctx.free(buffer);
        ctx.gfx.read_gpu_buffer(buffer, 0..1);
    });
}

#[test]
#[should_panic(expected = "read_render_target rectangle must be at least 1 x 1 and fit in the 8 x 8 target")]
fn a_rectangle_past_the_edge_panics_with_a_fix() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let target = ctx.gfx.create_render_target(8, 8);
        ctx.gfx.read_render_target(target, uvec2(7, 0), uvec2(2, 1));
    });
}

#[test]
#[should_panic(expected = "render target was freed or belongs to another game")]
fn reading_a_freed_render_target_panics() {
    Game::new("readback", 1, 1).run_headless(0, |ctx| {
        let target = ctx.gfx.create_render_target(8, 8);
        ctx.free(target);
        ctx.gfx.read_render_target(target, uvec2(0, 0), uvec2(1, 1));
    });
}
