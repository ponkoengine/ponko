//! What the retained UI saw of this frame's input, so input a system feeds
//! later in the frame (a replay, a test, a bot) reaches the UI in the same
//! frame: the engine runs the UI over what is new since, between systems
//! (`Ctx::feed_ui_same_frame`), and the edges it saw already are hidden from
//! that pass so a press is not taken twice.

use glam::Vec2;

use super::{ButtonStates, GamepadButton, Input, Key, MAX_GAMEPADS, MouseButton};

/// The frame's edges: pressed and released this frame, for every button
/// the UI reads, plus the wheel and how much text was typed.
#[derive(Debug, Clone)]
pub(crate) struct Edges {
    keys: ButtonStates<{ Key::COUNT }>,
    mouse: ButtonStates<{ MouseButton::COUNT }>,
    pads: [ButtonStates<{ GamepadButton::COUNT }>; MAX_GAMEPADS],
    wheel: Vec2,
    text_len: usize,
    pointer_pressed: bool,
    pointer_released: bool,
}

impl Default for Edges {
    fn default() -> Self {
        Self {
            keys: ButtonStates::new(),
            mouse: ButtonStates::new(),
            pads: [const { ButtonStates::new() }; MAX_GAMEPADS],
            wheel: Vec2::ZERO,
            text_len: 0,
            pointer_pressed: false,
            pointer_released: false,
        }
    }
}

/// The UI's view of this frame's input.
#[derive(Debug, Clone, Default)]
pub(crate) struct UiSeen {
    /// The UI took this frame's input (set after its pass at the start of
    /// the frame, cleared when the frame's input is).
    marked: bool,
    /// Input was fed since the UI last took it.
    pub(crate) fed: bool,
    /// The edges the UI took.
    seen: Edges,
    /// The frame's edges while a same-frame pass sees only the new ones.
    held_back: Edges,
    /// The typed text the UI took, held back during a same-frame pass.
    text: String,
}

/// Keeps in `now` the edges `seen` doesn't have, and returns them as
/// they were.
fn hide<const N: usize>(now: &mut ButtonStates<N>, seen: &ButtonStates<N>, back: &mut ButtonStates<N>) {
    back.pressed = now.pressed;
    back.released = now.released;
    for i in 0..N {
        now.pressed[i] &= !seen.pressed[i];
        now.released[i] &= !seen.released[i];
    }
}

impl Input {
    /// The retained UI took this frame's input: what a system feeds from
    /// now on is new to it.
    pub(crate) fn mark_ui_seen(&mut self) {
        let (pressed, released) = self.touch.pointer_edges();
        let s = &mut self.ui_seen.seen;
        s.keys.pressed = self.keys.pressed;
        s.keys.released = self.keys.released;
        s.mouse.pressed = self.mouse.pressed;
        s.mouse.released = self.mouse.released;
        for (seen, pad) in s.pads.iter_mut().zip(&self.pads) {
            seen.pressed = pad.buttons.pressed;
            seen.released = pad.buttons.released;
        }
        s.wheel = self.mouse_wheel;
        s.text_len = self.typed_text.len();
        s.pointer_pressed = pressed;
        s.pointer_released = released;
        self.ui_seen.marked = true;
        self.ui_seen.fed = false;
    }

    /// A system fed input: when the UI already took this frame's, it takes
    /// the new input after this system.
    pub(crate) fn fed_after_ui(&mut self) {
        self.ui_seen.fed |= self.ui_seen.marked;
    }

    /// Whether input was fed since the UI took it this frame.
    #[inline]
    pub(crate) fn fed_since_ui(&self) -> bool {
        self.ui_seen.fed
    }

    /// This frame's input from now until [`show_seen_edges`](Self::show_seen_edges)
    /// has only the edges the UI did not take yet: a press it saw is not
    /// pressed again, the text it saw is not typed again.
    pub(crate) fn hide_seen_edges(&mut self) {
        let UiSeen {
            seen, held_back, text, ..
        } = &mut self.ui_seen;
        hide(&mut self.keys, &seen.keys, &mut held_back.keys);
        hide(&mut self.mouse, &seen.mouse, &mut held_back.mouse);
        for ((pad, seen), back) in self.pads.iter_mut().zip(&seen.pads).zip(&mut held_back.pads) {
            hide(&mut pad.buttons, seen, back);
        }
        held_back.wheel = self.mouse_wheel;
        self.mouse_wheel -= seen.wheel;
        let typed = seen.text_len.min(self.typed_text.len());
        text.clear();
        text.push_str(&self.typed_text[..typed]);
        self.typed_text.drain(..typed);
        let (pressed, released) = self.touch.pointer_edges();
        held_back.pointer_pressed = pressed;
        held_back.pointer_released = released;
        self.touch
            .set_pointer_edges(pressed && !seen.pointer_pressed, released && !seen.pointer_released);
    }

    /// Puts back the edges [`hide_seen_edges`](Self::hide_seen_edges) hid:
    /// the frame's input is whole again, and the UI has taken all of it.
    pub(crate) fn show_seen_edges(&mut self) {
        let UiSeen { held_back, text, .. } = &mut self.ui_seen;
        self.keys.pressed = held_back.keys.pressed;
        self.keys.released = held_back.keys.released;
        self.mouse.pressed = held_back.mouse.pressed;
        self.mouse.released = held_back.mouse.released;
        for (pad, back) in self.pads.iter_mut().zip(&held_back.pads) {
            pad.buttons.pressed = back.pressed;
            pad.buttons.released = back.released;
        }
        self.mouse_wheel = held_back.wheel;
        self.typed_text.insert_str(0, text);
        self.touch
            .set_pointer_edges(held_back.pointer_pressed, held_back.pointer_released);
        self.mark_ui_seen();
    }

    /// The frame's input is cleared: the next frame's UI pass takes it anew.
    pub(crate) fn end_ui_frame(&mut self) {
        self.ui_seen.marked = false;
        self.ui_seen.fed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::InputEvent;

    #[test]
    fn hidden_edges_are_only_those_fed_after_the_ui_took_its_input() {
        let mut input = Input::new();
        input.feed(InputEvent::Key(Key::Tab, true));
        input.add_text("ab");
        input.feed(InputEvent::MouseWheel(Vec2::new(0.0, 1.0)));
        input.mark_ui_seen();
        assert!(!input.fed_since_ui());
        input.feed(InputEvent::Key(Key::Enter, true));
        input.add_text("c");
        input.feed(InputEvent::MouseWheel(Vec2::new(0.0, 2.0)));
        assert!(input.fed_since_ui());
        input.hide_seen_edges();
        assert!(!input.key_pressed(Key::Tab), "the UI saw Tab already");
        assert!(input.key_down(Key::Tab), "it is still held");
        assert!(input.key_pressed(Key::Enter));
        assert_eq!(input.typed_text(), "c");
        assert_eq!(input.mouse_wheel(), Vec2::new(0.0, 2.0));
        input.show_seen_edges();
        assert!(input.key_pressed(Key::Tab) && input.key_pressed(Key::Enter));
        assert_eq!(input.typed_text(), "abc");
        assert_eq!(input.mouse_wheel(), Vec2::new(0.0, 3.0));
        assert!(!input.fed_since_ui(), "the UI has taken everything");
    }

    #[test]
    fn input_fed_before_the_ui_took_it_is_not_fed_after() {
        let mut input = Input::new();
        input.feed(InputEvent::Key(Key::Tab, true));
        assert!(!input.fed_since_ui(), "the frame's UI pass has not run yet");
        input.mark_ui_seen();
        input.end_frame();
        input.feed(InputEvent::Key(Key::Tab, false));
        assert!(!input.fed_since_ui(), "a new frame: its UI pass takes it");
    }

    #[test]
    fn a_tap_fed_after_the_ui_is_a_new_press_and_release_of_the_touch_pointer() {
        let mut input = Input::new();
        input.mark_ui_seen();
        input.feed(InputEvent::touch_start(1, Vec2::new(10.0, 10.0)));
        input.feed(InputEvent::touch_end(1, Vec2::new(10.0, 10.0)));
        input.hide_seen_edges();
        assert_eq!(input.ui_pointer(), (Vec2::new(10.0, 10.0), true, false, true));
        input.show_seen_edges();
        // Taken: a second pass sees no new press.
        input.hide_seen_edges();
        assert_eq!(input.ui_pointer(), (Vec2::new(10.0, 10.0), false, false, false));
        input.show_seen_edges();
    }
}
