//! Steady debug drawing allocates nothing: once the buffers have grown,
//! drawing the same shapes, labels and built-in views each frame and
//! handing them over as a drawn frame does (3D lines to the GPU list, 2D
//! lines to sprites, depth-tested labels to the 3D text) makes no heap
//! allocation.
//!
//! One test per binary, and a thread-local counter: only the thread running
//! the game counts, so other threads in the process can't disturb the
//! zero-allocation assertion. The hand-over is `Ctx::bench_debug_frame`
//! (feature `bench-internals`, which the workspace's benchmark crate turns on).
#![cfg(feature = "bench-internals")]

use ponko::debug::{DebugDraw, DebugView};
use ponko::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// A frame's drawing: every kind of shape, 2D and 3D, labels of each kind.
fn draw(d: &mut DebugDraw) {
    for i in 0..200 {
        let f = i as f32;
        d.line(vec2(f, 0.0), vec2(f, 10.0), Color::GREEN);
        d.line(vec2(f, 0.0), vec2(f, 10.0), Color::RED).screen().on_top();
        d.line(vec3(f, 0.0, 0.0), vec3(f, 1.0, 0.0), Color::BLUE).width(2.0);
        d.arrow(vec3(f, 0.0, 1.0), vec3(f, 1.0, 1.0), Color::YELLOW).on_top();
    }
    d.circle(vec2(50.0, 50.0), 20.0, Color::WHITE);
    d.capsule(vec2(0.0, 0.0), vec2(0.0, 30.0), 5.0, Color::WHITE);
    d.polyline(&[vec2(0.0, 0.0), vec2(5.0, 5.0), vec2(9.0, 0.0)], true, Color::WHITE);
    d.grid(Vec3::ZERO, 1.0, 8, Color::WHITE);
    d.sphere(Vec3::Y, 0.5, Color::WHITE);
    d.obb(Vec3::ZERO, Vec3::ONE, Quat::from_rotation_y(0.3), Color::WHITE);
    d.axes(Vec3::ZERO, Quat::IDENTITY, 1.0);
    d.text(vec2(10.0, 10.0), "a 2D label", Color::WHITE).screen();
    d.text(vec2(10.0, 30.0), "a world label", Color::WHITE);
    d.text(vec3(0.0, 2.0, 0.0), "a 3D label on top", Color::WHITE);
    d.text(vec3(0.0, 1.0, 0.0), "a depth-tested 3D label", Color::WHITE)
        .depth_tested();
}

#[test]
fn steady_debug_drawing_and_its_views_allocate_nothing() {
    let mut allocations = None;
    Game::new("debug allocations", 320, 240)
        .debug_draw(true)
        .run_headless(0, |ctx| {
            for view in DebugView::ALL {
                ctx.debug.show(view, true);
            }
            // Something for each view to draw.
            for i in 0..20 {
                let at = vec2(i as f32 * 10.0, 0.0);
                ctx.world
                    .spawn((ponko::d2::Transform::at(at), ponko::d2::PointLight::default()));
            }
            #[cfg(feature = "3d")]
            for i in 0..20 {
                ctx.world.spawn((ponko::d3::Transform::at(vec3(i as f32, 0.0, 0.0)),));
            }
            #[cfg(feature = "physics3d")]
            {
                // A triangle mesh and a terrain tile, drawn as triangles near the camera.
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                for z in 0..20 {
                    for x in 0..20 {
                        vertices.push(vec3(x as f32, 0.0, z as f32));
                    }
                }
                for z in 0..19u32 {
                    for x in 0..19u32 {
                        let i = z * 20 + x;
                        indices.push([i, i + 20, i + 1]);
                        indices.push([i + 1, i + 20, i + 21]);
                    }
                }
                let mesh = ctx.physics3d.trimesh(&vertices, &indices).expect("a mesh");
                ctx.world.spawn((
                    ponko::d3::Transform::at(vec3(-10.0, 0.0, -10.0)),
                    ponko::d3::Collider::mesh(mesh),
                ));
                let ground = ctx.physics3d.heightfield(&[0.0; 17 * 17], 32.0);
                ctx.world.spawn((
                    ponko::d3::Transform::at(vec3(-16.0, -1.0, -16.0)),
                    ponko::d3::TerrainCollider::new(ground),
                ));
                ctx.world
                    .spawn((ponko::d3::Transform::default(), ponko::d3::Collider::sphere(1.0)));
            }
            // Warm up: the buffers grow to what a frame needs.
            for _ in 0..10 {
                ctx.bench_debug_frame(draw);
            }
            ALLOCS.set(0);
            TRACK.set(true);
            let mut counts = (0, 0, 0);
            for _ in 0..50 {
                counts = ctx.bench_debug_frame(draw);
            }
            TRACK.set(false);
            assert!(
                counts.0 > 0 || !cfg!(feature = "3d"),
                "3D lines were handed over: {counts:?}"
            );
            assert!(counts.1 > 0, "2D lines became sprites: {counts:?}");
            allocations = Some(ALLOCS.get());
        });
    assert_eq!(allocations, Some(0), "steady debug drawing allocated");
}
