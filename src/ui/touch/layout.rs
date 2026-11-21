//! How the on-screen controls look and where they sit: the game's style
//! and placement ([`TouchStyle`], [`TouchPlace`]) and the player's own
//! arrangement ([`TouchLayout`]), saved with the game's settings.

use std::collections::BTreeMap;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::texture::Texture;

/// How on-screen controls look, for [`TouchControls::style`](super::TouchControls::style)
/// (every control) and [`TouchControls::control_style`](super::TouchControls::control_style)
/// (one). The colors tint the images, which are drawn white: the default
/// disc is a see-through fill with a solid rim; a game's own image (a
/// texture with transparency, a square, a ring, a pictogram) replaces it.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::touch::{TouchControls, TouchStyle};
/// // Bigger, warmer buttons; the attack button red.
/// let style = TouchStyle { button: Color::rgba(1.0, 0.85, 0.6, 0.35), size: 1.2, ..TouchStyle::default() };
/// let red = TouchStyle { button: Color::rgba(1.0, 0.2, 0.2, 0.4), held: Color::rgba(1.0, 0.3, 0.3, 0.8), ..style };
/// let controls = TouchControls::new()
///     .stick("move")
///     .button("jump", "A")
///     .button("attack", "B")
///     .style(style)
///     .control_style("attack", red);
/// assert_eq!(controls.style_of("attack").button, red.button);
/// assert_eq!(controls.style_of("jump").size, 1.2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchStyle {
    /// A stick's base.
    pub base: Color,
    /// A stick's knob.
    pub knob: Color,
    /// A button while no finger holds it.
    pub button: Color,
    /// A button while a finger holds it.
    pub held: Color,
    /// A button's label.
    pub label: Color,
    /// The image of a stick's base and of a button: drawn white and tinted
    /// by the colors above. `None`: the engine's disc.
    pub image: Option<Texture>,
    /// The image of a stick's knob. `None`: the same as `image`.
    pub knob_image: Option<Texture>,
    /// The size, times the engine's (which follows the window). Default 1.
    pub size: f32,
    /// A label's height, as a share of the button's radius. Default 0.8.
    pub label_size: f32,
}

impl Default for TouchStyle {
    fn default() -> Self {
        Self {
            base: Color::rgba(1.0, 1.0, 1.0, 0.22),
            knob: Color::rgba(1.0, 1.0, 1.0, 0.6),
            button: Color::rgba(1.0, 1.0, 1.0, 0.3),
            held: Color::rgba(1.0, 1.0, 1.0, 0.7),
            label: Color::WHITE,
            image: None,
            knob_image: None,
            size: 1.0,
            label_size: 0.8,
        }
    }
}

/// Where an on-screen control's centre sits: at `anchor`, a point of the
/// view from `(0, 0)` (top left) to `(1, 1)` (bottom right), moved by
/// `offset` in UI units (window pixels at UI scale 1). Anchored to the
/// nearest corner, a control keeps its distance from it when the window
/// changes size or the phone turns. A control that would stick out of
/// the view is moved in.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::touch::TouchPlace;
/// // 80 units in from the bottom right corner, both ways.
/// let place = TouchPlace::new(vec2(1.0, 1.0), vec2(-80.0, -80.0));
/// assert_eq!(place.centre(vec2(960.0, 540.0)), vec2(880.0, 460.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TouchPlace {
    /// A point of the view, `(0, 0)` top left to `(1, 1)` bottom right.
    pub anchor: Vec2,
    /// From the anchor to the control's centre, in UI units.
    pub offset: Vec2,
}

impl TouchPlace {
    /// A control centred `offset` UI units from `anchor`.
    #[must_use]
    pub fn new(anchor: Vec2, offset: Vec2) -> Self {
        Self { anchor, offset }
    }

    /// Where its centre is in a view `view` UI units large.
    #[must_use]
    pub fn centre(&self, view: Vec2) -> Vec2 {
        self.anchor * view + self.offset
    }

    /// The place of a centre at `centre` in a view `view` UI units large,
    /// anchored to the view's nearest corner.
    #[must_use]
    pub fn nearest_corner(centre: Vec2, view: Vec2) -> Self {
        let anchor = Vec2::new(
            if centre.x < view.x / 2.0 { 0.0 } else { 1.0 },
            if centre.y < view.y / 2.0 { 0.0 } else { 1.0 },
        );
        Self::new(anchor, centre - anchor * view)
    }
}

/// What the player changed about one on-screen control, in a [`TouchLayout`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchAdjust {
    /// Where the player put it (`None`: where the game puts it).
    pub place: Option<TouchPlace>,
    /// Its size, times the game's. Default 1.
    pub size: f32,
}

impl Default for TouchAdjust {
    fn default() -> Self {
        Self { place: None, size: 1.0 }
    }
}

/// The player's arrangement of the on-screen controls: where each is, how
/// big, how see-through. It goes over the game's [`TouchControls`](super::TouchControls)
/// (a control the player didn't touch stays where the game put it), is
/// changed by moving the controls in [edit mode](crate::input::Input::edit_touch_controls)
/// or by a settings screen ([`Input::set_touch_layout`](crate::input::Input::set_touch_layout)),
/// and is saved with the settings
/// ([`Input::save_touch_layout`](crate::input::Input::save_touch_layout)).
/// Controls are known by the action they drive.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::touch::{TouchLayout, TouchPlace};
/// let mut layout = TouchLayout::default();
/// layout.opacity = 0.6; // fainter
/// layout.adjust("jump").size = 1.5; // a bigger jump button
/// layout.adjust("jump").place = Some(TouchPlace::new(vec2(1.0, 1.0), vec2(-90.0, -70.0)));
/// let json = layout.to_json().unwrap();
/// assert_eq!(TouchLayout::from_json(&json).unwrap(), layout);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchLayout {
    /// The controls the player changed, by the action they drive.
    pub controls: BTreeMap<String, TouchAdjust>,
    /// Every control's size, times the game's. Default 1.
    pub size: f32,
    /// How opaque every control is, times its style's colors: 1 as the
    /// game draws them, 0.5 half as visible. Default 1.
    pub opacity: f32,
}

impl Default for TouchLayout {
    fn default() -> Self {
        Self {
            controls: BTreeMap::new(),
            size: 1.0,
            opacity: 1.0,
        }
    }
}

impl TouchLayout {
    /// The file [`Input::save_touch_layout`](crate::input::Input::save_touch_layout)
    /// writes in the player's storage.
    pub const FILE: &'static str = "touch_controls.json";

    /// What the player changed about the control driving `action`, to
    /// change it (an entry with nothing changed yet when there was none).
    pub fn adjust(&mut self, action: &str) -> &mut TouchAdjust {
        self.controls.entry(action.to_string()).or_default()
    }

    /// The control driving `action` as the player left it.
    #[must_use]
    pub fn adjusted(&self, action: &str) -> TouchAdjust {
        self.controls.get(action).copied().unwrap_or_default()
    }

    /// The layout as JSON, for a settings file of the game's own.
    ///
    /// # Errors
    /// Never for a layout made of finite numbers; a JSON error otherwise.
    pub fn to_json(&self) -> Result<String, crate::error::Error> {
        serde_json::to_string_pretty(self).map_err(|e| {
            crate::error::Error::new(
                crate::error::ErrorKind::Json,
                format!("cannot save the touch layout: {e}"),
            )
        })
    }

    /// A layout from [`to_json`](Self::to_json)'s JSON. Fields left out
    /// take their defaults.
    ///
    /// # Errors
    /// The text is not a touch layout: a JSON error with where it broke.
    pub fn from_json(json: &str) -> Result<Self, crate::error::Error> {
        serde_json::from_str(json).map_err(|e| {
            crate::error::Error::new(
                crate::error::ErrorKind::Json,
                format!("cannot read the touch layout: {e}"),
            )
        })
    }

    /// The size and opacity numbers made usable: finite, size at least a
    /// tenth, opacity within 0..=1.
    pub(crate) fn sane(mut self) -> Self {
        let size = |s: f32| if s.is_finite() { s.max(0.1) } else { 1.0 };
        self.size = size(self.size);
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        for adjust in self.controls.values_mut() {
            adjust.size = size(adjust.size);
            if adjust
                .place
                .is_some_and(|p| !p.anchor.is_finite() || !p.offset.is_finite())
            {
                adjust.place = None;
            }
        }
        self
    }
}
