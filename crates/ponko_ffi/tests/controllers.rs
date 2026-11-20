//! Controllers from C: hot-plug through fed events, identity, rumble
//! requests, motion, the last used device, players, glyph prompts and the
//! glyph atlas.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::c_char;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const SOUTH: u32 = PonkoGamepadButton::PONKO_GAMEPAD_SOUTH as u32;
const START: u32 = PonkoGamepadButton::PONKO_GAMEPAD_START as u32;

unsafe fn ok(status: PonkoStatus) {
    assert_eq!(status, PONKO_OK, "{}", last_error());
}

unsafe fn connect(ctx: *mut PonkoCtx, slot: u32) {
    unsafe { ok(ponko_ctx_feed_input(ctx, 8, slot, 0, 0.0)) };
}

unsafe fn press(ctx: *mut PonkoCtx, slot: u32, button: u32) {
    unsafe { ok(ponko_ctx_feed_input(ctx, 2, slot, button, 1.0)) };
}

unsafe fn glyphs(ctx: *mut PonkoCtx, name: &std::ffi::CStr, kind: u32, slot: u32) -> Vec<PonkoGlyph> {
    let mut out = [PonkoGlyph::default(); 4];
    let mut n = 0;
    unsafe {
        ok(ponko_ctx_action_glyphs(
            ctx,
            name.as_ptr(),
            kind,
            slot,
            out.as_mut_ptr(),
            4,
            &mut n,
        ))
    };
    out[..n].to_vec()
}

fn label(g: PonkoGlyph) -> String {
    let mut buf = [0 as c_char; 32];
    let mut len = 0;
    // SAFETY: a buffer of the stated length.
    unsafe { ok(ponko_glyph_label(g, buf.as_mut_ptr(), buf.len(), &mut len)) };
    String::from_utf8(buf[..len].iter().map(|&c| c as u8).collect()).unwrap()
}

#[test]
fn a_fed_pad_connects_reports_the_edge_and_its_generic_info() {
    run_ctx(|ctx| unsafe {
        connect(ctx, 2);
        let mut pads = [0u32; 8];
        let mut n = 0;
        ok(ponko_ctx_gamepads_changed(ctx, false, pads.as_mut_ptr(), 8, &mut n));
        assert_eq!(&pads[..n], [2]);
        ok(ponko_ctx_gamepads_changed(ctx, true, null_mut(), 0, &mut n));
        assert_eq!(n, 0);
        let mut info = PonkoGamepadInfo::default();
        let mut connected = false;
        ok(ponko_ctx_gamepad_info(ctx, 2, &mut info, &mut connected));
        assert!(connected);
        assert_eq!(info.family, PonkoControllerFamily::PONKO_FAMILY_GENERIC as u32);
        assert!(!info.has_ids && !info.rumble);
        ok(ponko_ctx_set_gamepad_family(
            ctx,
            2,
            PonkoControllerFamily::PONKO_FAMILY_PLAYSTATION as u32,
        ));
        ok(ponko_ctx_gamepad_info(ctx, 2, &mut info, &mut connected));
        assert_eq!(info.family, PonkoControllerFamily::PONKO_FAMILY_PLAYSTATION as u32);
        let mut len = 9;
        ok(ponko_ctx_gamepad_name(ctx, 2, null_mut(), 0, &mut len));
        assert_eq!(len, 0, "a fed pad has no name");
        ok(ponko_ctx_gamepad_info(ctx, 5, &mut info, &mut connected));
        assert!(!connected);
        assert_eq!(ponko_ctx_set_gamepad_family(ctx, 2, 99), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_gamepad_info(ctx, 99, &mut info, &mut connected),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn rumble_requests_are_clamped_and_stop() {
    run_ctx(|ctx| unsafe {
        connect(ctx, 0);
        ok(ponko_ctx_gamepad_rumble(ctx, 0, 2.0, 0.5, 0.0, 0.25, f32::INFINITY));
        let mut levels = PonkoMotorLevels::default();
        ok(ponko_ctx_gamepad_rumble_levels(ctx, 0, &mut levels));
        assert_eq!((levels.strong, levels.weak, levels.right_trigger), (1.0, 0.5, 0.25));
        ok(ponko_ctx_gamepad_stop_rumble(ctx, 0));
        ok(ponko_ctx_gamepad_rumble_levels(ctx, 0, &mut levels));
        assert_eq!(levels, PonkoMotorLevels::default());
        ok(ponko_ctx_gamepad_rumble(ctx, 1, 1.0, 1.0, 0.0, 0.0, 1.0));
        ok(ponko_ctx_gamepad_rumble_levels(ctx, 1, &mut levels));
        assert_eq!(levels, PonkoMotorLevels::default(), "an empty slot does nothing");
    });
}

#[test]
fn fed_motion_is_read_back_and_must_be_finite() {
    run_ctx(|ctx| unsafe {
        connect(ctx, 0);
        let (mut gyro, mut accel, mut has) = (PonkoVec3::default(), PonkoVec3::default(), true);
        ok(ponko_ctx_gamepad_motion(ctx, 0, &mut gyro, &mut accel, &mut has));
        assert!(!has);
        let g = PonkoVec3 { x: 0.5, y: 0.0, z: 0.0 };
        let a = PonkoVec3 {
            x: 0.0,
            y: 9.81,
            z: 0.0,
        };
        ok(ponko_ctx_feed_gamepad_motion(ctx, 0, g, a));
        ok(ponko_ctx_gamepad_motion(ctx, 0, &mut gyro, &mut accel, &mut has));
        assert!(has);
        assert_eq!((gyro, accel), (g, a));
        let bad = PonkoVec3 {
            x: f32::NAN,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(ponko_ctx_feed_gamepad_motion(ctx, 0, bad, a), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn players_join_with_a_press_and_keep_their_device() {
    run_ctx(|ctx| unsafe {
        connect(ctx, 0);
        connect(ctx, 1);
        press(ctx, 1, START);
        let (mut slot, mut has) = (0, false);
        ok(ponko_ctx_join_pressed(ctx, START, &mut slot, &mut has));
        assert!(has);
        assert_eq!(slot, 1);
        let (mut player, mut free) = (9, false);
        ok(ponko_ctx_free_player(ctx, &mut player, &mut free));
        assert!(free);
        assert_eq!(player, 0);
        ok(ponko_ctx_assign_player(
            ctx,
            player,
            PonkoDeviceKind::PONKO_DEVICE_GAMEPAD as u32,
            slot,
        ));
        ok(ponko_ctx_join_pressed(ctx, START, &mut slot, &mut has));
        assert!(!has, "already a player");
        let (mut kind, mut pad, mut connected) = (9, 9, false);
        ok(ponko_ctx_player_device(
            ctx,
            0,
            &mut kind,
            &mut pad,
            &mut connected,
            &mut has,
        ));
        assert!(has && connected);
        assert_eq!((kind, pad), (PonkoDeviceKind::PONKO_DEVICE_GAMEPAD as u32, 1));
        ok(ponko_ctx_feed_input(ctx, 5, 1, 0, 0.0));
        ok(ponko_ctx_player_device(
            ctx,
            0,
            &mut kind,
            &mut pad,
            &mut connected,
            &mut has,
        ));
        assert!(has && !connected, "kept while unplugged");
        ok(ponko_ctx_unassign_player(ctx, 0));
        ok(ponko_ctx_player_device(
            ctx,
            0,
            &mut kind,
            &mut pad,
            &mut connected,
            &mut has,
        ));
        assert!(!has);
        assert_eq!(ponko_ctx_assign_player(ctx, 99, 0, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_assign_player(ctx, 0, 7, 0), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn prompts_follow_the_last_used_device() {
    run_ctx(|ctx| unsafe {
        let pad_jump = [PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_GAMEPAD_BUTTON as u32,
            pad: 0,
            first: SOUTH,
            second: 0,
            third: 0,
            fourth: 0,
            deadzone: 0.0,
        }];
        let space = PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_KEY as u32,
            first: PonkoKey::PONKO_KEY_SPACE as u32,
            ..pad_jump[0]
        };
        let map = ponko_action_map_new();
        ok(ponko_action_map_set(
            map,
            c"jump".as_ptr(),
            0,
            [space, pad_jump[0]].as_ptr(),
            2,
        ));
        let walk = PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_KEYS2 as u32,
            first: PonkoKey::PONKO_KEY_A as u32,
            second: PonkoKey::PONKO_KEY_D as u32,
            third: PonkoKey::PONKO_KEY_W as u32,
            fourth: PonkoKey::PONKO_KEY_S as u32,
            ..pad_jump[0]
        };
        ok(ponko_action_map_set(map, c"move".as_ptr(), 2, [walk].as_ptr(), 1));
        ok(ponko_ctx_set_actions(ctx, map));
        ponko_action_map_free(map);

        let (mut kind, mut slot, mut changed) = (9, 9, true);
        ok(ponko_ctx_last_device(ctx, &mut kind, &mut slot, &mut changed));
        assert_eq!(kind, PonkoDeviceKind::PONKO_DEVICE_KEYBOARD_MOUSE as u32);
        let g = glyphs(ctx, c"jump", u32::MAX, 0);
        assert_eq!(g.len(), 1);
        assert_eq!(label(g[0]), "Space");

        connect(ctx, 0);
        ok(ponko_ctx_set_gamepad_family(
            ctx,
            0,
            PonkoControllerFamily::PONKO_FAMILY_XBOX as u32,
        ));
        press(ctx, 0, START);
        ok(ponko_ctx_last_device(ctx, &mut kind, &mut slot, &mut changed));
        assert_eq!(
            (kind, slot, changed),
            (PonkoDeviceKind::PONKO_DEVICE_GAMEPAD as u32, 0, true)
        );
        let g = glyphs(ctx, c"jump", u32::MAX, 0);
        assert_eq!(g[0].kind, PonkoGlyphKind::PONKO_GLYPH_BUTTON as u32);
        assert_eq!(label(g[0]), "A");
        let keyboard = glyphs(ctx, c"jump", PonkoDeviceKind::PONKO_DEVICE_KEYBOARD_MOUSE as u32, 0);
        assert_eq!(label(keyboard[0]), "Space", "a player's own device");
        let walk = glyphs(ctx, c"move", 0, 0);
        assert_eq!(walk.iter().map(|&g| label(g)).collect::<Vec<_>>(), ["W", "A", "S", "D"]);
        assert!(glyphs(ctx, c"nope", u32::MAX, 0).is_empty());
        let bad = PonkoGlyph {
            kind: 9,
            family: 0,
            code: 0,
        };
        let mut len = 0;
        assert_eq!(ponko_glyph_label(bad, null_mut(), 0, &mut len), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn the_glyph_atlas_exposes_pixels_regions_a_texture_and_draws_prompts() {
    let atlas = ponko_glyph_atlas_new(32);
    run_ctx(|ctx| unsafe {
        let (mut pixels, mut w, mut h) = (null(), 0, 0);
        ok(ponko_glyph_atlas_pixels(atlas, &mut pixels, &mut w, &mut h));
        assert_eq!(w, 32 * 16);
        let bytes = std::slice::from_raw_parts(pixels, (w * h * 4) as usize);
        assert!(bytes.iter().any(|&b| b != 0));
        let a = PonkoGlyph {
            kind: PonkoGlyphKind::PONKO_GLYPH_BUTTON as u32,
            family: PonkoControllerFamily::PONKO_FAMILY_XBOX as u32,
            code: SOUTH,
        };
        let mut r = PonkoRect::default();
        ok(ponko_glyph_atlas_region(atlas, a, &mut r));
        assert_eq!((r.max.x - r.min.x, r.max.y - r.min.y), (32.0, 32.0));
        let mut texture = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        ok(ponko_glyph_atlas_texture(atlas, ctx, &mut texture));
        assert_eq!((texture.width, texture.height), (w, h));
        let mut width = 0.0;
        let white = PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        ok(ponko_glyph_atlas_draw_prompt(
            atlas,
            ctx,
            PonkoVec2 { x: 8.0, y: 8.0 },
            32.0,
            [a, a].as_ptr(),
            2,
            null(),
            white,
            &mut width,
        ));
        assert!(width >= 64.0, "two 32-pixel glyphs: {width}");
        assert_eq!(
            ponko_glyph_atlas_draw_prompt(
                null_mut(),
                ctx,
                PonkoVec2 { x: 0.0, y: 0.0 },
                32.0,
                null(),
                0,
                null(),
                white,
                null_mut()
            ),
            PONKO_INVALID_ARGUMENT
        );
    });
    unsafe { ponko_glyph_atlas_free(atlas) };
}
