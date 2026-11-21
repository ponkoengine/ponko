//! Retained, game-styled UI. Taffy owns only layout; this module owns nodes,
//! focus, screens and events, and Ponko draws the results with sprites/text.
//! Taffy's flex/grid algorithms embed without a renderer or event loop.
//! Egui embeds well but is immediate-mode (and remains the inspector); the
//! standard Xilem/Masonry stack couples widgets to Vello and winit. Keeping
//! our tree makes game-owned textures, stable IDs and queued events natural.
//! Layout uses logical window units; the window handles DPI and `Ui::set_scale`
//! adds an accessibility/user scale. Nodes persist until explicitly removed.

use super::anim::Xf;
use super::build::UiLayout;
use crate::{camera::Rect, color::Color, text::Font, texture::Texture};
use glam::{Vec2, vec2};
use taffy::prelude::*;

/// Stable node identity, never reused within a UI.
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// let id: ponko::ui::UiId = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
/// assert!(ui.rect(id).is_some());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UiId(pub(super) usize);

/// Bits of a [`UiId`] that name its slot in the node table; the bits above
/// count how often the slot was used before. A removed node's slot is
/// used again under the next generation, so a UI that builds and removes
/// menus all session keeps a table the size of its largest UI, and an id
/// kept past [`Ui::remove`] never names the node made in its slot. A slot
/// whose generations run out is retired instead of wrapping (after 4095
/// uses on 32-bit targets; never in practice on 64-bit ones).
const SLOT_BITS: u32 = if usize::BITS >= 64 { 32 } else { 20 };
const SLOT_MASK: usize = (1 << SLOT_BITS) - 1;
/// The last generation, never given out: `UiId::from_bits(u64::MAX)` has it.
const LAST_GENERATION: usize = usize::MAX >> SLOT_BITS;

impl UiId {
    /// The node table slot this id names (live or stale).
    #[inline]
    pub(super) const fn slot(self) -> usize {
        self.0 & SLOT_MASK
    }

    /// The id as an integer, for hosts that store ids outside the UI (the C
    /// API does).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// assert_eq!(ponko::ui::UiId::from_bits(id.bits()), id);
    /// ```
    pub const fn bits(self) -> u64 {
        self.0 as u64
    }
    /// Rebuild an id from [`UiId::bits`]. Unknown values are stale: every call
    /// ignores or rejects them instead of panicking.
    /// ```
    /// use ponko::prelude::*;
    /// assert!(ponko::ui::Ui::default().rect(ponko::ui::UiId::from_bits(7)).is_none());
    /// ```
    pub const fn from_bits(bits: u64) -> Self {
        // Out-of-range values (32-bit targets) become an id no node has.
        Self(if bits > usize::MAX as u64 {
            usize::MAX
        } else {
            bits as usize
        })
    }
}

/// Built-in behavior; text, images and decoration are configured separately.
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// let button = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
/// ui.set_text(button, "Buy");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum UiWidget {
    /// Non-focusable container.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Panel;
    /// ```
    Panel,
    /// Non-focusable text.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Label;
    /// ```
    Label,
    /// Press/release or confirm emits `Clicked`.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Button;
    /// ```
    Button,
    /// Non-focusable texture.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Image;
    /// ```
    Image,
    /// Clipped container with two-axis wheel scrolling and vertical drag/bar.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Scroll;
    /// ```
    Scroll,
    /// Single-line Unicode input, with selection, IME and clipboard shortcuts.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::TextInput;
    /// ```
    TextInput,
    /// Checkbox; confirm or clicking toggles its value.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Toggle;
    /// ```
    Toggle,
    /// Normalized 0..1 value, changed by pointer or left/right.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiWidget::Slider;
    /// ```
    Slider,
    /// A select box: shows its chosen option and opens a list of options
    /// ([`Ui::dropdown`]).
    Dropdown,
    /// A tab strip with one page per tab ([`Ui::tabs`], [`Ui::tab`]).
    Tabs,
    /// One tab of a [`Tabs`](Self::Tabs) strip; activating it shows its page.
    Tab,
    /// A group of radio buttons, one of which is chosen ([`Ui::radio_group`]).
    RadioGroup,
    /// One option of a [`RadioGroup`](Self::RadioGroup) ([`Ui::radio`]).
    Radio,
    /// A bar filled to its value, 0 to 1 ([`Ui::progress_bar`]).
    Progress,
    /// A busy indicator that turns while shown ([`Ui::spinner`]).
    Spinner,
    /// A number between a minimum and a maximum, stepped with the arrows or
    /// its `-` and `+` ends, or typed ([`Ui::number_field`]).
    NumberField,
    /// A scrolling list or grid of any number of items that keeps only the
    /// rows on screen as nodes ([`Ui::list`], [`Ui::grid_view`]).
    List,
    /// One shown row (or cell) of a [`List`](Self::List); the list reuses it
    /// for the items scrolled into view ([`Ui::list_item`]).
    ListItem,
    /// The pop-up list of a dropdown or a context menu, open as the top screen.
    Menu,
    /// One entry of a [`Menu`](Self::Menu).
    MenuItem,
    /// Two panes split by a divider the player drags ([`Ui::split`]).
    Split,
    /// The divider between a [`Split`](Self::Split)'s panes.
    Splitter,
    /// A field that shows a key, mouse button or gamepad button and records
    /// the next one pressed after it is activated ([`Ui::key_binding`]).
    KeyBinding,
}

/// Text alignment within a node's content box.
/// ```
/// use ponko::prelude::*;
/// let paint = ponko::ui::UiPaint { align: ponko::ui::UiTextAlign::Center, ..Default::default() };
/// assert_eq!(paint.align, ponko::ui::UiTextAlign::Center);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UiTextAlign {
    /// Start of each line.
    #[default]
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiTextAlign::Start;
    /// ```
    Start,
    /// Center each line.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiTextAlign::Center;
    /// ```
    Center,
    /// End of each line.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiTextAlign::End;
    /// ```
    End,
}

/// Visual style, independent of Taffy's layout style. A texture with
/// `slice > 0` is drawn as nine patches; `slice` is the source border in pixels.
/// ```
/// use ponko::prelude::*;
/// let paint = ponko::ui::UiPaint { background: Color::BLUE, font_size: 20.0, ..Default::default() };
/// assert_eq!(paint.font_size, 20.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct UiPaint {
    /// Normal background.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().background;
    /// ```
    pub background: Color,
    /// Hovered/focused background.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().hover;
    /// ```
    pub hover: Color,
    /// Pointer-held background.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().pressed;
    /// ```
    pub pressed: Color,
    /// Disabled background.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().disabled;
    /// ```
    pub disabled: Color,
    /// Border and focus-ring color.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().border;
    /// ```
    pub border: Color,
    /// Border thickness in logical units.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().border_width;
    /// ```
    pub border_width: f32,
    /// Text and image tint.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().color;
    /// ```
    pub color: Color,
    /// Font handle, from `Graphics::load_font`.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().font;
    /// ```
    pub font: Font,
    /// Text line height in logical units.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().font_size;
    /// ```
    pub font_size: f32,
    /// Break text at word boundaries, splitting oversized words.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().wrap;
    /// ```
    pub wrap: bool,
    /// Horizontal text alignment.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().align;
    /// ```
    pub align: UiTextAlign,
    /// Image or background texture; `None` draws only color.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().texture;
    /// ```
    pub texture: Option<Texture>,
    /// Source border size for nine-slice drawing; zero stretches the image.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiPaint::default().slice;
    /// ```
    pub slice: f32,
}
impl Default for UiPaint {
    fn default() -> Self {
        Self {
            background: Color::rgba(0.12, 0.14, 0.18, 1.0),
            hover: Color::rgba(0.25, 0.3, 0.4, 1.0),
            pressed: Color::rgba(0.08, 0.1, 0.15, 1.0),
            disabled: Color::rgba(0.15, 0.15, 0.15, 0.5),
            border: Color::hex(0xf28c28),
            border_width: 0.0,
            color: Color::WHITE,
            font: Font::DEFAULT,
            font_size: 18.0,
            wrap: false,
            align: UiTextAlign::Start,
            texture: None,
            slice: 0.0,
        }
    }
}

/// One semantic event; values are read from the node after `Changed`.
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// assert_eq!(ui.events().count(), 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEvent {
    /// A button was activated.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// let _ = ponko::ui::UiEvent::Clicked(id);
    /// ```
    Clicked(UiId),
    /// Text, checkbox or slider value changed.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// let _ = ponko::ui::UiEvent::Changed(id);
    /// ```
    Changed(UiId),
    /// Enter/confirm in a text field.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// let _ = ponko::ui::UiEvent::Submitted(id);
    /// ```
    Submitted(UiId),
    /// The screen was closed by the player: Cancel (Escape, a gamepad's East
    /// button) or a button that [closes its screen](Ui::set_closes_screen)
    /// popped it. The input is consumed for this frame.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// let _ = ponko::ui::UiEvent::ScreenPopped(id);
    /// ```
    ScreenPopped(UiId),
    /// A link in a rich node's text ([`Ui::set_rich`]) was clicked: the
    /// node, and which of its links, counted from 0 in the order they
    /// appear. [`Ui::link`] gives its target.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout, UiWidget};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let line = ui.label(root, "[link=rules]Rules[/link]").rich().id();
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// let at = ui.rect(line).unwrap().min + vec2(4.0, 4.0);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { pointer: at, pressed: true, held: true, ..Default::default() });
    /// ui.frame(vec2(320.0, 240.0), &UiInput { pointer: at, released: true, ..Default::default() });
    /// let link = ui.events().find_map(|e| match e { UiEvent::LinkClicked(id, link) => Some((id, link)), _ => None });
    /// assert_eq!(link, Some((line, 0)));
    /// assert_eq!(ui.link(line, 0), Some("rules"));
    /// ```
    LinkClicked(UiId, u32),
    /// An entry of a node's context menu ([`Ui::set_context_menu`]) was
    /// chosen: the node, and which entry, counted from 0.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let slot = ui.button(root, "Potion").context_menu(["Use", "Drop"]).id();
    /// ui.push_modeless(root);
    /// ui.open_context_menu(slot);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { direction: vec2(0.0, 1.0), ..Default::default() });
    /// ui.frame(vec2(320.0, 240.0), &UiInput { confirm: true, ..Default::default() });
    /// assert!(ui.events().any(|e| e == UiEvent::Chosen(slot, 1)));
    /// ```
    Chosen(UiId, u32),
    /// A draggable node ([`Ui::set_draggable`]) was dropped on a drop target
    /// ([`Ui::set_drop_target`]): the item, then the target. Nothing moves
    /// on its own; [`Ui::set_parent`] moves the item if the game accepts.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let bag = ui.panel(root).layout(UiLayout::fixed(100.0, 100.0)).drop_target().id();
    /// let chest = ui.panel(root).layout(UiLayout::fixed(100.0, 100.0)).drop_target().id();
    /// let gem = ui.label(bag, "Gem").draggable().id();
    /// ui.push_modeless(root);
    /// ui.set_focus(gem);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { confirm: true, ..Default::default() }); // pick up
    /// ui.set_focus(chest);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { confirm: true, ..Default::default() }); // drop
    /// assert!(ui.events().any(|e| e == UiEvent::Dropped(gem, chest)));
    /// ```
    Dropped(UiId, UiId),
    /// A list ([`Ui::list`]) shows other items than before: it scrolled, was
    /// resized or its length changed. Refill the rows [`Ui::list_rows`]
    /// gives when they show more than their text.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let list = ui.list(root, 20.0).layout(UiLayout::fixed(200.0, 100.0)).id();
    /// ui.set_list_len(list, 1000);
    /// ui.push_modeless(root);
    /// ui.frame(vec2(320.0, 240.0), &UiInput::default());
    /// assert!(ui.events().any(|e| e == UiEvent::RowsShown(list)));
    /// ```
    RowsShown(UiId),
    /// A tween ([`Ui::animate`]) or a timeline ([`Ui::play`]) on the node
    /// ended: its values are where they stay. Not sent for a timeline
    /// that loops, was stopped or had a property taken over.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiTween};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// ui.push_modeless(root);
    /// ui.animate(root, UiTween::opacity(0.5).seconds(0.1));
    /// ui.frame(vec2(320.0, 240.0), &UiInput { dt: 0.2, ..Default::default() });
    /// assert!(ui.events().any(|e| e == UiEvent::AnimationDone(root)));
    /// ```
    AnimationDone(UiId),
}

impl UiEvent {
    /// The node of a [`Clicked`](Self::Clicked) event, else `None`. For a
    /// screen that reacts only to clicks:
    /// `ui.events().filter_map(UiEvent::clicked).last()`.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// assert_eq!(ponko::ui::UiEvent::Clicked(id).clicked(), Some(id));
    /// assert_eq!(ponko::ui::UiEvent::Changed(id).clicked(), None);
    /// ```
    #[must_use]
    pub fn clicked(self) -> Option<UiId> {
        match self {
            Self::Clicked(id) => Some(id),
            _ => None,
        }
    }
}

/// Input snapshot in logical window coordinates. `Ctx` supplies this each
/// frame; tests and custom hosts can call `Ui::frame` directly.
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// ui.frame(vec2(800.0,600.0), &ponko::ui::UiInput::default());
/// ```
#[derive(Debug, Default, Clone)]
pub struct UiInput {
    /// Pointer position in logical window units (before user scaling).
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().pointer;
    /// ```
    pub pointer: Vec2,
    /// Primary pointer pressed this frame.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().pressed;
    /// ```
    pub pressed: bool,
    /// Primary pointer held.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().held;
    /// ```
    pub held: bool,
    /// Primary pointer released this frame.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().released;
    /// ```
    pub released: bool,
    /// Wheel displacement in logical units (positive scrolls down/right).
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().wheel;
    /// ```
    pub wheel: Vec2,
    /// Tab/next focus.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().next;
    /// ```
    pub next: bool,
    /// Shift-tab/previous focus.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().previous;
    /// ```
    pub previous: bool,
    /// Directional focus, caret or slider movement (-1..1 per axis).
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().direction;
    /// ```
    pub direction: Vec2,
    /// Enter/space/gamepad south.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().confirm;
    /// ```
    pub confirm: bool,
    /// Escape/gamepad east, pops a screen.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().cancel;
    /// ```
    pub cancel: bool,
    /// Shift extends text selection.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().shift;
    /// ```
    pub shift: bool,
    /// Select all text.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().select_all;
    /// ```
    pub select_all: bool,
    /// Delete before caret.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().backspace;
    /// ```
    pub backspace: bool,
    /// Delete after caret.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().delete;
    /// ```
    pub delete: bool,
    /// Move caret to start.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().home;
    /// ```
    pub home: bool,
    /// Move caret to end.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().end;
    /// ```
    pub end: bool,
    /// Committed text; control characters are ignored.
    /// ```
    /// use ponko::prelude::*;
    /// let _ = ponko::ui::UiInput::default().text;
    /// ```
    pub text: String,
    /// The IME composition so far ([`Input::preedit`](crate::input::Input::preedit)),
    /// shown underlined at the caret of the focused text input that takes
    /// IME. Empty: none.
    /// ```
    /// let _ = ponko::ui::UiInput::default().preedit;
    /// ```
    pub preedit: String,
    /// The caret inside `preedit`, a byte range
    /// ([`Input::preedit_cursor`](crate::input::Input::preedit_cursor)).
    /// ```
    /// let _ = ponko::ui::UiInput::default().preedit_cursor;
    /// ```
    pub preedit_cursor: Option<(usize, usize)>,
    /// A composition went on this frame
    /// ([`Input::composing`](crate::input::Input::composing)): the focused
    /// text input leaves Enter, Escape, Backspace, Delete, Home, End, the
    /// arrows and select-all to the IME, and takes only committed `text`.
    /// ```
    /// let _ = ponko::ui::UiInput::default().composing;
    /// ```
    pub composing: bool,
    /// Seconds since the last frame: tooltips wait for it
    /// ([`UiTheme::tooltip_delay`](super::UiTheme::tooltip_delay)) and
    /// spinners turn with it. Zero: time stands still.
    /// ```
    /// let _ = ponko::ui::UiInput::default().dt;
    /// ```
    pub dt: f32,
    /// The secondary pointer button (the right mouse button) was pressed
    /// this frame: opens the context menu of the node under the pointer.
    /// ```
    /// let _ = ponko::ui::UiInput::default().secondary;
    /// ```
    pub secondary: bool,
    /// The menu input (Shift+F10, a gamepad's North button) was pressed this
    /// frame: opens the context menu of the focused node.
    /// ```
    /// let _ = ponko::ui::UiInput::default().menu;
    /// ```
    pub menu: bool,
    /// The first key, mouse button or gamepad button pressed this frame, for
    /// a listening key-binding field ([`Ui::key_binding`]).
    /// ```
    /// let _ = ponko::ui::UiInput::default().captured;
    /// ```
    pub captured: Option<crate::actions::ButtonBinding>,
}

pub(super) struct Node {
    /// The id this node was made under: an id naming its slot under an
    /// older generation is stale.
    pub id: UiId,
    pub widget: UiWidget,
    pub layout: NodeId,
    pub parent: Option<UiId>,
    pub children: Vec<UiId>,
    pub text: String,
    pub paint: UiPaint,
    pub enabled: bool,
    pub rect: Rect,
    pub clip: Rect,
    pub scroll: Vec2,
    pub max_scroll: Vec2,
    pub value: f32,
    pub caret: usize,
    pub anchor: usize,
    /// Accessible name override; empty means "derive it from the text".
    pub access_name: String,
    /// What assistive technology reads after the name; empty: nothing.
    pub access_description: String,
    /// Whether changes to it are announced without it having focus.
    pub access_live: super::UiLive,
    /// The name it is found by ([`Ui::find`]); empty: none.
    pub name: String,
    /// A title ([`Ui::title`]): painted from the theme's title color and size.
    pub title: bool,
    /// Its screen opens with it focused ([`Ui::set_initial_focus`]).
    pub initial_focus: bool,
    /// Pressing it also closes its screen ([`Ui::set_closes_screen`]).
    pub closes_screen: bool,
    /// A text input takes IME composition ([`Ui::set_ime_allowed`]).
    pub ime: bool,
    /// The IME composition it shows at its caret; empty: none.
    pub preedit: String,
    /// The caret inside `preedit`, a byte range.
    pub preedit_cursor: Option<(usize, usize)>,
    /// The string table key its text comes from ([`Ui::set_text_key`]).
    pub key: Option<Box<super::retained_locale::TextKey>>,
    /// Its text is markup ([`Ui::set_rich`]), parsed.
    pub rich: Option<Box<super::rich::RichText>>,
    /// What the widget set keeps for the widgets that need more than text
    /// and a value (items, a choice, a number, a binding, a tooltip, ...).
    pub extra: Option<Box<super::widgets::Extra>>,
    /// Its animation: drawn properties, state and screen transitions,
    /// where it is drawn.
    pub motion: super::anim::NodeMotion,
    /// What its text was measured from in the last layout.
    pub measured: super::retained_layout::Measured,
}
/// How a screen pushed on the [`Ui`]'s stack treats the screens below it
/// ([`Ui::push_screen_with_mode`]). Either way only the top screen takes
/// input; more modes may come, so a `match` needs a `_` arm.
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::ScreenMode;
/// let mut ui = ponko::ui::Ui::default();
/// let root = ui.root().id();
/// ui.push_screen_with_mode(root, ScreenMode::Modal);
/// let dims = matches!(ui.screen_mode(root), Some(ScreenMode::Modal));
/// assert!(dims);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ScreenMode {
    /// A menu, page or HUD-like screen: drawn as it is, no overlay. The
    /// default.
    #[default]
    Modeless,
    /// A dialog: draws a dim overlay over the screens below it, owns the
    /// pointer outside its own rectangle (a click there is swallowed) and is
    /// a dialog to assistive technology.
    Modal,
}

pub(super) struct Screen {
    pub root: UiId,
    pub modal: bool,
    pub(super) saved_focus: Option<UiId>,
}
#[derive(Clone, Copy)]
pub(super) enum Drag {
    Scroll(UiId, Vec2, Vec2),
    Bar(UiId, f32, f32),
    Text(UiId),
    /// A split's divider, held.
    Split(UiId),
    /// A draggable node pressed at a point, not yet moved far enough to drag.
    Item(UiId, Vec2),
}

/// The screen's UI: `ctx.ui`. An owned widget tree and screen stack: build
/// once, then mutate node properties. Draining events avoids callbacks
/// borrowing the game and UI together.
///
/// - **What lives here:** the retained widget tree with its screen stack,
///   focus and queued events; who has the pointer and the keyboard
///   ([`wants_pointer`](Self::wants_pointer),
///   [`wants_keyboard`](Self::wants_keyboard)); and the immediate widgets
///   for debug overlays, [`immediate`](Self::immediate).
/// - **When to call it:** build the tree in setup, then change text, values
///   and screens from any system. The engine lays it out and feeds it input
///   before the frame's systems, so [`events`](Self::events) queued this
///   frame are read in a frame or simulation system. Input a system feeds
///   (`ctx.input.feed`) reaches it before the next system runs. Immediate widgets are
///   drawn the frame they are called.
/// - **What it needs:** nothing to build and query (tests drive it with
///   [`frame`](Self::frame)); drawing needs a window. It borrows apart from
///   the world and the other services: read its events while playing
///   sounds or changing entities.
///
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
/// ui.push_modeless(root);
/// ui.frame(vec2(800.0,600.0), &ponko::ui::UiInput::default());
/// ```
pub struct Ui {
    pub(super) tree: TaffyTree<usize>,
    pub(super) nodes: Vec<Option<Node>>,
    /// Ids of removed nodes whose slots are used next, under a new
    /// generation (see [`SLOT_BITS`]).
    free_ids: Vec<UiId>,
    pub(super) screens: Vec<Screen>,
    /// The HUD roots ([`Ui::show_hud`]), drawn below every screen, bottom first.
    pub(super) huds: Vec<UiId>,
    pub(super) visible: Vec<UiId>,
    pub(super) events: std::collections::VecDeque<UiEvent>,
    /// The screens the player closed in this frame (each also sent a
    /// `ScreenPopped`), kept apart from `events`, which systems drain.
    closed: Vec<UiId>,
    /// IME composition ([`Ui::preedit`], [`Ui::ime_area`]).
    pub(super) ime: super::retained_ime::UiImeState,
    /// Which string tables the keyed nodes show ([`Ui::localize`]).
    pub(super) locale: super::retained_locale::UiLocaleState,
    pub(super) focus: Option<UiId>,
    pub(super) hovered: Option<UiId>,
    pub(super) pressed: Option<UiId>,
    pub(super) drag: Option<Drag>,
    /// The widget set's state: the open pop-up, the item being dragged, the
    /// key-binding field listening, the tooltip, the clock.
    pub(super) widgets: super::widgets::WidgetState,
    /// The animations: the UI clock, playing tracks, screens going off.
    pub(super) motion: super::anim::Motion,
    pub(super) scale: f32,
    /// Physical pixels per window pixel where the tree is drawn (the
    /// window's scale factor; 1 for a world panel). Text is measured at
    /// the size it is drawn, in physical pixels, so glyph widths rounded
    /// to whole pixels add up the same in layout and drawing.
    pub(super) pixels: f32,
    pub(super) viewport: Vec2,
    pub(super) pointer: Vec2,
    pub(super) measure: crate::text::Text,
    pub(super) external_fonts: bool,
    pub(super) consumed: bool,
    /// Requests from assistive technology, applied by the next [`Ui::frame`].
    pub(super) access_actions: Vec<super::UiAccessAction>,
    pub(super) theme: super::UiTheme,
    /// The fonts and images rich text names ([`Ui::set_inline_font`]).
    pub(super) rich_assets: super::rich::RichAssets,
    /// Scratch space for laying out rich text.
    pub(super) rich_layout: super::rich::RichLayout,
    /// What the last layout was computed for, and how many ran.
    pub(super) layout_cache: super::retained_layout::LayoutCache,
    /// The named nodes ([`Ui::find`]).
    pub(super) names: std::collections::HashMap<String, UiId>,
    /// The immediate widgets (a panel, a label, a button drawn the frame
    /// they are called) and their style. Only the screen's UI (`ctx.ui`)
    /// shows them; a `d3::UiPanel`'s tree leaves it unused.
    pub immediate: super::UiImmediate,
    /// The mouse is over the inspector overlay.
    pub(crate) overlay_has_pointer: bool,
    /// The ray the mouse uses on world panels instead of the camera's through it.
    #[cfg(feature = "3d")]
    pub(crate) pointer_ray: Option<(glam::Vec3, glam::Vec3)>,
    /// A second pass in the same frame, for input a system fed.
    pub(super) same_frame: super::same_frame::SameFrame,
}
// SAFETY: the only part that isn't `Send`/`Sync` is taffy's `CompactLength`,
// which may hold an opaque `calc()` handle as a raw pointer. The engine
// never turns on taffy's `calc` feature or makes such values, so every
// length is plain numbers; the rest of `Ui` is owned data with no interior
// mutability. That lets a `Ui` live in a component (`UiPanel3D`).
unsafe impl Send for Ui {}
unsafe impl Sync for Ui {}
impl Default for Ui {
    fn default() -> Self {
        Self {
            tree: TaffyTree::new(),
            nodes: Vec::new(),
            free_ids: Vec::new(),
            screens: Vec::new(),
            huds: Vec::new(),
            visible: Vec::new(),
            events: std::collections::VecDeque::new(),
            closed: Vec::new(),
            ime: Default::default(),
            locale: Default::default(),
            focus: None,
            hovered: None,
            pressed: None,
            drag: None,
            widgets: Default::default(),
            motion: Default::default(),
            scale: 1.0,
            pixels: 1.0,
            viewport: Vec2::ZERO,
            pointer: Vec2::ZERO,
            measure: crate::text::Text::new(),
            external_fonts: false,
            consumed: false,
            access_actions: Vec::new(),
            theme: super::UiTheme::default(),
            rich_assets: Default::default(),
            rich_layout: Default::default(),
            layout_cache: Default::default(),
            names: std::collections::HashMap::new(),
            immediate: super::UiImmediate::default(),
            overlay_has_pointer: false,
            #[cfg(feature = "3d")]
            pointer_ray: None,
            same_frame: Default::default(),
        }
    }
}
impl Ui {
    pub(super) fn node(&self, id: UiId) -> Option<&Node> {
        self.nodes.get(id.slot())?.as_ref().filter(|n| n.id == id)
    }
    pub(super) fn node_mut(&mut self, id: UiId) -> Option<&mut Node> {
        self.nodes.get_mut(id.slot())?.as_mut().filter(|n| n.id == id)
    }
    pub(super) fn paint_mut(&mut self, id: UiId) -> Option<&mut UiPaint> {
        Some(&mut self.node_mut(id)?.paint)
    }

    /// Add a detached node. Push it as a screen or use `add_child` instead.
    /// Its paint comes from the theme ([`Ui::set_theme`]). [`Ui::root`] and
    /// the other builders do the same in one expression.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ```
    pub fn add(&mut self, widget: UiWidget, layout: UiLayout) -> UiId {
        let id = match self.free_ids.pop() {
            Some(freed) => UiId(freed.0 + (1 << SLOT_BITS)),
            None => {
                assert!(
                    self.nodes.len() < SLOT_MASK,
                    "ponko: more than {SLOT_MASK} UI nodes; remove nodes you no longer show"
                );
                self.nodes.push(None);
                UiId(self.nodes.len() - 1)
            }
        };
        let layout = self
            .tree
            .new_leaf_with_context(layout.into_unstable_style(), id.slot())
            .expect("valid UI layout");
        let paint = self.theme.paint(widget);
        self.nodes[id.slot()] = Some(Node {
            id,
            widget,
            layout,
            parent: None,
            children: Vec::new(),
            text: String::new(),
            paint,
            enabled: true,
            rect: Rect::default(),
            clip: Rect::default(),
            scroll: Vec2::ZERO,
            max_scroll: Vec2::ZERO,
            value: 0.0,
            caret: 0,
            anchor: 0,
            access_name: String::new(),
            access_description: String::new(),
            access_live: super::UiLive::Off,
            name: String::new(),
            title: false,
            initial_focus: false,
            closes_screen: false,
            ime: true,
            preedit: String::new(),
            preedit_cursor: None,
            key: None,
            rich: None,
            extra: None,
            motion: Default::default(),
            measured: Default::default(),
        });
        id
    }
    /// Add a child.
    ///
    /// # Panics
    ///
    /// If the parent was removed or belongs to another UI.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// let child = ui.add_child(root, ponko::ui::UiWidget::Label, ponko::ui::UiLayout::auto());
    /// ```
    pub fn add_child(&mut self, parent: UiId, widget: UiWidget, layout: UiLayout) -> UiId {
        let p = self
            .node(parent)
            .expect("UI parent is freed or absent; add a live parent in this UI first")
            .layout;
        let id = self.add(widget, layout);
        let node = self.node_mut(id).unwrap();
        node.parent = Some(parent);
        let layout = node.layout;
        self.tree.add_child(p, layout).expect("valid UI child");
        self.node_mut(parent).unwrap().children.push(id);
        id
    }
    /// Remove a subtree. Stale IDs stay invalid; removing a screen drops it.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.remove(id);
    /// assert!(ui.rect(id).is_none());
    /// ```
    pub fn remove(&mut self, id: UiId) {
        while let Some(child) = self.node(id).and_then(|n| n.children.last().copied()) {
            self.remove(child);
        }
        if let Some(n) = self.node(id).is_some().then(|| self.nodes[id.slot()].take()).flatten() {
            if (id.0 >> SLOT_BITS) + 1 < LAST_GENERATION {
                self.free_ids.push(id);
            }
            if let Some(parent) = n.parent.and_then(|p| self.node_mut(p)) {
                parent.children.retain(|&c| c != id);
            }
            let _ = self.tree.remove(n.layout);
            if self.names.get(&n.name) == Some(&id) {
                self.names.remove(&n.name);
            }
        }
        self.screens.retain(|s| s.root != id);
        self.huds.retain(|&h| h != id);
        self.visible.retain(|&n| n != id);
        if self.drag.is_some_and(|d| match d {
            Drag::Scroll(n, _, _) | Drag::Bar(n, _, _) | Drag::Text(n) | Drag::Split(n) | Drag::Item(n, _) => n == id,
        }) {
            self.drag = None;
        }
        if self.focus == Some(id) {
            self.focus = None;
        }
        if self.pressed == Some(id) {
            self.pressed = None;
            self.drag = None;
        }
        self.events.retain(|e| match e {
            UiEvent::Clicked(n)
            | UiEvent::Changed(n)
            | UiEvent::Submitted(n)
            | UiEvent::ScreenPopped(n)
            | UiEvent::LinkClicked(n, _)
            | UiEvent::Chosen(n, _)
            | UiEvent::RowsShown(n)
            | UiEvent::AnimationDone(n) => *n != id,
            UiEvent::Dropped(a, b) => *a != id && *b != id,
        });
        self.widget_removed(id);
        self.motion_removed(id);
    }
    /// Replace the node's layout (flex, grid, absolute positioning and
    /// constraints). [`Ui::node_layout`] reads it back.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.set_layout(id, ponko::ui::UiLayout::fixed(100.0, 40.0));
    /// ```
    pub fn set_layout(&mut self, id: UiId, layout: UiLayout) {
        if let Some(n) = self.node(id) {
            let style = layout.into_unstable_style();
            // The same layout again leaves the node's layout standing.
            if self.tree.style(n.layout).is_ok_and(|old| *old == style) {
                return;
            }
            let _ = self.tree.set_style(n.layout, style);
        }
    }
    /// The node's current layout, or `None` for a removed node.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::fill());
    /// assert_eq!(ui.node_layout(id), Some(ponko::ui::UiLayout::fill()));
    /// ```
    pub fn node_layout(&self, id: UiId) -> Option<UiLayout> {
        let style = self.tree.style(self.node(id)?.layout).ok()?;
        Some(UiLayout::unstable_custom(style.clone()))
    }
    /// Replace visual style.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.set_paint(id, ponko::ui::UiPaint::default());
    /// ```
    pub fn set_paint(&mut self, id: UiId, paint: UiPaint) {
        if let Some(n) = self.node_mut(id) {
            n.paint = paint;
        }
    }
    /// Set label or input text. Input control characters are discarded.
    /// A node that held a string table key ([`Ui::set_text_key`]) lets go of it.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Label, ponko::ui::UiLayout::auto());
    /// ui.set_text(id, "Shop"); assert_eq!(ui.text(id), Some("Shop"));
    /// ```
    pub fn set_text(&mut self, id: UiId, text: impl AsRef<str>) {
        if let Some(n) = self.node_mut(id) {
            n.key = None;
            n.put_text(text.as_ref());
        }
        self.parse_rich_node(id);
    }
    /// Read text, or `None` for a stale ID.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Label, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.text(id), Some(""));
    /// ```
    pub fn text(&self, id: UiId) -> Option<&str> {
        Some(&self.node(id)?.text)
    }
    /// Enable/disable a node and all descendant interaction.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Button, ponko::ui::UiLayout::auto());
    /// ui.set_enabled(id, false);
    /// ```
    pub fn set_enabled(&mut self, id: UiId, enabled: bool) {
        if let Some(n) = self.node_mut(id) {
            n.enabled = enabled;
        }
    }
    /// Slider value (0..1), or toggle value (zero/off, one/on).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Slider, ponko::ui::UiLayout::auto());
    /// ui.set_value(id, 0.5); assert_eq!(ui.value(id), Some(0.5));
    /// ```
    pub fn set_value(&mut self, id: UiId, value: f32) {
        if let Some(n) = self.node_mut(id) {
            n.value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        }
    }
    /// Read the current slider/toggle value.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Toggle, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.value(id), Some(0.0));
    /// ```
    pub fn value(&self, id: UiId) -> Option<f32> {
        Some(self.node(id)?.value)
    }
    /// Set scroll offset in unscaled layout units; clamped during layout.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Scroll, ponko::ui::UiLayout::auto());
    /// ui.set_scroll(id, vec2(0.0,40.0));
    /// ```
    pub fn set_scroll(&mut self, id: UiId, offset: Vec2) {
        if let Some(n) = self.node_mut(id) {
            n.scroll = offset.max(Vec2::ZERO);
        }
    }
    /// Current clamped offset, after layout.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Scroll, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.scroll(id), Some(Vec2::ZERO));
    /// ```
    pub fn scroll(&self, id: UiId) -> Option<Vec2> {
        Some(self.node(id)?.scroll)
    }
    /// Push a detached root as the new top screen, in `mode`: only the top
    /// screen is interactive, and a [`ScreenMode::Modal`] one also draws a
    /// dim overlay below itself, owns the pointer outside its rectangle and
    /// is a dialog to assistive technology. A detached root that has a
    /// parent, is already on the stack or is shown as a [HUD](Self::show_hud)
    /// panics.
    ///
    /// # Panics
    ///
    /// If `root` has a parent (or was removed), is already a screen, or is
    /// shown as a [HUD](Self::show_hud).
    ///
    /// The screen opens with its [initial focus](Self::set_initial_focus)
    /// (the builder's [`focused`](super::UiBuilder::focused)) focused, every
    /// time it is pushed; without one, nothing is focused until Tab, an
    /// arrow key or [`set_focus`](Self::set_focus).
    ///
    /// [`push_modal`](Self::push_modal) and
    /// [`push_modeless`](Self::push_modeless) say the same in fewer words.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{ScreenMode, UiLayout, UiWidget};
    /// let mut ui = ponko::ui::Ui::default();
    /// let menu = ui.add(UiWidget::Panel, UiLayout::fill());
    /// let dialog = ui.add(UiWidget::Panel, UiLayout::centered(300.0, 120.0));
    /// ui.push_screen_with_mode(menu, ScreenMode::Modeless);
    /// ui.push_screen_with_mode(dialog, ScreenMode::Modal);
    /// assert_eq!(ui.screen_mode(dialog), Some(ScreenMode::Modal));
    /// assert_eq!(ui.screen_mode(menu), Some(ScreenMode::Modeless));
    /// ```
    pub fn push_screen_with_mode(&mut self, root: UiId, mode: ScreenMode) {
        assert!(
            self.node(root).is_some_and(|n| n.parent.is_none()),
            "UI screen must be a detached root; add a root node or detach it before pushing"
        );
        assert!(
            !self.screens.iter().any(|s| s.root == root),
            "UI screen already on stack; pop it before pushing again"
        );
        assert!(
            !self.huds.contains(&root),
            "UI screen is shown as a HUD; hide the HUD before pushing it"
        );
        self.screens.push(Screen {
            root,
            modal: mode == ScreenMode::Modal,
            saved_focus: self.focus.take(),
        });
        self.pressed = None;
        self.drag = None;
        self.hovered = None;
        self.focus = self.initial_focus_in(root);
        self.screen_entered(root);
    }
    /// Push `root` as a [modal](ScreenMode::Modal) screen: a dialog that
    /// dims what is below it and keeps the input to itself. Short for
    /// [`push_screen_with_mode`](Self::push_screen_with_mode)`(root, ScreenMode::Modal)`.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.push_modal(root);
    /// assert_eq!(ui.screen_mode(root), Some(ponko::ui::ScreenMode::Modal));
    /// ```
    pub fn push_modal(&mut self, root: UiId) {
        self.push_screen_with_mode(root, ScreenMode::Modal);
    }
    /// Push `root` as a [modeless](ScreenMode::Modeless) screen: a menu or
    /// page drawn as it is, with no overlay. Short for
    /// [`push_screen_with_mode`](Self::push_screen_with_mode)`(root, ScreenMode::Modeless)`.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.push_modeless(root);
    /// assert_eq!(ui.top_screen(), Some(root));
    /// ```
    pub fn push_modeless(&mut self, root: UiId) {
        self.push_screen_with_mode(root, ScreenMode::Modeless);
    }
    /// The old form of [`push_screen_with_mode`](Self::push_screen_with_mode):
    /// `true` is [`ScreenMode::Modal`], `false` [`ScreenMode::Modeless`].
    #[deprecated(
        since = "0.2.0",
        note = "use `push_modeless(root)`, `push_modal(root)` or `push_screen_with_mode(root, mode)`"
    )]
    pub fn push_screen(&mut self, root: UiId, modal: bool) {
        self.push_screen_with_mode(root, if modal { ScreenMode::Modal } else { ScreenMode::Modeless });
    }
    /// The mode `root` was pushed with, or `None` when it is not on the
    /// screen stack.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.screen_mode(root), None);
    /// ui.push_modeless(root);
    /// assert_eq!(ui.screen_mode(root), Some(ponko::ui::ScreenMode::Modeless));
    /// ```
    pub fn screen_mode(&self, root: UiId) -> Option<ScreenMode> {
        let screen = self.screens.iter().find(|s| s.root == root)?;
        Some(if screen.modal {
            ScreenMode::Modal
        } else {
            ScreenMode::Modeless
        })
    }
    /// The first node under `root`, depth-first, marked as its screen's
    /// initial focus that can take the focus now.
    fn initial_focus_in(&self, root: UiId) -> Option<UiId> {
        let n = self.node(root)?;
        if n.initial_focus && self.can_focus(root) {
            return Some(root);
        }
        n.children.iter().find_map(|&child| self.initial_focus_in(child))
    }
    /// Moves the keyboard and gamepad focus to `id`, as Tab or a click
    /// would: Enter (or a gamepad's South button) then activates it. Returns
    /// whether it took. Only a button, text input, toggle or slider can
    /// have the focus, and only while it is enabled, [shown](Self::is_shown)
    /// and on the top screen; anything else leaves the focus as it is and
    /// returns `false`. A node with no size after the next layout loses it
    /// again.
    ///
    /// For the node a screen should open on every time it is pushed, mark it
    /// instead ([`set_initial_focus`](Self::set_initial_focus), or the
    /// builder's [`focused`](super::UiBuilder::focused)).
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiLayout};
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let play = ui.button(root, "Play").layout(UiLayout::row(40.0)).id();
    /// let quit = ui.button(root, "Quit").layout(UiLayout::row(40.0)).id();
    /// ui.push_modeless(root);
    /// assert!(ui.set_focus(quit));
    /// ui.frame(vec2(800.0, 600.0), &UiInput { confirm: true, ..Default::default() });
    /// assert_eq!(ui.events().filter_map(ponko::ui::UiEvent::clicked).last(), Some(quit));
    /// assert!(!ui.set_focus(root), "a panel can't have the focus");
    /// # let _ = play;
    /// ```
    pub fn set_focus(&mut self, id: UiId) -> bool {
        let ok = self.can_focus(id);
        if ok {
            self.focus = Some(id);
        }
        ok
    }
    /// Takes the focus away: no node has it until Tab, an arrow key, a click
    /// or [`set_focus`](Self::set_focus).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let ok = ui.button(root, "OK").focused().id();
    /// ui.push_modeless(root);
    /// assert_eq!(ui.focus(), Some(ok));
    /// ui.clear_focus();
    /// assert_eq!(ui.focus(), None);
    /// ```
    pub fn clear_focus(&mut self) {
        self.focus = None;
    }
    /// Marks `id` as the node its screen opens on (or unmarks it): every
    /// [push](Self::push_screen_with_mode) of the screen focuses it, so a
    /// keyboard or gamepad player can press Enter at once, and a menu that
    /// is opened again starts on it again. If several nodes of a screen are
    /// marked, the first one (depth-first) that can take the focus wins. The
    /// builder's [`focused`](super::UiBuilder::focused) does the same.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let play = ui.button(root, "Play").id();
    /// ui.set_initial_focus(play, true);
    /// assert!(ui.is_initial_focus(play));
    /// ui.push_modeless(root);
    /// assert_eq!(ui.focus(), Some(play));
    /// ```
    pub fn set_initial_focus(&mut self, id: UiId, on: bool) {
        if let Some(n) = self.node_mut(id) {
            n.initial_focus = on;
        }
    }
    /// Whether `id` is marked as the node its screen opens on
    /// ([`set_initial_focus`](Self::set_initial_focus)).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let play = ui.button(root, "Play").focused().id();
    /// assert!(ui.is_initial_focus(play));
    /// assert!(!ui.is_initial_focus(root));
    /// ```
    pub fn is_initial_focus(&self, id: UiId) -> bool {
        self.node(id).is_some_and(|n| n.initial_focus)
    }
    /// Makes pressing button `id` also close its screen (or stops it): a
    /// Back, Cancel, Close or Resume button. A click or Enter on it sends
    /// its [`Clicked`](UiEvent::Clicked) and then pops its screen as
    /// Escape would, with a [`ScreenPopped`](UiEvent::ScreenPopped), so a
    /// game that handles Escape handles the button too. Only while its
    /// screen is the top one, as only that one takes input. The builder's
    /// [`closes_screen`](super::UiBuilder::closes_screen) does the same.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiEvent, UiInput};
    /// let mut ui = ponko::ui::Ui::default();
    /// let dialog = ui.root().id();
    /// let back = ui.button(dialog, "Back").focused().id();
    /// ui.set_closes_screen(back, true);
    /// assert!(ui.closes_screen(back));
    /// ui.push_modal(dialog);
    /// ui.frame(vec2(800.0, 600.0), &UiInput { confirm: true, ..Default::default() });
    /// let events: Vec<UiEvent> = ui.events().collect();
    /// assert_eq!(events, [UiEvent::Clicked(back), UiEvent::ScreenPopped(dialog)]);
    /// assert!(!ui.is_screen(dialog));
    /// ```
    pub fn set_closes_screen(&mut self, id: UiId, on: bool) {
        if let Some(n) = self.node_mut(id) {
            n.closes_screen = on;
        }
    }
    /// Whether pressing `id` closes its screen
    /// ([`set_closes_screen`](Self::set_closes_screen)).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let ok = ui.button(root, "OK").id();
    /// assert!(!ui.closes_screen(ok));
    /// ```
    pub fn closes_screen(&self, id: UiId) -> bool {
        self.node(id).is_some_and(|n| n.closes_screen)
    }
    /// Whether `id` may take the focus now, before layout gives it a size.
    pub(super) fn can_focus(&self, id: UiId) -> bool {
        self.interactive(id) && self.is_shown(id) && self.node(id).is_some_and(|n| self.takes_focus(n))
    }
    /// Shows the tree under a detached `root` as a HUD: display-only UI,
    /// such as a score or a health bar, drawn below every screen and kept
    /// up while screens open and close above it. Escape (or a gamepad's
    /// East button) never closes it, its nodes never take the pointer or the
    /// focus, and it doesn't make [`wants_pointer`](Self::wants_pointer) or
    /// [`wants_keyboard`](Self::wants_keyboard) true, so the game keeps its
    /// keys. Anything the player clicks belongs on a screen
    /// ([`push_modeless`](Self::push_modeless)). Several HUDs are drawn in the
    /// order they were shown; showing one again changes nothing.
    ///
    /// # Panics
    ///
    /// Like [`push_screen_with_mode`](Self::push_screen_with_mode), for a node
    /// with a parent, or one on the screen stack.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiLayout};
    /// let mut ui = ponko::ui::Ui::default();
    /// let hud = ui.root().layout(UiLayout::fill().padding(16.0)).id();
    /// let coins = ui.label(hud, "Coins: 0").id();
    /// ui.show_hud(hud);
    /// assert!(ui.is_shown(coins) && !ui.wants_keyboard());
    /// ui.frame(vec2(800.0, 600.0), &UiInput { cancel: true, ..Default::default() });
    /// assert!(ui.is_shown(coins), "Escape leaves a HUD up");
    /// ```
    pub fn show_hud(&mut self, root: UiId) {
        assert!(
            self.node(root).is_some_and(|n| n.parent.is_none()),
            "UI HUD must be a detached root; add a root node or detach it first"
        );
        assert!(
            !self.is_screen(root),
            "UI HUD is already a screen; pop it before showing it as a HUD"
        );
        if !self.huds.contains(&root) {
            self.huds.push(root);
        }
        // A screen still going off stops going: it is the HUD now.
        self.screen_shown_as_hud(root);
    }
    /// Takes a HUD ([`show_hud`](Self::show_hud)) off the screen. Its nodes
    /// stay, so it can be shown again. Does nothing for a node that isn't a
    /// HUD.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let hud = ui.root().id();
    /// ui.show_hud(hud);
    /// ui.hide_hud(hud);
    /// assert!(!ui.is_hud(hud) && !ui.is_shown(hud));
    /// ```
    pub fn hide_hud(&mut self, root: UiId) {
        self.huds.retain(|&h| h != root);
    }
    /// Whether `id` is a HUD's root ([`show_hud`](Self::show_hud)) right now.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let hud = ui.root().id();
    /// assert!(!ui.is_hud(hud));
    /// ui.show_hud(hud);
    /// assert!(ui.is_hud(hud));
    /// ```
    pub fn is_hud(&self, id: UiId) -> bool {
        self.huds.contains(&id)
    }
    /// Pop a screen, restoring the previous focus. Nodes remain reusable.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// ui.push_modal(root); assert_eq!(ui.pop_screen(), Some(root));
    /// ```
    pub fn pop_screen(&mut self) -> Option<UiId> {
        let screen = self.screens.pop()?;
        self.consumed = true;
        self.focus = screen.saved_focus.filter(|&id| self.interactive(id));
        self.pressed = None;
        self.drag = None;
        self.hovered = None;
        self.screen_left(screen.root, screen.modal);
        Some(screen.root)
    }
    /// The node's parent, or `None` for a detached root and for stale IDs.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// let child = ui.add_child(root, ponko::ui::UiWidget::Label, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.parent(child), Some(root));
    /// assert_eq!(ui.parent(root), None);
    /// ```
    pub fn parent(&self, id: UiId) -> Option<UiId> {
        self.node(id)?.parent
    }
    /// Whether `id` is on the screen stack right now.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// assert!(!ui.is_screen(root));
    /// ui.push_modeless(root);
    /// assert!(ui.is_screen(root));
    /// ```
    pub fn is_screen(&self, id: UiId) -> bool {
        self.screens.iter().any(|s| s.root == id)
    }
    /// Whether the node is shown: it belongs to a screen on the stack (a
    /// modal above may cover it) or a [HUD](Self::show_hud), and neither it nor a node above it is
    /// [hidden](super::UiLayout::hidden). A tree built but not pushed, or a
    /// popped screen, is not shown. Stale ids are not shown.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let ok = ui.button(root, "OK").id();
    /// assert!(!ui.is_shown(ok), "built, not pushed");
    /// ui.push_modeless(root);
    /// assert!(ui.is_shown(ok));
    /// ui.set_layout(ok, ponko::ui::UiLayout::auto().hidden());
    /// assert!(!ui.is_shown(ok));
    /// ```
    pub fn is_shown(&self, id: UiId) -> bool {
        let mut at = id;
        loop {
            let Some(n) = self.node(at) else { return false };
            if self
                .tree
                .style(n.layout)
                .is_ok_and(|s| s.display == taffy::Display::None)
            {
                return false;
            }
            match n.parent {
                Some(parent) => at = parent,
                None => return self.is_screen(at) || self.is_hud(at),
            }
        }
    }
    /// Top screen root.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::ui::Ui::default().top_screen(), None);
    /// ```
    pub fn top_screen(&self) -> Option<UiId> {
        self.screens.last().map(|s| s.root)
    }
    /// User scale, multiplied with the window's existing DPI handling. The
    /// [immediate widgets](Ui::immediate) are drawn at it too: their sizes
    /// are in the same units as the tree's layout.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); ui.set_scale(1.5);
    /// assert_eq!(ui.immediate.scale(), 1.5);
    /// ```
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = if scale.is_finite() { scale.clamp(0.25, 8.0) } else { 1.0 };
        self.immediate.scale = self.scale;
    }
    /// The node's rectangle in logical window coordinates (including user
    /// scale) as of the last layout: [`Ui::frame`] (run by the engine before
    /// the frame's systems) or [`Ui::layout`]. It is a zero rectangle until the
    /// first layout, and changes made since then show only after the next
    /// one. `None` for an id not in the tree.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); let id = ui.add(ponko::ui::UiWidget::Panel, ponko::ui::UiLayout::auto());
    /// assert_eq!(ui.rect(id), Some(Rect::default()), "no layout yet");
    /// ```
    pub fn rect(&self, id: UiId) -> Option<Rect> {
        Some(self.node(id)?.rect)
    }
    /// Current keyboard/gamepad focus.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::ui::Ui::default().focus(), None);
    /// ```
    pub fn focus(&self) -> Option<UiId> {
        self.focus
    }
    /// Takes the queued semantic events, oldest first. Events left untaken
    /// (stopping the loop early) stay queued until the next frame clears
    /// them. To change the tree while reading, take them one at a time with
    /// [`next_event`](Self::next_event). For a screen that reacts only to
    /// clicks:
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::UiEvent;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().layout(ponko::ui::UiLayout::fill()).id();
    /// let ok = ui.button(root, "OK").layout(ponko::ui::UiLayout::fixed(100.0, 40.0)).id();
    /// ui.push_modeless(root);
    /// let at = |pressed, held, released| ponko::ui::UiInput {
    ///     pointer: vec2(50.0, 20.0),
    ///     pressed,
    ///     held,
    ///     released,
    ///     ..Default::default()
    /// };
    /// ui.frame(vec2(800.0, 600.0), &at(true, true, false));
    /// ui.frame(vec2(800.0, 600.0), &at(false, false, true));
    /// let clicked = ui.events().filter_map(UiEvent::clicked).last();
    /// assert_eq!(clicked, Some(ok));
    /// assert_eq!(ui.events().count(), 0);
    /// ```
    pub fn events(&mut self) -> impl Iterator<Item = UiEvent> + '_ {
        std::iter::from_fn(move || self.events.pop_front())
    }
    /// Takes the oldest queued event, or `None` when none are left: what
    /// [`events`](Self::events) gives one at a time, for a loop whose body
    /// changes the tree (`while let Some(e) = ui.next_event() { ... }`).
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::ui::Ui::default().next_event(), None);
    /// ```
    pub fn next_event(&mut self) -> Option<UiEvent> {
        self.events.pop_front()
    }
    /// The pointer belongs to UI: it is over a widget of the tree or an
    /// [immediate](Self::immediate) one drawn this frame or the previous
    /// one, a drag is captured, or a modal screen is open (it captures
    /// outside too). Check it before reacting to clicks in the world, so
    /// clicking a button doesn't also select a unit behind it.
    ///
    /// The pointer is where the mouse was when the running system started.
    /// ```
    /// use ponko::prelude::*;
    /// assert!(!ponko::ui::Ui::default().wants_pointer());
    /// # fn select(ctx: &mut Ctx) {
    /// if ctx.input.mouse_pressed(MouseButton::Left) && !ctx.ui.wants_pointer() {
    ///     let at = ctx.mouse_world();
    ///     # let _ = at;
    /// }
    /// # }
    /// ```
    pub fn wants_pointer(&self) -> bool {
        self.consumed
            || self.pressed.is_some()
            || self.drag.is_some()
            || self.hit_test(self.pointer).is_some()
            || self.screens.last().is_some_and(|s| s.modal)
            || self.overlay_has_pointer
            || self.immediate.covers(self.pointer)
    }
    /// Engine side: where the mouse is now, between frames of the tree.
    pub(crate) fn set_pointer(&mut self, at: Vec2) {
        self.pointer = at;
    }
    /// Sets the ray the mouse uses on [`UiPanel3D`](crate::d3::UiPanel)s this
    /// frame and later (origin and direction, in the world), instead of the
    /// camera's ray through the mouse: for a locked cursor, use the camera's
    /// forward ray; for VR, a controller's. `None` goes back to the mouse.
    /// Only the screen's UI (`ctx.ui`) reads it.
    /// ```
    /// # use ponko::prelude::*;
    /// # fn update(ctx: &mut Ctx) {
    /// // Aim with the screen's center while the cursor is locked.
    /// let (origin, forward) = (ctx.scene3d.camera.position, ctx.scene3d.camera.forward());
    /// ctx.ui.set_pointer_ray(Some((origin, forward)));
    /// # }
    /// ```
    #[cfg(feature = "3d")]
    pub fn set_pointer_ray(&mut self, ray: Option<(glam::Vec3, glam::Vec3)>) {
        self.pointer_ray = ray;
    }
    /// Keyboard/gamepad belongs to UI while a screen is open (including text
    /// focus); a [HUD](Self::show_hud) doesn't count. Check it before the game's own movement and actions, just as
    /// [`wants_pointer`](Self::wants_pointer) guards the mouse.
    /// ```
    /// use ponko::prelude::*;
    /// assert!(!ponko::ui::Ui::default().wants_keyboard());
    /// # fn update(ctx: &mut Ctx) {
    /// if !ctx.ui.wants_keyboard() && ctx.input.key_pressed(Key::Space) { /* jump */ }
    /// # }
    /// ```
    pub fn wants_keyboard(&self) -> bool {
        self.consumed || !self.screens.is_empty()
    }
    pub(super) fn interactive(&self, id: UiId) -> bool {
        let mut at = Some(id);
        while let Some(id) = at {
            let Some(n) = self.node(id) else { return false };
            if !n.enabled {
                return false;
            }
            if Some(id) == self.top_screen() {
                return true;
            }
            at = n.parent;
        }
        false
    }
    pub(super) fn focusable(&self, id: UiId) -> bool {
        self.interactive(id)
            && self
                .node(id)
                .is_some_and(|n| self.takes_focus(n) && n.rect.size().cmpgt(Vec2::ZERO).all())
    }
    /// Hit test the top screen, respecting ancestor clips and disabled nodes.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::ui::Ui::default().hit_test(Vec2::ZERO), None);
    /// ```
    pub fn hit_test(&self, point: Vec2) -> Option<UiId> {
        self.visible.iter().rev().copied().find(|&id| {
            self.interactive(id)
                && self.node(id).is_some_and(|n| {
                    n.rect.size().cmpgt(Vec2::ZERO).all()
                        && n.rect.contains(point)
                        && n.clip.contains(point)
                        && n.clip.size().cmpgt(Vec2::ZERO).all()
                })
        })
    }
    /// Resolve all screens against the logical window size. Percent sizes,
    /// min/max, flex/grid and absolute offsets are Taffy styles. Only what
    /// changed since the last layout is laid out again; with nothing
    /// changed, nodes are only placed where scrolling and animations put
    /// them ([`Ui::layout_passes`]).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); ui.layout(vec2(800.0,600.0));
    /// ```
    pub fn layout(&mut self, viewport: Vec2) {
        self.viewport = viewport.max(Vec2::ZERO);
        // State transitions start from what was shown when a state changed.
        self.update_states();
        // Split panes take their divider's place before Taffy runs.
        self.sync_splits();
        self.compute_layout(viewport);
        // A list that shows other items than its rows held is laid out again.
        if self.sync_lists() {
            self.compute_layout(viewport);
        }
        if self.focus.is_some_and(|id| !self.focusable(id)) {
            self.focus = None;
        }
    }
    /// Taffy's layout of the roots where something changed since the last
    /// one (`retained_layout`), then each node's rectangle and clip.
    fn compute_layout(&mut self, viewport: Vec2) {
        self.parse_rich();
        // Taffy lays out again only what changed: a style or a child (it
        // sees those), a node's text measure (marked here), or everything
        // for another window size, scale or font set.
        let key = super::retained_layout::LayoutKey {
            viewport,
            scale: self.scale,
            pixels: self.pixels,
            external_fonts: self.external_fonts,
            fonts: self.measure.revision(),
        };
        if self.mark_layout_changes(key) {
            self.layout_cache.passes += 1;
            self.run_taffy(viewport);
        }
        // Placing is a cheap walk: it runs every time, so scrolling and
        // animations move nodes without a layout.
        self.place_roots(viewport);
    }
    /// Taffy's layout of every shown root with something dirty, text
    /// measured with the engine's font layout (headlessly too).
    fn run_taffy(&mut self, viewport: Vec2) {
        let nodes = &self.nodes;
        let text = &mut self.measure;
        let scratch = &mut self.rich_layout;
        let external_fonts = self.external_fonts;
        // Physical pixels per layout unit: text is measured at the size
        // it is drawn.
        let k = self.scale * self.pixels;
        // A screen going off keeps the layout it had on the stack: it costs
        // no layout work while it goes.
        let roots = self.huds.iter().chain(self.screens.iter().map(|s| &s.root));
        for root in roots {
            let layout = nodes[root.slot()].as_ref().unwrap().layout;
            if !self.tree.dirty(layout).unwrap_or(true) {
                continue;
            }
            let _ = self.tree.compute_layout_with_measure(
                layout,
                Size {
                    width: AvailableSpace::Definite(viewport.x / self.scale),
                    height: AvailableSpace::Definite(viewport.y / self.scale),
                },
                |inputs, _, context, style| {
                    let Some(n) = context.and_then(|index| nodes[*index].as_ref()) else {
                        return taffy::tree::LayoutOutput::HIDDEN;
                    };
                    taffy::compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, available| {
                            if let Some(rich) = n.rich() {
                                return rich_size(rich, n, (external_fonts, k), known, available, text, scratch);
                            }
                            let size = text.layout(
                                &n.text,
                                if external_fonts { n.paint.font } else { Font::DEFAULT },
                                n.paint.font_size * k,
                                false,
                            ) / k;
                            let font = if external_fonts { n.paint.font } else { Font::DEFAULT };
                            let width = known.width.unwrap_or_else(|| match available.width {
                                AvailableSpace::Definite(w) if n.paint.wrap => size.x.min(w.max(0.0)),
                                AvailableSpace::MinContent if n.paint.wrap => {
                                    text.min_content_width(&n.text, font, n.paint.font_size * k) / k
                                }
                                _ => size.x,
                            });
                            let mut height = 0.0;
                            for paragraph in n.text.split('\n') {
                                let mut rest = paragraph;
                                loop {
                                    let end = if n.paint.wrap {
                                        wrap_end(text, rest, font, n.paint.font_size * k, width * k)
                                    } else {
                                        rest.len()
                                    };
                                    height += text.layout(&rest[..end], font, n.paint.font_size * k, false).y / k;
                                    if end == rest.len() {
                                        break;
                                    }
                                    rest = rest[end..].trim_start();
                                }
                            }
                            if n.text.is_empty() && !super::widgets::keeps_line(n.widget) {
                                height = 0.0;
                            }
                            Size {
                                width,
                                height: known.height.unwrap_or(height),
                            }
                        },
                    )
                },
            );
        }
    }
    /// Each shown node's rectangle and clip from Taffy's layout, where
    /// scrolling and animations put it.
    fn place_roots(&mut self, viewport: Vec2) {
        self.visible.clear();
        // HUDs first: `visible` is drawing order, so they are under the
        // screens. A screen going off is drawn where it was on the stack.
        let window = Rect::new(0.0, 0.0, viewport.x, viewport.y);
        for i in 0..self.huds.len() {
            let root = self.huds[i];
            self.place(root, self.root_offset(root), window, (Xf::IDENTITY, window));
        }
        let screens = self.screens.len();
        for depth in 0..=screens {
            for e in 0..self.motion.exiting.len() {
                let going = self.motion.exiting[e];
                if going.depth.min(screens) == depth {
                    let root = going.root;
                    self.place(root, self.root_offset(root), window, (Xf::IDENTITY, window));
                }
            }
            if depth < screens {
                let root = self.screens[depth].root;
                self.place(root, self.root_offset(root), window, (Xf::IDENTITY, window));
            }
        }
    }
    /// Where a screen root's `auto` margins move it: they take the window's
    /// free space, as they do for a child of a flex container (both sides
    /// `auto` centers it, one side pushes it to the other edge). Taffy gives
    /// a root's auto margins nothing, because a root has no container. Roots
    /// out of the flow are placed by their insets instead.
    fn root_offset(&self, root: UiId) -> Vec2 {
        let Some(node) = self.node(root) else { return Vec2::ZERO };
        let (Ok(style), Ok(layout)) = (self.tree.style(node.layout), self.tree.layout(node.layout)) else {
            return Vec2::ZERO;
        };
        if matches!(style.position, Position::Absolute | Position::Fixed) {
            return Vec2::ZERO;
        }
        let (margin, own) = (&style.margin, layout.margin);
        let share = |start: bool, end: bool, free: f32| match (start, end) {
            (true, true) => free * 0.5,
            (true, false) => free,
            _ => 0.0,
        };
        let free = self.viewport / self.scale
            - vec2(
                layout.size.width + own.left + own.right,
                layout.size.height + own.top + own.bottom,
            );
        let free = free.max(Vec2::ZERO);
        vec2(
            share(margin.left.is_auto(), margin.right.is_auto(), free.x),
            share(margin.top.is_auto(), margin.bottom.is_auto(), free.y),
        ) * self.scale
    }
    /// Places node `id` at `origin` inside `clip`, and its subtree;
    /// `shown` is its parent's map to the screen and its clip there.
    fn place(&mut self, id: UiId, origin: Vec2, clip: Rect, shown: (Xf, Rect)) {
        let layout = *self.tree.layout(self.node(id).unwrap().layout).unwrap();
        let scale = self.scale;
        let pos = origin + vec2(layout.location.x, layout.location.y) * scale;
        let size = vec2(layout.size.width, layout.size.height) * scale;
        let rect = Rect {
            min: pos,
            max: pos + size,
        };
        let xf = self.node_xf(id, shown.0, rect);
        let n = self.node_mut(id).unwrap();
        n.rect = rect;
        n.clip = clip;
        n.motion.xf = xf;
        n.motion.shown_clip = shown.1;
        let scrollable = super::widgets::scrolls(n.widget);
        n.max_scroll = if n.widget == UiWidget::Scroll {
            vec2(layout.scroll_width(), layout.scroll_height())
        } else if scrollable {
            // A list scrolls over all its items, not over the rows it keeps.
            super::widgets::list_max_scroll(n, size / scale)
        } else {
            Vec2::ZERO
        };
        n.scroll = n.scroll.clamp(Vec2::ZERO, n.max_scroll);
        let scroll = n.scroll * scale;
        self.visible.push(id);
        let (child_clip, child_shown) = if scrollable {
            let mut inner = rect;
            if self.node(id).unwrap().max_scroll.y > 0.0 {
                inner.max.x = (inner.max.x - 12.0 * scale).max(inner.min.x);
            }
            (intersect(clip, inner), intersect(shown.1, xf.map_rect(inner)))
        } else {
            (clip, shown.1)
        };
        let count = self.node(id).unwrap().children.len();
        for i in 0..count {
            let child = self.node(id).unwrap().children[i];
            self.place(child, pos - scroll, child_clip, (xf, child_shown));
        }
    }
    /// Layout and process one input snapshot. Call once per frame, before game
    /// logic reads events. Cancel consumes the frame, preventing click-through.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default(); ui.frame(vec2(800.0,600.0), &ponko::ui::UiInput::default());
    /// ```
    pub fn frame(&mut self, viewport: Vec2, input: &UiInput) {
        // A second pass in a frame (input a system fed) keeps the first's
        // events and closed screens, and its clock stays.
        let again = self.start_frame_pass();
        if !again {
            self.closed.clear();
            self.consumed = false;
        }
        self.pointer = input.pointer;
        self.widgets.advance(if again { 0.0 } else { input.dt });
        self.advance_motion(self.widgets.dt);
        self.layout(viewport);
        let access_moved_focus = self.apply_access_actions();
        // Mid-composition, Enter, Escape and the arrows belong to the IME.
        let ime_keys = self.ime_owns_keys(input);
        // A key-binding field that listens takes the whole frame's input.
        if self.capture_input(input) {
            self.end_frame(viewport, input);
            return;
        }
        if input.cancel && !ime_keys {
            // Escape closes a pop-up, puts back a carried item or undoes a
            // number being typed before it closes a screen.
            if self.cancel_widget() {
                self.consumed = true;
            } else if let Some(root) = self.pop_screen() {
                self.screen_closed(root);
            }
            self.end_frame(viewport, input);
            return;
        }
        // A press outside an open pop-up closes it and goes no further.
        if input.pressed && self.close_popup_outside(input.pointer) {
            self.end_frame(viewport, input);
            return;
        }
        let direction = if ime_keys { Vec2::ZERO } else { input.direction };
        self.hovered = self.hit_test(input.pointer);
        if input.next || input.previous {
            self.tab_focus(input.previous);
            self.widgets.focus_by_keys = true;
        }
        if self.focus.is_some_and(|id| !self.focusable(id)) {
            self.focus = None;
        }
        if input.pressed {
            self.pressed = self.hovered;
            self.drag = None;
            self.widgets.focus_by_keys = false;
            if let Some(id) = self.hovered {
                self.focus = self.focusable(id).then_some(id);
                let n = self.node(id).unwrap();
                if n.widget == UiWidget::TextInput {
                    let caret = self.caret_at(id, input.pointer);
                    let n = self.node_mut(id).unwrap();
                    n.caret = caret;
                    if !input.shift {
                        n.anchor = caret;
                    }
                    self.drag = Some(Drag::Text(id));
                } else if let Some(drag) = self.widget_press(id, input.pointer) {
                    self.drag = Some(drag);
                } else if n.widget != UiWidget::Slider {
                    let mut at = Some(id);
                    while let Some(id) = at {
                        let n = self.node(id).unwrap();
                        if super::widgets::scrolls(n.widget) {
                            self.drag = Some(if input.pointer.x >= n.rect.max.x - 12.0 * self.scale {
                                Drag::Bar(id, input.pointer.y, n.scroll.y)
                            } else {
                                Drag::Scroll(id, input.pointer, n.scroll)
                            });
                            break;
                        }
                        at = n.parent;
                    }
                }
            } else {
                self.focus = None;
            }
        }
        if input.secondary || input.menu {
            self.context_input(input);
        }
        let mut at = self.hovered;
        while let Some(id) = at {
            let n = self.node(id).unwrap();
            if super::widgets::scrolls(n.widget) {
                let next = n.scroll + input.wheel / self.scale;
                self.node_mut(id).unwrap().scroll = next;
                break;
            }
            at = n.parent;
        }
        if input.held {
            if let Some(drag) = self.drag {
                match drag {
                    Drag::Scroll(id, start, offset) => {
                        let scale = self.scale;
                        if let Some(n) = self.node_mut(id) {
                            n.scroll = offset - (input.pointer - start) / scale;
                        }
                        if (input.pointer - start).length() > 4.0 {
                            self.pressed = None;
                        }
                    }
                    Drag::Bar(id, start, offset) => {
                        let n = self.node(id).unwrap();
                        let h = n.rect.size().y;
                        let thumb = (h * h / (h + n.max_scroll.y * self.scale))
                            .max(20.0 * self.scale)
                            .min(h);
                        let track = (h - thumb).max(1.0);
                        let value = offset + (input.pointer.y - start) / track * n.max_scroll.y;
                        self.node_mut(id).unwrap().scroll.y = value;
                        self.pressed = None;
                    }
                    Drag::Text(id) => {
                        let caret = self.caret_at(id, input.pointer);
                        self.node_mut(id).unwrap().caret = caret;
                    }
                    Drag::Split(id) => self.drag_split(id, input.pointer),
                    Drag::Item(id, start) => {
                        if (input.pointer - start).length() > 6.0 * self.scale {
                            self.widgets.dragging = Some(id);
                            self.pressed = None;
                        }
                    }
                }
            }
            if let Some(id) = self
                .pressed
                .filter(|&id| self.node(id).is_some_and(|n| n.widget == UiWidget::Slider))
            {
                let n = self.node(id).unwrap();
                let value = ((input.pointer.x - n.rect.min.x) / n.rect.size().x.max(1.0)).clamp(0.0, 1.0);
                self.change_value(id, value);
            }
        }
        if input.released {
            if let Some(item) = self.widgets.dragging.take() {
                self.drop_at(item, input.pointer);
                self.pressed = None;
            }
            let released = self.pressed.take().filter(|id| Some(*id) == self.hovered);
            // A link in rich text takes the click (a button's too).
            let link = released.and_then(|id| Some((id, self.link_under(id, input.pointer)?)));
            if let Some((id, link)) = link {
                self.events.push_back(UiEvent::LinkClicked(id, link));
            } else if let Some(id) = released {
                self.release_on(id, input.pointer);
            }
            self.drag = None;
        }
        let focused = self.focus.and_then(|id| Some((id, self.node(id)?.widget)));
        let editing = focused.is_some_and(|(_, w)| w == UiWidget::TextInput);
        if editing && direction.y == 0.0 {
            self.edit(input, ime_keys);
        } else if let Some((id, UiWidget::NumberField)) = focused
            && !ime_keys
            && self.type_number(id, input)
        {
        } else if let Some((id, _)) = focused
            && direction != Vec2::ZERO
            && self.widget_direction(id, direction)
        {
        } else if direction != Vec2::ZERO {
            self.navigate(direction);
            self.widgets.focus_by_keys = true;
        }
        if input.confirm
            && !ime_keys
            && let Some(id) = self.focus
        {
            self.confirm(id);
        }
        self.layout(viewport);
        if input.next || input.previous || direction != Vec2::ZERO || access_moved_focus {
            self.reveal_focus();
            self.layout(viewport);
        }
        self.apply_composition(input);
        self.frame_widgets();
    }
    /// The end of a frame cut short (a cancel, a listening field, a press
    /// outside a pop-up): fresh layout, the IME, the widgets' bookkeeping.
    fn end_frame(&mut self, viewport: Vec2, input: &UiInput) {
        self.layout(viewport);
        self.apply_composition(input);
        self.frame_widgets();
    }
    fn reveal_focus(&mut self) {
        let Some(id) = self.focus else { return };
        let mut rect = self.node(id).unwrap().rect;
        let mut at = self.node(id).unwrap().parent;
        let scale = self.scale;
        while let Some(id) = at {
            let n = self.node_mut(id).unwrap();
            if n.widget == UiWidget::Scroll {
                let delta = (rect.max - n.rect.max).max(Vec2::ZERO) + (rect.min - n.rect.min).min(Vec2::ZERO);
                let old = n.scroll;
                n.scroll = (n.scroll + delta / scale).clamp(Vec2::ZERO, n.max_scroll);
                let moved = (n.scroll - old) * scale;
                rect.min -= moved;
                rect.max -= moved;
            }
            at = n.parent;
        }
    }
    fn tab_focus(&mut self, back: bool) {
        let current = self.focus.and_then(|id| self.visible.iter().position(|&n| n == id));
        let len = self.visible.len();
        for step in 1..=len {
            let index = if back {
                (current.unwrap_or(0) + len - step % len) % len
            } else {
                current.map_or(step - 1, |i| (i + step) % len)
            };
            let id = self.visible[index];
            if self.focusable(id) {
                self.focus = Some(id);
                break;
            }
        }
    }
    fn navigate(&mut self, direction: Vec2) {
        let Some(from) = self.focus.and_then(|id| self.rect(id)) else {
            self.tab_focus(false);
            return;
        };
        let mut best = None;
        let mut score = f32::INFINITY;
        for &id in &self.visible {
            if !self.focusable(id) || Some(id) == self.focus {
                continue;
            }
            let delta = self.rect(id).unwrap().center() - from.center();
            let along = delta.dot(direction);
            if along <= 0.0 {
                continue;
            }
            let cross = delta.perp_dot(direction).abs();
            let cost = along + cross * 4.0;
            if cost < score {
                best = Some(id);
                score = cost;
            }
        }
        if best.is_some() {
            self.focus = best;
        }
    }
    pub(super) fn activate(&mut self, id: UiId) {
        if !self.interactive(id) {
            return;
        }
        match self.node(id).unwrap().widget {
            UiWidget::Button => {
                self.events.push_back(UiEvent::Clicked(id));
                if self.node(id).is_some_and(|n| n.closes_screen) {
                    let mut root = id;
                    while let Some(parent) = self.parent(root) {
                        root = parent;
                    }
                    if self.top_screen() == Some(root) {
                        self.pop_screen();
                        self.screen_closed(root);
                    }
                }
            }
            UiWidget::Toggle => {
                let v = 1.0 - self.value(id).unwrap();
                self.change_value(id, v);
            }
            UiWidget::TextInput => self.events.push_back(UiEvent::Submitted(id)),
            _ => self.activate_widget(id),
        }
    }
    /// The player closed screen `root` (it was popped).
    pub(super) fn screen_closed(&mut self, root: UiId) {
        self.events.push_back(UiEvent::ScreenPopped(root));
        self.closed.push(root);
    }
    /// Whether the player closed screen `root` in this frame (Escape, or a
    /// button that closes its screen), whether or not a system has read the
    /// `ScreenPopped` event yet.
    pub(crate) fn closed_this_frame(&self, root: UiId) -> bool {
        self.closed.contains(&root)
    }
    pub(super) fn push_changed(&mut self, id: UiId) {
        self.events.push_back(UiEvent::Changed(id));
    }
    pub(super) fn change_value(&mut self, id: UiId, value: f32) {
        if self.value(id) != Some(value) {
            self.set_value(id, value);
            self.events.push_back(UiEvent::Changed(id));
        }
    }
    fn caret_at(&mut self, id: UiId, p: Vec2) -> usize {
        let n = self.nodes[id.slot()].as_ref().unwrap();
        let layout = self.tree.layout(n.layout).unwrap();
        let font = if self.external_fonts {
            n.paint.font
        } else {
            Font::DEFAULT
        };
        let x = (p.x - n.rect.min.x) / self.scale - layout.padding.left - layout.border.left;
        let k = self.text_pixels();
        let caret_x = self.measure.caret_x(&n.text, font, n.paint.font_size * k, n.caret) / k;
        let available = n.rect.size().x / self.scale - layout.padding.left - layout.padding.right;
        let target = x + (caret_x - available + 3.0).max(0.0);
        self.measure.hit(&n.text, font, n.paint.font_size * k, target * k)
    }
    /// Applies this frame's editing to the focused text input. With
    /// `ime_keys`, a composition owns the keys: only committed text goes in.
    fn edit(&mut self, input: &UiInput, ime_keys: bool) {
        let id = self.focus.unwrap();
        let n = self.node_mut(id).unwrap();
        let mut changed = false;
        let keys = !ime_keys;
        let (backspace, delete) = (keys && input.backspace, keys && input.delete);
        if keys && input.select_all {
            n.anchor = 0;
            n.caret = n.text.len();
        }
        if keys && input.home {
            n.caret = 0;
            if !input.shift {
                n.anchor = n.caret;
            }
        }
        if keys && input.end {
            n.caret = n.text.len();
            if !input.shift {
                n.anchor = n.caret;
            }
        }
        if keys && input.direction.x != 0.0 {
            n.caret = if input.direction.x < 0.0 {
                n.text[..n.caret].char_indices().next_back().map_or(0, |(i, _)| i)
            } else {
                n.text[n.caret..]
                    .chars()
                    .next()
                    .map_or(n.caret, |c| n.caret + c.len_utf8())
            };
            if !input.shift {
                n.anchor = n.caret;
            }
        }
        if backspace || delete || input.text.chars().any(|c| !c.is_control()) {
            let lo = n.caret.min(n.anchor);
            let hi = n.caret.max(n.anchor);
            if lo != hi {
                n.text.replace_range(lo..hi, "");
                n.caret = lo;
                changed = true;
            } else if backspace && n.caret > 0 {
                let prev = n.text[..n.caret].char_indices().next_back().unwrap().0;
                n.text.replace_range(prev..n.caret, "");
                n.caret = prev;
                changed = true;
            } else if delete && n.caret < n.text.len() {
                let end = n.caret + n.text[n.caret..].chars().next().unwrap().len_utf8();
                n.text.replace_range(n.caret..end, "");
                changed = true;
            }
            for ch in input.text.chars().filter(|c| !c.is_control()) {
                n.text.insert(n.caret, ch);
                n.caret += ch.len_utf8();
                changed = true;
            }
            n.anchor = n.caret;
        }
        if changed {
            self.events.push_back(UiEvent::Changed(id));
        }
    }
    pub(super) fn layout_with_text(&mut self, viewport: Vec2, text: &mut crate::text::Text) {
        std::mem::swap(&mut self.measure, text);
        self.external_fonts = true;
        self.layout(viewport);
        self.external_fonts = false;
        std::mem::swap(&mut self.measure, text);
    }
    pub(super) fn frame_with_text(&mut self, viewport: Vec2, input: &UiInput, text: &mut crate::text::Text) {
        std::mem::swap(&mut self.measure, text);
        self.external_fonts = true;
        self.frame(viewport, input);
        self.external_fonts = false;
        std::mem::swap(&mut self.measure, text);
    }
    pub(super) fn scale(&self) -> f32 {
        self.scale
    }
    /// Physical pixels per layout unit, the size text is measured at.
    pub(super) fn text_pixels(&self) -> f32 {
        self.scale * self.pixels
    }
    pub(super) fn modal_root(&self, id: UiId) -> bool {
        self.screens.iter().any(|s| s.root == id && s.modal)
    }
}
pub(super) fn intersect(a: Rect, b: Rect) -> Rect {
    let min = a.min.max(b.min);
    Rect {
        min,
        max: a.max.min(b.max).max(min),
    }
}

/// A rich node's size for Taffy, as plain text is measured: wrapped at the
/// width it is given when it wraps.
fn rich_size(
    rich: &super::rich::RichText,
    n: &Node,
    (external_fonts, pixels): (bool, f32),
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
    text: &mut crate::text::Text,
    scratch: &mut super::rich::RichLayout,
) -> Size<f32> {
    use super::rich::{Align, Base};
    let base = Base {
        font: if external_fonts { n.paint.font } else { Font::DEFAULT },
        size: n.paint.font_size,
        scale: 1.0,
        fonts: external_fonts,
        pixels,
    };
    let natural = rich.layout(base, None, Align::MEASURE, text, scratch);
    let width = known.width.unwrap_or_else(|| match available.width {
        AvailableSpace::Definite(w) if n.paint.wrap => natural.x.min(w.max(0.0)),
        AvailableSpace::MinContent if n.paint.wrap => rich.min_content(base, text),
        _ => natural.x,
    });
    let mut height = if n.paint.wrap {
        rich.layout(base, Some(width), Align::MEASURE, text, scratch).y
    } else {
        natural.y
    };
    if rich.text.is_empty() && !super::widgets::keeps_line(n.widget) {
        height = 0.0;
    }
    Size {
        width,
        height: known.height.unwrap_or(height),
    }
}

pub(super) fn wrap_end(text: &mut crate::text::Text, line: &str, font: Font, size: f32, width: f32) -> usize {
    if let Some(end) = text.wrap_end_shaped_or_none(line, font, size, width) {
        return end;
    }
    let mut space = None;
    for (i, ch) in line.char_indices() {
        if ch.is_whitespace() {
            space = Some(i);
        }
        let next = i + ch.len_utf8();
        if text.layout(&line[..next], font, size, false).x > width {
            return space.filter(|&s| s > 0).unwrap_or(if i == 0 { next } else { i });
        }
    }
    line.len()
}

#[path = "clipboard.rs"]
mod clipboard;
