use super::{DeviceKey, Gamepad, GamepadBackend, Input, InputDevice};
use crate::actions::InputEvent;
use crate::color::Color;

/// One of the first two finger slots on a controller touchpad.
#[derive(Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GamepadTouchpadFinger {
    /// Horizontal position, 0 at the left and 1 at the right.
    pub x: f32,
    /// Vertical position, 0 at the top and 1 at the bottom.
    pub y: f32,
    /// Pressure from 0 to 1. Some controllers only report 0 or 1.
    pub pressure: f32,
    /// Whether a finger is on the surface.
    pub down: bool,
}

/// Resistance or vibration of a DualSense adaptive trigger. Its travel is
/// divided into ten zones, numbered 0 (released) through 9 (fully pulled).
/// Strengths run from 0 (off) through 8 (maximum). Values are clamped.
/// An effect persists until replaced, cleared or the controller goes away.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TriggerEffect {
    /// Free travel.
    #[default]
    Off,
    /// Constant resistance from `start` to the end of travel.
    Resistance {
        /// First active zone (0–9).
        start: u8,
        /// Resistance (0–8).
        strength: u8,
    },
    /// A distinct break between two positions.
    Weapon {
        /// Where resistance starts (2–7).
        start: u8,
        /// Break position (`start + 1` through 8).
        end: u8,
        /// Resistance (0–8).
        strength: u8,
    },
    /// Vibration from `start` to the end of travel.
    Vibration {
        /// First active zone (0–9).
        start: u8,
        /// Vibration amplitude (0–8).
        amplitude: u8,
        /// Frequency in hertz. Zero switches the effect off.
        frequency: u8,
    },
    /// Independent resistance for each of the ten zones.
    ResistanceProfile([u8; 10]),
}

/// The latest requested value of each output, with `None` for untouched
/// outputs. A negative player index explicitly switches the indicators off.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GamepadOutput {
    pub(crate) light: Option<[u8; 3]>,
    pub(crate) player: Option<i16>,
    pub(crate) triggers: Option<[TriggerEffect; 2]>,
}

impl GamepadOutput {
    pub(crate) fn send_changes(self, previous: Self, backend: &mut dyn GamepadBackend, device: DeviceKey) {
        if self.light != previous.light
            && let Some(rgb) = self.light
        {
            backend.set_light(device, rgb);
        }
        if self.player != previous.player
            && let Some(player) = self.player
        {
            backend.set_player_leds(device, u8::try_from(player).ok());
        }
        if self.triggers != previous.triggers
            && let Some(effects) = self.triggers
        {
            backend.set_trigger_effects(device, effects);
        }
    }

    pub(crate) fn reset(self, backend: &mut dyn GamepadBackend, device: DeviceKey) {
        let off = Self {
            light: self.light.map(|_| [0; 3]),
            player: self.player.map(|_| -1),
            triggers: self.triggers.map(|_| [TriggerEffect::Off; 2]),
        };
        off.send_changes(self, backend, device);
    }
}

fn unit(value: f32) -> f32 {
    if value.is_nan() { 0.0 } else { value.clamp(0.0, 1.0) }
}

impl Input {
    /// Sets the controller's light bar. Alpha is ignored, and black turns
    /// the bar off. A disconnected or unsupported controller is ignored.
    pub fn set_gamepad_light(&mut self, pad: Gamepad, color: Color) {
        if self.gamepad_info(pad).is_some_and(|i| i.capabilities.light) {
            let [r, g, b, _] = color.to_rgba8();
            self.controllers.output[pad.slot()].light = Some([r, g, b]);
        }
    }

    /// Sets the physical player indicator. `Some(0)` indicates player one,
    /// and `None` turns the LEDs off. Available patterns depend on the
    /// controller. This does not change Ponko's local player assignments.
    pub fn set_gamepad_player_leds(&mut self, pad: Gamepad, player: Option<u8>) {
        if self.gamepad_info(pad).is_some_and(|i| i.capabilities.player_leds) {
            self.controllers.output[pad.slot()].player = Some(player.map_or(-1, i16::from));
        }
    }

    /// Sets both adaptive triggers. Use `TriggerEffect::Off` on either side
    /// to clear that trigger. Pads without adaptive triggers are ignored.
    pub fn set_gamepad_trigger_effects(&mut self, pad: Gamepad, left: TriggerEffect, right: TriggerEffect) {
        if self.gamepad_info(pad).is_some_and(|i| i.capabilities.adaptive_triggers) {
            self.controllers.output[pad.slot()].triggers = Some([left, right]);
        }
    }

    /// The first two finger slots on the controller's first touchpad.
    /// Read `down` before using a position. Returns `None` when the
    /// controller is disconnected or the backend cannot read its touchpad.
    #[must_use]
    pub fn gamepad_touchpad(&self, pad: Gamepad) -> Option<[GamepadTouchpadFinger; 2]> {
        self.gamepad_info(pad)
            .filter(|i| i.capabilities.touchpad)
            .map(|_| self.controllers.touchpad[pad.slot()])
    }

    /// Touchpad contacts that changed this frame, in order, as
    /// `(gamepad, finger slot, contact)`. The finger slot is 0 or 1.
    #[must_use]
    pub fn gamepad_touchpad_events(&self) -> &[(Gamepad, u8, GamepadTouchpadFinger)] {
        &self.controllers.touchpad_events
    }

    pub(crate) fn set_gamepad_touchpad_finger(&mut self, pad: Gamepad, finger: u8, contact: GamepadTouchpadFinger) {
        if finger >= 2 || !self.gamepad_info(pad).is_some_and(|i| i.capabilities.touchpad) {
            return;
        }
        let contact = GamepadTouchpadFinger {
            x: unit(contact.x),
            y: unit(contact.y),
            pressure: unit(contact.pressure),
            down: contact.down,
        };
        let current = &mut self.controllers.touchpad[pad.slot()][usize::from(finger)];
        if *current == contact {
            return;
        }
        *current = contact;
        if contact.down {
            self.controllers.used(InputDevice::Gamepad(pad));
        }
        self.controllers.touchpad_events.push((pad, finger, contact));
        self.record(InputEvent::GamepadTouchpad(pad, finger, contact));
    }
}
