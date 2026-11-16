//! Accessibility semantics of the retained UI from C (Rust
//! `Ui::accessibility_tree`, `UiAccessTracker`, `UiAccessAction`).
//!
//! What assistive technology reads of a UI: each shown node's role, name,
//! description, value and states, and which node has focus. Give sliders,
//! text inputs, images and icon-only buttons a name with
//! `ponko_ctx_ui_set_accessible_name`.
//!
//! A `PonkoUiAccess` is a snapshot of one UI that also knows what changed
//! since it last looked: call `ponko_ui_access_update` once a frame and read
//! the nodes from it. While nothing changed, an update allocates nothing.
//! `ponko_ctx_ui_access_action` asks the UI to do what a screen reader's
//! user asked for (press, focus, set a value); it is applied by the UI's
//! next frame and queues the same events as a key press.
use std::ffi::c_char;

use ponko::ui::{UiAccessAction, UiAccessChanges, UiAccessNode, UiAccessTracker, UiAccessValue, UiId, UiLive, UiRole};

use crate::ctx::out;
use crate::types::{PonkoRect, to_c};
use crate::ui_retained::{check_output, copy_string, existing, id_to_c, optional_text, with_ui};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid};

/// What a node is to assistive technology (`PonkoUiAccessNode::role`).
/// Rust's `UiRole` can gain roles in a minor release; until ABI 2 has a
/// constant for one, such a node reads as `PONKO_UI_ROLE_GROUP` here (its
/// text, `ponko_ctx_ui_access_text`, names the real role).
/// ```
/// assert_eq!(ponko_ffi::PonkoUiRole::PONKO_UI_ROLE_SLIDER as u32, 8);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiRole {
    /// A container: a panel or a non-modal screen root.
    #[default]
    PONKO_UI_ROLE_GROUP,
    /// The root of a modal screen.
    PONKO_UI_ROLE_DIALOG,
    /// Static text.
    PONKO_UI_ROLE_LABEL,
    /// Activates on press or confirm.
    PONKO_UI_ROLE_BUTTON,
    /// A picture.
    PONKO_UI_ROLE_IMAGE,
    /// A scrolling container.
    PONKO_UI_ROLE_SCROLL_VIEW,
    /// Single-line editable text; its text is the value, not the name.
    PONKO_UI_ROLE_TEXT_INPUT,
    /// An on/off toggle.
    PONKO_UI_ROLE_CHECKBOX,
    /// A value from 0 to 1.
    PONKO_UI_ROLE_SLIDER,
}

/// Which kind of value a node has (`PonkoUiAccessNode::value_kind`). A kind
/// of value Rust gains in a minor release reads as
/// `PONKO_UI_ACCESS_VALUE_NONE` until this enum has a constant for it.
/// ```
/// assert_eq!(ponko_ffi::PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_RANGE as u32, 3);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiAccessValueKind {
    /// No value (groups, labels, buttons, images).
    #[default]
    PONKO_UI_ACCESS_VALUE_NONE,
    /// A text input's text: read it with `ponko_ui_access_value_text`.
    PONKO_UI_ACCESS_VALUE_TEXT,
    /// A toggle: `value` is 1 when on, 0 when off.
    PONKO_UI_ACCESS_VALUE_CHECKED,
    /// A slider: `value` is 0 to 1.
    PONKO_UI_ACCESS_VALUE_RANGE,
}

/// Whether a node's changes are announced while something else has focus.
/// ```
/// assert_eq!(ponko_ffi::PonkoUiLive::PONKO_UI_LIVE_ASSERTIVE as u32, 2);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiLive {
    /// Not announced on its own (the default).
    #[default]
    PONKO_UI_LIVE_OFF,
    /// Announced when the screen reader is next idle.
    PONKO_UI_LIVE_POLITE,
    /// Announced at once, interrupting speech.
    PONKO_UI_LIVE_ASSERTIVE,
}

/// What `ponko_ctx_ui_access_action` asks the UI to do.
/// ```
/// assert_eq!(ponko_ffi::PonkoUiAccessActionKind::PONKO_UI_ACCESS_ACTION_SET_TEXT as u32, 5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiAccessActionKind {
    /// Move keyboard focus to the node.
    #[default]
    PONKO_UI_ACCESS_ACTION_FOCUS,
    /// Press a button, flip a toggle, submit a text input; focuses it too.
    PONKO_UI_ACCESS_ACTION_ACTIVATE,
    /// Set a slider (`value`, clamped to 0..1) or a toggle (on from 0.5).
    PONKO_UI_ACCESS_ACTION_SET_VALUE,
    /// Move a slider up one step (0.05).
    PONKO_UI_ACCESS_ACTION_INCREMENT,
    /// Move a slider down one step (0.05).
    PONKO_UI_ACCESS_ACTION_DECREMENT,
    /// Replace a text input's text with `text`.
    PONKO_UI_ACCESS_ACTION_SET_TEXT,
}

/// One exposed node of a `PonkoUiAccess` snapshot. Its name, description and
/// text value are strings: read them with `ponko_ui_access_name`,
/// `ponko_ui_access_description` and `ponko_ui_access_value_text`.
/// ```
/// let node = ponko_ffi::PonkoUiAccessNode::default();
/// assert_eq!(node.role, ponko_ffi::PonkoUiRole::PONKO_UI_ROLE_GROUP);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoUiAccessNode {
    /// The UI node.
    pub id: u64,
    /// The exposed parent; `PONKO_UI_NONE` for a screen root.
    pub parent: u64,
    /// A text input's selection anchor, as a byte offset into its text.
    pub selection_anchor: u64,
    /// A text input's caret, as a byte offset into its text; equal to the
    /// anchor when nothing is selected.
    pub selection_caret: u64,
    /// Bounds in logical window coordinates, including the user scale.
    pub rect: PonkoRect,
    /// What the node is.
    pub role: PonkoUiRole,
    /// Which kind of value it has.
    pub value_kind: PonkoUiAccessValueKind,
    /// A toggle's state (0 or 1) or a slider's value (0 to 1); else 0.
    pub value: f32,
    /// Whether its changes are announced without focus.
    pub live: PonkoUiLive,
    /// Nesting below the screen root (which is 0).
    pub depth: u32,
    /// How many exposed children it has (`ponko_ui_access_child`).
    pub child_count: u32,
    /// It is a text input: the selection fields are meaningful.
    pub has_selection: bool,
    /// Keyboard and gamepad navigation can focus it right now.
    pub focusable: bool,
    /// It has keyboard focus.
    pub focused: bool,
    /// It or an ancestor is disabled.
    pub disabled: bool,
    /// It is on a screen below the top one: readable, not interactive.
    pub inert: bool,
    /// It is the root of a modal screen.
    pub modal: bool,
}

/// What changed between two `ponko_ui_access_update` calls. The ids are read
/// with `ponko_ui_access_changed` and `ponko_ui_access_removed`.
/// ```
/// assert!(!ponko_ffi::PonkoUiAccessChanges::default().any);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoUiAccessChanges {
    /// Nodes that are new, or whose role, name, description, value,
    /// selection, bounds, state or children changed.
    pub changed_count: u32,
    /// Nodes that are no longer exposed.
    pub removed_count: u32,
    /// Something changed. False: the snapshot is the one from last time.
    pub any: bool,
    /// The list of exposed screen roots changed.
    pub screens_changed: bool,
    /// The focused node changed.
    pub focus_changed: bool,
}

/// A snapshot of one UI's accessibility semantics that knows what changed
/// since its last update. Made by `ponko_ui_access_new`, freed with
/// `ponko_ui_access_free`.
/// ```
/// let access = ponko_ffi::ponko_ui_access_new();
/// unsafe { ponko_ffi::ponko_ui_access_free(access) };
/// ```
pub struct PonkoUiAccess {
    tracker: UiAccessTracker,
    last: UiAccessChanges,
}

fn role_to_c(role: UiRole) -> PonkoUiRole {
    match role {
        UiRole::Group => PonkoUiRole::PONKO_UI_ROLE_GROUP,
        UiRole::Dialog => PonkoUiRole::PONKO_UI_ROLE_DIALOG,
        UiRole::Label => PonkoUiRole::PONKO_UI_ROLE_LABEL,
        UiRole::Button => PonkoUiRole::PONKO_UI_ROLE_BUTTON,
        UiRole::Image => PonkoUiRole::PONKO_UI_ROLE_IMAGE,
        UiRole::ScrollView => PonkoUiRole::PONKO_UI_ROLE_SCROLL_VIEW,
        UiRole::TextInput => PonkoUiRole::PONKO_UI_ROLE_TEXT_INPUT,
        UiRole::CheckBox => PonkoUiRole::PONKO_UI_ROLE_CHECKBOX,
        UiRole::Slider => PonkoUiRole::PONKO_UI_ROLE_SLIDER,
        // A role added in Rust before it has a constant here: the generic
        // container is the closest safe description.
        _ => PonkoUiRole::PONKO_UI_ROLE_GROUP,
    }
}

fn live_to_c(live: UiLive) -> PonkoUiLive {
    match live {
        UiLive::Off => PonkoUiLive::PONKO_UI_LIVE_OFF,
        UiLive::Polite => PonkoUiLive::PONKO_UI_LIVE_POLITE,
        UiLive::Assertive => PonkoUiLive::PONKO_UI_LIVE_ASSERTIVE,
    }
}

fn node_to_c(n: &UiAccessNode) -> PonkoUiAccessNode {
    let (value_kind, value) = match &n.value {
        UiAccessValue::None => (PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_NONE, 0.0),
        UiAccessValue::Text(_) => (PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_TEXT, 0.0),
        UiAccessValue::Checked(on) => (
            PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_CHECKED,
            f32::from(u8::from(*on)),
        ),
        UiAccessValue::Range(v) => (PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_RANGE, *v),
        // A kind of value added in Rust before it has a constant here.
        _ => (PonkoUiAccessValueKind::PONKO_UI_ACCESS_VALUE_NONE, 0.0),
    };
    let (anchor, caret) = n.selection.unwrap_or((0, 0));
    PonkoUiAccessNode {
        id: n.id.bits(),
        parent: id_to_c(n.parent),
        selection_anchor: anchor as u64,
        selection_caret: caret as u64,
        rect: PonkoRect {
            min: to_c(n.rect.min),
            max: to_c(n.rect.max),
        },
        role: role_to_c(n.role),
        value_kind,
        value,
        live: live_to_c(n.live),
        depth: n.depth as u32,
        child_count: n.children.len() as u32,
        has_selection: n.selection.is_some(),
        focusable: n.focusable,
        focused: n.focused,
        disabled: n.disabled,
        inert: n.inert,
        modal: n.modal,
    }
}

/// The live handle, or a failure for null.
///
/// # Safety
/// `access` is null or a live handle not used elsewhere during the call.
unsafe fn handle<'a>(access: *mut PonkoUiAccess) -> Result<&'a mut PonkoUiAccess, Failure> {
    // SAFETY: forwarded caller contract.
    unsafe { access.as_mut() }.ok_or_else(|| invalid("access is null"))
}

/// The snapshot's node for `id`.
fn exposed(access: &PonkoUiAccess, id: u64) -> Result<&UiAccessNode, Failure> {
    access
        .tracker
        .tree()
        .get(UiId::from_bits(id))
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "UI node is not exposed"))
}

// ---- names, descriptions and live regions ---------------------------------

/// Overrides the name assistive technology reads for a node (Rust
/// `Ui::set_accessible_name`). By default it is the node's text (none for
/// text inputs, whose text is their value): sliders, text inputs, images and
/// icon-only buttons need one. Empty or null restores the default. Names
/// never change layout, drawing, focus or events.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_accessible_name(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, c"Volume".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_accessible_name(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    name: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let name = optional_text(name)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_accessible_name(id, name);
            Ok(())
        })
    })
}

/// Reads the name assistive technology reads for a node into `buffer`, the
/// way `ponko_ctx_ui_text` reads its text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_accessible_name(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_accessible_name(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            copy_string(ui.accessible_name(id).unwrap_or(""), buffer, capacity, out_len)
        })
    })
}

/// Sets what assistive technology reads after a node's name: what a control
/// does, or a hint (Rust `Ui::set_accessible_description`). Empty or null
/// removes it.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_accessible_description(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, c"Deletes every save".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `description` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_accessible_description(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    description: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let description = optional_text(description)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_accessible_description(id, description);
            Ok(())
        })
    })
}

/// Reads a node's accessible description into `buffer`, the way
/// `ponko_ctx_ui_text` reads its text; a node without one gives an empty
/// string.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_accessible_description(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_accessible_description(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            copy_string(ui.accessible_description(id).unwrap_or(""), buffer, capacity, out_len)
        })
    })
}

/// Has a node's changes announced while something else has focus (Rust
/// `Ui::set_live_region`). `live` is a `PonkoUiLive`; use it for a status
/// line, not for something that changes every frame.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_live_region(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_live_region(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    live: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let live = match live {
            0 => UiLive::Off,
            1 => UiLive::Polite,
            2 => UiLive::Assertive,
            _ => return Err(invalid("unknown live region setting")),
        };
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_live_region(id, live);
            Ok(())
        })
    })
}

/// Writes a node's live-region setting to `out_live`.
/// ```
/// let mut live = ponko_ffi::PonkoUiLive::PONKO_UI_LIVE_OFF;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_live_region(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut live) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_live` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_live_region(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_live: *mut PonkoUiLive,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_live.is_null() {
            return Err(invalid("output is null"));
        }
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_live, live_to_c(ui.live_region(id).unwrap_or_default()))
        })
    })
}

// ---- requests and the readable text ---------------------------------------

/// Queues a request from assistive technology for the UI's next frame (Rust
/// `Ui::push_access_action`). `kind` is a `PonkoUiAccessActionKind`; `value`
/// is read by `SET_VALUE` and `text` (null: empty) by `SET_TEXT`. The request
/// does what the matching key does and queues the same events; one for a
/// node keyboard navigation couldn't reach right now does nothing.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_access_action(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 1, 0, 0.0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `text` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_access_action(
    ctx: *mut PonkoCtx,
    target: u64,
    kind: u32,
    id: u64,
    value: f32,
    text: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let text = optional_text(text)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.push_access_action(match kind {
                0 => UiAccessAction::Focus(id),
                1 => UiAccessAction::Activate(id),
                2 => UiAccessAction::SetValue(id, value),
                3 => UiAccessAction::Increment(id),
                4 => UiAccessAction::Decrement(id),
                5 => UiAccessAction::SetText(id, text.to_owned()),
                _ => return Err(invalid("unknown accessibility action")),
            });
            Ok(())
        })
    })
}

/// Reads the UI's accessibility semantics as text into `buffer`, the way
/// `ponko_ctx_ui_text` reads a node's text: one line per exposed node,
/// indented by depth (`button "Play" focusable focused`). For golden tests
/// and tools.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_access_text(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_access_text(
    ctx: *mut PonkoCtx,
    target: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        with_ui(ctx, target, |ui| {
            copy_string(&ui.accessibility_tree().to_string(), buffer, capacity, out_len)
        })
    })
}

// ---- the snapshot ----------------------------------------------------------

/// An empty snapshot that has seen nothing: its first update reports every
/// node. Free once with `ponko_ui_access_free`.
/// ```
/// let access = ponko_ffi::ponko_ui_access_new();
/// assert!(!access.is_null());
/// unsafe { ponko_ffi::ponko_ui_access_free(access) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_access_new() -> *mut PonkoUiAccess {
    let mut result = std::ptr::null_mut();
    boundary(|| {
        result = Box::into_raw(Box::new(PonkoUiAccess {
            tracker: UiAccessTracker::new(),
            last: UiAccessChanges::default(),
        }));
        Ok(())
    });
    result
}

/// Frees a snapshot; null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_ui_access_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// Null, or an owned, live handle not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_free(access: *mut PonkoUiAccess) {
    if !access.is_null() {
        // SAFETY: caller transfers the owned handle exactly once.
        drop(unsafe { Box::from_raw(access) });
    }
}

/// Looks at the UI of `target` and writes what changed since the last look
/// to `out_changes` (Rust `UiAccessTracker::update`). Call it after the UI's
/// frame. When `any` is false nothing changed and nothing was allocated; the
/// first look reports every exposed node as changed. Use one snapshot per
/// UI.
/// ```
/// let mut changes = ponko_ffi::PonkoUiAccessChanges::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_update(std::ptr::null_mut(), std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut changes) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `ctx` null or live; `out_changes` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_update(
    access: *mut PonkoUiAccess,
    ctx: *mut PonkoCtx,
    target: u64,
    out_changes: *mut PonkoUiAccessChanges,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let access = handle(access)?;
        if out_changes.is_null() {
            return Err(invalid("output is null"));
        }
        with_ui(ctx, target, |ui| {
            let changes = access.tracker.update(ui);
            let summary = changes
                .as_ref()
                .map_or_else(PonkoUiAccessChanges::default, |c| PonkoUiAccessChanges {
                    changed_count: c.changed.len() as u32,
                    removed_count: c.removed.len() as u32,
                    any: true,
                    screens_changed: c.screens_changed,
                    focus_changed: c.focus_changed,
                });
            access.last = changes.unwrap_or_default();
            out(out_changes, summary)
        })
    })
}

/// Forgets what the snapshot saw: the next update reports every node again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_reset(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_reset(access: *mut PonkoUiAccess) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let access = handle(access)?;
        access.tracker.reset();
        access.last = UiAccessChanges::default();
        Ok(())
    })
}

/// Writes how many nodes the snapshot exposes to `out_count`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_count(std::ptr::null_mut(), &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_count(access: *mut PonkoUiAccess, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_count, handle(access)?.tracker.tree().nodes().len()) })
}

/// Writes the snapshot's node number `index` to `out_node`. Nodes are in
/// tree order: depth-first, screens from the bottom up, a parent before its
/// children. An index past the end is `PONKO_NOT_FOUND`.
/// ```
/// let mut node = ponko_ffi::PonkoUiAccessNode::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_node(std::ptr::null_mut(), 0, &mut node) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_node` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_node(
    access: *mut PonkoUiAccess,
    index: usize,
    out_node: *mut PonkoUiAccessNode,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let node = handle(access)?
            .tracker
            .tree()
            .nodes()
            .get(index)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no exposed node at that index"))?;
        out(out_node, node_to_c(node))
    })
}

/// Writes the snapshot's node for the UI node `id` to `out_node`. A node
/// that is hidden, removed or covered by a modal screen is not exposed:
/// `PONKO_NOT_FOUND`.
/// ```
/// let mut node = ponko_ffi::PonkoUiAccessNode::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_find(std::ptr::null_mut(), 0, &mut node) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_node` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_find(
    access: *mut PonkoUiAccess,
    id: u64,
    out_node: *mut PonkoUiAccessNode,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_node, node_to_c(exposed(handle(access)?, id)?)) })
}

/// Writes the id of an exposed node's child number `index` (below its
/// `child_count`) to `out_child`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_child(std::ptr::null_mut(), 0, 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_child` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_child(
    access: *mut PonkoUiAccess,
    id: u64,
    index: usize,
    out_child: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let child = exposed(handle(access)?, id)?
            .children
            .get(index)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no child at that index"))?;
        out(out_child, child.bits())
    })
}

/// Reads an exposed node's accessible name into `buffer`, the way
/// `ponko_ctx_ui_text` reads a node's text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_name(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `buffer` writable for `capacity` bytes (or null
/// with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_name(
    access: *mut PonkoUiAccess,
    id: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        copy_string(&exposed(handle(access)?, id)?.name, buffer, capacity, out_len)
    })
}

/// Reads an exposed node's description into `buffer`; empty without one.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_description(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `buffer` writable for `capacity` bytes (or null
/// with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_description(
    access: *mut PonkoUiAccess,
    id: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        copy_string(&exposed(handle(access)?, id)?.description, buffer, capacity, out_len)
    })
}

/// Reads an exposed text input's text (its value) into `buffer`; empty for
/// a node whose value is not text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_value_text(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `buffer` writable for `capacity` bytes (or null
/// with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_value_text(
    access: *mut PonkoUiAccess,
    id: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        let text = match &exposed(handle(access)?, id)?.value {
            UiAccessValue::Text(text) => text.as_str(),
            _ => "",
        };
        copy_string(text, buffer, capacity, out_len)
    })
}

/// Writes the focused exposed node to `out_id`, or `PONKO_UI_NONE`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_focus(std::ptr::null_mut(), &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_focus(access: *mut PonkoUiAccess, out_id: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_id, id_to_c(handle(access)?.tracker.tree().focus())) })
}

/// Writes the id of changed node number `index` of the last update (below
/// its `changed_count`) to `out_id`. Changed nodes are in tree order.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_changed(std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_changed(
    access: *mut PonkoUiAccess,
    index: usize,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let id = handle(access)?
            .last
            .changed
            .get(index)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no changed node at that index"))?;
        out(out_id, id.bits())
    })
}

/// Writes the id of removed node number `index` of the last update (below
/// its `removed_count`) to `out_id`: a node that is no longer exposed.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_access_removed(std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `access` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_access_removed(
    access: *mut PonkoUiAccess,
    index: usize,
    out_id: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let id = handle(access)?
            .last
            .removed
            .get(index)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no removed node at that index"))?;
        out(out_id, id.bits())
    })
}
