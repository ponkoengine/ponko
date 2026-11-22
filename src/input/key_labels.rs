//! What each key is labelled on the player's keyboard: [`KeyLabels`].
//!
//! [`Key`] names a key by its place on a US keyboard, so controls stay put
//! on every layout. A prompt, though, should show what is printed on the
//! key: the key in `W`'s place reads `Z` on a French keyboard. The engine
//! keeps a table of the character keys' labels in [`Input`]: the system's
//! current layout read when the window opens and when it gets the focus
//! back (macOS and Windows: [`KeyLabels::system`]; the browser's Keyboard
//! Layout Map where it has one), the active xkb layout followed as it
//! changes (Linux and the BSDs, X11 and Wayland: `xkb.rs`, `watch.rs`),
//! corrected by every character key the player presses (all desktop
//! systems and the web).

use super::glyphs::{Glyph, Glyphs};
use super::{Input, Key};

/// The layout while a window is open, per system.
#[cfg(feature = "window")]
pub(crate) mod watch;
/// Labels from an xkb keymap (Linux and the BSDs); tested everywhere.
#[cfg(all(feature = "window", not(target_arch = "wasm32")))]
#[cfg_attr(
    not(any(
        test,
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )),
    allow(dead_code)
)]
pub(crate) mod xkb;

/// The keys whose label depends on the keyboard layout: the letters, the
/// digit row and the punctuation keys, with their macOS virtual key code
/// (`kVK_ANSI_*`) and their PC scan code (set 1, which Windows uses).
pub(crate) const LAYOUT_KEYS: [(Key, u16, u16); 47] = [
    (Key::A, 0x00, 0x1E),
    (Key::B, 0x0B, 0x30),
    (Key::C, 0x08, 0x2E),
    (Key::D, 0x02, 0x20),
    (Key::E, 0x0E, 0x12),
    (Key::F, 0x03, 0x21),
    (Key::G, 0x05, 0x22),
    (Key::H, 0x04, 0x23),
    (Key::I, 0x22, 0x17),
    (Key::J, 0x26, 0x24),
    (Key::K, 0x28, 0x25),
    (Key::L, 0x25, 0x26),
    (Key::M, 0x2E, 0x32),
    (Key::N, 0x2D, 0x31),
    (Key::O, 0x1F, 0x18),
    (Key::P, 0x23, 0x19),
    (Key::Q, 0x0C, 0x10),
    (Key::R, 0x0F, 0x13),
    (Key::S, 0x01, 0x1F),
    (Key::T, 0x11, 0x14),
    (Key::U, 0x20, 0x16),
    (Key::V, 0x09, 0x2F),
    (Key::W, 0x0D, 0x11),
    (Key::X, 0x07, 0x2D),
    (Key::Y, 0x10, 0x15),
    (Key::Z, 0x06, 0x2C),
    (Key::Num1, 0x12, 0x02),
    (Key::Num2, 0x13, 0x03),
    (Key::Num3, 0x14, 0x04),
    (Key::Num4, 0x15, 0x05),
    (Key::Num5, 0x17, 0x06),
    (Key::Num6, 0x16, 0x07),
    (Key::Num7, 0x1A, 0x08),
    (Key::Num8, 0x1C, 0x09),
    (Key::Num9, 0x19, 0x0A),
    (Key::Num0, 0x1D, 0x0B),
    (Key::Minus, 0x1B, 0x0C),
    (Key::Equal, 0x18, 0x0D),
    (Key::LeftBracket, 0x21, 0x1A),
    (Key::RightBracket, 0x1E, 0x1B),
    (Key::Backslash, 0x2A, 0x2B),
    (Key::Semicolon, 0x29, 0x27),
    (Key::Apostrophe, 0x27, 0x28),
    (Key::Backquote, 0x32, 0x29),
    (Key::Comma, 0x2B, 0x33),
    (Key::Period, 0x2F, 0x34),
    (Key::Slash, 0x2C, 0x35),
];

/// Whether `key`'s label depends on the layout.
pub(crate) fn is_layout_key(key: Key) -> bool {
    LAYOUT_KEYS.iter().any(|&(k, ..)| k == key)
}

/// One key's label from the layout: a character or two, kept inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Label {
    bytes: [u8; 8],
    len: u8,
}

impl Label {
    /// The label a layout character gives a key: upper case where that is
    /// one character (`z` reads `Z`, `ß` stays `ß`), nothing for a control
    /// or blank character.
    fn of(c: char) -> Option<Label> {
        if c.is_control() || c.is_whitespace() {
            return None;
        }
        let mut upper = c.to_uppercase();
        let c = match (upper.next(), upper.next()) {
            (Some(u), None) => u,
            _ => c,
        };
        let mut bytes = [0; 8];
        let len = c.encode_utf8(&mut bytes).len() as u8;
        Some(Label { bytes, len })
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// What each key is labelled on a keyboard layout, for prompts:
/// [`Input::key_labels`] is the player's. Keys outside the layout's
/// character keys (Space, Shift, the arrows, F1, the numpad) keep their
/// usual names.
///
/// ```
/// use ponko::input::KeyLabels;
/// use ponko::prelude::*;
/// // A French (AZERTY) keyboard, where the letters differ from the US ones.
/// let azerty = KeyLabels::from_layout(|key| match key {
///     Key::Q => Some('a'),
///     Key::W => Some('z'),
///     Key::A => Some('q'),
///     Key::Semicolon => Some('m'),
///     _ => None, // as on a US keyboard
/// });
/// assert_eq!(azerty.label(Key::W), "Z");
/// assert_eq!(azerty.label(Key::S), "S");
/// assert_eq!(azerty.label(Key::Space), "Space");
/// assert_eq!(KeyLabels::us().label(Key::W), "W");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyLabels {
    labels: [Option<Label>; Key::COUNT],
}

impl Default for KeyLabels {
    fn default() -> Self {
        Self::us()
    }
}

impl KeyLabels {
    /// A US keyboard: every key named as [`Key`] names it.
    ///
    /// ```
    /// assert_eq!(ponko::input::KeyLabels::us().label(ponko::prelude::Key::Slash), "/");
    /// ```
    #[must_use]
    pub fn us() -> Self {
        Self {
            labels: [None; Key::COUNT],
        }
    }

    /// A layout given as the character each key types without a modifier
    /// (`None`: as on a US keyboard). Letters are labelled in upper case;
    /// only the character keys (letters, digits, punctuation) take a label
    /// from it.
    ///
    /// ```
    /// use ponko::input::KeyLabels;
    /// use ponko::prelude::*;
    /// let qwertz = KeyLabels::from_layout(|k| match k {
    ///     Key::Y => Some('z'),
    ///     Key::Z => Some('y'),
    ///     Key::Minus => Some('ß'),
    ///     Key::Space => Some('x'), // not a character key: ignored
    ///     _ => None,
    /// });
    /// assert_eq!((qwertz.label(Key::Y), qwertz.label(Key::Minus)), ("Z", "ß"));
    /// assert_eq!(qwertz.label(Key::Space), "Space");
    /// ```
    #[must_use]
    pub fn from_layout(mut layout: impl FnMut(Key) -> Option<char>) -> Self {
        let mut labels = Self::us();
        for (key, ..) in LAYOUT_KEYS {
            if let Some(c) = layout(key) {
                labels.set(key, c);
            }
        }
        labels
    }

    /// Labels `key` with what typing `c` on it gives (upper case for a
    /// letter). Ignored for a key that isn't a character key and for a
    /// control or blank character. Returns whether the label changed.
    ///
    /// ```
    /// use ponko::input::KeyLabels;
    /// use ponko::prelude::*;
    /// let mut labels = KeyLabels::us();
    /// assert!(labels.set(Key::Q, 'a'));
    /// assert!(!labels.set(Key::Q, 'a'), "already so");
    /// assert!(!labels.set(Key::Enter, 'x'), "not a character key");
    /// assert_eq!(labels.label(Key::Q), "A");
    /// ```
    pub fn set(&mut self, key: Key, c: char) -> bool {
        if !is_layout_key(key) {
            return false;
        }
        let Some(mut label) = Label::of(c) else { return false };
        // The US label itself needs no entry.
        if label.as_str() == super::glyphs::key_label(key).0 {
            label.len = 0;
        }
        let label = (label.len > 0).then_some(label);
        let slot = &mut self.labels[key as usize];
        if *slot == label {
            return false;
        }
        *slot = label;
        true
    }

    /// What `key` is labelled: the layout's character for a character key,
    /// else its usual name ("Space", "Left Shift", "Numpad 1").
    ///
    /// ```
    /// assert_eq!(ponko::input::KeyLabels::us().label(ponko::prelude::Key::LeftShift), "Left Shift");
    /// ```
    #[must_use]
    pub fn label(&self, key: Key) -> &str {
        match &self.labels[key as usize] {
            Some(label) => label.as_str(),
            None => super::glyphs::key_label(key).0,
        }
    }

    /// The short label drawn on `key`'s cap in a [`GlyphAtlas`](super::GlyphAtlas).
    pub(crate) fn cap(&self, key: Key) -> &str {
        match &self.labels[key as usize] {
            Some(label) => label.as_str(),
            None => super::glyphs::key_label(key).1,
        }
    }

    /// [`Glyph::label`] with keys labelled for this layout.
    ///
    /// ```
    /// use ponko::input::{Glyph, KeyLabels};
    /// use ponko::prelude::*;
    /// let azerty = KeyLabels::from_layout(|k| (k == Key::W).then_some('z'));
    /// assert_eq!(azerty.glyph(Glyph::Key(Key::W)), "Z");
    /// assert_eq!(azerty.glyph(Glyph::Mouse(MouseButton::Left)), "Left Mouse");
    /// ```
    #[must_use]
    pub fn glyph(&self, glyph: Glyph) -> &str {
        match glyph {
            Glyph::Key(key) => self.label(key),
            other => other.label(),
        }
    }

    /// The glyphs' labels for this layout, joined with `/` (`"Z/Q/S/D"` for
    /// WASD on a French keyboard): a text-only prompt. `glyphs.to_string()`
    /// is the same on a US keyboard.
    ///
    /// ```
    /// use ponko::input::{Glyph, Glyphs, KeyLabels};
    /// use ponko::prelude::*;
    /// let wasd = Glyphs::from_slice(&[Key::W, Key::A, Key::S, Key::D].map(Glyph::Key));
    /// let azerty = KeyLabels::from_layout(|k| match k {
    ///     Key::W => Some('z'),
    ///     Key::A => Some('q'),
    ///     _ => None,
    /// });
    /// assert_eq!(azerty.text(&wasd), "Z/Q/S/D");
    /// assert_eq!(KeyLabels::us().text(&wasd), wasd.to_string());
    /// ```
    #[must_use]
    pub fn text(&self, glyphs: &Glyphs) -> String {
        let mut out = String::new();
        for (i, glyph) in glyphs.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            out.push_str(self.glyph(glyph));
        }
        out
    }

    /// The system's current keyboard layout, read without a window: on
    /// macOS (the current input source, through `UCKeyTranslate`) and
    /// Windows (the thread's keyboard layout, through `MapVirtualKeyExW`).
    /// `None` elsewhere and when the system has no layout to give. The
    /// window reads it into [`Input::key_labels`] when it opens and when it
    /// gets the focus back. (On Linux and the BSDs the window follows the
    /// display server's xkb layout instead, and in a browser it asks the
    /// Keyboard Layout Map; neither can be read without a window.)
    ///
    /// ```
    /// if let Some(labels) = ponko::input::KeyLabels::system() {
    ///     let _ = labels.label(ponko::prelude::Key::W);
    /// }
    /// ```
    #[must_use]
    pub fn system() -> Option<Self> {
        system::layout().map(|chars| {
            let mut labels = Self::us();
            for ((key, ..), c) in LAYOUT_KEYS.iter().zip(chars) {
                if let Some(c) = c {
                    labels.set(*key, c);
                }
            }
            labels
        })
    }
}

/// The system's layout: the character each of [`LAYOUT_KEYS`] types.
#[cfg(target_os = "macos")]
mod system {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> CFTypeRef;
        fn TISGetInputSourceProperty(source: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        static kTISPropertyUnicodeKeyLayoutData: CFTypeRef;
        fn LMGetKbdType() -> u8;
        #[allow(clippy::too_many_arguments)]
        fn UCKeyTranslate(
            layout: *const c_void,
            code: u16,
            action: u16,
            modifiers: u32,
            keyboard: u32,
            options: u32,
            dead_key_state: *mut u32,
            max: usize,
            len: *mut usize,
            chars: *mut u16,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFRelease(object: CFTypeRef);
    }

    /// `kUCKeyActionDisplay`: what the key shows, not a key press.
    const ACTION_DISPLAY: u16 = 3;
    /// `kUCKeyTranslateNoDeadKeysMask`: a dead key gives its own character.
    const NO_DEAD_KEYS: u32 = 1;

    pub(super) fn layout() -> Option<[Option<char>; 47]> {
        // SAFETY: the input source is a +1 reference released below; its
        // layout data (owned by the source, not retained) is read while the
        // source is alive; UCKeyTranslate writes at most `max` UTF-16 units.
        unsafe {
            let source = TISCopyCurrentKeyboardLayoutInputSource();
            if source.is_null() {
                return None;
            }
            let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
            if data.is_null() {
                CFRelease(source);
                return None;
            }
            let layout = CFDataGetBytePtr(data).cast::<c_void>();
            let keyboard = u32::from(LMGetKbdType());
            let mut chars = [None; 47];
            for (out, &(_, code, _)) in chars.iter_mut().zip(&super::LAYOUT_KEYS) {
                let (mut dead, mut len, mut buf) = (0u32, 0usize, [0u16; 4]);
                let status = UCKeyTranslate(
                    layout,
                    code,
                    ACTION_DISPLAY,
                    0,
                    keyboard,
                    NO_DEAD_KEYS,
                    &mut dead,
                    buf.len(),
                    &mut len,
                    buf.as_mut_ptr(),
                );
                if status == 0 {
                    *out = char::decode_utf16(buf[..len.min(buf.len())].iter().copied())
                        .next()
                        .and_then(Result::ok);
                }
            }
            CFRelease(source);
            Some(chars)
        }
    }
}

/// The system's layout: the character each of [`LAYOUT_KEYS`] types.
#[cfg(target_os = "windows")]
mod system {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetKeyboardLayout(thread: u32) -> *mut c_void;
        fn MapVirtualKeyExW(code: u32, map_type: u32, layout: *mut c_void) -> u32;
    }

    /// `MAPVK_VSC_TO_VK_EX` and `MAPVK_VK_TO_CHAR`.
    const SCAN_TO_VK: u32 = 3;
    const VK_TO_CHAR: u32 = 2;

    pub(super) fn layout() -> Option<[Option<char>; 47]> {
        // SAFETY: plain value calls; a null layout (none) is handled.
        unsafe {
            let layout = GetKeyboardLayout(0);
            if layout.is_null() {
                return None;
            }
            let mut chars = [None; 47];
            for (out, &(_, _, scan)) in chars.iter_mut().zip(&super::LAYOUT_KEYS) {
                let vk = MapVirtualKeyExW(u32::from(scan), SCAN_TO_VK, layout);
                // The low word is the unshifted character (the top bit marks
                // a dead key, which still gives its character).
                let c = MapVirtualKeyExW(vk, VK_TO_CHAR, layout) & 0xFFFF;
                *out = char::from_u32(c).filter(|&c| c != '\0');
            }
            Some(chars)
        }
    }
}

/// No system query here: labels come from the keys pressed.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod system {
    pub(super) fn layout() -> Option<[Option<char>; 47]> {
        None
    }
}

impl Input {
    /// What each key is labelled on the player's keyboard, for prompts: a
    /// French player's `Key::W` reads `Z`. In a window it follows the
    /// system's layout (read when the window opens and when it gets the
    /// focus back, on macOS and Windows; the browser's Keyboard Layout Map,
    /// where it has one, likewise; the active xkb layout as it changes, on
    /// Linux and the BSDs under X11 and Wayland) and every character key
    /// pressed (everywhere); headless it is a US
    /// keyboard's until [`set_key_labels`](Self::set_key_labels). Draw
    /// prompts with it: [`GlyphAtlas::set_key_labels`](super::GlyphAtlas::set_key_labels),
    /// [`KeyLabels::text`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.input.key_labels().label(Key::W), "W");
    /// });
    /// ```
    #[must_use]
    pub fn key_labels(&self) -> &KeyLabels {
        &self.key_labels.labels
    }

    /// Replaces the key labels: a "keyboard layout" option, or a test.
    /// Keys pressed later still correct them.
    ///
    /// ```
    /// use ponko::input::KeyLabels;
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     ctx.input.set_key_labels(KeyLabels::from_layout(|k| (k == Key::W).then_some('z')));
    ///     assert!(ctx.input.key_labels_changed());
    ///     assert_eq!(ctx.input.key_labels().label(Key::W), "Z");
    /// });
    /// ```
    pub fn set_key_labels(&mut self, labels: KeyLabels) {
        if labels != self.key_labels.labels {
            self.key_labels.labels = labels;
            self.key_labels.changed = true;
        }
    }

    /// Whether the key labels changed this frame: redraw text prompts.
    #[must_use]
    pub fn key_labels_changed(&self) -> bool {
        self.key_labels.changed
    }

    /// A character key was pressed and typed `c` without modifiers: its
    /// label on this layout.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn learn_key_label(&mut self, key: Key, c: char) {
        if self.key_labels.labels.set(key, c) {
            self.key_labels.changed = true;
        }
    }
}

/// The character a key event says its key types, to label it with: the
/// character the key types without modifiers (`unmodified`, from winit's
/// `key_without_modifiers` on the desktop), a dead key's own character
/// included. With only the logical key, which Shift changes (the web),
/// `letters_only` keeps just letters, whose case Shift alone changes.
#[cfg(feature = "window")]
pub(crate) fn typed_char(unmodified: &winit::keyboard::Key, letters_only: bool) -> Option<char> {
    use winit::keyboard::Key as W;
    let c = match unmodified {
        W::Character(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => c,
                _ => return None,
            }
        }
        W::Dead(Some(c)) if !letters_only => *c,
        _ => return None,
    };
    (!letters_only || c.is_alphabetic()).then_some(c)
}

/// The labels and their change edge: a field of [`Input`].
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyLabelState {
    labels: KeyLabels,
    changed: bool,
}

impl KeyLabelState {
    pub(crate) fn end_frame(&mut self) {
        self.changed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A French AZERTY keyboard's unshifted characters, by US position.
    fn azerty(key: Key) -> Option<char> {
        Some(match key {
            Key::Q => 'a',
            Key::W => 'z',
            Key::A => 'q',
            Key::Z => 'w',
            Key::Semicolon => 'm',
            Key::M => ',',
            Key::Comma => ';',
            Key::Period => ':',
            Key::Slash => '!',
            Key::Num1 => '&',
            Key::Num2 => 'é',
            Key::LeftBracket => '^', // a dead key: its own character
            Key::Apostrophe => 'ù',
            _ => return None,
        })
    }

    #[test]
    fn every_layout_key_has_distinct_codes_on_both_systems() {
        let mut keys: Vec<_> = LAYOUT_KEYS.iter().map(|e| e.0 as usize).collect();
        let mut mac: Vec<_> = LAYOUT_KEYS.iter().map(|e| e.1).collect();
        let mut pc: Vec<_> = LAYOUT_KEYS.iter().map(|e| e.2).collect();
        for list in [&mut mac, &mut pc] {
            list.sort_unstable();
            list.dedup();
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!((keys.len(), mac.len(), pc.len()), (47, 47, 47));
    }

    #[test]
    fn an_azerty_layout_relabels_letters_digits_and_punctuation_and_nothing_else() {
        let labels = KeyLabels::from_layout(azerty);
        let got: Vec<_> = [Key::W, Key::A, Key::M, Key::Num2, Key::LeftBracket, Key::Apostrophe]
            .map(|k| labels.label(k))
            .to_vec();
        assert_eq!(got, ["Z", "Q", ",", "É", "^", "Ù"]);
        assert_eq!(labels.label(Key::S), "S", "the same on both layouts");
        for key in [Key::Space, Key::LeftShift, Key::Numpad1, Key::F1, Key::Up] {
            assert_eq!(labels.label(key), KeyLabels::us().label(key));
        }
        assert_eq!(labels.cap(Key::Escape), "Esc", "short names stay");
    }

    #[test]
    fn letters_become_upper_case_but_a_letter_without_a_single_upper_case_stays() {
        let mut labels = KeyLabels::us();
        labels.set(Key::Minus, 'ß');
        labels.set(Key::Semicolon, 'ö');
        assert_eq!((labels.label(Key::Minus), labels.label(Key::Semicolon)), ("ß", "Ö"));
        assert!(!labels.set(Key::Q, '\u{7}'), "a control character");
        assert!(!labels.set(Key::Q, ' '), "a blank");
        assert_eq!(labels.label(Key::Q), "Q");
    }

    #[test]
    fn the_us_labels_need_no_entries_so_a_us_layout_equals_the_default() {
        let us = KeyLabels::from_layout(|k| {
            Some(match k {
                Key::W => 'w',
                Key::Num1 => '1',
                Key::Slash => '/',
                _ => return None,
            })
        });
        assert_eq!(us, KeyLabels::us());
        let mut back = KeyLabels::from_layout(azerty);
        back.set(Key::W, 'w');
        assert_eq!(back.label(Key::W), "W");
    }

    #[test]
    fn pressing_a_key_teaches_its_label_and_raises_the_change_edge_once() {
        let mut input = Input::new();
        input.learn_key_label(Key::Y, 'z');
        assert!(input.key_labels_changed());
        assert_eq!(input.key_labels().label(Key::Y), "Z");
        input.end_frame();
        assert!(!input.key_labels_changed());
        input.learn_key_label(Key::Y, 'z');
        assert!(!input.key_labels_changed(), "nothing new");
        input.set_key_labels(KeyLabels::us());
        assert!(input.key_labels_changed());
    }

    #[cfg(feature = "window")]
    #[test]
    fn key_events_label_with_the_unmodified_character_and_the_web_only_with_letters() {
        use winit::keyboard::{Key as W, NamedKey};
        let z = W::Character("z".into());
        assert_eq!(typed_char(&z, false), Some('z'));
        assert_eq!(typed_char(&W::Dead(Some('^')), false), Some('^'), "a dead key");
        assert_eq!(typed_char(&W::Dead(Some('^')), true), None);
        assert_eq!(
            typed_char(&W::Character("&".into()), true),
            None,
            "Shift may have made it"
        );
        assert_eq!(typed_char(&W::Character("Z".into()), true), Some('Z'));
        assert_eq!(typed_char(&W::Character("ab".into()), false), None, "not one character");
        assert_eq!(typed_char(&W::Named(NamedKey::Enter), false), None);
        // An AZERTY player's presses, as the window sees them.
        let mut input = Input::new();
        for (key, typed) in [(Key::W, "z"), (Key::Q, "a"), (Key::Num2, "é"), (Key::Space, " ")] {
            if let Some(c) = typed_char(&W::Character(typed.into()), false) {
                input.learn_key_label(key, c);
            }
        }
        let labels = input.key_labels();
        assert_eq!(
            [Key::W, Key::Q, Key::Num2, Key::Space].map(|k| labels.label(k)),
            ["Z", "A", "É", "Space"]
        );
    }

    /// On macOS and Windows the system's layout is read without a window;
    /// whatever layout this machine has, every letter key gets one letter.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn the_systems_layout_gives_every_letter_key_a_label() {
        let labels = KeyLabels::system().expect("the system has a keyboard layout");
        for (key, ..) in &LAYOUT_KEYS[..26] {
            let label = labels.label(*key);
            assert_eq!(label.chars().count(), 1, "{key:?} reads {label:?}");
        }
    }
}
