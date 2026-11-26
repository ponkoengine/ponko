//! Button prompts: which glyph shows an action's binding on the device the
//! player uses ([`Input::action_glyphs`]), and a [`GlyphAtlas`] that draws
//! every glyph into one texture.
//!
//! All the art is drawn here, from circles, rounded boxes, strokes and the
//! built-in monospace font: no platform artwork or logos. A game with its
//! own art keeps the [`Glyph`] lookup and draws its own images for them.

use glam::Vec2;

use super::controller::{ControllerFamily, InputDevice};
use super::key_labels::KeyLabels;
use super::{Gamepad, GamepadAxis, GamepadButton, GamepadStick, Input, Key, MouseButton};
use crate::actions::{Action, ActionRef, Axis2Binding, AxisBinding, ButtonBinding};
use crate::camera::Rect;
use crate::color::Color;
use crate::gfx::Graphics as Gfx;
use crate::texture::{DrawParams, Texture};

/// One button-prompt image: a key, a mouse button, or a gamepad button,
/// stick or axis as a controller family labels it.
///
/// More kinds may be added, so a `match` needs a `_` arm.
///
/// ```
/// use ponko::input::{ControllerFamily, GamepadButton, Glyph};
/// use ponko::prelude::*;
/// assert_eq!(Glyph::Button(ControllerFamily::Xbox, GamepadButton::South).label(), "A");
/// assert_eq!(Glyph::Button(ControllerFamily::PlayStation, GamepadButton::South).label(), "Cross");
/// assert_eq!(Glyph::Key(Key::Space).label(), "Space");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Glyph {
    /// A keyboard key.
    Key(Key),
    /// A mouse button.
    Mouse(MouseButton),
    /// A gamepad button, labelled for a family.
    Button(ControllerFamily, GamepadButton),
    /// A whole stick (a movement or aim action).
    Stick(ControllerFamily, GamepadStick),
    /// One stick direction or a trigger's travel (an axis action).
    Axis(ControllerFamily, GamepadAxis),
}

/// Cells per family in the atlas: buttons, then sticks, then axes.
const PER_FAMILY: usize = GamepadButton::COUNT + 2 + GamepadAxis::COUNT;
/// Where the gamepad cells start.
const PAD_START: usize = Key::COUNT + MouseButton::COUNT;
/// Every glyph's cell count.
const GLYPH_COUNT: usize = PAD_START + ControllerFamily::ALL.len() * PER_FAMILY;

fn family_index(family: ControllerFamily) -> usize {
    ControllerFamily::ALL.iter().position(|&f| f == family).unwrap_or(0)
}

impl Glyph {
    /// Every glyph, in atlas order: keys, mouse buttons, then each family's
    /// buttons, sticks and axes.
    ///
    /// ```
    /// let n = ponko::input::Glyph::all().count();
    /// assert!(n > 150);
    /// ```
    pub fn all() -> impl Iterator<Item = Glyph> {
        let keys = Key::ALL.into_iter().map(Glyph::Key);
        let mouse = MouseButton::ALL.into_iter().map(Glyph::Mouse);
        let pads = ControllerFamily::ALL.into_iter().flat_map(|f| {
            GamepadButton::ALL
                .into_iter()
                .map(move |b| Glyph::Button(f, b))
                .chain([GamepadStick::Left, GamepadStick::Right].map(|s| Glyph::Stick(f, s)))
                .chain(GamepadAxis::ALL.into_iter().map(move |a| Glyph::Axis(f, a)))
        });
        keys.chain(mouse).chain(pads)
    }

    /// The glyph's cell number in a [`GlyphAtlas`] (its place in [`all`](Self::all)).
    fn index(self) -> usize {
        match self {
            Glyph::Key(k) => k as usize,
            Glyph::Mouse(b) => Key::COUNT + b as usize,
            Glyph::Button(f, b) => PAD_START + family_index(f) * PER_FAMILY + b as usize,
            Glyph::Stick(f, s) => {
                PAD_START + family_index(f) * PER_FAMILY + GamepadButton::COUNT + usize::from(s == GamepadStick::Right)
            }
            Glyph::Axis(f, a) => PAD_START + family_index(f) * PER_FAMILY + GamepadButton::COUNT + 2 + a as usize,
        }
    }

    /// The name a player reads: "A", "Cross", "Space", "Left Mouse", "LB",
    /// "Left Stick". Text-only prompts and screen readers use it.
    ///
    /// ```
    /// use ponko::input::{ControllerFamily, GamepadButton, Glyph};
    /// assert_eq!(Glyph::Button(ControllerFamily::Nintendo, GamepadButton::South).label(), "B");
    /// assert_eq!(Glyph::Button(ControllerFamily::Generic, GamepadButton::South).label(), "South");
    /// ```
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Glyph::Key(k) => key_label(k).0,
            Glyph::Mouse(b) => match b {
                MouseButton::Left => "Left Mouse",
                MouseButton::Right => "Right Mouse",
                MouseButton::Middle => "Middle Mouse",
                MouseButton::Back => "Mouse Back",
                MouseButton::Forward => "Mouse Forward",
            },
            Glyph::Button(f, b) => button_label(f, b).0,
            Glyph::Stick(_, GamepadStick::Left) => "Left Stick",
            Glyph::Stick(_, GamepadStick::Right) => "Right Stick",
            Glyph::Axis(f, a) => match a {
                GamepadAxis::LeftStickX => "Left Stick X",
                GamepadAxis::LeftStickY => "Left Stick Y",
                GamepadAxis::RightStickX => "Right Stick X",
                GamepadAxis::RightStickY => "Right Stick Y",
                GamepadAxis::LeftTrigger => button_label(f, GamepadButton::LeftTrigger).0,
                GamepadAxis::RightTrigger => button_label(f, GamepadButton::RightTrigger).0,
            },
        }
    }

    /// A stable name for the glyph, usable as a rich-text image name:
    /// `key.space`, `mouse.left`, `pad.xbox.south`, `stick.generic.left`,
    /// `axis.playstation.right_trigger`.
    ///
    /// ```
    /// use ponko::input::{ControllerFamily, GamepadButton, Glyph};
    /// assert_eq!(Glyph::Button(ControllerFamily::Xbox, GamepadButton::South).name(), "pad.xbox.south");
    /// ```
    #[must_use]
    pub fn name(self) -> String {
        let snake = |debug: String| {
            let mut out = String::new();
            for (i, ch) in debug.chars().enumerate() {
                if ch.is_ascii_uppercase() && i > 0 {
                    out.push('_');
                }
                out.push(ch.to_ascii_lowercase());
            }
            out
        };
        match self {
            Glyph::Key(k) => format!("key.{}", snake(format!("{k:?}"))),
            Glyph::Mouse(b) => format!("mouse.{}", snake(format!("{b:?}"))),
            Glyph::Button(f, b) => format!("pad.{}.{}", f.name(), snake(format!("{b:?}"))),
            Glyph::Stick(f, s) => format!("stick.{}.{}", f.name(), snake(format!("{s:?}"))),
            Glyph::Axis(f, a) => format!("axis.{}.{}", f.name(), snake(format!("{a:?}"))),
        }
    }
}

/// A key's long label and the short one drawn on its cap.
pub(crate) fn key_label(k: Key) -> (&'static str, &'static str) {
    use Key as K;
    macro_rules! same {
        ($s:literal) => {
            ($s, $s)
        };
    }
    match k {
        K::A => same!("A"),
        K::B => same!("B"),
        K::C => same!("C"),
        K::D => same!("D"),
        K::E => same!("E"),
        K::F => same!("F"),
        K::G => same!("G"),
        K::H => same!("H"),
        K::I => same!("I"),
        K::J => same!("J"),
        K::K => same!("K"),
        K::L => same!("L"),
        K::M => same!("M"),
        K::N => same!("N"),
        K::O => same!("O"),
        K::P => same!("P"),
        K::Q => same!("Q"),
        K::R => same!("R"),
        K::S => same!("S"),
        K::T => same!("T"),
        K::U => same!("U"),
        K::V => same!("V"),
        K::W => same!("W"),
        K::X => same!("X"),
        K::Y => same!("Y"),
        K::Z => same!("Z"),
        K::Num0 => same!("0"),
        K::Num1 => same!("1"),
        K::Num2 => same!("2"),
        K::Num3 => same!("3"),
        K::Num4 => same!("4"),
        K::Num5 => same!("5"),
        K::Num6 => same!("6"),
        K::Num7 => same!("7"),
        K::Num8 => same!("8"),
        K::Num9 => same!("9"),
        K::F1 => same!("F1"),
        K::F2 => same!("F2"),
        K::F3 => same!("F3"),
        K::F4 => same!("F4"),
        K::F5 => same!("F5"),
        K::F6 => same!("F6"),
        K::F7 => same!("F7"),
        K::F8 => same!("F8"),
        K::F9 => same!("F9"),
        K::F10 => same!("F10"),
        K::F11 => same!("F11"),
        K::F12 => same!("F12"),
        K::Up => ("Up", ""),
        K::Down => ("Down", ""),
        K::Left => ("Left", ""),
        K::Right => ("Right", ""),
        K::Space => same!("Space"),
        K::Enter => same!("Enter"),
        K::Escape => ("Escape", "Esc"),
        K::Tab => same!("Tab"),
        K::Backspace => ("Backspace", "Bksp"),
        K::CapsLock => ("Caps Lock", "Caps"),
        K::Insert => ("Insert", "Ins"),
        K::Delete => ("Delete", "Del"),
        K::Home => same!("Home"),
        K::End => same!("End"),
        K::PageUp => ("Page Up", "PgUp"),
        K::PageDown => ("Page Down", "PgDn"),
        K::LeftShift => ("Left Shift", "Shift"),
        K::RightShift => ("Right Shift", "Shift"),
        K::LeftCtrl => ("Left Ctrl", "Ctrl"),
        K::RightCtrl => ("Right Ctrl", "Ctrl"),
        K::LeftAlt => ("Left Alt", "Alt"),
        K::RightAlt => ("Right Alt", "Alt"),
        K::LeftSuper => ("Left Super", "Super"),
        K::RightSuper => ("Right Super", "Super"),
        K::Minus => same!("-"),
        K::Equal => same!("="),
        K::LeftBracket => same!("["),
        K::RightBracket => same!("]"),
        K::Backslash => same!("\\"),
        K::Semicolon => same!(";"),
        K::Apostrophe => same!("'"),
        K::Backquote => same!("`"),
        K::Comma => same!(","),
        K::Period => same!("."),
        K::Slash => same!("/"),
        K::Numpad0 => ("Numpad 0", "N0"),
        K::Numpad1 => ("Numpad 1", "N1"),
        K::Numpad2 => ("Numpad 2", "N2"),
        K::Numpad3 => ("Numpad 3", "N3"),
        K::Numpad4 => ("Numpad 4", "N4"),
        K::Numpad5 => ("Numpad 5", "N5"),
        K::Numpad6 => ("Numpad 6", "N6"),
        K::Numpad7 => ("Numpad 7", "N7"),
        K::Numpad8 => ("Numpad 8", "N8"),
        K::Numpad9 => ("Numpad 9", "N9"),
        K::NumpadAdd => ("Numpad +", "N+"),
        K::NumpadSubtract => ("Numpad -", "N-"),
        K::NumpadMultiply => ("Numpad *", "N*"),
        K::NumpadDivide => ("Numpad /", "N/"),
        K::NumpadDecimal => ("Numpad .", "N."),
        K::NumpadEnter => ("Numpad Enter", "NEnt"),
    }
}

/// A gamepad button's label for a family, and the text drawn on it ("" for
/// buttons drawn as shapes).
fn button_label(f: ControllerFamily, b: GamepadButton) -> (&'static str, &'static str) {
    use ControllerFamily as F;
    use GamepadButton as B;
    match (f, b) {
        (F::Xbox, B::South) => ("A", "A"),
        (F::Xbox, B::East) => ("B", "B"),
        (F::Xbox, B::North) => ("Y", "Y"),
        (F::Xbox, B::West) => ("X", "X"),
        (F::PlayStation, B::South) => ("Cross", ""),
        (F::PlayStation, B::East) => ("Circle", ""),
        (F::PlayStation, B::North) => ("Triangle", ""),
        (F::PlayStation, B::West) => ("Square", ""),
        (F::Nintendo, B::South) => ("B", "B"),
        (F::Nintendo, B::East) => ("A", "A"),
        (F::Nintendo, B::North) => ("X", "X"),
        (F::Nintendo, B::West) => ("Y", "Y"),
        (_, B::South) => ("South", ""),
        (_, B::East) => ("East", ""),
        (_, B::North) => ("North", ""),
        (_, B::West) => ("West", ""),
        (F::PlayStation, B::LeftBumper) => ("L1", "L1"),
        (F::PlayStation, B::RightBumper) => ("R1", "R1"),
        (F::PlayStation, B::LeftTrigger) => ("L2", "L2"),
        (F::PlayStation, B::RightTrigger) => ("R2", "R2"),
        (F::Nintendo, B::LeftBumper) => ("L", "L"),
        (F::Nintendo, B::RightBumper) => ("R", "R"),
        (F::Nintendo, B::LeftTrigger) => ("ZL", "ZL"),
        (F::Nintendo, B::RightTrigger) => ("ZR", "ZR"),
        (_, B::LeftBumper) => ("LB", "LB"),
        (_, B::RightBumper) => ("RB", "RB"),
        (_, B::LeftTrigger) => ("LT", "LT"),
        (_, B::RightTrigger) => ("RT", "RT"),
        (F::Xbox, B::Select) => ("View", "View"),
        (F::Xbox, B::Start) => ("Menu", "Menu"),
        (F::PlayStation, B::Select) => ("Share", "Share"),
        (F::PlayStation, B::Start) => ("Options", "Opts"),
        (F::Nintendo, B::Select) => ("Minus", "-"),
        (F::Nintendo, B::Start) => ("Plus", "+"),
        (_, B::Select) => ("Select", "Sel"),
        (_, B::Start) => ("Start", "Start"),
        (F::PlayStation, B::LeftStick) => ("L3", "L3"),
        (F::PlayStation, B::RightStick) => ("R3", "R3"),
        (_, B::LeftStick) => ("Left Stick Press", "LS"),
        (_, B::RightStick) => ("Right Stick Press", "RS"),
        (_, B::DPadUp) => ("D-Pad Up", ""),
        (_, B::DPadDown) => ("D-Pad Down", ""),
        (_, B::DPadLeft) => ("D-Pad Left", ""),
        (_, B::DPadRight) => ("D-Pad Right", ""),
        (_, B::Touchpad) => ("Touchpad", ""),
    }
}

/// The glyphs of one action's binding: at most four (a WASD movement
/// action is four keys; most actions are one glyph). A fixed-size value,
/// so looking prompts up every frame allocates nothing.
///
/// ```
/// use ponko::input::{Glyph, Glyphs};
/// use ponko::prelude::*;
/// let none = Glyphs::default();
/// assert!(none.is_empty());
/// assert_eq!(none.to_string(), "");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Glyphs {
    items: [Option<Glyph>; 4],
}

impl Glyphs {
    fn push(&mut self, glyph: Glyph) {
        if let Some(slot) = self.items.iter_mut().find(|s| s.is_none()) {
            *slot = Some(glyph);
        }
    }

    /// Up to four glyphs from a list (the rest are dropped): a prompt
    /// for something that isn't an action.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{Glyph, Glyphs};
    /// let g = Glyphs::from_slice(&[Glyph::Key(Key::LeftShift), Glyph::Key(Key::W)]);
    /// assert_eq!(g.to_string(), "Left Shift/W");
    /// ```
    #[must_use]
    pub fn from_slice(glyphs: &[Glyph]) -> Self {
        let mut out = Self::default();
        for &g in glyphs {
            out.push(g);
        }
        out
    }

    /// How many glyphs.
    ///
    /// ```
    /// assert_eq!(ponko::input::Glyphs::default().len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.iter().flatten().count()
    }

    /// Whether there are none: the action has no binding on that device.
    ///
    /// ```
    /// assert!(ponko::input::Glyphs::default().is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items[0].is_none()
    }

    /// The glyphs, in reading order.
    ///
    /// ```
    /// assert_eq!(ponko::input::Glyphs::default().iter().count(), 0);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = Glyph> + '_ {
        self.items.iter().flatten().copied()
    }

    /// The first glyph, if any.
    ///
    /// ```
    /// assert_eq!(ponko::input::Glyphs::default().first(), None);
    /// ```
    #[must_use]
    pub fn first(&self) -> Option<Glyph> {
        self.items[0]
    }
}

/// The labels joined with `/` ("W/A/S/D", "A", "Left Mouse"): a text-only
/// prompt.
impl std::fmt::Display for Glyphs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, g) in self.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            f.write_str(g.label())?;
        }
        Ok(())
    }
}

/// Button prompts: an action's binding as glyphs.
impl Input {
    /// The glyphs that show `action`'s binding on the device used last
    /// ([`last_device`](Self::last_device)): its key or mouse button after
    /// keyboard or mouse input, its gamepad button labelled for that pad's
    /// family after gamepad input. Empty when the action has no binding on
    /// that kind of device (or isn't installed). Allocates nothing.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::input::{Gamepad, GamepadButton};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let pad = Gamepad::from_slot(0).unwrap();
    ///     let mut map = ActionMap::default();
    ///     map.set("jump", Action::Button(vec![
    ///         ButtonBinding::Key(Key::Space),
    ///         ButtonBinding::Gamepad { pad, button: GamepadButton::South },
    ///     ]));
    ///     ctx.input.set_actions(map);
    ///     assert_eq!(ctx.input.action_glyphs("jump").to_string(), "Space");
    ///     ctx.input.feed(InputEvent::GamepadButton(pad, GamepadButton::East, true));
    ///     assert_eq!(ctx.input.action_glyphs("jump").to_string(), "South", "an unnamed pad is generic");
    /// });
    /// ```
    #[must_use]
    pub fn action_glyphs(&self, action: impl ActionRef) -> Glyphs {
        self.action_glyphs_for(action, self.last_device())
    }

    /// The glyphs that show `action`'s binding on `device`: a player's own
    /// prompts in local multiplayer ([`player_device`](Self::player_device)).
    /// For a pad, a binding to that pad's slot is preferred, then a binding
    /// to any slot (a map made for player one still shows the right button).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::InputDevice;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mut map = ActionMap::default();
    ///     map.set("move", Action::Axis2(vec![Axis2Binding::Keys { left: Key::A, right: Key::D, up: Key::W, down: Key::S }]));
    ///     ctx.input.set_actions(map);
    ///     assert_eq!(ctx.input.action_glyphs_for("move", InputDevice::KeyboardMouse).to_string(), "W/A/S/D");
    /// });
    /// ```
    #[must_use]
    pub fn action_glyphs_for(&self, action: impl ActionRef, device: InputDevice) -> Glyphs {
        let Some(slot) = action.slot(&self.actions) else {
            return Glyphs::default();
        };
        let Some(action) = self.actions.binding_at(slot) else {
            return Glyphs::default();
        };
        match device {
            InputDevice::Gamepad(pad) => pad_glyphs(action, pad, self.gamepad_family(pad)),
            _ => keyboard_glyphs(action),
        }
    }
}

fn keyboard_glyphs(action: &Action) -> Glyphs {
    for b in action.buttons() {
        match *b {
            ButtonBinding::Key(k) => return Glyphs::from_slice(&[Glyph::Key(k)]),
            ButtonBinding::Mouse(m) => return Glyphs::from_slice(&[Glyph::Mouse(m)]),
            _ => {}
        }
    }
    for a in action.axes() {
        if let AxisBinding::Keys { negative, positive } = *a {
            return Glyphs::from_slice(&[Glyph::Key(negative), Glyph::Key(positive)]);
        }
    }
    for a in action.axes2() {
        if let Axis2Binding::Keys { left, right, up, down } = *a {
            return Glyphs::from_slice(&[Glyph::Key(up), Glyph::Key(left), Glyph::Key(down), Glyph::Key(right)]);
        }
    }
    Glyphs::default()
}

fn pad_glyphs(action: &Action, pad: Gamepad, family: ControllerFamily) -> Glyphs {
    // A binding for this pad first, then one for any pad.
    for mine in [true, false] {
        for b in action.buttons() {
            if let ButtonBinding::Gamepad { pad: p, button } = *b
                && (!mine || p == pad)
            {
                return Glyphs::from_slice(&[Glyph::Button(family, button)]);
            }
        }
        for a in action.axes() {
            if let AxisBinding::Gamepad { pad: p, axis, .. } = *a
                && (!mine || p == pad)
            {
                return Glyphs::from_slice(&[Glyph::Axis(family, axis)]);
            }
        }
        for a in action.axes2() {
            if let Axis2Binding::Gamepad { pad: p, stick, .. } = *a
                && (!mine || p == pad)
            {
                return Glyphs::from_slice(&[Glyph::Stick(family, stick)]);
            }
        }
    }
    Glyphs::default()
}

/// Every [`Glyph`] drawn into one RGBA image, a square cell each, for
/// prompts: upload it once with [`texture`](Self::texture) and draw cells
/// with [`draw_prompt`](Self::draw_prompt) or [`region`](Self::region), or
/// name single glyphs for rich text with [`inline_image`](Self::inline_image).
///
/// The art is the engine's own simple shapes (keycaps, a mouse, round face
/// buttons with letters or geometric symbols, shoulder buttons, sticks, a
/// d-pad), drawn on the CPU when the atlas is made: about 200 cells, so a
/// 48-pixel atlas is 768 × 624 and takes a few milliseconds to build. Make
/// it once in setup.
///
/// ```
/// use ponko::input::{ControllerFamily, GamepadButton, Glyph, GlyphAtlas};
/// let atlas = GlyphAtlas::new(32);
/// let (w, h) = atlas.size();
/// assert_eq!(atlas.rgba().len(), (w * h * 4) as usize);
/// let a = atlas.region(Glyph::Button(ControllerFamily::Xbox, GamepadButton::South));
/// assert_eq!(a.size(), glam::vec2(32.0, 32.0));
/// ```
#[derive(Debug, Clone)]
pub struct GlyphAtlas {
    cell: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    texture: Option<Texture>,
    /// Whether `rgba` changed since `texture` was uploaded.
    dirty: bool,
    /// Glyphs made into their own textures for rich text, and whether each
    /// is up to date with `rgba`.
    inline: Vec<(Glyph, Texture, bool)>,
    /// The layout the key caps are labelled for.
    key_labels: KeyLabels,
}

impl GlyphAtlas {
    /// Cells per row.
    pub const COLUMNS: u32 = 16;

    /// Draws every glyph at `cell` pixels square (clamped to `16..=256`).
    ///
    /// ```
    /// let atlas = ponko::input::GlyphAtlas::new(48);
    /// assert_eq!(atlas.cell_size(), 48);
    /// ```
    #[must_use]
    pub fn new(cell: u32) -> Self {
        let cell = cell.clamp(16, 256);
        let rows = (GLYPH_COUNT as u32).div_ceil(Self::COLUMNS);
        let (width, height) = (Self::COLUMNS * cell, rows * cell);
        let rgba = vec![0; (width * height * 4) as usize];
        let mut atlas = Self {
            cell,
            width,
            height,
            rgba,
            texture: None,
            dirty: false,
            inline: Vec::new(),
            key_labels: KeyLabels::us(),
        };
        atlas.paint_cells(Glyph::all());
        atlas
    }

    /// Draws `glyphs` into their cells.
    fn paint_cells(&mut self, glyphs: impl Iterator<Item = Glyph>) {
        let font = fontdue::Font::from_bytes(epaint_default_fonts::HACK_REGULAR, fontdue::FontSettings::default())
            .expect("invariant: the built-in font is valid");
        let (cell, width) = (self.cell, self.width);
        let mut canvas = Canvas::new(cell);
        for glyph in glyphs {
            canvas.clear();
            paint(&mut canvas, &font, glyph, &self.key_labels);
            let i = glyph.index() as u32;
            let (x0, y0) = ((i % Self::COLUMNS) * cell, (i / Self::COLUMNS) * cell);
            for y in 0..cell {
                let row = ((y0 + y) * width + x0) as usize * 4;
                let src = (y * cell) as usize * 4;
                self.rgba[row..row + cell as usize * 4].copy_from_slice(&canvas.pixels[src..src + cell as usize * 4]);
            }
        }
    }

    /// Labels the key caps for a keyboard layout: pass
    /// [`ctx.input.key_labels()`](Input::key_labels) before drawing prompts,
    /// and a French player's `Key::W` cap reads `Z`. Only the caps whose
    /// label changed are drawn again (nothing at all when none did, so
    /// calling it every frame costs a comparison); the texture is updated
    /// on its next use.
    ///
    /// ```
    /// use ponko::input::{Glyph, GlyphAtlas, KeyLabels};
    /// use ponko::prelude::*;
    /// let mut atlas = GlyphAtlas::new(32);
    /// let us = atlas.glyph_rgba(Glyph::Key(Key::W));
    /// atlas.set_key_labels(&KeyLabels::from_layout(|k| (k == Key::W).then_some('z')));
    /// assert_ne!(atlas.glyph_rgba(Glyph::Key(Key::W)), us, "the cap reads Z now");
    /// assert_eq!(atlas.glyph_rgba(Glyph::Key(Key::Z)), GlyphAtlas::new(32).glyph_rgba(Glyph::Key(Key::Z)));
    /// ```
    pub fn set_key_labels(&mut self, labels: &KeyLabels) {
        if *labels == self.key_labels {
            return;
        }
        let changed: Vec<Glyph> = Key::ALL
            .into_iter()
            .filter(|&k| labels.cap(k) != self.key_labels.cap(k))
            .map(Glyph::Key)
            .collect();
        self.key_labels = labels.clone();
        if changed.is_empty() {
            return;
        }
        for (glyph, _, fresh) in &mut self.inline {
            if changed.contains(glyph) {
                *fresh = false;
            }
        }
        self.paint_cells(changed.into_iter());
        self.dirty = true;
    }

    /// The layout the key caps are labelled for ([`set_key_labels`](Self::set_key_labels)).
    #[must_use]
    pub fn key_labels(&self) -> &KeyLabels {
        &self.key_labels
    }

    /// The side of one cell, in pixels.
    ///
    /// ```
    /// assert_eq!(ponko::input::GlyphAtlas::new(8).cell_size(), 16, "clamped");
    /// ```
    #[must_use]
    pub fn cell_size(&self) -> u32 {
        self.cell
    }

    /// The image's width and height, in pixels.
    ///
    /// ```
    /// let (w, _) = ponko::input::GlyphAtlas::new(16).size();
    /// assert_eq!(w, 16 * ponko::input::GlyphAtlas::COLUMNS);
    /// ```
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The image: RGBA8, straight alpha, rows top to bottom.
    ///
    /// ```
    /// let atlas = ponko::input::GlyphAtlas::new(16);
    /// assert!(atlas.rgba().iter().any(|&b| b != 0));
    /// ```
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Where `glyph` is in the image, in pixels: the `region` for
    /// [`DrawParams::with_region`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{Glyph, GlyphAtlas};
    /// let atlas = GlyphAtlas::new(32);
    /// assert_eq!(atlas.region(Glyph::Key(Key::A)), Rect::new(0.0, 0.0, 32.0, 32.0));
    /// ```
    #[must_use]
    pub fn region(&self, glyph: Glyph) -> Rect {
        let i = glyph.index() as u32;
        let c = self.cell as f32;
        Rect::new((i % Self::COLUMNS) as f32 * c, (i / Self::COLUMNS) as f32 * c, c, c)
    }

    /// One glyph's pixels: a `cell_size()` square, RGBA8.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let atlas = ponko::input::GlyphAtlas::new(16);
    /// assert_eq!(atlas.glyph_rgba(ponko::input::Glyph::Key(Key::Q)).len(), 16 * 16 * 4);
    /// ```
    #[must_use]
    pub fn glyph_rgba(&self, glyph: Glyph) -> Vec<u8> {
        let r = self.region(glyph);
        let (x0, y0, c) = (r.min.x as u32, r.min.y as u32, self.cell);
        let mut out = Vec::with_capacity((c * c * 4) as usize);
        for y in 0..c {
            let row = ((y0 + y) * self.width + x0) as usize * 4;
            out.extend_from_slice(&self.rgba[row..row + c as usize * 4]);
        }
        out
    }

    /// The atlas as a texture, uploaded on the first call.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn setup(ctx: &mut Ctx) {
    /// let mut atlas = ponko::input::GlyphAtlas::new(48);
    /// let texture = atlas.texture(&mut ctx.gfx);
    /// # let _ = texture; }
    /// ```
    pub fn texture(&mut self, gfx: &mut Gfx) -> Texture {
        match self.texture {
            Some(texture) => {
                if std::mem::take(&mut self.dirty) {
                    gfx.update_texture(texture, &self.rgba);
                }
                texture
            }
            None => {
                self.dirty = false;
                *self
                    .texture
                    .insert(gfx.create_texture(self.width, self.height, &self.rgba))
            }
        }
    }

    /// Names `glyph` for rich text (`[img=<name>]`, with
    /// [`Glyph::name`]) and returns the markup that shows it, with its label
    /// as the alt text (keys labelled for the atlas's
    /// [`key_labels`](Self::key_labels)): `[img=pad.xbox.south alt=A]`. The
    /// glyph's texture is made on the first call for it, and drawn again on
    /// the first call after its key's label changed.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::input::GlyphAtlas;
    /// # fn prompt(ctx: &mut Ctx, atlas: &mut GlyphAtlas, label: ponko::ui::UiId) {
    /// let glyphs = ctx.input.action_glyphs("jump");
    /// let mut text = String::from("Press ");
    /// for glyph in glyphs.iter() {
    ///     text += &atlas.inline_image(&mut ctx.gfx, &mut ctx.ui, glyph);
    /// }
    /// text += " to jump";
    /// ctx.ui.set_text(label, &text);
    /// # }
    /// ```
    pub fn inline_image(&mut self, gfx: &mut Gfx, ui: &mut crate::ui::Ui, glyph: Glyph) -> String {
        let name = glyph.name();
        match self.inline.iter().position(|(g, ..)| *g == glyph) {
            None => {
                let texture = gfx.create_texture(self.cell, self.cell, &self.glyph_rgba(glyph));
                ui.set_inline_image(&name, texture);
                self.inline.push((glyph, texture, true));
            }
            Some(at) if !self.inline[at].2 => {
                let rgba = self.glyph_rgba(glyph);
                gfx.update_texture(self.inline[at].1, &rgba);
                self.inline[at].2 = true;
            }
            Some(_) => {}
        }
        format!(
            "[img={name} alt={}]",
            self.key_labels.glyph(glyph).replace(' ', "\u{a0}")
        )
    }

    /// Draws a prompt row in window pixels: the glyphs as `height`-pixel
    /// squares from `pos` (top-left), then `label` after them in `color`.
    /// Returns the row's width, to place the next prompt. Draws nothing for
    /// the glyphs when there are none (the label still shows).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::input::GlyphAtlas;
    /// fn hud(ctx: &mut Ctx, atlas: &mut GlyphAtlas) {
    ///     let jump = ctx.input.action_glyphs("jump");
    ///     let width = atlas.draw_prompt(&mut ctx.gfx, vec2(16.0, 16.0), 32.0, &jump, "Jump", Color::WHITE);
    ///     let back = ctx.input.action_glyphs("back");
    ///     atlas.draw_prompt(&mut ctx.gfx, vec2(32.0 + width, 16.0), 32.0, &back, "Back", Color::WHITE);
    /// }
    /// ```
    pub fn draw_prompt(
        &mut self,
        gfx: &mut Gfx,
        pos: Vec2,
        height: f32,
        glyphs: &Glyphs,
        label: &str,
        color: Color,
    ) -> f32 {
        let texture = self.texture(gfx);
        self.draw_prompt_on(gfx, texture, pos, height, glyphs, label, color)
    }

    /// [`draw_prompt`](Self::draw_prompt) with the atlas's texture given (the
    /// offscreen test draws with a stand-in).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_prompt_on(
        &self,
        gfx: &mut Gfx,
        texture: Texture,
        pos: Vec2,
        height: f32,
        glyphs: &Glyphs,
        label: &str,
        color: Color,
    ) -> f32 {
        let scale = height / self.cell as f32;
        let gap = (height * 0.15).round();
        let mut x = pos.x;
        for glyph in glyphs.iter() {
            let params = DrawParams::default()
                .with_space(crate::draw_space::DrawSpace::Screen)
                .with_region(self.region(glyph))
                .with_pivot(Vec2::ZERO)
                .with_scale(Vec2::splat(scale));
            gfx.draw_ex(texture, Vec2::new(x, pos.y), params);
            x += height + gap * 0.5;
        }
        if !label.is_empty() {
            if !glyphs.is_empty() {
                x += gap;
            }
            let size = height * 0.6;
            let text = gfx.measure_text(label, size);
            gfx.draw_text(label, Vec2::new(x, pos.y + (height - text.y) * 0.5), size, color);
            x += text.x;
        }
        x - pos.x
    }
}

// ---------------------------------------------------------------- drawing

/// One cell being drawn: straight-alpha RGBA, coordinates in cell units
/// (`0..1` across).
struct Canvas {
    side: u32,
    pixels: Vec<u8>,
}

/// Colors of the art.
mod ink {
    pub const CAP: [u8; 3] = [232, 232, 236];
    pub const CAP_EDGE: [u8; 3] = [92, 96, 104];
    pub const CAP_TEXT: [u8; 3] = [32, 34, 38];
    pub const BODY: [u8; 3] = [44, 48, 56];
    pub const BODY_EDGE: [u8; 3] = [200, 204, 212];
    pub const LIGHT: [u8; 3] = [240, 240, 244];
    pub const DIM: [u8; 3] = [120, 124, 132];
    pub const ACCENT: [u8; 3] = [242, 178, 51];
    pub const GREEN: [u8; 3] = [96, 196, 104];
    pub const RED: [u8; 3] = [228, 92, 76];
    pub const BLUE: [u8; 3] = [86, 150, 228];
    pub const YELLOW: [u8; 3] = [240, 200, 64];
    pub const TEAL: [u8; 3] = [76, 196, 168];
    pub const PINK: [u8; 3] = [228, 140, 196];
}

impl Canvas {
    fn new(side: u32) -> Self {
        Self {
            side,
            pixels: vec![0; (side * side * 4) as usize],
        }
    }

    fn clear(&mut self) {
        self.pixels.fill(0);
    }

    /// Blends `color` over every pixel by the coverage of the shape whose
    /// signed distance (cell units, negative inside) `sdf` gives.
    fn fill(&mut self, color: [u8; 3], sdf: impl Fn(Vec2) -> f32) {
        let side = self.side as f32;
        for y in 0..self.side {
            for x in 0..self.side {
                let p = Vec2::new((x as f32 + 0.5) / side, (y as f32 + 0.5) / side);
                let coverage = (0.5 - sdf(p) * side).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    fn blend(&mut self, x: u32, y: u32, color: [u8; 3], alpha: f32) {
        let i = ((y * self.side + x) * 4) as usize;
        let dst = &mut self.pixels[i..i + 4];
        let da = f32::from(dst[3]) / 255.0;
        let out_a = alpha + da * (1.0 - alpha);
        if out_a <= 0.0 {
            return;
        }
        for c in 0..3 {
            let v = (f32::from(color[c]) * alpha + f32::from(dst[c]) * da * (1.0 - alpha)) / out_a;
            dst[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        dst[3] = (out_a * 255.0).round() as u8;
    }

    /// `text` centered on `center` (cell units), at most `height` tall and
    /// `width` wide (cell units).
    fn text(&mut self, font: &fontdue::Font, text: &str, center: Vec2, height: f32, width: f32, color: [u8; 3]) {
        if text.is_empty() {
            return;
        }
        let side = self.side as f32;
        let mut px = height * side;
        let advance = |px: f32| text.chars().map(|c| font.metrics(c, px).advance_width).sum::<f32>();
        let w = advance(px);
        if w > width * side {
            px *= width * side / w;
        }
        let total = advance(px);
        let Some(line) = font.horizontal_line_metrics(px) else {
            return;
        };
        // Caps sit centered: the cap height is about 0.7 of the ascent.
        let cap = line.ascent * 0.72;
        let baseline = center.y * side + cap * 0.5;
        let mut pen = center.x * side - total * 0.5;
        for ch in text.chars() {
            let (m, bitmap) = font.rasterize(ch, px);
            let left = (pen + m.xmin as f32).round() as i32;
            let top = (baseline - m.ymin as f32 - m.height as f32).round() as i32;
            for gy in 0..m.height {
                for gx in 0..m.width {
                    let a = f32::from(bitmap[gy * m.width + gx]) / 255.0;
                    let (x, y) = (left + gx as i32, top + gy as i32);
                    if a > 0.0 && x >= 0 && y >= 0 && (x as u32) < self.side && (y as u32) < self.side {
                        self.blend(x as u32, y as u32, color, a);
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}

fn circle(c: Vec2, r: f32) -> impl Fn(Vec2) -> f32 {
    move |p| (p - c).length() - r
}

fn ring(c: Vec2, r: f32, w: f32) -> impl Fn(Vec2) -> f32 {
    move |p| ((p - c).length() - r).abs() - w * 0.5
}

fn rounded(c: Vec2, half: Vec2, radius: f32) -> impl Fn(Vec2) -> f32 {
    move |p| {
        let q = (p - c).abs() - half + Vec2::splat(radius);
        q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius
    }
}

fn segment(a: Vec2, b: Vec2, w: f32) -> impl Fn(Vec2) -> f32 {
    move |p| {
        let (pa, ba) = (p - a, b - a);
        let h = (pa.dot(ba) / ba.dot(ba)).clamp(0.0, 1.0);
        (pa - ba * h).length() - w * 0.5
    }
}

/// A filled triangle pointing `dir` (unit), centered on `c`, `r` from center
/// to tip.
fn arrow_head(c: Vec2, dir: Vec2, r: f32) -> impl Fn(Vec2) -> f32 {
    let side = dir.perp();
    let tip = c + dir * r;
    let a = c - dir * r * 0.6 + side * r * 0.85;
    let b = c - dir * r * 0.6 - side * r * 0.85;
    move |p| {
        // Distance to a convex triangle: the largest of its edge distances
        // (inside negative), good enough for coverage.
        let edge = |u: Vec2, v: Vec2| {
            let n = (v - u).perp().normalize();
            (p - u).dot(n)
        };
        // Wound so outward normals point away from the center.
        edge(tip, b).max(edge(b, a)).max(edge(a, tip))
    }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

const CENTER: Vec2 = Vec2::new(0.5, 0.5);

fn paint(canvas: &mut Canvas, font: &fontdue::Font, glyph: Glyph, labels: &KeyLabels) {
    match glyph {
        Glyph::Key(k) => paint_key(canvas, font, k, labels.cap(k)),
        Glyph::Mouse(b) => paint_mouse(canvas, b),
        Glyph::Button(f, b) => paint_button(canvas, font, f, b),
        Glyph::Stick(_, s) => {
            paint_stick(canvas, font, if s == GamepadStick::Left { "L" } else { "R" });
            for dir in [v(0.0, -1.0), v(0.0, 1.0), v(-1.0, 0.0), v(1.0, 0.0)] {
                canvas.fill(ink::LIGHT, arrow_head(CENTER + dir * 0.4, dir, 0.06));
            }
        }
        Glyph::Axis(f, a) => match a {
            GamepadAxis::LeftTrigger => paint_button(canvas, font, f, GamepadButton::LeftTrigger),
            GamepadAxis::RightTrigger => paint_button(canvas, font, f, GamepadButton::RightTrigger),
            _ => {
                let left = matches!(a, GamepadAxis::LeftStickX | GamepadAxis::LeftStickY);
                paint_stick(canvas, font, if left { "L" } else { "R" });
                let horizontal = matches!(a, GamepadAxis::LeftStickX | GamepadAxis::RightStickX);
                let dirs = if horizontal {
                    [v(-1.0, 0.0), v(1.0, 0.0)]
                } else {
                    [v(0.0, -1.0), v(0.0, 1.0)]
                };
                for dir in dirs {
                    canvas.fill(ink::ACCENT, arrow_head(CENTER + dir * 0.4, dir, 0.08));
                }
            }
        },
    }
}

fn paint_key(canvas: &mut Canvas, font: &fontdue::Font, k: Key, text: &str) {
    let half = v(0.42, 0.40);
    // The cap's lower edge, then its face and outline.
    canvas.fill(ink::CAP_EDGE, rounded(CENTER + v(0.0, 0.04), half, 0.1));
    canvas.fill(ink::CAP_EDGE, rounded(CENTER, half, 0.1));
    canvas.fill(ink::CAP, rounded(CENTER - v(0.0, 0.01), half - v(0.04, 0.04), 0.07));
    let arrow = match k {
        Key::Up => Some(v(0.0, -1.0)),
        Key::Down => Some(v(0.0, 1.0)),
        Key::Left => Some(v(-1.0, 0.0)),
        Key::Right => Some(v(1.0, 0.0)),
        _ => None,
    };
    if let Some(dir) = arrow {
        canvas.fill(ink::CAP_TEXT, segment(CENTER - dir * 0.18, CENTER + dir * 0.05, 0.07));
        canvas.fill(ink::CAP_TEXT, arrow_head(CENTER + dir * 0.06, dir, 0.14));
        return;
    }
    let height = if text.chars().count() <= 2 { 0.5 } else { 0.34 };
    canvas.text(font, text, CENTER - v(0.0, 0.01), height, 0.7, ink::CAP_TEXT);
}

fn paint_mouse(canvas: &mut Canvas, b: MouseButton) {
    let body = rounded(CENTER, v(0.28, 0.40), 0.26);
    canvas.fill(ink::CAP_EDGE, &body);
    canvas.fill(ink::CAP, rounded(CENTER, v(0.24, 0.36), 0.22));
    // The buttons' split line and the wheel.
    let top = CENTER.y - 0.36;
    let split = CENTER.y - 0.05;
    let inside = rounded(CENTER, v(0.24, 0.36), 0.22);
    let half_box = |left: bool| {
        let inside = rounded(CENTER, v(0.24, 0.36), 0.22);
        move |p: Vec2| {
            let side = if left { p.x - CENTER.x } else { CENTER.x - p.x };
            inside(p).max(side).max(p.y - split)
        }
    };
    match b {
        MouseButton::Left => canvas.fill(ink::ACCENT, half_box(true)),
        MouseButton::Right => canvas.fill(ink::ACCENT, half_box(false)),
        _ => {}
    }
    canvas.fill(ink::CAP_EDGE, segment(v(0.24, split), v(0.76, split), 0.03));
    canvas.fill(ink::CAP_EDGE, move |p: Vec2| {
        inside(p).max(segment(v(0.5, top), v(0.5, split), 0.03)(p))
    });
    let wheel = rounded(v(0.5, CENTER.y - 0.2), v(0.045, 0.08), 0.04);
    let wheel_color = if b == MouseButton::Middle {
        ink::ACCENT
    } else {
        ink::CAP_EDGE
    };
    canvas.fill(wheel_color, wheel);
    if matches!(b, MouseButton::Back | MouseButton::Forward) {
        let y = if b == MouseButton::Back { 0.58 } else { 0.42 };
        canvas.fill(ink::ACCENT, rounded(v(0.2, y), v(0.05, 0.07), 0.03));
        let dir = if b == MouseButton::Back {
            v(-1.0, 0.0)
        } else {
            v(1.0, 0.0)
        };
        canvas.fill(ink::CAP_TEXT, arrow_head(v(0.5, 0.66), dir, 0.09));
    }
}

/// A round face button: the dark disc and its light rim.
fn face_disc(canvas: &mut Canvas) {
    canvas.fill(ink::BODY_EDGE, circle(CENTER, 0.44));
    canvas.fill(ink::BODY, circle(CENTER, 0.40));
}

fn paint_button(canvas: &mut Canvas, font: &fontdue::Font, f: ControllerFamily, b: GamepadButton) {
    use GamepadButton as B;
    let text = button_label(f, b).1;
    match b {
        B::South | B::East | B::North | B::West => {
            face_disc(canvas);
            match f {
                ControllerFamily::Xbox => {
                    let color = match b {
                        B::South => ink::GREEN,
                        B::East => ink::RED,
                        B::North => ink::YELLOW,
                        _ => ink::BLUE,
                    };
                    canvas.text(font, text, CENTER, 0.5, 0.6, color);
                }
                ControllerFamily::Nintendo => canvas.text(font, text, CENTER, 0.5, 0.6, ink::LIGHT),
                ControllerFamily::PlayStation => match b {
                    B::South => {
                        canvas.fill(ink::BLUE, segment(v(0.33, 0.33), v(0.67, 0.67), 0.07));
                        canvas.fill(ink::BLUE, segment(v(0.67, 0.33), v(0.33, 0.67), 0.07));
                    }
                    B::East => canvas.fill(ink::RED, ring(CENTER, 0.18, 0.07)),
                    B::North => {
                        let (a, b2, c) = (v(0.5, 0.29), v(0.69, 0.63), v(0.31, 0.63));
                        for (p, q) in [(a, b2), (b2, c), (c, a)] {
                            canvas.fill(ink::TEAL, segment(p, q, 0.065));
                        }
                    }
                    _ => {
                        let s = 0.17;
                        let corners = [
                            v(0.5 - s, 0.5 - s),
                            v(0.5 + s, 0.5 - s),
                            v(0.5 + s, 0.5 + s),
                            v(0.5 - s, 0.5 + s),
                        ];
                        for i in 0..4 {
                            canvas.fill(ink::PINK, segment(corners[i], corners[(i + 1) % 4], 0.065));
                        }
                    }
                },
                _ => {
                    // Four dots in a diamond, this button's lit.
                    let dirs = [
                        (B::South, v(0.0, 1.0)),
                        (B::East, v(1.0, 0.0)),
                        (B::North, v(0.0, -1.0)),
                        (B::West, v(-1.0, 0.0)),
                    ];
                    for (which, dir) in dirs {
                        let at = CENTER + dir * 0.2;
                        if which == b {
                            canvas.fill(ink::ACCENT, circle(at, 0.1));
                        } else {
                            canvas.fill(ink::DIM, ring(at, 0.075, 0.035));
                        }
                    }
                }
            }
        }
        B::LeftBumper | B::RightBumper => {
            canvas.fill(ink::BODY_EDGE, rounded(CENTER, v(0.44, 0.24), 0.12));
            canvas.fill(ink::BODY, rounded(CENTER, v(0.40, 0.20), 0.09));
            canvas.fill(ink::LIGHT, rounded(CENTER - v(0.0, 0.17), v(0.30, 0.012), 0.012));
            canvas.text(font, text, CENTER + v(0.0, 0.02), 0.34, 0.66, ink::LIGHT);
        }
        B::LeftTrigger | B::RightTrigger => {
            // A trigger: rounded on top, square below.
            let shape = |inset: f32| {
                let top = circle(v(0.5, 0.42), 0.38 - inset);
                let bottom = rounded(v(0.5, 0.6), v(0.38 - inset, 0.3 - inset), 0.06);
                move |p: Vec2| top(p).min(bottom(p)).max(p.y - (0.92 - inset)).max(0.08 + inset - p.y)
            };
            canvas.fill(ink::BODY_EDGE, shape(0.0));
            canvas.fill(ink::BODY, shape(0.04));
            canvas.text(font, text, CENTER + v(0.0, 0.06), 0.36, 0.62, ink::LIGHT);
        }
        B::Select | B::Start => {
            canvas.fill(ink::BODY_EDGE, rounded(CENTER, v(0.44, 0.2), 0.2));
            canvas.fill(ink::BODY, rounded(CENTER, v(0.40, 0.16), 0.16));
            let height = if text.chars().count() <= 1 { 0.42 } else { 0.26 };
            canvas.text(font, text, CENTER, height, 0.72, ink::LIGHT);
        }
        B::LeftStick | B::RightStick => {
            paint_stick(canvas, font, text);
            canvas.fill(ink::ACCENT, arrow_head(v(0.5, 0.86), v(0.0, 1.0), 0.07));
        }
        B::Touchpad => {
            canvas.fill(ink::BODY_EDGE, rounded(CENTER, v(0.42, 0.27), 0.09));
            canvas.fill(ink::BODY, rounded(CENTER, v(0.37, 0.22), 0.07));
            canvas.fill(ink::ACCENT, circle(CENTER, 0.08));
        }
        B::DPadUp | B::DPadDown | B::DPadLeft | B::DPadRight => {
            let plus = |w: f32, l: f32| {
                let a = rounded(CENTER, v(w, l), 0.05);
                let b2 = rounded(CENTER, v(l, w), 0.05);
                move |p: Vec2| a(p).min(b2(p))
            };
            canvas.fill(ink::BODY_EDGE, plus(0.17, 0.44));
            canvas.fill(ink::BODY, plus(0.13, 0.40));
            let dir = match b {
                B::DPadUp => v(0.0, -1.0),
                B::DPadDown => v(0.0, 1.0),
                B::DPadLeft => v(-1.0, 0.0),
                _ => v(1.0, 0.0),
            };
            canvas.fill(ink::ACCENT, arrow_head(CENTER + dir * 0.26, dir, 0.1));
        }
    }
}

/// A stick seen from above: the rim, the cap, and its letter.
fn paint_stick(canvas: &mut Canvas, font: &fontdue::Font, text: &str) {
    canvas.fill(ink::BODY_EDGE, circle(CENTER, 0.34));
    canvas.fill(ink::BODY, circle(CENTER, 0.30));
    canvas.fill(ink::DIM, ring(CENTER, 0.2, 0.025));
    canvas.text(font, text, CENTER, 0.3, 0.4, ink::LIGHT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(atlas: &GlyphAtlas, glyph: Glyph) -> usize {
        atlas.glyph_rgba(glyph).chunks(4).filter(|p| p[3] > 128).count()
    }

    #[test]
    fn every_glyph_has_its_own_cell() {
        let mut seen = [false; GLYPH_COUNT];
        for glyph in Glyph::all() {
            let i = glyph.index();
            assert!(!seen[i], "{glyph:?} shares cell {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&s| s), "every cell is used");
    }

    #[test]
    fn every_glyph_is_drawn() {
        let atlas = GlyphAtlas::new(32);
        for glyph in Glyph::all() {
            assert!(ink(&atlas, glyph) > 40, "{glyph:?} has ink");
        }
    }

    #[test]
    fn families_draw_their_face_buttons_differently() {
        let atlas = GlyphAtlas::new(32);
        let cells: Vec<Vec<u8>> = ControllerFamily::ALL
            .iter()
            .map(|&f| atlas.glyph_rgba(Glyph::Button(f, GamepadButton::South)))
            .collect();
        for i in 0..cells.len() {
            for j in i + 1..cells.len() {
                assert_ne!(cells[i], cells[j], "families {i} and {j} differ");
            }
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<String> = Glyph::all().map(Glyph::name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
        assert_eq!(Glyph::Key(Key::LeftShift).name(), "key.left_shift");
    }

    #[test]
    fn every_glyph_has_a_label() {
        for glyph in Glyph::all() {
            assert!(!glyph.label().is_empty(), "{glyph:?}");
        }
    }
}
