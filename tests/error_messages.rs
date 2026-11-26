//! The first-hour mistakes keep messages that say what the caller did and
//! how to fix it: a missing or misspelled asset, a component not registered
//! for reflection, one component queried mutably twice, a feature left out
//! of the build, a freed handle, and a shader that doesn't compile. The
//! `should_panic(expected)` strings pin the wording of the panics.

use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};

fn in_tick(f: impl FnOnce(&mut Ctx) + 'static) {
    Game::new("errors", 1, 1).run_headless(0, f);
}

/// Writes `bytes` to a temporary file and returns its absolute path, which
/// the asset loaders accept as is.
#[allow(dead_code)]
fn temp_file(name: &str, bytes: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("ponko-error-messages-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

// Missing asset: the folders searched and the near misses next to it.

#[test]
#[should_panic(expected = "\"crab.pgn\" not found in assets/. Looked in:")]
fn a_missing_asset_panics_with_the_folders_it_searched() {
    in_tick(|ctx| {
        ctx.gfx.load_texture("crab.pgn");
    });
}

#[test]
#[should_panic(
    expected = "Check spelling and case, or place the file under one of these assets/ folders. Nearby names: crab.png"
)]
fn a_misspelled_asset_suggests_the_file_next_to_it() {
    in_tick(|ctx| {
        ctx.gfx.load_texture("crab.pgn");
    });
}

#[test]
fn a_missing_asset_error_names_the_near_miss_too() {
    in_tick(|ctx| {
        let err = ctx.gfx.try_load_texture("crab.pgn").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.path(), Some("crab.pgn"));
        assert!(err.message().contains("Nearby names: crab.png"), "{err}");
        let searched: Vec<&str> = err
            .message()
            .lines()
            .skip(1)
            .take_while(|line| line.starts_with("  "))
            .collect();
        let mut once = searched.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(once.len(), searched.len(), "each folder is listed once: {err}");
        let err = ctx.gfx.try_load_texture("nothing-like-it-anywhere.png").unwrap_err();
        assert!(err.message().contains("Nearby names: (none)"), "{err}");
    });
}

// Component not registered for reflection.

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, Component)]
struct Health {
    hp: f32,
}

#[test]
fn an_unreflected_component_names_the_registration_call() {
    let path = temp_file("health.json", br#"{"hp": 3}"#);
    in_tick(move |ctx| {
        let e = ctx.world.spawn((Health::default(),));
        let err = ctx
            .assets
            .load_reflected_component_json::<Health>(&mut ctx.world, &path, e)
            .unwrap_err();
        assert!(
            err.message()
                .contains("no reflected component named `Health`: register the type with `world.reflect::<Health>()`"),
            "{err}"
        );
        // After the call the message names, the same load works.
        ctx.world.reflect::<Health>();
        ctx.assets
            .load_reflected_component_json::<Health>(&mut ctx.world, &path, e)
            .unwrap();
        assert_eq!(ctx.world.get::<Health>(e).unwrap().hp, 3.0);
    });
}

#[test]
fn an_unreflected_resource_names_the_registration_call() {
    let err = ponko::reflect::ReflectError::UnknownResource("Tuning".into());
    assert!(
        err.to_string()
            .contains("register the type with `world.reflect_resource::<Tuning>()`"),
        "{err}"
    );
}

// The same component queried mutably twice.

#[derive(Clone, Copy, Default, Component)]
struct Speed;

#[test]
#[should_panic(
    expected = "accesses `error_messages::Speed` twice, once mutably; remove the duplicate access or split it into sequential queries"
)]
fn querying_one_component_mutably_twice_says_how_to_split_it() {
    in_tick(|ctx| {
        ctx.world.spawn((Speed,));
        ctx.world.query::<(&mut Speed, &Speed)>().for_each(|_| {});
    });
}

// A feature the build leaves out.

#[cfg(not(feature = "image-qoi"))]
#[test]
#[should_panic(expected = "but this build of ponko doesn't read QOI: turn on its `image-qoi` cargo feature")]
fn an_image_format_left_out_of_the_build_names_its_cargo_feature() {
    // A QOI header: magic, 1 x 1, RGBA, sRGB; the pixels never get read.
    let path = temp_file("pixel.qoi", b"qoif\0\0\0\x01\0\0\0\x01\x04\0");
    in_tick(move |ctx| {
        ctx.gfx.load_texture(&path);
    });
}

// A freed handle.

#[test]
#[cfg(feature = "3d")]
#[should_panic(
    expected = "GPU buffer was freed or belongs to another game; create a live buffer with create_gpu_buffer"
)]
fn freed_gpu_buffer_names_the_replacement_operation() {
    in_tick(|ctx| {
        let buffer = ctx.gfx.create_gpu_buffer(1, &[]);
        ctx.free(buffer);
        ctx.gfx.write_gpu_buffer(buffer, 0, &[Vec4::ZERO]);
    });
}

#[test]
#[cfg(feature = "3d")]
#[should_panic(
    expected = "GPU buffer was freed or belongs to another game; create a live buffer with create_gpu_buffer"
)]
fn a_freed_gpu_buffer_stays_freed_when_a_new_buffer_takes_its_slot() {
    in_tick(|ctx| {
        let old = ctx.gfx.create_gpu_buffer(1, &[]);
        ctx.free(old);
        let new = ctx.gfx.create_gpu_buffer(1, &[]);
        assert_ne!(new, old);
        ctx.gfx.write_gpu_buffer(new, 0, &[Vec4::ONE]);
        ctx.gfx.write_gpu_buffer(old, 0, &[Vec4::ZERO]);
    });
}

#[test]
#[cfg(feature = "3d")]
#[should_panic(expected = "instance batch: mesh is freed or foreign; create a live mesh in this game")]
fn a_freed_mesh_stays_freed_when_a_new_mesh_takes_its_slot() {
    in_tick(|ctx| {
        let material = ctx.gfx.create_material(MaterialDesc::default());
        let old = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
        ctx.free(old);
        let new = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
        assert_ne!(new, old);
        ctx.gfx
            .create_instance_batch(new, &[Mat4::IDENTITY], InstanceBatchOptions::default());
        ctx.gfx
            .create_instance_batch(old, &[Mat4::IDENTITY], InstanceBatchOptions::default());
    });
}

#[test]
#[cfg(feature = "3d")]
#[should_panic(
    expected = "create_mesh: that material wasn't made by this game; create it with create_material in this game"
)]
fn a_material_from_another_game_says_where_to_make_one() {
    let other = std::rc::Rc::new(std::cell::Cell::new(None));
    let out = other.clone();
    in_tick(move |ctx| {
        for _ in 0..4 {
            out.set(Some(ctx.gfx.create_material(MaterialDesc::default())));
        }
    });
    let foreign = other.get().unwrap();
    in_tick(move |ctx| {
        ctx.gfx.create_mesh(&MeshData::cube(1.0), foreign);
    });
}

// A shader that doesn't compile: file, line, column and the line itself.

#[cfg(all(feature = "3d", feature = "window"))]
const BROKEN_SHADER: &str = "// tints everything red\nfn surface(s: Surface) -> Surface {\n    return 1.0;\n}\n";

#[test]
#[cfg(all(feature = "3d", feature = "window"))]
#[should_panic(expected = "red.wgsl:3:12: error: ")]
fn a_shader_that_does_not_compile_panics_with_its_file_line_and_column() {
    in_tick(|ctx| {
        ctx.gfx.create_shader("red.wgsl", BROKEN_SHADER);
    });
}

#[test]
#[cfg(all(feature = "3d", feature = "window"))]
#[should_panic(expected = "\n        return 1.0;\n               ^ ")]
fn a_shader_that_does_not_compile_panics_with_the_line_and_a_caret() {
    in_tick(|ctx| {
        ctx.gfx.create_shader("red.wgsl", BROKEN_SHADER);
    });
}

#[test]
#[cfg(all(feature = "3d", feature = "window"))]
fn the_shader_panic_and_shader_status_read_the_same() {
    in_tick(|ctx| {
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.gfx.create_shader("red.wgsl", BROKEN_SHADER);
        }))
        .unwrap_err();
        let printed = panic.downcast_ref::<String>().unwrap();
        let status = &ctx.gfx.shader_status()[0];
        assert_eq!(printed, &format!("ponko: {}", status.diagnostics[0]));
    });
}
