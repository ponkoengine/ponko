//! Keyboard keys, mouse buttons and gamepad inputs as C enums, in the same
//! order as the Rust ones.
use ponko::input::GamepadAxis;
use ponko::input::GamepadButton;
use ponko::prelude::{Key, MouseButton};

/// A keyboard key, by position on a US layout (`PONKO_KEY_A`, ...).
/// ```
/// assert_eq!(ponko_ffi::PonkoKey::PONKO_KEY_A as u32, 0);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoKey {
    /// `A`.
    PONKO_KEY_A,
    /// `B`.
    PONKO_KEY_B,
    /// `C`.
    PONKO_KEY_C,
    /// `D`.
    PONKO_KEY_D,
    /// `E`.
    PONKO_KEY_E,
    /// `F`.
    PONKO_KEY_F,
    /// `G`.
    PONKO_KEY_G,
    /// `H`.
    PONKO_KEY_H,
    /// `I`.
    PONKO_KEY_I,
    /// `J`.
    PONKO_KEY_J,
    /// `K`.
    PONKO_KEY_K,
    /// `L`.
    PONKO_KEY_L,
    /// `M`.
    PONKO_KEY_M,
    /// `N`.
    PONKO_KEY_N,
    /// `O`.
    PONKO_KEY_O,
    /// `P`.
    PONKO_KEY_P,
    /// `Q`.
    PONKO_KEY_Q,
    /// `R`.
    PONKO_KEY_R,
    /// `S`.
    PONKO_KEY_S,
    /// `T`.
    PONKO_KEY_T,
    /// `U`.
    PONKO_KEY_U,
    /// `V`.
    PONKO_KEY_V,
    /// `W`.
    PONKO_KEY_W,
    /// `X`.
    PONKO_KEY_X,
    /// `Y`.
    PONKO_KEY_Y,
    /// `Z`.
    PONKO_KEY_Z,
    /// `Num0`.
    PONKO_KEY_NUM_0,
    /// `Num1`.
    PONKO_KEY_NUM_1,
    /// `Num2`.
    PONKO_KEY_NUM_2,
    /// `Num3`.
    PONKO_KEY_NUM_3,
    /// `Num4`.
    PONKO_KEY_NUM_4,
    /// `Num5`.
    PONKO_KEY_NUM_5,
    /// `Num6`.
    PONKO_KEY_NUM_6,
    /// `Num7`.
    PONKO_KEY_NUM_7,
    /// `Num8`.
    PONKO_KEY_NUM_8,
    /// `Num9`.
    PONKO_KEY_NUM_9,
    /// `F1`.
    PONKO_KEY_F1,
    /// `F2`.
    PONKO_KEY_F2,
    /// `F3`.
    PONKO_KEY_F3,
    /// `F4`.
    PONKO_KEY_F4,
    /// `F5`.
    PONKO_KEY_F5,
    /// `F6`.
    PONKO_KEY_F6,
    /// `F7`.
    PONKO_KEY_F7,
    /// `F8`.
    PONKO_KEY_F8,
    /// `F9`.
    PONKO_KEY_F9,
    /// `F10`.
    PONKO_KEY_F10,
    /// `F11`.
    PONKO_KEY_F11,
    /// `F12`.
    PONKO_KEY_F12,
    /// `Up`.
    PONKO_KEY_UP,
    /// `Down`.
    PONKO_KEY_DOWN,
    /// `Left`.
    PONKO_KEY_LEFT,
    /// `Right`.
    PONKO_KEY_RIGHT,
    /// `Space`.
    PONKO_KEY_SPACE,
    /// `Enter`.
    PONKO_KEY_ENTER,
    /// `Escape`.
    PONKO_KEY_ESCAPE,
    /// `Tab`.
    PONKO_KEY_TAB,
    /// `Backspace`.
    PONKO_KEY_BACKSPACE,
    /// `CapsLock`.
    PONKO_KEY_CAPS_LOCK,
    /// `Insert`.
    PONKO_KEY_INSERT,
    /// `Delete`.
    PONKO_KEY_DELETE,
    /// `Home`.
    PONKO_KEY_HOME,
    /// `End`.
    PONKO_KEY_END,
    /// `PageUp`.
    PONKO_KEY_PAGE_UP,
    /// `PageDown`.
    PONKO_KEY_PAGE_DOWN,
    /// `LeftShift`.
    PONKO_KEY_LEFT_SHIFT,
    /// `RightShift`.
    PONKO_KEY_RIGHT_SHIFT,
    /// `LeftCtrl`.
    PONKO_KEY_LEFT_CTRL,
    /// `RightCtrl`.
    PONKO_KEY_RIGHT_CTRL,
    /// `LeftAlt`.
    PONKO_KEY_LEFT_ALT,
    /// `RightAlt`.
    PONKO_KEY_RIGHT_ALT,
    /// `LeftSuper`.
    PONKO_KEY_LEFT_SUPER,
    /// `RightSuper`.
    PONKO_KEY_RIGHT_SUPER,
    /// `Minus`.
    PONKO_KEY_MINUS,
    /// `Equal`.
    PONKO_KEY_EQUAL,
    /// `LeftBracket`.
    PONKO_KEY_LEFT_BRACKET,
    /// `RightBracket`.
    PONKO_KEY_RIGHT_BRACKET,
    /// `Backslash`.
    PONKO_KEY_BACKSLASH,
    /// `Semicolon`.
    PONKO_KEY_SEMICOLON,
    /// `Apostrophe`.
    PONKO_KEY_APOSTROPHE,
    /// `Backquote`.
    PONKO_KEY_BACKQUOTE,
    /// `Comma`.
    PONKO_KEY_COMMA,
    /// `Period`.
    PONKO_KEY_PERIOD,
    /// `Slash`.
    PONKO_KEY_SLASH,
    /// `Numpad0`.
    PONKO_KEY_NUMPAD_0,
    /// `Numpad1`.
    PONKO_KEY_NUMPAD_1,
    /// `Numpad2`.
    PONKO_KEY_NUMPAD_2,
    /// `Numpad3`.
    PONKO_KEY_NUMPAD_3,
    /// `Numpad4`.
    PONKO_KEY_NUMPAD_4,
    /// `Numpad5`.
    PONKO_KEY_NUMPAD_5,
    /// `Numpad6`.
    PONKO_KEY_NUMPAD_6,
    /// `Numpad7`.
    PONKO_KEY_NUMPAD_7,
    /// `Numpad8`.
    PONKO_KEY_NUMPAD_8,
    /// `Numpad9`.
    PONKO_KEY_NUMPAD_9,
    /// `NumpadAdd`.
    PONKO_KEY_NUMPAD_ADD,
    /// `NumpadSubtract`.
    PONKO_KEY_NUMPAD_SUBTRACT,
    /// `NumpadMultiply`.
    PONKO_KEY_NUMPAD_MULTIPLY,
    /// `NumpadDivide`.
    PONKO_KEY_NUMPAD_DIVIDE,
    /// `NumpadDecimal`.
    PONKO_KEY_NUMPAD_DECIMAL,
    /// `NumpadEnter`.
    PONKO_KEY_NUMPAD_ENTER,
}

pub(crate) fn key(k: PonkoKey) -> Key {
    use PonkoKey::*;
    match k {
        PONKO_KEY_A => Key::A,
        PONKO_KEY_B => Key::B,
        PONKO_KEY_C => Key::C,
        PONKO_KEY_D => Key::D,
        PONKO_KEY_E => Key::E,
        PONKO_KEY_F => Key::F,
        PONKO_KEY_G => Key::G,
        PONKO_KEY_H => Key::H,
        PONKO_KEY_I => Key::I,
        PONKO_KEY_J => Key::J,
        PONKO_KEY_K => Key::K,
        PONKO_KEY_L => Key::L,
        PONKO_KEY_M => Key::M,
        PONKO_KEY_N => Key::N,
        PONKO_KEY_O => Key::O,
        PONKO_KEY_P => Key::P,
        PONKO_KEY_Q => Key::Q,
        PONKO_KEY_R => Key::R,
        PONKO_KEY_S => Key::S,
        PONKO_KEY_T => Key::T,
        PONKO_KEY_U => Key::U,
        PONKO_KEY_V => Key::V,
        PONKO_KEY_W => Key::W,
        PONKO_KEY_X => Key::X,
        PONKO_KEY_Y => Key::Y,
        PONKO_KEY_Z => Key::Z,
        PONKO_KEY_NUM_0 => Key::Num0,
        PONKO_KEY_NUM_1 => Key::Num1,
        PONKO_KEY_NUM_2 => Key::Num2,
        PONKO_KEY_NUM_3 => Key::Num3,
        PONKO_KEY_NUM_4 => Key::Num4,
        PONKO_KEY_NUM_5 => Key::Num5,
        PONKO_KEY_NUM_6 => Key::Num6,
        PONKO_KEY_NUM_7 => Key::Num7,
        PONKO_KEY_NUM_8 => Key::Num8,
        PONKO_KEY_NUM_9 => Key::Num9,
        PONKO_KEY_F1 => Key::F1,
        PONKO_KEY_F2 => Key::F2,
        PONKO_KEY_F3 => Key::F3,
        PONKO_KEY_F4 => Key::F4,
        PONKO_KEY_F5 => Key::F5,
        PONKO_KEY_F6 => Key::F6,
        PONKO_KEY_F7 => Key::F7,
        PONKO_KEY_F8 => Key::F8,
        PONKO_KEY_F9 => Key::F9,
        PONKO_KEY_F10 => Key::F10,
        PONKO_KEY_F11 => Key::F11,
        PONKO_KEY_F12 => Key::F12,
        PONKO_KEY_UP => Key::Up,
        PONKO_KEY_DOWN => Key::Down,
        PONKO_KEY_LEFT => Key::Left,
        PONKO_KEY_RIGHT => Key::Right,
        PONKO_KEY_SPACE => Key::Space,
        PONKO_KEY_ENTER => Key::Enter,
        PONKO_KEY_ESCAPE => Key::Escape,
        PONKO_KEY_TAB => Key::Tab,
        PONKO_KEY_BACKSPACE => Key::Backspace,
        PONKO_KEY_CAPS_LOCK => Key::CapsLock,
        PONKO_KEY_INSERT => Key::Insert,
        PONKO_KEY_DELETE => Key::Delete,
        PONKO_KEY_HOME => Key::Home,
        PONKO_KEY_END => Key::End,
        PONKO_KEY_PAGE_UP => Key::PageUp,
        PONKO_KEY_PAGE_DOWN => Key::PageDown,
        PONKO_KEY_LEFT_SHIFT => Key::LeftShift,
        PONKO_KEY_RIGHT_SHIFT => Key::RightShift,
        PONKO_KEY_LEFT_CTRL => Key::LeftCtrl,
        PONKO_KEY_RIGHT_CTRL => Key::RightCtrl,
        PONKO_KEY_LEFT_ALT => Key::LeftAlt,
        PONKO_KEY_RIGHT_ALT => Key::RightAlt,
        PONKO_KEY_LEFT_SUPER => Key::LeftSuper,
        PONKO_KEY_RIGHT_SUPER => Key::RightSuper,
        PONKO_KEY_MINUS => Key::Minus,
        PONKO_KEY_EQUAL => Key::Equal,
        PONKO_KEY_LEFT_BRACKET => Key::LeftBracket,
        PONKO_KEY_RIGHT_BRACKET => Key::RightBracket,
        PONKO_KEY_BACKSLASH => Key::Backslash,
        PONKO_KEY_SEMICOLON => Key::Semicolon,
        PONKO_KEY_APOSTROPHE => Key::Apostrophe,
        PONKO_KEY_BACKQUOTE => Key::Backquote,
        PONKO_KEY_COMMA => Key::Comma,
        PONKO_KEY_PERIOD => Key::Period,
        PONKO_KEY_SLASH => Key::Slash,
        PONKO_KEY_NUMPAD_0 => Key::Numpad0,
        PONKO_KEY_NUMPAD_1 => Key::Numpad1,
        PONKO_KEY_NUMPAD_2 => Key::Numpad2,
        PONKO_KEY_NUMPAD_3 => Key::Numpad3,
        PONKO_KEY_NUMPAD_4 => Key::Numpad4,
        PONKO_KEY_NUMPAD_5 => Key::Numpad5,
        PONKO_KEY_NUMPAD_6 => Key::Numpad6,
        PONKO_KEY_NUMPAD_7 => Key::Numpad7,
        PONKO_KEY_NUMPAD_8 => Key::Numpad8,
        PONKO_KEY_NUMPAD_9 => Key::Numpad9,
        PONKO_KEY_NUMPAD_ADD => Key::NumpadAdd,
        PONKO_KEY_NUMPAD_SUBTRACT => Key::NumpadSubtract,
        PONKO_KEY_NUMPAD_MULTIPLY => Key::NumpadMultiply,
        PONKO_KEY_NUMPAD_DIVIDE => Key::NumpadDivide,
        PONKO_KEY_NUMPAD_DECIMAL => Key::NumpadDecimal,
        PONKO_KEY_NUMPAD_ENTER => Key::NumpadEnter,
    }
}

/// A mouse button.
/// ```
/// assert_eq!(ponko_ffi::PonkoMouseButton::PONKO_MOUSE_LEFT as u32, 0);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoMouseButton {
    /// Left button.
    PONKO_MOUSE_LEFT,
    /// Right button.
    PONKO_MOUSE_RIGHT,
    /// Middle button (wheel click).
    PONKO_MOUSE_MIDDLE,
    /// Side "back" button.
    PONKO_MOUSE_BACK,
    /// Side "forward" button.
    PONKO_MOUSE_FORWARD,
}

pub(crate) fn button(b: PonkoMouseButton) -> MouseButton {
    use PonkoMouseButton::*;
    match b {
        PONKO_MOUSE_LEFT => MouseButton::Left,
        PONKO_MOUSE_RIGHT => MouseButton::Right,
        PONKO_MOUSE_MIDDLE => MouseButton::Middle,
        PONKO_MOUSE_BACK => MouseButton::Back,
        PONKO_MOUSE_FORWARD => MouseButton::Forward,
    }
}

const KEY_COUNT: u32 = PonkoKey::PONKO_KEY_NUMPAD_ENTER as u32 + 1;
const _: () = assert!(size_of::<PonkoKey>() == 4 && size_of::<PonkoMouseButton>() == 4);

/// C passes enums as plain ints: check the range before turning one into a Rust enum.
pub(crate) fn key_from_c(k: u32) -> Option<Key> {
    // SAFETY: PonkoKey is a 4-byte fieldless enum with discriminants 0..KEY_COUNT.
    (k < KEY_COUNT).then(|| key(unsafe { std::mem::transmute::<u32, PonkoKey>(k) }))
}
pub(crate) fn button_from_c(b: u32) -> Option<MouseButton> {
    // SAFETY: as above, with 5 buttons.
    (b <= PonkoMouseButton::PONKO_MOUSE_FORWARD as u32)
        .then(|| button(unsafe { std::mem::transmute::<u32, PonkoMouseButton>(b) }))
}

/// A gamepad button, by position (`PONKO_GAMEPAD_SOUTH` is A on Xbox,
/// Cross on PlayStation). See Rust's `GamepadButton`.
/// ```
/// assert_eq!(ponko_ffi::PonkoGamepadButton::PONKO_GAMEPAD_SOUTH as u32, 0);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoGamepadButton {
    /// Bottom face button (A, Cross).
    PONKO_GAMEPAD_SOUTH,
    /// Right face button (B, Circle).
    PONKO_GAMEPAD_EAST,
    /// Top face button (Y, Triangle).
    PONKO_GAMEPAD_NORTH,
    /// Left face button (X, Square).
    PONKO_GAMEPAD_WEST,
    /// Upper left shoulder (LB, L1).
    PONKO_GAMEPAD_LEFT_BUMPER,
    /// Upper right shoulder (RB, R1).
    PONKO_GAMEPAD_RIGHT_BUMPER,
    /// Lower left trigger pulled past halfway (LT, L2).
    PONKO_GAMEPAD_LEFT_TRIGGER,
    /// Lower right trigger pulled past halfway (RT, R2).
    PONKO_GAMEPAD_RIGHT_TRIGGER,
    /// Left center button (View, Share).
    PONKO_GAMEPAD_SELECT,
    /// Right center button (Menu, Options).
    PONKO_GAMEPAD_START,
    /// Left stick pressed in.
    PONKO_GAMEPAD_LEFT_STICK,
    /// Right stick pressed in.
    PONKO_GAMEPAD_RIGHT_STICK,
    /// D-pad up.
    PONKO_GAMEPAD_DPAD_UP,
    /// D-pad down.
    PONKO_GAMEPAD_DPAD_DOWN,
    /// D-pad left.
    PONKO_GAMEPAD_DPAD_LEFT,
    /// D-pad right.
    PONKO_GAMEPAD_DPAD_RIGHT,
    /// Clicking a gamepad touchpad.
    PONKO_GAMEPAD_TOUCHPAD,
}

/// A gamepad axis: sticks `-1..1` (y down is positive), triggers `0..1`.
/// ```
/// assert_eq!(ponko_ffi::PonkoGamepadAxis::PONKO_GAMEPAD_LEFT_STICK_X as u32, 0);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoGamepadAxis {
    /// Left stick, right is positive.
    PONKO_GAMEPAD_LEFT_STICK_X,
    /// Left stick, down is positive.
    PONKO_GAMEPAD_LEFT_STICK_Y,
    /// Right stick, right is positive.
    PONKO_GAMEPAD_RIGHT_STICK_X,
    /// Right stick, down is positive.
    PONKO_GAMEPAD_RIGHT_STICK_Y,
    /// Left trigger, `0` released to `1` fully pulled.
    PONKO_GAMEPAD_LEFT_TRIGGER_AXIS,
    /// Right trigger, `0` released to `1` fully pulled.
    PONKO_GAMEPAD_RIGHT_TRIGGER_AXIS,
}

/// How many gamepads the engine tracks: slots run `0..PONKO_MAX_GAMEPADS`.
/// ```
/// assert_eq!(ponko_ffi::PONKO_MAX_GAMEPADS as usize, ponko::input::MAX_GAMEPADS);
/// ```
pub const PONKO_MAX_GAMEPADS: u32 = 8;
const _: () = assert!(PONKO_MAX_GAMEPADS as usize == ponko::input::MAX_GAMEPADS);

/// The Rust buttons in `PonkoGamepadButton` order.
const GAMEPAD_BUTTONS: [GamepadButton; 17] = {
    use GamepadButton::*;
    [
        South,
        East,
        North,
        West,
        LeftBumper,
        RightBumper,
        LeftTrigger,
        RightTrigger,
        Select,
        Start,
        LeftStick,
        RightStick,
        DPadUp,
        DPadDown,
        DPadLeft,
        DPadRight,
        Touchpad,
    ]
};

/// The Rust axes in `PonkoGamepadAxis` order.
const GAMEPAD_AXES: [GamepadAxis; 6] = {
    use GamepadAxis::*;
    [
        LeftStickX,
        LeftStickY,
        RightStickX,
        RightStickY,
        LeftTrigger,
        RightTrigger,
    ]
};

pub(crate) fn gamepad_button_from_c(b: u32) -> Option<GamepadButton> {
    GAMEPAD_BUTTONS.get(b as usize).copied()
}
pub(crate) fn gamepad_axis_from_c(a: u32) -> Option<GamepadAxis> {
    GAMEPAD_AXES.get(a as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepad_enums_line_up_with_rust() {
        use PonkoGamepadButton::*;
        assert_eq!(
            gamepad_button_from_c(PONKO_GAMEPAD_SOUTH as u32),
            Some(GamepadButton::South)
        );
        assert_eq!(
            gamepad_button_from_c(PONKO_GAMEPAD_START as u32),
            Some(GamepadButton::Start)
        );
        assert_eq!(
            gamepad_button_from_c(PONKO_GAMEPAD_DPAD_RIGHT as u32),
            Some(GamepadButton::DPadRight)
        );
        assert_eq!(
            gamepad_button_from_c(PONKO_GAMEPAD_TOUCHPAD as u32),
            Some(GamepadButton::Touchpad)
        );
        assert_eq!(gamepad_button_from_c(PONKO_GAMEPAD_TOUCHPAD as u32 + 1), None);
        use PonkoGamepadAxis::*;
        assert_eq!(
            gamepad_axis_from_c(PONKO_GAMEPAD_RIGHT_STICK_Y as u32),
            Some(GamepadAxis::RightStickY)
        );
        assert_eq!(
            gamepad_axis_from_c(PONKO_GAMEPAD_RIGHT_TRIGGER_AXIS as u32),
            Some(GamepadAxis::RightTrigger)
        );
        assert_eq!(gamepad_axis_from_c(6), None);
    }

    #[test]
    fn every_c_key_maps_to_a_distinct_rust_key() {
        let keys: std::collections::HashSet<_> = (0..KEY_COUNT).map(|k| key_from_c(k).unwrap()).collect();
        assert_eq!(keys.len(), KEY_COUNT as usize);
        assert_eq!(key_from_c(KEY_COUNT), None);
        assert_eq!(button_from_c(5), None);
        assert_eq!(button_from_c(1), Some(MouseButton::Right));
    }
}
