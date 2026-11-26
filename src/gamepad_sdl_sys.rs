//! The [`PadSystem`] that calls SDL 3, for the `gamepad-sdl` backend
//! (`gamepad_sdl`, which has the rest and its scripted tests).

use std::ffi::CStr;
use std::ptr::NonNull;

use sdl3_sys::events::{SDL_Event, SDL_EventType, SDL_PollEvent};
use sdl3_sys::gamepad::{
    SDL_CloseGamepad, SDL_Gamepad, SDL_GamepadHasSensor, SDL_GetGamepadGUIDForID, SDL_GetGamepadName,
    SDL_GetGamepadProduct, SDL_GetGamepadProperties, SDL_GetGamepadSerial, SDL_GetGamepadTouchpadFinger,
    SDL_GetGamepadType, SDL_GetGamepadVendor, SDL_GetGamepads, SDL_GetNumGamepadTouchpadFingers,
    SDL_GetRealGamepadType, SDL_OpenGamepad, SDL_PROP_GAMEPAD_CAP_PLAYER_LED_BOOLEAN,
    SDL_PROP_GAMEPAD_CAP_RGB_LED_BOOLEAN, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
    SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN, SDL_RumbleGamepad, SDL_RumbleGamepadTriggers, SDL_SendGamepadEffect,
    SDL_SetGamepadLED, SDL_SetGamepadPlayerIndex, SDL_SetGamepadSensorEnabled,
};
use sdl3_sys::haptic::{
    SDL_CloseHaptic, SDL_CreateHapticEffect, SDL_GetHapticFeatures, SDL_HAPTIC_CONSTANT, SDL_HAPTIC_DAMPER,
    SDL_HAPTIC_FRICTION, SDL_HAPTIC_INFINITY, SDL_HAPTIC_SINE, SDL_HAPTIC_SPRING, SDL_HAPTIC_STEERING_AXIS, SDL_Haptic,
    SDL_HapticCondition, SDL_HapticConstant, SDL_HapticDirection, SDL_HapticEffect, SDL_HapticEffectID,
    SDL_HapticPeriodic, SDL_IsJoystickHaptic, SDL_OpenHapticFromJoystick, SDL_RunHapticEffect, SDL_SetHapticAutocenter,
    SDL_SetHapticGain, SDL_StopHapticEffect, SDL_UpdateHapticEffect,
};
use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_INIT_HAPTIC, SDL_InitSubSystem, SDL_QuitSubSystem};
use sdl3_sys::joystick::{
    SDL_CloseJoystick, SDL_GetJoystickGUID, SDL_GetJoystickName, SDL_GetJoystickProduct, SDL_GetJoystickSerial,
    SDL_GetJoystickType, SDL_GetJoystickTypeForID, SDL_GetJoystickVendor, SDL_GetJoysticks, SDL_GetNumJoystickAxes,
    SDL_GetNumJoystickButtons, SDL_GetNumJoystickHats, SDL_IsJoystickVirtual, SDL_Joystick, SDL_JoystickID,
    SDL_OpenJoystick,
};
use sdl3_sys::properties::SDL_GetBooleanProperty;
use sdl3_sys::sensor::{SDL_SENSOR_ACCEL, SDL_SENSOR_GYRO};

use crate::gamepad_sdl::{EFFECT_SLOTS, Effect, Opened, OpenedJoystick, PadEvent, PadSystem, dualsense, raw};
use crate::input::{GamepadTouchpadFinger, TriggerEffect};

fn send_dualsense_effect(pad: NonNull<SDL_Gamepad>, effect: dualsense::Payload) -> bool {
    // SAFETY: callers supply a live gamepad. SDL copies the bytes before returning.
    unsafe { SDL_SendGamepadEffect(pad.as_ptr(), effect.as_ptr().cast(), dualsense::PAYLOAD_SIZE as i32) }
}

/// A joystick opened through SDL, its force-feedback device, and the
/// effect in each slot (SDL's id, and whether it is playing).
struct OpenJoystick {
    id: u32,
    joystick: NonNull<SDL_Joystick>,
    haptic: Option<NonNull<SDL_Haptic>>,
    effects: [Option<(SDL_HapticEffectID, bool)>; EFFECT_SLOTS],
}

/// SDL's gamepad subsystem, and the pads and joysticks opened through it.
pub(crate) struct Sdl {
    open: Vec<(u32, NonNull<SDL_Gamepad>)>,
    joysticks: Vec<OpenJoystick>,
    /// Whether the haptic subsystem started (force feedback works without
    /// it only in that nothing plays).
    haptic: bool,
}

/// `effect` as SDL's: along the steering axis, playing until stopped.
pub(crate) fn sdl_effect(effect: Effect) -> SDL_HapticEffect {
    let direction = SDL_HapticDirection {
        r#type: SDL_HAPTIC_STEERING_AXIS,
        dir: [0; 3],
    };
    let condition = |kind, coeff: i16, center: i16| SDL_HapticEffect {
        condition: SDL_HapticCondition {
            r#type: kind,
            direction,
            length: SDL_HAPTIC_INFINITY,
            right_sat: [u16::MAX; 3],
            left_sat: [u16::MAX; 3],
            right_coeff: [coeff; 3],
            left_coeff: [coeff; 3],
            center: [center; 3],
            ..SDL_HapticCondition::default()
        },
    };
    match effect {
        Effect::Constant { level } => SDL_HapticEffect {
            constant: SDL_HapticConstant {
                r#type: SDL_HAPTIC_CONSTANT,
                direction,
                length: SDL_HAPTIC_INFINITY,
                level,
                ..SDL_HapticConstant::default()
            },
        },
        Effect::Spring { coeff, center } => condition(SDL_HAPTIC_SPRING, coeff, center),
        Effect::Damper { coeff } => condition(SDL_HAPTIC_DAMPER, coeff, 0),
        Effect::Friction { coeff } => condition(SDL_HAPTIC_FRICTION, coeff, 0),
        Effect::Sine { magnitude, period_ms } => SDL_HapticEffect {
            periodic: SDL_HapticPeriodic {
                r#type: SDL_HAPTIC_SINE,
                direction,
                length: SDL_HAPTIC_INFINITY,
                period: period_ms,
                magnitude,
                ..SDL_HapticPeriodic::default()
            },
        },
    }
}

// SAFETY: SDL's gamepad functions lock SDL's joystick lock themselves, so
// they may be called from any thread; `Sdl` is used by one thread at a
// time (the engine's backend sits behind a mutex), and the pad pointers
// it holds are owned by it alone.
unsafe impl Send for Sdl {}

/// SDL's last error message.
fn error() -> String {
    // SAFETY: SDL_GetError returns a valid, NUL-terminated string (empty
    // when there is no error) that stays valid until the next SDL call.
    unsafe { CStr::from_ptr(sdl3_sys::error::SDL_GetError()) }
        .to_string_lossy()
        .into_owned()
}

/// A string SDL returned, or "" for none.
fn text(s: *const std::ffi::c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: not null, and SDL's strings are NUL-terminated and live
    // while the pad they describe is open.
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

impl Sdl {
    /// Starts SDL's gamepad subsystem (and its event queue). SDL's
    /// handlers for Ctrl-C are left off: the game's own stay in charge.
    pub(crate) fn start() -> Result<Self, String> {
        // SAFETY: the hint name and value are NUL-terminated constants;
        // initialising a subsystem is allowed from any thread.
        let ok = unsafe {
            sdl3_sys::hints::SDL_SetHint(sdl3_sys::hints::SDL_HINT_NO_SIGNAL_HANDLERS, c"1".as_ptr());
            SDL_InitSubSystem(SDL_INIT_GAMEPAD)
        };
        if !ok {
            return Err(error());
        }
        // SAFETY: as above. Without it joysticks still read; no effect plays.
        let haptic = unsafe { SDL_InitSubSystem(SDL_INIT_HAPTIC) };
        Ok(Self {
            open: Vec::new(),
            joysticks: Vec::new(),
            haptic,
        })
    }

    fn joystick(&mut self, id: u32) -> Option<&mut OpenJoystick> {
        self.joysticks.iter_mut().find(|j| j.id == id)
    }

    fn pad(&self, id: u32) -> Option<*mut SDL_Gamepad> {
        self.open.iter().find(|(i, _)| *i == id).map(|(_, p)| p.as_ptr())
    }
}

/// Closes a joystick and its force-feedback device (which ends its effects).
fn close_joystick(j: OpenJoystick) {
    // SAFETY: both were opened by `open_joystick` and are closed once, here.
    unsafe {
        if let Some(h) = j.haptic {
            SDL_CloseHaptic(h.as_ptr());
        }
        SDL_CloseJoystick(j.joystick.as_ptr());
    }
}

impl Drop for Sdl {
    fn drop(&mut self) {
        for (_, pad) in self.open.drain(..) {
            // SAFETY: opened by `open` and not closed since.
            unsafe { SDL_CloseGamepad(pad.as_ptr()) };
        }
        for j in self.joysticks.drain(..) {
            close_joystick(j);
        }
        // SAFETY: balances the `SDL_InitSubSystem` calls of `start`.
        unsafe {
            if self.haptic {
                SDL_QuitSubSystem(SDL_INIT_HAPTIC);
            }
            SDL_QuitSubSystem(SDL_INIT_GAMEPAD);
        }
    }
}

impl PadSystem for Sdl {
    fn next_event(&mut self) -> Option<PadEvent> {
        loop {
            let mut event = std::mem::MaybeUninit::<SDL_Event>::uninit();
            // SAFETY: SDL writes a whole event when it returns true.
            if !unsafe { SDL_PollEvent(event.as_mut_ptr()) } {
                return None;
            }
            // SAFETY: written by SDL_PollEvent above.
            let event = unsafe { event.assume_init() };
            // SAFETY: every event starts with its type, and the variant
            // read below is the one that type names.
            let kind = SDL_EventType(unsafe { event.r#type });
            let read = unsafe {
                match kind {
                    SDL_EventType::GAMEPAD_ADDED => PadEvent::Added(event.gdevice.which.0),
                    SDL_EventType::GAMEPAD_REMOVED => PadEvent::Removed(event.gdevice.which.0),
                    SDL_EventType::GAMEPAD_BUTTON_DOWN | SDL_EventType::GAMEPAD_BUTTON_UP => PadEvent::Button(
                        event.gbutton.which.0,
                        i32::from(event.gbutton.button),
                        event.gbutton.down,
                    ),
                    SDL_EventType::GAMEPAD_AXIS_MOTION => {
                        PadEvent::Axis(event.gaxis.which.0, i32::from(event.gaxis.axis), event.gaxis.value)
                    }
                    SDL_EventType::GAMEPAD_TOUCHPAD_DOWN
                    | SDL_EventType::GAMEPAD_TOUCHPAD_MOTION
                    | SDL_EventType::GAMEPAD_TOUCHPAD_UP => {
                        let touch = event.gtouchpad;
                        PadEvent::Touchpad(
                            touch.which.0,
                            touch.touchpad,
                            touch.finger,
                            GamepadTouchpadFinger {
                                x: touch.x,
                                y: touch.y,
                                pressure: touch.pressure,
                                down: kind != SDL_EventType::GAMEPAD_TOUCHPAD_UP,
                            },
                        )
                    }
                    SDL_EventType::GAMEPAD_SENSOR_UPDATE => {
                        PadEvent::Sensor(event.gsensor.which.0, event.gsensor.sensor, event.gsensor.data)
                    }
                    SDL_EventType::JOYSTICK_ADDED => PadEvent::JoyAdded(event.jdevice.which.0),
                    SDL_EventType::JOYSTICK_REMOVED => PadEvent::JoyRemoved(event.jdevice.which.0),
                    SDL_EventType::JOYSTICK_AXIS_MOTION => {
                        PadEvent::JoyAxis(event.jaxis.which.0, i32::from(event.jaxis.axis), event.jaxis.value)
                    }
                    SDL_EventType::JOYSTICK_BUTTON_DOWN | SDL_EventType::JOYSTICK_BUTTON_UP => PadEvent::JoyButton(
                        event.jbutton.which.0,
                        i32::from(event.jbutton.button),
                        event.jbutton.down,
                    ),
                    SDL_EventType::JOYSTICK_HAT_MOTION => {
                        PadEvent::JoyHat(event.jhat.which.0, i32::from(event.jhat.hat), event.jhat.value)
                    }
                    _ => continue,
                }
            };
            return Some(read);
        }
    }

    fn present(&mut self, into: &mut Vec<u32>) {
        let mut count = 0;
        // SAFETY: SDL returns an array of `count` ids (or null), which
        // the caller frees with SDL_free.
        unsafe {
            let ids = SDL_GetGamepads(&mut count);
            if ids.is_null() {
                return;
            }
            let n = usize::try_from(count).unwrap_or(0);
            into.extend(std::slice::from_raw_parts(ids, n).iter().map(|id| id.0));
            sdl3_sys::stdinc::SDL_free(ids.cast());
        }
    }

    fn open(&mut self, id: u32) -> Option<Opened> {
        let instance = sdl3_sys::joystick::SDL_JoystickID(id);
        // SAFETY: opening by an id SDL reported; a null result is handled.
        let pad = NonNull::new(unsafe { SDL_OpenGamepad(instance) })?;
        let p = pad.as_ptr();
        // SAFETY: `p` is an open gamepad; the property names are SDL's
        // NUL-terminated constants.
        let opened = unsafe {
            let guid = SDL_GetGamepadGUIDForID(instance);
            let mut identity: String = guid.data.iter().map(|b| format!("{b:02x}")).collect();
            let serial = text(SDL_GetGamepadSerial(p));
            if !serial.is_empty() {
                identity.push('/');
                identity.push_str(&serial);
            }
            let vendor_id = SDL_GetGamepadVendor(p);
            let product_id = SDL_GetGamepadProduct(p);
            // SDL has no adaptive-trigger property. Check the real layout,
            // Sony's DualSense/Edge IDs and whether the driver accepts an
            // neutral trigger report. This also clears any resistance left
            // behind by a link loss. MFI and virtual pads can have the same
            // IDs without speaking this HID report format.
            let adaptive_triggers = SDL_GetRealGamepadType(p).0 == raw::TYPE_PS5
                && vendor_id == 0x054c
                && matches!(product_id, 0x0ce6 | 0x0df2)
                && !SDL_IsJoystickVirtual(instance)
                && send_dualsense_effect(pad, dualsense::trigger_report([TriggerEffect::Off; 2]));
            let props = SDL_GetGamepadProperties(p);
            let fingers = SDL_GetNumGamepadTouchpadFingers(p, 0).clamp(0, 2);
            let touchpad = (fingers > 0).then(|| {
                let mut contacts = [GamepadTouchpadFinger::default(); 2];
                for (finger, contact) in (0..fingers).zip(&mut contacts) {
                    SDL_GetGamepadTouchpadFinger(
                        p,
                        0,
                        finger,
                        &mut contact.down,
                        &mut contact.x,
                        &mut contact.y,
                        &mut contact.pressure,
                    );
                }
                contacts
            });
            Opened {
                name: text(SDL_GetGamepadName(p)),
                vendor_id,
                product_id,
                kind: SDL_GetGamepadType(p).0,
                identity,
                rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN, false),
                trigger_rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN, false),
                gyro: SDL_GamepadHasSensor(p, SDL_SENSOR_GYRO),
                accel: SDL_GamepadHasSensor(p, SDL_SENSOR_ACCEL),
                light: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_RGB_LED_BOOLEAN, false),
                player_leds: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_PLAYER_LED_BOOLEAN, false),
                touchpad,
                adaptive_triggers,
            }
        };
        self.open.push((id, pad));
        Some(opened)
    }

    fn close(&mut self, id: u32) {
        if let Some(at) = self.open.iter().position(|(i, _)| *i == id) {
            let (_, pad) = self.open.swap_remove(at);
            // SAFETY: opened by `open` and not closed since (removed above).
            unsafe { SDL_CloseGamepad(pad.as_ptr()) };
        }
    }

    fn enable_motion(&mut self, id: u32, gyro: bool, accel: bool) -> bool {
        let Some(p) = self.pad(id) else { return false };
        // SAFETY: `p` is an open gamepad.
        unsafe {
            if accel {
                SDL_SetGamepadSensorEnabled(p, SDL_SENSOR_ACCEL, true);
            }
            gyro && SDL_SetGamepadSensorEnabled(p, SDL_SENSOR_GYRO, true)
        }
    }

    fn rumble(&mut self, id: u32, low: u16, high: u16, ms: u32) {
        if let Some(p) = self.pad(id) {
            // SAFETY: `p` is an open gamepad. A pad without motors says no.
            unsafe { SDL_RumbleGamepad(p, low, high, ms) };
        }
    }

    fn rumble_triggers(&mut self, id: u32, left: u16, right: u16, ms: u32) {
        if let Some(p) = self.pad(id) {
            // SAFETY: `p` is an open gamepad. A pad without trigger motors says no.
            unsafe { SDL_RumbleGamepadTriggers(p, left, right, ms) };
        }
    }

    fn set_light(&mut self, id: u32, [r, g, b]: [u8; 3]) {
        if let Some(pad) = self.pad(id) {
            // SAFETY: pad is owned by Sdl and stays open through the call.
            unsafe { SDL_SetGamepadLED(pad, r, g, b) };
        }
    }

    fn set_player_leds(&mut self, id: u32, player: Option<u8>) {
        if let Some(pad) = self.pad(id) {
            // SAFETY: pad is open and -1 is SDL's unassigned index.
            unsafe { SDL_SetGamepadPlayerIndex(pad, player.map_or(-1, i32::from)) };
        }
    }

    fn set_trigger_effects(&mut self, id: u32, effects: [TriggerEffect; 2]) {
        if let Some(pad) = self.pad(id).and_then(NonNull::new) {
            send_dualsense_effect(pad, dualsense::trigger_report(effects));
        }
    }

    fn now_ms(&self) -> u64 {
        // SAFETY: a plain clock read, valid once SDL is initialised.
        unsafe { sdl3_sys::timer::SDL_GetTicks() }
    }

    fn present_joysticks(&mut self, into: &mut Vec<u32>) {
        let mut count = 0;
        // SAFETY: SDL returns an array of `count` ids (or null), which
        // the caller frees with SDL_free.
        unsafe {
            let ids = SDL_GetJoysticks(&mut count);
            if ids.is_null() {
                return;
            }
            let n = usize::try_from(count).unwrap_or(0);
            into.extend(std::slice::from_raw_parts(ids, n).iter().map(|id| id.0));
            sdl3_sys::stdinc::SDL_free(ids.cast());
        }
    }

    fn is_gamepad(&mut self, id: u32) -> bool {
        // SAFETY: a query by id; an unknown id reads false.
        unsafe { sdl3_sys::gamepad::SDL_IsGamepad(SDL_JoystickID(id)) }
    }

    fn joystick_type(&mut self, id: u32) -> i32 {
        // SAFETY: a query by id; an unknown id reads as unknown.
        unsafe { SDL_GetJoystickTypeForID(SDL_JoystickID(id)).0 }
    }

    fn open_joystick(&mut self, id: u32) -> Option<OpenedJoystick> {
        // SAFETY: opening by an id SDL reported; a null result is handled.
        let joystick = NonNull::new(unsafe { SDL_OpenJoystick(SDL_JoystickID(id)) })?;
        let j = joystick.as_ptr();
        let count = |n: std::ffi::c_int| usize::try_from(n).unwrap_or(0);
        // SAFETY: `j` is an open joystick; a haptic device that won't open
        // is left out, and one that does is closed with the joystick.
        let (opened, haptic) = unsafe {
            let mut identity: String = SDL_GetJoystickGUID(j).data.iter().map(|b| format!("{b:02x}")).collect();
            let serial = text(SDL_GetJoystickSerial(j));
            if !serial.is_empty() {
                identity.push('/');
                identity.push_str(&serial);
            }
            let haptic = if self.haptic && SDL_IsJoystickHaptic(j) {
                NonNull::new(SDL_OpenHapticFromJoystick(j))
            } else {
                None
            };
            let features = haptic.map_or(0, |h| SDL_GetHapticFeatures(h.as_ptr()));
            if let Some(h) = haptic
                && features & raw::HAPTIC_GAIN != 0
            {
                SDL_SetHapticGain(h.as_ptr(), 100);
            }
            let opened = OpenedJoystick {
                name: text(SDL_GetJoystickName(j)),
                vendor_id: SDL_GetJoystickVendor(j),
                product_id: SDL_GetJoystickProduct(j),
                kind: SDL_GetJoystickType(j).0,
                identity,
                axes: count(SDL_GetNumJoystickAxes(j)),
                buttons: count(SDL_GetNumJoystickButtons(j)),
                hats: count(SDL_GetNumJoystickHats(j)),
                haptic: features,
            };
            (opened, haptic)
        };
        self.joysticks.push(OpenJoystick {
            id,
            joystick,
            haptic,
            effects: [None; EFFECT_SLOTS],
        });
        Some(opened)
    }

    fn close_joystick(&mut self, id: u32) {
        if let Some(at) = self.joysticks.iter().position(|j| j.id == id) {
            close_joystick(self.joysticks.swap_remove(at));
        }
    }

    fn haptic_effect(&mut self, id: u32, slot: usize, effect: Option<Effect>) {
        let Some(j) = self.joystick(id) else { return };
        let (Some(h), Some(current)) = (j.haptic, j.effects.get_mut(slot)) else {
            return;
        };
        let h = h.as_ptr();
        // SAFETY: `h` is the open haptic device of an open joystick, and
        // the effect ids were made on it.
        unsafe {
            match (effect, current.as_mut()) {
                (None, Some((effect_id, playing))) => {
                    if *playing {
                        SDL_StopHapticEffect(h, *effect_id);
                        *playing = false;
                    }
                }
                (None, None) => {}
                (Some(effect), Some((effect_id, playing))) => {
                    SDL_UpdateHapticEffect(h, *effect_id, &sdl_effect(effect));
                    if !*playing {
                        *playing = SDL_RunHapticEffect(h, *effect_id, 1);
                    }
                }
                (Some(effect), None) => {
                    let effect_id = SDL_CreateHapticEffect(h, &sdl_effect(effect));
                    if effect_id.0 >= 0 {
                        *current = Some((effect_id, SDL_RunHapticEffect(h, effect_id, 1)));
                    }
                }
            }
        }
    }

    fn haptic_autocenter(&mut self, id: u32, level: u8) {
        if let Some(h) = self.joystick(id).and_then(|j| j.haptic) {
            // SAFETY: `h` is the open haptic device of an open joystick.
            unsafe { SDL_SetHapticAutocenter(h.as_ptr(), std::ffi::c_int::from(level.min(100))) };
        }
    }
}

#[cfg(test)]
mod tests {
    //! The calls into SDL, against SDL's own virtual gamepads: no device,
    //! no window. SDL is one process-wide library, so it is one test.
    use std::sync::atomic::{AtomicU32, Ordering};

    use sdl3_sys::gamepad::*;
    use sdl3_sys::haptic::*;
    use sdl3_sys::hints::{SDL_HINT_JOYSTICK_HIDAPI, SDL_SetHint};
    use sdl3_sys::joystick::*;
    use sdl3_sys::sensor::*;

    use super::Sdl;
    use crate::ctx::Ctx;
    use crate::gamepad_sdl::{PadSystem, SdlGamepads, raw};
    use crate::input::{ControllerFamily, GamepadAxis, GamepadButton, Rumble};

    static COLOR: AtomicU32 = AtomicU32::new(0);
    static EFFECT: std::sync::Mutex<Option<super::dualsense::Payload>> = std::sync::Mutex::new(None);

    unsafe extern "C" fn set_led(_: *mut std::ffi::c_void, r: u8, g: u8, b: u8) -> bool {
        COLOR.store(u32::from_be_bytes([0, r, g, b]), Ordering::SeqCst);
        true
    }

    unsafe extern "C" fn send_effect(_: *mut std::ffi::c_void, data: *const std::ffi::c_void, size: i32) -> bool {
        if data.is_null() || size != super::dualsense::PAYLOAD_SIZE as i32 {
            return false;
        }
        // SAFETY: SDL supplies a readable buffer of size bytes for this callback.
        let payload = unsafe { data.cast::<super::dualsense::Payload>().read() };
        *EFFECT.lock().unwrap() = Some(payload);
        true
    }

    static LOW: AtomicU32 = AtomicU32::new(0);
    static RIGHT_TRIGGER: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn rumble(_: *mut std::ffi::c_void, low: u16, _high: u16) -> bool {
        LOW.store(u32::from(low), Ordering::SeqCst);
        true
    }

    unsafe extern "C" fn rumble_triggers(_: *mut std::ffi::c_void, _left: u16, right: u16) -> bool {
        RIGHT_TRIGGER.store(u32::from(right), Ordering::SeqCst);
        true
    }

    #[test]
    fn sdls_numbers_are_the_ones_the_backend_uses() {
        assert_eq!(SDL_GAMEPAD_BUTTON_SOUTH.0, raw::BUTTON_SOUTH);
        assert_eq!(SDL_GAMEPAD_BUTTON_WEST.0, raw::BUTTON_WEST);
        assert_eq!(SDL_GAMEPAD_BUTTON_BACK.0, raw::BUTTON_BACK);
        assert_eq!(SDL_GAMEPAD_BUTTON_GUIDE.0, raw::BUTTON_GUIDE);
        assert_eq!(SDL_GAMEPAD_BUTTON_START.0, raw::BUTTON_START);
        assert_eq!(SDL_GAMEPAD_BUTTON_LEFT_SHOULDER.0, raw::BUTTON_LEFT_SHOULDER);
        assert_eq!(SDL_GAMEPAD_BUTTON_DPAD_RIGHT.0, raw::BUTTON_DPAD_RIGHT);
        assert_eq!(SDL_GAMEPAD_AXIS_RIGHTY.0, raw::AXIS_RIGHTY);
        assert_eq!(SDL_GAMEPAD_AXIS_RIGHT_TRIGGER.0, raw::AXIS_RIGHT_TRIGGER);
        assert_eq!(SDL_GAMEPAD_TYPE_STANDARD.0, raw::TYPE_STANDARD);
        assert_eq!(SDL_GAMEPAD_TYPE_XBOXONE.0, raw::TYPE_XBOXONE);
        assert_eq!(SDL_GAMEPAD_TYPE_PS5.0, raw::TYPE_PS5);
        assert_eq!(SDL_GAMEPAD_TYPE_NINTENDO_SWITCH_PRO.0, raw::TYPE_SWITCH_PRO);
        assert_eq!(SDL_GAMEPAD_TYPE_GAMECUBE.0, raw::TYPE_GAMECUBE);
        assert_eq!(SDL_SENSOR_ACCEL.0, raw::SENSOR_ACCEL);
        assert_eq!(SDL_SENSOR_GYRO.0, raw::SENSOR_GYRO);
        assert_eq!(SDL_JOYSTICK_TYPE_UNKNOWN.0, raw::JOYSTICK_UNKNOWN);
        assert_eq!(SDL_JOYSTICK_TYPE_GAMEPAD.0, raw::JOYSTICK_GAMEPAD);
        assert_eq!(SDL_JOYSTICK_TYPE_WHEEL.0, raw::JOYSTICK_WHEEL);
        assert_eq!(SDL_JOYSTICK_TYPE_ARCADE_STICK.0, raw::JOYSTICK_ARCADE_STICK);
        assert_eq!(SDL_JOYSTICK_TYPE_FLIGHT_STICK.0, raw::JOYSTICK_FLIGHT_STICK);
        assert_eq!(SDL_JOYSTICK_TYPE_DANCE_PAD.0, raw::JOYSTICK_DANCE_PAD);
        assert_eq!(SDL_JOYSTICK_TYPE_GUITAR.0, raw::JOYSTICK_GUITAR);
        assert_eq!(SDL_JOYSTICK_TYPE_DRUM_KIT.0, raw::JOYSTICK_DRUM_KIT);
        assert_eq!(SDL_JOYSTICK_TYPE_ARCADE_PAD.0, raw::JOYSTICK_ARCADE_PAD);
        assert_eq!(SDL_JOYSTICK_TYPE_THROTTLE.0, raw::JOYSTICK_THROTTLE);
        assert_eq!(u32::from(SDL_HAPTIC_CONSTANT.0), raw::HAPTIC_CONSTANT);
        assert_eq!(u32::from(SDL_HAPTIC_SINE.0), raw::HAPTIC_SINE);
        assert_eq!(u32::from(SDL_HAPTIC_SPRING.0), raw::HAPTIC_SPRING);
        assert_eq!(u32::from(SDL_HAPTIC_DAMPER.0), raw::HAPTIC_DAMPER);
        assert_eq!(u32::from(SDL_HAPTIC_FRICTION.0), raw::HAPTIC_FRICTION);
        assert_eq!(SDL_HAT_UP | SDL_HAT_RIGHT | SDL_HAT_DOWN | SDL_HAT_LEFT, 0b1111);
        assert_eq!(
            crate::input::hat_from_bits(SDL_HAT_LEFTUP),
            glam::IVec2::new(-1, -1),
            "SDL's bits are the ones hat_from_bits reads"
        );
    }

    #[test]
    fn effects_become_sdl_effects_on_the_steering_axis_that_play_until_stopped() {
        use crate::gamepad_sdl::Effect;
        let spring = super::sdl_effect(Effect::Spring {
            coeff: 100,
            center: -200,
        });
        // SAFETY: reading the union by the variant `sdl_effect` wrote.
        unsafe {
            assert_eq!(spring.r#type.0, SDL_HAPTIC_SPRING.0);
            assert_eq!(spring.condition.direction.r#type.0, SDL_HAPTIC_STEERING_AXIS.0);
            assert_eq!(spring.condition.length, SDL_HAPTIC_INFINITY);
            assert_eq!(spring.condition.right_coeff, [100; 3]);
            assert_eq!(spring.condition.left_coeff, [100; 3]);
            assert_eq!(spring.condition.center, [-200; 3]);
            assert_eq!(spring.condition.right_sat, [u16::MAX; 3]);
            let push = super::sdl_effect(Effect::Constant { level: -5 });
            assert_eq!(push.r#type.0, SDL_HAPTIC_CONSTANT.0);
            assert_eq!(push.constant.level, -5);
            assert_eq!(push.constant.length, SDL_HAPTIC_INFINITY);
            let shake = super::sdl_effect(Effect::Sine {
                magnitude: 7,
                period_ms: 25,
            });
            assert_eq!(shake.r#type.0, SDL_HAPTIC_SINE.0);
            assert_eq!((shake.periodic.magnitude, shake.periodic.period), (7, 25));
            for (effect, kind) in [
                (Effect::Damper { coeff: 1 }, SDL_HAPTIC_DAMPER),
                (Effect::Friction { coeff: 1 }, SDL_HAPTIC_FRICTION),
            ] {
                let e = super::sdl_effect(effect);
                assert_eq!(e.r#type.0, kind.0);
                assert_eq!(e.condition.center, [0; 3]);
            }
        }
    }

    /// A virtual Xbox-style pad with a gyro, an accelerometer, body and
    /// trigger motors goes through SDL into the engine: connection with
    /// its name, family and capabilities, a button, a trigger, motion,
    /// rumble on both kinds of motor, and its removal.
    #[test]
    fn a_virtual_sdl_pad_reaches_the_engine_with_motion_and_both_kinds_of_rumble() {
        // SAFETY (whole test): SDL calls with valid arguments; the
        // descriptor and the strings it points to outlive the attach call.
        unsafe {
            // Only virtual pads: SDL opens none of the machine's devices.
            SDL_SetHint(SDL_HINT_JOYSTICK_HIDAPI, c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_MFI".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_IOKIT".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_RAWINPUT".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_WGI".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_DIRECTINPUT".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_XINPUT".as_ptr(), c"0".as_ptr());
            SDL_SetHint(c"SDL_JOYSTICK_LINUX_CLASSIC".as_ptr(), c"0".as_ptr());
            let mut system = Sdl::start().expect("SDL's gamepad subsystem starts");
            assert!(system.open(u32::MAX).is_none(), "no such pad");
            assert!(!super::error().is_empty(), "SDL says why");
            let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
            ctx.set_gamepad_backend(SdlGamepads::new(system));
            assert!(ctx.input.gamepads().is_empty(), "no virtual pad yet");

            let sensors = [
                SDL_VirtualJoystickSensorDesc {
                    r#type: SDL_SENSOR_GYRO,
                    rate: 0.0,
                },
                SDL_VirtualJoystickSensorDesc {
                    r#type: SDL_SENSOR_ACCEL,
                    rate: 0.0,
                },
            ];
            // `default()` fills in the version (SDL_INIT_INTERFACE).
            let touchpad = SDL_VirtualJoystickTouchpadDesc {
                nfingers: 2,
                ..Default::default()
            };
            let desc = SDL_VirtualJoystickDesc {
                r#type: SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16,
                ntouchpads: 1,
                touchpads: &touchpad,
                vendor_id: 0x045e,
                product_id: 0x0b13,
                naxes: SDL_GAMEPAD_AXIS_COUNT.0 as u16,
                nbuttons: SDL_GAMEPAD_BUTTON_COUNT.0 as u16,
                nsensors: 2,
                sensors: sensors.as_ptr(),
                name: c"Virtual Test Pad".as_ptr(),
                SetLED: Some(set_led),
                SendEffect: Some(send_effect),
                Rumble: Some(rumble),
                RumbleTriggers: Some(rumble_triggers),
                ..SDL_VirtualJoystickDesc::default()
            };
            let id = SDL_AttachVirtualJoystick(&desc);
            assert_ne!(id.0, 0, "attached");
            let joystick = SDL_OpenJoystick(id);
            assert!(!joystick.is_null());

            ctx.poll_gamepads(1.0 / 60.0);
            let pads = ctx.input.gamepads().to_vec();
            assert_eq!(pads.len(), 1, "connected");
            let pad = pads[0];
            let info = ctx.input.gamepad_info(pad).unwrap().clone();
            assert_eq!(info.name, "Virtual Test Pad");
            assert_eq!(info.vendor_id, Some(0x045e));
            assert_eq!(info.family, ControllerFamily::Xbox);
            assert!(info.capabilities.rumble && info.capabilities.trigger_rumble && info.capabilities.motion);

            assert!(info.capabilities.light && info.capabilities.touchpad);
            assert!(!info.capabilities.adaptive_triggers && !info.capabilities.player_leds);
            SDL_SetJoystickVirtualButton(joystick, SDL_GAMEPAD_BUTTON_SOUTH.0, true);
            SDL_SetJoystickVirtualButton(joystick, SDL_GAMEPAD_BUTTON_TOUCHPAD.0, true);
            assert!(SDL_SetJoystickVirtualTouchpad(joystick, 0, 0, true, 0.25, 0.75, 0.5));
            assert!(SDL_SetJoystickVirtualTouchpad(joystick, 0, 1, true, 0.5, 0.25, 0.75));
            SDL_SetJoystickVirtualAxis(joystick, SDL_GAMEPAD_AXIS_RIGHT_TRIGGER.0, i16::MAX);
            let gyro = [0.5f32, -1.0, 2.0];
            let accel = [0.0f32, 9.81, 0.0];
            SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_GYRO, 1, gyro.as_ptr(), 3);
            SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_ACCEL, 1, accel.as_ptr(), 3);
            ctx.input.begin_frame(0.0);
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.gamepad_down(pad, GamepadButton::South));
            assert!(ctx.input.gamepad_pressed(pad, GamepadButton::Touchpad));
            let [first, second] = ctx.input.gamepad_touchpad(pad).unwrap();
            assert_eq!((first.x, first.y, first.pressure, first.down), (0.25, 0.75, 0.5, true));
            assert_eq!(
                (second.x, second.y, second.pressure, second.down),
                (0.5, 0.25, 0.75, true)
            );
            assert_eq!(
                ctx.input.gamepad_touchpad_events(),
                &[(pad, 0, first), (pad, 1, second)]
            );
            assert!(ctx.input.gamepad_down(pad, GamepadButton::RightTrigger));
            assert_eq!(ctx.input.gamepad_axis(pad, GamepadAxis::RightTrigger), 1.0);
            let motion = ctx.input.gamepad_motion(pad).expect("motion arrived");
            assert_eq!(motion.gyro, glam::Vec3::from(gyro));
            assert_eq!(motion.accel, glam::Vec3::from(accel));

            ctx.input.set_gamepad_light(pad, crate::color::Color::hex(0xff8000));
            ctx.input.end_frame();
            assert!(SDL_SetJoystickVirtualTouchpad(joystick, 0, 0, false, 0.0, 0.0, 0.0));
            ctx.input.begin_frame(0.0);
            ctx.input
                .rumble(pad, Rumble::new(1.0, 0.0).triggers(0.0, 0.5).seconds(10.0));
            ctx.poll_gamepads(1.0 / 60.0);
            // SDL sends rumble to a virtual pad when it updates it.
            SDL_UpdateJoysticks();
            assert_eq!(LOW.load(Ordering::SeqCst), 65535, "the body motor");
            assert_eq!(RIGHT_TRIGGER.load(Ordering::SeqCst), 32768, "the trigger motor");

            assert_eq!(COLOR.load(Ordering::SeqCst), 0xff8000);
            assert!(!ctx.input.gamepad_touchpad(pad).unwrap()[0].down);
            assert_eq!(ctx.input.gamepad_touchpad_events().len(), 1);

            // Exercise the send call without advertising DualSense support
            // on a virtual controller. It receives the common payload.
            let mut effect_layer = Sdl::start().unwrap();
            effect_layer.open(id.0).unwrap();
            let sample = super::dualsense::trigger_report([crate::input::TriggerEffect::Off; 2]);
            effect_layer.set_trigger_effects(id.0, [crate::input::TriggerEffect::Off; 2]);
            assert_eq!(*EFFECT.lock().unwrap(), Some(sample));
            effect_layer.set_player_leds(id.0, Some(2));
            assert_eq!(SDL_GetJoystickPlayerIndex(joystick), 2);
            effect_layer.set_player_leds(id.0, None);
            assert_eq!(SDL_GetJoystickPlayerIndex(joystick), -1);
            drop(effect_layer);

            SDL_CloseJoystick(joystick);
            SDL_DetachVirtualJoystick(id);
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.gamepads().is_empty(), "removed");

            // A virtual wheel: a joystick, with its axes, buttons and hat.
            // Virtual devices have no force feedback, so effects are
            // accepted and play nothing.
            let desc = SDL_VirtualJoystickDesc {
                r#type: SDL_JOYSTICK_TYPE_WHEEL.0 as u16,
                vendor_id: 0x046d,
                product_id: 0xc24f,
                naxes: 4,
                nbuttons: 20,
                nhats: 1,
                name: c"Virtual Test Wheel".as_ptr(),
                ..SDL_VirtualJoystickDesc::default()
            };
            let id = SDL_AttachVirtualJoystick(&desc);
            assert_ne!(id.0, 0, "attached");
            let joystick = SDL_OpenJoystick(id);
            assert!(!joystick.is_null());
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.gamepads().is_empty(), "not a gamepad");
            let wheels = ctx.input.joysticks().to_vec();
            assert_eq!(wheels.len(), 1, "a joystick");
            let wheel = wheels[0];
            let info = ctx.input.joystick_info(wheel).unwrap().clone();
            assert_eq!(info.name, "Virtual Test Wheel");
            assert_eq!(info.kind, crate::input::JoystickKind::Wheel);
            assert_eq!((info.axes, info.buttons, info.hats), (4, 20, 1));
            assert_eq!(info.vendor_id, Some(0x046d));
            assert!(!info.force_feedback.any(), "a virtual wheel has none");

            SDL_SetJoystickVirtualAxis(joystick, 0, i16::MIN);
            SDL_SetJoystickVirtualAxis(joystick, 2, i16::MAX);
            SDL_SetJoystickVirtualButton(joystick, 19, true);
            SDL_SetJoystickVirtualHat(joystick, 0, SDL_HAT_RIGHTDOWN);
            ctx.input.begin_frame(0.0);
            ctx.poll_gamepads(1.0 / 60.0);
            assert_eq!(ctx.input.joystick_axis(wheel, 0), -1.0);
            assert_eq!(ctx.input.joystick_axis(wheel, 2), 1.0);
            assert!(ctx.input.joystick_down(wheel, 19));
            assert_eq!(ctx.input.joystick_hat(wheel, 0), glam::IVec2::new(1, 1));
            ctx.input.set_force_feedback(
                wheel,
                crate::input::ForceFeedback {
                    spring: 1.0,
                    ..crate::input::ForceFeedback::OFF
                },
            );
            ctx.poll_gamepads(1.0 / 60.0);

            // The device layer on its own: a joystick opens and closes, and
            // effects on one without force feedback are accepted and play nothing.
            let mut layer = Sdl::start().expect("a second user of SDL starts too");
            let mut present = Vec::new();
            layer.present_joysticks(&mut present);
            assert!(present.contains(&id.0));
            assert!(!layer.is_gamepad(id.0));
            assert_eq!(layer.joystick_type(id.0), raw::JOYSTICK_WHEEL);
            let opened = layer.open_joystick(id.0).expect("opens");
            assert_eq!(opened.haptic, 0);
            layer.haptic_effect(
                id.0,
                1,
                Some(crate::gamepad_sdl::Effect::Spring { coeff: 1, center: 0 }),
            );
            layer.haptic_effect(id.0, 1, None);
            layer.haptic_autocenter(id.0, 50);
            layer.close_joystick(id.0);
            drop(layer);

            SDL_CloseJoystick(joystick);
            SDL_DetachVirtualJoystick(id);
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.joysticks().is_empty(), "removed");
        }
    }
}
