//! How on-screen controls look and where they sit, from C: the game's
//! styles and placement (Rust `TouchStyle`, `TouchPlace`,
//! `TouchControls::style`, `control_style` and `place`, set together with
//! the controls by `ponko_ctx_set_touch_controls_ex`), the player's own
//! arrangement (Rust `TouchLayout`: read, change, reset, as JSON, saved
//! with the settings) and its edit mode (`Input::edit_touch_controls`).
//!
//! ```c
//! PonkoTouchControl controls[] = {{PONKO_TOUCH_STICK, "move", NULL}, {PONKO_TOUCH_BUTTON, "jump", "A"}};
//! PonkoTouchControlsDesc desc = {.struct_size = sizeof desc, .version = PONKO_TOUCH_CONTROLS_DESC_VERSION};
//! ponko_touch_controls_desc_defaults(&desc);
//! desc.controls = controls; desc.count = 2;
//! desc.style.size = 1.2f; // bigger, every control
//! ponko_ctx_set_touch_controls_ex(ctx, &desc);
//! ponko_ctx_restore_touch_layout(ctx); // the player's arrangement from last time
//! ```

use std::ffi::c_char;
use std::mem::{offset_of, size_of};

use ponko::ui::touch::{TouchAdjust, TouchLayout, TouchPlace, TouchStyle};

use crate::ctx::{ctx_access, out, text};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::touch::{PonkoTouchControl, controls_from_c};
use crate::types::{PonkoColor, PonkoTexture, PonkoVec2, texture_from_c, texture_to_c, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, invalid, write_text};

/// The version of `PonkoTouchControlsDesc` this header describes.
pub const PONKO_TOUCH_CONTROLS_DESC_VERSION: u32 = 1;

/// How on-screen controls look (Rust `TouchStyle`). The colors tint the
/// images, which are drawn white: the default disc is a see-through fill
/// with a solid rim; a game's own image replaces it. Start from
/// `ponko_touch_style_default`.
/// ```
/// let s = ponko_ffi::ponko_touch_style_default();
/// assert_eq!((s.size, s.label_size, s.has_image), (1.0, 0.8, false));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoTouchStyle {
    /// A stick's base.
    pub base: PonkoColor,
    /// A stick's knob.
    pub knob: PonkoColor,
    /// A button while no finger holds it.
    pub button: PonkoColor,
    /// A button while a finger holds it.
    pub held: PonkoColor,
    /// A button's label.
    pub label: PonkoColor,
    /// Whether `image` replaces the engine's disc.
    pub has_image: bool,
    /// The image of a stick's base and of a button, with `has_image`.
    pub image: PonkoTexture,
    /// Whether `knob_image` is a stick's knob (else the same as `image`).
    pub has_knob_image: bool,
    /// The image of a stick's knob, with `has_knob_image`.
    pub knob_image: PonkoTexture,
    /// The size, times the engine's (which follows the window). Default 1.
    pub size: f32,
    /// A label's height, as a share of the button's radius. Default 0.8.
    pub label_size: f32,
}

/// Where an on-screen control's centre sits (Rust `TouchPlace`): at
/// `anchor`, a point of the view from (0, 0) top left to (1, 1) bottom
/// right, moved by `offset` UI units. Anchored to a corner, a control keeps
/// its distance from it when the window changes size.
/// ```
/// let p = ponko_ffi::PonkoTouchPlace::default();
/// assert_eq!(p.anchor.x, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoTouchPlace {
    /// A point of the view, (0, 0) top left to (1, 1) bottom right.
    pub anchor: PonkoVec2,
    /// From the anchor to the control's centre, in UI units.
    pub offset: PonkoVec2,
}

/// The look of the control driving `action` (Rust `TouchControls::control_style`).
/// ```
/// let s = ponko_ffi::PonkoTouchControlStyle { action: c"attack".as_ptr(), style: ponko_ffi::ponko_touch_style_default() };
/// assert_eq!(s.style.size, 1.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoTouchControlStyle {
    /// The action, NUL-terminated UTF-8, read during the call.
    pub action: *const c_char,
    /// Its look, over the set's.
    pub style: PonkoTouchStyle,
}

/// Where the game puts the control driving `action` (Rust `TouchControls::place`).
/// ```
/// let p = ponko_ffi::PonkoTouchControlPlace { action: c"jump".as_ptr(), place: Default::default() };
/// assert_eq!(p.place.offset.y, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoTouchControlPlace {
    /// The action, NUL-terminated UTF-8, read during the call.
    pub action: *const c_char,
    /// Where its centre goes instead of where the engine lays it out.
    pub place: PonkoTouchPlace,
}

/// A set of on-screen controls with their look and places (Rust
/// `TouchControls`), for `ponko_ctx_set_touch_controls_ex`. A versioned
/// descriptor: set `struct_size` and `version`, fill it with
/// `ponko_touch_controls_desc_defaults`, then set the controls.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoTouchControlsDesc {
    /// `sizeof(PonkoTouchControlsDesc)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_TOUCH_CONTROLS_DESC_VERSION` as the caller compiled it.
    pub version: u32,
    /// The controls, in the order they are placed (as `ponko_ctx_set_touch_controls`).
    pub controls: *const PonkoTouchControl,
    /// How many `controls` there are.
    pub count: usize,
    /// A `PonkoTouchShow`. Default `PONKO_TOUCH_SHOW_AUTO`.
    pub show: u32,
    /// Keep them up while a UI screen is open. Default false.
    pub over_screens: bool,
    /// Every control's look. Default the engine's discs.
    pub style: PonkoTouchStyle,
    /// Controls with a look of their own; null with `style_count` 0 for none.
    pub styles: *const PonkoTouchControlStyle,
    /// How many `styles` there are.
    pub style_count: usize,
    /// Controls the game places; null with `place_count` 0 for none.
    pub places: *const PonkoTouchControlPlace,
    /// How many `places` there are.
    pub place_count: usize,
}

const _: () =
    assert!(offset_of!(PonkoTouchControlsDesc, struct_size) == 0 && offset_of!(PonkoTouchControlsDesc, version) == 4);
impl Versioned for PonkoTouchControlsDesc {
    const NAME: &'static str = "PonkoTouchControlsDesc";
    const VERSION: u32 = PONKO_TOUCH_CONTROLS_DESC_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, place_count) + size_of::<usize>()),
            _ => None,
        }
    }
}

/// What the player changed about one on-screen control (Rust
/// `TouchAdjust`), in their layout.
/// ```
/// let a = ponko_ffi::PonkoTouchAdjust::default();
/// assert!(!a.placed);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoTouchAdjust {
    /// Whether the player moved it (to `place`); else it sits where the game puts it.
    pub placed: bool,
    /// Where the player put it, with `placed`.
    pub place: PonkoTouchPlace,
    /// Its size, times the game's. Default 1.
    pub size: f32,
}

fn style_to_c(s: &TouchStyle) -> PonkoTouchStyle {
    let none = PonkoTexture {
        id: 0,
        width: 0,
        height: 0,
    };
    PonkoTouchStyle {
        base: to_c(s.base),
        knob: to_c(s.knob),
        button: to_c(s.button),
        held: to_c(s.held),
        label: to_c(s.label),
        has_image: s.image.is_some(),
        image: s.image.map_or(none, texture_to_c),
        has_knob_image: s.knob_image.is_some(),
        knob_image: s.knob_image.map_or(none, texture_to_c),
        size: s.size,
        label_size: s.label_size,
    }
}

fn style_from_c(s: &PonkoTouchStyle) -> Result<TouchStyle, Failure> {
    let colors = [s.base, s.knob, s.button, s.held, s.label];
    if colors
        .iter()
        .any(|c| ![c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite()))
    {
        return Err(invalid("touch style colors must be finite"));
    }
    if !(s.size.is_finite() && s.size > 0.0 && s.label_size.is_finite() && s.label_size >= 0.0) {
        return Err(invalid(
            "a touch style's size must be positive and its label_size not negative",
        ));
    }
    Ok(TouchStyle {
        base: to_c(s.base),
        knob: to_c(s.knob),
        button: to_c(s.button),
        held: to_c(s.held),
        label: to_c(s.label),
        image: s.has_image.then(|| texture_from_c(s.image)),
        knob_image: s.has_knob_image.then(|| texture_from_c(s.knob_image)),
        size: s.size,
        label_size: s.label_size,
    })
}

fn place_from_c(p: &PonkoTouchPlace) -> Result<TouchPlace, Failure> {
    if [p.anchor.x, p.anchor.y, p.offset.x, p.offset.y]
        .iter()
        .all(|v| v.is_finite())
    {
        Ok(TouchPlace::new(to_c(p.anchor), to_c(p.offset)))
    } else {
        Err(invalid("a touch place must be finite"))
    }
}

fn place_to_c(p: &TouchPlace) -> PonkoTouchPlace {
    PonkoTouchPlace {
        anchor: to_c(p.anchor),
        offset: to_c(p.offset),
    }
}

/// The engine's look of on-screen controls: see-through white discs, size
/// 1, labels 0.8 of a button's radius (Rust `TouchStyle::default`).
/// ```
/// assert_eq!(ponko_ffi::ponko_touch_style_default().label.a, 1.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_touch_style_default() -> PonkoTouchStyle {
    style_to_c(&TouchStyle::default())
}

/// Fills the fields of `desc`'s version with the defaults: no controls,
/// shown on touch devices, stepping aside under UI screens, the engine's
/// look, no styles or places of their own. Set `struct_size` and `version`
/// first.
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoTouchControlsDesc = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoTouchControlsDesc>() as u32, PONKO_TOUCH_CONTROLS_DESC_VERSION);
/// assert_eq!(unsafe { ponko_touch_controls_desc_defaults(&mut d) }, PonkoStatus::PONKO_OK);
/// assert_eq!(d.style.size, 1.0);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_touch_controls_desc_defaults(desc: *mut PonkoTouchControlsDesc) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(desc, &desc_defaults()) })
}

fn desc_defaults() -> PonkoTouchControlsDesc {
    PonkoTouchControlsDesc {
        struct_size: size_of::<PonkoTouchControlsDesc>() as u32,
        version: PONKO_TOUCH_CONTROLS_DESC_VERSION,
        controls: std::ptr::null(),
        count: 0,
        show: 0,
        over_screens: false,
        style: style_to_c(&TouchStyle::default()),
        styles: std::ptr::null(),
        style_count: 0,
        places: std::ptr::null(),
        place_count: 0,
    }
}

/// `count` items at `ptr`, or none.
///
/// # Safety
/// `ptr` readable for `count` items, or null with `count` 0.
unsafe fn items<'a, T>(ptr: *const T, count: usize, what: &'static str) -> Result<&'a [T], Failure> {
    if count == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(invalid(what));
    }
    // SAFETY: caller contract.
    Ok(unsafe { std::slice::from_raw_parts(ptr, count) })
}

/// Puts on-screen controls on the screen with their look and places (Rust
/// `Input::set_touch_controls` with `TouchControls::style`, `control_style`
/// and `place`), replacing any. The player's layout goes over the game's
/// places: a control the player moved stays where they put it. Bad kinds,
/// strings, styles or places: `PONKO_INVALID_ARGUMENT`, nothing changed.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_controls_ex(std::ptr::null_mut(), std::ptr::null()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `desc` null or a readable descriptor with its
/// header set, its arrays readable for their counts and their strings
/// NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_controls_ex(
    ctx: *mut PonkoCtx,
    desc: *const PonkoTouchControlsDesc,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let d = read_desc(desc, desc_defaults())?;
        let mut set = controls_from_c(d.controls, d.count, d.show, d.over_screens)?.style(style_from_c(&d.style)?);
        for s in items(d.styles, d.style_count, "styles is null")? {
            set = set.control_style(text(s.action)?, style_from_c(&s.style)?);
        }
        for p in items(d.places, d.place_count, "places is null")? {
            set = set.place(text(p.action)?, place_from_c(&p.place)?);
        }
        ctx_access(ctx, |c| {
            c.input.set_touch_controls(set);
            Ok(())
        })
    })
}

/// Turns the player's arranging of the on-screen controls on or off (Rust
/// `Input::edit_touch_controls`). While on, the controls show (also under a
/// UI screen, also off a touch screen), press no actions, and a finger or
/// the mouse dragging one moves it: where it is let go it stays, anchored to
/// the nearest corner, in the player's layout. Save it when the player is
/// done (`ponko_ctx_save_touch_layout`). Turning it off lets go of a
/// control being moved.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_edit_touch_controls(std::ptr::null_mut(), true) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_edit_touch_controls(ctx: *mut PonkoCtx, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.edit_touch_controls(on);
            Ok(())
        })
    })
}

/// Opens the engine's ready-made screen where the player arranges the
/// on-screen controls (Rust `Input::arrange_touch_controls`): a settings
/// page's "arrange controls" button. Edit mode starts at once; the screen
/// (UI nodes named `touch.arrange.<part>`: `title`, `note`, `reset`,
/// `cancel`, `save`, ...) is built at the UI's next pass. The player drags,
/// resizes and resets the controls; `Save` writes the layout to storage and
/// closes it, `Cancel` puts back the layout it opened with. Called again
/// while it is open, nothing changes.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_arrange_touch_controls(std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_arrange_touch_controls(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.arrange_touch_controls();
            Ok(())
        })
    })
}

/// Whether the player is arranging the on-screen controls (Rust
/// `Input::editing_touch_controls`).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_editing_touch_controls(std::ptr::null_mut(), &mut false) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `out_editing` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_editing_touch_controls(ctx: *mut PonkoCtx, out_editing: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_editing, c.input.editing_touch_controls())) })
}

/// Runs `f` on a copy of the player's layout and sets it back (the controls
/// take their places at once).
///
/// # Safety
/// Context live or null.
unsafe fn change_layout(ctx: *mut PonkoCtx, f: impl FnOnce(&mut TouchLayout) -> Result<(), Failure>) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let mut layout = c.input.touch_layout().clone();
            f(&mut layout)?;
            c.input.set_touch_layout(layout);
            Ok(())
        })
    })
}

/// The size and opacity of every control in the player's layout (Rust
/// `TouchLayout::size` and `opacity`): size times the game's, opacity from
/// 0 (invisible) to 1 (as the game draws them). Either output may be null.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touch_layout(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; each output null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touch_layout(
    ctx: *mut PonkoCtx,
    out_size: *mut f32,
    out_opacity: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let layout = c.input.touch_layout();
            if !out_size.is_null() {
                out(out_size, layout.size)?;
            }
            if !out_opacity.is_null() {
                out(out_opacity, layout.opacity)?;
            }
            Ok(())
        })
    })
}

/// Sets the size and opacity of every control in the player's layout (a
/// settings screen's sliders; Rust `Input::set_touch_layout`); each
/// control's own place and size stay. Sizes below a tenth are a tenth,
/// opacity is kept within 0 to 1. Not finite: `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_layout(std::ptr::null_mut(), 1.0, 0.5) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_layout(ctx: *mut PonkoCtx, size: f32, opacity: f32) -> PonkoStatus {
    if !(size.is_finite() && opacity.is_finite()) {
        return boundary(|| Err(invalid("touch layout size and opacity must be finite")));
    }
    // SAFETY: forwarded caller contract.
    unsafe {
        change_layout(ctx, |layout| {
            layout.size = size;
            layout.opacity = opacity;
            Ok(())
        })
    }
}

/// What the player changed about the control driving `action` (Rust
/// `TouchLayout::adjusted`): not placed and size 1 when nothing.
/// ```
/// let mut a = ponko_ffi::PonkoTouchAdjust::default();
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touch_layout_control(std::ptr::null_mut(), c"jump".as_ptr(), &mut a) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `action` NUL-terminated; `out_adjust` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touch_layout_control(
    ctx: *mut PonkoCtx,
    action: *const c_char,
    out_adjust: *mut PonkoTouchAdjust,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let action = text(action)?;
        ctx_access(ctx, |c| {
            let a = c.input.touch_layout().adjusted(action);
            out(
                out_adjust,
                PonkoTouchAdjust {
                    placed: a.place.is_some(),
                    place: a.place.as_ref().map(place_to_c).unwrap_or_default(),
                    size: a.size,
                },
            )
        })
    })
}

/// Sets what the player changed about the control driving `action` in
/// their layout (Rust `TouchLayout::adjust`): where it is (`placed`) and its
/// size. Null `adjust` forgets the player's changes to it, so it follows
/// the game again. Not finite: `PONKO_INVALID_ARGUMENT`, nothing changed.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_layout_control(std::ptr::null_mut(), c"jump".as_ptr(), std::ptr::null()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `action` NUL-terminated; `adjust` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_layout_control(
    ctx: *mut PonkoCtx,
    action: *const c_char,
    adjust: *const PonkoTouchAdjust,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        change_layout(ctx, |layout| {
            let action = text(action)?;
            match adjust.as_ref() {
                None => {
                    layout.controls.remove(action);
                }
                Some(a) => {
                    if !a.size.is_finite() {
                        return Err(invalid("a touch adjust's size must be finite"));
                    }
                    let place = if a.placed { Some(place_from_c(&a.place)?) } else { None };
                    *layout.adjust(action) = TouchAdjust { place, size: a.size };
                }
            }
            Ok(())
        })
    }
}

/// Resets the player's layout (Rust `TouchLayout::default`): every control
/// back where the game puts it, size and opacity 1.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_reset_touch_layout(std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_reset_touch_layout(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        change_layout(ctx, |layout| {
            *layout = TouchLayout::default();
            Ok(())
        })
    }
}

/// The player's layout as JSON (Rust `TouchLayout::to_json`), for a
/// settings file of the game's own, as NUL-terminated text: `*out_len` is
/// always its length; it is copied only when it fits.
/// ```
/// let mut n = 0;
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touch_layout_json(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `buffer` writable for `capacity` bytes (or null
/// with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touch_layout_json(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let json = ctx_access(ctx, |c| c.input.touch_layout().to_json().map_err(engine_error))?;
        write_text(buffer, capacity, out_len, &json)
    })
}

/// Replaces the player's layout with one read from `json`
/// (`ponko_ctx_touch_layout_json`'s text; Rust `TouchLayout::from_json`);
/// fields left out take their defaults. Not a touch layout: an error with
/// where it broke (kind `PONKO_ERROR_JSON`), nothing changed.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_layout_json(std::ptr::null_mut(), c"{}".as_ptr()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `json` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_layout_json(ctx: *mut PonkoCtx, json: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        change_layout(ctx, |layout| {
            *layout = TouchLayout::from_json(text(json)?).map_err(engine_error)?;
            Ok(())
        })
    }
}

/// Saves the player's layout in the game's storage as
/// `touch_controls.json`, next to the other settings (Rust
/// `Input::save_touch_layout`). The game needs an app id.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_save_touch_layout(std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_save_touch_layout(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.input.save_touch_layout(&mut c.storage).map_err(engine_error)) })
}

/// Reads back the layout `ponko_ctx_save_touch_layout` saved, at setup
/// (Rust `Input::restore_touch_layout`). No file (the player never moved a
/// control) leaves the layout as it is; a broken one is an error and
/// leaves it too.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_restore_touch_layout(std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_restore_touch_layout(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.input.restore_touch_layout(&c.storage).map_err(engine_error)) })
}
