//! Touch input: the fingers on the screen ([`Input::touches`]), the
//! gestures they make ([`Gesture`], [`Input::gesture_events`]) and the
//! on-screen controls that claim them (see [`crate::ui::touch`]).
//!
//! Touches arrive from the window (a phone, a tablet, a touch screen, a
//! browser on any of them) or from [`InputEvent::Touch`](crate::actions::InputEvent::Touch),
//! which is how tests and replays play a game by touch. Positions are in
//! window pixels from the top-left corner, like [`Input::mouse_pos`].
//!
//! Gestures are recognized as the touches move, with the thresholds in
//! [`GestureSettings`]; they feed the action map like keys do
//! ([`ButtonBinding::Gesture`](crate::actions::ButtonBinding::Gesture),
//! [`AxisBinding::Gesture`](crate::actions::AxisBinding::Gesture),
//! [`Axis2Binding::Drag`](crate::actions::Axis2Binding::Drag)). A touch that
//! starts on an on-screen control belongs to that control and makes no
//! gesture. Nothing allocates per frame once the first touches were seen.
#![deny(missing_docs)]

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::Input;

/// How far a touch is along, for [`InputEvent::Touch`](crate::actions::InputEvent::Touch).
///
/// ```
/// use ponko::input::TouchPhase;
/// assert_ne!(TouchPhase::Started, TouchPhase::Ended);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhase {
    /// A finger went down.
    Started,
    /// It moved.
    Moved,
    /// It lifted.
    Ended,
    /// The system took it away (a call came in, the browser started
    /// scrolling): it ends without making a tap or a swipe.
    Cancelled,
}

/// A finger on the screen, from [`Input::touches`].
///
/// ```
/// use ponko::prelude::*;
/// use ponko::actions::InputEvent;
/// Game::new("t", 320, 240).run_headless(0, |ctx| {
///     ctx.input.feed(InputEvent::touch_start(7, vec2(10.0, 20.0)));
///     ctx.input.feed(InputEvent::touch_move(7, vec2(15.0, 20.0)));
///     let finger = ctx.input.touches()[0];
///     assert_eq!(finger.id(), 7);
///     assert_eq!(finger.position(), vec2(15.0, 20.0));
///     assert_eq!(finger.start(), vec2(10.0, 20.0));
///     assert!(!finger.claimed());
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    id: u64,
    position: Vec2,
    start: Vec2,
    /// Where it was when the frame started.
    frame_start: Vec2,
    started_at: f64,
    /// The on-screen control (zone index) it belongs to.
    claim: Option<u16>,
    /// It went farther than the tap distance from its start.
    moved: bool,
    /// It was part of a two-finger gesture: it makes no tap, press or swipe.
    multi: bool,
    /// It made a long press.
    long_pressed: bool,
    /// It moves a control in edit mode (its `claim`).
    moves_control: bool,
}

impl Touch {
    /// The id the system gave this finger, the same from its start to its end.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Where it is now, in window pixels.
    #[must_use]
    pub fn position(&self) -> Vec2 {
        self.position
    }
    /// Where it went down.
    #[must_use]
    pub fn start(&self) -> Vec2 {
        self.start
    }
    /// How far it moved this frame.
    #[must_use]
    pub fn delta(&self) -> Vec2 {
        self.position - self.frame_start
    }
    /// Whether an on-screen control has it (it started on one): such a
    /// touch makes no gestures.
    #[must_use]
    pub fn claimed(&self) -> bool {
        self.claim.is_some()
    }
}

/// A gesture, for [`ButtonBinding::Gesture`](crate::actions::ButtonBinding::Gesture):
/// a bound action is down while the gesture holds (drag, long press, pinch,
/// rotate) or for the one frame it happens in (tap, double tap, swipes).
/// Only touches no on-screen control has make gestures.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::actions::InputEvent;
/// Game::new("t", 320, 240).run_headless(0, |ctx| {
///     ctx.input.set_action("fire", Action::Button(vec![ButtonBinding::Gesture(Gesture::Tap)]));
///     ctx.input.feed(InputEvent::touch_start(1, vec2(100.0, 100.0)));
///     ctx.input.feed(InputEvent::touch_end(1, vec2(101.0, 100.0)));
///     assert!(ctx.input.action_pressed("fire"));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Gesture {
    /// A short touch that lifts near where it started.
    Tap,
    /// A second tap soon after a tap, near it. The second tap is reported
    /// as this instead of a [`Tap`](Self::Tap).
    DoubleTap,
    /// A touch held still for [`GestureSettings::long_press_time`]: held
    /// from then until it lifts.
    LongPress,
    /// One finger moving: held from when it moved farther than
    /// [`GestureSettings::tap_distance`] until it lifts.
    Drag,
    /// Two fingers moving apart or together, past
    /// [`GestureSettings::pinch_distance`]: held until one lifts.
    Pinch,
    /// Two fingers turning, past [`GestureSettings::rotate_angle`]: held
    /// until one lifts.
    Rotate,
    /// A quick drag to the left, at least [`GestureSettings::swipe_distance`]
    /// long: reported when the finger lifts.
    SwipeLeft,
    /// A quick drag to the right.
    SwipeRight,
    /// A quick drag up.
    SwipeUp,
    /// A quick drag down.
    SwipeDown,
}

impl Gesture {
    pub(crate) const COUNT: usize = 10;
    /// Every gesture, in discriminant order.
    pub const ALL: [Gesture; Self::COUNT] = [
        Gesture::Tap,
        Gesture::DoubleTap,
        Gesture::LongPress,
        Gesture::Drag,
        Gesture::Pinch,
        Gesture::Rotate,
        Gesture::SwipeLeft,
        Gesture::SwipeRight,
        Gesture::SwipeUp,
        Gesture::SwipeDown,
    ];
}

/// A gesture's amount this frame, for [`AxisBinding::Gesture`](crate::actions::AxisBinding::Gesture).
/// Like the mouse wheel these are changes, zero in a frame where nothing moved.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::input::GestureAxis;
/// let zoom = Action::Axis(vec![AxisBinding::Gesture { axis: GestureAxis::Pinch, scale: 1.0 }]);
/// assert_eq!(zoom.axes().len(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GestureAxis {
    /// How much a pinch spread this frame: `0.1` is 10 % farther apart,
    /// negative is closer together ([`Input::pinch_scale`] minus one).
    Pinch,
    /// How far two fingers turned this frame, in radians, clockwise on the
    /// screen positive ([`Input::rotation_delta`]).
    Rotate,
    /// A one-finger drag's movement this frame across, in window pixels.
    DragX,
    /// A one-finger drag's movement this frame down, in window pixels.
    DragY,
}

/// A gesture that happened this frame, from [`Input::gesture_events`]:
/// which, and where (a tap's or a press's point, a swipe's start, the middle
/// of a pinch or rotation, a drag's start).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::actions::InputEvent;
/// use ponko::input::GestureEvent;
/// Game::new("t", 320, 240).run_headless(0, |ctx| {
///     ctx.input.feed(InputEvent::touch_start(1, vec2(40.0, 50.0)));
///     ctx.input.feed(InputEvent::touch_end(1, vec2(40.0, 50.0)));
///     assert_eq!(ctx.input.gesture_events(), [GestureEvent { gesture: Gesture::Tap, position: vec2(40.0, 50.0) }]);
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureEvent {
    /// Which gesture.
    pub gesture: Gesture,
    /// Where, in window pixels.
    pub position: Vec2,
}

/// The thresholds gestures are recognized with:
/// [`Input::set_gesture_settings`]. Distances are window pixels, times
/// seconds of game time ([`Time::elapsed`](crate::time::Time::elapsed)), so
/// a headless test recognizes the same gestures as a phone.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::input::GestureSettings;
/// Game::new("t", 320, 240).run_headless(0, |ctx| {
///     ctx.input.set_gesture_settings(GestureSettings { long_press_time: 0.8, ..Default::default() });
///     assert_eq!(ctx.input.gesture_settings().long_press_time, 0.8);
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GestureSettings {
    /// A touch that moves farther than this from its start is a drag, not
    /// a tap or a long press. Default 10.
    pub tap_distance: f32,
    /// A tap lifts within this time. Default 0.3.
    pub tap_time: f32,
    /// A second tap within this time of the first is a double tap. Default 0.3.
    pub double_tap_time: f32,
    /// ...and within this distance of it. Default 40.
    pub double_tap_distance: f32,
    /// A touch held still this long is a long press. Default 0.5.
    pub long_press_time: f32,
    /// A drag at least this long... Default 50.
    pub swipe_distance: f32,
    /// ...that lifts within this time of its start is a swipe. Default 0.5.
    pub swipe_time: f32,
    /// Two fingers whose distance changed by this much are pinching. Default 16.
    pub pinch_distance: f32,
    /// Two fingers that turned by this many radians are rotating. Default 0.2.
    pub rotate_angle: f32,
}

impl Default for GestureSettings {
    fn default() -> Self {
        Self {
            tap_distance: 10.0,
            tap_time: 0.3,
            double_tap_time: 0.3,
            double_tap_distance: 40.0,
            long_press_time: 0.5,
            swipe_distance: 50.0,
            swipe_time: 0.5,
            pinch_distance: 16.0,
            rotate_angle: 0.2,
        }
    }
}

/// The two fingers of a pinch or rotation.
#[derive(Debug, Clone, Copy)]
struct TwoFingers {
    ids: [u64; 2],
    start_distance: f32,
    start_angle: f32,
    last_distance: f32,
    last_angle: f32,
    pinching: bool,
    rotating: bool,
}

/// What an on-screen control is, for claiming touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoneKind {
    /// A stick: its value is where its touch is, from the centre, over the radius.
    Stick,
    /// A button: held while a touch that started on it is down.
    Button,
}

/// An on-screen control's place and state, kept here so a touch is claimed
/// the moment it starts (before it can become a gesture).
#[derive(Debug, Clone)]
pub(crate) struct Zone {
    pub(crate) kind: ZoneKind,
    /// The action it drives.
    pub(crate) action: String,
    /// The action's state slot, once the action map has the name.
    pub(crate) slot: Option<u32>,
    pub(crate) center: Vec2,
    /// The drawn radius: a stick's full deflection.
    pub(crate) radius: f32,
    /// Touches that start within this distance of the centre are its.
    pub(crate) reach: f32,
    /// A stick reads zero this close to its centre (a fraction of the radius).
    pub(crate) deadzone: f32,
    /// The stick's touch.
    pub(crate) touch: Option<u64>,
    /// How many touches hold the button.
    pub(crate) holders: u8,
    /// The stick's value, length at most 1, y down.
    pub(crate) value: Vec2,
    /// Shown (and taking touches).
    pub(crate) shown: bool,
}

impl Zone {
    pub(crate) fn held(&self) -> bool {
        self.holders > 0 || self.touch.is_some()
    }

    fn follow(&mut self, at: Vec2) {
        let offset = (at - self.center) / self.radius.max(1.0);
        let len = offset.length();
        self.value = if !len.is_finite() || len <= self.deadzone {
            Vec2::ZERO
        } else {
            let scaled = ((len - self.deadzone) / (1.0 - self.deadzone).max(1e-3)).min(1.0);
            offset / len * scaled
        };
    }
}

/// Whether on-screen controls take touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchShow {
    /// Shown on touch devices ([`Input::touch_device`]), hidden elsewhere.
    #[default]
    Auto,
    /// Always shown.
    Always,
    /// Never shown.
    Never,
}

/// Every touch, the gestures being recognized and the on-screen controls'
/// zones.
#[derive(Debug, Clone)]
pub(crate) struct TouchState {
    touches: Vec<Touch>,
    now: f64,
    pub(crate) settings: GestureSettings,
    /// The platform says it has a touch screen.
    platform: Option<bool>,
    /// A touch was seen.
    seen: bool,
    events: Vec<GestureEvent>,
    /// Gestures that happened this frame.
    fired: [bool; Gesture::COUNT],
    drag: Option<u64>,
    drag_delta: Vec2,
    two: Option<TwoFingers>,
    pinch_scale: f32,
    rotation: f32,
    last_tap: Option<(f64, Vec2)>,
    pub(crate) zones: Vec<Zone>,
    pub(crate) show: TouchShow,
    /// The on-screen controls the zones are for, and how many times they
    /// were set (the UI rebuilds their nodes when it changes).
    pub(crate) controls: crate::ui::touch::TouchControls,
    pub(crate) generation: u64,
    /// The part of the window the zones are placed in (its safe area, in
    /// window pixels) and the UI scale.
    pub(crate) view: (crate::camera::Rect, f32),
    /// A menu screen is open over the controls: they step aside.
    pub(crate) suspended: bool,
    /// The first free finger, the UI's pointer while it is down.
    primary: Option<u64>,
    /// Where the UI's touch pointer is: the primary finger, or where it lifted.
    pointer: Vec2,
    /// The primary finger went down / lifted (not cancelled) this frame.
    pointer_pressed: bool,
    pointer_released: bool,
    /// A finger was the last pointer used (not the mouse).
    pub(crate) pointer_is_touch: bool,
    /// The player's arrangement of the controls.
    pub(crate) layout: crate::ui::touch::TouchLayout,
    /// The player is moving the controls ([`Input::edit_touch_controls`]).
    editing: bool,
    /// The control being moved, and by what.
    edit: Option<EditDrag>,
    /// The control the player grabbed last while arranging them: the one
    /// the arrange screen resizes.
    picked: Option<u16>,
    /// Where the arrange screen's panel is, in window pixels: a press there
    /// is the panel's, not a control's under it.
    pub(crate) shield: Option<crate::camera::Rect>,
    /// The arrange screen is asked for or open ([`Input::arrange_touch_controls`]).
    pub(crate) arranging: bool,
    /// The arrange screen was asked for and is not built yet: the layout
    /// its Cancel puts back.
    pub(crate) arrange_before: Option<crate::ui::touch::TouchLayout>,
    /// How many times a control was moved (the UI lays them out again).
    pub(crate) moved: u64,
    /// The mouse's left button went down on a control being moved this
    /// frame: the UI doesn't see it.
    mouse_took: bool,
    /// Where a finger lifted last, and the frame it lifted in: mouse
    /// events the system makes up for it are not the mouse's.
    last_lift: Option<(u64, Vec2)>,
    /// Frames ended, for `last_lift`.
    frames: u64,
    /// A trackpad pinch / rotation is going on.
    trackpad_pinch: bool,
    trackpad_rotate: bool,
}

/// What moves a control in edit mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mover {
    Finger(u64),
    Mouse,
}

/// A control the player is moving.
#[derive(Debug, Clone, Copy)]
struct EditDrag {
    zone: u16,
    by: Mover,
    /// From the pointer to the control's centre.
    grab: Vec2,
}

impl Default for TouchState {
    fn default() -> Self {
        Self {
            touches: Vec::new(),
            now: 0.0,
            settings: GestureSettings::default(),
            platform: None,
            seen: false,
            events: Vec::new(),
            fired: [false; Gesture::COUNT],
            drag: None,
            drag_delta: Vec2::ZERO,
            two: None,
            pinch_scale: 1.0,
            rotation: 0.0,
            last_tap: None,
            zones: Vec::new(),
            show: TouchShow::Auto,
            controls: crate::ui::touch::TouchControls::default(),
            generation: 0,
            view: (crate::camera::Rect::default(), 1.0),
            suspended: false,
            primary: None,
            pointer: Vec2::ZERO,
            pointer_pressed: false,
            pointer_released: false,
            pointer_is_touch: false,
            layout: Default::default(),
            editing: false,
            edit: None,
            picked: None,
            shield: None,
            arranging: false,
            arrange_before: None,
            moved: 0,
            mouse_took: false,
            last_lift: None,
            frames: 0,
            trackpad_pinch: false,
            trackpad_rotate: false,
        }
    }
}

impl TouchState {
    pub(crate) fn device(&self) -> bool {
        self.seen || self.platform.unwrap_or(false)
    }

    /// Whether the on-screen controls are up (and take touches).
    pub(crate) fn controls_active(&self) -> bool {
        !self.zones.is_empty()
            && !self.suspended
            && match self.show {
                TouchShow::Always => true,
                TouchShow::Never => false,
                TouchShow::Auto => self.device(),
            }
    }

    /// Whether the controls' nodes are up: they take touches, or the
    /// player is moving them.
    pub(crate) fn controls_drawn(&self) -> bool {
        self.controls_active() || (self.editing && !self.zones.is_empty())
    }

    /// The control the player is moving now.
    pub(crate) fn edited_zone(&self) -> Option<usize> {
        self.edit.map(|e| usize::from(e.zone))
    }

    /// The control the player grabbed last while arranging them.
    pub(crate) fn picked_zone(&self) -> Option<usize> {
        self.picked.map(usize::from)
    }

    /// In edit mode, `by` going down at `at`: the control under it is moved
    /// from now on. Returns it.
    fn start_edit(&mut self, by: Mover, at: Vec2) -> Option<u16> {
        if !self.editing || self.edit.is_some() || self.shield.is_some_and(|r| r.contains(at)) {
            return None;
        }
        let zone = self
            .zones
            .iter()
            .enumerate()
            .filter(|(_, z)| z.shown && at.distance(z.center) <= z.reach)
            .min_by(|(_, a), (_, b)| at.distance(a.center).total_cmp(&at.distance(b.center)))
            .map(|(i, _)| i as u16)?;
        self.edit = Some(EditDrag {
            zone,
            by,
            grab: self.zones[usize::from(zone)].center - at,
        });
        self.picked = Some(zone);
        Some(zone)
    }

    /// The pointer moving a control went to `at`.
    fn drag_edit(&mut self, by: Mover, at: Vec2) {
        let Some(edit) = self.edit.filter(|e| e.by == by) else {
            return;
        };
        if let Some(zone) = self.zones.get_mut(usize::from(edit.zone)) {
            zone.center = crate::ui::touch::drag_to(at + edit.grab, zone.radius, self.view);
            self.moved += 1;
        }
    }

    /// The control being moved is let go: the player's layout keeps it
    /// there, anchored to the nearest corner of the view (its safe area).
    fn finish_edit(&mut self) {
        let Some(edit) = self.edit.take() else {
            return;
        };
        let Some(zone) = self.zones.get(usize::from(edit.zone)) else {
            return;
        };
        let (area, scale) = self.view;
        let scale = scale.max(1e-3);
        let place = crate::ui::touch::TouchPlace::nearest_corner((zone.center - area.min) / scale, area.size() / scale);
        let action = zone.action.clone();
        self.layout.adjust(&action).place = Some(place);
    }

    /// Whether a mouse event at `at` is one the system made up for a
    /// finger (Windows does, for the first finger): a finger is down there
    /// or lifted there this frame or the last.
    fn mouse_for_finger(&self, at: Vec2) -> bool {
        let near = |p: Vec2| p.distance(at) <= 2.0 * self.settings.tap_distance.max(1.0);
        self.touches.iter().any(|t| near(t.position) || near(t.frame_start))
            || self
                .last_lift
                .is_some_and(|(frame, p)| frame + 1 >= self.frames && near(p))
    }

    /// Whether `gesture` happened (or began) this frame.
    pub(crate) fn fired(&self, gesture: Gesture) -> bool {
        self.fired[gesture as usize]
    }

    /// Whether `gesture` holds now or happened this frame.
    pub(crate) fn down(&self, gesture: Gesture) -> bool {
        self.fired[gesture as usize]
            || match gesture {
                Gesture::Drag => self.drag.is_some(),
                Gesture::LongPress => self.touches.iter().any(|t| t.long_pressed && t.claim.is_none()),
                Gesture::Pinch => self.trackpad_pinch || self.two.is_some_and(|t| t.pinching),
                Gesture::Rotate => self.trackpad_rotate || self.two.is_some_and(|t| t.rotating),
                _ => false,
            }
    }

    pub(crate) fn axis(&self, axis: GestureAxis) -> f32 {
        match axis {
            GestureAxis::Pinch => self.pinch_scale - 1.0,
            GestureAxis::Rotate => self.rotation,
            GestureAxis::DragX => self.drag_delta.x,
            GestureAxis::DragY => self.drag_delta.y,
        }
    }

    pub(crate) fn drag_delta(&self) -> Vec2 {
        self.drag_delta
    }

    /// The on-screen controls change: fingers on the old ones keep making
    /// no gestures until they lift.
    pub(crate) fn replace_zones(&mut self, zones: Vec<Zone>) {
        for t in &mut self.touches {
            if t.claim.take().is_some() {
                t.multi = true;
                t.moved = true;
            }
        }
        self.zones = zones;
        self.picked = None;
    }

    /// Gives zones whose action the map has its state slot.
    pub(crate) fn resolve_slots(&mut self, map: &crate::actions::ActionMap) {
        for zone in &mut self.zones {
            if zone.slot.is_none() {
                zone.slot = map.slot_of(&zone.action);
            }
        }
    }

    /// The zones that drive action slot `slot`.
    pub(crate) fn zones_for(&self, slot: u32) -> impl Iterator<Item = &Zone> + '_ {
        self.zones.iter().filter(move |z| z.slot == Some(slot))
    }

    fn fire(&mut self, gesture: Gesture, position: Vec2) {
        self.fired[gesture as usize] = true;
        self.events.push(GestureEvent { gesture, position });
    }

    fn index(&self, id: u64) -> Option<usize> {
        self.touches.iter().position(|t| t.id == id)
    }

    fn start(&mut self, id: u64, at: Vec2) {
        self.seen = true;
        if self.index(id).is_some() {
            // The system reused a live id: the old touch ends first.
            self.end(id, at, true);
        }
        if let Some(zone) = self.start_edit(Mover::Finger(id), at) {
            // Moving a control: no gesture, no action, not the UI's.
            self.touches.push(Touch {
                id,
                position: at,
                start: at,
                frame_start: at,
                started_at: self.now,
                claim: Some(zone),
                moved: false,
                multi: false,
                long_pressed: false,
                moves_control: true,
            });
            return;
        }
        let claim = if self.controls_active() && !self.editing {
            self.claim(at)
        } else {
            None
        };
        let mut touch = Touch {
            id,
            position: at,
            start: at,
            frame_start: at,
            started_at: self.now,
            claim,
            moved: false,
            multi: false,
            long_pressed: false,
            moves_control: false,
        };
        if let Some(zone) = claim.and_then(|z| self.zones.get_mut(usize::from(z))) {
            match zone.kind {
                ZoneKind::Stick => {
                    zone.touch = Some(id);
                    zone.follow(at);
                }
                ZoneKind::Button => zone.holders = zone.holders.saturating_add(1),
            }
            self.touches.push(touch);
            return;
        }
        if self.primary.is_none() {
            self.primary = Some(id);
            self.pointer = at;
            self.pointer_pressed = true;
            self.pointer_is_touch = true;
        }
        // A second free finger: a pinch or rotation may start, and the
        // first finger's drag stops.
        if self.two.is_none()
            && let Some(other) = self.touches.iter_mut().find(|t| t.claim.is_none())
        {
            other.multi = true;
            touch.multi = true;
            let (a, b) = (other.position, at);
            self.two = Some(TwoFingers {
                ids: [other.id, id],
                start_distance: a.distance(b),
                start_angle: angle(a, b),
                last_distance: a.distance(b),
                last_angle: angle(a, b),
                pinching: false,
                rotating: false,
            });
            self.drag = None;
        } else if self.two.is_some() {
            touch.multi = true;
        }
        self.touches.push(touch);
    }

    /// The control under `at`: the nearest whose reach holds it; a stick
    /// already held by another finger takes no second one.
    fn claim(&self, at: Vec2) -> Option<u16> {
        self.zones
            .iter()
            .enumerate()
            .filter(|(_, z)| z.shown && at.distance(z.center) <= z.reach)
            .filter(|(_, z)| !(z.kind == ZoneKind::Stick && z.touch.is_some()))
            .min_by(|(_, a), (_, b)| at.distance(a.center).total_cmp(&at.distance(b.center)))
            .map(|(i, _)| i as u16)
    }

    fn moved(&mut self, id: u64, at: Vec2) {
        let Some(i) = self.index(id) else {
            // A move without a start (the touch began before the game
            // listened): start it here.
            self.start(id, at);
            return;
        };
        let old = self.touches[i].position;
        self.touches[i].position = at;
        if self.primary == Some(id) {
            self.pointer = at;
        }
        if self.touches[i].moves_control {
            self.drag_edit(Mover::Finger(id), at);
            return;
        }
        if let Some(zone) = self.touches[i].claim {
            if let Some(zone) = self.zones.get_mut(usize::from(zone))
                && zone.touch == Some(id)
            {
                zone.follow(at);
            }
            return;
        }
        if let Some(mut two) = self.two
            && two.ids.contains(&id)
        {
            let pos = |id| self.touches.iter().find(|t| t.id == id).map(|t| t.position);
            if let (Some(a), Some(b)) = (pos(two.ids[0]), pos(two.ids[1])) {
                let (distance, turn) = (a.distance(b), angle(a, b));
                let middle = (a + b) / 2.0;
                // Past a threshold, the change counts from where the
                // fingers went down.
                if !two.pinching && (distance - two.start_distance).abs() >= self.settings.pinch_distance {
                    two.pinching = true;
                    two.last_distance = two.start_distance;
                    self.fire(Gesture::Pinch, middle);
                }
                if !two.rotating && wrap(turn - two.start_angle).abs() >= self.settings.rotate_angle {
                    two.rotating = true;
                    two.last_angle = two.start_angle;
                    self.fire(Gesture::Rotate, middle);
                }
                if two.pinching && two.last_distance > 0.0 {
                    self.pinch_scale *= distance / two.last_distance;
                }
                if two.rotating {
                    self.rotation += wrap(turn - two.last_angle);
                }
                two.last_distance = distance;
                two.last_angle = turn;
                self.two = Some(two);
            }
            return;
        }
        let touch = &mut self.touches[i];
        if !touch.moved && touch.position.distance(touch.start) > self.settings.tap_distance {
            touch.moved = true;
            if !touch.multi && self.drag.is_none() {
                self.drag = Some(id);
                let start = touch.start;
                self.fire(Gesture::Drag, start);
                // The drag counts from where the finger went down.
                self.drag_delta += at - start;
                return;
            }
        }
        if self.drag == Some(id) {
            self.drag_delta += at - old;
        }
    }

    fn end(&mut self, id: u64, at: Vec2, cancelled: bool) {
        if self.index(id).is_none() {
            return;
        }
        if !cancelled {
            self.moved(id, at);
        }
        let Some(i) = self.index(id) else {
            return;
        };
        let touch = self.touches.remove(i);
        self.last_lift = Some((self.frames, touch.position));
        if touch.moves_control {
            if self.edit.is_some_and(|e| e.by == Mover::Finger(id)) {
                self.finish_edit();
            }
            return;
        }
        if let Some(zone) = touch.claim {
            if let Some(zone) = self.zones.get_mut(usize::from(zone)) {
                match zone.kind {
                    ZoneKind::Stick => {
                        if zone.touch == Some(id) {
                            zone.touch = None;
                            zone.value = Vec2::ZERO;
                        }
                    }
                    ZoneKind::Button => zone.holders = zone.holders.saturating_sub(1),
                }
            }
            return;
        }
        if self.primary == Some(id) {
            self.primary = None;
            self.pointer_released = !cancelled;
        }
        if self.two.is_some_and(|t| t.ids.contains(&id)) {
            self.two = None;
        }
        let dragging = self.drag == Some(id);
        if dragging {
            self.drag = None;
        }
        if cancelled || touch.multi {
            return;
        }
        let held = (self.now - touch.started_at) as f32;
        let travel = touch.position - touch.start;
        let s = self.settings;
        if dragging && travel.length() >= s.swipe_distance && held <= s.swipe_time {
            let gesture = if travel.x.abs() >= travel.y.abs() {
                if travel.x < 0.0 {
                    Gesture::SwipeLeft
                } else {
                    Gesture::SwipeRight
                }
            } else if travel.y < 0.0 {
                Gesture::SwipeUp
            } else {
                Gesture::SwipeDown
            };
            self.fire(gesture, touch.start);
        } else if !touch.moved && !touch.long_pressed && held <= s.tap_time {
            let double = self.last_tap.is_some_and(|(when, where_)| {
                (self.now - when) as f32 <= s.double_tap_time
                    && where_.distance(touch.position) <= s.double_tap_distance
            });
            if double {
                self.last_tap = None;
                self.fire(Gesture::DoubleTap, touch.position);
            } else {
                self.last_tap = Some((self.now, touch.position));
                self.fire(Gesture::Tap, touch.position);
            }
        }
    }

    /// A trackpad's gesture at `at` (the mouse pointer).
    fn trackpad(&mut self, gesture: Gesture, phase: TouchPhase, amount: f32, at: Vec2) {
        let amount = if amount.is_finite() { amount } else { 0.0 };
        let going = matches!(phase, TouchPhase::Started | TouchPhase::Moved);
        let counts = phase != TouchPhase::Cancelled;
        match gesture {
            Gesture::Pinch => {
                if going && !self.trackpad_pinch {
                    self.fire(Gesture::Pinch, at);
                }
                self.trackpad_pinch = going;
                if counts {
                    self.pinch_scale *= (1.0 + amount).max(0.01);
                }
            }
            Gesture::Rotate => {
                if going && !self.trackpad_rotate {
                    self.fire(Gesture::Rotate, at);
                }
                self.trackpad_rotate = going;
                if counts {
                    self.rotation += amount;
                }
            }
            Gesture::DoubleTap => self.fire(Gesture::DoubleTap, at),
            _ => {}
        }
    }

    /// Every touch ends without a gesture (focus lost).
    pub(crate) fn cancel_all(&mut self) {
        self.trackpad_pinch = false;
        self.trackpad_rotate = false;
        while let Some(t) = self.touches.last() {
            let (id, at) = (t.id, t.position);
            self.end(id, at, true);
        }
    }

    /// The clock moved on: touches held still long enough are long presses.
    pub(crate) fn begin_frame(&mut self, now: f64) -> bool {
        self.now = now;
        let mut fired = false;
        for i in 0..self.touches.len() {
            let t = self.touches[i];
            if t.claim.is_none()
                && !t.moved
                && !t.multi
                && !t.long_pressed
                && (now - t.started_at) as f32 >= self.settings.long_press_time
            {
                self.touches[i].long_pressed = true;
                self.fire(Gesture::LongPress, t.position);
                fired = true;
            }
        }
        fired
    }

    /// The touch pointer went down / lifted this frame.
    pub(crate) fn pointer_edges(&self) -> (bool, bool) {
        (self.pointer_pressed, self.pointer_released)
    }

    pub(crate) fn set_pointer_edges(&mut self, pressed: bool, released: bool) {
        self.pointer_pressed = pressed;
        self.pointer_released = released;
    }

    pub(crate) fn end_frame(&mut self) {
        self.frames += 1;
        self.mouse_took = self.edit.is_some_and(|e| e.by == Mover::Mouse);
        self.pointer_pressed = false;
        self.pointer_released = false;
        self.events.clear();
        self.fired = [false; Gesture::COUNT];
        self.drag_delta = Vec2::ZERO;
        self.pinch_scale = 1.0;
        self.rotation = 0.0;
        for t in &mut self.touches {
            t.frame_start = t.position;
        }
    }

    fn feed(&mut self, id: u64, phase: TouchPhase, at: Vec2) {
        if !at.is_finite() {
            return;
        }
        match phase {
            TouchPhase::Started => self.start(id, at),
            TouchPhase::Moved => self.moved(id, at),
            TouchPhase::Ended => self.end(id, at, false),
            TouchPhase::Cancelled => self.end(id, at, true),
        }
    }
}

/// The direction from `a` to `b`, in radians (clockwise on the screen, y down).
fn angle(a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    d.y.atan2(d.x)
}

/// `a` in `-π..=π`.
fn wrap(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = (a + PI).rem_euclid(TAU) - PI;
    if a.is_finite() { a } else { 0.0 }
}

/// Whether this platform has a touch screen: a phone or tablet, a browser
/// that reports touch points, a Windows or Linux desktop with one.
#[cfg(feature = "window")]
pub(crate) fn platform_has_touch() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().is_some_and(|w| w.navigator().max_touch_points() > 0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        cfg!(any(target_os = "ios", target_os = "android")) || super::touch_desktop::has_touch_screen()
    }
}

/// Keeps the browser from scrolling or zooming the page while fingers are
/// on the game's canvas, so its touches stay the game's.
#[cfg(all(target_arch = "wasm32", feature = "window"))]
pub(crate) fn prepare_canvas(window: &winit::window::Window) {
    use winit::platform::web::WindowExtWebSys;
    if let Some(canvas) = window.canvas() {
        let _ = canvas.style().set_property("touch-action", "none");
    }
}

impl Input {
    /// The mouse's left button went down or up at the pointer: moves an
    /// on-screen control in edit mode. Returns `false` for a press the
    /// system made up for a finger (the finger is the input; the press is
    /// dropped).
    pub(crate) fn touch_mouse_button(&mut self, down: bool) -> bool {
        let at = self.mouse_pos();
        let t = &mut self.touch;
        if down && t.mouse_for_finger(at) {
            return false;
        }
        if down {
            if t.start_edit(Mover::Mouse, at).is_some() {
                t.mouse_took = true;
            }
        } else if t.edit.is_some_and(|e| e.by == Mover::Mouse) {
            t.finish_edit();
        }
        true
    }

    /// The mouse moved to `at`: drags a control being moved with it.
    /// Returns `false` for a move the system made up for a finger, which
    /// leaves the finger the UI's pointer.
    pub(crate) fn touch_mouse_moved(&mut self, at: Vec2) -> bool {
        let t = &mut self.touch;
        t.drag_edit(Mover::Mouse, at);
        !(t.primary.is_some() || t.mouse_for_finger(at))
    }

    pub(crate) fn feed_trackpad(&mut self, gesture: Gesture, phase: TouchPhase, amount: f32) {
        self.record(crate::actions::InputEvent::Trackpad { gesture, phase, amount });
        let at = self.mouse_pos();
        self.touch.trackpad(gesture, phase, amount, at);
        self.refresh_actions();
    }

    pub(crate) fn feed_touch(&mut self, id: u64, phase: TouchPhase, at: Vec2) {
        self.record(crate::actions::InputEvent::Touch {
            id,
            phase,
            position: at,
        });
        self.touch.feed(id, phase, at);
        self.refresh_actions();
    }

    /// The pointer the retained UI follows: `(position, pressed, held,
    /// released)` of the primary finger when a finger was used last, else
    /// of the mouse's left button. A finger on an on-screen control is
    /// not a pointer.
    pub(crate) fn ui_pointer(&self) -> (Vec2, bool, bool, bool) {
        let t = &self.touch;
        if t.pointer_is_touch {
            (t.pointer, t.pointer_pressed, t.primary.is_some(), t.pointer_released)
        } else if self.touch.mouse_took {
            // The mouse is moving an on-screen control.
            (self.mouse_pos(), false, false, false)
        } else {
            use super::MouseButton::Left;
            (
                self.mouse_pos(),
                self.mouse_pressed(Left),
                self.mouse_down(Left),
                self.mouse_released(Left),
            )
        }
    }

    /// The platform's answer to "is there a touch screen", from the window.
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub(crate) fn set_platform_touch(&mut self, touch: bool) {
        self.touch.platform = Some(touch);
    }

    /// The fingers on the screen now, in the order they went down.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(10.0, 10.0)));
    ///     ctx.input.feed(InputEvent::touch_start(2, vec2(90.0, 10.0)));
    ///     assert_eq!(ctx.input.touches().len(), 2);
    ///     ctx.input.feed(InputEvent::touch_end(1, vec2(10.0, 10.0)));
    ///     assert_eq!(ctx.input.touches()[0].id(), 2);
    /// });
    /// ```
    #[must_use]
    pub fn touches(&self) -> &[Touch] {
        &self.touch.touches
    }

    /// The gestures that happened this frame, in order. Held gestures
    /// (drag, long press, pinch, rotate) are here on the frame they began;
    /// read [`drag_delta`](Self::drag_delta), [`pinch_scale`](Self::pinch_scale)
    /// and [`rotation_delta`](Self::rotation_delta) for how they go on, or
    /// bind them in the action map.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     // A quick flick to the right.
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(20.0, 100.0)));
    ///     ctx.input.feed(InputEvent::touch_move(1, vec2(120.0, 104.0)));
    ///     ctx.input.feed(InputEvent::touch_end(1, vec2(120.0, 104.0)));
    ///     let gestures: Vec<Gesture> = ctx.input.gesture_events().iter().map(|e| e.gesture).collect();
    ///     assert_eq!(gestures, [Gesture::Drag, Gesture::SwipeRight]);
    /// });
    /// ```
    #[must_use]
    pub fn gesture_events(&self) -> &[GestureEvent] {
        &self.touch.events
    }

    /// How far a one-finger drag moved this frame, in window pixels (zero
    /// without one). Its first frame counts from where the finger went down.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(100.0, 100.0)));
    ///     ctx.input.feed(InputEvent::touch_move(1, vec2(100.0, 130.0)));
    ///     assert_eq!(ctx.input.drag_delta(), vec2(0.0, 30.0));
    /// });
    /// ```
    #[must_use]
    pub fn drag_delta(&self) -> Vec2 {
        self.touch.drag_delta
    }

    /// How much a pinch scaled this frame: `1.0` for none, `1.25` when the
    /// fingers moved a quarter farther apart. Multiply a zoom by it. The
    /// frame a pinch begins in counts from where the fingers went down.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(100.0, 100.0)));
    ///     ctx.input.feed(InputEvent::touch_start(2, vec2(200.0, 100.0)));
    ///     ctx.input.feed(InputEvent::touch_move(2, vec2(220.0, 100.0))); // past the threshold
    ///     ctx.input.feed(InputEvent::touch_move(2, vec2(270.0, 100.0)));
    ///     assert!((ctx.input.pinch_scale() - 1.7).abs() < 1e-5); // 100 apart, then 170
    ///     assert!(ctx.input.gesture_events().iter().any(|e| e.gesture == Gesture::Pinch));
    /// });
    /// ```
    #[must_use]
    pub fn pinch_scale(&self) -> f32 {
        self.touch.pinch_scale
    }

    /// How far two fingers turned this frame, in radians, clockwise on the
    /// screen positive (zero without a rotation).
    #[must_use]
    pub fn rotation_delta(&self) -> f32 {
        self.touch.rotation
    }

    /// Whether this is a touch device: the platform has a touch screen (a
    /// phone, a tablet, a browser reporting touch points) or a touch was
    /// seen. On-screen controls show by default only then.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     assert!(!ctx.input.touch_device(), "headless: no touch screen");
    ///     ctx.input.feed(InputEvent::touch_start(1, vec2(1.0, 1.0)));
    ///     assert!(ctx.input.touch_device());
    /// });
    /// ```
    #[must_use]
    pub fn touch_device(&self) -> bool {
        self.touch.device()
    }

    /// Says whether this is a touch device, over what the platform said
    /// (a settings toggle, a test). A touch seen later still makes it one.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 320, 240).run_headless(0, |ctx| {
    ///     ctx.input.set_touch_device(true);
    ///     assert!(ctx.input.touch_device());
    /// });
    /// ```
    pub fn set_touch_device(&mut self, touch: bool) {
        self.touch.platform = Some(touch);
        self.touch.seen &= touch;
    }

    /// The thresholds gestures are recognized with.
    #[must_use]
    pub fn gesture_settings(&self) -> &GestureSettings {
        &self.touch.settings
    }

    /// Changes the thresholds gestures are recognized with, from the next
    /// touch event on.
    pub fn set_gesture_settings(&mut self, settings: GestureSettings) {
        self.touch.settings = settings;
    }

    /// Puts on-screen `controls` on the screen, in place of any set before
    /// (an empty set takes them away). They take touches and press their
    /// actions at once, as their [`show`](crate::ui::touch::TouchControls::show)
    /// says (on touch devices, by default), and are drawn from the next
    /// frame. See [`TouchControls`](crate::ui::touch::TouchControls); each
    /// control's node is named `touch.<action>` for
    /// [`Ui::find`](crate::ui::Ui::find).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::ui::touch::{TouchControls, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    ///     ctx.input.set_touch_controls(TouchControls::new().button("jump", "A").show(TouchShow::Always));
    ///     let at = ctx.input.touch_control_center("jump").unwrap();
    ///     ctx.input.feed(InputEvent::touch_start(1, at));
    ///     assert!(ctx.input.action_pressed("jump"));
    /// });
    /// ```
    pub fn set_touch_controls(&mut self, controls: crate::ui::touch::TouchControls) {
        let (window, scale) = self.touch.view;
        let mut zones = crate::ui::touch::zones(&controls);
        crate::ui::touch::place_zones(&controls, &self.touch.layout, &mut zones, window, scale);
        self.touch.edit = None;
        self.touch.replace_zones(zones);
        self.touch.show = controls.show;
        self.touch.controls = controls;
        self.touch.generation += 1;
        self.refresh_actions();
    }

    /// The middle of the on-screen control bound to `action`
    /// ([`set_touch_controls`](Self::set_touch_controls)), in window pixels: where a test's finger presses it. `None` when no
    /// control drives that action.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::ui::touch::{TouchControls, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    ///     ctx.input.set_touch_controls(TouchControls::new().button("jump", "A").show(TouchShow::Always));
    ///     let at = ctx.input.touch_control_center("jump").unwrap();
    ///     ctx.input.feed(InputEvent::touch_start(1, at));
    ///     assert!(ctx.input.action_pressed("jump"));
    ///     assert_eq!(ctx.input.touch_control_center("fly"), None);
    /// });
    /// ```
    #[must_use]
    pub fn touch_control_center(&self, action: &str) -> Option<Vec2> {
        self.touch.zones.iter().find(|z| z.action == action).map(|z| z.center)
    }

    /// Whether the on-screen controls are up now and take touches: there
    /// are some, their [`TouchShow`] allows it, and no UI screen holds them
    /// back.
    #[must_use]
    pub fn touch_controls_shown(&self) -> bool {
        self.touch.controls_active()
    }

    /// Changes when the on-screen controls show: on touch devices
    /// ([`TouchShow::Auto`], the default), always, or never. A settings
    /// screen's "on-screen controls" option. They appear or go from the
    /// next frame.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::touch::{TouchControls, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_touch_controls(TouchControls::new().stick("move"));
    ///     ctx.input.set_touch_controls_show(TouchShow::Always);
    ///     assert!(ctx.input.touch_controls_shown());
    ///     ctx.input.set_touch_controls_show(TouchShow::Never);
    ///     assert!(!ctx.input.touch_controls_shown());
    /// });
    /// ```
    pub fn set_touch_controls_show(&mut self, show: TouchShow) {
        self.touch.show = show;
    }

    /// Shows or hides the on-screen control bound to `action`, for controls
    /// that mean something on some screens only: a pause button hidden on
    /// the title. A hidden control takes no new touches at once (a finger
    /// already on it keeps it until it lifts) and goes from the screen from
    /// the next frame.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::ui::touch::{TouchControls, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_action("pause", Action::Button(vec![]));
    ///     ctx.input.set_touch_controls(TouchControls::new().corner_button("pause", "II").show(TouchShow::Always));
    ///     let at = ctx.input.touch_control_center("pause").unwrap();
    ///     ctx.input.set_touch_control_shown("pause", false);
    ///     ctx.input.feed(InputEvent::touch_start(1, at));
    ///     assert!(!ctx.input.action_down("pause"));
    /// });
    /// ```
    pub fn set_touch_control_shown(&mut self, action: &str, shown: bool) {
        for zone in self.touch.zones.iter_mut().filter(|z| z.action == action) {
            zone.shown = shown;
        }
    }

    /// Turns the players' arranging of the on-screen controls on or off.
    /// While it is on, the controls show (also under a UI screen, also off
    /// a touch screen), press no actions, and a finger or the mouse
    /// dragging one moves it: where it is let go, it stays, anchored to the
    /// nearest corner, in the player's [`TouchLayout`](crate::ui::touch::TouchLayout).
    /// Save it when the player is done ([`save_touch_layout`](Self::save_touch_layout)).
    /// Turning it off lets go of a control being moved.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::ui::touch::{TouchControls, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_action("jump", Action::Button(vec![]));
    ///     ctx.input.set_touch_controls(TouchControls::new().button("jump", "A").show(TouchShow::Always));
    ///     ctx.input.edit_touch_controls(true);
    ///     // The player drags the jump button to the left.
    ///     let at = ctx.input.touch_control_center("jump").unwrap();
    ///     ctx.input.feed(InputEvent::touch_start(1, at));
    ///     ctx.input.feed(InputEvent::touch_move(1, at - vec2(300.0, 0.0)));
    ///     ctx.input.feed(InputEvent::touch_end(1, at - vec2(300.0, 0.0)));
    ///     assert!(!ctx.input.action_pressed("jump"), "moving it doesn't press it");
    ///     ctx.input.edit_touch_controls(false);
    ///     assert_eq!(ctx.input.touch_control_center("jump"), Some(at - vec2(300.0, 0.0)));
    ///     assert!(ctx.input.touch_layout().controls["jump"].place.is_some());
    /// });
    /// ```
    pub fn edit_touch_controls(&mut self, on: bool) {
        if !on {
            self.touch.finish_edit();
            self.touch.picked = None;
        }
        self.touch.editing = on;
    }

    /// Opens the ready-made screen where the player arranges the on-screen
    /// controls ([`TouchControls`](crate::ui::touch::TouchControls)), a settings page's
    /// "arrange controls" button. It shows the controls (also on a desktop,
    /// also under the screen that opened it) and a small panel that keeps
    /// clear of them. With a finger or the mouse, the player:
    ///
    /// - **moves** a control by dragging it (it stays where it is let go,
    ///   anchored to the nearest corner);
    /// - **resizes** one: tap or click it, then `-` and `+`; `Size` makes
    ///   them all bigger or smaller and `Opacity` fainter;
    /// - **resets** them to where the game puts them (`Reset`);
    /// - **saves** with `Save`, which writes the arrangement to the player's
    ///   storage ([`Input::save_touch_layout`](crate::input::Input::save_touch_layout))
    ///   and closes the screen. `Cancel`, Escape, a gamepad's East button
    ///   or the game closing the screen put back the arrangement it had
    ///   when it opened.
    ///
    /// Edit mode starts at once; the engine builds the screen at the UI's
    /// next pass, before the next system runs (or the next frame, called
    /// from the last). Called again while it is open, nothing changes.
    /// However it closes, edit mode ends and
    /// [`UiEvent::ScreenPopped`](crate::ui::UiEvent::ScreenPopped) is queued
    /// with the screen's root (`ctx.ui.find("touch.arrange")` while it is
    /// open). Its nodes are named `touch.arrange.<part>` (`title`, `note`,
    /// `size_label`, `opacity_label`, `reset`, `cancel`, `save`, ...): a
    /// game gives them its own words with [`Ui::set_text`](crate::ui::Ui::set_text)
    /// or [`Ui::set_text_key`](crate::ui::Ui::set_text_key) after opening it.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::touch::TouchControls;
    /// Game::new("touch", 960, 540)
    ///     .headless_frames(true)
    ///     .add(Phase::Frame, |ctx: &mut Ctx| {
    ///         // Built for the frame after the call.
    ///         assert!(ctx.ui.find("touch.arrange.save").is_some());
    ///     })
    ///     .run_headless(2, |ctx| {
    ///         ctx.input.set_touch_controls(TouchControls::new().stick("move").button("jump", "A"));
    ///         // The settings page's "Arrange controls" button:
    ///         ctx.input.arrange_touch_controls();
    ///         assert!(ctx.input.editing_touch_controls());
    ///     });
    /// ```
    pub fn arrange_touch_controls(&mut self) {
        if self.touch.arranging {
            return;
        }
        self.touch.arranging = true;
        self.touch.arrange_before = Some(self.touch.layout.clone());
        self.edit_touch_controls(true);
    }

    /// Whether the player is arranging the on-screen controls
    /// ([`edit_touch_controls`](Self::edit_touch_controls)).
    #[must_use]
    pub fn editing_touch_controls(&self) -> bool {
        self.touch.editing
    }

    /// The player's arrangement of the on-screen controls: where they
    /// moved each, its size, their opacity.
    #[must_use]
    pub fn touch_layout(&self) -> &crate::ui::touch::TouchLayout {
        &self.touch.layout
    }

    /// Replaces the player's arrangement of the on-screen controls: a
    /// settings screen's size and opacity sliders, a "reset layout" button
    /// (`TouchLayout::default()`), a layout read back from a file. The
    /// controls take their places at once and their look from the next
    /// frame. Sizes below a tenth are a tenth, opacity is kept within 0..=1.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::touch::{TouchControls, TouchLayout, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     ctx.input.set_touch_controls(TouchControls::new().button("jump", "A").show(TouchShow::Always));
    ///     let before = ctx.input.touch_control_center("jump").unwrap();
    ///     let mut layout = ctx.input.touch_layout().clone();
    ///     layout.size = 1.5;
    ///     layout.opacity = 0.5;
    ///     ctx.input.set_touch_layout(layout);
    ///     // Where it was, half as big again.
    ///     assert_eq!(ctx.input.touch_control_center("jump"), Some(before));
    ///     ctx.input.set_touch_layout(TouchLayout::default());
    /// });
    /// ```
    pub fn set_touch_layout(&mut self, layout: crate::ui::touch::TouchLayout) {
        self.touch.edit = None;
        self.touch.layout = layout.sane();
        let t = &mut self.touch;
        let (window, scale) = t.view;
        crate::ui::touch::place_zones(&t.controls, &t.layout, &mut t.zones, window, scale);
        // Built again for the new opacity.
        t.generation += 1;
    }

    /// Saves the player's arrangement of the on-screen controls in the
    /// game's player storage, as [`TouchLayout::FILE`](crate::ui::touch::TouchLayout::FILE),
    /// next to the other settings (`Window::save_settings` keeps the
    /// window's there).
    ///
    /// # Errors
    /// The storage could not write the file.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::touch::TouchLayout;
    /// # let dir = std::env::temp_dir().join(format!("ponko-touch-doc-{}", std::process::id()));
    /// Game::new("touch", 960, 540).data_dir(&dir).run_headless(0, |ctx| {
    ///     let mut layout = TouchLayout::default();
    ///     layout.opacity = 0.4;
    ///     ctx.input.set_touch_layout(layout);
    ///     ctx.input.save_touch_layout(&mut ctx.storage).unwrap();
    /// });
    /// // The next run, at setup:
    /// Game::new("touch", 960, 540).data_dir(&dir).run_headless(0, |ctx| {
    ///     ctx.input.restore_touch_layout(&ctx.storage).unwrap();
    ///     assert_eq!(ctx.input.touch_layout().opacity, 0.4);
    /// });
    /// # std::fs::remove_dir_all(&dir).ok();
    /// ```
    pub fn save_touch_layout(&self, storage: &mut crate::storage::Storage) -> Result<(), crate::error::Error> {
        storage.write_json(crate::ui::touch::TouchLayout::FILE, &self.touch.layout)
    }

    /// Reads back the arrangement [`save_touch_layout`](Self::save_touch_layout)
    /// saved, at setup. No file (the player never moved a control) leaves
    /// the layout as it is.
    ///
    /// # Errors
    /// The file is there but could not be read or is not a touch layout;
    /// the layout is left as it is.
    pub fn restore_touch_layout(&mut self, storage: &crate::storage::Storage) -> Result<(), crate::error::Error> {
        match storage.read_json(crate::ui::touch::TouchLayout::FILE) {
            Ok(layout) => {
                self.set_touch_layout(layout);
                Ok(())
            }
            Err(e) if e.kind() == crate::error::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Input {
        Input::new()
    }

    fn gestures(input: &Input) -> Vec<Gesture> {
        input.gesture_events().iter().map(|e| e.gesture).collect()
    }

    /// A new frame at time `t`.
    fn frame(input: &mut Input, t: f64) {
        input.end_frame();
        input.begin_frame(t);
    }

    #[test]
    fn a_short_touch_that_stays_put_is_a_tap() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::new(50.0, 50.0));
        frame(&mut i, 0.1);
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(55.0, 52.0));
        assert_eq!(gestures(&i), [Gesture::Tap]);
        frame(&mut i, 0.2);
        assert!(gestures(&i).is_empty(), "a gesture lasts one frame");
    }

    #[test]
    fn a_slow_touch_is_not_a_tap() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::ZERO);
        frame(&mut i, 0.4);
        i.feed_touch(1, TouchPhase::Ended, Vec2::ZERO);
        assert!(gestures(&i).is_empty(), "held past the tap time, short of a long press");
    }

    #[test]
    fn a_second_tap_nearby_and_soon_is_a_double_tap_and_a_third_is_a_tap_again() {
        let mut i = input();
        let tap = |i: &mut Input, id, at: Vec2| {
            i.feed_touch(id, TouchPhase::Started, at);
            i.feed_touch(id, TouchPhase::Ended, at);
        };
        tap(&mut i, 1, Vec2::new(100.0, 100.0));
        frame(&mut i, 0.15);
        tap(&mut i, 2, Vec2::new(110.0, 100.0));
        assert_eq!(gestures(&i), [Gesture::DoubleTap]);
        frame(&mut i, 0.3);
        tap(&mut i, 3, Vec2::new(110.0, 100.0));
        assert_eq!(gestures(&i), [Gesture::Tap]);
        // Too far from the last one: a tap, not a double tap.
        frame(&mut i, 0.35);
        tap(&mut i, 4, Vec2::new(300.0, 100.0));
        assert_eq!(gestures(&i), [Gesture::Tap]);
        // Too late after the last one.
        frame(&mut i, 0.9);
        tap(&mut i, 5, Vec2::new(300.0, 100.0));
        assert_eq!(gestures(&i), [Gesture::Tap]);
    }

    #[test]
    fn a_touch_held_still_becomes_a_long_press_once_and_no_tap() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::new(10.0, 10.0));
        frame(&mut i, 0.49);
        assert!(gestures(&i).is_empty());
        frame(&mut i, 0.5);
        assert_eq!(gestures(&i), [Gesture::LongPress]);
        assert!(i.touch.down(Gesture::LongPress));
        frame(&mut i, 0.6);
        assert!(gestures(&i).is_empty(), "reported once");
        assert!(i.touch.down(Gesture::LongPress), "held while the finger stays");
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(10.0, 10.0));
        assert!(gestures(&i).is_empty(), "no tap after a long press");
        assert!(!i.touch.down(Gesture::LongPress));
    }

    #[test]
    fn moving_past_the_tap_distance_starts_a_drag_counted_from_the_start() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::new(0.0, 0.0));
        i.feed_touch(1, TouchPhase::Moved, Vec2::new(8.0, 0.0));
        assert!(!i.touch.down(Gesture::Drag), "inside the tap distance");
        i.feed_touch(1, TouchPhase::Moved, Vec2::new(12.0, 0.0));
        assert_eq!(gestures(&i), [Gesture::Drag]);
        assert_eq!(i.drag_delta(), Vec2::new(12.0, 0.0));
        frame(&mut i, 1.0);
        i.feed_touch(1, TouchPhase::Moved, Vec2::new(12.0, 30.0));
        assert_eq!(i.drag_delta(), Vec2::new(0.0, 30.0));
        assert!(i.touch.down(Gesture::Drag));
        frame(&mut i, 1.1);
        assert!(!i.touch.down(Gesture::LongPress), "a moved finger never long-presses");
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(12.0, 30.0));
        assert!(!i.touch.down(Gesture::Drag));
        assert!(gestures(&i).is_empty(), "slow: no swipe");
    }

    #[test]
    fn a_quick_long_drag_is_a_swipe_in_its_main_direction() {
        for (to, swipe) in [
            (Vec2::new(-80.0, 10.0), Gesture::SwipeLeft),
            (Vec2::new(80.0, -10.0), Gesture::SwipeRight),
            (Vec2::new(10.0, -80.0), Gesture::SwipeUp),
            (Vec2::new(-10.0, 80.0), Gesture::SwipeDown),
        ] {
            let mut i = input();
            let from = Vec2::new(200.0, 200.0);
            i.feed_touch(1, TouchPhase::Started, from);
            frame(&mut i, 0.1);
            i.feed_touch(1, TouchPhase::Moved, from + to);
            i.feed_touch(1, TouchPhase::Ended, from + to);
            assert_eq!(gestures(&i).last(), Some(&swipe), "{to}");
        }
        // Too short.
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::ZERO);
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(30.0, 0.0));
        assert_eq!(gestures(&i), [Gesture::Drag]);
    }

    #[test]
    fn two_fingers_pinch_and_rotate_past_their_thresholds() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::new(100.0, 100.0));
        i.feed_touch(1, TouchPhase::Moved, Vec2::new(130.0, 100.0));
        assert!(i.touch.down(Gesture::Drag));
        i.feed_touch(2, TouchPhase::Started, Vec2::new(230.0, 100.0));
        assert!(i.touch.drag.is_none(), "a second finger ends the drag");
        frame(&mut i, 0.1);
        assert!(!i.touch.down(Gesture::Drag), "...and the frame it began in is over");
        // 100 apart; 10 farther is under the threshold.
        i.feed_touch(2, TouchPhase::Moved, Vec2::new(240.0, 100.0));
        assert!(!i.touch.down(Gesture::Pinch));
        assert_eq!(i.pinch_scale(), 1.0);
        i.feed_touch(2, TouchPhase::Moved, Vec2::new(250.0, 100.0));
        assert!(i.touch.down(Gesture::Pinch));
        i.feed_touch(2, TouchPhase::Moved, Vec2::new(310.0, 100.0));
        assert!((i.pinch_scale() - 1.8).abs() < 1e-5, "from where the fingers went down");
        assert_eq!(i.touch.axis(GestureAxis::Pinch), i.pinch_scale() - 1.0);
        frame(&mut i, 0.2);
        assert_eq!(i.pinch_scale(), 1.0, "a per-frame change");
        // Turn the second finger a quarter turn around the first.
        i.feed_touch(2, TouchPhase::Moved, Vec2::new(130.0, 280.0));
        assert!(i.touch.down(Gesture::Rotate));
        assert!(
            (i.rotation_delta() - std::f32::consts::FRAC_PI_2).abs() < 1e-4,
            "clockwise on screen: {}",
            i.rotation_delta()
        );
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(130.0, 100.0));
        assert!(i.touch.two.is_none(), "a lifted finger ends the pinch");
        frame(&mut i, 0.25);
        assert!(!i.touch.down(Gesture::Pinch) && !i.touch.down(Gesture::Rotate));
        i.feed_touch(2, TouchPhase::Ended, Vec2::new(130.0, 280.0));
        assert!(gestures(&i).is_empty(), "the fingers of a pinch make no tap");
    }

    #[test]
    fn a_cancelled_touch_makes_no_gesture_and_focus_loss_cancels_all() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::ZERO);
        i.feed_touch(1, TouchPhase::Cancelled, Vec2::ZERO);
        assert!(gestures(&i).is_empty());
        i.feed_touch(2, TouchPhase::Started, Vec2::ZERO);
        i.feed_touch(3, TouchPhase::Started, Vec2::new(5.0, 5.0));
        i.release_all();
        assert!(i.touches().is_empty());
        assert!(gestures(&i).is_empty());
    }

    #[test]
    fn non_finite_touches_are_ignored() {
        let mut i = input();
        i.feed_touch(1, TouchPhase::Started, Vec2::new(f32::NAN, 0.0));
        assert!(i.touches().is_empty());
    }

    #[test]
    fn a_touch_on_a_control_is_claimed_and_makes_no_gesture() {
        let mut i = input();
        i.touch.show = TouchShow::Always;
        i.touch.zones.push(Zone {
            kind: ZoneKind::Button,
            action: "jump".into(),
            slot: None,
            center: Vec2::new(100.0, 100.0),
            radius: 30.0,
            reach: 40.0,
            deadzone: 0.0,
            touch: None,
            holders: 0,
            value: Vec2::ZERO,
            shown: true,
        });
        i.feed_touch(1, TouchPhase::Started, Vec2::new(120.0, 100.0));
        assert!(i.touches()[0].claimed());
        assert!(i.touch.zones[0].held());
        i.feed_touch(1, TouchPhase::Moved, Vec2::new(300.0, 100.0));
        i.feed_touch(1, TouchPhase::Ended, Vec2::new(300.0, 100.0));
        assert!(gestures(&i).is_empty());
        assert!(!i.touch.zones[0].held());
        // Hidden controls take nothing.
        i.touch.show = TouchShow::Never;
        i.feed_touch(2, TouchPhase::Started, Vec2::new(100.0, 100.0));
        assert!(!i.touches()[0].claimed());
    }

    #[test]
    fn a_stick_follows_its_finger_with_a_deadzone_and_full_deflection_at_its_radius() {
        let mut zone = Zone {
            kind: ZoneKind::Stick,
            action: "move".into(),
            slot: None,
            center: Vec2::ZERO,
            radius: 100.0,
            reach: 150.0,
            deadzone: 0.1,
            touch: Some(1),
            holders: 0,
            value: Vec2::ZERO,
            shown: true,
        };
        zone.follow(Vec2::new(5.0, 0.0));
        assert_eq!(zone.value, Vec2::ZERO);
        zone.follow(Vec2::new(55.0, 0.0));
        assert!((zone.value.x - 0.5).abs() < 1e-5);
        zone.follow(Vec2::new(0.0, -300.0));
        assert_eq!(zone.value, Vec2::new(0.0, -1.0));
    }

    #[test]
    fn wrap_keeps_angles_in_one_turn() {
        use std::f32::consts::PI;
        assert!((wrap(3.0 * PI / 2.0) + PI / 2.0).abs() < 1e-5);
        assert!((wrap(-3.0 * PI / 2.0) - PI / 2.0).abs() < 1e-5);
        assert_eq!(wrap(f32::NAN), 0.0);
    }
}
