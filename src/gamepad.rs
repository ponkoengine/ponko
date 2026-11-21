//! The built-in gamepad backend: gilrs (the `gamepad` feature), installed
//! by the window before setup.
//!
//! What gilrs 0.11 can do differs per platform (see the guide's table):
//! rumble of the two body motors through its force-feedback API on Linux
//! (evdev) and Windows; nothing on macOS. On the web its backend has no
//! force feedback either, so there the engine plays rumble itself through
//! the browser's `vibrationActuator` (`web_rumble`), finding each pad in
//! `navigator.getGamepads()` by its name, with the trigger motors where
//! the browser's actuator plays `trigger-rumble`. It reads no gyro or
//! accelerometer anywhere and drives no trigger motors on the desktop, so
//! those capabilities are reported `false` there.

use gilrs::ff::Effect;
use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};

use crate::input::{
    ControllerFamily, DeviceKey, GamepadAxis, GamepadBackend, GamepadButton, GamepadCapabilities, GamepadEvents,
    GamepadInfo, MotorLevels,
};

/// Reads gilrs's pads and plays rumble on them.
pub(crate) struct GilrsBackend {
    gilrs: Option<Gilrs>,
    /// The rumble effect playing on each pad, by gilrs id. Dropping one stops it.
    effects: Vec<(GamepadId, Effect)>,
    /// The pads already there when it opened, reported on the first poll.
    initial: Vec<GamepadId>,
    /// The browser's rumble, which gilrs has no force feedback for.
    #[cfg(target_arch = "wasm32")]
    web: WebRumbleState,
}

/// What a web build keeps to rumble pads that gilrs reads.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct WebRumbleState {
    rumble: crate::web_rumble::WebRumble,
    browser: crate::web_rumble::Browser,
}

impl GilrsBackend {
    /// Opens the gamepad system. Without it (no permission, unsupported
    /// system), there are no pads.
    pub(crate) fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(gilrs) => Some(gilrs),
            Err(gilrs::Error::NotImplemented(gilrs)) => Some(gilrs),
            Err(e) => {
                eprintln!("ponko: gamepads unavailable ({e})");
                None
            }
        };
        let initial = gilrs.iter().flat_map(|g| g.gamepads().map(|(id, _)| id)).collect();
        Self {
            gilrs,
            effects: Vec::new(),
            initial,
            #[cfg(target_arch = "wasm32")]
            web: WebRumbleState::default(),
        }
    }

    fn key(id: GamepadId) -> DeviceKey {
        DeviceKey(usize::from(id) as u64)
    }

    /// The browser's pad that gilrs's `id` is (gilrs doesn't say where in
    /// `navigator.getGamepads()` a pad is): the pad whose `id` text is the
    /// name gilrs has, counting the pads of that name in connection order.
    #[cfg(target_arch = "wasm32")]
    fn browser_pad(gilrs: &Gilrs, id: GamepadId) -> Option<crate::web_rumble::BrowserPad> {
        let name = gilrs.gamepad(id).os_name().to_string();
        let nth = gilrs
            .gamepads()
            .filter(|(other, pad)| pad.is_connected() && pad.os_name() == name && usize::from(*other) < usize::from(id))
            .count();
        let pads = crate::web_rumble::Browser::pads();
        crate::web_rumble::pick(&pads, &name, nth).cloned()
    }

    fn info(gilrs: &Gilrs, id: GamepadId) -> GamepadInfo {
        let pad = gilrs.gamepad(id);
        #[cfg(target_arch = "wasm32")]
        let (web_rumble, trigger_rumble) =
            Self::browser_pad(gilrs, id).map_or((false, false), |p| (p.rumble, p.trigger_rumble));
        #[cfg(not(target_arch = "wasm32"))]
        let (web_rumble, trigger_rumble) = (false, false);
        GamepadInfo {
            name: pad.name().to_string(),
            family: ControllerFamily::detect(pad.vendor_id(), pad.product_id(), pad.name()),
            vendor_id: pad.vendor_id(),
            product_id: pad.product_id(),
            capabilities: GamepadCapabilities {
                rumble: pad.is_ff_supported() || web_rumble,
                trigger_rumble,
                motion: false,
            },
        }
    }
}

impl GamepadBackend for GilrsBackend {
    fn poll(&mut self, events: &mut GamepadEvents<'_>) {
        let Some(gilrs) = self.gilrs.as_mut() else { return };
        #[cfg(target_arch = "wasm32")]
        self.web.rumble.refresh(&mut self.web.browser);
        for id in std::mem::take(&mut self.initial) {
            events.connected(Self::key(id), Self::info(gilrs, id));
        }
        while let Some(event) = gilrs.next_event() {
            let device = Self::key(event.id);
            match event.event {
                EventType::Connected => {
                    events.connected(device, Self::info(gilrs, event.id));
                }
                EventType::Disconnected => {
                    self.effects.retain(|(id, _)| *id != event.id);
                    #[cfg(target_arch = "wasm32")]
                    self.web.rumble.forget(device);
                    events.disconnected(device);
                }
                EventType::ButtonPressed(b, _) => {
                    if let Some(b) = button(b) {
                        events.button(device, b, true);
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(b) = button(b) {
                        events.button(device, b, false);
                    }
                }
                EventType::ButtonChanged(b, value, _) => match b {
                    Button::LeftTrigger2 => events.axis(device, GamepadAxis::LeftTrigger, value),
                    Button::RightTrigger2 => events.axis(device, GamepadAxis::RightTrigger, value),
                    _ => {}
                },
                EventType::AxisChanged(a, value, _) => {
                    if let Some((axis, sign)) = axis(a) {
                        events.axis(device, axis, value * sign);
                    }
                }
                _ => {}
            }
        }
    }

    /// On the web: the browser's `vibrationActuator` (gilrs has no force
    /// feedback there), replayed before the browser ends each effect.
    #[cfg(target_arch = "wasm32")]
    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        let Some(gilrs) = self.gilrs.as_ref() else { return };
        let Some(id) = gilrs.gamepads().map(|(id, _)| id).find(|&id| Self::key(id) == device) else {
            return;
        };
        match Self::browser_pad(gilrs, id) {
            Some(pad) => self
                .web
                .rumble
                .set(&mut self.web.browser, device, pad.index, pad.trigger_rumble, levels),
            // Its browser pad is gone: nothing to play or to stop.
            None => self.web.rumble.forget(device),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
        let Some(gilrs) = self.gilrs.as_mut() else { return };
        let Some(id) = gilrs.gamepads().map(|(id, _)| id).find(|&id| Self::key(id) == device) else {
            return;
        };
        // A new effect replaces the old one (dropping it stops it).
        self.effects.retain(|(i, _)| *i != id);
        if levels.is_off() || !gilrs.gamepad(id).is_ff_supported() {
            return;
        }
        let magnitude = |v: f32| (v.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16;
        // Plays until replaced: the engine times the rumble and sends OFF.
        let scheduling = Replay {
            play_for: Ticks::from_ms(u32::MAX / 2),
            ..Replay::default()
        };
        let effect = EffectBuilder::new()
            .add_effect(BaseEffect {
                kind: BaseEffectType::Strong {
                    magnitude: magnitude(levels.strong),
                },
                scheduling,
                ..BaseEffect::default()
            })
            .add_effect(BaseEffect {
                kind: BaseEffectType::Weak {
                    magnitude: magnitude(levels.weak),
                },
                scheduling,
                ..BaseEffect::default()
            })
            .gamepads(&[id])
            .finish(gilrs);
        match effect {
            Ok(effect) => {
                if effect.play().is_ok() {
                    self.effects.push((id, effect));
                }
            }
            Err(e) => eprintln!("ponko: rumble failed on a gamepad ({e})"),
        }
    }
}

/// gilrs's button names (by position) to ours.
fn button(b: Button) -> Option<GamepadButton> {
    use GamepadButton as G;
    Some(match b {
        Button::South => G::South,
        Button::East => G::East,
        Button::North => G::North,
        Button::West => G::West,
        Button::LeftTrigger => G::LeftBumper,
        Button::RightTrigger => G::RightBumper,
        Button::LeftTrigger2 => G::LeftTrigger,
        Button::RightTrigger2 => G::RightTrigger,
        Button::Select => G::Select,
        Button::Start => G::Start,
        Button::LeftThumb => G::LeftStick,
        Button::RightThumb => G::RightStick,
        Button::DPadUp => G::DPadUp,
        Button::DPadDown => G::DPadDown,
        Button::DPadLeft => G::DPadLeft,
        Button::DPadRight => G::DPadRight,
        _ => return None,
    })
}

/// gilrs's axes to ours, with the sign that makes y point down.
fn axis(a: Axis) -> Option<(GamepadAxis, f32)> {
    Some(match a {
        Axis::LeftStickX => (GamepadAxis::LeftStickX, 1.0),
        Axis::LeftStickY => (GamepadAxis::LeftStickY, -1.0),
        Axis::RightStickX => (GamepadAxis::RightStickX, 1.0),
        Axis::RightStickY => (GamepadAxis::RightStickY, -1.0),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_button_has_a_gilrs_button() {
        let all = [
            Button::South,
            Button::East,
            Button::North,
            Button::West,
            Button::LeftTrigger,
            Button::RightTrigger,
            Button::LeftTrigger2,
            Button::RightTrigger2,
            Button::Select,
            Button::Start,
            Button::LeftThumb,
            Button::RightThumb,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ];
        let mapped: std::collections::HashSet<_> = all.iter().filter_map(|&b| button(b)).collect();
        assert_eq!(mapped.len(), GamepadButton::COUNT);
        assert_eq!(button(Button::Mode), None);
    }

    #[test]
    fn stick_y_points_down() {
        assert_eq!(axis(Axis::LeftStickY), Some((GamepadAxis::LeftStickY, -1.0)));
        assert_eq!(axis(Axis::LeftStickX), Some((GamepadAxis::LeftStickX, 1.0)));
        assert_eq!(axis(Axis::DPadX), None);
    }
}
