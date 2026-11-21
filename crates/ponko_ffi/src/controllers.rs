//! Controllers: identity and family, rumble, motion, hot-plug edges, the
//! last used device, local players, and button prompts (glyph lookup and
//! the drawn glyph atlas). The C side of `ponko::input`'s controller API.

use crate::ctx::{ctx_access, out, text};
use crate::keys::gamepad_button_from_c;
use crate::three::PonkoVec3;
use crate::types::{PonkoColor, PonkoRect, PonkoTexture, PonkoVec2, texture_to_c, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid, write_text};
use ponko::input::{
    ControllerFamily, Gamepad, GamepadAxis, GamepadButton, GamepadMotion, GamepadStick, Glyph, GlyphAtlas, Glyphs,
    InputDevice, Key, MouseButton, Rumble,
};
use std::ffi::c_char;

/// Button-label families for prompts (Rust `ControllerFamily`), passed as
/// `uint32_t`.
/// ```
/// assert_eq!(ponko_ffi::PonkoControllerFamily::PONKO_FAMILY_XBOX as u32, 1);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoControllerFamily {
    /// Unknown make: positional prompts.
    PONKO_FAMILY_GENERIC = 0,
    /// A, B, X, Y; LB, RB, LT, RT.
    PONKO_FAMILY_XBOX = 1,
    /// Cross, circle, triangle, square; L1, R1, L2, R2.
    PONKO_FAMILY_PLAYSTATION = 2,
    /// B, A, X, Y; L, R, ZL, ZR.
    PONKO_FAMILY_NINTENDO = 3,
}

/// Device kinds (Rust `InputDevice`), passed as `uint32_t`: the last used
/// device and what a player holds.
/// ```
/// assert_eq!(ponko_ffi::PonkoDeviceKind::PONKO_DEVICE_GAMEPAD as u32, 1);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoDeviceKind {
    /// The keyboard and mouse.
    PONKO_DEVICE_KEYBOARD_MOUSE = 0,
    /// One gamepad; the slot is passed next to it.
    PONKO_DEVICE_GAMEPAD = 1,
}

/// Glyph kinds for `PonkoGlyph::kind`, passed as `uint32_t`.
/// ```
/// assert_eq!(ponko_ffi::PonkoGlyphKind::PONKO_GLYPH_BUTTON as u32, 2);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoGlyphKind {
    /// A key (`code` = PonkoKey).
    PONKO_GLYPH_KEY = 0,
    /// A mouse button (`code` = PonkoMouseButton).
    PONKO_GLYPH_MOUSE = 1,
    /// A gamepad button (`code` = PonkoGamepadButton).
    PONKO_GLYPH_BUTTON = 2,
    /// A whole stick (`code` 0 left, 1 right).
    PONKO_GLYPH_STICK = 3,
    /// A stick direction or trigger travel (`code` = PonkoGamepadAxis).
    PONKO_GLYPH_AXIS = 4,
}

/// One button-prompt image (Rust `Glyph`): a key, mouse button, or a
/// gamepad button, stick or axis as `family` labels it.
/// ```
/// let a = ponko_ffi::PonkoGlyph { kind: 2, family: 1, code: 0 };
/// assert_eq!(a.code, 0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PonkoGlyph {
    /// PonkoGlyphKind.
    pub kind: u32,
    /// PonkoControllerFamily, for the gamepad kinds (0 otherwise).
    pub family: u32,
    /// The key, mouse button, gamepad button, stick or axis (see PonkoGlyphKind).
    pub code: u32,
}

/// What a connected controller is (Rust `GamepadInfo`, without the name:
/// `ponko_ctx_gamepad_name`).
/// ```
/// let info = ponko_ffi::PonkoGamepadInfo::default();
/// assert!(!info.rumble);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PonkoGamepadInfo {
    /// PonkoControllerFamily.
    pub family: u32,
    /// USB vendor id, when `has_ids`.
    pub vendor_id: u16,
    /// USB product id, when `has_ids`.
    pub product_id: u16,
    /// Whether the backend reported the ids.
    pub has_ids: bool,
    /// The two body motors can be driven.
    pub rumble: bool,
    /// The trigger motors can be driven.
    pub trigger_rumble: bool,
    /// Gyro and accelerometer readings arrive.
    pub motion: bool,
}

/// Motor levels, each `0..=1` (Rust `MotorLevels`).
/// ```
/// let off = ponko_ffi::PonkoMotorLevels::default();
/// assert_eq!(off.strong, 0.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PonkoMotorLevels {
    /// The low-frequency body motor.
    pub strong: f32,
    /// The high-frequency body motor.
    pub weak: f32,
    /// The left trigger's motor.
    pub left_trigger: f32,
    /// The right trigger's motor.
    pub right_trigger: f32,
}

/// A drawn glyph atlas (Rust `GlyphAtlas`). Free with `ponko_glyph_atlas_free`.
/// ```
/// let atlas = ponko_ffi::ponko_glyph_atlas_new(16);
/// unsafe { ponko_ffi::ponko_glyph_atlas_free(atlas) };
/// ```
pub struct PonkoGlyphAtlas(GlyphAtlas);

fn pad(slot: u32) -> Result<Gamepad, Failure> {
    Gamepad::from_slot(slot as usize).ok_or_else(|| invalid("gamepad slot out of range"))
}

fn family_to_c(f: ControllerFamily) -> u32 {
    match f {
        ControllerFamily::Xbox => 1,
        ControllerFamily::PlayStation => 2,
        ControllerFamily::Nintendo => 3,
        _ => 0,
    }
}

fn family_from_c(f: u32) -> Result<ControllerFamily, Failure> {
    Ok(match f {
        0 => ControllerFamily::Generic,
        1 => ControllerFamily::Xbox,
        2 => ControllerFamily::PlayStation,
        3 => ControllerFamily::Nintendo,
        _ => return Err(invalid("unknown controller family")),
    })
}

fn device_to_c(device: InputDevice) -> (u32, u32) {
    match device {
        InputDevice::Gamepad(p) => (1, p.slot() as u32),
        _ => (0, 0),
    }
}

fn device_from_c(kind: u32, slot: u32) -> Result<InputDevice, Failure> {
    match kind {
        0 => Ok(InputDevice::KeyboardMouse),
        1 => Ok(InputDevice::Gamepad(pad(slot)?)),
        _ => Err(invalid("unknown device kind")),
    }
}

fn glyph_to_c(g: Glyph) -> PonkoGlyph {
    let (kind, family, code) = match g {
        Glyph::Key(k) => (0, 0, k as u32),
        Glyph::Mouse(b) => (1, 0, b as u32),
        Glyph::Button(f, b) => (2, family_to_c(f), b as u32),
        Glyph::Stick(f, s) => (3, family_to_c(f), u32::from(s == GamepadStick::Right)),
        Glyph::Axis(f, a) => (4, family_to_c(f), a as u32),
        _ => (u32::MAX, 0, 0),
    };
    PonkoGlyph { kind, family, code }
}

fn glyph_from_c(g: PonkoGlyph) -> Result<Glyph, Failure> {
    let family = || family_from_c(g.family);
    let pick = |n: usize| (g.code as usize) < n;
    Ok(match g.kind {
        0 => Glyph::Key(crate::keys::key_from_c(g.code).ok_or_else(|| invalid("unknown key"))?),
        1 => Glyph::Mouse(crate::keys::button_from_c(g.code).ok_or_else(|| invalid("unknown mouse button"))?),
        2 => Glyph::Button(
            family()?,
            gamepad_button_from_c(g.code).ok_or_else(|| invalid("unknown gamepad button"))?,
        ),
        3 if pick(2) => Glyph::Stick(
            family()?,
            if g.code == 0 {
                GamepadStick::Left
            } else {
                GamepadStick::Right
            },
        ),
        4 => Glyph::Axis(
            family()?,
            crate::keys::gamepad_axis_from_c(g.code).ok_or_else(|| invalid("unknown gamepad axis"))?,
        ),
        _ => return Err(invalid("unknown glyph")),
    })
}

/// Writes `glyphs` to `(out, capacity, out_count)`.
unsafe fn write_glyphs(
    glyphs: Glyphs,
    out_glyphs: *mut PonkoGlyph,
    capacity: usize,
    out_count: *mut usize,
) -> Result<(), Failure> {
    if out_count.is_null() || (out_glyphs.is_null() && capacity != 0) {
        return Err(invalid("output is null"));
    }
    // SAFETY: checked non-null; the caller supplies `capacity` writable glyphs.
    unsafe {
        out_count.write(glyphs.len());
        for (i, g) in glyphs.iter().take(capacity).enumerate() {
            out_glyphs.add(i).write(glyph_to_c(g));
        }
    }
    Ok(())
}

/// Starts rumble on gamepad `pad` (Rust `Input::rumble`): the body motors
/// `strong` (low frequency) and `weak` (high frequency) and the trigger
/// motors, each `0..=1` (clamped), for `seconds` of real time (`INFINITY`
/// until `ponko_ctx_gamepad_stop_rumble`). Replaces what it was playing; an
/// empty slot does nothing; motors the pad or backend lacks stay still.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_rumble(std::ptr::null_mut(), 0, 1.0, 0.5, 0.0, 0.0, 0.2) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_rumble(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    strong: f32,
    weak: f32,
    left_trigger: f32,
    right_trigger: f32,
    seconds: f32,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let rumble = Rumble::new(strong, weak)
                .triggers(left_trigger, right_trigger)
                .seconds(seconds);
            c.input.rumble(pad(pad_slot)?, rumble);
            Ok(())
        })
    })
}

/// Stops gamepad `pad`'s rumble now.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_stop_rumble(std::ptr::null_mut(), 0) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_stop_rumble(ctx: *mut PonkoCtx, pad_slot: u32) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.stop_rumble(pad(pad_slot)?);
            Ok(())
        })
    })
}

/// The motor levels gamepad `pad` is asked to play now (all zero when
/// nothing plays).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_rumble_levels(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `out_levels` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_rumble_levels(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    out_levels: *mut PonkoMotorLevels,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable output.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let l = c.input.rumble_levels(pad(pad_slot)?);
            out(
                out_levels,
                PonkoMotorLevels {
                    strong: l.strong,
                    weak: l.weak,
                    left_trigger: l.left_trigger,
                    right_trigger: l.right_trigger,
                },
            )
        })
    })
}

/// What gamepad `pad` is (family, USB ids, capabilities). `*out_connected`
/// is false (and `*out_info` zeroed) for a slot with no described pad.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_info(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_info(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    out_info: *mut PonkoGamepadInfo,
    out_connected: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let info = c.input.gamepad_info(pad(pad_slot)?);
            let value = info.map_or_else(PonkoGamepadInfo::default, |i| PonkoGamepadInfo {
                family: family_to_c(i.family),
                vendor_id: i.vendor_id.unwrap_or(0),
                product_id: i.product_id.unwrap_or(0),
                has_ids: i.vendor_id.is_some() && i.product_id.is_some(),
                rumble: i.capabilities.rumble,
                trigger_rumble: i.capabilities.trigger_rumble,
                motion: i.capabilities.motion,
            });
            out(out_connected, info.is_some())?;
            out(out_info, value)
        })
    })
}

/// The name the system gives gamepad `pad`, as `(buffer, capacity,
/// out_len)` text (empty for a slot with no described pad).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_name(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `buffer` holds `capacity` bytes (or is
/// null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_name(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and the text output.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = c.input.gamepad_info(pad(pad_slot)?).map_or("", |i| i.name.as_str());
            write_text(buffer, capacity, out_len, name)
        })
    })
}

/// Overrides gamepad `pad`'s prompt family (`PonkoControllerFamily`) until
/// it disconnects; an empty slot does nothing.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_gamepad_family(std::ptr::null_mut(), 0, 1) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_gamepad_family(ctx: *mut PonkoCtx, pad_slot: u32, family: u32) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.set_gamepad_family(pad(pad_slot)?, family_from_c(family)?);
            Ok(())
        })
    })
}

/// The latest gyro (rad/s) and accelerometer (m/s²) reading of gamepad
/// `pad`; `*out_has` is false (outputs zeroed) when it sent none.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepad_motion(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepad_motion(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    out_gyro: *mut PonkoVec3,
    out_accel: *mut PonkoVec3,
    out_has: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let m = c.input.gamepad_motion(pad(pad_slot)?);
            let v = |v: ponko::prelude::Vec3| PonkoVec3 { x: v.x, y: v.y, z: v.z };
            let m2 = m.unwrap_or_default();
            out(out_gyro, v(m2.gyro))?;
            out(out_accel, v(m2.accel))?;
            out(out_has, m.is_some())
        })
    })
}

/// Feeds a gyro and accelerometer reading of gamepad `pad` (Rust
/// `InputEvent::GamepadMotion`), for tests and replays. Not finite:
/// `PONKO_INVALID_ARGUMENT`.
/// ```
/// let zero = ponko_ffi::PonkoVec3::default();
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_feed_gamepad_motion(std::ptr::null_mut(), 0, zero, zero) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_feed_gamepad_motion(
    ctx: *mut PonkoCtx,
    pad_slot: u32,
    gyro: PonkoVec3,
    accel: PonkoVec3,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let g = ponko::prelude::vec3(gyro.x, gyro.y, gyro.z);
            let a = ponko::prelude::vec3(accel.x, accel.y, accel.z);
            if !g.is_finite() || !a.is_finite() {
                return Err(invalid("a motion reading must be finite"));
            }
            let motion = GamepadMotion { gyro: g, accel: a };
            c.input
                .feed(ponko::actions::InputEvent::GamepadMotion(pad(pad_slot)?, motion));
            Ok(())
        })
    })
}

/// The device used last (Rust `Input::last_device`): `*out_kind` is a
/// `PonkoDeviceKind`, `*out_pad` the slot for a gamepad (0 otherwise).
/// `out_changed` (may be null) receives whether it changed this frame.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_last_device(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `out_kind` and `out_pad` writable;
/// `out_changed` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_last_device(
    ctx: *mut PonkoCtx,
    out_kind: *mut u32,
    out_pad: *mut u32,
    out_changed: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (kind, slot) = device_to_c(c.input.last_device());
            out(out_kind, kind)?;
            out(out_pad, slot)?;
            if !out_changed.is_null() {
                out(out_changed, c.input.last_device_changed())?;
            }
            Ok(())
        })
    })
}

/// The gamepads that connected (`disconnected` false) or disconnected
/// (`true`) this frame, as slots: `*out_count` receives how many, at most
/// `capacity` are written to `out_pads` (null with 0 to count).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gamepads_changed(std::ptr::null_mut(), false, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `out_pads` holds `capacity` slots (or is
/// null with 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gamepads_changed(
    ctx: *mut PonkoCtx,
    disconnected: bool,
    out_pads: *mut u32,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and the outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_count.is_null() || (out_pads.is_null() && capacity != 0) {
                return Err(invalid("output is null"));
            }
            let mut n = 0;
            let mut write = |p: Gamepad| {
                if n < capacity {
                    out_pads.add(n).write(p.slot() as u32);
                }
                n += 1;
            };
            if disconnected {
                c.input.gamepads_disconnected().for_each(&mut write);
            } else {
                c.input.gamepads_connected().for_each(&mut write);
            }
            out(out_count, n)
        })
    })
}

/// Gives `player` the device `kind` (`PonkoDeviceKind`) / `pad`, taking it
/// from any other player (Rust `Input::assign_player`). A player number
/// out of range is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_assign_player(std::ptr::null_mut(), 0, 1, 0) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_assign_player(
    ctx: *mut PonkoCtx,
    player: u32,
    kind: u32,
    pad_slot: u32,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let device = device_from_c(kind, pad_slot)?;
            if !c.input.assign_player(player as usize, device) {
                return Err(invalid("player number out of range"));
            }
            Ok(())
        })
    })
}

/// Takes `player`'s device away. Out of range does nothing.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_unassign_player(std::ptr::null_mut(), 0) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_unassign_player(ctx: *mut PonkoCtx, player: u32) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.unassign_player(player as usize);
            Ok(())
        })
    })
}

/// The device `player` holds: `*out_has` false when none; otherwise
/// `*out_kind` (`PonkoDeviceKind`), `*out_pad` and `*out_connected` (the
/// device is there now: a pad while plugged in).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_player_device(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_player_device(
    ctx: *mut PonkoCtx,
    player: u32,
    out_kind: *mut u32,
    out_pad: *mut u32,
    out_connected: *mut bool,
    out_has: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let device = c.input.player_device(player as usize);
            let (kind, slot) = device.map_or((0, 0), device_to_c);
            out(out_kind, kind)?;
            out(out_pad, slot)?;
            out(out_connected, c.input.player_connected(player as usize))?;
            out(out_has, device.is_some())
        })
    })
}

/// The lowest player number with no device (`*out_has` false when all
/// have one).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_free_player(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_free_player(
    ctx: *mut PonkoCtx,
    out_player: *mut u32,
    out_has: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let p = c.input.free_player();
            out(out_player, p.unwrap_or(0) as u32)?;
            out(out_has, p.is_some())
        })
    })
}

/// A connected pad no player holds that pressed `button`
/// (`PonkoGamepadButton`) this frame: "press Start to join". `*out_has`
/// false when none did.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_join_pressed(std::ptr::null_mut(), 9, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_join_pressed(
    ctx: *mut PonkoCtx,
    button: u32,
    out_pad: *mut u32,
    out_has: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and writable outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let button: GamepadButton =
                gamepad_button_from_c(button).ok_or_else(|| invalid("unknown gamepad button"))?;
            let p = c.input.join_pressed(button);
            out(out_pad, p.map_or(0, |p| p.slot() as u32))?;
            out(out_has, p.is_some())
        })
    })
}

/// The glyphs that show action `name`'s binding (Rust
/// `Input::action_glyphs_for`): on the device used last when `kind` is
/// `UINT32_MAX`, else on device `kind` (`PonkoDeviceKind`) / `pad`.
/// `*out_count` receives how many (at most 4; 0 when the action has no
/// binding on that device or doesn't exist); at most `capacity` are written.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_action_glyphs(std::ptr::null_mut(), c"jump".as_ptr(), u32::MAX, 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `name` NUL-terminated; `out_glyphs` holds
/// `capacity` glyphs (or is null with 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_action_glyphs(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    kind: u32,
    pad_slot: u32,
    out_glyphs: *mut PonkoGlyph,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context, string and outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            let device = if kind == u32::MAX {
                c.input.last_device()
            } else {
                device_from_c(kind, pad_slot)?
            };
            write_glyphs(c.input.action_glyphs_for(name, device), out_glyphs, capacity, out_count)
        })
    })
}

/// The name a player reads for `glyph` ("A", "Cross", "Space"), as
/// `(buffer, capacity, out_len)` text.
/// ```
/// let a = ponko_ffi::PonkoGlyph { kind: 2, family: 1, code: 0 };
/// let mut buf = [0 as std::ffi::c_char; 8];
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_glyph_label(a, buf.as_mut_ptr(), 8, &mut len) } as u32, 0);
/// assert_eq!(len, 1);
/// ```
/// # Safety
/// `buffer` holds `capacity` bytes (or is null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_label(
    glyph: PonkoGlyph,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let g = glyph_from_c(glyph)?;
        // SAFETY: forwarded caller contract.
        unsafe { write_text(buffer, capacity, out_len, g.label()) }
    })
}

/// Draws every glyph at `cell` pixels square (clamped to 16..=256): free
/// with `ponko_glyph_atlas_free`. Make it once.
///
/// Ownership: the returned atlas is the caller's (null on failure); free it
/// with ponko_glyph_atlas_free.
///
/// ```
/// let atlas = ponko_ffi::ponko_glyph_atlas_new(32);
/// assert!(!atlas.is_null());
/// unsafe { ponko_ffi::ponko_glyph_atlas_free(atlas) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_glyph_atlas_new(cell: u32) -> *mut PonkoGlyphAtlas {
    Box::into_raw(Box::new(PonkoGlyphAtlas(GlyphAtlas::new(cell))))
}

/// Frees an atlas from `ponko_glyph_atlas_new` (null does nothing). Its
/// texture stays until the game ends.
/// ```
/// unsafe { ponko_ffi::ponko_glyph_atlas_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `atlas` null or live, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_atlas_free(atlas: *mut PonkoGlyphAtlas) {
    if !atlas.is_null() {
        // SAFETY: caller passes an atlas from ponko_glyph_atlas_new, once.
        drop(unsafe { Box::from_raw(atlas) });
    }
}

unsafe fn atlas_mut<'a>(atlas: *mut PonkoGlyphAtlas) -> Result<&'a mut GlyphAtlas, Failure> {
    // SAFETY: caller supplies a live, exclusively accessed atlas.
    unsafe { atlas.as_mut() }
        .map(|a| &mut a.0)
        .ok_or_else(|| invalid("glyph atlas is null"))
}

/// The atlas image: `*out_pixels` points at `width * height * 4` RGBA8
/// bytes owned by the atlas (valid until it is freed).
///
/// Ownership: `out_pixels` receives a view of bytes the atlas owns, valid until
/// the atlas is freed; never free it.
///
/// ```
/// let atlas = ponko_ffi::ponko_glyph_atlas_new(16);
/// let (mut p, mut w, mut h) = (std::ptr::null(), 0, 0);
/// assert_eq!(unsafe { ponko_ffi::ponko_glyph_atlas_pixels(atlas, &mut p, &mut w, &mut h) } as u32, 0);
/// assert_eq!(w, 16 * 16);
/// unsafe { ponko_ffi::ponko_glyph_atlas_free(atlas) };
/// ```
/// # Safety
/// `atlas` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_atlas_pixels(
    atlas: *mut PonkoGlyphAtlas,
    out_pixels: *mut *const u8,
    out_width: *mut u32,
    out_height: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let a = atlas_mut(atlas)?;
        let (w, h) = a.size();
        out(out_pixels, a.rgba().as_ptr())?;
        out(out_width, w)?;
        out(out_height, h)
    })
}

/// Where `glyph` is in the atlas image, in pixels: a draw region.
/// ```
/// let atlas = ponko_ffi::ponko_glyph_atlas_new(16);
/// let mut r = ponko_ffi::PonkoRect::default();
/// let a = ponko_ffi::PonkoGlyph { kind: 0, family: 0, code: 0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_glyph_atlas_region(atlas, a, &mut r) } as u32, 0);
/// assert_eq!(r.max.x, 16.0);
/// unsafe { ponko_ffi::ponko_glyph_atlas_free(atlas) };
/// ```
/// # Safety
/// `atlas` null or live; `out_rect` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_atlas_region(
    atlas: *mut PonkoGlyphAtlas,
    glyph: PonkoGlyph,
    out_rect: *mut PonkoRect,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let a = atlas_mut(atlas)?;
        out(out_rect, to_c(a.region(glyph_from_c(glyph)?)))
    })
}

/// The atlas as a texture of `ctx`, uploaded on the first call, its key
/// caps labelled for the player's keyboard layout (Rust
/// `GlyphAtlas::set_key_labels` with `Input::key_labels`).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_glyph_atlas_texture(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// `atlas` and context live or null; `out_texture` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_atlas_texture(
    atlas: *mut PonkoGlyphAtlas,
    ctx: *mut PonkoCtx,
    out_texture: *mut PonkoTexture,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let a = atlas_mut(atlas)?;
        ctx_access(ctx, |c| {
            a.set_key_labels(c.input.key_labels());
            out(out_texture, texture_to_c(a.texture(&mut c.gfx)))
        })
    })
}

/// Draws a prompt row in window pixels (Rust `GlyphAtlas::draw_prompt`):
/// `count` glyphs as `height`-pixel squares from `pos` (top-left), then
/// `label` (may be null) in `color`. `out_width` (may be null) receives the
/// row's width. Key caps are labelled for the player's keyboard layout (a
/// French player's `PONKO_KEY_W` cap reads Z).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_glyph_atlas_draw_prompt(std::ptr::null_mut(), std::ptr::null_mut(), Default::default(), 32.0, std::ptr::null(), 0, std::ptr::null(), ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// `atlas` and context live or null; `glyphs` holds `count` glyphs (at most
/// 4 are drawn); `label` null or NUL-terminated; `out_width` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_glyph_atlas_draw_prompt(
    atlas: *mut PonkoGlyphAtlas,
    ctx: *mut PonkoCtx,
    pos: PonkoVec2,
    height: f32,
    glyphs: *const PonkoGlyph,
    count: usize,
    label: *const c_char,
    color: PonkoColor,
    out_width: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let a = atlas_mut(atlas)?;
        if glyphs.is_null() && count != 0 {
            return Err(invalid("glyphs is null"));
        }
        let list: Vec<Glyph> = (0..count.min(4))
            .map(|i| glyph_from_c(glyphs.add(i).read()))
            .collect::<Result<_, _>>()?;
        let label = if label.is_null() { "" } else { text(label)? };
        ctx_access(ctx, |c| {
            a.set_key_labels(c.input.key_labels());
            let width = a.draw_prompt(
                &mut c.gfx,
                to_c(pos),
                height,
                &Glyphs::from_slice(&list),
                label,
                to_c(color),
            );
            if !out_width.is_null() {
                out(out_width, width)?;
            }
            Ok(())
        })
    })
}

// Keep the enums' discriminants in step with the Rust ones they name.
const _: () = {
    assert!(Key::A as u32 == 0);
    assert!(MouseButton::Left as u32 == 0);
    assert!(GamepadAxis::LeftStickX as u32 == 0);
};
