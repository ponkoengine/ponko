//! Where 2D coordinates live: the world or the screen.
//!
//! One type for sprite draws ([`DrawParams::with_space`](crate::texture::DrawParams::with_space))
//! and debug shapes ([`DebugShape::space`](crate::debug::DebugShape::space)),
//! public at [`crate::debug::DrawSpace`] and in the prelude.

/// Where 2D coordinates are: in the world (units of the 2D camera) or on the
/// screen (window pixels from the top-left corner, ignoring the camera).
///
/// Sprite draws (`DrawParams::with_space`) and debug shapes
/// (`DebugShape::space`) take the same type. 3D debug shapes are always in
/// the 3D world.
///
/// ```
/// use ponko::prelude::*;
/// assert_eq!(DrawSpace::default(), DrawSpace::World);
/// let hud = DrawParams::default().with_space(DrawSpace::Screen);
/// assert_eq!(hud.with_space(DrawSpace::World), DrawParams::default());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrawSpace {
    /// World units, moved and zoomed by the 2D camera. The default.
    #[default]
    World,
    /// Window pixels from the top-left corner, ignoring the camera and
    /// drawn over the world.
    Screen,
}
