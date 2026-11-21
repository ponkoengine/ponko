//! The widget set a settings menu, a shop and an inventory need, on top of
//! the retained tree's buttons, toggles, sliders and text inputs: dropdowns,
//! tabs, radio groups, progress bars, spinners, number fields, virtual lists
//! and grids, context menus, split panes, drag and drop, key-binding fields
//! and tooltips.
//!
//! Every widget is an ordinary node: it is laid out by Taffy, drawn by the
//! tree, reached by Tab, the arrows and a gamepad's d-pad, and exposed to
//! assistive technology with its own [`UiRole`]. Pop-ups (a dropdown's
//! options, a context menu) are screens of their own while open, so the
//! focus stays inside them and Escape closes them. A list keeps only the
//! rows on screen as nodes and reuses them as it scrolls, so a list of ten
//! thousand items costs what a list of twenty does.

use super::UiRole;
use super::build::{UiBuilder, UiLayout};
use super::retained::{Drag, Screen, Ui, UiEvent, UiId, UiInput, UiWidget};
use crate::actions::ButtonBinding;
use crate::camera::Rect;
use crate::input::Key;
use crate::text::Font;
use glam::{Vec2, vec2};
use taffy::prelude::*;

/// Which way a [split](Ui::split) lays out its two panes.
/// ```
/// use ponko::ui::{Ui, UiAxis, UiLayout};
/// let mut ui = Ui::default();
/// let root = ui.root().layout(UiLayout::fill()).id();
/// let split = ui.split(root, UiAxis::Horizontal).layout(UiLayout::fill()).id();
/// assert!(ui.panes(split).is_some());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum UiAxis {
    /// Side by side: the divider is upright and moves left and right.
    #[default]
    Horizontal,
    /// One above the other: the divider lies flat and moves up and down.
    Vertical,
}

/// A number field's number and its bounds.
#[derive(Debug, Clone, Copy)]
pub(super) struct Number {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Default for Number {
    fn default() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }
    }
}

/// What a list knows about the items it shows.
#[derive(Debug, Clone, Default)]
pub(super) struct ListState {
    /// How many items it has.
    pub len: usize,
    /// One row's height, in UI units.
    pub row_height: f32,
    /// Items per row: 1 for a list, more for a grid.
    pub columns: usize,
    /// The first item shown and how many are.
    pub first: usize,
    pub shown: usize,
    /// One cell's size, in UI units, as the rows were last placed.
    pub cell: Vec2,
    /// The rows show other items than when a system last heard of it.
    pub rows_changed: bool,
}

/// What a node of the widget set keeps besides its text and value.
#[derive(Debug, Default)]
pub(crate) struct Extra {
    /// A dropdown's options, a list's items (when it has text items).
    pub items: Vec<String>,
    /// The chosen option, tab, radio button or list item.
    pub selected: Option<usize>,
    /// The widget this one is part of: a tab's strip, a radio button's
    /// group, a pop-up entry's dropdown or menu node, a list row's list, a
    /// divider's split.
    pub owner: Option<UiId>,
    /// Which tab, radio button or pop-up entry it is; the item a list row
    /// shows (`usize::MAX`: none).
    pub index: usize,
    /// A number field's number.
    pub number: Number,
    /// What it shows when the pointer rests on it.
    pub tooltip: String,
    /// Its context menu's entries.
    pub menu: Vec<String>,
    pub draggable: bool,
    pub drop_target: bool,
    /// A key-binding field's binding.
    pub binding: Option<ButtonBinding>,
    pub list: ListState,
    /// A tab strip's tabs, a radio group's buttons, a list's rows, a
    /// split's panes and divider.
    pub parts: Vec<UiId>,
    /// A tab strip's pages.
    pub pages: Vec<UiId>,
    pub axis: UiAxis,
    /// The role the node has instead of its widget's (a tab strip's bar and
    /// pages, a dropdown's options).
    pub role: Option<UiRole>,
}

/// An open pop-up: a dropdown's options or a context menu.
#[derive(Debug, Clone, Copy)]
pub(super) struct Popup {
    pub root: UiId,
    pub owner: UiId,
    /// A context menu (its entries send `Chosen`), else a dropdown's options.
    pub context: bool,
}

/// The widget set's state across frames.
#[derive(Debug, Default)]
pub(crate) struct WidgetState {
    /// Seconds of `UiInput::dt` so far: spinners turn with it.
    pub time: f32,
    /// This frame's `UiInput::dt`.
    pub dt: f32,
    pub popup: Option<Popup>,
    /// The node the pointer drags.
    pub dragging: Option<UiId>,
    /// The node picked up with the keyboard or a gamepad.
    pub carrying: Option<UiId>,
    /// The key-binding field listening for the next key.
    pub capturing: Option<UiId>,
    /// The number field being typed into.
    pub typing: Option<UiId>,
    /// The focus last moved by Tab, the arrows or the d-pad (not the pointer).
    pub focus_by_keys: bool,
    /// The node with a tooltip under the pointer, and how long it has been.
    hover: Option<UiId>,
    hover_time: f32,
    /// The tooltip shown: whose, and where (window pixels).
    pub tooltip: Option<(UiId, Rect)>,
    /// The tooltip text last measured: it is measured again only when it
    /// changes, not each frame it follows the pointer.
    tip: TipSize,
    /// Reused for the nodes a pass over the widgets visits.
    scratch: Vec<UiId>,
}

/// A tooltip's text as measured, and what it was measured with.
#[derive(Debug, Default)]
struct TipSize {
    text: String,
    font: Font,
    /// Physical pixels per line.
    px: f32,
    /// The fonts' revision ([`crate::text::Text::revision`]).
    fonts: u64,
    /// Its size in physical pixels; `None`: not measured yet.
    size: Option<Vec2>,
}

impl WidgetState {
    /// A new frame `dt` seconds after the last.
    pub(super) fn advance(&mut self, dt: f32) {
        self.dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.time = (self.time + self.dt) % 3600.0;
    }
}

/// The widgets that scroll their children.
pub(super) fn scrolls(widget: UiWidget) -> bool {
    matches!(widget, UiWidget::Scroll | UiWidget::List)
}

/// The widgets that keep a line's height with no text.
pub(super) fn keeps_line(widget: UiWidget) -> bool {
    matches!(
        widget,
        UiWidget::TextInput
            | UiWidget::Button
            | UiWidget::Toggle
            | UiWidget::Dropdown
            | UiWidget::Tab
            | UiWidget::Radio
            | UiWidget::NumberField
            | UiWidget::MenuItem
            | UiWidget::KeyBinding
    )
}

/// How far a list of `size` UI units scrolls over all its items.
pub(super) fn list_max_scroll(n: &super::retained::Node, size: Vec2) -> Vec2 {
    let Some(list) = n.extra.as_ref().map(|e| &e.list) else {
        return Vec2::ZERO;
    };
    let rows = list.len.div_ceil(list.columns.max(1));
    vec2(0.0, (rows as f32 * list.row_height - size.y).max(0.0))
}

/// What a key-binding field shows for `binding`.
/// ```
/// use ponko::prelude::*;
/// assert_eq!(ponko::ui::binding_label(&ButtonBinding::Key(Key::Space)), "Space");
/// assert_eq!(ponko::ui::binding_label(&ButtonBinding::Mouse(MouseButton::Right)), "Mouse Right");
/// ```
pub fn binding_label(binding: &ButtonBinding) -> String {
    match binding {
        ButtonBinding::Key(key) => {
            let name = format!("{key:?}");
            // `Num1` is the 1 key.
            match name.strip_prefix("Num") {
                Some(digit) if digit.len() == 1 => digit.to_string(),
                _ => name,
            }
        }
        ButtonBinding::Mouse(button) => format!("Mouse {button:?}"),
        ButtonBinding::Gamepad { button, .. } => format!("Pad {button:?}"),
        ButtonBinding::Gesture(gesture) => format!("Gesture {gesture:?}"),
    }
}

/// `value` as a number field shows it: as many decimals as its step has.
fn format_number(value: f64, step: f64) -> String {
    let decimals = if step <= 0.0 || step.fract() == 0.0 {
        0
    } else {
        ((-step.log10()).ceil().max(0.0) as usize).min(6)
    };
    format!("{value:.decimals$}")
}

/// The style of a node placed at `at` with `size`, in UI units, out of flow.
fn placed(at: Vec2, size: Vec2) -> Style {
    Style {
        position: Position::Absolute,
        inset: taffy::Rect {
            left: length(at.x),
            top: length(at.y),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(size.x),
            height: length(size.y),
        },
        flex_direction: FlexDirection::Column,
        ..Default::default()
    }
}

/// A list row `size` big in its list's flow, moved `down` from its place
/// there. Rows stay in the flow: an absolutely placed node is laid out
/// again by every Taffy pass, an in-flow one only when it changes.
fn row_style(down: f32, size: Vec2) -> Style {
    Style {
        position: Position::Relative,
        inset: taffy::Rect {
            left: auto(),
            top: length(down),
            right: auto(),
            bottom: auto(),
        },
        size: Size {
            width: length(size.x),
            height: length(size.y),
        },
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        ..Default::default()
    }
}

impl UiBuilder<'_> {
    /// A dropdown's options or a text list's items ([`Ui::set_items`]).
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let mode = ui.dropdown(root, ["Windowed"]).items(["Windowed", "Fullscreen"]).id();
    /// assert_eq!(ui.items(mode).map(|i| i.len()), Some(2));
    /// ```
    pub fn items<S: AsRef<str>>(self, items: impl IntoIterator<Item = S>) -> Self {
        self.ui.set_items(self.id, items);
        self
    }
    /// Choose an option, tab, radio button or list item, counted from 0
    /// ([`Ui::set_selected`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let quality = ui.dropdown(root, ["Low", "High"]).selected(1).id();
    /// assert_eq!(ui.selected(quality), Some(1));
    /// ```
    pub fn selected(self, index: usize) -> Self {
        self.ui.set_selected(self.id, Some(index));
        self
    }
    /// A number field's smallest and largest number ([`Ui::set_number_range`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let count = ui.number_field(root, 150.0).range(1.0, 99.0).id();
    /// assert_eq!(ui.number(count), Some(99.0));
    /// ```
    pub fn range(self, min: f64, max: f64) -> Self {
        let step = self.ui.extra(self.id).map_or(1.0, |e| e.number.step);
        self.ui.set_number_range(self.id, min, max, step);
        self
    }
    /// How much a number field's arrows and ends change it
    /// ([`Ui::set_number_range`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let price = ui.number_field(root, 2.5).range(0.0, 10.0).step(0.5).id();
    /// assert_eq!(ui.text(price), Some("2.5"));
    /// ```
    pub fn step(self, step: f64) -> Self {
        let number = self.ui.extra(self.id).map(|e| e.number).unwrap_or_default();
        self.ui.set_number_range(self.id, number.min, number.max, step);
        self
    }
    /// What shows when the pointer rests on the node or the focus reaches
    /// it ([`Ui::set_tooltip`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let vsync = ui.toggle(root, "V-sync").tooltip("Waits for the display").id();
    /// assert_eq!(ui.tooltip(vsync), Some("Waits for the display"));
    /// ```
    pub fn tooltip(self, text: impl AsRef<str>) -> Self {
        self.ui.set_tooltip(self.id, text);
        self
    }
    /// The node's context menu ([`Ui::set_context_menu`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let slot = ui.button(root, "Potion").context_menu(["Use", "Drop"]).id();
    /// assert_eq!(ui.context_menu(slot).map(|m| m.len()), Some(2));
    /// ```
    pub fn context_menu<S: AsRef<str>>(self, entries: impl IntoIterator<Item = S>) -> Self {
        self.ui.set_context_menu(self.id, entries);
        self
    }
    /// The player can drag the node onto a drop target ([`Ui::set_draggable`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let gem = ui.label(root, "Gem").draggable().id();
    /// assert!(ui.is_draggable(gem));
    /// ```
    pub fn draggable(self) -> Self {
        self.ui.set_draggable(self.id, true);
        self
    }
    /// Draggable nodes can be dropped on the node ([`Ui::set_drop_target`]).
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let bag = ui.panel(root).drop_target().id();
    /// assert!(ui.is_drop_target(bag));
    /// ```
    pub fn drop_target(self) -> Self {
        self.ui.set_drop_target(self.id, true);
        self
    }
}

/// How thick a split's divider is, in layout units.
const SPLIT_DIVIDER: f32 = 6.0;
impl Ui {
    pub(super) fn extra(&self, id: UiId) -> Option<&super::widgets::Extra> {
        self.node(id)?.extra.as_deref()
    }
    pub(super) fn extra_mut(&mut self, id: UiId) -> Option<&mut Extra> {
        Some(self.node_mut(id)?.extra.get_or_insert_with(Default::default))
    }
    /// Changes the node's extra state with `f`, making it first; nothing
    /// for a removed node.
    fn with_extra(&mut self, id: UiId, f: impl FnOnce(&mut Extra)) {
        if let Some(e) = self.extra_mut(id) {
            f(e);
        }
    }
    /// What kind of widget node `id` is; `None` for a removed node.
    /// ```
    /// use ponko::ui::{Ui, UiWidget};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let tabs = ui.tabs(root).id();
    /// assert_eq!(ui.widget(tabs), Some(UiWidget::Tabs));
    /// ```
    pub fn widget(&self, id: UiId) -> Option<UiWidget> {
        Some(self.node(id)?.widget)
    }
    fn owner(&self, id: UiId) -> Option<UiId> {
        self.extra(id)?.owner
    }

    /// Whether the node takes keyboard and gamepad focus.
    pub(super) fn takes_focus(&self, n: &super::retained::Node) -> bool {
        matches!(
            n.widget,
            UiWidget::Button
                | UiWidget::TextInput
                | UiWidget::Toggle
                | UiWidget::Slider
                | UiWidget::Dropdown
                | UiWidget::Tab
                | UiWidget::Radio
                | UiWidget::NumberField
                | UiWidget::List
                | UiWidget::MenuItem
                | UiWidget::Splitter
                | UiWidget::KeyBinding
        ) || n
            .extra
            .as_ref()
            .is_some_and(|e| e.draggable || (e.drop_target && self.widgets.carrying.is_some()))
    }

    // ---- building --------------------------------------------------------

    /// Add a dropdown (a select box) showing its chosen option; activating
    /// it opens the options, and choosing one queues
    /// [`UiEvent::Changed`] for the dropdown. [`Ui::selected`] reads the
    /// choice. The first option starts chosen.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let quality = ui.dropdown(root, ["Low", "Medium", "High"]).layout(UiLayout::fixed(160.0, 32.0)).id();
    /// ui.push_modeless(root);
    /// ui.set_focus(quality);
    /// let size = vec2(320.0, 240.0);
    /// ui.frame(size, &UiInput { confirm: true, ..Default::default() }); // open
    /// ui.frame(size, &UiInput { direction: vec2(0.0, 1.0), ..Default::default() });
    /// ui.frame(size, &UiInput { confirm: true, ..Default::default() }); // choose
    /// assert_eq!(ui.selected(quality), Some(1));
    /// assert!(ui.events().any(|e| e == UiEvent::Changed(quality)));
    /// ```
    pub fn dropdown<S: AsRef<str>>(&mut self, parent: UiId, options: impl IntoIterator<Item = S>) -> UiBuilder<'_> {
        let builder = self.build(parent, UiWidget::Dropdown);
        builder.ui.set_items(builder.id, options);
        builder
    }

    /// Add a tab strip: a bar of tabs over one page per tab, of which only
    /// the chosen tab's shows. Add tabs with [`Ui::tab`]; choosing one
    /// queues [`UiEvent::Changed`] for the strip, and [`Ui::selected`]
    /// reads which.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let tabs = ui.tabs(root).layout(UiLayout::fill()).id();
    /// let audio = ui.tab(tabs, "Audio").id();
    /// let video = ui.tab(tabs, "Video").id();
    /// ui.push_modeless(root);
    /// ui.layout(ponko::prelude::vec2(320.0, 240.0)); // pages show and hide at layout
    /// assert!(ui.is_shown(audio) && !ui.is_shown(video));
    /// ```
    pub fn tabs(&mut self, parent: UiId) -> UiBuilder<'_> {
        let builder = self.build(parent, UiWidget::Tabs);
        let (ui, tabs) = (builder.ui, builder.id);
        let spacing = ui.theme.spacing;
        let bar = ui.add_child(
            tabs,
            UiWidget::Panel,
            UiLayout::unstable_custom(Style {
                flex_direction: FlexDirection::Row,
                gap: Size::length(spacing * 0.5),
                flex_shrink: 0.0,
                ..Default::default()
            }),
        );
        if let Some(p) = ui.paint_mut(bar) {
            p.background = crate::color::Color::rgba(0.0, 0.0, 0.0, 0.0);
        }
        ui.with_extra(bar, |e| e.role = Some(UiRole::TabList));
        UiBuilder { ui, id: tabs }
    }

    /// Add a tab titled `title` to a [tab strip](Ui::tabs), and its page:
    /// the builder is the page's, a panel that fills the strip below the
    /// bar. The first tab starts chosen.
    ///
    /// # Panics
    ///
    /// If `tabs` is not a tab strip.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let tabs = ui.tabs(root).layout(UiLayout::fill()).id();
    /// let audio = ui.tab(tabs, "Audio").id();
    /// ui.slider(audio).accessible_name("Music volume");
    /// assert_eq!(ui.tab_button(tabs, 0).and_then(|t| ui.text(t)), Some("Audio"));
    /// ```
    pub fn tab(&mut self, tabs: UiId, title: impl AsRef<str>) -> UiBuilder<'_> {
        let bar = self
            .node(tabs)
            .filter(|n| n.widget == UiWidget::Tabs)
            .and_then(|n| n.children.first().copied())
            .expect("ponko: Ui::tab needs a tab strip made with Ui::tabs");
        let index = self.extra(tabs).map_or(0, |e| e.parts.len());
        let (wide, tall) = (self.theme.spacing, self.theme.spacing * 0.5);
        let button = self
            .build(bar, UiWidget::Tab)
            .text(title)
            .layout(UiLayout::unstable_custom(Style {
                padding: taffy::Rect {
                    left: LengthPercentage::length(wide),
                    right: LengthPercentage::length(wide),
                    top: LengthPercentage::length(tall),
                    bottom: LengthPercentage::length(tall),
                },
                ..Default::default()
            }))
            .id();
        let page = self.add_child(
            tabs,
            UiWidget::Panel,
            UiLayout::unstable_custom(Style {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                min_size: Size::length(0.0),
                ..Default::default()
            }),
        );
        for (part, role) in [(button, None), (page, Some(UiRole::TabPanel))] {
            self.with_extra(part, |e| {
                e.owner = Some(tabs);
                e.index = index;
                e.role = role;
            });
        }
        self.with_extra(tabs, |e| {
            e.parts.push(button);
            e.pages.push(page);
        });
        if index == 0 {
            self.set_selected(tabs, Some(0));
        }
        UiBuilder { ui: self, id: page }
    }

    /// The tab button of a tab strip's tab `index`.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let tabs = ui.tabs(root).id();
    /// ui.tab(tabs, "Audio");
    /// assert!(ui.tab_button(tabs, 0).is_some() && ui.tab_button(tabs, 1).is_none());
    /// ```
    pub fn tab_button(&self, tabs: UiId, index: usize) -> Option<UiId> {
        self.extra(tabs)?
            .parts
            .get(index)
            .copied()
            .filter(|_| self.widget(tabs) == Some(UiWidget::Tabs))
    }

    /// The page of a tab strip's tab `index`.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let tabs = ui.tabs(root).id();
    /// let audio = ui.tab(tabs, "Audio").id();
    /// assert_eq!(ui.tab_page(tabs, 0), Some(audio));
    /// ```
    pub fn tab_page(&self, tabs: UiId, index: usize) -> Option<UiId> {
        self.extra(tabs)?.pages.get(index).copied()
    }

    /// Add a radio group: a column of radio buttons ([`Ui::radio`]) of which
    /// one is chosen. Choosing one queues [`UiEvent::Changed`] for the
    /// group; [`Ui::selected`] reads which. None is chosen at first.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let shadows = ui.radio_group(root).id();
    /// ui.radio(shadows, "Off");
    /// let high = ui.radio(shadows, "High").id();
    /// ui.push_modeless(root);
    /// ui.set_focus(high);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { confirm: true, ..Default::default() });
    /// assert_eq!(ui.selected(shadows), Some(1));
    /// ```
    pub fn radio_group(&mut self, parent: UiId) -> UiBuilder<'_> {
        let spacing = self.theme.spacing;
        let builder = self.build(parent, UiWidget::RadioGroup);
        builder.ui.set_layout(
            builder.id,
            UiLayout::unstable_custom(Style {
                flex_direction: FlexDirection::Column,
                gap: Size::length(spacing * 0.5),
                ..Default::default()
            }),
        );
        builder
    }

    /// Add a radio button labelled `text` to a [radio group](Ui::radio_group).
    ///
    /// # Panics
    ///
    /// If `group` is not a radio group.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let group = ui.radio_group(root).selected(0).id();
    /// let off = ui.radio(group, "Off").id();
    /// assert_eq!(ui.value(off), Some(1.0));
    /// ```
    pub fn radio(&mut self, group: UiId, text: impl AsRef<str>) -> UiBuilder<'_> {
        let index = self.extra(group).map_or(0, |e| e.parts.len());
        let chosen = self.extra(group).and_then(|e| e.selected) == Some(index);
        assert!(
            self.widget(group) == Some(UiWidget::RadioGroup),
            "ponko: Ui::radio needs a group made with Ui::radio_group"
        );
        let radio = self
            .build(group, UiWidget::Radio)
            .text(text)
            .value(if chosen { 1.0 } else { 0.0 })
            .id();
        self.with_extra(radio, |e| {
            e.owner = Some(group);
            e.index = index;
        });
        self.with_extra(group, |e| e.parts.push(radio));
        UiBuilder { ui: self, id: radio }
    }

    /// Add a progress bar filled to its value, from 0 to 1 (set it with
    /// [`Ui::set_value`] or the builder's `.value(..)`). It takes no input.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let loading = ui.progress_bar(root).layout(UiLayout::row(12.0)).value(0.4).id();
    /// assert_eq!(ui.value(loading), Some(0.4));
    /// ```
    pub fn progress_bar(&mut self, parent: UiId) -> UiBuilder<'_> {
        self.build(parent, UiWidget::Progress)
    }

    /// Add a spinner, a busy indicator that turns while shown (with
    /// [`UiInput::dt`]). It is 24 units square unless laid out otherwise.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let busy = ui.spinner(root).accessible_name("Saving").id();
    /// assert_eq!(ui.accessible_name(busy), Some("Saving"));
    /// ```
    pub fn spinner(&mut self, parent: UiId) -> UiBuilder<'_> {
        self.build(parent, UiWidget::Spinner)
            .layout(UiLayout::fixed(24.0, 24.0))
    }

    /// Add a number field holding `value`, from 0 to 100 in steps of 1
    /// unless given a [`range`](UiBuilder::range) and a
    /// [`step`](UiBuilder::step). Left and right (and its `-` and `+` ends)
    /// step it; typing digits replaces it, Enter or moving on keeps what was
    /// typed and Escape drops it. A change queues [`UiEvent::Changed`];
    /// [`Ui::number`] reads it.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let count = ui.number_field(root, 1.0).range(1.0, 20.0).layout(UiLayout::fixed(120.0, 32.0)).id();
    /// ui.push_modeless(root);
    /// ui.set_focus(count);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { direction: vec2(1.0, 0.0), ..Default::default() });
    /// assert_eq!(ui.number(count), Some(2.0));
    /// ```
    pub fn number_field(&mut self, parent: UiId, value: f64) -> UiBuilder<'_> {
        let builder = self
            .build(parent, UiWidget::NumberField)
            .align(super::UiTextAlign::Center);
        builder.ui.set_number(builder.id, value);
        builder
    }

    /// Add a scrolling list of rows `row_height` units tall. Give it items
    /// with [`Ui::set_items`] (text) or a length with [`Ui::set_list_len`]
    /// (rows you fill yourself, [`Ui::list_rows`]). It keeps only the rows
    /// on screen as nodes ([`UiWidget::ListItem`]), so its cost doesn't grow
    /// with its length. Clicking a row, or the arrows while it has focus,
    /// chooses an item ([`UiEvent::Changed`], [`Ui::selected`]); confirm
    /// queues [`UiEvent::Submitted`].
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let saves = ui.list(root, 24.0).layout(UiLayout::fixed(200.0, 120.0)).id();
    /// ui.set_items(saves, (0..10_000).map(|i| format!("Save {i}")));
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// // 120 / 24 = 5 rows on screen, plus one for the row scrolling in.
    /// assert_eq!(ui.list_rows(saves).count(), 6);
    /// ```
    pub fn list(&mut self, parent: UiId, row_height: f32) -> UiBuilder<'_> {
        self.grid_view(parent, 1, row_height)
    }

    /// Add a scrolling grid of `columns` cells per row, rows `row_height`
    /// units tall: a [list](Ui::list) whose arrows move by cell. An
    /// inventory of any size.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let bag = ui.grid_view(root, 4, 64.0).layout(UiLayout::fixed(256.0, 128.0)).id();
    /// ui.set_list_len(bag, 400);
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// assert_eq!(ui.list_rows(bag).count(), 12); // 3 rows of 4
    /// ```
    pub fn grid_view(&mut self, parent: UiId, columns: usize, row_height: f32) -> UiBuilder<'_> {
        let builder = self.build(parent, UiWidget::List);
        builder.ui.with_extra(builder.id, |e| {
            e.list.columns = columns.max(1);
            e.list.row_height = if row_height.is_finite() {
                row_height.max(1.0)
            } else {
                24.0
            };
        });
        builder
    }

    /// Add two panes split by a divider the player drags (or moves with the
    /// arrows while it has focus). The split's value
    /// ([`Ui::value`], [`Ui::set_value`]) is the first pane's share of the
    /// length the divider leaves, 0.5 (equal panes) at first. Dragging
    /// queues [`UiEvent::Changed`] for the split.
    /// [`Ui::panes`] gives the two panes to fill.
    /// ```
    /// use ponko::ui::{Ui, UiAxis, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let split = ui.split(root, UiAxis::Horizontal).layout(UiLayout::fill()).value(0.25).id();
    /// let (left, right) = ui.panes(split).unwrap();
    /// ui.push_modeless(root);
    /// ui.layout(vec2(406.0, 100.0)); // 400 for the panes, 6 for the divider
    /// assert_eq!(ui.rect(left).unwrap().size().x, 100.0);
    /// assert_eq!(ui.rect(right).unwrap().size().x, 300.0);
    /// ```
    pub fn split(&mut self, parent: UiId, axis: UiAxis) -> UiBuilder<'_> {
        let builder = self.build(parent, UiWidget::Split).value(0.5);
        let (ui, split) = (builder.ui, builder.id);
        let first = ui.add_child(split, UiWidget::Panel, UiLayout::auto());
        let divider = ui.add_child(split, UiWidget::Splitter, UiLayout::auto());
        let second = ui.add_child(split, UiWidget::Panel, UiLayout::auto());
        ui.with_extra(divider, |e| e.owner = Some(split));
        ui.with_extra(split, |e| {
            e.axis = axis;
            e.parts = vec![first, divider, second];
        });
        UiBuilder { ui, id: split }
    }

    /// A split's two panes: left then right, or top then bottom.
    /// ```
    /// use ponko::ui::{Ui, UiAxis};
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let split = ui.split(root, UiAxis::Vertical).id();
    /// let (top, bottom) = ui.panes(split).unwrap();
    /// assert_ne!(top, bottom);
    /// ```
    pub fn panes(&self, split: UiId) -> Option<(UiId, UiId)> {
        match self.extra(split)?.parts.as_slice() {
            &[first, _, second] if self.widget(split) == Some(UiWidget::Split) => Some((first, second)),
            _ => None,
        }
    }

    /// Add a key-binding field showing `binding`. Activating it listens:
    /// the next key, mouse button or gamepad button pressed
    /// ([`UiInput::captured`]) becomes its binding and queues
    /// [`UiEvent::Changed`]; Escape stops listening and keeps the old one.
    /// [`Ui::binding`] reads it.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let jump = ui.key_binding(root, Some(ButtonBinding::Key(Key::Space))).layout(UiLayout::fixed(120.0, 32.0)).id();
    /// ui.push_modeless(root);
    /// ui.set_focus(jump);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { confirm: true, ..Default::default() });
    /// assert!(ui.is_capturing(jump));
    /// let j = ButtonBinding::Key(Key::J);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { captured: Some(j.clone()), ..Default::default() });
    /// assert_eq!(ui.binding(jump), Some(&j));
    /// assert_eq!(ui.text(jump), Some("J"));
    /// ```
    pub fn key_binding(&mut self, parent: UiId, binding: Option<ButtonBinding>) -> UiBuilder<'_> {
        let builder = self
            .build(parent, UiWidget::KeyBinding)
            .align(super::UiTextAlign::Center);
        builder.ui.set_binding(builder.id, binding);
        builder
    }

    // ---- reading and changing ---------------------------------------------

    /// Replace a dropdown's options or a list's items (the list's length
    /// becomes their count). A dropdown keeps its choice when it is still
    /// an option, else chooses the first.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let lang = ui.dropdown(root, ["English"]).id();
    /// ui.set_items(lang, ["English", "Deutsch"]);
    /// assert_eq!(ui.items(lang).unwrap()[1], "Deutsch");
    /// ```
    pub fn set_items<S: AsRef<str>>(&mut self, id: UiId, items: impl IntoIterator<Item = S>) {
        let Some(widget) = self.widget(id) else { return };
        let Some(e) = self.extra_mut(id) else { return };
        e.items.clear();
        e.items.extend(items.into_iter().map(|s| s.as_ref().to_string()));
        let count = e.items.len();
        match widget {
            UiWidget::Dropdown => {
                let keep = e.selected.filter(|&i| i < count).or((count > 0).then_some(0));
                e.selected = None;
                self.set_selected(id, keep);
            }
            UiWidget::List => {
                self.set_list_len(id, count);
                // Same length, other text: the rows show it at the next layout.
                self.with_extra(id, |e| e.list.shown = usize::MAX);
            }
            _ => {}
        }
    }

    /// A dropdown's options or a list's text items.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let lang = ui.dropdown(root, ["English"]).id();
    /// assert_eq!(ui.items(lang), Some(&["English".to_string()][..]));
    /// ```
    pub fn items(&self, id: UiId) -> Option<&[String]> {
        Some(&self.extra(id)?.items)
    }

    /// The chosen option of a dropdown, tab of a tab strip, button of a
    /// radio group or item of a list; `None` when nothing is chosen or the
    /// node chooses nothing.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let quality = ui.dropdown(root, ["Low", "High"]).id();
    /// assert_eq!(ui.selected(quality), Some(0));
    /// ```
    pub fn selected(&self, id: UiId) -> Option<usize> {
        self.extra(id)?.selected
    }

    /// Choose an option, tab, radio button or list item by index (out of
    /// range: nothing chosen, except a tab strip, which keeps its tab).
    /// Queues no event: events are the player's changes.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let quality = ui.dropdown(root, ["Low", "High"]).id();
    /// ui.set_selected(quality, Some(1));
    /// assert_eq!(ui.text(quality), Some("High"));
    /// ```
    pub fn set_selected(&mut self, id: UiId, index: Option<usize>) {
        self.select(id, index);
    }

    /// Chooses `index` on `id`; true if the choice changed.
    fn select(&mut self, id: UiId, index: Option<usize>) -> bool {
        let Some(widget) = self.widget(id) else { return false };
        let count = match widget {
            UiWidget::Dropdown => self.extra(id).map_or(0, |e| e.items.len()),
            UiWidget::Tabs | UiWidget::RadioGroup => self.extra(id).map_or(0, |e| e.parts.len()),
            UiWidget::List => self.extra(id).map_or(0, |e| e.list.len),
            _ => return false,
        };
        // A radio group chosen before its buttons are added keeps the
        // choice for the button that gets that index.
        let pending = widget == UiWidget::RadioGroup && count == 0;
        let mut index = index.filter(|&i| pending || i < count);
        if widget == UiWidget::Tabs && index.is_none() {
            // A tab strip shows a page, unless it has none yet (the first
            // tab chooses itself).
            if count > 0 {
                return false;
            }
            index = self.extra(id).and_then(|e| e.selected);
        }
        let Some(e) = self.extra_mut(id) else { return false };
        let changed = e.selected != index;
        e.selected = index;
        match widget {
            UiWidget::Dropdown => {
                let text = index.and_then(|i| e.items.get(i).cloned()).unwrap_or_default();
                if let Some(n) = self.node_mut(id) {
                    n.key = None;
                    n.put_text(&text);
                }
            }
            UiWidget::Tabs | UiWidget::RadioGroup => {
                let parts = std::mem::take(&mut e.parts);
                for (i, &part) in parts.iter().enumerate() {
                    self.set_value(part, if Some(i) == index { 1.0 } else { 0.0 });
                }
                self.with_extra(id, |e| e.parts = parts);
            }
            _ => {}
        }
        changed
    }

    /// The player chose `index` on `id`: chooses it and queues `Changed`.
    fn choose(&mut self, id: UiId, index: usize) {
        if self.select(id, Some(index)) {
            self.push_changed(id);
        }
    }

    /// A number field's number.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let count = ui.number_field(root, 3.0).id();
    /// assert_eq!(ui.number(count), Some(3.0));
    /// ```
    pub fn number(&self, id: UiId) -> Option<f64> {
        let e = self
            .extra(id)
            .filter(|_| self.widget(id) == Some(UiWidget::NumberField))?;
        Some(e.number.value)
    }

    /// Set a number field's number, kept in its range. Queues no event.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let count = ui.number_field(root, 3.0).id();
    /// ui.set_number(count, 250.0);
    /// assert_eq!(ui.number(count), Some(100.0)); // 0 to 100 by default
    /// ```
    pub fn set_number(&mut self, id: UiId, value: f64) {
        if self.widget(id) != Some(UiWidget::NumberField) || !value.is_finite() {
            return;
        }
        let Some(e) = self.extra_mut(id) else { return };
        let n = &mut e.number;
        n.value = value.clamp(n.min, n.max);
        let (number, text) = (*n, format_number(n.value, n.step));
        let span = number.max - number.min;
        let fraction = if span > 0.0 {
            (number.value - number.min) / span
        } else {
            0.0
        };
        if self.widgets.typing == Some(id) {
            self.widgets.typing = None;
        }
        if let Some(node) = self.node_mut(id) {
            node.key = None;
            node.put_text(&text);
            node.value = fraction as f32;
        }
    }

    /// Set a number field's range and step; its number is kept in the new
    /// range. A `min` above `max` swaps them; a step of zero or less (or
    /// not finite) is 1.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let volume = ui.number_field(root, 50.0).id();
    /// ui.set_number_range(volume, 0.0, 10.0, 0.5);
    /// assert_eq!(ui.number_range(volume), Some((0.0, 10.0, 0.5)));
    /// assert_eq!(ui.number(volume), Some(10.0));
    /// ```
    pub fn set_number_range(&mut self, id: UiId, min: f64, max: f64, step: f64) {
        if self.widget(id) != Some(UiWidget::NumberField) || !min.is_finite() || !max.is_finite() {
            return;
        }
        let Some(e) = self.extra_mut(id) else { return };
        e.number.min = min.min(max);
        e.number.max = max.max(min);
        e.number.step = if step.is_finite() && step > 0.0 { step } else { 1.0 };
        let value = e.number.value;
        self.set_number(id, value);
    }

    /// A number field's smallest and largest number and its step.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let count = ui.number_field(root, 3.0).id();
    /// assert_eq!(ui.number_range(count), Some((0.0, 100.0, 1.0)));
    /// ```
    pub fn number_range(&self, id: UiId) -> Option<(f64, f64, f64)> {
        let n = self
            .extra(id)
            .filter(|_| self.widget(id) == Some(UiWidget::NumberField))?
            .number;
        Some((n.min, n.max, n.step))
    }

    /// Steps a number field `steps` steps; queues `Changed` if it moved.
    fn step_number(&mut self, id: UiId, steps: f64) {
        let Some(n) = self.extra(id).map(|e| e.number) else {
            return;
        };
        self.set_number_by_player(id, n.value + steps * n.step);
    }

    fn set_number_by_player(&mut self, id: UiId, value: f64) {
        let before = self.number(id);
        self.set_number(id, value);
        if self.number(id) != before {
            self.push_changed(id);
        }
    }

    /// Set a list's number of items, for rows you fill yourself
    /// ([`Ui::list_rows`]). A choice past the end is dropped.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let list = ui.list(root, 20.0).id();
    /// ui.set_list_len(list, 10_000);
    /// assert_eq!(ui.list_len(list), Some(10_000));
    /// ```
    pub fn set_list_len(&mut self, id: UiId, len: usize) {
        if self.widget(id) != Some(UiWidget::List) {
            return;
        }
        let Some(e) = self.extra_mut(id) else { return };
        if e.list.len != len {
            e.list.len = len;
            e.list.rows_changed = true;
            e.list.shown = usize::MAX;
        }
        if e.selected.is_some_and(|s| s >= len) {
            e.selected = None;
        }
    }

    /// A list's number of items.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let list = ui.list(root, 20.0).id();
    /// assert_eq!(ui.list_len(list), Some(0));
    /// ```
    pub fn list_len(&self, id: UiId) -> Option<usize> {
        let e = self.extra(id).filter(|_| self.widget(id) == Some(UiWidget::List))?;
        Some(e.list.len)
    }

    /// The rows a list shows now, as of its last layout: each row's node and
    /// the item it shows. Fill rows that show more than text from here
    /// (after [`UiEvent::RowsShown`], or every frame).
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let list = ui.list(root, 20.0).layout(UiLayout::fixed(100.0, 60.0)).id();
    /// ui.set_list_len(list, 3);
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// let items: Vec<usize> = ui.list_rows(list).map(|(_, item)| item).collect();
    /// assert_eq!(items, [0, 1, 2]);
    /// ```
    pub fn list_rows(&self, list: UiId) -> impl Iterator<Item = (UiId, usize)> + '_ {
        let (first, shown, parts) = match self.extra(list).filter(|_| self.widget(list) == Some(UiWidget::List)) {
            Some(e) if e.list.shown != usize::MAX => (e.list.first, e.list.shown, e.parts.as_slice()),
            _ => (0, 0, &[][..]),
        };
        (first..first + shown).map(move |item| (parts[item % shown], item))
    }

    /// The item a list's row shows, `None` for a row that shows none (or a
    /// node that isn't a row).
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let list = ui.list(root, 20.0).layout(UiLayout::fixed(100.0, 60.0)).id();
    /// ui.set_list_len(list, 3);
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// let (row, _) = ui.list_rows(list).nth(1).unwrap();
    /// assert_eq!(ui.list_item(row), Some(1));
    /// ```
    pub fn list_item(&self, row: UiId) -> Option<usize> {
        let e = self
            .extra(row)
            .filter(|_| self.widget(row) == Some(UiWidget::ListItem))?;
        (e.index != usize::MAX).then_some(e.index)
    }

    /// Scroll a list so its item `index` is on screen.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// use ponko::prelude::*;
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let list = ui.list(root, 20.0).layout(UiLayout::fixed(100.0, 60.0)).id();
    /// ui.set_list_len(list, 1000);
    /// ui.push_modeless(root);
    /// ui.layout(vec2(320.0, 240.0));
    /// ui.scroll_to_item(list, 500);
    /// ui.layout(vec2(320.0, 240.0));
    /// assert!(ui.list_rows(list).any(|(_, item)| item == 500));
    /// ```
    pub fn scroll_to_item(&mut self, list: UiId, index: usize) {
        let scale = self.scale;
        let Some(n) = self.node(list).filter(|n| n.widget == UiWidget::List) else {
            return;
        };
        let Some(e) = n.extra.as_deref() else { return };
        let row = (index / e.list.columns.max(1)) as f32 * e.list.row_height;
        let view = n.rect.size().y / scale;
        let mut scroll = n.scroll.y;
        if row < scroll {
            scroll = row;
        } else if row + e.list.row_height > scroll + view {
            scroll = row + e.list.row_height - view;
        }
        if let Some(n) = self.node_mut(list) {
            n.scroll.y = scroll.max(0.0);
        }
    }

    /// A key-binding field's binding.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let jump = ui.key_binding(root, Some(ButtonBinding::Key(Key::Space))).id();
    /// assert_eq!(ui.binding(jump), Some(&ButtonBinding::Key(Key::Space)));
    /// ```
    pub fn binding(&self, id: UiId) -> Option<&ButtonBinding> {
        self.extra(id)?.binding.as_ref()
    }

    /// Set a key-binding field's binding (`None`: unbound, shown as `-`).
    /// Queues no event.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let jump = ui.key_binding(root, None).id();
    /// assert_eq!(ui.text(jump), Some("-"));
    /// ui.set_binding(jump, Some(ButtonBinding::Mouse(MouseButton::Left)));
    /// assert_eq!(ui.text(jump), Some("Mouse Left"));
    /// ```
    pub fn set_binding(&mut self, id: UiId, binding: Option<ButtonBinding>) {
        if self.widget(id) != Some(UiWidget::KeyBinding) {
            return;
        }
        let text = binding.as_ref().map_or_else(|| "-".to_string(), binding_label);
        self.with_extra(id, |e| e.binding = binding);
        if let Some(n) = self.node_mut(id) {
            n.key = None;
            n.put_text(&text);
        }
    }

    /// Whether a key-binding field is listening for the next key.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let jump = ui.key_binding(root, None).id();
    /// assert!(!ui.is_capturing(jump));
    /// ```
    pub fn is_capturing(&self, id: UiId) -> bool {
        self.widgets.capturing == Some(id)
    }

    /// Show `text` when the pointer rests on the node
    /// ([`UiTheme::tooltip_delay`](super::UiTheme::tooltip_delay)) or the
    /// keyboard or a gamepad moves the focus to it. Assistive technology
    /// reads it as the node's description unless it has one. Empty: none.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let vsync = ui.toggle(root, "V-sync").layout(UiLayout::fixed(120.0, 30.0)).id();
    /// ui.set_tooltip(vsync, "Waits for the display");
    /// ui.push_modeless(root);
    /// let over = UiInput { pointer: vec2(10.0, 10.0), dt: 0.3, ..Default::default() };
    /// ui.frame(vec2(320.0, 240.0), &over); // the pointer arrives
    /// ui.frame(vec2(320.0, 240.0), &over); // 0.3 s on it
    /// assert!(ui.shown_tooltip().is_none());
    /// ui.frame(vec2(320.0, 240.0), &over); // 0.6 s: past the 0.5 s delay
    /// assert_eq!(ui.shown_tooltip().map(|(id, _)| id), Some(vsync));
    /// ```
    pub fn set_tooltip(&mut self, id: UiId, text: impl AsRef<str>) {
        if let Some(e) = self.node(id).is_some().then(|| self.extra_mut(id)).flatten() {
            e.tooltip.clear();
            e.tooltip.push_str(text.as_ref());
        }
    }

    /// A node's tooltip ([`Ui::set_tooltip`]).
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert_eq!(ui.tooltip(root), None);
    /// ```
    pub fn tooltip(&self, id: UiId) -> Option<&str> {
        self.extra(id).map(|e| e.tooltip.as_str()).filter(|t| !t.is_empty())
    }

    /// The tooltip on screen now: whose, and where, in window pixels.
    /// ```
    /// assert!(ponko::ui::Ui::default().shown_tooltip().is_none());
    /// ```
    pub fn shown_tooltip(&self) -> Option<(UiId, Rect)> {
        self.widgets.tooltip
    }

    /// Give a node a context menu: the right mouse button over it, or the
    /// menu input (Shift+F10, a gamepad's North button) while it has focus,
    /// opens `entries` as a pop-up; choosing one queues
    /// [`UiEvent::Chosen`]. Empty: none.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{Ui, UiEvent, UiInput, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let slot = ui.button(root, "Potion").layout(UiLayout::fixed(100.0, 40.0)).id();
    /// ui.set_context_menu(slot, ["Use", "Drop"]);
    /// ui.push_modeless(root);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { pointer: vec2(20.0, 20.0), secondary: true, ..Default::default() });
    /// assert_eq!(ui.popup_owner(), Some(slot));
    /// ```
    pub fn set_context_menu<S: AsRef<str>>(&mut self, id: UiId, entries: impl IntoIterator<Item = S>) {
        if self.node(id).is_none() {
            return;
        }
        self.with_extra(id, |e| {
            e.menu.clear();
            e.menu.extend(entries.into_iter().map(|s| s.as_ref().to_string()));
        });
    }

    /// A node's context menu entries.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert_eq!(ui.context_menu(root), None);
    /// ```
    pub fn context_menu(&self, id: UiId) -> Option<&[String]> {
        self.extra(id).map(|e| e.menu.as_slice()).filter(|m| !m.is_empty())
    }

    /// Open a node's context menu below it, as the menu input does. Does
    /// nothing for a node that has none or isn't shown.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let slot = ui.button(root, "Potion").context_menu(["Use"]).id();
    /// ui.push_modeless(root);
    /// ui.open_context_menu(slot);
    /// assert_eq!(ui.popup_owner(), Some(slot));
    /// ```
    pub fn open_context_menu(&mut self, id: UiId) {
        if !self.is_shown(id) {
            return;
        }
        let Some(entries) = self.context_menu(id).map(<[String]>::to_vec) else {
            return;
        };
        let rect = self.rect(id).unwrap_or_default();
        self.open_popup(id, entries, vec2(rect.min.x, rect.max.y), 0.0, None, true);
    }

    /// The node whose pop-up (a dropdown's options, a context menu) is open.
    /// ```
    /// assert_eq!(ponko::ui::Ui::default().popup_owner(), None);
    /// ```
    pub fn popup_owner(&self) -> Option<UiId> {
        self.widgets.popup.map(|p| p.owner)
    }

    /// Close the open pop-up, if any, choosing nothing.
    /// ```
    /// use ponko::ui::{Ui, UiLayout};
    /// let mut ui = Ui::default();
    /// let root = ui.root().layout(UiLayout::fill()).id();
    /// let slot = ui.button(root, "Potion").context_menu(["Use"]).id();
    /// ui.push_modeless(root);
    /// ui.open_context_menu(slot);
    /// ui.close_popup();
    /// assert_eq!(ui.popup_owner(), None);
    /// ```
    pub fn close_popup(&mut self) {
        let Some(p) = self.widgets.popup.take() else { return };
        let saved = self
            .screens
            .iter()
            .find(|s| s.root == p.root)
            .and_then(|s| s.saved_focus);
        self.screens.retain(|s| s.root != p.root);
        self.remove(p.root);
        self.pressed = None;
        self.hovered = None;
        self.drag = None;
        self.focus = saved.filter(|&id| self.interactive(id));
    }

    /// Let the player drag the node (with the pointer, or pick it up with
    /// confirm while it has focus) onto a [drop target](Ui::set_drop_target).
    /// A draggable node takes focus. Confirm picks it up instead of
    /// pressing it; a pointer click without moving still presses a button.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let gem = ui.label(root, "Gem").id();
    /// ui.set_draggable(gem, true);
    /// assert!(ui.is_draggable(gem));
    /// ```
    pub fn set_draggable(&mut self, id: UiId, on: bool) {
        if let Some(e) = self.node(id).is_some().then(|| self.extra_mut(id)).flatten() {
            e.draggable = on;
        }
    }

    /// Whether the node is draggable.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert!(!ui.is_draggable(root));
    /// ```
    pub fn is_draggable(&self, id: UiId) -> bool {
        self.extra(id).is_some_and(|e| e.draggable)
    }

    /// Let draggable nodes be dropped on the node: a drop queues
    /// [`UiEvent::Dropped`]. While an item is picked up with the keyboard
    /// or a gamepad, drop targets take focus.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// ui.set_drop_target(root, true);
    /// assert!(ui.is_drop_target(root));
    /// ```
    pub fn set_drop_target(&mut self, id: UiId, on: bool) {
        if let Some(e) = self.node(id).is_some().then(|| self.extra_mut(id)).flatten() {
            e.drop_target = on;
        }
    }

    /// Whether the node is a drop target.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert!(!ui.is_drop_target(root));
    /// ```
    pub fn is_drop_target(&self, id: UiId) -> bool {
        self.extra(id).is_some_and(|e| e.drop_target)
    }

    /// The node being dragged with the pointer or carried with the keyboard
    /// or a gamepad.
    /// ```
    /// assert_eq!(ponko::ui::Ui::default().dragging(), None);
    /// ```
    pub fn dragging(&self) -> Option<UiId> {
        self.widgets.dragging.or(self.widgets.carrying)
    }

    /// Move a node, with everything under it, to the end of `parent`'s
    /// children: what a game does when it accepts a drop. False (and
    /// nothing moves) when either is gone, they belong to different
    /// trees, `parent` is the node or under it, or the node is a screen.
    /// ```
    /// use ponko::ui::Ui;
    /// let mut ui = Ui::default();
    /// let root = ui.root().id();
    /// let (bag, chest) = (ui.panel(root).id(), ui.panel(root).id());
    /// let gem = ui.label(bag, "Gem").id();
    /// assert!(ui.set_parent(gem, chest));
    /// assert_eq!(ui.parent(gem), Some(chest));
    /// ```
    pub fn set_parent(&mut self, id: UiId, parent: UiId) -> bool {
        if self.node(parent).is_none() || self.is_screen(id) || self.is_hud(id) {
            return false;
        }
        // Not under itself.
        let mut at = Some(parent);
        while let Some(a) = at {
            if a == id {
                return false;
            }
            at = self.parent(a);
        }
        let (Some((old, layout)), Some(new_layout)) = (
            self.node(id).map(|n| (n.parent, n.layout)),
            self.node(parent).map(|n| n.layout),
        ) else {
            return false;
        };
        if let Some(old) = old.and_then(|old| self.node_mut(old)) {
            old.children.retain(|&c| c != id);
            let old_layout = old.layout;
            let _ = self.tree.remove_child(old_layout, layout);
        }
        let _ = self.tree.add_child(new_layout, layout);
        if let Some(n) = self.node_mut(parent) {
            n.children.push(id);
        }
        if let Some(n) = self.node_mut(id) {
            n.parent = Some(parent);
        }
        true
    }

    // ---- pop-ups ------------------------------------------------------------

    /// Opens `entries` as a pop-up for `owner` at `at` (window pixels), at
    /// least `min_width` wide, with the focus on entry `chosen` (else the
    /// first).
    fn open_popup(
        &mut self,
        owner: UiId,
        entries: Vec<String>,
        at: Vec2,
        min_width: f32,
        chosen: Option<usize>,
        context: bool,
    ) {
        self.close_popup();
        if entries.is_empty() {
            return;
        }
        let (scale, theme) = (self.scale, self.theme);
        let item_height = theme.text_size + theme.spacing;
        let pad = theme.spacing * 0.5;
        let font = if self.external_fonts { theme.font } else { Font::DEFAULT };
        let k = self.text_pixels();
        let mut width = min_width / scale;
        for entry in &entries {
            let w = self.measure.layout(entry, font, theme.text_size * k, false).x / k;
            width = width.max(w + 2.0 * theme.spacing + 2.0 * pad);
        }
        let height = entries.len() as f32 * item_height + 2.0 * pad;
        let view = self.viewport / scale;
        let mut pos = at / scale;
        if pos.y + height > view.y {
            // Above the owner when there is no room below.
            let above = if context {
                pos.y
            } else {
                self.rect(owner).map_or(pos.y, |r| r.min.y / scale)
            };
            pos.y = above - height;
        }
        pos.y = pos.y.min(view.y - height).max(0.0);
        pos.x = pos.x.min(view.x - width).max(0.0);
        let mut style = placed(pos, vec2(width, height));
        style.padding = taffy::Rect::length(pad);
        let root = self.add(UiWidget::Menu, UiLayout::unstable_custom(style));
        if !context {
            self.with_extra(root, |e| e.role = Some(UiRole::List));
        }
        let focus_at = chosen.filter(|&c| c < entries.len()).unwrap_or(0);
        let mut first = None;
        for (i, entry) in entries.iter().enumerate() {
            let item = self.add_child(root, UiWidget::MenuItem, UiLayout::row(item_height));
            if let Some(n) = self.node_mut(item) {
                n.put_text(entry);
            }
            self.with_extra(item, |e| {
                e.owner = Some(owner);
                e.index = i;
                if !context {
                    e.role = Some(UiRole::ListItem);
                }
            });
            if i == focus_at {
                first = Some(item);
            }
        }
        self.screens.push(Screen {
            root,
            modal: false,
            saved_focus: self.focus.or(Some(owner)),
        });
        self.pressed = None;
        self.drag = None;
        self.hovered = None;
        self.focus = first;
        self.widgets.focus_by_keys = true;
        self.widgets.popup = Some(Popup { root, owner, context });
        // Placed now, so the rest of the frame can focus and hit it.
        let viewport = self.viewport;
        self.layout(viewport);
    }

    /// A press at `pointer` outside the open pop-up closes it; true if so.
    pub(super) fn close_popup_outside(&mut self, pointer: Vec2) -> bool {
        let Some(p) = self.widgets.popup else { return false };
        if self.rect(p.root).is_some_and(|r| r.contains(pointer)) {
            return false;
        }
        self.close_popup();
        self.consumed = true;
        true
    }

    /// The secondary button or the menu input: opens a context menu.
    pub(super) fn context_input(&mut self, input: &UiInput) {
        let from = if input.secondary {
            self.hit_test(input.pointer)
        } else {
            self.focus
        };
        let mut at = from;
        while let Some(id) = at {
            if self.context_menu(id).is_some() {
                break;
            }
            at = self.parent(id);
        }
        let Some((id, entries)) = at.and_then(|id| Some((id, self.context_menu(id)?.to_vec()))) else {
            return;
        };
        if input.secondary && self.can_focus(id) {
            self.focus = Some(id);
        }
        let place = if input.secondary {
            input.pointer
        } else {
            let r = self.rect(id).unwrap_or_default();
            vec2(r.min.x, r.max.y)
        };
        self.open_popup(id, entries, place, 0.0, None, true);
    }

    // ---- input --------------------------------------------------------------

    /// A listening key-binding field takes this frame's input; true if one
    /// listens.
    pub(super) fn capture_input(&mut self, input: &UiInput) -> bool {
        let Some(id) = self.widgets.capturing else { return false };
        if self.node(id).is_none() {
            self.widgets.capturing = None;
            return false;
        }
        let escape = matches!(input.captured, Some(ButtonBinding::Key(Key::Escape)))
            || (input.cancel && input.captured.is_none());
        if escape {
            self.widgets.capturing = None;
            self.consumed = true;
        } else if let Some(binding) = input.captured.clone() {
            self.widgets.capturing = None;
            self.consumed = true;
            let changed = self.binding(id) != Some(&binding);
            self.set_binding(id, Some(binding));
            if changed {
                self.push_changed(id);
            }
        }
        true
    }

    /// Escape on a widget: closes a pop-up, puts back what is carried or
    /// dragged, drops a number being typed. True if it did one.
    pub(super) fn cancel_widget(&mut self) -> bool {
        if self.widgets.popup.is_some() {
            self.close_popup();
            return true;
        }
        if self.widgets.carrying.take().is_some() || self.widgets.dragging.take().is_some() {
            self.pressed = None;
            self.drag = None;
            return true;
        }
        if let Some(id) = self.widgets.typing.take() {
            let value = self.number(id).unwrap_or_default();
            self.set_number(id, value);
            return true;
        }
        false
    }

    /// A press on `id`: what it drags, if it is a split's divider or (in) a
    /// draggable node.
    pub(super) fn widget_press(&self, id: UiId, pointer: Vec2) -> Option<Drag> {
        if self.widget(id)? == UiWidget::Splitter {
            return self.owner(id).map(Drag::Split);
        }
        let mut at = Some(id);
        while let Some(a) = at {
            if self.is_draggable(a) {
                return Some(Drag::Item(a, pointer));
            }
            at = self.parent(a);
        }
        None
    }

    pub(super) fn drag_split(&mut self, split: UiId, pointer: Vec2) {
        let Some(n) = self.node(split) else { return };
        let (r, axis) = (n.rect, self.extra(split).map(|e| e.axis).unwrap_or_default());
        // The pointer holds the divider by its middle; the panes share the
        // length it leaves.
        let thick = SPLIT_DIVIDER * self.scale;
        let (at, start, length) = match axis {
            UiAxis::Horizontal => (pointer.x, r.min.x, r.size().x),
            UiAxis::Vertical => (pointer.y, r.min.y, r.size().y),
        };
        let f = (at - start - thick * 0.5) / (length - thick).max(1.0);
        self.change_value(split, f.clamp(0.05, 0.95));
    }

    /// The dragged `item` let go at `pointer`: dropped on the drop target
    /// under it, if any.
    pub(super) fn drop_at(&mut self, item: UiId, pointer: Vec2) {
        self.drag = None;
        if let Some(target) = self.drop_target_at(item, pointer) {
            self.events.push_back(UiEvent::Dropped(item, target));
        }
    }

    /// The drop target under `pointer` that `item` can go to: not the item
    /// itself or a node under it.
    pub(super) fn drop_target_at(&self, item: UiId, pointer: Vec2) -> Option<UiId> {
        let mut at = self.hit_test(pointer);
        while let Some(a) = at {
            if a == item {
                // The item itself, or under it: look past it.
                at = self.parent(a);
                continue;
            }
            if self.is_drop_target(a) {
                return Some(a);
            }
            at = self.parent(a);
        }
        None
    }

    /// A pointer press and release on `id`.
    pub(super) fn release_on(&mut self, id: UiId, pointer: Vec2) {
        let Some(n) = self.node(id) else { return };
        match n.widget {
            UiWidget::NumberField => {
                if !self.interactive(id) {
                    return;
                }
                let at = (pointer.x - n.rect.min.x) / n.rect.size().x.max(1.0);
                if at < 0.25 {
                    self.step_number(id, -1.0);
                } else if at > 0.75 {
                    self.step_number(id, 1.0);
                }
            }
            UiWidget::ListItem => {
                let (Some(list), Some(item)) = (self.owner(id), self.list_item(id)) else {
                    return;
                };
                if !self.interactive(list) {
                    return;
                }
                if self.can_focus(list) {
                    self.focus = Some(list);
                }
                self.choose(list, item);
            }
            UiWidget::Button
            | UiWidget::Toggle
            | UiWidget::Dropdown
            | UiWidget::Tab
            | UiWidget::Radio
            | UiWidget::MenuItem
            | UiWidget::KeyBinding => self.activate(id),
            // A click focuses a text input; it doesn't submit it.
            _ => {}
        }
    }

    /// Typing into a focused number field; true if anything was typed.
    pub(super) fn type_number(&mut self, id: UiId, input: &UiInput) -> bool {
        let allowed = |c: &char| c.is_ascii_digit() || matches!(c, '.' | '-');
        if !input.text.chars().any(|c| allowed(&c)) && !(input.backspace && self.widgets.typing == Some(id)) {
            return false;
        }
        let fresh = self.widgets.typing != Some(id);
        self.widgets.typing = Some(id);
        let Some(n) = self.node_mut(id) else { return false };
        if fresh {
            n.text.clear();
        }
        n.text.extend(input.text.chars().filter(allowed));
        if input.backspace {
            n.text.pop();
        }
        true
    }

    /// Keeps what was typed into a number field (if it is a number).
    fn commit_number(&mut self, id: UiId) {
        if self.widgets.typing != Some(id) {
            return;
        }
        self.widgets.typing = None;
        let typed = self.text(id).and_then(|t| t.trim().parse::<f64>().ok());
        let old = self.number(id).unwrap_or_default();
        self.set_number_by_player(id, typed.unwrap_or(old));
        // An unchanged or unreadable entry shows the number again.
        let value = self.number(id).unwrap_or_default();
        self.set_number(id, value);
    }

    /// The arrows on a focused widget that uses them; false to move the
    /// focus instead.
    pub(super) fn widget_direction(&mut self, id: UiId, direction: Vec2) -> bool {
        let Some(widget) = self.widget(id) else { return false };
        match widget {
            UiWidget::Slider if direction.x != 0.0 => {
                let value = self.value(id).unwrap_or_default() + direction.x * super::access::SLIDER_STEP;
                self.change_value(id, value.clamp(0.0, 1.0));
                true
            }
            UiWidget::NumberField if direction.x != 0.0 => {
                self.commit_number(id);
                self.step_number(id, direction.x.signum() as f64);
                true
            }
            UiWidget::Splitter => {
                let Some(split) = self.owner(id) else { return false };
                let along = match self.extra(split).map(|e| e.axis).unwrap_or_default() {
                    UiAxis::Horizontal => direction.x,
                    UiAxis::Vertical => direction.y,
                };
                if along == 0.0 {
                    return false;
                }
                let value = self.value(split).unwrap_or(0.5) + along.signum() * super::access::SLIDER_STEP;
                self.change_value(split, value.clamp(0.05, 0.95));
                true
            }
            UiWidget::List => {
                let Some(e) = self.extra(id) else { return false };
                let (len, columns) = (e.list.len, e.list.columns.max(1));
                if len == 0 {
                    return false;
                }
                // `signum` of 0.0 is 1.0: the sign by hand.
                let sign = |v: f32| (v > 0.0) as isize - (v < 0.0) as isize;
                let step = sign(direction.x) + sign(direction.y) * columns as isize;
                let Some(current) = e.selected else {
                    self.choose(id, 0);
                    self.scroll_to_item(id, 0);
                    return true;
                };
                // A grid's left and right stay in the row.
                if direction.x != 0.0 {
                    let column = current % columns;
                    if (direction.x < 0.0 && column == 0) || (direction.x > 0.0 && column + 1 == columns) {
                        return false;
                    }
                }
                let next = current as isize + step;
                if next < 0 || next >= len as isize {
                    return false;
                }
                self.choose(id, next as usize);
                self.scroll_to_item(id, next as usize);
                true
            }
            _ => false,
        }
    }

    /// Confirm (Enter, a gamepad's South button, assistive technology's
    /// activate) on `id`: drops or picks up an item, or activates it.
    pub(crate) fn confirm(&mut self, id: UiId) {
        if !self.interactive(id) {
            return;
        }
        if let Some(item) = self.widgets.carrying {
            if id == item {
                // Put it back where it was.
                self.widgets.carrying = None;
                return;
            }
            if self.is_drop_target(id) {
                self.widgets.carrying = None;
                self.events.push_back(UiEvent::Dropped(item, id));
                // The item may stop being focusable with its target.
                return;
            }
        }
        if self.is_draggable(id) && self.widgets.carrying.is_none() {
            self.widgets.carrying = Some(id);
            return;
        }
        self.activate(id);
    }

    /// Activating one of the widget set's widgets.
    pub(super) fn activate_widget(&mut self, id: UiId) {
        let Some(widget) = self.widget(id) else { return };
        match widget {
            UiWidget::Dropdown => {
                if self.popup_owner() == Some(id) {
                    self.close_popup();
                    return;
                }
                let Some(e) = self.extra(id) else { return };
                let (entries, chosen) = (e.items.clone(), e.selected);
                let r = self.rect(id).unwrap_or_default();
                self.open_popup(id, entries, vec2(r.min.x, r.max.y), r.size().x, chosen, false);
            }
            UiWidget::Tab | UiWidget::Radio => {
                let (Some(owner), Some(index)) = (self.owner(id), self.extra(id).map(|e| e.index)) else {
                    return;
                };
                self.choose(owner, index);
            }
            UiWidget::MenuItem => {
                let (Some(owner), Some(index)) = (self.owner(id), self.extra(id).map(|e| e.index)) else {
                    return;
                };
                let context = self.widgets.popup.is_some_and(|p| p.context);
                self.close_popup();
                if context {
                    self.events.push_back(UiEvent::Chosen(owner, index as u32));
                } else {
                    self.choose(owner, index);
                }
            }
            UiWidget::KeyBinding => self.widgets.capturing = Some(id),
            UiWidget::List => {
                if self.selected(id).is_some() {
                    self.events.push_back(UiEvent::Submitted(id));
                }
            }
            UiWidget::NumberField => self.commit_number(id),
            _ => {}
        }
    }

    /// Assistive technology sets a value: a number field's number, a
    /// dropdown's or list's choice (by index), a split's divider.
    pub(super) fn access_set_value(&mut self, id: UiId, value: f32) {
        match self.widget(id) {
            Some(UiWidget::NumberField) => {
                self.widgets.typing = None;
                self.set_number_by_player(id, value as f64);
            }
            Some(UiWidget::Dropdown | UiWidget::List) if value >= 0.0 => self.choose(id, value as usize),
            Some(UiWidget::Splitter) => {
                if let Some(split) = self.owner(id) {
                    self.change_value(split, value.clamp(0.05, 0.95));
                }
            }
            _ => {}
        }
    }

    /// Assistive technology's increment and decrement.
    pub(super) fn access_step(&mut self, id: UiId, up: bool) {
        let sign = if up { 1.0 } else { -1.0 };
        match self.widget(id) {
            Some(UiWidget::NumberField) => {
                self.widgets.typing = None;
                self.step_number(id, sign as f64);
            }
            Some(UiWidget::Splitter) => {
                if let Some(split) = self.owner(id) {
                    let value = self.value(split).unwrap_or(0.5) + sign * super::access::SLIDER_STEP;
                    self.change_value(split, value.clamp(0.05, 0.95));
                }
            }
            Some(UiWidget::Dropdown) => {
                let count = self.items(id).map_or(0, <[String]>::len);
                let next = self.selected(id).map_or(0, |s| s as isize + sign as isize);
                if next >= 0 && (next as usize) < count {
                    self.choose(id, next as usize);
                }
            }
            _ => {}
        }
    }

    // ---- bookkeeping --------------------------------------------------------

    /// The end of each frame: a number left behind keeps what was typed,
    /// the tooltip follows the pointer and the focus, lists say they show
    /// other items.
    pub(super) fn frame_widgets(&mut self) {
        if let Some(id) = self.widgets.typing
            && self.focus != Some(id)
        {
            self.commit_number(id);
        }
        self.update_tooltip();
        let mut lists = std::mem::take(&mut self.widgets.scratch);
        lists.clear();
        lists.extend(
            self.nodes
                .iter()
                .flatten()
                .filter(|n| n.widget == UiWidget::List && n.extra.as_ref().is_some_and(|e| e.list.rows_changed))
                .map(|n| n.id),
        );
        for &list in &lists {
            if let Some(e) = self.extra_mut(list) {
                e.list.rows_changed = false;
            }
            self.events.push_back(UiEvent::RowsShown(list));
        }
        self.widgets.scratch = lists;
    }

    fn update_tooltip(&mut self) {
        let mut over = self.hovered;
        while let Some(id) = over {
            if self.tooltip(id).is_some() {
                break;
            }
            over = self.parent(id);
        }
        if over != self.widgets.hover {
            self.widgets.hover = over;
            self.widgets.hover_time = 0.0;
        } else if over.is_some() {
            self.widgets.hover_time += self.widgets.dt;
        }
        let busy = self.pressed.is_some() || self.dragging().is_some() || self.widgets.capturing.is_some();
        let (scale, theme) = (self.scale, self.theme);
        let shown = if busy {
            None
        } else if let Some(id) = over.filter(|_| self.widgets.hover_time >= theme.tooltip_delay) {
            Some((id, self.pointer + vec2(12.0, 20.0) * scale))
        } else if let Some(id) = self
            .focus
            .filter(|&id| self.widgets.focus_by_keys && self.tooltip(id).is_some())
        {
            let r = self.rect(id).unwrap_or_default();
            Some((id, vec2(r.min.x, r.max.y + 4.0 * scale)))
        } else {
            None
        };
        self.widgets.tooltip = shown.map(|(id, at)| {
            let font = if self.external_fonts { theme.font } else { Font::DEFAULT };
            let k = self.text_pixels();
            let px = theme.text_size * k;
            let fonts = self.measure.revision();
            let mut tip = std::mem::take(&mut self.widgets.tip);
            let text = self.tooltip(id).unwrap_or_default();
            if tip.text != text || tip.font != font || tip.px != px || tip.fonts != fonts {
                tip.text.clear();
                tip.text.push_str(text);
                tip.size = None;
            }
            let measured = *tip
                .size
                .get_or_insert_with(|| self.measure.layout(&tip.text, font, px, false));
            (tip.font, tip.px, tip.fonts) = (font, px, fonts);
            self.widgets.tip = tip;
            // The theme's spacing on each side, half of it above and below.
            let size = measured / k * scale + vec2(2.0 * theme.spacing, theme.spacing) * scale;
            let at = at.min(self.viewport - size).max(Vec2::ZERO);
            (
                id,
                Rect {
                    min: at,
                    max: at + size,
                },
            )
        });
    }

    /// A node is being removed: forget it in the widget state.
    pub(super) fn widget_removed(&mut self, id: UiId) {
        let w = &mut self.widgets;
        for slot in [
            &mut w.dragging,
            &mut w.carrying,
            &mut w.capturing,
            &mut w.typing,
            &mut w.hover,
        ] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        if w.tooltip.is_some_and(|(t, _)| t == id) {
            w.tooltip = None;
        }
        match w.popup {
            Some(p) if p.root == id => w.popup = None,
            Some(p) if p.owner == id => self.close_popup(),
            _ => {}
        }
    }

    /// Before layout: a split's panes take their shares and its direction,
    /// a tab strip shows only its chosen page.
    pub(super) fn sync_splits(&mut self) {
        let mut nodes = std::mem::take(&mut self.widgets.scratch);
        nodes.clear();
        nodes.extend(
            self.nodes
                .iter()
                .flatten()
                .filter(|n| matches!(n.widget, UiWidget::Split | UiWidget::Tabs))
                .map(|n| n.id),
        );
        for &id in &nodes {
            let Some(n) = self.node(id) else { continue };
            let (widget, value, layout) = (n.widget, n.value, n.layout);
            let Some(e) = n.extra.as_deref() else { continue };
            if widget == UiWidget::Tabs {
                let (chosen, count) = (e.selected, e.pages.len());
                for i in 0..count {
                    let Some(page) = self.extra(id).and_then(|e| e.pages.get(i).copied()) else {
                        break;
                    };
                    let display = if Some(i) == chosen {
                        Display::Flex
                    } else {
                        Display::None
                    };
                    self.restyle(page, |s| s.display = display);
                }
                // The bar stays at the top whatever layout the strip gets.
                self.restyle_node(layout, |s| s.flex_direction = FlexDirection::Column);
                continue;
            }
            let axis = e.axis;
            let &[first, divider, second] = e.parts.as_slice() else {
                continue;
            };
            let row = axis == UiAxis::Horizontal;
            self.restyle_node(layout, |s| {
                s.flex_direction = if row { FlexDirection::Row } else { FlexDirection::Column };
            });
            // The panes share what the divider leaves, `value` and
            // `1 - value` of it: grow weights that add up to one over a zero
            // basis hand out exactly that.
            let value = value.clamp(0.0, 1.0);
            for (pane, share) in [(first, value), (second, 1.0 - value)] {
                self.restyle(pane, |s| {
                    s.flex_basis = length(0.0);
                    s.flex_grow = share;
                    s.flex_shrink = 1.0;
                    s.min_size = Size::length(0.0);
                    s.size = Size::auto();
                });
            }
            self.restyle(divider, |s| {
                s.flex_shrink = 0.0;
                s.flex_grow = 0.0;
                s.flex_basis = length(SPLIT_DIVIDER);
                s.size = Size::auto();
            });
        }
        self.widgets.scratch = nodes;
    }

    /// Changes node `id`'s Taffy style with `f`; true if that changed it.
    fn restyle(&mut self, id: UiId, f: impl FnOnce(&mut Style)) -> bool {
        match self.node(id).map(|n| n.layout) {
            Some(layout) => self.restyle_node(layout, f),
            None => false,
        }
    }

    /// Changes a Taffy node's style with `f`; true if that changed it.
    fn restyle_node(&mut self, layout: NodeId, f: impl FnOnce(&mut Style)) -> bool {
        let Ok(style) = self.tree.style(layout) else {
            return false;
        };
        let mut changed = style.clone();
        f(&mut changed);
        if &changed == style {
            return false;
        }
        let _ = self.tree.set_style(layout, changed);
        true
    }

    /// After layout: each list keeps a row node for every row on screen
    /// (and one more), each showing the item scrolled to its place. True
    /// if a row moved or changed size, so layout runs again.
    pub(super) fn sync_lists(&mut self) -> bool {
        let mut lists = std::mem::take(&mut self.widgets.scratch);
        lists.clear();
        lists.extend(
            self.nodes
                .iter()
                .flatten()
                .filter(|n| n.widget == UiWidget::List)
                .map(|n| n.id),
        );
        let mut moved = false;
        for &list in &lists {
            moved |= self.sync_list(list);
        }
        self.widgets.scratch = lists;
        moved
    }

    fn sync_list(&mut self, list: UiId) -> bool {
        let scale = self.scale;
        let Some(n) = self.node(list) else { return false };
        let (view, scroll, max_scroll) = (n.rect.size() / scale, n.scroll.y, n.max_scroll.y);
        let Some(e) = n.extra.as_deref() else { return false };
        let (len, columns, row_height) = (e.list.len, e.list.columns.max(1), e.list.row_height);
        let rows_on_screen = if view.y > 0.0 {
            (view.y / row_height).ceil() as usize + 1
        } else {
            0
        };
        let want = (rows_on_screen * columns).min(len);
        let bar = if max_scroll > 0.0 { 12.0 } else { 0.0 };
        let cell = vec2(((view.x - bar) / columns as f32).max(0.0), row_height);
        let first = ((scroll / row_height).floor().max(0.0) as usize * columns).min(len.saturating_sub(want));
        let (old_first, old_shown, old_cell) = (e.list.first, e.list.shown, e.list.cell);
        let have = e.parts.len();
        // The rows go top to bottom (a grid's cells wrap left to right,
        // top to bottom), whatever layout the list was given. One line of
        // rows lays out cheaper than a line per row.
        let flow = self.restyle(list, |s| {
            if columns > 1 {
                s.flex_direction = FlexDirection::Row;
                s.flex_wrap = FlexWrap::Wrap;
            } else {
                s.flex_direction = FlexDirection::Column;
                s.flex_wrap = FlexWrap::NoWrap;
            }
            s.align_items = AlignItems::START;
            s.align_content = AlignContent::START;
            s.gap = Size::length(0.0);
            s.padding = taffy::Rect::length(0.0);
        });
        if (first, want, cell) == (old_first, old_shown, old_cell) && have >= want {
            return flow;
        }
        // More rows when the list grew taller; extra ones are hidden.
        for _ in have..want {
            let row = self.add_child(list, UiWidget::ListItem, UiLayout::auto());
            self.with_extra(row, |e| {
                e.owner = Some(list);
                e.index = usize::MAX;
            });
            self.with_extra(list, |e| e.parts.push(row));
        }
        let Some(e) = self.extra_mut(list) else { return false };
        let rows = std::mem::take(&mut e.parts);
        let items = std::mem::take(&mut e.items);
        let mut moved = false;
        for (slot, &row) in rows.iter().enumerate() {
            if slot >= want {
                moved |= self.restyle(row, |s| s.display = Display::None);
                self.with_extra(row, |e| e.index = usize::MAX);
                continue;
            }
            // The item at this row: the one of first..first + want that
            // falls on this slot.
            let item = first + (slot + want - first % want) % want;
            // `want` is whole rows, so the item is in the slot's column; it
            // moves down (or up) from the slot's row to its own.
            let down = (item / columns) as f32 * row_height - (slot / columns) as f32 * row_height;
            let style = row_style(down, cell);
            moved |= self.restyle(row, |s| *s = style);
            let Some(n) = self.node_mut(row) else { continue };
            if let Some(text) = items.get(item)
                && n.text != *text
            {
                n.put_text(text);
            }
            if let Some(e) = n.extra.as_deref_mut() {
                e.index = item;
            }
        }
        let Some(e) = self.extra_mut(list) else { return false };
        e.parts = rows;
        e.items = items;
        e.list.first = first;
        e.list.shown = want;
        e.list.cell = cell;
        if (first, want) != (old_first, old_shown) || old_shown == usize::MAX {
            e.list.rows_changed = true;
        }
        moved || flow || cell != old_cell
    }
}

/// How assistive technology sees one of the widget set's nodes.
pub(super) struct AccessState {
    pub role: UiRole,
    pub selected: bool,
    pub expanded: Option<bool>,
    pub position: Option<(usize, usize)>,
}

impl Ui {
    /// The role, choice, open state and place in its set of node `id`.
    pub(super) fn access_state(&self, id: UiId) -> AccessState {
        let mut state = AccessState {
            role: UiRole::Group,
            selected: false,
            expanded: None,
            position: None,
        };
        let Some(n) = self.node(id) else { return state };
        let e = n.extra.as_deref();
        state.role = e.and_then(|e| e.role).unwrap_or(UiRole::of(n.widget));
        let owner = e.and_then(|e| e.owner);
        let index = e.map_or(0, |e| e.index);
        let set_of = |owner: Option<UiId>| owner.and_then(|o| self.extra(o)).map_or(0, |e| e.parts.len());
        match n.widget {
            UiWidget::List if e.is_some_and(|e| e.list.columns > 1) => state.role = UiRole::Grid,
            UiWidget::ListItem => {
                let list = owner.and_then(|o| self.extra(o));
                if list.is_some_and(|l| l.list.columns > 1) {
                    state.role = UiRole::GridCell;
                }
                if let Some(list) = list.filter(|_| index != usize::MAX) {
                    state.selected = list.selected == Some(index);
                    state.position = Some((index + 1, list.list.len));
                }
            }
            UiWidget::Tab => {
                state.selected = n.value >= 0.5;
                state.position = Some((index + 1, set_of(owner)));
            }
            UiWidget::Radio => state.position = Some((index + 1, set_of(owner))),
            UiWidget::Dropdown => state.expanded = Some(self.popup_owner() == Some(id)),
            UiWidget::MenuItem => {
                let entries = n.parent.and_then(|p| self.node(p)).map_or(0, |p| p.children.len());
                state.position = Some((index + 1, entries));
                if owner.and_then(|o| self.widget(o)) == Some(UiWidget::Dropdown) {
                    state.selected = owner.and_then(|o| self.selected(o)) == Some(index);
                }
            }
            _ => {}
        }
        state
    }
}
