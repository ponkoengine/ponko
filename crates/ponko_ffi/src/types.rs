//! Plain C structs shared with the engine. Most have the exact layout of the
//! Rust type (checked at compile time), so values pass through without copies.
use ponko::d2::Sprite;
use ponko::d2::Transform as Transform2D;
use ponko::prelude::{Color, DrawParams, Rect, Texture, Vec2};
use ponko::spatial::GridEntry;
use ponko_ecs::Entity;

/// A 2D vector.
/// ```
/// let v = ponko_ffi::PonkoVec2 { x: 1.0, y: 2.0 };
/// assert_eq!(v.x + v.y, 3.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoVec2 {
    /// Horizontal, right is positive.
    pub x: f32,
    /// Vertical, down is positive.
    pub y: f32,
}

/// RGBA color, each channel 0..1 (same layout as Rust `Color`).
/// ```
/// let white = ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
/// assert_eq!(white.a, 1.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoColor {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha (1 = opaque).
    pub a: f32,
}

/// A texture handle from `ponko_ctx_load_texture` (same layout as Rust `Texture`).
/// Never make one up: only use values the engine gave you.
/// ```
/// assert_eq!(std::mem::size_of::<ponko_ffi::PonkoTexture>(), 12);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoTexture {
    /// Engine-internal id.
    pub id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Position, rotation (radians, clockwise) and scale; the built-in
/// `Transform2D` component, same layout as in Rust.
/// ```
/// let t = ponko_ffi::PonkoTransform2D::default();
/// assert_eq!(t.scale.x, 1.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoTransform2D {
    /// Where the entity is.
    pub position: PonkoVec2,
    /// Radians, clockwise on screen.
    pub rotation: f32,
    /// Size multiplier; negative flips.
    pub scale: PonkoVec2,
}

impl Default for PonkoTransform2D {
    fn default() -> Self {
        to_c(Transform2D::default())
    }
}

/// An entity as stored in query chunks. `ponko_entity_bits` (or
/// `(uint64_t)generation << 32 | index`) gives the id other calls take.
/// ```
/// let e = ponko_ffi::PonkoEntity { index: 3, generation: 1 };
/// assert_eq!(ponko_ffi::ponko_entity_bits(e), (1u64 << 32) | 3);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoEntity {
    /// Slot number.
    pub index: u32,
    /// Bumped when the slot is reused, so old ids stay dead.
    pub generation: u32,
}

/// The id of `entity` that other calls take.
/// ```
/// let e = ponko_ffi::PonkoEntity { index: 7, generation: 0 };
/// assert_eq!(ponko_ffi::ponko_entity_bits(e), 7);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_entity_bits(entity: PonkoEntity) -> u64 {
    (entity.generation as u64) << 32 | entity.index as u64
}

/// An axis-aligned rectangle from `min` (top-left) to `max`.
/// ```
/// let r = ponko_ffi::PonkoRect { min: Default::default(), max: ponko_ffi::PonkoVec2 { x: 16.0, y: 16.0 } };
/// assert_eq!(r.max.x, 16.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoRect {
    /// Top-left corner.
    pub min: PonkoVec2,
    /// Bottom-right corner.
    pub max: PonkoVec2,
}

/// The built-in `Sprite` component as a C value. Get and set it with the
/// sprite component id like any component; it can't be a query column,
/// because the engine stores it in a Rust layout.
/// ```
/// let s = ponko_ffi::PonkoSprite::default();
/// assert_eq!(s.pivot.x, 0.5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoSprite {
    /// What to draw.
    pub texture: PonkoTexture,
    /// Tint. Default white.
    pub color: PonkoColor,
    /// Draw order, higher on top. Default 0.
    pub z: f32,
    /// Point placed at the position: (0.5, 0.5) = center (default).
    pub pivot: PonkoVec2,
    /// Show only `region` of the texture (texture pixels).
    pub has_region: bool,
    /// Used when `has_region` is true.
    pub region: PonkoRect,
}

impl Default for PonkoSprite {
    fn default() -> Self {
        sprite_to_c(&Sprite::new(texture_from_c(PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        })))
    }
}

/// Options for `ponko_ctx_draw_ex`. Start from `ponko_draw_params_default()`.
/// ```
/// let p = ponko_ffi::ponko_draw_params_default();
/// assert_eq!(p.scale.x, 1.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoDrawParams {
    /// Size multiplier; negative flips. Default (1, 1).
    pub scale: PonkoVec2,
    /// Radians, clockwise. Default 0.
    pub rotation: f32,
    /// Tint. Default white.
    pub color: PonkoColor,
    /// Draw order, higher on top. Default 0.
    pub z: f32,
    /// Point placed at the position. Default (0.5, 0.5).
    pub pivot: PonkoVec2,
    /// Window pixels, ignoring the camera (UI). Default false.
    pub screen: bool,
    /// Draw only `region` of the texture.
    pub has_region: bool,
    /// Used when `has_region` is true.
    pub region: PonkoRect,
}

/// Coordinate space for immediate sprite draws; preserves the existing descriptor layout.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoDrawSpace {
    /// World units through the camera.
    PONKO_DRAW_SPACE_WORLD = 0,
    /// Window pixels over the world, ignoring the camera.
    PONKO_DRAW_SPACE_SCREEN = 1,
}

/// Returns draw options with a named coordinate space.
/// ```
/// use ponko_ffi::*;
/// let p = ponko_draw_params_with_space(ponko_draw_params_default(), PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN);
/// assert!(p.screen);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_draw_params_with_space(mut params: PonkoDrawParams, space: PonkoDrawSpace) -> PonkoDrawParams {
    params.screen = space == PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN;
    params
}

/// Default draw options: no scale, rotation or tint, centered.
/// ```
/// assert!(!ponko_ffi::ponko_draw_params_default().screen);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_draw_params_default() -> PonkoDrawParams {
    let d = DrawParams::default();
    let (has_region, region) = rect_to_c(d.region);
    PonkoDrawParams {
        scale: to_c(d.scale),
        rotation: d.rotation,
        color: to_c(d.color),
        z: d.z,
        pivot: to_c(d.pivot),
        screen: d.screen,
        has_region,
        region,
    }
}

/// A spatial-grid hit: an entity and its position.
/// ```
/// assert_eq!(std::mem::size_of::<ponko_ffi::PonkoGridHit>(), 16);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoGridHit {
    /// Who.
    pub entity: PonkoEntity,
    /// Where it was when the grid was built.
    pub position: PonkoVec2,
}

/// An item of a `PonkoGrid`: entity, position and a number of your own
/// (an index into your own arrays, for example).
/// ```
/// assert_eq!(std::mem::size_of::<ponko_ffi::PonkoGridItem>(), 20);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoGridItem {
    /// Who.
    pub entity: PonkoEntity,
    /// Where.
    pub position: PonkoVec2,
    /// Yours.
    pub data: u32,
}

// ---- layout checks and conversions -------------------------------------------

/// `A` and `B` have the same size, alignment and field offsets.
/// # Safety
/// Only for type pairs whose size, alignment and field offsets match (checked by `same_layout!`).
pub(crate) unsafe trait SameLayout<B>: Copy {}
macro_rules! same_layout {
    ($c:ty, $rust:ty $(, $cf:ident = $rf:ident)*) => {
        unsafe impl SameLayout<$rust> for $c {}
        unsafe impl SameLayout<$c> for $rust {}
        const _: () = {
            assert!(size_of::<$c>() == size_of::<$rust>());
            assert!(align_of::<$c>() == align_of::<$rust>());
            $(assert!(std::mem::offset_of!($c, $cf) == std::mem::offset_of!($rust, $rf));)*
        };
    };
}
pub(crate) use same_layout;
same_layout!(PonkoVec2, Vec2, x = x, y = y);
same_layout!(PonkoColor, Color, r = r, g = g, b = b, a = a);
same_layout!(
    PonkoTransform2D,
    Transform2D,
    position = position,
    rotation = rotation,
    scale = scale
);
same_layout!(PonkoRect, Rect, min = min, max = max);
same_layout!(PonkoGridHit, GridEntry<()>, entity = entity, position = position);
same_layout!(
    PonkoGridItem,
    GridEntry<u32>,
    entity = entity,
    position = position,
    data = data
);
// Entity is repr(C) { index, generation }; its fields are private to ponko_ecs.
same_layout!(PonkoEntity, Entity);
const _: () = assert!(size_of::<PonkoTexture>() == size_of::<Texture>());

/// Converts between two types with the same layout.
#[inline(always)]
pub(crate) fn to_c<A: SameLayout<B>, B>(a: A) -> B {
    // SAFETY: SameLayout is only implemented for checked pairs of plain f32/u32 structs.
    unsafe { std::mem::transmute_copy(&a) }
}

pub(crate) fn texture_to_c(t: Texture) -> PonkoTexture {
    // SAFETY: both are repr(C) { u32 id, u32 width, u32 height } (sizes asserted above).
    unsafe { std::mem::transmute(t) }
}
pub(crate) fn texture_from_c(t: PonkoTexture) -> Texture {
    // SAFETY: as above. Ids the engine doesn't know draw nothing or the wrong texture; they are not UB.
    unsafe { std::mem::transmute(t) }
}

fn rect_to_c(r: Option<Rect>) -> (bool, PonkoRect) {
    match r {
        Some(r) => (true, to_c(r)),
        None => (false, PonkoRect::default()),
    }
}
fn rect_from_c(has: bool, r: PonkoRect) -> Option<Rect> {
    has.then(|| to_c(r))
}

pub(crate) fn sprite_to_c(s: &Sprite) -> PonkoSprite {
    let (has_region, region) = rect_to_c(s.region);
    PonkoSprite {
        texture: texture_to_c(s.texture),
        color: to_c(s.color),
        z: s.z,
        pivot: to_c(s.pivot),
        has_region,
        region,
    }
}
pub(crate) fn sprite_from_c(s: &PonkoSprite) -> Sprite {
    let mut sprite = Sprite::new(texture_from_c(s.texture));
    sprite.color = to_c(s.color);
    sprite.z = s.z;
    sprite.pivot = to_c(s.pivot);
    sprite.region = rect_from_c(s.has_region, s.region);
    sprite
}
impl crate::CMirror for Sprite {
    type C = PonkoSprite;
    fn from_c(c: &PonkoSprite) -> Result<Self, crate::Failure> {
        Ok(sprite_from_c(c))
    }
    fn to_c(&self) -> PonkoSprite {
        sprite_to_c(self)
    }
}
pub(crate) fn draw_params_from_c(p: &PonkoDrawParams) -> DrawParams {
    let mut params = DrawParams::default()
        .with_scale(to_c(p.scale))
        .with_rotation(p.rotation)
        .with_color(to_c(p.color))
        .with_z(p.z)
        .with_pivot(to_c(p.pivot))
        .with_space(if p.screen {
            ponko::prelude::DrawSpace::Screen
        } else {
            ponko::prelude::DrawSpace::World
        });
    params.region = rect_from_c(p.has_region, p.region);
    params
}
