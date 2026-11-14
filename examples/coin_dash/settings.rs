//! The player's choices: UI scale, volume and rebindable controls, kept in a
//! `settings.json` next to the progress file, and the settings screen that
//! edits them.
//!
//! The screen is the engine's retained UI (`ctx.ui`), so it is the
//! reference for three things the engine already offers:
//!
//! - **Navigation.** Tab and Shift-Tab, the arrow keys or the d-pad move the
//!   focus, Enter or the gamepad's South button press the focused control,
//!   Escape or East closes the screen, and the list of controls scrolls to keep
//!   the focus in view. The game writes no navigation code.
//! - **UI scale.** `Ui::set_scale` scales the settings screen's geometry, text
//!   and hit testing together; `hud` applies the same factor to the text it
//!   draws itself.
//! - **Rebinding.** The installed `ActionMap` (`ctx.input.actions()`) is the
//!   one record of what is bound: the screen shows the keys it reads back
//!   from it, and a new key replaces one action in place with
//!   `ctx.input.set_action`, keeping the `ActionId`s the game resolved at
//!   startup. [`Controls`] is only the shape of the saved file:
//!   [`bindings_for`] turns it into a map when the game starts and
//!   [`Controls::from_bindings`] reads it back from the map before saving.
//!   Pressing a control's button opens a small prompt; the next key or button
//!   pressed becomes the new binding, unless it is reserved or already used.
//!   The prompt asks the input service what went down this frame
//!   (`ctx.input.pressed_this_frame()`); it polls no key or button itself.
//!
//! Limitations are listed in the book's chapter on this example.

use super::*;
use ponko::locale::{Arg, Strings};
use std::collections::HashSet;

/// Layout version of the settings file; another version is recovered from.
pub const SETTINGS_VERSION: u32 = 1;

/// The UI scales the settings offer. More than 1.5 would push the title
/// screen's text out of the window, less than 0.75 is too small to read.
pub const SCALES: [f32; 4] = [0.75, 1.0, 1.25, 1.5];

/// Something the player can do, for which a key (and a pad button) can be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Confirm,
    Restart,
    Pause,
    Back,
    Settings,
}

impl Control {
    pub const ALL: [Control; 9] = [
        Control::MoveUp,
        Control::MoveDown,
        Control::MoveLeft,
        Control::MoveRight,
        Control::Confirm,
        Control::Restart,
        Control::Pause,
        Control::Back,
        Control::Settings,
    ];

    /// The string table key of its name on the settings screen.
    pub fn key(self) -> &'static str {
        match self {
            Control::MoveUp => "control.move_up",
            Control::MoveDown => "control.move_down",
            Control::MoveLeft => "control.move_left",
            Control::MoveRight => "control.move_right",
            Control::Confirm => "control.confirm",
            Control::Restart => "control.restart",
            Control::Pause => "control.pause",
            Control::Back => "control.back",
            Control::Settings => "control.settings",
        }
    }

    /// Its name on the settings screen, in the current language.
    pub fn label(self, strings: &Strings) -> String {
        strings.get(self.key()).into_owned()
    }

    /// The action this control's key and button feed: every movement key
    /// is one source of `"move"`, so movement has none of its own.
    pub fn action(self) -> Option<&'static str> {
        match self {
            Control::MoveUp | Control::MoveDown | Control::MoveLeft | Control::MoveRight => None,
            Control::Confirm => Some("confirm"),
            Control::Restart => Some("restart"),
            Control::Pause => Some("pause"),
            Control::Back => Some("back"),
            Control::Settings => Some("settings"),
        }
    }

    /// Movement is the keys here and the left stick on the pad; the stick is not rebindable.
    pub fn is_movement(self) -> bool {
        matches!(
            self,
            Control::MoveUp | Control::MoveDown | Control::MoveLeft | Control::MoveRight
        )
    }
}

/// Keys that always work next to the chosen ones (arrows for movement, Space
/// to confirm, P to pause). They can't be given to another control.
pub const FIXED_KEYS: [Key; 6] = [Key::Left, Key::Right, Key::Up, Key::Down, Key::Space, Key::P];

/// Why a key or button was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The retained UI owns them: Escape closes dialogs, Tab moves the focus,
    /// and on the gamepad East closes dialogs while the four D-pad directions
    /// move the focus. (Escape and East stay the defaults of Pause and Back;
    /// "Reset to defaults" gives them back.)
    Reserved,
    /// One of [`FIXED_KEYS`].
    Fixed,
    /// Another control has it.
    InUse(Control),
    /// Movement has no gamepad button (the stick moves).
    NoButton,
}

/// The gamepad buttons the retained UI uses for navigation, which no control
/// may take: the keyboard side reserves Escape, Tab and the arrow keys the same way.
pub fn is_reserved_pad(button: ponko::input::GamepadButton) -> bool {
    matches!(
        button,
        ponko::input::GamepadButton::East
            | ponko::input::GamepadButton::DPadUp
            | ponko::input::GamepadButton::DPadDown
            | ponko::input::GamepadButton::DPadLeft
            | ponko::input::GamepadButton::DPadRight
    )
}

/// The key, and the gamepad button, chosen for each [`Control`], in the order
/// of [`Control::ALL`]: what `settings.json` stores. The installed bindings are
/// the live record; [`Controls::from_bindings`] reads this back from them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    pub keys: [Key; 9],
    pub pads: [Option<ponko::input::GamepadButton>; 9],
}

impl Default for Controls {
    fn default() -> Self {
        Controls {
            keys: [
                Key::W,
                Key::S,
                Key::A,
                Key::D,
                Key::Enter,
                Key::R,
                Key::Escape,
                Key::Q,
                Key::O,
            ],
            pads: [
                None,
                None,
                None,
                None,
                Some(ponko::input::GamepadButton::South),
                Some(ponko::input::GamepadButton::West),
                Some(ponko::input::GamepadButton::Start),
                Some(ponko::input::GamepadButton::East),
                Some(ponko::input::GamepadButton::North),
            ],
        }
    }
}

impl Controls {
    pub fn key(&self, control: Control) -> Key {
        self.keys[control as usize]
    }

    pub fn pad(&self, control: Control) -> Option<ponko::input::GamepadButton> {
        self.pads[control as usize]
    }

    /// The key as the player reads it: `Enter`, `Esc`, `1`.
    pub fn key_name(&self, control: Control) -> String {
        key_name(self.key(control))
    }

    // ANCHOR: from_bindings
    /// Reads the chosen keys and buttons back from bindings made by
    /// [`bindings_for`] (the installed ones are `ctx.input.actions()`). A
    /// control the map doesn't bind keeps its default.
    pub fn from_bindings(map: &ActionMap) -> Controls {
        let mut controls = Controls::default();
        if let Some(Axis2Binding::Keys { left, right, up, down }) = map.get("move").and_then(|a| a.axes2().first()) {
            controls.keys[Control::MoveLeft as usize] = *left;
            controls.keys[Control::MoveRight as usize] = *right;
            controls.keys[Control::MoveUp as usize] = *up;
            controls.keys[Control::MoveDown as usize] = *down;
        }
        for control in Control::ALL {
            let Some(action) = control.action().and_then(|name| map.get(name)) else {
                continue;
            };
            // The chosen key comes first, then the chosen button (if any),
            // then the fixed alternate key.
            let sources = action.buttons();
            if let Some(ButtonBinding::Key(key)) = sources.first() {
                controls.keys[control as usize] = *key;
            }
            controls.pads[control as usize] = sources.iter().find_map(|b| match b {
                ButtonBinding::Gamepad { button, .. } => Some(*button),
                _ => None,
            });
        }
        controls
    }
    // ANCHOR_END: from_bindings

    // ANCHOR: set_key
    /// Gives `control` the key, unless the key is not allowed. Nothing changes on refusal.
    pub fn set_key(&mut self, control: Control, key: Key) -> Result<(), Refused> {
        if matches!(key, Key::Escape | Key::Tab) {
            return Err(Refused::Reserved);
        }
        if FIXED_KEYS.contains(&key) {
            return Err(Refused::Fixed);
        }
        if let Some(other) = Control::ALL.into_iter().find(|&c| c != control && self.key(c) == key) {
            return Err(Refused::InUse(other));
        }
        self.keys[control as usize] = key;
        Ok(())
    }
    // ANCHOR_END: set_key

    /// Gives `control` the gamepad button, unless it is not allowed.
    pub fn set_pad(&mut self, control: Control, button: ponko::input::GamepadButton) -> Result<(), Refused> {
        if control.is_movement() {
            return Err(Refused::NoButton);
        }
        if is_reserved_pad(button) {
            return Err(Refused::Reserved);
        }
        if let Some(other) = Control::ALL
            .into_iter()
            .find(|&c| c != control && self.pad(c) == Some(button))
        {
            return Err(Refused::InUse(other));
        }
        self.pads[control as usize] = Some(button);
        Ok(())
    }

    /// Whether a settings file could have been written by this game: no key
    /// or button twice, no reserved or fixed key, no button on a movement.
    pub fn is_valid(&self) -> bool {
        let mut keys = HashSet::new();
        let mut pads = HashSet::new();
        Control::ALL.into_iter().all(|c| {
            let key = self.key(c);
            let key_ok = !FIXED_KEYS.contains(&key)
                && key != Key::Tab
                && (key != Key::Escape || c == Control::Pause)
                && keys.insert(key);
            let pad_ok = match self.pad(c) {
                None => true,
                // East is Back's default, so it is allowed there and nowhere else.
                Some(button) => {
                    !c.is_movement()
                        && (!is_reserved_pad(button)
                            || (button == ponko::input::GamepadButton::East && c == Control::Back))
                        && pads.insert(button)
                }
            };
            key_ok && pad_ok
        })
    }
}

/// A key as the player reads it: `Enter`, `Esc`, `1`.
pub fn key_name(key: Key) -> String {
    match key {
        Key::Escape => "Esc".to_string(),
        key => {
            let name = format!("{key:?}");
            match name.strip_prefix("Num") {
                Some(digit) if digit.len() == 1 => digit.to_string(),
                _ => name,
            }
        }
    }
}

// ANCHOR: bindings_for
/// The input bindings for `controls`: the chosen keys and buttons, the fixed
/// alternates, and the left stick.
pub fn bindings_for(controls: &Controls) -> ActionMap {
    let pad = ponko::input::Gamepad::from_slot(0).expect("slot 0 exists");
    let key = |c| controls.key(c);
    let mut map = ActionMap::default();
    map.set(
        "move",
        Action::Axis2(vec![
            Axis2Binding::Keys {
                left: key(Control::MoveLeft),
                right: key(Control::MoveRight),
                up: key(Control::MoveUp),
                down: key(Control::MoveDown),
            },
            Axis2Binding::Keys {
                left: Key::Left,
                right: Key::Right,
                up: Key::Up,
                down: Key::Down,
            },
            Axis2Binding::Gamepad {
                pad,
                stick: ponko::input::GamepadStick::Left,
                deadzone: 0.2,
            },
        ]),
    );
    let button = |control: Control, extra: Option<Key>| {
        let mut list = vec![ButtonBinding::Key(key(control))];
        if let Some(button) = controls.pad(control) {
            list.push(ButtonBinding::Gamepad { pad, button });
        }
        list.extend(extra.map(ButtonBinding::Key));
        Action::Button(list)
    };
    let fixed = |control| match control {
        Control::Confirm => Some(Key::Space),
        Control::Pause => Some(Key::P),
        _ => None,
    };
    for control in Control::ALL {
        if let Some(name) = control.action() {
            map.set(name, button(control, fixed(control)));
        }
    }
    map
}
// ANCHOR_END: bindings_for

/// What the player chose. A resource; the saved copy is `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// One of [`SCALES`].
    pub ui_scale: f32,
    /// Sound effects, `0.0..=1.0`.
    pub volume: f32,
    pub controls: Controls,
    /// The language's code (`"en"`, `"ar"`); one the game has no table for
    /// (or can't show, see [`languages`]) leaves it in English.
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            ui_scale: 1.0,
            volume: 0.8,
            controls: Controls::default(),
            language: "en".to_string(),
        }
    }
}

impl Settings {
    /// Reads `path`: like the progress file, a missing file is a first run and
    /// a damaged or impossible one is replaced by the defaults (and kept as `.bad`).
    pub fn load(storage: &mut Storage) -> (Settings, LoadStatus) {
        load_json(storage, SETTINGS_FILE, Settings::is_valid)
    }

    pub fn save(&self, storage: &mut Storage) -> Result<(), Error> {
        storage.write_json(SETTINGS_FILE, self)
    }

    fn is_valid(&self) -> bool {
        self.version == SETTINGS_VERSION
            && SCALES.contains(&self.ui_scale)
            && self.volume.is_finite()
            && (0.0..=1.0).contains(&self.volume)
            && self.controls.is_valid()
            && !self.language.is_empty()
    }
}

/// The settings file's name in `ctx.storage`, beside the progress file.
pub const SETTINGS_FILE: &str = "settings.json";

/// What happened to the settings file. A resource.
#[derive(Debug, Clone)]
pub struct SettingsFile {
    pub load: LoadStatus,
    pub save_failed: bool,
}

/// Writes the settings and returns to the title screen. Called by `rules`.
pub fn leave_settings(ctx: &mut Ctx) {
    let settings = ctx.world.resource::<Settings>().clone();
    let failed = ctx.storage.is_available() && settings.save(&mut ctx.storage).is_err();
    ctx.world.resource_mut::<SettingsFile>().save_failed = failed;
    ctx.world.insert_resource(Screen::Title);
}

// ---- The settings screen ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Key,
    Pad,
}

/// A rebinding in progress: the prompt is open and the next press is the answer.
#[derive(Debug, Clone, Copy)]
struct Capture {
    control: Control,
    device: Device,
    /// False in the frame the prompt opened: the press that opened it (Enter on
    /// the focused button) is still "pressed this frame" and must not answer it.
    armed: bool,
}

/// The languages the settings offer, in the order the language button
/// cycles through them: every table the game loaded, except right-to-left
/// ones in a build without the engine's `text-shaping` feature (their
/// letters would not join, nor run right to left). The packaged game and the
/// web build have it.
pub fn languages(strings: &Strings) -> Vec<String> {
    strings
        .languages()
        .into_iter()
        .filter(|l| cfg!(feature = "text-shaping") || !strings.table(l).is_some_and(|t| t.is_rtl()))
        .map(str::to_string)
        .collect()
}

/// The language after `current` in [`languages`], wrapping around.
pub fn next_language(strings: &Strings, current: &str) -> String {
    let all = languages(strings);
    let at = all.iter().position(|l| l == current).map_or(0, |i| i + 1);
    all.get(at % all.len().max(1))
        .cloned()
        .unwrap_or_else(|| "en".to_string())
}

struct Row {
    control: Control,
    key: UiId,
    pad: Option<UiId>,
}

/// The settings screen's widgets. A resource, built the first time the screen opens.
pub struct SettingsUi {
    root: UiId,
    prompt: UiId,
    prompt_text: UiId,
    status: UiId,
    scale_label: UiId,
    scale_down: UiId,
    scale_up: UiId,
    volume: UiId,
    reset: UiId,
    back: UiId,
    language: UiId,
    rows: Vec<Row>,
    /// The [`Strings::revision`] the accessible names were written for.
    revision: u64,
    /// The screen is pushed on the UI's stack.
    open: bool,
    capture: Option<Capture>,
}

impl SettingsUi {
    fn build(ui: &mut Ui, settings: &Settings) -> SettingsUi {
        let root = ui
            .root()
            .layout(ponko::ui::UiLayout::fill().padding(16.0).gap(6.0))
            .background(Color::hex(0x14181f))
            .name("settings")
            .id();
        ui.label(root, "")
            .text_key("settings.title")
            .layout(ponko::ui::UiLayout::row(40.0))
            .size(28.0);

        let strip = ui.panel(root).layout(ponko::ui::UiLayout::row(34.0).gap(8.0)).id();
        ui.label(strip, "")
            .text_key("settings.scale")
            .layout(ponko::ui::UiLayout::fixed(150.0, 34.0));
        let scale_down = ui
            .button(strip, "-")
            .layout(ponko::ui::UiLayout::fixed(40.0, 34.0))
            .id();
        let scale_label = ui
            .label(strip, "")
            .layout(ponko::ui::UiLayout::fixed(70.0, 34.0))
            .name("settings.scale")
            .id();
        let scale_up = ui
            .button(strip, "+")
            .layout(ponko::ui::UiLayout::fixed(40.0, 34.0))
            .name("settings.scale_up")
            .id();

        let strip = ui.panel(root).layout(ponko::ui::UiLayout::row(34.0).gap(8.0)).id();
        ui.label(strip, "")
            .text_key("settings.volume")
            .layout(ponko::ui::UiLayout::fixed(150.0, 34.0));
        let volume = ui
            .slider(strip)
            .layout(ponko::ui::UiLayout::fixed(240.0, 34.0))
            .value(settings.volume)
            .id();

        let strip = ui.panel(root).layout(ponko::ui::UiLayout::row(34.0).gap(8.0)).id();
        ui.label(strip, "")
            .text_key("settings.language")
            .layout(ponko::ui::UiLayout::fixed(150.0, 34.0));
        let language = ui
            .button(strip, "")
            .layout(ponko::ui::UiLayout::fixed(160.0, 34.0))
            .name("settings.language")
            .id();

        ui.label(root, "")
            .text_key("settings.hint")
            .layout(ponko::ui::UiLayout::row(52.0))
            .size(16.0)
            .wrap();
        let scroll = ui.scroll_area(root).layout(ponko::ui::UiLayout::fill()).id();
        let list = ui
            .panel(scroll)
            .layout(ponko::ui::UiLayout::auto().vertical().gap(4.0).width_fraction(0.95))
            .id();
        let mut rows = Vec::new();
        for control in Control::ALL {
            let strip = ui.panel(list).layout(ponko::ui::UiLayout::row(32.0).gap(8.0)).id();
            ui.label(strip, "")
                .text_key(control.key())
                .layout(ponko::ui::UiLayout::fixed(150.0, 32.0));
            let key = ui
                .button(strip, "")
                .layout(ponko::ui::UiLayout::fixed(120.0, 32.0))
                .id();
            let pad = if control.is_movement() {
                ui.label(strip, "")
                    .text_key("settings.left_stick")
                    .layout(ponko::ui::UiLayout::fixed(120.0, 32.0));
                None
            } else {
                Some(
                    ui.button(strip, "")
                        .layout(ponko::ui::UiLayout::fixed(120.0, 32.0))
                        .id(),
                )
            };
            rows.push(Row { control, key, pad });
        }

        let strip = ui.panel(root).layout(ponko::ui::UiLayout::row(36.0).gap(8.0)).id();
        let reset = ui
            .button(strip, "")
            .text_key("settings.reset")
            .layout(ponko::ui::UiLayout::fixed(190.0, 36.0))
            .id();
        let back = ui
            .button(strip, "")
            .text_key("settings.back")
            .layout(ponko::ui::UiLayout::fixed(120.0, 36.0))
            .name("settings.back")
            .id();
        let status = ui
            .label(root, "")
            .layout(ponko::ui::UiLayout::row(24.0))
            .size(16.0)
            .id();

        // The prompt: a modal screen with nothing to focus, so no key can press
        // a button behind it; Escape and East pop it (the engine does that).
        let prompt = ui
            .root()
            .layout(ponko::ui::UiLayout::centered(460.0, 110.0).padding(16.0).gap(8.0))
            .background(Color::hex(0x2b3240))
            .id();
        let prompt_text = ui
            .label(prompt, "")
            .layout(ponko::ui::UiLayout::fixed(420.0, 78.0))
            .wrap()
            .id();

        SettingsUi {
            root,
            prompt,
            prompt_text,
            status,
            scale_label,
            scale_down,
            scale_up,
            volume,
            reset,
            back,
            language,
            rows,
            revision: 0,
            open: false,
            capture: None,
        }
    }

    /// Shows the current settings in the labels and buttons: the scale and
    /// volume from `settings`, the keys and buttons read back from the
    /// installed `bindings`, and the names a screen reader says, in the
    /// current language of `strings`. (Labels that only hold a key follow the
    /// language by themselves.)
    fn refresh(&mut self, ui: &mut Ui, settings: &Settings, bindings: &ActionMap, strings: &Strings) {
        self.revision = strings.revision();
        ui.set_text(
            self.scale_label,
            strings.number(f64::from(settings.ui_scale) * 100.0, Some(0)) + "%",
        );
        ui.set_value(self.volume, settings.volume);
        ui.set_accessible_name(self.scale_down, strings.get("settings.smaller"));
        ui.set_accessible_name(self.scale_up, strings.get("settings.larger"));
        ui.set_accessible_name(self.volume, strings.get("settings.volume"));
        let language = strings
            .table(strings.language())
            .map_or(strings.language(), |t| t.name());
        ui.set_text(self.language, language);
        ui.set_accessible_name(
            self.language,
            format!("{}: {language}", strings.get("settings.language")),
        );
        let bound = Controls::from_bindings(bindings);
        // A button's name is its text unless told otherwise; "Enter" alone says nothing.
        for row in &self.rows {
            let key = bound.key_name(row.control);
            let control = row.control.label(strings);
            ui.set_text(row.key, &key);
            let args = [("control", Arg::from(&control)), ("key", Arg::from(&key))];
            ui.set_accessible_name(row.key, strings.format("settings.key_name", &args));
            if let (Some(id), Some(button)) = (row.pad, bound.pad(row.control)) {
                let button = format!("{button:?}");
                ui.set_text(id, &button);
                let args = [("control", Arg::from(&control)), ("button", Arg::from(button))];
                ui.set_accessible_name(id, strings.format("settings.pad_name", &args));
            }
        }
    }
}

/// The sentence a refused key or button gets: its string table key and
/// arguments, so the prompt follows the language too.
pub fn refusal(refused: Refused, name: &str, strings: &Strings) -> (&'static str, Vec<(&'static str, Arg)>) {
    let key = ("key", Arg::from(name));
    match refused {
        Refused::Reserved => ("refused.reserved", vec![key]),
        Refused::Fixed => ("refused.fixed", vec![key]),
        Refused::InUse(other) => (
            "refused.in_use",
            vec![key, ("control", Arg::from(other.label(strings)))],
        ),
        Refused::NoButton => ("refused.no_button", Vec::new()),
    }
}

/// The next step of a UI scale: `+1` larger, `-1` smaller, stopping at the ends.
pub fn step_scale(current: f32, direction: i32) -> f32 {
    let index = SCALES.iter().position(|&s| s == current).unwrap_or(1) as i32;
    SCALES[(index + direction).clamp(0, SCALES.len() as i32 - 1) as usize]
}

// ANCHOR: settings_ui
/// Frame system: the settings screen. Runs after the engine has processed this
/// frame's UI input, so the focus has moved and the events are waiting.
///
/// It opens the screen when the game is on `Screen::Settings`, turns the UI's
/// events into changes of the [`Settings`] (and `Intent::close_settings` for Back
/// or Escape), runs the rebinding prompt, and closes the screen again when the
/// game has left it.
pub fn settings_ui(ctx: &mut Ctx) {
    let on_screen = *ctx.world.resource::<Screen>() == Screen::Settings;
    if ctx.world.get_resource::<SettingsUi>().is_none() {
        if !on_screen {
            return;
        }
        let settings = ctx.world.resource::<Settings>().clone();
        let built = SettingsUi::build(&mut ctx.ui, &settings);
        ctx.world.insert_resource(built);
    }
    let mut state = ctx.world.remove_resource::<SettingsUi>().expect("built above");
    drive(ctx, &mut state, on_screen);
    ctx.world.insert_resource(state);
}
// ANCHOR_END: settings_ui

fn drive(ctx: &mut Ctx, state: &mut SettingsUi, on_screen: bool) {
    if !on_screen {
        if state.open {
            // Left through the Back button (or `Intent`): take the screens off the stack.
            while ctx.ui.top_screen().is_some() {
                ctx.ui.pop_screen();
            }
            state.open = false;
            state.capture = None;
        }
        return;
    }
    if !state.open {
        let settings = ctx.world.resource::<Settings>().clone();
        state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
        ctx.ui.set_text(state.status, "");
        ctx.ui.push_screen(state.root, false);
        state.open = true;
    }

    // One at a time: the body changes the tree between events.
    while let Some(event) = ctx.ui.next_event() {
        match event {
            ponko::ui::UiEvent::Clicked(id) => click(ctx, state, id),
            ponko::ui::UiEvent::Changed(id) if id == state.volume => {
                let volume = ctx.ui.value(id).unwrap_or(0.8).clamp(0.0, 1.0);
                ctx.world.resource_mut::<Settings>().volume = volume;
            }
            ponko::ui::UiEvent::ScreenPopped(id) if id == state.prompt => state.capture = None,
            // Escape (or East) on the settings screen: leave, as the Back button does.
            ponko::ui::UiEvent::ScreenPopped(id) if id == state.root => {
                state.open = false;
                ctx.world.resource_mut::<Intent>().close_settings = true;
            }
            _ => {}
        }
    }
    if let Some(capture) = state.capture {
        capture_press(ctx, state, capture);
    }
    // A table reloaded while the screen is up: the spoken names follow.
    if state.revision != strings(&ctx.world).revision() {
        let settings = ctx.world.resource::<Settings>().clone();
        state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
    }
}

fn click(ctx: &mut Ctx, state: &mut SettingsUi, id: UiId) {
    if id == state.back {
        ctx.world.resource_mut::<Intent>().close_settings = true;
    } else if id == state.scale_down || id == state.scale_up {
        let settings = ctx.world.resource_mut::<Settings>();
        settings.ui_scale = step_scale(settings.ui_scale, if id == state.scale_up { 1 } else { -1 });
        let settings = settings.clone();
        ctx.ui.set_scale(settings.ui_scale);
        state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
    } else if id == state.language {
        let next = next_language(strings(&ctx.world), &ctx.world.resource::<Settings>().language);
        ctx.world.resource_mut::<Settings>().language = next;
        let settings = ctx.world.resource::<Settings>().clone();
        apply_language(&mut ctx.world, &settings);
        state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
    } else if id == state.reset {
        let settings = Settings {
            // The language is not a control: Reset keeps it.
            language: ctx.world.resource::<Settings>().language.clone(),
            ..Settings::default()
        };
        apply(ctx, &settings);
        state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
        ctx.world.insert_resource(settings);
        ctx.ui.set_text_key(state.status, "settings.restored", &[]);
    } else if let Some((control, device)) = state.rows.iter().find_map(|row| {
        if row.key == id {
            Some((row.control, Device::Key))
        } else if row.pad == Some(id) {
            Some((row.control, Device::Pad))
        } else {
            None
        }
    }) {
        let prompt = match device {
            Device::Key => "settings.prompt_key",
            Device::Pad => "settings.prompt_pad",
        };
        let name = control.label(strings(&ctx.world));
        ctx.ui
            .set_text_key(state.prompt_text, prompt, &[("control", Arg::from(name))]);
        ctx.ui.push_screen(state.prompt, true);
        state.capture = Some(Capture {
            control,
            device,
            armed: false,
        });
    }
}

/// Installs the bindings and scale of `settings`.
fn apply(ctx: &mut Ctx, settings: &Settings) {
    ctx.input.set_actions(bindings_for(&settings.controls));
    ctx.ui.set_scale(settings.ui_scale);
}

/// Switches the string tables to `settings`' language, or English if the
/// game can't show it (see [`languages`]). The labels follow on the next frame.
pub fn apply_language(world: &mut World, settings: &Settings) {
    let strings = world.resource_mut::<Strings>();
    let language = if languages(strings).contains(&settings.language) {
        settings.language.as_str()
    } else {
        "en"
    };
    strings.set_language(language);
}

/// What the player pressed while the prompt was open, if anything: the first
/// new press of the device the prompt asked for. The engine reports every key
/// and button that went down this frame, so the screen lists none of them.
fn pressed(ctx: &Ctx, device: Device) -> Option<(String, Result<Key, ponko::input::GamepadButton>)> {
    ctx.input.pressed_this_frame().find_map(|press| match (device, press) {
        (Device::Key, PressedInput::Button(ButtonBinding::Key(key))) => Some((format!("{key:?}"), Ok(key))),
        (Device::Pad, PressedInput::Button(ButtonBinding::Gamepad { button, .. })) => {
            Some((format!("{button:?}"), Err(button)))
        }
        _ => None,
    })
}

fn capture_press(ctx: &mut Ctx, state: &mut SettingsUi, capture: Capture) {
    if !capture.armed {
        state.capture = Some(Capture { armed: true, ..capture });
        return;
    }
    let Some((name, press)) = pressed(ctx, capture.device) else {
        return;
    };
    // ANCHOR: rebind
    // What is bound now, read back from the installed map: no copy to drift.
    let mut controls = Controls::from_bindings(ctx.input.actions());
    let result = match press {
        Ok(key) => controls.set_key(capture.control, key),
        Err(button) => controls.set_pad(capture.control, button),
    };
    match result {
        Ok(()) => {
            // Replace only the action this control feeds; its id stays valid.
            let action_name = capture.control.action().unwrap_or("move");
            let action = bindings_for(&controls).get(action_name).cloned();
            ctx.input
                .set_action(action_name, action.expect("bindings_for sets every action"));
            // The saved copy is read back from the map, too.
            let mut settings = ctx.world.resource::<Settings>().clone();
            settings.controls = Controls::from_bindings(ctx.input.actions());
            // ANCHOR_END: rebind
            state.refresh(&mut ctx.ui, &settings, ctx.input.actions(), strings(&ctx.world));
            let control = capture.control.label(strings(&ctx.world));
            ctx.world.insert_resource(settings);
            let args = [("control", Arg::from(control)), ("key", Arg::from(name))];
            ctx.ui.set_text_key(state.status, "settings.bound", &args);
            ctx.ui.pop_screen(); // the prompt
            state.capture = None;
        }
        // Stay in the prompt and say why: pressing Escape still cancels.
        Err(refused) => {
            let (key, args) = refusal(refused, &name, strings(&ctx.world));
            ctx.ui.set_text_key(state.prompt_text, key, &args);
        }
    }
}
