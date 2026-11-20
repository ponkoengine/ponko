//! The shared, immutable description of a 2D skeleton: bones, slots,
//! attachments, skins, clips, IK constraints and the optional state machine.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use glam::{Affine2, Vec2};

use super::{Clip, SkeletonGraph};
use crate::camera::Rect;
use crate::color::Color;
use crate::texture::Texture;

/// A bone's transform relative to its parent: translation, then rotation
/// (radians, clockwise on screen like [`Transform2D`](crate::d2::Transform)),
/// then scale, with an optional shear. A bone points along its local +x axis.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::BonePose;
/// let elbow = BonePose::at(vec2(20.0, 0.0)).with_rotation(0.5);
/// assert_eq!(elbow.scale, Vec2::ONE);
/// assert_eq!(elbow.shear, Vec2::ZERO);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct BonePose {
    /// Offset from the parent's origin, in the parent's units.
    pub translation: Vec2,
    /// Radians, clockwise on screen.
    pub rotation: f32,
    /// Size multiplier along the bone's own axes; negative mirrors.
    pub scale: Vec2,
    /// Extra turn of each of the bone's axes, radians clockwise on screen:
    /// `x` turns its +x axis and `y` its +y axis (as Spine's shear does,
    /// mirrored to y-down). Zero: no shear, the axes stay square.
    #[serde(default)]
    pub shear: Vec2,
}

impl Default for BonePose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl BonePose {
    /// No offset, no rotation, scale 1.
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        rotation: 0.0,
        scale: Vec2::ONE,
        shear: Vec2::ZERO,
    };

    /// A pose from its parts: `translation`, `rotation` (radians, clockwise
    /// on screen) and `scale`, without shear.
    #[must_use]
    pub fn new(translation: Vec2, rotation: f32, scale: Vec2) -> Self {
        Self {
            translation,
            rotation,
            scale,
            shear: Vec2::ZERO,
        }
    }

    /// A pose at `translation` with no rotation and scale 1.
    #[must_use]
    pub fn at(translation: Vec2) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// The same pose rotated to `rotation` radians.
    #[must_use]
    pub fn with_rotation(self, rotation: f32) -> Self {
        Self { rotation, ..self }
    }

    /// The same pose with scale `scale`.
    #[must_use]
    pub fn with_scale(self, scale: Vec2) -> Self {
        Self { scale, ..self }
    }

    /// The same pose with shear `shear` (radians; see [`shear`](Self::shear)).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::BonePose;
    /// // Its +y axis turns an eighth of a turn clockwise; +x stays.
    /// let lean = BonePose::IDENTITY.with_shear(vec2(0.0, std::f32::consts::FRAC_PI_4));
    /// assert_eq!(lean.shear.y, std::f32::consts::FRAC_PI_4);
    /// ```
    #[must_use]
    pub fn with_shear(self, shear: Vec2) -> Self {
        Self { shear, ..self }
    }

    /// The pose's matrix: translate · rotate · scale, its +x axis turned by
    /// `rotation + shear.x` and its +y axis by `rotation + shear.y`.
    pub(crate) fn matrix(&self) -> Affine2 {
        if self.shear == Vec2::ZERO {
            return Affine2::from_scale_angle_translation(self.scale, self.rotation, self.translation);
        }
        let (sx, cx) = (self.rotation + self.shear.x).sin_cos();
        let (sy, cy) = (self.rotation + self.shear.y).sin_cos();
        Affine2::from_cols(
            Vec2::new(cx, sx) * self.scale.x,
            Vec2::new(-sy, cy) * self.scale.y,
            self.translation,
        )
    }

    /// The pose whose [`matrix`](Self::matrix) is `m`, without shear on
    /// its x axis (a turn of x is a rotation): exact for any invertible
    /// matrix, so `pose_of(p.matrix())` gives `p.matrix()` back.
    pub(crate) fn of_matrix(m: Affine2) -> Self {
        let (x, y) = (m.matrix2.x_axis, m.matrix2.y_axis);
        let sx = x.length();
        if sx <= 1e-6 {
            // No x axis to measure the turn from: turn by the y axis alone.
            return Self {
                translation: m.translation,
                rotation: (-y.x).atan2(y.y),
                scale: Vec2::new(0.0, y.length()),
                shear: Vec2::ZERO,
            };
        }
        let rotation = x.y.atan2(x.x);
        let det = x.x * y.y - y.x * x.y;
        let sy = if det < 0.0 { -y.length() } else { y.length() };
        // The y axis turned back by the rotation: sy (-sin shear, cos shear).
        let back = glam::Mat2::from_angle(-rotation) * y;
        let shear = if sy == 0.0 {
            0.0
        } else {
            (-back.x / sy).atan2(back.y / sy)
        };
        Self {
            translation: m.translation,
            rotation,
            scale: Vec2::new(sx, sy),
            shear: Vec2::new(0.0, shear),
        }
    }

    /// The pose `t` of the way to `other`: translation, scale and shear
    /// linearly, rotation along the shorter way round.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::BonePose;
    /// let a = BonePose::at(vec2(0.0, 0.0)).with_rotation(3.0);
    /// let b = BonePose::at(vec2(10.0, 0.0)).with_rotation(-3.0);
    /// let mid = a.lerp(b, 0.5);
    /// assert_eq!(mid.translation, vec2(5.0, 0.0));
    /// // 3 and -3 radians are 0.28 apart across pi, not 6 apart across 0.
    /// assert!((mid.rotation.abs() - std::f32::consts::PI).abs() < 1e-3);
    /// ```
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation + angle_delta(self.rotation, other.rotation) * t,
            scale: self.scale.lerp(other.scale, t),
            shear: self.shear.lerp(other.shear, t),
        }
    }
}

/// `b - a` wrapped into `-pi..pi`.
pub(crate) fn angle_delta(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (b - a + PI).rem_euclid(TAU) - PI
}

/// One bone of a [`SkeletonData`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Bone {
    /// Its name, unique in the skeleton.
    pub name: Arc<str>,
    /// Its parent, which always comes earlier in the list; `None` for a root.
    pub parent: Option<usize>,
    /// Where it is when nothing animates it, relative to the parent.
    pub rest: BonePose,
    /// How long it is along its +x axis (two-bone IK reaches with the
    /// second bone's length). 0 for a bone that is only a pivot.
    pub length: f32,
}

/// A place that shows one attachment at a time, on a bone. Slots are
/// listed in their rest draw order: later slots draw on top.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Slot {
    /// Its name, unique in the skeleton.
    pub name: Arc<str>,
    /// The bone it follows.
    pub bone: usize,
    /// The attachment it shows at rest, by name; `None` shows nothing.
    pub attachment: Option<Arc<str>>,
    /// Tint for everything it shows.
    pub color: Color,
}

/// A rigid image on a bone: one sprite that moves with it.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::skeleton2d::SpritePart;
/// # fn f(ctx: &mut Ctx) {
/// let arm = ctx.gfx.load_texture("arm.png");
/// let part = SpritePart::new(arm).with_pivot(vec2(0.0, 0.5)); // hinged at its left edge
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SpritePart {
    /// The image file it was loaded from (`""` for one made in code).
    pub image: Arc<str>,
    /// The image.
    pub texture: Texture,
    /// The part of the image to show, in texture pixels; `None`: all of it.
    pub region: Option<Rect>,
    /// The region holds the image turned a quarter counter-clockwise, as
    /// texture packers store some regions (Spine's `rotate: 90`): it is
    /// `size.y` wide and `size.x` tall in the texture and draws upright.
    pub rotated: bool,
    /// Where it sits on its bone.
    pub offset: BonePose,
    /// Its size in skeleton units (the region's or image's size by default).
    pub size: Vec2,
    /// The point of the image placed at `offset`: `(0.5, 0.5)` is its center.
    pub pivot: Vec2,
    /// Tint, multiplied with the slot's.
    pub color: Color,
}

impl SpritePart {
    /// All of `texture`, centered on its bone, at its own size.
    #[must_use]
    pub fn new(texture: Texture) -> Self {
        Self {
            image: "".into(),
            texture,
            region: None,
            rotated: false,
            offset: BonePose::IDENTITY,
            size: texture.size(),
            pivot: Vec2::splat(0.5),
            color: Color::WHITE,
        }
    }

    /// The same part showing `region` of its texture (texture pixels), at
    /// the region's size.
    #[must_use]
    pub fn with_region(self, region: Rect) -> Self {
        Self {
            region: Some(region),
            size: region.size(),
            ..self
        }
    }

    /// The same part showing `region` of its texture (texture pixels),
    /// where a packer stored the image a quarter turn counter-clockwise:
    /// `region` is the rectangle as packed, and the part is drawn upright
    /// at the image's size (the region's height by its width).
    #[must_use]
    pub fn with_rotated_region(self, region: Rect) -> Self {
        Self {
            region: Some(region),
            rotated: true,
            size: Vec2::new(region.size().y, region.size().x),
            ..self
        }
    }

    /// The same part placed at `offset` on its bone.
    #[must_use]
    pub fn with_offset(self, offset: BonePose) -> Self {
        Self { offset, ..self }
    }

    /// The same part with pivot `pivot` (0..1 over the image).
    #[must_use]
    pub fn with_pivot(self, pivot: Vec2) -> Self {
        Self { pivot, ..self }
    }

    /// The same part drawn `size` skeleton units big.
    #[must_use]
    pub fn with_size(self, size: Vec2) -> Self {
        Self { size, ..self }
    }
}

/// One vertex of a [`SkinnedMesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MeshVertex {
    /// Where it is when the skeleton is at rest, in skeleton units.
    pub position: Vec2,
    /// Where it is in the image (0..1 over the whole texture).
    pub uv: Vec2,
    /// Up to four bones that move it.
    pub bones: [u16; 4],
    /// How much each of `bones` moves it; they add up to 1 (unused ones are 0).
    pub weights: [f32; 4],
}

impl MeshVertex {
    /// A vertex at `position` (skeleton units, at rest), showing `uv`,
    /// moved by `influences` (bone, weight). Weights are normalized to add
    /// up to 1; more than four keep the four strongest.
    ///
    /// # Panics
    /// With no influence, or weights that don't add up to more than 0.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::MeshVertex;
    /// let v = MeshVertex::new(vec2(10.0, 0.0), vec2(0.5, 0.5), &[(1, 3.0), (2, 1.0)]);
    /// assert_eq!(v.weights, [0.75, 0.25, 0.0, 0.0]);
    /// ```
    #[must_use]
    pub fn new(position: Vec2, uv: Vec2, influences: &[(usize, f32)]) -> Self {
        let mut strongest: Vec<(usize, f32)> = influences.iter().copied().filter(|(_, w)| *w > 0.0).collect();
        strongest.sort_by(|a, b| b.1.total_cmp(&a.1));
        strongest.truncate(4);
        let total: f32 = strongest.iter().map(|(_, w)| w).sum();
        assert!(
            total > 0.0 && total.is_finite(),
            "ponko: a mesh vertex needs at least one bone with a positive weight"
        );
        let mut bones = [0; 4];
        let mut weights = [0.0; 4];
        for (i, (bone, w)) in strongest.into_iter().enumerate() {
            bones[i] = bone as u16;
            weights[i] = w / total;
        }
        Self {
            position,
            uv,
            bones,
            weights,
        }
    }
}

/// A 2D mesh that bends with its bones: each vertex follows up to four
/// bones by weight (linear blend skinning).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SkinnedMesh {
    /// The image file it was loaded from (`""` for one made in code).
    pub image: Arc<str>,
    /// The image.
    pub texture: Texture,
    /// Its vertices.
    pub vertices: Vec<MeshVertex>,
    /// Every three indices into `vertices` are one triangle.
    pub triangles: Vec<u16>,
    /// Tint, multiplied with the slot's.
    pub color: Color,
}

impl SkinnedMesh {
    /// A mesh showing `texture` with `vertices`, drawn as `triangles`.
    ///
    /// # Panics
    /// If `triangles` isn't a whole number of triangles or names a vertex
    /// that isn't there.
    #[must_use]
    pub fn new(texture: Texture, vertices: Vec<MeshVertex>, triangles: Vec<u16>) -> Self {
        assert!(
            triangles.len().is_multiple_of(3) && triangles.iter().all(|&i| usize::from(i) < vertices.len()),
            "ponko: a skinned mesh's triangles are three indices each, into its vertices"
        );
        Self {
            image: "".into(),
            texture,
            vertices,
            triangles,
            color: Color::WHITE,
        }
    }
}

/// A polygon that clips what the slots after it draw: from its own slot's
/// place in the draw order up to and including slot `end`, only the parts
/// inside the polygon are drawn (Spine's clipping attachment). Its
/// vertices follow their bones as a [`SkinnedMesh`]'s do; their `uv` is
/// unused. Any simple polygon, convex or not, in either winding.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{BonePose, ClippingMask, MeshVertex, SkeletonData};
/// let mut data = SkeletonData::new();
/// let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
/// let window = data.add_slot("window", root);
/// let corner = |x, y| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(root, 1.0)]);
/// let square = vec![corner(0.0, 0.0), corner(10.0, 0.0), corner(10.0, 10.0), corner(0.0, 10.0)];
/// data.add_attachment(window, "window", ClippingMask::new(square, None)); // clips every slot after it
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ClippingMask {
    /// The polygon's corners, in order, at rest (skeleton units).
    pub vertices: Vec<MeshVertex>,
    /// The last slot it clips; `None`: every slot drawn after it.
    pub end: Option<usize>,
}

impl ClippingMask {
    /// A mask with corners `vertices` (in order around the polygon) that
    /// clips the slots drawn after its own up to `end` (`None`: all of them).
    #[must_use]
    pub fn new(vertices: Vec<MeshVertex>, end: Option<usize>) -> Self {
        Self { vertices, end }
    }
}

/// What a slot can show.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Attachment {
    /// A rigid image on the slot's bone.
    Sprite(SpritePart),
    /// A mesh deformed by its bones.
    Mesh(SkinnedMesh),
    /// Nothing itself: clips what the next slots draw.
    Clipping(ClippingMask),
}

/// The texture of attachments that show no image.
const NO_TEXTURE: Texture = Texture::new(u32::MAX, 0, 0);

impl Attachment {
    /// The image it shows (an empty texture for a [`ClippingMask`], which
    /// shows none).
    #[must_use]
    pub fn texture(&self) -> Texture {
        match self {
            Self::Sprite(s) => s.texture,
            Self::Mesh(m) => m.texture,
            Self::Clipping(_) => NO_TEXTURE,
        }
    }

    /// The image file it was loaded from (`""` for one made in code).
    pub(crate) fn image(&self) -> &str {
        match self {
            Self::Sprite(s) => &s.image,
            Self::Mesh(m) => &m.image,
            Self::Clipping(_) => "",
        }
    }

    /// Its image file and texture, for the ones that show an image.
    pub(crate) fn texture_mut(&mut self) -> Option<(&str, &mut Texture)> {
        match self {
            Self::Sprite(s) => Some((&s.image, &mut s.texture)),
            Self::Mesh(m) => Some((&m.image, &mut m.texture)),
            Self::Clipping(_) => None,
        }
    }

    /// The vertices that follow bones by weight: a mesh's, or a clipping
    /// mask's corners.
    fn weighted(&self) -> Option<&[MeshVertex]> {
        match self {
            Self::Sprite(_) => None,
            Self::Mesh(m) => Some(&m.vertices),
            Self::Clipping(c) => Some(&c.vertices),
        }
    }
}

/// An inverse-kinematics constraint, applied after animation every update.
/// With two bones (the second a child of the first) it bends the chain so
/// the end of the second reaches the target; with one bone it turns the
/// bone to point at the target (look-at).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct IkConstraint {
    /// Its name, unique in the skeleton.
    pub name: Arc<str>,
    /// The bones it moves: `[bone]` looks at the target, `[upper, lower]`
    /// reaches it.
    pub bones: Vec<usize>,
    /// The bone whose position is the target, unless the game sets one
    /// with [`Skeleton::set_ik_target`](super::Skeleton::set_ik_target).
    pub target: usize,
    /// Two bones: bend so the joint turns the positive way (clockwise on
    /// screen) from the line to the target; `false` the other way.
    pub bend_positive: bool,
    /// How much it applies, 0 (none) to 1 (fully).
    pub mix: f32,
}

impl IkConstraint {
    /// A constraint named `name` moving `bones` (one bone looks at the
    /// target, two reach it) toward bone `target`, bending the positive
    /// way, fully applied.
    pub fn new(name: &str, bones: &[usize], target: usize) -> Self {
        Self {
            name: name.into(),
            bones: bones.to_vec(),
            target,
            bend_positive: true,
            mix: 1.0,
        }
    }

    /// Bends the other way (two bones).
    pub fn bend_negative(mut self) -> Self {
        self.bend_positive = false;
        self
    }

    /// Applies it by `mix`, 0 (none) to 1 (fully).
    pub fn with_mix(mut self, mix: f32) -> Self {
        self.mix = mix;
        self
    }
}

/// A transform constraint, applied after animation (in its turn among the
/// IK constraints): its bones take on some of its target bone's rotation,
/// position, scale and shear, as Spine's transform constraints do.
///
/// In world space (the default) each bone moves toward the target's
/// skeleton-space transform; `local` mixes the bones' poses relative to
/// their parents instead. `relative` adds the target's transform to the
/// bone's rather than moving toward it. Each mix is 0 (none) to 1 (fully);
/// the offsets are added to the target's values.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{BonePose, SkeletonData, TransformConstraint};
/// let mut data = SkeletonData::new();
/// let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
/// let leader = data.add_bone("leader", Some(root), BonePose::at(vec2(10.0, 0.0)), 0.0);
/// let follower = data.add_bone("follower", Some(root), BonePose::IDENTITY, 0.0);
/// let mut follow = TransformConstraint::new("follow", &[follower], leader);
/// follow.mix_translate = vec2(0.5, 0.5); // half way to the leader
/// data.add_transform_constraint(follow);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TransformConstraint {
    /// Its name, unique in the skeleton.
    pub name: Arc<str>,
    /// The bones it moves, in order.
    pub bones: Vec<usize>,
    /// The bone they follow.
    pub target: usize,
    /// How much of the target's rotation the bones take.
    pub mix_rotate: f32,
    /// How much of the target's position, along x and along y.
    pub mix_translate: Vec2,
    /// How much of the target's scale, along the bones' x and y axes.
    pub mix_scale: Vec2,
    /// How much of the target's shear of its y axis.
    pub mix_shear_y: f32,
    /// Added to the target's rotation (radians, clockwise on screen).
    pub offset_rotation: f32,
    /// A point in the target's space whose position the bones take
    /// (world space), or added to the target's translation (`local`).
    pub offset_translation: Vec2,
    /// Added to the target's scale.
    pub offset_scale: Vec2,
    /// Added to the target's y shear (radians).
    pub offset_shear_y: f32,
    /// Mix the bones' poses relative to their parents with the target's,
    /// instead of their skeleton-space transforms.
    pub local: bool,
    /// Add the target's transform to the bones' instead of moving them
    /// toward it.
    pub relative: bool,
}

impl TransformConstraint {
    /// A constraint named `name` that moves `bones` fully onto bone
    /// `target`'s skeleton-space rotation, position, scale and shear, with
    /// no offsets.
    #[must_use]
    pub fn new(name: &str, bones: &[usize], target: usize) -> Self {
        Self {
            name: name.into(),
            bones: bones.to_vec(),
            target,
            mix_rotate: 1.0,
            mix_translate: Vec2::ONE,
            mix_scale: Vec2::ONE,
            mix_shear_y: 1.0,
            offset_rotation: 0.0,
            offset_translation: Vec2::ZERO,
            offset_scale: Vec2::ZERO,
            offset_shear_y: 0.0,
            local: false,
            relative: false,
        }
    }
}

/// A constraint in the order constraints apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constraint {
    Ik(usize),
    Transform(usize),
}

/// An attachment under a name in a slot of a skin.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Named {
    pub(crate) skin: usize,
    pub(crate) slot: usize,
    pub(crate) name: Arc<str>,
    pub(crate) attachment: Attachment,
    /// For meshes and clipping masks: each vertex's influences in its
    /// bones' rest spaces (bone, position, weight), four per vertex.
    pub(crate) binds: Vec<(u16, Vec2, f32)>,
    /// For meshes: the bounds of its rest positions, for sprite shaders' `local`.
    pub(crate) bounds: (Vec2, Vec2),
}

/// Why a skeleton was refused: a name used twice or not found, an index
/// out of range, a value out of range. The message says which.
///
/// ```
/// use ponko::skeleton2d::SkeletonData;
/// let err = SkeletonData::from_json("{}", "").unwrap_err();
/// assert!(err.to_string().contains("format"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkeletonError(pub(crate) String);

impl fmt::Display for SkeletonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SkeletonError {}

impl From<SkeletonError> for crate::error::Error {
    fn from(e: SkeletonError) -> Self {
        crate::error::Error::new(crate::error::ErrorKind::Decode, e.0)
    }
}

pub(crate) fn err(message: impl Into<String>) -> SkeletonError {
    SkeletonError(message.into())
}

/// A 2D skeleton: its bones, slots, attachments, skins, clips, IK
/// constraints and, optionally, a state machine. Built once (in code or
/// from a file with [`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d)),
/// shared by every [`Skeleton`](super::Skeleton) that plays it.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{BonePose, SkeletonData, SpritePart};
/// # fn f(ctx: &mut Ctx) {
/// let body = ctx.gfx.load_texture("body.png");
/// let mut data = SkeletonData::new();
/// let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
/// let slot = data.add_slot("body", root);
/// data.add_attachment(slot, "body", SpritePart::new(body));
/// # }
/// ```
#[derive(Clone, Default)]
pub struct SkeletonData {
    pub(crate) name: Option<Arc<str>>,
    pub(crate) bones: Vec<Bone>,
    /// Each bone's rest pose in skeleton space, and its inverse.
    pub(crate) rest_world: Vec<(Affine2, Affine2)>,
    pub(crate) slots: Vec<Slot>,
    pub(crate) skins: Vec<Arc<str>>,
    pub(crate) attachments: Vec<Named>,
    pub(crate) lookup: HashMap<(usize, usize, Arc<str>), usize>,
    pub(crate) clips: Vec<Clip>,
    pub(crate) ik: Vec<IkConstraint>,
    pub(crate) transforms: Vec<TransformConstraint>,
    /// Every constraint, in the order they apply.
    pub(crate) constraints: Vec<Constraint>,
    pub(crate) graph: Option<Arc<SkeletonGraph>>,
    /// What an import left out or changed, each naming the file and the
    /// feature; printed when the file loads.
    pub(crate) warnings: Vec<String>,
    /// Images whose colors are premultiplied by alpha (a `pma` atlas page).
    pub(crate) premultiplied: bool,
    /// Images to sample without smoothing (`filter: Nearest` atlas pages).
    pub(crate) nearest: Vec<Arc<str>>,
}

impl fmt::Debug for SkeletonData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkeletonData")
            .field("name", &self.name)
            .field("bones", &self.bones.len())
            .field("slots", &self.slots.len())
            .field("attachments", &self.attachments.len())
            .field("clips", &self.clips.len())
            .finish_non_exhaustive()
    }
}

impl SkeletonData {
    /// An empty skeleton with the `"default"` skin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            skins: vec!["default".into()],
            ..Default::default()
        }
    }

    /// The file it was loaded from, if it was.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// What importing it left out: each warning names the file and the
    /// feature it doesn't read (a Spine inherit mode, a linked mesh...).
    /// [`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d) prints
    /// them when the file loads. Empty for Ponko's own format, which
    /// refuses what it doesn't know.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Whether its images' colors are premultiplied by alpha (a Spine
    /// atlas with `pma: true`). Skeletons playing it draw with
    /// [`BlendMode::Premultiplied`](crate::render::BlendMode::Premultiplied)
    /// unless their entity has a blend mode of its own.
    #[must_use]
    pub fn premultiplied(&self) -> bool {
        self.premultiplied
    }

    /// Adds a bone and returns its index. A parent must be added first.
    ///
    /// # Panics
    /// If the name is taken or `parent` isn't a bone.
    pub fn add_bone(&mut self, name: &str, parent: Option<usize>, rest: BonePose, length: f32) -> usize {
        assert!(
            self.bone(name).is_none(),
            "ponko: the skeleton has a bone named \"{name}\" already; use a unique name"
        );
        assert!(
            parent.is_none_or(|p| p < self.bones.len()),
            "ponko: bone \"{name}\"'s parent isn't in the skeleton (add parents first)"
        );
        let local = rest.matrix();
        let world = parent.map_or(local, |p| self.rest_world[p].0 * local);
        self.rest_world.push((world, world.inverse()));
        self.bones.push(Bone {
            name: name.into(),
            parent,
            rest,
            length,
        });
        self.bones.len() - 1
    }

    /// Adds a slot on `bone`, drawn above the slots added before, and
    /// returns its index. It shows nothing until it gets an attachment:
    /// the first one added to it becomes what it shows at rest.
    ///
    /// # Panics
    /// If the name is taken or `bone` isn't a bone.
    pub fn add_slot(&mut self, name: &str, bone: usize) -> usize {
        assert!(
            self.slot(name).is_none(),
            "ponko: the skeleton has a slot named \"{name}\" already; use a unique name"
        );
        assert!(
            bone < self.bones.len(),
            "ponko: slot \"{name}\"'s bone isn't in the skeleton; add the bone before the slot"
        );
        self.slots.push(Slot {
            name: name.into(),
            bone,
            attachment: None,
            color: Color::WHITE,
        });
        self.slots.len() - 1
    }

    /// Sets what slot `slot` shows at rest (`None`: nothing) and its tint.
    ///
    /// # Panics
    /// If `slot` isn't a slot.
    pub fn set_slot_rest(&mut self, slot: usize, attachment: Option<&str>, color: Color) {
        let s = &mut self.slots[slot];
        s.attachment = attachment.map(Into::into);
        s.color = color;
    }

    /// Adds a skin (a set of attachments that replaces the default skin's
    /// under the same names) and returns its index; `"default"` is 0.
    ///
    /// # Panics
    /// If the name is taken.
    pub fn add_skin(&mut self, name: &str) -> usize {
        assert!(
            self.skin(name).is_none(),
            "ponko: the skeleton has a skin named \"{name}\" already; use a unique name"
        );
        self.skins.push(name.into());
        self.skins.len() - 1
    }

    /// Adds `attachment` to slot `slot` of the default skin under `name`.
    /// The first attachment of a slot is what it shows at rest.
    ///
    /// # Panics
    /// If `slot` isn't a slot, the name is taken in the slot, or a mesh
    /// vertex names a bone that isn't there.
    pub fn add_attachment(&mut self, slot: usize, name: &str, attachment: impl Into<Attachment>) {
        self.add_skin_attachment(0, slot, name, attachment);
    }

    /// [`add_attachment`](Self::add_attachment) into skin `skin`.
    ///
    /// # Panics
    /// As `add_attachment`, and if `skin` isn't a skin.
    pub fn add_skin_attachment(&mut self, skin: usize, slot: usize, name: &str, attachment: impl Into<Attachment>) {
        assert!(
            skin < self.skins.len(),
            "ponko: skin {skin} isn't in the skeleton; add the skin first"
        );
        assert!(
            slot < self.slots.len(),
            "ponko: slot {slot} isn't in the skeleton; add the slot first"
        );
        let key = (skin, slot, Arc::<str>::from(name));
        assert!(
            !self.lookup.contains_key(&key),
            "ponko: slot \"{}\" has an attachment named \"{name}\" already; use a unique name",
            self.slots[slot].name
        );
        let attachment = attachment.into();
        let mut binds = Vec::new();
        let mut bounds = (Vec2::ZERO, Vec2::ONE);
        if let Attachment::Clipping(mask) = &attachment {
            assert!(
                mask.end.is_none_or(|e| e < self.slots.len()),
                "ponko: clipping mask \"{name}\" ends at a slot that isn't in the skeleton; add that slot first or end at None"
            );
        }
        if let Some(vertices) = attachment.weighted() {
            let mut min = Vec2::splat(f32::MAX);
            let mut max = Vec2::splat(f32::MIN);
            for v in vertices {
                min = min.min(v.position);
                max = max.max(v.position);
                for (bone, weight) in v.bones.into_iter().zip(v.weights) {
                    let b = usize::from(bone);
                    assert!(
                        weight == 0.0 || b < self.bones.len(),
                        "ponko: mesh \"{name}\" names bone {b}, which isn't in the skeleton; add that bone before the mesh"
                    );
                    let local = if weight == 0.0 {
                        Vec2::ZERO
                    } else {
                        self.rest_world[b].1.transform_point2(v.position)
                    };
                    binds.push((bone, local, weight));
                }
            }
            if !vertices.is_empty() {
                bounds = (min, (max - min).max(Vec2::splat(1e-6)));
            }
        }
        if skin == 0 && self.slots[slot].attachment.is_none() && !self.attachments.iter().any(|a| a.slot == slot) {
            self.slots[slot].attachment = Some(key.2.clone());
        }
        self.lookup.insert(key.clone(), self.attachments.len());
        self.attachments.push(Named {
            skin,
            slot,
            name: key.2,
            attachment,
            binds,
            bounds,
        });
    }

    /// Adds clip `clip` (its bone and slot indices must be this
    /// skeleton's) and returns its index.
    ///
    /// # Panics
    /// If a clip with its name is there already, or it names a bone or slot
    /// that isn't.
    pub fn add_clip(&mut self, clip: Clip) -> usize {
        assert!(
            self.clip(&clip.name).is_none(),
            "ponko: the skeleton has a clip named \"{}\" already; use a unique name",
            clip.name
        );
        if let Err(e) = clip
            .check(self.bones.len(), self.slots.len())
            .and_then(|()| self.check_deform(&clip))
        {
            panic!("ponko: {e}");
        }
        self.clips.push(clip);
        self.clips.len() - 1
    }

    /// Adds an IK constraint and returns its index. Constraints apply in
    /// the order they were added.
    ///
    /// # Panics
    /// If the name is taken, it moves no bone or more than two, the second
    /// bone isn't a child of the first, or a bone isn't there.
    pub fn add_ik(&mut self, ik: IkConstraint) -> usize {
        if let Err(e) = self.check_ik(&ik) {
            panic!("ponko: {e}");
        }
        self.push_ik(ik)
    }

    pub(crate) fn push_ik(&mut self, ik: IkConstraint) -> usize {
        self.ik.push(ik);
        self.constraints.push(Constraint::Ik(self.ik.len() - 1));
        self.ik.len() - 1
    }

    /// Adds a transform constraint and returns its index. Constraints (IK
    /// and transform) apply in the order they were added.
    ///
    /// # Panics
    /// If the name is taken, a bone isn't there, the target is one of its
    /// bones, or a mix or offset isn't a number.
    pub fn add_transform_constraint(&mut self, constraint: TransformConstraint) -> usize {
        if let Err(e) = self.check_transform(&constraint) {
            panic!("ponko: {e}");
        }
        self.push_transform(constraint)
    }

    pub(crate) fn push_transform(&mut self, constraint: TransformConstraint) -> usize {
        self.transforms.push(constraint);
        self.constraints.push(Constraint::Transform(self.transforms.len() - 1));
        self.transforms.len() - 1
    }

    pub(crate) fn check_transform(&self, c: &TransformConstraint) -> Result<(), SkeletonError> {
        let n = self.bones.len();
        if self.transform_constraint(&c.name).is_some() {
            return Err(err(format!(
                "the skeleton has a transform constraint named \"{}\" already; use a unique name",
                c.name
            )));
        }
        if c.target >= n || c.bones.iter().any(|&b| b >= n) {
            return Err(err(format!(
                "transform constraint \"{}\" needs bones and a target that are in the skeleton",
                c.name
            )));
        }
        if c.bones.contains(&c.target) {
            return Err(err(format!(
                "transform constraint \"{}\" can't move its own target",
                c.name
            )));
        }
        let numbers = [
            c.mix_rotate,
            c.mix_translate.x,
            c.mix_translate.y,
            c.mix_scale.x,
            c.mix_scale.y,
            c.mix_shear_y,
            c.offset_rotation,
            c.offset_translation.x,
            c.offset_translation.y,
            c.offset_scale.x,
            c.offset_scale.y,
            c.offset_shear_y,
        ];
        if !numbers.iter().all(|v| v.is_finite()) {
            return Err(err(format!(
                "transform constraint \"{}\"'s mixes and offsets must be numbers",
                c.name
            )));
        }
        Ok(())
    }

    /// `Ok` if every deform track of `clip` names a mesh of this skeleton
    /// and has one offset per vertex and influence (four per vertex).
    pub(crate) fn check_deform(&self, clip: &super::Clip) -> Result<(), SkeletonError> {
        for t in &clip.deform {
            let vertices = self
                .lookup
                .get(&(t.skin, t.slot, t.attachment.clone()))
                .and_then(|&i| self.attachments[i].attachment.weighted());
            let Some(vertices) = vertices else {
                return Err(err(format!(
                    "clip \"{}\" deforms \"{}\" in slot {} of skin {}, which isn't a mesh or clipping mask there",
                    clip.name, t.attachment, t.slot, t.skin
                )));
            };
            let want = vertices.len() * 4;
            if t.keys.iter().any(|k| k.value.len() != want) {
                return Err(err(format!(
                    "clip \"{}\": every deform key of \"{}\" needs {want} offsets (four per vertex)",
                    clip.name, t.attachment
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn check_ik(&self, ik: &IkConstraint) -> Result<(), SkeletonError> {
        let n = self.bones.len();
        if self.ik_constraint(&ik.name).is_some() {
            return Err(err(format!(
                "the skeleton has an IK constraint named \"{}\" already; use a unique name",
                ik.name
            )));
        }
        if ik.bones.is_empty() || ik.bones.len() > 2 || ik.target >= n || ik.bones.iter().any(|&b| b >= n) {
            return Err(err(format!(
                "IK constraint \"{}\" needs one or two bones and a target that are in the skeleton",
                ik.name
            )));
        }
        if ik.bones.len() == 2 && self.bones[ik.bones[1]].parent != Some(ik.bones[0]) {
            return Err(err(format!(
                "IK constraint \"{}\"'s second bone must be a child of its first",
                ik.name
            )));
        }
        if !ik.mix.is_finite() {
            return Err(err(format!("IK constraint \"{}\"'s mix must be a number", ik.name)));
        }
        Ok(())
    }

    /// Sets the state machine every [`Skeleton`](super::Skeleton) made from
    /// this data starts with.
    ///
    /// # Errors
    /// If a state plays a clip that isn't in the skeleton.
    pub fn set_graph(&mut self, graph: Arc<SkeletonGraph>) -> Result<(), SkeletonError> {
        graph.check_clips(self.clips.len())?;
        self.graph = Some(graph);
        Ok(())
    }

    /// Its bones, parents before children.
    #[must_use]
    pub fn bones(&self) -> &[Bone] {
        &self.bones
    }

    /// Its slots, in rest draw order (later on top).
    #[must_use]
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// Its clips.
    #[must_use]
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Its IK constraints.
    #[must_use]
    pub fn ik_constraints(&self) -> &[IkConstraint] {
        &self.ik
    }

    /// Its transform constraints.
    #[must_use]
    pub fn transform_constraints(&self) -> &[TransformConstraint] {
        &self.transforms
    }

    /// Its skins' names; `"default"` is the first.
    #[must_use]
    pub fn skins(&self) -> &[Arc<str>] {
        &self.skins
    }

    /// Its state machine, if it has one.
    #[must_use]
    pub fn graph(&self) -> Option<&Arc<SkeletonGraph>> {
        self.graph.as_ref()
    }

    /// The index of the bone named `name`.
    #[must_use]
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| &*b.name == name)
    }

    /// The index of the slot named `name`.
    #[must_use]
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.slots.iter().position(|s| &*s.name == name)
    }

    /// The index of the clip named `name`.
    #[must_use]
    pub fn clip(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| &*c.name == name)
    }

    /// The index of the skin named `name`.
    #[must_use]
    pub fn skin(&self, name: &str) -> Option<usize> {
        self.skins.iter().position(|s| &**s == name)
    }

    /// The index of the IK constraint named `name`.
    #[must_use]
    pub fn ik_constraint(&self, name: &str) -> Option<usize> {
        self.ik.iter().position(|c| &*c.name == name)
    }

    /// The index of the transform constraint named `name`.
    #[must_use]
    pub fn transform_constraint(&self, name: &str) -> Option<usize> {
        self.transforms.iter().position(|c| &*c.name == name)
    }

    /// The attachment slot `slot` shows under `name` with skin `skin`: the
    /// skin's own, else the default skin's.
    #[must_use]
    pub fn attachment(&self, skin: usize, slot: usize, name: &str) -> Option<&Attachment> {
        self.find(skin, slot, name).map(|i| &self.attachments[i].attachment)
    }

    pub(crate) fn find(&self, skin: usize, slot: usize, name: &str) -> Option<usize> {
        // `Arc<str>: Borrow<str>` doesn't reach into the tuple: look up
        // with a key made for it (only when an attachment changes).
        let key = |skin| (skin, slot, Arc::<str>::from(name));
        self.lookup
            .get(&key(skin))
            .or_else(|| (skin != 0).then(|| self.lookup.get(&key(0))).flatten())
            .copied()
    }

    /// Every image file its attachments name, once each.
    #[must_use]
    pub fn images(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for a in &self.attachments {
            let image = a.attachment.image();
            if !image.is_empty() && !names.contains(&image) {
                names.push(image);
            }
        }
        names
    }
}

impl From<SpritePart> for Attachment {
    fn from(part: SpritePart) -> Self {
        Self::Sprite(part)
    }
}

impl From<SkinnedMesh> for Attachment {
    fn from(mesh: SkinnedMesh) -> Self {
        Self::Mesh(mesh)
    }
}

impl From<ClippingMask> for Attachment {
    fn from(mask: ClippingMask) -> Self {
        Self::Clipping(mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matrix_decomposes_into_a_pose_that_builds_it_again() {
        for (t, r, s, h) in [
            (Vec2::new(3.0, -2.0), 0.7, Vec2::new(2.0, 0.5), 0.0),
            (Vec2::ZERO, -2.5, Vec2::new(1.5, -3.0), 0.4),
            (Vec2::ONE, 1.2, Vec2::new(-1.0, 2.0), -0.9),
        ] {
            let m = BonePose::new(t, r, s).with_shear(Vec2::new(0.0, h)).matrix();
            let back = BonePose::of_matrix(m).matrix();
            assert!(back.abs_diff_eq(m, 1e-4), "{m} != {back}");
        }
    }
}
