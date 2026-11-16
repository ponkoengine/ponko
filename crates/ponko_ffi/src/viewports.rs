//! Several cameras in one window (Rust `ponko::camera::{Viewport,
//! RenderLayers}`, `ctx.views2d`, `ctx.scene3d.views`): split screen, a
//! minimap, a picture in picture.
//!
//! A camera draws into its rectangle of the window (`PonkoViewport`, in
//! fractions of the window) the entities on one of its render layers (bits
//! of a `uint32_t`; an entity without layers is on layer 0). The main
//! cameras draw first, then each view in order. The views are versioned
//! descriptors (STABILITY.md, "Versioned descriptors"; the reading rules
//! are in `descriptors.rs`): set the header, fill one with its `_defaults`
//! call (or read a camera into it), change what you want, then set it.
//!
//! ```c
//! PonkoView3D p2 = {.struct_size = sizeof p2, .version = PONKO_VIEW3D_VERSION};
//! ponko_ctx_camera3d_view(ctx, &p2);           // start from the main camera
//! p2.rect = ponko_viewport_split(2, 1);         // the right half
//! ponko_ctx_set_view3d(ctx, 0, &p2);            // add it as view 0
//! PonkoView3D p1 = {.struct_size = sizeof p1, .version = PONKO_VIEW3D_VERSION};
//! ponko_ctx_camera3d_view(ctx, &p1);
//! p1.rect = ponko_viewport_split(2, 0);
//! ponko_ctx_set_camera3d_view(ctx, &p1);        // the main camera: the left half
//! ```

use std::mem::{offset_of, size_of};

use ponko::camera::{RenderLayers, Viewport};
use ponko::d2::Camera as Camera2D;
use ponko::d2::View as View2D;
use ponko::d3::View as View3D;
use ponko::prelude::{Color, Ctx, Vec2};
use ponko::render::Post;

use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::three::{PonkoCamera3D, PonkoPost, camera_from_c, camera_to_c, post_from_c, post_to_c};
use crate::types::{PonkoColor, PonkoVec2, to_c};
use crate::{Failure, PonkoCamera, PonkoCtx, PonkoStatus, PonkoWorld, access, boundary, error, invalid};

/// The render layers of an entity without any: layer 0.
pub const PONKO_RENDER_LAYERS_DEFAULT: u32 = 1;
/// Every render layer: what a camera draws unless told otherwise.
pub const PONKO_RENDER_LAYERS_ALL: u32 = u32::MAX;

/// A part of the window, in fractions of its size (Rust
/// `ponko::camera::Viewport`): `{0, 0, 1, 1}` is the whole window, `{0.5,
/// 0, 0.5, 1}` its right half. The same layout as the Rust type.
/// ```
/// let right = ponko_ffi::ponko_viewport_split(2, 1);
/// assert_eq!((right.x, right.width), (0.5, 0.5));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoViewport {
    /// The left edge, 0 (the window's left) to 1 (its right).
    pub x: f32,
    /// The top edge, 0 (the window's top) to 1 (its bottom).
    pub y: f32,
    /// The width, as a fraction of the window's.
    pub width: f32,
    /// The height, as a fraction of the window's.
    pub height: f32,
}

const _: () = {
    assert!(size_of::<PonkoViewport>() == size_of::<Viewport>());
    assert!(offset_of!(PonkoViewport, height) == offset_of!(Viewport, height));
};

fn viewport_to_c(v: Viewport) -> PonkoViewport {
    PonkoViewport {
        x: v.x,
        y: v.y,
        width: v.width,
        height: v.height,
    }
}

fn viewport_from_c(v: PonkoViewport) -> Result<Viewport, Failure> {
    if ![v.x, v.y, v.width, v.height].iter().all(|f| f.is_finite()) {
        return Err(invalid("viewport: every number must be finite"));
    }
    Ok(Viewport::new(v.x, v.y, v.width, v.height))
}

/// Player `index`'s part of a split screen for `players` players (Rust
/// `Viewport::split`): the whole window for one, the left and right halves
/// for two, the quarters for three and four (left to right, top to
/// bottom), equal columns for more.
#[unsafe(no_mangle)]
pub extern "C" fn ponko_viewport_split(players: u32, index: u32) -> PonkoViewport {
    viewport_to_c(Viewport::split(players, index))
}

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// Implements `Versioned` for a version-1 descriptor ending at `$last`.
macro_rules! versioned_v1 {
    ($t:ty, $name:literal, $version:ident, $last:ident: $lt:ty) => {
        const _: () = assert!(offset_of!($t, struct_size) == 0 && offset_of!($t, version) == 4);
        impl Versioned for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = $version;
            fn len_of(version: u32) -> Option<usize> {
                match version {
                    1 => Some(end_of!(Self, $last: $lt)),
                    _ => None,
                }
            }
        }
    };
}

// ---- 2D -------------------------------------------------------------------------

/// The version of `PonkoView2D` this header describes.
pub const PONKO_VIEW2D_VERSION: u32 = 1;

/// A 2D camera and where it draws (Rust `d2::View`, or the main
/// `ctx.camera` with `ponko_ctx_camera_view`). A versioned descriptor.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut v: PonkoView2D = unsafe { std::mem::zeroed() };
/// (v.struct_size, v.version) = (size_of::<PonkoView2D>() as u32, PONKO_VIEW2D_VERSION);
/// unsafe { assert_eq!(ponko_view2d_defaults(&mut v), PonkoStatus::PONKO_OK) };
/// assert_eq!((v.camera.zoom, v.rect.width, v.layers), (1.0, 1.0, PONKO_RENDER_LAYERS_ALL));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoView2D {
    /// `sizeof(PonkoView2D)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_VIEW2D_VERSION` as the caller compiled it.
    pub version: u32,
    /// Where the camera is and its zoom. Default: at 0, 0, zoom 1.
    pub camera: PonkoCamera,
    /// Its part of the window. Default: the whole window.
    pub rect: PonkoViewport,
    /// The render layers it draws. Default: all.
    pub layers: u32,
    /// Fill its rectangle with `clear` before drawing (views only; the
    /// main camera's clear is the game's). Default false.
    pub has_clear: bool,
    /// See `has_clear`.
    pub clear: PonkoColor,
}
versioned_v1!(PonkoView2D, "PonkoView2D", PONKO_VIEW2D_VERSION, clear: PonkoColor);

fn view2d_to_c(camera: &Camera2D, clear: Option<Color>) -> PonkoView2D {
    PonkoView2D {
        struct_size: size_of::<PonkoView2D>() as u32,
        version: PONKO_VIEW2D_VERSION,
        camera: PonkoCamera {
            position: to_c(camera.position),
            zoom: camera.zoom,
        },
        rect: viewport_to_c(camera.rect),
        layers: camera.layers.0,
        has_clear: clear.is_some(),
        clear: to_c(clear.unwrap_or(Color::BLACK)),
    }
}

/// The defaults: a camera at 0, 0, zoom 1, the whole window, every layer,
/// no clear.
fn default_view2d() -> PonkoView2D {
    PonkoView2D {
        struct_size: size_of::<PonkoView2D>() as u32,
        version: PONKO_VIEW2D_VERSION,
        camera: PonkoCamera {
            position: PonkoVec2 { x: 0.0, y: 0.0 },
            zoom: 1.0,
        },
        rect: viewport_to_c(Viewport::FULL),
        layers: RenderLayers::ALL.0,
        has_clear: false,
        clear: to_c(Color::BLACK),
    }
}

/// `base` changed by `v`, checked; and its clear.
fn view2d_from_c(mut base: Camera2D, v: &PonkoView2D) -> Result<(Camera2D, Option<Color>), Failure> {
    if !(v.camera.zoom > 0.0 && v.camera.zoom.is_finite()) {
        return Err(invalid("PonkoView2D: zoom must be positive"));
    }
    let position: Vec2 = to_c(v.camera.position);
    if !position.is_finite() {
        return Err(invalid("PonkoView2D: position must be finite"));
    }
    base.position = position;
    base.zoom = v.camera.zoom;
    base.rect = viewport_from_c(v.rect)?;
    base.layers = RenderLayers(v.layers);
    Ok((base, v.has_clear.then(|| to_c(v.clear))))
}

/// Fills the fields of `view`'s version with the defaults (a camera at
/// 0, 0, zoom 1, the whole window, every layer, no clear). `struct_size`
/// and `version` must be set first.
/// # Safety
/// `view` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_view2d_defaults(view: *mut PonkoView2D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(view, &default_view2d()) })
}

/// Reads the main 2D camera (`ctx.camera`) as a view: its position, zoom,
/// rectangle and layers (`has_clear` false).
/// # Safety
/// `ctx` null or live; `out_view` null or a writable descriptor with its
/// header set.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_camera_view(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera_view(ctx: *mut PonkoCtx, out_view: *mut PonkoView2D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| Ok(view2d_to_c(&c.camera, None)))?;
        write_desc(out_view, &v)
    })
}

/// Sets the main 2D camera from a view: its position, zoom, rectangle and
/// layers (the clear is ignored: the game's clear color is the main
/// camera's). Zoom must be positive and the rectangle finite.
/// # Safety
/// `ctx` null or live; `view` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_camera_view(ctx: *mut PonkoCtx, view: *const PonkoView2D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = read_desc(view, default_view2d())?;
        ctx_access(ctx, |c| {
            c.camera = view2d_from_c(c.camera, &v)?.0;
            Ok(())
        })
    })
}

/// How many 2D views there are (`ctx.views2d`).
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_view2d_count(ctx: *mut PonkoCtx, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let n = ctx_access(ctx, |c| Ok(c.views2d.len()))?;
        out(out_count, n)
    })
}

fn no_view() -> Failure {
    error(
        PonkoStatus::PONKO_NOT_FOUND,
        "no view at that index: it is past the last one",
    )
}

/// Reads 2D view `index`. `PONKO_NOT_FOUND` past the last one.
/// # Safety
/// `ctx` null or live; `out_view` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_view2d(ctx: *mut PonkoCtx, index: usize, out_view: *mut PonkoView2D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| {
            let view = c.views2d.get(index).ok_or_else(no_view)?;
            Ok(view2d_to_c(&view.camera, view.clear))
        })?;
        write_desc(out_view, &v)
    })
}

/// Sets 2D view `index` from `view`; `index` equal to the count adds one
/// at the end (past it: `PONKO_NOT_FOUND`). Views draw after the main
/// camera, in order. Zoom must be positive and the rectangle finite.
/// # Safety
/// `ctx` null or live; `view` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_view2d(
    ctx: *mut PonkoCtx,
    index: usize,
    view: *const PonkoView2D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = read_desc(view, default_view2d())?;
        ctx_access(ctx, |c| {
            let count = c.views2d.len();
            let base = c.views2d.get(index).map_or(c.camera, |v| v.camera);
            let (camera, clear) = view2d_from_c(base, &v)?;
            let new = View2D { camera, clear };
            match index.cmp(&count) {
                std::cmp::Ordering::Less => c.views2d[index] = new,
                std::cmp::Ordering::Equal => c.views2d.push(new),
                std::cmp::Ordering::Greater => return Err(no_view()),
            }
            Ok(())
        })
    })
}

/// Removes 2D view `index`; the ones after it move down one.
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_view2d(ctx: *mut PonkoCtx, index: usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if index >= c.views2d.len() {
                return Err(no_view());
            }
            c.views2d.remove(index);
            Ok(())
        })
    })
}

/// Which 2D camera draws the window at `screen` (Rust `d2::View::camera_at`):
/// the index of the last view whose rectangle holds it, or -1 for the main
/// camera (also outside every rectangle). Convert with that camera's
/// position and zoom, or `ponko_ctx_screen_to_world_at`.
/// # Safety
/// `ctx` null or live; `out_view` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera_at(ctx: *mut PonkoCtx, screen: PonkoVec2, out_view: *mut i32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let at = ctx_access(ctx, |c| Ok(view_at(c, to_c(screen))))?;
        out(out_view, at)
    })
}

fn view_at(c: &Ctx, screen: Vec2) -> i32 {
    let camera = ponko::d2::View::camera_at(&c.camera, &c.views2d, screen);
    c.views2d
        .iter()
        .position(|v| std::ptr::eq(&v.camera, camera))
        .map_or(-1, |i| i as i32)
}

/// The 2D world point under window point `screen`, through the camera that
/// draws there (Rust
/// `d2::View::camera_at(&ctx.camera, &ctx.views2d, screen).screen_to_world(screen)`).
/// # Safety
/// `ctx` null or live; `out_world` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_screen_to_world_at(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    out_world: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = ctx_access(ctx, |c| {
            let screen = to_c(screen);
            Ok(ponko::d2::View::camera_at(&c.camera, &c.views2d, screen).screen_to_world(screen))
        })?;
        out(out_world, to_c(p))
    })
}

/// Puts what this frame draws in the world from now on on `layers` (Rust
/// `ctx.gfx.set_layers`): a 2D camera draws it if its layers share one.
/// Each frame starts on layer 0 (`PONKO_RENDER_LAYERS_DEFAULT`).
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_draw_layers(ctx: *mut PonkoCtx, layers: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.gfx.set_layers(RenderLayers(layers));
            Ok(())
        })
    })
}

// ---- entities ---------------------------------------------------------------------

/// Puts `entity` (a 3D model or a 2D sprite) on `layers` (Rust
/// `RenderLayers`): a camera draws it if its layers share one. A 3D model
/// a camera doesn't draw still casts its shadow there.
/// `PONKO_RENDER_LAYERS_DEFAULT` takes the component off.
/// # Safety
/// `world` null or live.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { assert_eq!(ponko_world_set_render_layers(w, 42, 2), PonkoStatus::PONKO_NOT_FOUND); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_set_render_layers(
    world: *mut PonkoWorld,
    entity: u64,
    layers: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        access(world, |w, _| {
            let e = ponko_ecs::Entity::from_bits(entity);
            if !w.is_alive(e) {
                return Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity not found"));
            }
            if layers == PONKO_RENDER_LAYERS_DEFAULT {
                w.remove::<RenderLayers>(e);
            } else {
                w.insert(e, RenderLayers(layers));
            }
            Ok(())
        })
    })
}

/// The render layers `entity` is on (`PONKO_RENDER_LAYERS_DEFAULT` without
/// the component).
/// # Safety
/// `world` null or live; `out_layers` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_render_layers(
    world: *mut PonkoWorld,
    entity: u64,
    out_layers: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let layers = access(world, |w, _| {
            let e = ponko_ecs::Entity::from_bits(entity);
            if !w.is_alive(e) {
                return Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity not found"));
            }
            Ok(w.get::<RenderLayers>(e).copied().unwrap_or_default().0)
        })?;
        out(out_layers, layers)
    })
}

// ---- 3D -------------------------------------------------------------------------

/// The version of `PonkoView3D` this header describes.
pub const PONKO_VIEW3D_VERSION: u32 = 1;

/// A 3D camera and where it draws (Rust `d3::View`, or the main
/// `ctx.scene3d.camera` with `ponko_ctx_camera3d_view`). A versioned
/// descriptor. Each view is a whole frame of its camera, at its
/// rectangle's size: its own culling and temporal history, the sun's
/// shadow maps shared with the others.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut v: PonkoView3D = unsafe { std::mem::zeroed() };
/// (v.struct_size, v.version) = (size_of::<PonkoView3D>() as u32, PONKO_VIEW3D_VERSION);
/// unsafe { assert_eq!(ponko_view3d_defaults(&mut v), PonkoStatus::PONKO_OK) };
/// assert_eq!((v.rect.height, v.has_post), (1.0, false));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoView3D {
    /// `sizeof(PonkoView3D)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_VIEW3D_VERSION` as the caller compiled it.
    pub version: u32,
    /// Where it is, where it looks, its field of view and depth range.
    /// Default: the engine's default camera.
    pub camera: PonkoCamera3D,
    /// Its part of the window. Default: the whole window.
    pub rect: PonkoViewport,
    /// The render layers it draws. Default: all.
    pub layers: u32,
    /// The color behind a flat sky, instead of the game's (views only).
    /// Default false.
    pub has_clear: bool,
    /// See `has_clear`.
    pub clear: PonkoColor,
    /// Its own post-processing, instead of the main camera's (views only;
    /// the MSAA sample count is the game's). Default false.
    pub has_post: bool,
    /// See `has_post`. Default: the engine's default post settings.
    pub post: PonkoPost,
}
versioned_v1!(PonkoView3D, "PonkoView3D", PONKO_VIEW3D_VERSION, post: PonkoPost);

fn view3d_to_c(view: &View3D) -> PonkoView3D {
    PonkoView3D {
        struct_size: size_of::<PonkoView3D>() as u32,
        version: PONKO_VIEW3D_VERSION,
        camera: camera_to_c(&view.camera),
        rect: viewport_to_c(view.camera.rect),
        layers: view.camera.layers.0,
        has_clear: view.clear.is_some(),
        clear: to_c(view.clear.unwrap_or(Color::BLACK)),
        has_post: view.post.is_some(),
        post: post_to_c(view.post.as_ref().unwrap_or(&Post::default())),
    }
}

fn default_view3d() -> PonkoView3D {
    view3d_to_c(&View3D::new(ponko::render::SceneSettings::default().camera))
}

/// `base` changed by `v`, checked.
fn view3d_from_c(base: &View3D, v: &PonkoView3D) -> Result<View3D, Failure> {
    let mut camera = camera_from_c(base.camera, &v.camera)?;
    camera.rect = viewport_from_c(v.rect)?;
    camera.layers = RenderLayers(v.layers);
    let post = if v.has_post {
        let mut post = base.post.unwrap_or_default();
        post_from_c(&mut post, &v.post)?;
        Some(post)
    } else {
        None
    };
    Ok(View3D {
        camera,
        clear: v.has_clear.then(|| to_c(v.clear)),
        post,
    })
}

/// Fills the fields of `view`'s version with the defaults (the default
/// camera, the whole window, every layer, the game's clear and post
/// settings). `struct_size` and `version` must be set first.
/// # Safety
/// `view` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_view3d_defaults(view: *mut PonkoView3D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(view, &default_view3d()) })
}

/// Reads the main 3D camera (`ctx.scene3d.camera`) as a view: the camera,
/// its rectangle and layers (`has_clear` and `has_post` false).
/// # Safety
/// `ctx` null or live; `out_view` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera3d_view(ctx: *mut PonkoCtx, out_view: *mut PonkoView3D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| Ok(view3d_to_c(&View3D::new(c.scene3d.camera))))?;
        write_desc(out_view, &v)
    })
}

/// Sets the main 3D camera from a view: the camera, its rectangle and
/// layers (the clear and post are ignored: the main camera's are the
/// game's and `ponko_ctx_set_post`'s). Checked as `ponko_ctx_set_camera3d`.
/// # Safety
/// `ctx` null or live; `view` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_camera3d_view(ctx: *mut PonkoCtx, view: *const PonkoView3D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = read_desc(view, default_view3d())?;
        ctx_access(ctx, |c| {
            let mut v = v;
            v.has_post = false;
            c.scene3d.camera = view3d_from_c(&View3D::new(c.scene3d.camera), &v)?.camera;
            Ok(())
        })
    })
}

/// How many 3D views there are (`ctx.scene3d.views`).
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_view3d_count(ctx: *mut PonkoCtx, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let n = ctx_access(ctx, |c| Ok(c.scene3d.views.len()))?;
        out(out_count, n)
    })
}

/// Reads 3D view `index`. `PONKO_NOT_FOUND` past the last one.
/// # Safety
/// `ctx` null or live; `out_view` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_view3d(ctx: *mut PonkoCtx, index: usize, out_view: *mut PonkoView3D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| {
            let views = &c.scene3d.views;
            let view = views.get(index).ok_or_else(no_view)?;
            Ok(view3d_to_c(view))
        })?;
        write_desc(out_view, &v)
    })
}

/// Sets 3D view `index` from `view`; `index` equal to the count adds one
/// at the end (past it: `PONKO_NOT_FOUND`). Views draw after the main
/// camera, in order. Checked as `ponko_ctx_set_camera3d` and, with
/// `has_post`, `ponko_ctx_set_post`.
/// # Safety
/// `ctx` null or live; `view` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_view3d(
    ctx: *mut PonkoCtx,
    index: usize,
    view: *const PonkoView3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = read_desc(view, default_view3d())?;
        ctx_access(ctx, |c| {
            let views = &mut c.scene3d.views;
            let count = views.len();
            let base = views.get(index).copied().unwrap_or(View3D::new(c.scene3d.camera));
            let new = view3d_from_c(&base, &v)?;
            match index.cmp(&count) {
                std::cmp::Ordering::Less => views[index] = new,
                std::cmp::Ordering::Equal => views.push(new),
                std::cmp::Ordering::Greater => return Err(no_view()),
            }
            Ok(())
        })
    })
}

/// Removes 3D view `index`; the ones after it move down one. Its frame
/// state (targets, history) is freed with the next frame.
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_view3d(ctx: *mut PonkoCtx, index: usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let views = &mut c.scene3d.views;
            if index >= views.len() {
                return Err(no_view());
            }
            views.remove(index);
            Ok(())
        })
    })
}

/// Which 3D camera draws the window at `screen` (Rust
/// `ctx.scene3d.camera_at`): the index of the last view whose rectangle
/// holds it, or -1 for the main camera (also outside every rectangle).
/// # Safety
/// `ctx` null or live; `out_view` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera3d_at(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    out_view: *mut i32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let at = ctx_access(ctx, |c| {
            let camera = c.scene3d.camera_at(to_c(screen));
            Ok(c.scene3d
                .views
                .iter()
                .position(|v| std::ptr::eq(&v.camera, camera))
                .map_or(-1, |i| i as i32))
        })?;
        out(out_view, at)
    })
}

/// Where the ray under window point `screen`, through the 3D camera that
/// draws there, meets the level plane `y = height` (Rust
/// `ctx.scene3d.camera_at(screen).screen_to_ground(screen, height)`). When
/// it points away from it: `*out_found` false and `*out_point` untouched.
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_screen_to_ground_at(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    height: f32,
    out_found: *mut bool,
    out_point: *mut crate::PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_found.is_null() || out_point.is_null() {
            return Err(invalid("output is null"));
        }
        let p = ctx_access(ctx, |c| {
            let screen = to_c(screen);
            Ok(c.scene3d.camera_at(screen).screen_to_ground(screen, height))
        })?;
        out_found.write(p.is_some());
        if let Some(p) = p {
            out_point.write(to_c(p));
        }
        Ok(())
    })
}
