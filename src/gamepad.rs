//! The built-in gamepad backend on the desktop: gilrs (the `gamepad`
//! feature), installed by the window before setup. Web builds read the
//! browser's gamepads themselves (`gamepad_web`).
//!
//! What gilrs 0.11 can do differs per platform (see the guide's table):
//! rumble of the two body motors through its force-feedback API on Linux
//! (evdev) and Windows; nothing on macOS. It reads no gyro or
//! accelerometer and drives no trigger motors, so those capabilities are
//! reported `false`.

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
        }
    }

    fn key(id: GamepadId) -> DeviceKey {
        DeviceKey(usize::from(id) as u64)
    }

    fn info(gilrs: &Gilrs, id: GamepadId) -> GamepadInfo {
        let pad = gilrs.gamepad(id);
        GamepadInfo {
            name: pad.name().to_string(),
            family: ControllerFamily::detect(pad.vendor_id(), pad.product_id(), pad.name()),
            vendor_id: pad.vendor_id(),
            product_id: pad.product_id(),
            capabilities: GamepadCapabilities {
                rumble: pad.is_ff_supported(),
                trigger_rumble: false,
                motion: false,
                ..Default::default()
            },
        }
    }
}

impl GamepadBackend for GilrsBackend {
    fn poll(&mut self, events: &mut GamepadEvents<'_>) {
        let Some(gilrs) = self.gilrs.as_mut() else { return };
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
        assert_eq!(mapped.len(), GamepadButton::COUNT - 1); // gilrs has no touchpad click
        assert_eq!(button(Button::Mode), None);
    }

    #[test]
    fn stick_y_points_down() {
        assert_eq!(axis(Axis::LeftStickY), Some((GamepadAxis::LeftStickY, -1.0)));
        assert_eq!(axis(Axis::LeftStickX), Some((GamepadAxis::LeftStickX, 1.0)));
        assert_eq!(axis(Axis::DPadX), None);
    }
}
