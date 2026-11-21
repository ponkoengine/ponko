//! The sky kit and the day-night driver from C: `ponko_ctx_set_sky_kit`
//! and `ponko_ctx_sky_kit` round-trip every field, and `PonkoDayNight`
//! moves the sun and the moon as Rust's `DayNight` does.
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;
use ponko_ffi::*;

fn kit() -> PonkoSkyKit {
    let mut k: PonkoSkyKit = unsafe { std::mem::zeroed() };
    k.struct_size = size_of::<PonkoSkyKit>() as u32;
    k.version = PONKO_SKY_KIT_VERSION;
    assert_eq!(unsafe { ponko_sky_kit_defaults(&mut k) }, PONKO_OK);
    k
}

fn day(hour: f32) -> PonkoDayNight {
    let mut d: PonkoDayNight = unsafe { std::mem::zeroed() };
    d.struct_size = size_of::<PonkoDayNight>() as u32;
    d.version = PONKO_DAY_NIGHT_VERSION;
    assert_eq!(unsafe { ponko_day_night_new(hour, &mut d) }, PONKO_OK);
    d
}

/// Runs `f` once inside a headless game's setup.
fn in_game(f: unsafe extern "C" fn(*mut PonkoCtx, *mut c_void), user: *mut c_void) {
    unsafe {
        let game = ponko_game_new(c"sky".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, Some(f), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

#[test]
fn the_kit_defaults_match_rust() {
    let k = kit();
    assert!(k.enabled);
    assert_eq!(k.quality, PonkoCloudQuality::PONKO_CLOUDS_MEDIUM as u32);
    assert_eq!((k.coverage, k.kind), (0.5, 0.5));
    assert_eq!((k.wind.x, k.wind.y), (6.0, 2.0));
    assert_eq!((k.altitude, k.thickness, k.shadows), (1500.0, 1500.0, 0.8));
    assert!(!k.moon_has_direction);
    assert_eq!(k.moon_phase, 0.5);
    assert!((ponko_moon_illumination(0.5) - 1.0).abs() < 1e-6);
    assert!(ponko_moon_illumination(0.0) < 1e-6);
}

#[test]
fn the_sky_kit_set_from_c_reads_back_field_for_field() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            let mut now = kit();
            assert_eq!(ponko_ctx_sky_kit(ctx, &mut now), PONKO_OK);
            assert!(!now.enabled, "off until a game turns it on");

            let mut k = kit();
            k.quality = PonkoCloudQuality::PONKO_CLOUDS_HIGH as u32;
            k.coverage = 0.8;
            k.wind = PonkoVec2 { x: -3.0, y: 1.0 };
            k.offset = PonkoVec2 { x: 10.0, y: 20.0 };
            k.altitude = 900.0;
            k.moon_has_direction = true;
            k.moon_direction = PonkoVec3 {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            };
            k.moon_phase = 0.25;
            k.moon_brightness = 0.5;
            assert_eq!(ponko_ctx_set_sky_kit(ctx, &k), PONKO_OK);
            let mut back = kit();
            assert_eq!(ponko_ctx_sky_kit(ctx, &mut back), PONKO_OK);
            assert_eq!(back, k);

            k.enabled = false;
            assert_eq!(ponko_ctx_set_sky_kit(ctx, &k), PONKO_OK);
            assert_eq!(ponko_ctx_sky_kit(ctx, &mut back), PONKO_OK);
            assert!(!back.enabled, "enabled false turns it off");
        }
    }
    in_game(setup, null_mut());
}

#[test]
fn bad_kits_are_refused_and_change_nothing() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            let mut k = kit();
            k.quality = 9;
            assert_eq!(ponko_ctx_set_sky_kit(ctx, &k), PONKO_INVALID_ARGUMENT);
            k = kit();
            k.coverage = f32::NAN;
            assert_eq!(ponko_ctx_set_sky_kit(ctx, &k), PONKO_INVALID_ARGUMENT);
            k = kit();
            k.version = 0;
            assert_eq!(ponko_ctx_set_sky_kit(ctx, &k), PONKO_INVALID_ARGUMENT);
            let mut back = kit();
            assert_eq!(ponko_ctx_sky_kit(ctx, &mut back), PONKO_OK);
            assert!(!back.enabled);
        }
    }
    in_game(setup, null_mut());
}

#[test]
fn day_night_advances_its_clock_and_moves_the_moon_through_its_phases() {
    unsafe {
        let mut d = day(23.0);
        assert_eq!((d.hour, d.day), (23.0, 0));
        assert_eq!(d.first_phase, 0.5);
        d.hours_per_second = 1.0;
        assert_eq!(ponko_day_night_advance(&mut d, 2.0), PONKO_OK);
        assert_eq!(d.day, 1, "midnight passed");
        assert!((d.hour - 1.0).abs() < 1e-4);
        let mut phase = 0.0;
        assert_eq!(ponko_day_night_moon_phase(&d, &mut phase), PONKO_OK);
        assert!(phase > 0.5 && phase < 0.6, "a day on from full: {phase}");
        assert_eq!(ponko_day_night_new(f32::NAN, &mut d), PONKO_INVALID_ARGUMENT);
    }
}

#[test]
fn day_night_sets_the_sun_and_the_kits_moon() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
        unsafe {
            let d = &mut *user.cast::<PonkoDayNight>();
            assert_eq!(ponko_ctx_apply_day_night(ctx, d), PONKO_OK);
            let mut k = kit();
            assert_eq!(ponko_ctx_sky_kit(ctx, &mut k), PONKO_OK);
            assert!(k.enabled, "applying a cycle turns the kit on");
            let mut phase = 0.0;
            ponko_day_night_moon_phase(d, &mut phase);
            assert!((k.moon_phase - phase).abs() < 1e-6);
            let mut light: PonkoLighting = std::mem::zeroed();
            assert_eq!(ponko_ctx_lighting(ctx, &mut light), PONKO_OK);
            let noon = ponko_sun_direction_at(12.0);
            assert!((light.sun_direction.y - noon.y).abs() < 1e-4, "the sun is at noon");
            assert_eq!(light.sun_intensity, d.sun_intensity);
            let before = d.hour;
            assert_eq!(ponko_ctx_update_day_night(ctx, d), PONKO_OK);
            assert!(d.hour >= before, "update advances by the frame's time");
        }
    }
    let mut d = day(12.0);
    in_game(setup, (&raw mut d).cast());
}
