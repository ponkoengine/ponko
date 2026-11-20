//! What a headless C game can play and see: the pointer moved by hand or
//! by a scenario, the sounds it started, whether it asked to quit, zooming
//! at a point, and drawing and sounds from inside a query callback.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

type Body = Box<dyn FnOnce(*mut PonkoCtx)>;
struct Slot {
    body: Option<Body>,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// Runs `body` once in the setup of a headless 800 x 600 game. A failed
/// assertion inside it fails the test after the run (it must not unwind
/// through the C frames).
fn with_ctx(body: impl FnOnce(*mut PonkoCtx)) {
    unsafe extern "C" fn trampoline(ctx: *mut PonkoCtx, user: *mut c_void) {
        // SAFETY: `user` is the `Slot` below, alive for the run.
        let slot = unsafe { &mut *user.cast::<Slot>() };
        let body = slot.body.take().expect("setup runs once");
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(ctx))) {
            slot.panic = Some(p);
        }
    }
    let body: Box<dyn FnOnce(*mut PonkoCtx) + '_> = Box::new(body);
    // SAFETY: the box is consumed inside the headless run, before this returns.
    let body: Body = unsafe { std::mem::transmute(body) };
    let mut slot = Slot {
        body: Some(body),
        panic: None,
    };
    unsafe {
        let game = ponko_game_new(c"headless-play".as_ptr(), 800, 600);
        let mut world = null_mut();
        let status = ponko_game_run_headless(game, 1, Some(trampoline), (&raw mut slot).cast(), &mut world);
        if let Some(p) = slot.panic.take() {
            std::panic::resume_unwind(p);
        }
        assert_eq!(status, PONKO_OK);
        ponko_world_free(world);
    }
}

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

#[test]
fn the_pointer_and_the_wheel_can_be_moved_by_hand() {
    with_ctx(|ctx| unsafe {
        let (mut at, mut wheel, mut world_at) = (v2(-1.0, -1.0), v2(-1.0, -1.0), v2(0.0, 0.0));
        assert_eq!(ponko_ctx_mouse_pos(ctx, &mut at), PONKO_OK);
        assert_eq!((at.x, at.y), (0.0, 0.0), "a headless pointer starts at the corner");
        assert_eq!(ponko_ctx_feed_mouse_move(ctx, v2(400.0, 300.0)), PONKO_OK);
        assert_eq!(ponko_ctx_feed_mouse_wheel(ctx, v2(0.0, 2.0)), PONKO_OK);
        assert_eq!(ponko_ctx_mouse_pos(ctx, &mut at), PONKO_OK);
        assert_eq!(ponko_ctx_mouse_wheel(ctx, &mut wheel), PONKO_OK);
        assert_eq!(ponko_ctx_mouse_world(ctx, &mut world_at), PONKO_OK);
        assert_eq!((at.x, at.y, wheel.y), (400.0, 300.0, 2.0));
        assert_eq!((world_at.x, world_at.y), (400.0, 300.0), "the camera is at the origin");
        assert_eq!(
            ponko_ctx_feed_mouse_move(ctx, v2(f32::NAN, 0.0)),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_feed_mouse_wheel(ctx, v2(0.0, f32::INFINITY)),
            PONKO_INVALID_ARGUMENT
        );
    });
}

unsafe extern "C" fn watch_pointer(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let (mut at, mut wheel) = (v2(0.0, 0.0), v2(0.0, 0.0));
        ponko_ctx_mouse_pos(ctx, &mut at);
        ponko_ctx_mouse_wheel(ctx, &mut wheel);
        (*user.cast::<Vec<(f32, f32, f32)>>()).push((at.x, at.y, wheel.y));
    }
}

#[test]
fn a_scenario_moves_the_pointer_and_turns_the_wheel_on_its_ticks() {
    let mut seen: Vec<(f32, f32, f32)> = Vec::new();
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(ponko_input_scenario_mouse_move(s, 1, v2(320.0, 240.0)), PONKO_OK);
        assert_eq!(ponko_input_scenario_mouse_wheel(s, 2, v2(0.0, -1.0)), PONKO_OK);
        assert_eq!(
            ponko_input_scenario_mouse_move(s, 3, v2(f32::NAN, 1.0)),
            PONKO_INVALID_ARGUMENT
        );
        let g = ponko_game_new(c"pointer".as_ptr(), 640, 480);
        assert_eq!(ponko_game_input_scenario(g, s), PONKO_OK);
        let user = (&raw mut seen).cast::<c_void>();
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch_pointer), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 4, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
        ponko_input_scenario_free(s);
    }
    assert_eq!(
        seen,
        [
            (0.0, 0.0, 0.0),
            (320.0, 240.0, 0.0),
            (320.0, 240.0, -1.0),
            (320.0, 240.0, 0.0)
        ]
    );
}

fn started(ctx: *mut PonkoCtx, index: usize) -> PonkoSoundStarted {
    // SAFETY: zeroes are a valid PonkoSoundStarted; the header is set before the call.
    let mut s: PonkoSoundStarted = unsafe { std::mem::zeroed() };
    (s.struct_size, s.version) = (size_of::<PonkoSoundStarted>() as u32, PONKO_SOUND_STARTED_VERSION);
    // SAFETY: live ctx, writable descriptor.
    assert_eq!(unsafe { ponko_ctx_sound_started(ctx, index, &mut s) }, PONKO_OK);
    s
}

#[test]
fn a_headless_game_lists_the_sounds_it_started_with_their_bus_and_volume() {
    with_ctx(|ctx| unsafe {
        let mut beep = PonkoSound { id: 0, seconds: 0.0 };
        let samples = [0.0f32; 800];
        assert_eq!(
            ponko_ctx_create_sound(ctx, 8000, samples.as_ptr(), samples.len(), &mut beep),
            PONKO_OK
        );
        let mut count = usize::MAX;
        assert_eq!(ponko_ctx_sounds_started(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 0);
        let mut params = ponko_sound_params_default();
        params.volume = 0.5;
        params.speed = 1.25;
        let (mut a, mut b) = (PonkoPlaying::default(), PonkoPlaying::default());
        assert_eq!(ponko_ctx_play_sound(ctx, beep, &params, &mut a), PONKO_OK);
        assert_eq!(ponko_ctx_play_sound(ctx, beep, null(), &mut b), PONKO_OK);
        assert_ne!(a, b, "each sound has its own handle, headless too");
        assert_eq!(ponko_ctx_sounds_started(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 2);
        let first = started(ctx, 0);
        assert_eq!(
            (first.playing, first.sound, first.volume, first.speed, first.looping),
            (a, beep, 0.5, 1.25, false)
        );
        assert_eq!(started(ctx, 1).playing, b);
        let mut past: PonkoSoundStarted = std::mem::zeroed();
        (past.struct_size, past.version) = (size_of::<PonkoSoundStarted>() as u32, PONKO_SOUND_STARTED_VERSION);
        assert_eq!(ponko_ctx_sound_started(ctx, 2, &mut past), PONKO_NOT_FOUND);
        past.version = 0;
        assert_eq!(
            ponko_ctx_sound_started(ctx, 0, &mut past),
            PONKO_INVALID_ARGUMENT,
            "the header isn't set"
        );
        past.version = PONKO_SOUND_STARTED_VERSION + 1;
        assert_eq!(
            ponko_ctx_sound_started(ctx, 0, &mut past),
            PONKO_INVALID_ARGUMENT,
            "too new"
        );
        assert_eq!(ponko_ctx_sound_started(ctx, 0, null_mut()), PONKO_INVALID_ARGUMENT);
    });
}

unsafe extern "C" fn quit_on_tick_three(ctx: *mut PonkoCtx, _: *mut c_void) {
    if unsafe { ponko_ctx_sim_tick(ctx) } == 3 {
        assert_eq!(unsafe { ponko_ctx_quit(ctx) }, PONKO_OK);
    }
}

unsafe extern "C" fn watch_quit(ctx: *mut PonkoCtx, user: *mut c_void) {
    let mut asked = false;
    unsafe {
        assert_eq!(ponko_ctx_quit_requested(ctx, &mut asked), PONKO_OK);
        (*user.cast::<Vec<bool>>()).push(asked);
    }
}

#[test]
fn a_system_after_the_one_that_quits_sees_the_request() {
    let mut seen: Vec<bool> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"quit".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_sim_system(g, c"quit".as_ptr(), 1, Some(quit_on_tick_three), null_mut()),
            PONKO_OK
        );
        let user = (&raw mut seen).cast::<c_void>();
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch_quit), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 10, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(seen, [false, false, false, true], "and no tick after it");
}

#[test]
fn zooming_at_a_point_keeps_the_world_point_under_it() {
    with_ctx(|ctx| unsafe {
        let mut camera = PonkoCamera {
            position: v2(120.0, 80.0),
            zoom: 1.0,
        };
        assert_eq!(ponko_ctx_set_camera(ctx, &camera), PONKO_OK);
        let at = v2(600.0, 150.0);
        let (mut before, mut after) = (v2(0.0, 0.0), v2(0.0, 0.0));
        assert_eq!(ponko_ctx_screen_to_world(ctx, at, &mut before), PONKO_OK);
        assert_eq!(ponko_ctx_camera_zoom_at(ctx, at, 2.0), PONKO_OK);
        assert_eq!(ponko_ctx_screen_to_world(ctx, at, &mut after), PONKO_OK);
        assert_eq!(ponko_ctx_camera(ctx, &mut camera), PONKO_OK);
        assert_eq!(camera.zoom, 2.0);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        assert_eq!(ponko_ctx_camera_zoom_at(ctx, at, 0.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_camera_zoom_at(ctx, at, f32::NAN), PONKO_INVALID_ARGUMENT);
    });
}

struct Visit {
    ctx: *mut PonkoCtx,
    sound: PonkoSound,
    statuses: Vec<PonkoStatus>,
}

unsafe extern "C" fn draw_each(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let visit = &mut *user.cast::<Visit>();
        let ctx = visit.ctx;
        for _ in 0..(*chunk).count {
            let white = PonkoColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            };
            let style = ponko_debug_style_default();
            visit.statuses.extend([
                ponko_ctx_debug_circle2d(ctx, v2(10.0, 10.0), 5.0, white, &style),
                ponko_ctx_debug_text2d(ctx, v2(10.0, 10.0), c"idle".as_ptr(), white, null()),
                ponko_ctx_draw_rect(
                    ctx,
                    PonkoRect {
                        min: v2(0.0, 0.0),
                        max: v2(4.0, 4.0),
                    },
                    white,
                ),
                ponko_ctx_draw_line(ctx, v2(0.0, 0.0), v2(4.0, 4.0), 1.0, white),
                ponko_ctx_draw_circle(ctx, v2(0.0, 0.0), 2.0, white),
                ponko_ctx_play_sound(ctx, visit.sound, null(), null_mut()),
            ]);
            // Everything else that needs the whole ctx still waits for the query to end.
            let mut camera = PonkoCamera {
                position: v2(0.0, 0.0),
                zoom: 1.0,
            };
            visit.statuses.push(match ponko_ctx_camera(ctx, &mut camera) {
                PONKO_BUSY => PONKO_OK,
                other => other,
            });
        }
    }
}

#[test]
fn debug_shapes_drawing_and_sounds_work_inside_a_query_callback() {
    with_ctx(|ctx| unsafe {
        assert_eq!(ponko_ctx_debug_set_enabled(ctx, true), PONKO_OK);
        let mut sound = PonkoSound { id: 0, seconds: 0.0 };
        let samples = [0.0f32; 100];
        assert_eq!(
            ponko_ctx_create_sound(ctx, 8000, samples.as_ptr(), samples.len(), &mut sound),
            PONKO_OK
        );
        let world = ponko_ctx_world(ctx);
        let mut marker = 0;
        assert_eq!(
            ponko_component_register(world, c"marker".as_ptr(), 4, 4, &mut marker),
            PONKO_OK
        );
        for _ in 0..3 {
            let value = 1u32;
            let values = [PonkoValue {
                component: marker,
                data: (&raw const value).cast(),
                size: 4,
            }];
            let mut e = 0;
            assert_eq!(ponko_spawn(world, values.as_ptr(), 1, &mut e), PONKO_OK);
        }
        let mut visit = Visit {
            ctx,
            sound,
            statuses: Vec::new(),
        };
        let user = (&raw mut visit).cast::<c_void>();
        assert_eq!(
            ponko_query(world, [marker].as_ptr(), 1, Some(draw_each), user),
            PONKO_OK
        );
        assert_eq!(visit.statuses.len(), 3 * 7, "three entities visited");
        assert!(visit.statuses.iter().all(|s| *s == PONKO_OK), "{:?}", visit.statuses);
        let mut stats = PonkoDebugStats::default();
        assert_eq!(ponko_ctx_debug_stats(ctx, &mut stats), PONKO_OK);
        assert_eq!(stats.labels, 3, "a label per entity");
        assert!(stats.lines_2d >= 3 * 16, "a circle per entity: {}", stats.lines_2d);
        let mut count = 0;
        assert_eq!(ponko_ctx_sounds_started(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 3, "a sound per entity");
    });
}

/// What the physics calls returned for each entity a query visited.
/// A lock, because `ponko_query_par` may visit them on several threads.
struct Probe {
    ctx: *mut PonkoCtx,
    statuses: std::sync::Mutex<Vec<PonkoStatus>>,
}

unsafe extern "C" fn probe_physics(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let probe = &*user.cast::<Probe>();
        let ctx = probe.ctx;
        for i in 0..(*chunk).count {
            let mut statuses = Vec::new();
            let (mut found, mut hit) = (true, PonkoRayHit::default());
            let all = ponko_collision_groups_all();
            statuses.push(ponko_ctx_cast_ray2d(
                ctx,
                v2(0.0, 0.0),
                v2(0.0, 1.0),
                50.0,
                Default::default(),
                all,
                &mut found,
                &mut hit,
            ));
            statuses.push(ponko_ctx_cast_ray(
                ctx,
                v2(0.0, 0.0),
                v2(1.0, 0.0),
                50.0,
                &mut found,
                &mut hit,
            ));
            let mut n = 7;
            statuses.push(ponko_ctx_physics_at(ctx, v2(0.0, 0.0), null_mut(), 0, &mut n));
            let mut rig: PonkoThirdPersonCamera = std::mem::zeroed();
            rig.struct_size = size_of::<PonkoThirdPersonCamera>() as u32;
            rig.version = PONKO_THIRD_PERSON_CAMERA_VERSION;
            ponko_third_person_camera_defaults(&mut rig);
            let target = PonkoVec3 {
                x: 10.0 * i as f32,
                y: 0.0,
                z: 0.0,
            };
            statuses.push(ponko_third_person_camera_update_physics(
                &mut rig,
                ctx,
                target,
                null(),
                1.0,
            ));
            probe.statuses.lock().unwrap().extend(statuses);
        }
    }
}

/// Spawns `n` entities with a component of their own and returns it.
unsafe fn spawn_marked(ctx: *mut PonkoCtx, n: usize) -> u32 {
    unsafe {
        let world = ponko_ctx_world(ctx);
        let mut marker = 0;
        assert_eq!(
            ponko_component_register(world, c"probe".as_ptr(), 4, 4, &mut marker),
            PONKO_OK
        );
        for _ in 0..n {
            let value = 1u32;
            let values = [PonkoValue {
                component: marker,
                data: (&raw const value).cast(),
                size: 4,
            }];
            let mut e = 0;
            assert_eq!(ponko_spawn(world, values.as_ptr(), 1, &mut e), PONKO_OK);
        }
        marker
    }
}

#[test]
fn physics_queries_and_the_camera_rig_work_inside_a_query_callback_but_not_a_parallel_one() {
    with_ctx(|ctx| unsafe {
        let marker = spawn_marked(ctx, 2);
        let world = ponko_ctx_world(ctx);
        let mut probe = Probe {
            ctx,
            statuses: Default::default(),
        };
        let user = (&raw mut probe).cast::<c_void>();
        assert_eq!(
            ponko_query(world, [marker].as_ptr(), 1, Some(probe_physics), user),
            PONKO_OK
        );
        assert_eq!(
            *probe.statuses.lock().unwrap(),
            [PONKO_OK; 8],
            "two entities, four calls each"
        );
        let mut camera: PonkoCamera3D = std::mem::zeroed();
        assert_eq!(ponko_ctx_camera3d(ctx, &mut camera), PONKO_OK);
        assert!(
            camera.position.x > 5.0,
            "the rig followed the last one: {:?}",
            camera.position
        );

        probe.statuses.lock().unwrap().clear();
        assert_eq!(
            ponko_query_par(world, [marker].as_ptr(), 1, Some(probe_physics), user),
            PONKO_OK
        );
        assert_eq!(
            *probe.statuses.lock().unwrap(),
            [PONKO_BUSY; 8],
            "refused on worker threads"
        );
    });
}

#[test]
fn c_aseprite_loads_frames_and_starts_tags_on_live_entities() {
    // Original 2x2 RGBA PNG (red, green on the first row; blue, yellow below).
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0, 114, 182,
        13, 36, 0, 0, 0, 20, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 12, 129, 52, 16, 48, 252, 7, 0, 71,
        202, 8, 248, 139, 78, 67, 133, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let dir = std::env::temp_dir().join(format!("ponko-c-aseprite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("layers.png"), PNG).unwrap();
    let json = dir.join("hero.json");
    std::fs::write(&json, include_str!("../../../tests/fixtures/aseprite/composite.json")).unwrap();
    let path = std::ffi::CString::new(json.to_str().unwrap()).unwrap();

    with_ctx(|ctx| unsafe {
        let world = ponko_ctx_world(ctx);
        let mut entity = 0;
        assert_eq!(ponko_spawn(world, null(), 0, &mut entity), PONKO_OK);
        let mut first = PonkoSprite::default();
        assert_eq!(
            ponko_ctx_aseprite_frame(ctx, path.as_ptr(), false, 0, null(), &mut first),
            PONKO_OK
        );
        assert!(first.has_region);
        let mut second = first;
        assert_eq!(
            ponko_ctx_aseprite_frame(ctx, path.as_ptr(), false, 1, null(), &mut second),
            PONKO_OK
        );
        assert_eq!(first.texture.id, second.texture.id);
        assert_ne!(first.region.min, second.region.min);
        assert_eq!(
            ponko_ctx_aseprite_frame(ctx, path.as_ptr(), false, 2, null(), &mut second),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_play_aseprite(ctx, entity, path.as_ptr(), false, c"Missing".as_ptr(), null()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_play_aseprite(ctx, entity, path.as_ptr(), false, c"Run".as_ptr(), null()),
            PONKO_OK
        );
        let mut component = 0;
        assert_eq!(ponko_sprite_component(world, &mut component), PONKO_OK);
        assert_eq!(
            ponko_get(
                world,
                entity,
                component,
                (&raw mut second).cast(),
                size_of::<PonkoSprite>()
            ),
            PONKO_OK
        );
        assert_eq!(first.texture.id, second.texture.id);
        assert_eq!(first.region.min, second.region.min);
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn c_ldtk_spawns_at_the_world_offset_and_maps_fields_to_native_components() {
    let dir = std::env::temp_dir().join(format!("ponko-c-ldtk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let json = dir.join("world.ldtk");
    let mut layout: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/tilemaps/layout.ldtk")).unwrap();
    // Native reflected layouts describe scalar fields. This component uses only value.
    layout["worlds"][0]["levels"][0]["layerInstances"][0]["entityInstances"][0]["fieldInstances"]
        .as_array_mut()
        .unwrap()
        .pop();
    std::fs::write(&json, layout.to_string()).unwrap();
    let path = std::ffi::CString::new(json.to_str().unwrap()).unwrap();

    with_ctx(|ctx| unsafe {
        let world = ponko_ctx_world(ctx);
        let mut stats = 0;
        assert_eq!(
            ponko_component_register(world, c"PickupStats".as_ptr(), 4, 4, &mut stats),
            PONKO_OK
        );
        let field = PonkoField {
            name: c"value".as_ptr(),
            kind: PonkoFieldType::PONKO_FIELD_U32 as u32,
            offset: 0,
        };
        assert_eq!(ponko_component_reflect(world, stats, &field, 1), PONKO_OK);
        let level = PonkoLevelOptions {
            prefab_classes: null(),
            prefab_paths: null(),
            prefab_count: 0,
            solid: null(),
            solid_count: 0,
            level: c"hub-room".as_ptr(),
            no_colliders: false,
            z: 0.,
        };
        let classes = [c"Pickup".as_ptr()];
        let components = [c"PickupStats".as_ptr()];
        let options = PonkoLdtkOptions {
            level: &level,
            at_world_position: true,
            without_saved_objects: false,
            classes: classes.as_ptr(),
            components: components.as_ptr(),
            mapping_count: 1,
        };
        let mut root = 0;
        assert_eq!(
            ponko_ctx_spawn_ldtk_level(ctx, path.as_ptr(), v2(10., 20.), &options, &mut root),
            PONKO_OK
        );
        let mut entity = 0;
        let mut total = 0;
        assert_eq!(ponko_level_entities(world, root, &mut entity, 1, &mut total), PONKO_OK);
        assert_eq!(total, 1);
        let mut value = 0u32;
        assert_eq!(ponko_get(world, entity, stats, (&raw mut value).cast(), 4), PONKO_OK);
        assert_eq!(value, 7);
        let mut transform_id = 0;
        let mut transform = PonkoTransform2D::default();
        assert_eq!(ponko_transform_component(world, &mut transform_id), PONKO_OK);
        assert_eq!(
            ponko_get(
                world,
                root,
                transform_id,
                (&raw mut transform).cast(),
                size_of::<PonkoTransform2D>()
            ),
            PONKO_OK
        );
        assert_eq!(transform.position, v2(-38., 100.));
    });
    std::fs::remove_dir_all(dir).unwrap();
}
