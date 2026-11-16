//! Stencil state on materials: [`Stencil`] and its parts.

/// How a [`StencilFace`] compares the material's reference with the value
/// already in the stencil buffer (both masked by
/// [`Stencil::read_mask`]): the draw passes where `reference <op> stored`
/// holds.
///
/// ```
/// # use ponko::render::StencilCompare;
/// assert_eq!(StencilCompare::default(), StencilCompare::Always);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum StencilCompare {
    /// Never passes.
    Never = 0,
    /// Passes where the reference is less than the stored value.
    Less = 1,
    /// Passes where they are equal.
    Equal = 2,
    /// Passes where the reference is less than or equal to the stored value.
    LessEqual = 3,
    /// Passes where the reference is greater than the stored value.
    Greater = 4,
    /// Passes where they differ.
    NotEqual = 5,
    /// Passes where the reference is greater than or equal to the stored value.
    GreaterEqual = 6,
    /// Always passes (the default): no test.
    #[default]
    Always = 7,
}

/// What a draw does to the stored stencil value of a pixel (only the bits
/// of [`Stencil::write_mask`] change).
///
/// ```
/// # use ponko::render::StencilOp;
/// assert_eq!(StencilOp::default(), StencilOp::Keep);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum StencilOp {
    /// Leaves it (the default).
    #[default]
    Keep = 0,
    /// Sets it to 0.
    Zero = 1,
    /// Sets it to the material's [`Stencil::reference`].
    Replace = 2,
    /// Flips its bits.
    Invert = 3,
    /// Adds 1, stopping at 255.
    IncrementClamp = 4,
    /// Takes 1 away, stopping at 0.
    DecrementClamp = 5,
    /// Adds 1, from 255 back to 0.
    IncrementWrap = 6,
    /// Takes 1 away, from 0 back to 255.
    DecrementWrap = 7,
}

/// The stencil test and its operations for one side of the triangles.
///
/// ```
/// # use ponko::render::{StencilCompare, StencilFace, StencilOp};
/// let face = StencilFace::default();
/// assert_eq!((face.compare, face.pass), (StencilCompare::Always, StencilOp::Keep));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)] // shared with C/C++ (`PonkoStencilFace`)
pub struct StencilFace {
    /// The test. Default [`StencilCompare::Always`].
    pub compare: StencilCompare,
    /// What a pixel that fails the stencil test does to the stored value.
    pub fail: StencilOp,
    /// What a pixel that passes the stencil test but fails the depth test does.
    pub depth_fail: StencilOp,
    /// What a pixel that passes both does.
    pub pass: StencilOp,
}

impl StencilFace {
    /// A test that changes nothing, whatever the outcome.
    #[must_use]
    pub const fn test(compare: StencilCompare) -> Self {
        Self {
            compare,
            fail: StencilOp::Keep,
            depth_fail: StencilOp::Keep,
            pass: StencilOp::Keep,
        }
    }
}

/// A material's stencil state ([`MaterialDesc::stencil`](super::MaterialDesc::stencil)):
/// an 8-bit value per pixel that materials test before they draw and
/// change as they draw. For masks, portals, outlines and see-through
/// surfaces drawn once where they overlap.
///
/// The test and the operations happen where the material draws its color:
/// the opaque or the transparent pass, of the main camera and of
/// [`TargetCamera`](super::TargetCamera)s. Shadow maps and global
/// illumination's probe views ignore it. The stencil starts at 0
/// every frame.
///
/// Drawing order matters to a stencil, so these materials keep one:
///
/// - Solid ([`Blend::Opaque`](super::Blend::Opaque)) stencil materials skip
///   the depth prepass and draw after every solid material without a
///   stencil, lowest [`order`](Self::order) first, writing their own depth.
///   A pixel that fails their test shows what's behind it. As they are not
///   in the prepass's depth, screen-space ambient occlusion, reflections
///   and motion vectors don't see them.
/// - [`Blend::Cutout`](super::Blend::Cutout) ones keep their prepass (it
///   cuts the holes); a pixel failing their test shows the background.
/// - [`Blend::Transparent`](super::Blend::Transparent) ones draw far to near
///   with the other transparent surfaces; `order` doesn't apply. While a
///   transparent material that reads the scene behind it is on screen, the
///   transparent pass reads the stencil without writing it.
///
/// Scenes without stencil materials draw exactly as before: the depth
/// target gets its stencil bits only once a material asks for them.
///
/// ```
/// # use ponko::prelude::*;
/// # use ponko::render::{MaterialDesc, Blend, Stencil};
/// // Overlapping glass strips that blend only once where they cross.
/// let glass = MaterialDesc::color(Color::rgba(0.2, 0.6, 1.0, 0.5))
///     .with_blend(Blend::Transparent)
///     .with_stencil(Stencil::once(1));
/// // A solid outline drawn only where the object it surrounds isn't.
/// let body = MaterialDesc::default().with_stencil(Stencil::mark(1));
/// let outline = MaterialDesc::color(Color::rgb(1.0, 0.8, 0.0)).with_stencil(Stencil::outside(1).with_order(1));
/// assert_eq!(outline.stencil.unwrap().order, 1);
/// # let _ = (glass, body);
/// ```
///
/// New fields may be added in any release: build one with its
/// constructors or [`Default`] and the `with_*` methods.
///
/// # Defaults and cost
///
/// [`Default`] compares against a reference of 0 with all bits read and
/// written, at order 0, and each face's test and operations are those of
/// [`StencilFace::default`]. A material without a stencil costs nothing
/// for it. A solid material with one gives up the depth prepass and draws
/// after the other solids, so it is not in the depth that ambient occlusion,
/// reflections and motion vectors read, and the depth target gets its stencil
/// bits only once a material asks for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Stencil {
    /// The front faces' test and operations.
    pub front: StencilFace,
    /// The back faces' (seen only on [double-sided](super::MaterialDesc::double_sided)
    /// materials).
    pub back: StencilFace,
    /// The value the test compares with, and [`StencilOp::Replace`] writes.
    /// Default 0.
    pub reference: u8,
    /// The stored bits the test reads. Default all (`0xFF`).
    pub read_mask: u8,
    /// The stored bits the operations change. Default all (`0xFF`).
    pub write_mask: u8,
    /// Where solid stencil materials draw among each other: lower first.
    /// Default 0.
    pub order: i32,
}

impl Default for Stencil {
    fn default() -> Self {
        Self {
            front: StencilFace::default(),
            back: StencilFace::default(),
            reference: 0,
            read_mask: 0xFF,
            write_mask: 0xFF,
            order: 0,
        }
    }
}

impl Stencil {
    /// The same test and operations on both faces.
    #[must_use]
    pub const fn both(face: StencilFace, reference: u8) -> Self {
        Self {
            front: face,
            back: face,
            reference,
            read_mask: 0xFF,
            write_mask: 0xFF,
            order: 0,
        }
    }

    /// Draws everywhere and writes `reference` where it drew (passing the
    /// depth test): a mask for others to test.
    ///
    /// ```
    /// # use ponko::render::{Stencil, StencilCompare, StencilOp};
    /// let s = Stencil::mark(3);
    /// assert_eq!((s.front.compare, s.front.pass, s.reference), (StencilCompare::Always, StencilOp::Replace, 3));
    /// ```
    #[must_use]
    pub const fn mark(reference: u8) -> Self {
        Self::both(
            StencilFace {
                pass: StencilOp::Replace,
                ..StencilFace::test(StencilCompare::Always)
            },
            reference,
        )
    }

    /// Draws only where the stencil holds `reference` (inside a mask, a portal).
    #[must_use]
    pub const fn inside(reference: u8) -> Self {
        Self::both(StencilFace::test(StencilCompare::Equal), reference)
    }

    /// Draws only where the stencil doesn't hold `reference` (an outline
    /// around a marked object).
    #[must_use]
    pub const fn outside(reference: u8) -> Self {
        Self::both(StencilFace::test(StencilCompare::NotEqual), reference)
    }

    /// Draws each pixel at most once: only where the stencil doesn't hold
    /// `reference` yet, writing it there. Overlapping transparent surfaces
    /// with it blend once instead of twice.
    #[must_use]
    pub const fn once(reference: u8) -> Self {
        Self::both(
            StencilFace {
                pass: StencilOp::Replace,
                ..StencilFace::test(StencilCompare::NotEqual)
            },
            reference,
        )
    }

    /// Sets [`front`](Self::front).
    #[must_use]
    pub const fn with_front(mut self, front: StencilFace) -> Self {
        self.front = front;
        self
    }

    /// Sets [`back`](Self::back).
    #[must_use]
    pub const fn with_back(mut self, back: StencilFace) -> Self {
        self.back = back;
        self
    }

    /// Sets [`reference`](Self::reference).
    #[must_use]
    pub const fn with_reference(mut self, reference: u8) -> Self {
        self.reference = reference;
        self
    }

    /// Sets [`read_mask`](Self::read_mask).
    #[must_use]
    pub const fn with_read_mask(mut self, read_mask: u8) -> Self {
        self.read_mask = read_mask;
        self
    }

    /// Sets [`write_mask`](Self::write_mask).
    #[must_use]
    pub const fn with_write_mask(mut self, write_mask: u8) -> Self {
        self.write_mask = write_mask;
        self
    }

    /// Sets [`order`](Self::order).
    #[must_use]
    pub const fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Whether any operation can change the stored value.
    #[must_use]
    pub fn writes(&self) -> bool {
        let face_writes = |f: &StencilFace| {
            [f.fail, f.depth_fail, f.pass]
                .into_iter()
                .any(|op| op != StencilOp::Keep)
        };
        self.write_mask != 0 && (face_writes(&self.front) || face_writes(&self.back))
    }
}
