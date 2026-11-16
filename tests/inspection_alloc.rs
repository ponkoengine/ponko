//! The steady-state allocations of a batch of view systems.
//!
//! Profiling runs the batch on the calling thread, even when parallelism is
//! requested. Count that thread for exact assertions: idle Rayon workers can
//! still allocate when parking, after the pool has started and warm-up ended.
//! Keep the engine's process-wide counter installed for the profiling report.
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

#[allow(unused_imports)]
use ponko::perf::CountingAllocator;
use ponko::prelude::*;

struct ThreadCountingAllocator;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn thread_allocs() -> u64 {
    ALLOCS.with(Cell::get)
}

fn note_allocation() {
    // Const-initialized TLS neither allocates nor needs a destructor. Ignore
    // threads whose TLS is unavailable during teardown.
    let _ = ALLOCS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: all operations forward unchanged to the engine's counting allocator;
// the extra accounting only updates an allocation-free thread-local Cell.
unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller supplies the allocator's layout contract.
        unsafe { CountingAllocator.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller supplies the allocator's layout contract.
        unsafe { CountingAllocator.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note_allocation();
        // SAFETY: the pointer, layout and size retain the caller's contract.
        unsafe { CountingAllocator.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout retain the caller's contract.
        unsafe { CountingAllocator.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

use ponko::inventory::AssetClass;
use ponko::locale::{StringTable, Strings};

#[test]
fn borrowed_inspection_preserves_order_without_allocating() {
    let mut table = StringTable::new("en");
    table.set("z", "Last");
    table.set("a", "First");
    let mut strings = Strings::default();
    strings.insert(table.clone());
    strings.insert(StringTable::new("fr"));
    let plan = Game::new("inspection", 1, 1).system(|_: &mut Ctx| {}).schedule_plan();
    let before = thread_allocs();
    assert_eq!(table.iter_keys().len(), 2);
    assert!(table.iter_keys().eq(["a", "z"]));
    assert!(strings.iter_languages().eq(["en", "fr"]));
    for stage in plan.iter_stages_in(Phase::Frame) {
        assert!(plan.iter_names(stage).next().is_some());
    }
    assert_eq!(thread_allocs(), before);
    assert_eq!(table.keys(), table.iter_keys().collect::<Vec<_>>());
    assert_eq!(strings.languages(), strings.iter_languages().collect::<Vec<_>>());
    for stage in plan.stages_in(Phase::Frame) {
        assert_eq!(plan.names(stage), plan.iter_names(stage).collect::<Vec<_>>());
    }
}

#[test]
fn reusable_inspection_replaces_records_without_steady_state_allocations() {
    Game::new("inspection", 1, 1).run_headless(0, |ctx| {
        let mut source = ponko::assets::MemorySource::new("inspection");
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        source.insert("short.png", png.clone());
        source.insert("longer-name.png", png);
        ctx.assets.add_source(source);
        ctx.gfx.resident_texture("short.png");
        ctx.gfx.resident_texture("longer-name.png");
        let sound = ctx.audio.create(8000, &[0.0; 80]);
        let mut textures = ctx.gfx.asset_handles(AssetClass::Texture);
        let mut sounds = ctx.audio.asset_handles();
        let mut streamed = ctx.gfx.streamed_assets();
        assert_eq!(streamed.len(), 2);
        assert_eq!(sounds.len(), 1);
        // Repeated warm-up also accommodates records whose sorted output order
        // differs from the backing handle enumeration order.
        for _ in 0..3 {
            ctx.gfx.asset_handles_into(AssetClass::Texture, &mut textures);
            ctx.audio.asset_handles_into(&mut sounds);
            ctx.gfx.streamed_assets_into(&mut streamed);
        }
        let before = thread_allocs();
        for _ in 0..10 {
            ctx.gfx.asset_handles_into(AssetClass::Texture, &mut textures);
            ctx.audio.asset_handles_into(&mut sounds);
            ctx.gfx.streamed_assets_into(&mut streamed);
        }
        assert_eq!(thread_allocs(), before);
        assert_eq!(textures.len(), ctx.gfx.asset_handles(AssetClass::Texture).len());
        ctx.audio.free(sound);
        ctx.audio.asset_handles_into(&mut sounds);
        assert!(sounds.is_empty());
        let capacity = textures.capacity();
        ctx.gfx.asset_handles_into(AssetClass::Sound, &mut textures);
        assert!(textures.is_empty());
        assert_eq!(textures.capacity(), capacity);
        #[cfg(feature = "3d")]
        {
            ctx.gfx.load_mesh("models/house.glb");
            let mut materials = ctx.gfx.loaded_materials("walls");
            assert_eq!(materials.len(), 1);
            let mut costs = ctx.gfx.mesh_costs();
            let before = thread_allocs();
            ctx.gfx.loaded_materials_into("walls", &mut materials);
            ctx.gfx.mesh_costs_into(&mut costs);
            assert_eq!(thread_allocs(), before);
            assert_eq!(materials.len(), 1);
            let capacity = materials.capacity();
            ctx.gfx.loaded_materials_into("missing", &mut materials);
            assert!(materials.is_empty());
            assert_eq!(materials.capacity(), capacity);
            assert!(costs.is_empty());
        }
    });
}

#[test]
fn named_draw_space_preserves_other_options_and_boolean_compatibility() {
    let params = DrawParams::default().with_z(7.0).with_rotation(0.4);
    assert!(!params.screen);
    let screen = params.with_space(DrawSpace::Screen);
    assert!(screen.screen);
    assert_eq!(screen.z, 7.0);
    assert_eq!(screen.rotation, 0.4);
    assert_eq!(screen.with_space(DrawSpace::World), params);
    #[allow(deprecated)]
    {
        assert_eq!(params.with_screen(true), screen);
        assert_eq!(screen.with_screen(false), params);
    }
}

#[test]
fn sprite_and_debug_drawing_share_one_draw_space_type() {
    let space: ponko::debug::DrawSpace = DrawSpace::Screen;
    assert_eq!(space, DrawSpace::Screen);
    assert_eq!(ponko::debug::DrawSpace::default(), DrawSpace::World);
}
