//! Named, rebindable input. Configuration allocates; evaluating input does not.

#![deny(missing_docs)]

use crate::input::Gamepad;
use crate::input::GamepadAxis;
use crate::input::GamepadButton;
use crate::input::GamepadStick;
use crate::input::touch::{Gesture, GestureAxis, TouchState, ZoneKind};
use crate::input::{ButtonStates, PadState};
use crate::input::{Key, MAX_GAMEPADS, MouseButton};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A digital source. Gamepad slots are stable while connected.
/// ```
/// use ponko::prelude::{ButtonBinding, Key};
/// let binding = ButtonBinding::Key(Key::Space);
/// ```
///
/// External matches must handle future variants:
/// ```compile_fail,E0004
/// use ponko::actions::ButtonBinding;
/// fn exhaustive(value: ButtonBinding) {
///     match value { ButtonBinding::Key(_) => (), ButtonBinding::Mouse(_) => (), ButtonBinding::Gamepad { .. } => () }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ButtonBinding {
    /// Physical keyboard key.
    Key(Key),
    /// Mouse button.
    Mouse(MouseButton),
    /// Gamepad slot and button.
    Gamepad {
        /// Slot in `0..MAX_GAMEPADS`.
        pad: Gamepad,
        /// Button to read.
        button: GamepadButton,
    },
    /// A touch gesture: down while it holds (drag, long press, pinch,
    /// rotate) or for the frame it happens in (tap, double tap, swipes).
    /// ```
    /// use ponko::prelude::*;
    /// let jump = Action::Button(vec![ButtonBinding::Key(Key::Space), ButtonBinding::Gesture(Gesture::SwipeUp)]);
    /// ```
    Gesture(Gesture),
}

/// A scalar source. Gamepad values inside the deadzone read zero; outside
/// they are rescaled continuously to full range. Invalid deadzones read zero.
/// ```
/// use ponko::prelude::{AxisBinding, Key};
/// let binding = AxisBinding::Keys { negative: Key::A, positive: Key::D };
/// ```
///
/// External matches must handle future variants:
/// ```compile_fail,E0004
/// use ponko::actions::AxisBinding;
/// fn exhaustive(value: AxisBinding) {
///     match value { AxisBinding::Keys { .. } => (), AxisBinding::Gamepad { .. } => () }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AxisBinding {
    /// Opposing keys cancel.
    Keys {
        /// Key for -1.
        negative: Key,
        /// Key for +1.
        positive: Key,
    },
    /// Stick component or trigger, with a deadzone in `0..1`.
    Gamepad {
        /// Controller slot.
        pad: Gamepad,
        /// Component or trigger.
        axis: GamepadAxis,
        /// Deadzone in `0..1`.
        deadzone: f32,
    },
    /// A touch gesture's change this frame (pinch, rotation or drag),
    /// times `scale`. Not clamped: like the mouse wheel, it is an amount
    /// per frame, zero in a frame where the fingers didn't move.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::GestureAxis;
    /// // Spreading two fingers by 10 % zooms in by one step.
    /// let zoom = Action::Axis(vec![AxisBinding::Gesture { axis: GestureAxis::Pinch, scale: 10.0 }]);
    /// ```
    Gesture {
        /// Which amount.
        axis: GestureAxis,
        /// What it is multiplied by.
        scale: f32,
    },
}

/// A vector source. Keyboard diagonals are normalized; sticks use a radial
/// deadzone and retain their direction. Y is positive down.
/// ```
/// use ponko::prelude::{Axis2Binding, Key};
/// let binding = Axis2Binding::Keys { left: Key::A, right: Key::D, up: Key::W, down: Key::S };
/// ```
///
/// External matches must handle future variants:
/// ```compile_fail,E0004
/// use ponko::actions::Axis2Binding;
/// fn exhaustive(value: Axis2Binding) {
///     match value { Axis2Binding::Keys { .. } => (), Axis2Binding::Gamepad { .. } => () }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Axis2Binding {
    /// Four physical keys (for example WASD).
    Keys {
        /// Negative x.
        left: Key,
        /// Positive x.
        right: Key,
        /// Negative y.
        up: Key,
        /// Positive y.
        down: Key,
    },
    /// Gamepad stick with a radial deadzone in `0..1`.
    Gamepad {
        /// Controller slot.
        pad: Gamepad,
        /// Stick to read.
        stick: GamepadStick,
        /// Deadzone in `0..1`.
        deadzone: f32,
    },
    /// A one-finger touch drag's movement this frame, in window pixels,
    /// times `scale` (y down). Not clamped: an amount per frame, like
    /// [`Input::mouse_delta`](crate::input::Input::mouse_delta), for
    /// turning a camera.
    /// ```
    /// use ponko::prelude::*;
    /// let look = Action::Axis2(vec![Axis2Binding::Drag { scale: 0.01 }]);
    /// ```
    Drag {
        /// What the movement is multiplied by.
        scale: f32,
    },
}

/// The axis value a stick or trigger must reach to count as "pressed" in
/// [`Input::pressed_this_frame`](crate::input::Input::pressed_this_frame).
pub const PRESS_THRESHOLD: f32 = 0.5;

/// The deadzone of the [`AxisBinding`] that
/// [`Input::pressed_this_frame`](crate::input::Input::pressed_this_frame)
/// reports for a gamepad axis: under the [`PRESS_THRESHOLD`], so a bound axis
/// still reaches full range.
pub const DEFAULT_DEADZONE: f32 = 0.15;

/// One thing that went down this frame, as a binding an [`ActionMap`] accepts:
/// what [`Input::pressed_this_frame`](crate::input::Input::pressed_this_frame)
/// yields, for a rebinding screen to bind directly.
/// ```
/// use ponko::prelude::*;
/// let pressed = PressedInput::Button(ButtonBinding::Key(Key::Space));
/// assert_eq!(pressed.button(), Some(&ButtonBinding::Key(Key::Space)));
/// assert_eq!(pressed.axis(), None);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PressedInput {
    /// A key, mouse button or gamepad button: the source of an `Action::Button`.
    Button(ButtonBinding),
    /// A gamepad stick axis or trigger past the threshold: the source of an
    /// `Action::Axis`.
    Axis(AxisBinding),
}
impl PressedInput {
    /// The button binding, for a key, mouse button or gamepad button.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let pressed = PressedInput::Button(ButtonBinding::Key(Key::Space));
    /// assert_eq!(pressed.button(), Some(&ButtonBinding::Key(Key::Space)));
    /// ```
    pub fn button(&self) -> Option<&ButtonBinding> {
        match self {
            PressedInput::Button(b) => Some(b),
            _ => None,
        }
    }
    /// The axis binding, for a stick or trigger.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let pressed = PressedInput::Button(ButtonBinding::Key(Key::Space));
    /// assert!(pressed.axis().is_none()); // a key is not an axis
    /// ```
    pub fn axis(&self) -> Option<&AxisBinding> {
        match self {
            PressedInput::Axis(a) => Some(a),
            _ => None,
        }
    }
}

/// An action's kind and sources. Buttons OR together; axes select the source
/// with greatest magnitude (first wins ties), avoiding doubled movement.
/// ```
/// use ponko::prelude::{Action, ButtonBinding, Key};
/// let jump = Action::Button(vec![ButtonBinding::Key(Key::Space)]);
/// ```
///
/// External matches must handle future variants:
/// ```compile_fail,E0004
/// use ponko::actions::Action;
/// fn exhaustive(value: Action) {
///     match value { Action::Button(_) => (), Action::Axis(_) => (), Action::Axis2(_) => () }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Action {
    /// Digital sources.
    Button(Vec<ButtonBinding>),
    /// Scalar sources.
    Axis(Vec<AxisBinding>),
    /// Vector sources.
    Axis2(Vec<Axis2Binding>),
}
impl Action {
    /// The sources of a button action; empty for an axis.
    /// ```
    /// # use ponko::prelude::*;
    /// let jump = Action::Button(vec![ButtonBinding::Key(Key::Space)]);
    /// assert_eq!(jump.buttons(), [ButtonBinding::Key(Key::Space)]);
    /// assert!(jump.axes().is_empty());
    /// ```
    pub fn buttons(&self) -> &[ButtonBinding] {
        match self {
            Action::Button(b) => b,
            _ => &[],
        }
    }
    /// The same action with every gamepad binding moved to `pad`: write the
    /// bindings once for player one, then install a copy per player
    /// (`"jump.p2"` from `jump.for_pad(pad)`). Keyboard and mouse bindings
    /// stay as they are.
    /// ```
    /// # use ponko::prelude::*;
    /// let p1 = Gamepad::from_slot(0).unwrap();
    /// let p2 = Gamepad::from_slot(1).unwrap();
    /// let jump = Action::Button(vec![ButtonBinding::Gamepad { pad: p1, button: GamepadButton::South }]);
    /// assert_eq!(jump.for_pad(p2).buttons(), [ButtonBinding::Gamepad { pad: p2, button: GamepadButton::South }]);
    /// ```
    #[must_use]
    pub fn for_pad(&self, pad: Gamepad) -> Action {
        let mut out = self.clone();
        match &mut out {
            Action::Button(bindings) => {
                for b in bindings {
                    if let ButtonBinding::Gamepad { pad: p, .. } = b {
                        *p = pad;
                    }
                }
            }
            Action::Axis(bindings) => {
                for b in bindings {
                    if let AxisBinding::Gamepad { pad: p, .. } = b {
                        *p = pad;
                    }
                }
            }
            Action::Axis2(bindings) => {
                for b in bindings {
                    if let Axis2Binding::Gamepad { pad: p, .. } = b {
                        *p = pad;
                    }
                }
            }
        }
        out
    }
    /// The sources of a scalar axis action; empty for the other kinds.
    /// ```
    /// # use ponko::prelude::*;
    /// let steer = Action::Axis(vec![AxisBinding::Keys { negative: Key::A, positive: Key::D }]);
    /// assert_eq!(steer.axes().len(), 1);
    /// ```
    pub fn axes(&self) -> &[AxisBinding] {
        match self {
            Action::Axis(a) => a,
            _ => &[],
        }
    }
    /// The sources of a vector axis action; empty for the other kinds.
    /// ```
    /// # use ponko::prelude::*;
    /// let walk = Action::Axis2(vec![Axis2Binding::Keys { left: Key::A, right: Key::D, up: Key::W, down: Key::S }]);
    /// assert_eq!(walk.axes2().len(), 1);
    /// ```
    pub fn axes2(&self) -> &[Axis2Binding] {
        match self {
            Action::Axis2(a) => a,
            _ => &[],
        }
    }
}

/// An action resolved once, so a query is an array index instead of a string
/// lookup. Get one from [`Input::action_id`](crate::input::Input::action_id) (or
/// [`actions!`](crate::actions!) for a set), keep it, and pass it where the
/// `&str` forms take a name.
///
/// An id stays valid when bindings are replaced with
/// [`Input::set_actions`](crate::input::Input::set_actions) (rebinding, loading a file):
/// the name keeps its slot. [`ActionId::NONE`] is what resolving an unknown
/// name gives; it reads false/zero. An id belongs to the context it came from.
/// ```
/// # use ponko::prelude::*;
/// Game::new("ids", 1, 1).run_headless(0, |ctx| {
///     let mut map = ActionMap::default();
///     map.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
///     ctx.input.set_actions(map);
///     let jump = ctx.input.action_id("jump");
///     ctx.input.feed(ponko::actions::InputEvent::Key(Key::Space, true));
///     assert!(ctx.input.action_down(jump));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionId(u32);
impl ActionId {
    /// The id of no action: reads false/zero. Resolving an unknown name
    /// returns it (after logging an error).
    pub const NONE: ActionId = ActionId(u32::MAX);
    /// Whether this is [`NONE`](Self::NONE).
    ///
    /// ```
    /// use ponko::actions::ActionId;
    /// use ponko::prelude::*;
    /// let map = ActionMap::default();
    /// assert!(ActionId::NONE.is_none());
    /// assert!(ActionId::default().is_none());
    /// assert!(map.resolve("jump").is_err());
    /// ```
    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
    /// The id as a number, for the C ABI. Not a stable format.
    #[doc(hidden)]
    pub fn to_bits(self) -> u32 {
        self.0
    }
    /// An id from [`to_bits`](Self::to_bits). A number that names no action
    /// of the context reads false/zero, like [`NONE`](Self::NONE).
    #[doc(hidden)]
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}
impl Default for ActionId {
    fn default() -> Self {
        Self::NONE
    }
}

/// Resolving a name that no installed action has. Its message names the
/// known actions, which is usually enough to spot the typo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAction {
    name: String,
    known: Vec<String>,
}
impl UnknownAction {
    /// The name that was asked for.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let err = ActionMap::default().resolve("jmup").unwrap_err();
    /// assert_eq!(err.name(), "jmup");
    /// ```
    pub fn name(&self) -> &str {
        &self.name
    }
}
impl std::fmt::Display for UnknownAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown action {:?}", self.name)?;
        if self.known.is_empty() {
            write!(f, " (no actions are installed; see Input::set_actions)")
        } else {
            write!(f, " (known: {})", self.known.join(", "))
        }
    }
}
impl std::error::Error for UnknownAction {}

/// Something that names an action in a query: an [`ActionId`] (resolved once,
/// the fast form) or a `&str` name (a lookup per call). Implemented for
/// both so `ctx.input.action_down("jump")` and `ctx.input.action_down(jump)` work.
pub trait ActionRef: sealed::Sealed + Copy {
    #[doc(hidden)]
    fn slot(self, map: &ActionMap) -> Option<usize>;
}
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::ActionId {}
    impl Sealed for &str {}
    impl Sealed for &String {}
}
impl ActionRef for ActionId {
    #[inline]
    fn slot(self, map: &ActionMap) -> Option<usize> {
        let i = self.0 as usize;
        (i < map.states.len()).then_some(i)
    }
}
impl ActionRef for &str {
    #[inline]
    fn slot(self, map: &ActionMap) -> Option<usize> {
        map.slots.get(self).map(|&i| i as usize)
    }
}
impl ActionRef for &String {
    #[inline]
    fn slot(self, map: &ActionMap) -> Option<usize> {
        self.as_str().slot(map)
    }
}

/// Declares a set of actions once: a `Copy` struct with one [`ActionId`] per
/// field, resolved in one call, and the names as a constant. No proc macro,
/// and a mistyped field is a compile error instead of a silent `false`.
/// ```
/// # use ponko::prelude::*;
/// actions! {
///     /// The game's actions.
///     pub struct Act { jump = "jump", steer = "steer" }
/// }
/// Game::new("act", 1, 1).run_headless(0, |ctx| {
///     let mut map = ActionMap::default();
///     for name in Act::NAMES {
///         map.set(*name, Action::Button(vec![]));
///     }
///     ctx.input.set_actions(map);
///     let act = Act::resolve(ctx);
///     assert!(!ctx.input.action_down(act.jump));
/// });
/// ```
#[doc(hidden)]
#[macro_export]
macro_rules! actions {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident { $($field:ident = $text:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy)]
        $vis struct $name {
            $(
                #[doc = concat!("The action `", $text, "`.")]
                $vis $field: $crate::actions::ActionId,
            )+
        }
        impl $name {
            /// The action names, in declaration order.
            $vis const NAMES: &'static [&'static str] = &[$($text),+];
            /// Resolves every action once; each unknown name logs an error
            /// and reads false/zero.
            $vis fn resolve(ctx: &$crate::prelude::Ctx) -> Self {
                Self { $($field: ctx.input.action_id($text)),+ }
            }
        }
    };
}

#[derive(Debug, Clone, Default)]
struct State {
    down: bool,
    pressed: bool,
    released: bool,
    axis: f32,
    axis2: Vec2,
}

/// Serializable bindings, ordered by name. Runtime state is never serialized.
/// Equality compares bindings only, ignoring runtime edges and context-local ids.
/// Unknown names and queries of the wrong kind return false/zero; resolve a
/// name once with [`ActionMap::resolve`] or `Input::action_id` to learn of a typo.
///
/// The bindings read back with [`get`](Self::get) and [`iter`](Self::iter),
/// so a rebinding screen can show `ctx.input.actions()` instead of keeping
/// its own copy; [`Input::set_action`](crate::input::Input::set_action) changes one
/// installed action in place.
/// ```
/// use ponko::prelude::{ActionMap, Action, ButtonBinding, Key};
/// let mut map = ActionMap::default();
/// map.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
/// let json = map.to_json().unwrap();
/// let restored = ActionMap::from_json(&json).unwrap();
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionMap {
    bindings: BTreeMap<String, Action>,
    // Runtime state lives in `states`, one slot per name ever installed (an
    // `ActionId` is the slot). `order` is the slot of each binding, in
    // `bindings` order, so refreshing needs no name lookups.
    #[serde(skip)]
    slots: BTreeMap<String, u32>,
    #[serde(skip)]
    states: Vec<State>,
    #[serde(skip)]
    order: Vec<u32>,
}
// Compare persistent configuration, not cached edges or context-local ids.
impl PartialEq for ActionMap {
    fn eq(&self, other: &Self) -> bool {
        self.bindings == other.bindings
    }
}
impl ActionMap {
    /// Add or replace an action. Installation on a context starts with no edges.
    /// ```
    /// # use ponko::prelude::*;
    /// let mut map = ActionMap::default();
    /// map.set("jump", Action::Button(vec![]));
    /// ```
    pub fn set(&mut self, name: impl Into<String>, action: Action) {
        let name = name.into();
        self.bindings.insert(name.clone(), action);
        self.sync();
        let slot = self.slots[&name] as usize;
        self.states[slot] = State::default();
    }
    /// Remove a binding.
    /// ```
    /// # use ponko::prelude::*;
    /// let mut map = ActionMap::default();
    /// map.remove("jump");
    /// ```
    pub fn remove(&mut self, name: &str) {
        self.bindings.remove(name);
        if let Some(&slot) = self.slots.get(name) {
            self.states[slot as usize] = State::default();
        }
        self.sync();
    }
    /// The bindings of one action, or `None` for an unknown name.
    /// ```
    /// # use ponko::prelude::*;
    /// let mut map = ActionMap::default();
    /// map.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    /// assert_eq!(map.get("jump").unwrap().buttons(), [ButtonBinding::Key(Key::Space)]);
    /// assert!(map.get("fire").is_none());
    /// ```
    pub fn get(&self, name: &str) -> Option<&Action> {
        self.bindings.get(name)
    }
    /// Every action and its bindings, ordered by name (the order the JSON
    /// file stores them in, whatever order they were set in).
    /// ```
    /// # use ponko::prelude::*;
    /// let mut map = ActionMap::default();
    /// map.set("jump", Action::Button(vec![]));
    /// map.set("fire", Action::Button(vec![]));
    /// let names: Vec<&str> = map.iter().map(|(name, _)| name).collect();
    /// assert_eq!(names, ["fire", "jump"]);
    /// ```
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Action)> + '_ {
        self.bindings.iter().map(|(name, action)| (name.as_str(), action))
    }
    /// The number of actions.
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ActionMap::default().len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
    /// Whether the map has no actions.
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ActionMap::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
    /// The state slot of an action the map has.
    pub(crate) fn slot_of(&self, name: &str) -> Option<u32> {
        self.bindings
            .contains_key(name)
            .then(|| self.slots.get(name).copied())
            .flatten()
    }
    /// Resolves an action name once, for the id-based queries. An unknown
    /// name is an error that lists the known actions.
    ///
    /// # Errors
    /// [`UnknownAction`] if `name` is not bound in this map.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut map = ActionMap::default();
    /// map.set("jump", Action::Button(vec![]));
    /// assert!(map.resolve("jump").is_ok());
    /// assert!(map.resolve("jmup").is_err());
    /// ```
    pub fn resolve(&self, name: &str) -> Result<ActionId, UnknownAction> {
        match self.slots.get(name) {
            Some(&slot) if self.bindings.contains_key(name) => Ok(ActionId(slot)),
            _ => Err(UnknownAction {
                name: name.to_string(),
                known: self.bindings.keys().cloned().collect(),
            }),
        }
    }
    /// Gives every binding a state slot (new names get the next free one)
    /// and rebuilds the per-binding slot order.
    fn sync(&mut self) {
        for name in self.bindings.keys() {
            if !self.slots.contains_key(name) {
                self.slots.insert(name.clone(), self.states.len() as u32);
                self.states.push(State::default());
            }
        }
        self.order = self.bindings.keys().map(|n| self.slots[n]).collect();
    }
    /// Save bindings to JSON. Use `std::fs::write` to persist them.
    ///
    /// # Errors
    /// [`ErrorKind::Json`](crate::error::ErrorKind::Json) with encoding context.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let json = ActionMap::default().to_json().unwrap();
    /// ```
    pub fn to_json(&self) -> Result<String, crate::error::Error> {
        serde_json::to_string_pretty(self).map_err(|e| crate::error::Error::from(e).context("encoding action map"))
    }
    /// Load bindings from JSON. A malformed document returns
    /// [`ErrorKind::Json`](crate::error::ErrorKind::Json), preserving the decoder
    /// message (including line and column) with action-map context.
    ///
    /// # Errors
    /// [`ErrorKind::Json`](crate::error::ErrorKind::Json) if `json` is not a
    /// valid action map.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let map = ActionMap::from_json(r#"{"bindings":{}}"#).unwrap();
    /// ```
    pub fn from_json(json: &str) -> Result<Self, crate::error::Error> {
        serde_json::from_str(json).map_err(|e| crate::error::Error::from(e).context("decoding action map"))
    }
    /// Installs over `old`: names that had a slot keep it (so ids resolved
    /// earlier stay valid), new names get the next ones; all state restarts.
    pub(crate) fn prepare_after(&mut self, old: &ActionMap) {
        self.slots = old.slots.clone();
        self.states = vec![State::default(); old.states.len()];
        self.sync();
    }
    /// The bindings of the action in state slot `slot` (an `ActionId`).
    pub(crate) fn binding_at(&self, slot: usize) -> Option<&Action> {
        let i = self.order.iter().position(|&o| o as usize == slot)?;
        self.bindings.values().nth(i)
    }
    /// Clears one action's edges, so changing its bindings fires none.
    pub(crate) fn clear_edges(&mut self, name: &str) {
        if let Some(&slot) = self.slots.get(name) {
            let s = &mut self.states[slot as usize];
            s.pressed = false;
            s.released = false;
        }
    }
    pub(crate) fn end_frame(&mut self) {
        for s in &mut self.states {
            s.pressed = false;
            s.released = false;
        }
    }
    #[inline]
    pub(crate) fn down(&self, action: impl ActionRef) -> bool {
        action.slot(self).is_some_and(|i| self.states[i].down)
    }
    #[inline]
    pub(crate) fn pressed(&self, action: impl ActionRef) -> bool {
        action.slot(self).is_some_and(|i| self.states[i].pressed)
    }
    #[inline]
    pub(crate) fn released(&self, action: impl ActionRef) -> bool {
        action.slot(self).is_some_and(|i| self.states[i].released)
    }
    #[inline]
    pub(crate) fn axis(&self, action: impl ActionRef) -> f32 {
        action.slot(self).map_or(0.0, |i| self.states[i].axis)
    }
    #[inline]
    pub(crate) fn axis2(&self, action: impl ActionRef) -> Vec2 {
        action.slot(self).map_or(Vec2::ZERO, |i| self.states[i].axis2)
    }
    pub(crate) fn refresh(
        &mut self,
        keys: &ButtonStates<{ Key::COUNT }>,
        mouse: &ButtonStates<{ MouseButton::COUNT }>,
        pads: &[PadState; MAX_GAMEPADS],
        touch: &TouchState,
    ) {
        let key = |k: Key| f32::from(keys.down[k as usize]);
        let pad_axis = |p: Gamepad, a: GamepadAxis| pads.get(p.slot()).map_or(0.0, |s| s.axes[a as usize]);
        for (action, &slot) in self.bindings.values().zip(&self.order) {
            let s = &mut self.states[slot as usize];
            match action {
                Action::Button(bindings) => {
                    let down = bindings.iter().any(|b| match *b {
                        ButtonBinding::Key(k) => keys.down[k as usize],
                        ButtonBinding::Mouse(b) => mouse.down[b as usize],
                        ButtonBinding::Gamepad { pad, button } => {
                            pads.get(pad.slot()).is_some_and(|s| s.buttons.down[button as usize])
                        }
                        ButtonBinding::Gesture(g) => touch.down(g),
                    }) || touch.zones_for(slot).any(|z| z.held());
                    s.pressed |= !s.down && down;
                    s.released |= s.down && !down;
                    s.down = down;
                }
                Action::Axis(bindings) => {
                    s.axis = 0.0;
                    for b in bindings {
                        let v = match *b {
                            AxisBinding::Keys { negative, positive } => key(positive) - key(negative),
                            AxisBinding::Gamepad { pad, axis, deadzone } => {
                                deadzone_value(pad_axis(pad, axis), deadzone)
                            }
                            AxisBinding::Gesture { axis, scale } => finite(touch.axis(axis) * scale),
                        };
                        if v.abs() > s.axis.abs() {
                            s.axis = v;
                        }
                    }
                    // An on-screen stick across, a button as 1.
                    for zone in touch.zones_for(slot) {
                        let v = match zone.kind {
                            ZoneKind::Stick => zone.value.x,
                            ZoneKind::Button => f32::from(zone.held()),
                        };
                        if v.abs() > s.axis.abs() {
                            s.axis = v;
                        }
                    }
                }
                Action::Axis2(bindings) => {
                    s.axis2 = Vec2::ZERO;
                    for b in bindings {
                        let v = match *b {
                            Axis2Binding::Keys { left, right, up, down } => {
                                Vec2::new(key(right) - key(left), key(down) - key(up)).clamp_length_max(1.0)
                            }
                            Axis2Binding::Gamepad { pad, stick, deadzone } => {
                                let (x, y) = match stick {
                                    GamepadStick::Left => (GamepadAxis::LeftStickX, GamepadAxis::LeftStickY),
                                    GamepadStick::Right => (GamepadAxis::RightStickX, GamepadAxis::RightStickY),
                                };
                                let v = Vec2::new(pad_axis(pad, x), pad_axis(pad, y));
                                let len = v.length();
                                if len.is_finite() && len > 0.0 {
                                    v / len * deadzone_value(len, deadzone)
                                } else {
                                    Vec2::ZERO
                                }
                            }
                            Axis2Binding::Drag { scale } => {
                                let v = touch.drag_delta() * scale;
                                if v.is_finite() { v } else { Vec2::ZERO }
                            }
                        };
                        if v.length_squared() > s.axis2.length_squared() {
                            s.axis2 = v;
                        }
                    }
                    for zone in touch.zones_for(slot).filter(|z| z.kind == ZoneKind::Stick) {
                        if zone.value.length_squared() > s.axis2.length_squared() {
                            s.axis2 = zone.value;
                        }
                    }
                }
            }
        }
    }
}
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}
fn deadzone_value(value: f32, deadzone: f32) -> f32 {
    if !value.is_finite() || !(0.0..1.0).contains(&deadzone) {
        return 0.0;
    }
    value.signum() * ((value.abs().min(1.0) - deadzone).max(0.0) / (1.0 - deadzone))
}

/// The held state of a synthetic digital input, not a one-frame edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonState {
    /// The button is up.
    Released,
    /// The button remains down until released (or focus/device loss).
    Held,
}

/// Raw input for headless tests and replay. Feed before querying actions.
/// A transition from released to held generates a pressed-this-frame edge;
/// repeated held events do not generate another edge. A release generates a
/// released edge. Both edges survive a tap within a frame and clear at its end.
/// Axis events use ordinary float equality: NaN does not equal itself.
/// ```
/// use ponko::prelude::*;
/// use ponko::actions::{ButtonState, InputEvent};
/// let event = InputEvent::key(Key::Space, ButtonState::Held);
/// ```
///
/// External matches must handle future variants:
/// ```compile_fail,E0004
/// use ponko::actions::InputEvent;
/// fn exhaustive(value: InputEvent) {
///     match value { InputEvent::Key(..) => (), InputEvent::Mouse(..) => (), InputEvent::GamepadButton(..) => (), InputEvent::GamepadAxis(..) => (), InputEvent::FocusLost => (), InputEvent::GamepadDisconnected(_) => (), InputEvent::MouseMove(_) => (), InputEvent::MouseWheel(_) => (), InputEvent::GamepadConnected(_) => (), InputEvent::GamepadMotion(..) => () }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum InputEvent {
    /// Key and held state.
    Key(Key, bool),
    /// Mouse button and held state.
    Mouse(MouseButton, bool),
    /// Gamepad slot, button and held state.
    GamepadButton(Gamepad, GamepadButton, bool),
    /// Gamepad slot, axis and value.
    GamepadAxis(Gamepad, GamepadAxis, f32),
    /// Release all keyboard and mouse buttons (focus loss).
    FocusLost,
    /// Release a controller's buttons and center its axes.
    GamepadDisconnected(Gamepad),
    /// The pointer moves to this point of the window, in window pixels from
    /// the top-left corner (`ctx.input.mouse_pos()`). A headless run's
    /// pointer starts at `(0, 0)` and stays wherever this puts it.
    MouseMove(Vec2),
    /// The wheel turns by this much this frame, in lines (notches): `y > 0`
    /// is away from the user (`ctx.input.mouse_wheel()`). Several in one
    /// frame add up.
    MouseWheel(Vec2),
    /// A gamepad connects into this slot (no name, generic family): what
    /// makes it listed by `ctx.input.gamepads()` in a replay or a test
    /// without a backend. Connecting a connected slot does nothing.
    GamepadConnected(Gamepad),
    /// A gyro and accelerometer reading of a gamepad
    /// (`ctx.input.gamepad_motion(pad)`).
    GamepadMotion(Gamepad, crate::input::GamepadMotion),
    /// A finger went down, moved, lifted or was cancelled, at `position`
    /// in window pixels (`ctx.input.touches()`). `id` tells fingers apart
    /// and stays the same from a touch's start to its end. Touches make
    /// gestures and press on-screen controls as the window's touches do.
    /// Non-finite positions are ignored.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::input::TouchPhase;
    /// let down = InputEvent::Touch { id: 1, phase: TouchPhase::Started, position: vec2(10.0, 20.0) };
    /// assert_eq!(down, InputEvent::touch_start(1, vec2(10.0, 20.0)));
    /// ```
    Touch {
        /// Which finger.
        id: u64,
        /// What it did.
        phase: crate::input::TouchPhase,
        /// Where, in window pixels.
        position: Vec2,
    },
}

impl InputEvent {
    /// Set a key's held state. Repeating `Held` does not repeat its pressed edge.
    ///
    /// ```
    /// use ponko::actions::{ButtonState, InputEvent};
    /// use ponko::prelude::*;
    /// assert_eq!(InputEvent::key(Key::Space, ButtonState::Held), InputEvent::Key(Key::Space, true));
    /// assert_eq!(InputEvent::key(Key::Space, ButtonState::Released), InputEvent::Key(Key::Space, false));
    /// ```
    pub const fn key(key: Key, state: ButtonState) -> Self {
        Self::Key(key, matches!(state, ButtonState::Held))
    }
    /// Set a mouse button's held state, with the same edge rules as [`Self::key`].
    ///
    /// ```
    /// use ponko::actions::{ButtonState, InputEvent};
    /// use ponko::prelude::*;
    /// let down = InputEvent::mouse(MouseButton::Left, ButtonState::Held);
    /// assert_eq!(down, InputEvent::Mouse(MouseButton::Left, true));
    /// ```
    pub const fn mouse(button: MouseButton, state: ButtonState) -> Self {
        Self::Mouse(button, matches!(state, ButtonState::Held))
    }
    /// Finger `id` goes down at `position` (window pixels).
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(5.0, 5.0)));
    ///     assert_eq!(ctx.input.touches().len(), 1);
    /// });
    /// ```
    pub const fn touch_start(id: u64, position: Vec2) -> Self {
        Self::Touch {
            id,
            phase: crate::input::TouchPhase::Started,
            position,
        }
    }
    /// Finger `id` moves to `position`.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// let to = InputEvent::touch_move(1, vec2(6.0, 5.0));
    /// ```
    pub const fn touch_move(id: u64, position: Vec2) -> Self {
        Self::Touch {
            id,
            phase: crate::input::TouchPhase::Moved,
            position,
        }
    }
    /// Finger `id` lifts at `position`.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// let up = InputEvent::touch_end(1, vec2(6.0, 5.0));
    /// ```
    pub const fn touch_end(id: u64, position: Vec2) -> Self {
        Self::Touch {
            id,
            phase: crate::input::TouchPhase::Ended,
            position,
        }
    }
    /// Set a gamepad button's held state, with the same edge rules as [`Self::key`].
    pub const fn gamepad_button(pad: Gamepad, button: GamepadButton, state: ButtonState) -> Self {
        Self::GamepadButton(pad, button, matches!(state, ButtonState::Held))
    }
}
