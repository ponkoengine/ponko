//! The [`PadSystem`] that calls SDL 3, for the `gamepad-sdl` backend
//! (`gamepad_sdl`, which has the rest and its scripted tests).

use std::ffi::CStr;
use std::ptr::NonNull;

use sdl3_sys::events::{SDL_Event, SDL_EventType, SDL_PollEvent};
use sdl3_sys::gamepad::{
    SDL_CloseGamepad, SDL_Gamepad, SDL_GamepadHasSensor, SDL_GetGamepadGUIDForID, SDL_GetGamepadName,
    SDL_GetGamepadProduct, SDL_GetGamepadProperties, SDL_GetGamepadSerial, SDL_GetGamepadType, SDL_GetGamepadVendor,
    SDL_GetGamepads, SDL_OpenGamepad, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN, SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN,
    SDL_RumbleGamepad, SDL_RumbleGamepadTriggers, SDL_SetGamepadSensorEnabled,
};
use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_InitSubSystem, SDL_QuitSubSystem};
use sdl3_sys::properties::SDL_GetBooleanProperty;
use sdl3_sys::sensor::{SDL_SENSOR_ACCEL, SDL_SENSOR_GYRO};

use crate::gamepad_sdl::{Opened, PadEvent, PadSystem};

/// SDL's gamepad subsystem, and the pads opened through it.
pub(crate) struct Sdl {
    open: Vec<(u32, NonNull<SDL_Gamepad>)>,
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
        if ok {
            Ok(Self { open: Vec::new() })
        } else {
            Err(error())
        }
    }

    fn pad(&self, id: u32) -> Option<*mut SDL_Gamepad> {
        self.open.iter().find(|(i, _)| *i == id).map(|(_, p)| p.as_ptr())
    }
}

impl Drop for Sdl {
    fn drop(&mut self) {
        for (_, pad) in self.open.drain(..) {
            // SAFETY: opened by `open` and not closed since.
            unsafe { SDL_CloseGamepad(pad.as_ptr()) };
        }
        // SAFETY: balances the `SDL_InitSubSystem` of `start`.
        unsafe { SDL_QuitSubSystem(SDL_INIT_GAMEPAD) };
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
                    SDL_EventType::GAMEPAD_SENSOR_UPDATE => {
                        PadEvent::Sensor(event.gsensor.which.0, event.gsensor.sensor, event.gsensor.data)
                    }
                    // Joystick events (the same pads, raw) and the rest.
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
            let props = SDL_GetGamepadProperties(p);
            Opened {
                name: text(SDL_GetGamepadName(p)),
                vendor_id: SDL_GetGamepadVendor(p),
                product_id: SDL_GetGamepadProduct(p),
                kind: SDL_GetGamepadType(p).0,
                identity,
                rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN, false),
                trigger_rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN, false),
                gyro: SDL_GamepadHasSensor(p, SDL_SENSOR_GYRO),
                accel: SDL_GamepadHasSensor(p, SDL_SENSOR_ACCEL),
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

    fn now_ms(&self) -> u64 {
        // SAFETY: a plain clock read, valid once SDL is initialised.
        unsafe { sdl3_sys::timer::SDL_GetTicks() }
    }
}

#[cfg(test)]
mod tests {
    //! The calls into SDL, against SDL's own virtual gamepads: no device,
    //! no window. SDL is one process-wide library, so it is one test.
    use std::sync::atomic::{AtomicU32, Ordering};

    use sdl3_sys::gamepad::*;
    use sdl3_sys::hints::{SDL_HINT_JOYSTICK_HIDAPI, SDL_SetHint};
    use sdl3_sys::joystick::*;
    use sdl3_sys::sensor::*;

    use super::Sdl;
    use crate::ctx::Ctx;
    use crate::gamepad_sdl::{PadSystem, SdlGamepads, raw};
    use crate::input::{ControllerFamily, GamepadAxis, GamepadButton, Rumble};

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
            let desc = SDL_VirtualJoystickDesc {
                r#type: SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16,
                vendor_id: 0x045e,
                product_id: 0x0b13,
                naxes: SDL_GAMEPAD_AXIS_COUNT.0 as u16,
                nbuttons: SDL_GAMEPAD_BUTTON_COUNT.0 as u16,
                nsensors: 2,
                sensors: sensors.as_ptr(),
                name: c"Virtual Test Pad".as_ptr(),
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

            SDL_SetJoystickVirtualButton(joystick, SDL_GAMEPAD_BUTTON_SOUTH.0, true);
            SDL_SetJoystickVirtualAxis(joystick, SDL_GAMEPAD_AXIS_RIGHT_TRIGGER.0, i16::MAX);
            let gyro = [0.5f32, -1.0, 2.0];
            let accel = [0.0f32, 9.81, 0.0];
            SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_GYRO, 1, gyro.as_ptr(), 3);
            SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_ACCEL, 1, accel.as_ptr(), 3);
            ctx.input.begin_frame(0.0);
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.gamepad_down(pad, GamepadButton::South));
            assert!(ctx.input.gamepad_down(pad, GamepadButton::RightTrigger));
            assert_eq!(ctx.input.gamepad_axis(pad, GamepadAxis::RightTrigger), 1.0);
            let motion = ctx.input.gamepad_motion(pad).expect("motion arrived");
            assert_eq!(motion.gyro, glam::Vec3::from(gyro));
            assert_eq!(motion.accel, glam::Vec3::from(accel));

            ctx.input
                .rumble(pad, Rumble::new(1.0, 0.0).triggers(0.0, 0.5).seconds(10.0));
            ctx.poll_gamepads(1.0 / 60.0);
            // SDL sends rumble to a virtual pad when it updates it.
            SDL_UpdateJoysticks();
            assert_eq!(LOW.load(Ordering::SeqCst), 65535, "the body motor");
            assert_eq!(RIGHT_TRIGGER.load(Ordering::SeqCst), 32768, "the trigger motor");

            SDL_CloseJoystick(joystick);
            SDL_DetachVirtualJoystick(id);
            ctx.poll_gamepads(1.0 / 60.0);
            assert!(ctx.input.gamepads().is_empty(), "removed");
        }
    }
}
