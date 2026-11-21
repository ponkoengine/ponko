//! Crowds: thousands of skinned characters drawn as one batch.
//!
//! A [`Crowd`] is the skinned sibling of an
//! [`InstanceBatch`](crate::render::InstanceBatch): one record per
//! character (where it stands, its tint, which clip it plays and how far
//! in, and a second clip it blends to), culled per view on the GPU and
//! drawn with one indirect draw per level of detail and mesh part. Nothing
//! is posed on the CPU each frame: the clips are baked once into a
//! [`CrowdAnimation`] (each joint's matrix at a fixed rate), kept in a
//! storage buffer, and every vertex is skinned in the vertex shader from
//! it, between the two nearest baked frames. Devices without compute
//! culling (WebGL2) take a CPU path instead: the baked frames are posed
//! once when the crowd is made, and each frame the characters are culled
//! on the CPU and drawn in groups that show the same frame (see the
//! guide's "Crowds").
use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::collect::Instance3D;
use super::skin::{Joint, MeshSkin, Skeleton};
use super::{AnimationClip, Mesh};
use crate::color::Color;
use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;

/// The most vec4s a crowd's baked animation takes on the GPU (32 MiB):
/// three per mesh joint and frame, per level, after the clip table.
pub(crate) const MAX_ANIMATION_ROWS: usize = 1 << 21;
/// The most levels of detail a crowd has.
pub const MAX_CROWD_LEVELS: usize = 8;
/// The most characters in one crowd.
pub const MAX_CROWD_INSTANCES: usize = 1 << 22;

/// A clip to bake into a [`CrowdAnimation`], and whether it loops.
///
/// ```
/// use ponko::anim::{AnimationClip, Skeleton};
/// use ponko::render::CrowdClip;
/// let s = Skeleton::new(vec![]);
/// let walk = CrowdClip::looping(AnimationClip::new("walk", &s, vec![]));
/// let fall = CrowdClip::once(AnimationClip::new("fall", &s, vec![]));
/// assert!(walk.looping && !fall.looping);
/// ```
///
/// New fields may be added in any release: build one with
/// [`looping`](Self::looping) or [`once`](Self::once).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CrowdClip {
    /// The clip.
    pub clip: AnimationClip,
    /// Past its end it starts again (`true`), or holds its last pose.
    pub looping: bool,
}

impl CrowdClip {
    /// `clip`, starting again past its end.
    #[must_use]
    pub fn looping(clip: AnimationClip) -> Self {
        Self { clip, looping: true }
    }

    /// `clip`, holding its last pose past its end.
    #[must_use]
    pub fn once(clip: AnimationClip) -> Self {
        Self { clip, looping: false }
    }
}

impl From<AnimationClip> for CrowdClip {
    fn from(clip: AnimationClip) -> Self {
        Self::looping(clip)
    }
}

/// One baked clip: its name, its first frame in the bake, how many frames
/// (the last at its end), its length and whether it loops.
#[derive(Clone, Debug)]
pub(crate) struct BakedClip {
    pub name: String,
    pub first: u32,
    pub frames: u32,
    pub duration: f32,
    pub looping: bool,
}

#[derive(Debug)]
pub(crate) struct Baked {
    /// The skeleton's joints (meshes made for another skeleton are matched by name).
    pub joints: Arc<[Joint]>,
    pub fps: f32,
    pub clips: Vec<BakedClip>,
    /// Frame after frame, every joint's matrix in the model's space.
    pub matrices: Vec<Mat4>,
}

/// Animation clips baked for crowds: every joint's matrix at a fixed rate,
/// clip after clip. Baked once on the CPU, shared by every crowd made from
/// it (cloning is cheap). Characters play its clips by index (see
/// [`clip`](Self::clip)); between two baked frames the pose is blended, so
/// 30 frames a second is plenty for walking people.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimationClip, AnimationTrack, Interpolation, Joint, JointAnimation, Skeleton};
/// use ponko::render::CrowdAnimation;
/// let skeleton = Skeleton::new(vec![Joint::new("hips", None)]);
/// let mut bob = JointAnimation::new(0, "hips");
/// bob.translation = Some(AnimationTrack::new(
///     vec![0.0, 0.5, 1.0],
///     vec![Vec4::ZERO, Vec4::new(0.0, 0.1, 0.0, 0.0), Vec4::ZERO],
///     Interpolation::Linear,
/// ));
/// let walk = AnimationClip::new("walk", &skeleton, vec![bob]);
/// let animation = CrowdAnimation::bake(&skeleton, &[walk.into()], 30.0);
/// assert_eq!(animation.clip("walk"), Some(0));
/// assert_eq!(animation.frames(), 31); // one second at 30 frames, both ends
/// ```
#[derive(Clone, Debug)]
pub struct CrowdAnimation(pub(crate) Arc<Baked>);

impl CrowdAnimation {
    /// Samples every clip of `clips` on `skeleton` at `fps` frames a second,
    /// from 0 to its duration (both ends: a one-second clip at 30 fps is 31
    /// frames). Joints a clip doesn't move stay at rest.
    ///
    /// # Panics
    ///
    /// If `fps` is not finite and positive. A crowd holds at most 2,097,152
    /// rows of the bake (three per mesh joint and frame, per level of
    /// detail); [`Graphics::create_crowd`] says so if one needs more.
    #[must_use]
    pub fn bake(skeleton: &Skeleton, clips: &[CrowdClip], fps: f32) -> Self {
        assert!(
            fps.is_finite() && fps > 0.0,
            "CrowdAnimation::bake: fps must be finite and > 0; 30 suits most clips"
        );
        let joints = skeleton.shared_joints().clone();
        let mut pose = Skeleton::from_shared(joints.clone());
        let mut baked = Vec::new();
        let mut matrices = Vec::new();
        let mut first = 0u32;
        for c in clips {
            // Finite and nonnegative: the last key time of tracks that
            // `AnimationTrack::try_new` checked.
            let duration = c.clip.duration();
            let frames = (duration * fps).round() as u32 + 1;
            for f in 0..frames {
                let t = (f as f32 / fps).min(duration);
                pose.reset_pose();
                c.clip.sample(t, &mut pose);
                pose.update();
                matrices.extend_from_slice(pose.matrices());
            }
            baked.push(BakedClip {
                name: c.clip.name().to_owned(),
                first,
                frames,
                duration,
                looping: c.looping,
            });
            first += frames;
        }
        Self(Arc::new(Baked {
            joints,
            fps,
            clips: baked,
            matrices,
        }))
    }

    /// The index of the first clip called `name`: what
    /// [`CrowdInstance::playing`] takes.
    #[must_use]
    pub fn clip(&self, name: &str) -> Option<u16> {
        self.0.clips.iter().position(|c| c.name == name).map(|i| i as u16)
    }

    /// How many clips it has.
    #[must_use]
    pub fn clip_count(&self) -> usize {
        self.0.clips.len()
    }

    /// Clip `clip`'s length in seconds, or `None` past the last clip.
    ///
    /// ```
    /// # use ponko::anim::{AnimationClip, Skeleton};
    /// # use ponko::render::CrowdAnimation;
    /// let s = Skeleton::new(vec![]);
    /// let a = CrowdAnimation::bake(&s, &[AnimationClip::new("idle", &s, vec![]).into()], 30.0);
    /// assert_eq!(a.duration(0), Some(0.0));
    /// assert_eq!(a.duration(1), None);
    /// ```
    #[must_use]
    pub fn duration(&self, clip: u16) -> Option<f32> {
        self.0.clips.get(usize::from(clip)).map(|c| c.duration)
    }

    /// Frames a second it was baked at.
    #[must_use]
    pub fn fps(&self) -> f32 {
        self.0.fps
    }

    /// Frames baked, over every clip.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.0.clips.iter().map(|c| c.frames as usize).sum()
    }
}

/// One level of detail of a [`Crowd`]: a skinned mesh drawn for characters
/// whose origin is in a half-open distance band, as an
/// [`InstanceLevel`](crate::render::InstanceLevel) does for static batches
/// (overlapping bands draw both; sun cascades measure from the main camera,
/// light shadows from the light). Far levels can be cheaper twice over:
/// a mesh with fewer triangles, made for a skeleton with fewer joints, and
/// fewer joints moving each corner ([`with_influences`](Self::with_influences)).
///
/// ```
/// # use ponko::prelude::*;
/// # use ponko::render::CrowdLevel;
/// # Game::new("t", 1, 1).run_headless(0, |ctx| {
/// # let mesh = ponko::render::Mesh::NONE;
/// // Up to 30 m with every joint; beyond, one joint a corner and no shadow.
/// let near = CrowdLevel::new(mesh, 0.0, 30.0);
/// let far = CrowdLevel::new(mesh, 30.0, f32::INFINITY).with_influences(1).with_shadows(false);
/// assert_eq!((near.influences, far.influences), (4, 1));
/// # });
/// ```
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) and the `with_*` methods.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CrowdLevel {
    /// A skinned mesh (made with
    /// [`create_skinned_mesh`](crate::gfx::Graphics::create_skinned_mesh) or
    /// loaded from a skinned glTF file). Must outlive the crowd: while it is
    /// freed, the whole crowd draws nothing.
    pub mesh: Mesh,
    /// Inclusive, finite, nonnegative lower distance.
    pub min_distance: f32,
    /// Exclusive upper distance; positive infinity is allowed.
    pub max_distance: f32,
    /// Drawn in the main camera and target cameras.
    pub camera: bool,
    /// Casts sun and local-light shadows (also needs
    /// [`CrowdOptions::cast_shadows`]).
    pub shadows: bool,
    /// Joints moving each corner, 1 to 4: the strongest ones, their
    /// weights scaled back to a whole. 4 keeps the mesh as made; 1 moves
    /// each corner rigidly with its main joint, the cheapest.
    pub influences: u8,
    /// How far before [`max_distance`](Self::max_distance) this level
    /// starts to crossfade, in world units: over that stretch it dithers
    /// out while the level whose band starts at its `max_distance` dithers
    /// in (or, if none does, it fades out to nothing), so characters
    /// don't pop from one mesh to the next. 0 (the default) switches at
    /// once. Finite, at most the band's width; a level whose
    /// `max_distance` is infinite never fades.
    pub fade: f32,
}

impl CrowdLevel {
    /// `mesh` between `min_distance` and `max_distance`, in cameras and
    /// shadows, with every joint of each corner.
    #[must_use]
    pub fn new(mesh: Mesh, min_distance: f32, max_distance: f32) -> Self {
        Self {
            mesh,
            min_distance,
            max_distance,
            camera: true,
            shadows: true,
            influences: 4,
            fade: 0.0,
        }
    }

    with_fields! {
        /// Sets [`camera`](Self::camera).
        camera: bool => with_camera;
        /// Sets [`shadows`](Self::shadows).
        shadows: bool => with_shadows;
        /// Sets [`influences`](Self::influences), 1 to 4.
        influences: u8 => with_influences;
        /// Sets [`fade`](Self::fade), the crossfade into the next level.
        fade: f32 => with_fade;
    }
}

/// One character of a [`Crowd`]: where it stands, its tint, the clip it
/// plays and how far in, and a second clip it blends to (a walk turning
/// into a run). Between updates its clips play on by themselves at
/// [`speed`](Self::speed), on the GPU: a crowd standing in place costs no
/// upload at all.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::render::CrowdInstance;
/// let walker = CrowdInstance::new(Mat4::from_translation(vec3(2.0, 0.0, 0.0)))
///     .playing(0, 0.25)
///     .blending(1, 0.1, 0.5) // half way to clip 1
///     .with_speed(1.2)
///     .with_color(Color::rgb(0.9, 0.2, 0.2));
/// assert_eq!((walker.clip, walker.blend_clip, walker.blend), (0, 1, 0.5));
/// ```
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) and the builder methods.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CrowdInstance {
    /// Model to world. Must be finite and affine.
    pub transform: Mat4,
    /// Tint, in sRGB like [`Model::with_color`](crate::render::Model::with_color).
    pub color: Color,
    /// The clip it plays, an index into its [`CrowdAnimation`].
    pub clip: u16,
    /// Seconds into [`clip`](Self::clip) at this update.
    pub time: f32,
    /// The clip it blends to.
    pub blend_clip: u16,
    /// Seconds into [`blend_clip`](Self::blend_clip) at this update.
    pub blend_time: f32,
    /// How much of [`blend_clip`](Self::blend_clip) it shows, 0 to 1
    /// (stored to 1/65535).
    pub blend: f32,
    /// How fast both clips play on until the next update, 1 for their own
    /// pace, 0 to hold (stored to 1/256, -127 to 127).
    pub speed: f32,
}

impl CrowdInstance {
    /// A white character at `transform` playing clip 0 from its start at
    /// its own pace.
    #[must_use]
    pub fn new(transform: Mat4) -> Self {
        Self {
            transform,
            color: Color::WHITE,
            clip: 0,
            time: 0.0,
            blend_clip: 0,
            blend_time: 0.0,
            blend: 0.0,
            speed: 1.0,
        }
    }

    /// Plays `clip`, `time` seconds in.
    #[must_use]
    pub fn playing(mut self, clip: u16, time: f32) -> Self {
        self.clip = clip;
        self.time = time;
        self
    }

    /// Shows `weight` (0 to 1) of `clip` at `time` seconds over the clip it plays.
    #[must_use]
    pub fn blending(mut self, clip: u16, time: f32, weight: f32) -> Self {
        self.blend_clip = clip;
        self.blend_time = time;
        self.blend = weight;
        self
    }

    with_fields! {
        /// Sets [`color`](Self::color).
        color: Color => with_color;
        /// Sets [`speed`](Self::speed).
        speed: f32 => with_speed;
    }
}

/// Settings for a crowd.
///
/// ```
/// let options = ponko::render::CrowdOptions::default().with_cast_shadows(false);
/// assert!(!options.cast_shadows);
/// ```
///
/// New fields may be added in any release: start from [`Default`] and use
/// the `with_*` methods.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CrowdOptions {
    /// Whether its levels with [`CrowdLevel::shadows`] cast sun and light
    /// shadows. A crowd that casts redraws the shadow views it reaches every
    /// frame (it moves). Default true.
    pub cast_shadows: bool,
}

impl Default for CrowdOptions {
    fn default() -> Self {
        Self { cast_shadows: true }
    }
}

impl CrowdOptions {
    with_fields! {
        /// Sets [`cast_shadows`](Self::cast_shadows).
        cast_shadows: bool => with_cast_shadows;
    }
}

/// A crowd of skinned characters: see [`Graphics::create_crowd`]. Free it
/// with [`Ctx::free`](crate::ctx::Ctx::free) before the meshes it draws.
/// Handles are never recycled; freeing twice is harmless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Crowd {
    pub(crate) id: u32,
}

/// One character as the GPU reads it: its instance record (model matrix,
/// decal layers, tint) and its animation (clips, times, blend and speed).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CrowdRecord {
    pub instance: Instance3D,
    /// Clip | blend clip << 16, the two times' bits, then the blend in
    /// unorm16 | the speed in signed 8.8 fixed point << 16.
    pub anim: [u32; 4],
}

impl CrowdRecord {
    fn new(i: &CrowdInstance) -> Self {
        let mut model = i.transform.to_cols_array_2d();
        model[0][3] = super::projected::receiver_mask(None);
        model[1][3] = 0.0;
        let blend = (i.blend.clamp(0.0, 1.0) * 65535.0).round() as u32;
        let speed = (i.speed.clamp(-127.0, 127.0) * 256.0).round() as i32 as i16 as u16;
        Self {
            instance: Instance3D {
                model,
                color: super::linear(i.color),
            },
            anim: [
                u32::from(i.clip) | u32::from(i.blend_clip) << 16,
                i.time.to_bits(),
                i.blend_time.to_bits(),
                blend | u32::from(speed) << 16,
            ],
        }
    }
}

/// A character's culling box in the world: the box of the clips it plays
/// (see [`CrowdData::clip_bounds`]) under its transform, a center and
/// three half axes (orthogonal unless the transform shears). The GPU
/// path's cull pass works out the same box (`crowd_cull.wgsl`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CullBox {
    pub center: Vec3,
    pub axes: [Vec3; 3],
}

impl CullBox {
    /// The model-space box `(lo, hi)` under `transform`.
    pub fn new(transform: &Mat4, (lo, hi): (Vec3, Vec3)) -> Self {
        let half = (hi - lo) * 0.5;
        Self {
            center: transform.transform_point3((lo + hi) * 0.5),
            axes: [
                transform.x_axis.truncate() * half.x,
                transform.y_axis.truncate() * half.y,
                transform.z_axis.truncate() * half.z,
            ],
        }
    }

    /// Half the size of the world-axis box around it.
    pub fn extent(&self) -> Vec3 {
        self.axes[0].abs() + self.axes[1].abs() + self.axes[2].abs()
    }

    /// Half its diagonal (its size, for global illumination's probe cut).
    pub fn radius(&self) -> f32 {
        self.axes.iter().map(|a| a.length_squared()).sum::<f32>().sqrt()
    }

    /// Whether it lies wholly behind `plane` (normal xyz, offset w).
    pub fn behind(&self, plane: glam::Vec4) -> bool {
        let n = plane.truncate();
        let reach: f32 = self.axes.iter().map(|a| n.dot(*a).abs()).sum();
        n.dot(self.center) + plane.w < -reach
    }
}

/// The model-space box of a character playing the clips of `anim` (its
/// record's animation words): its clip's, its blend clip's, or both
/// joined while it blends between them. `clip_bounds` has at least one clip.
pub(crate) fn character_bounds(clip_bounds: &[(Vec3, Vec3)], anim: [u32; 4]) -> (Vec3, Vec3) {
    let last = clip_bounds.len().saturating_sub(1);
    let of = |clip: u32| clip_bounds[(clip as usize).min(last)];
    let blend = anim[3] & 0xffff;
    match blend {
        0 => of(anim[0] & 0xffff),
        0xffff => of(anim[0] >> 16),
        _ => {
            let (a, b) = (of(anim[0] & 0xffff), of(anim[0] >> 16));
            (a.0.min(b.0), a.1.max(b.1))
        }
    }
}

/// Where one level's skin matrices start in the bake, and
/// how many the mesh has a frame (one per joint it binds).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Section {
    pub start: u32,
    pub slots: u32,
}

/// A crowd as the engine keeps it, and hands the renderer.
pub(crate) struct CrowdData {
    pub levels: Vec<CrowdLevel>,
    pub options: CrowdOptions,
    /// The bake as the GPU reads it, a vec4 each: the clip table (first frame, frames, fps,
    /// looping per clip), then each level's skin matrices (three rows a
    /// joint, joints of a frame together, frame after frame). Made anew only
    /// when the crowd is.
    pub texels: Arc<[[f32; 4]]>,
    pub sections: Vec<Section>,
    pub clips: u32,
    /// Per clip: a box (lowest and highest corner) around every level's
    /// mesh in every frame baked of it, in model space.
    pub clip_bounds: Vec<(Vec3, Vec3)>,
    pub records: Vec<CrowdRecord>,
    /// Per character: its culling box in the world (see [`CullBox`]).
    pub boxes: Vec<CullBox>,
    /// A box around every character's culling box and origin.
    pub min: Vec3,
    pub max: Vec3,
    /// Withdrawn from the renderer because a level's mesh was freed.
    pub suspended: bool,
}

/// Whether `instances` can be the characters of a crowd with `clips` clips
/// (see [`Graphics::try_update_crowd`]).
fn check_instances(instances: &[CrowdInstance], clips: u32) -> Result<(), Error> {
    let invalid = |message: String| Err(Error::new(ErrorKind::Invalid, message));
    if instances.len() > MAX_CROWD_INSTANCES {
        return invalid(format!(
            "crowd: {} characters, more than {MAX_CROWD_INSTANCES}; split them into several crowds",
            instances.len()
        ));
    }
    for (n, i) in instances.iter().enumerate() {
        if !i.transform.is_finite() || i.transform.row(3) != glam::Vec4::W {
            return invalid(format!("crowd: character {n} needs a finite affine transform"));
        }
        if u32::from(i.clip) >= clips || u32::from(i.blend_clip) >= clips {
            return invalid(format!(
                "crowd: character {n} plays clip {} or {}, past the animation's {clips} clips; use CrowdAnimation::clip for an index",
                i.clip, i.blend_clip
            ));
        }
        if !(i.time.is_finite() && i.blend_time.is_finite() && i.speed.is_finite() && i.blend.is_finite()) {
            return invalid(format!("crowd: character {n}'s times, blend and speed must be finite"));
        }
    }
    Ok(())
}

impl CrowdData {
    /// Makes `instances` (checked) the crowd's characters.
    fn set_instances(&mut self, instances: &[CrowdInstance]) {
        self.records.clear();
        self.boxes.clear();
        self.min = Vec3::splat(f32::INFINITY);
        self.max = Vec3::splat(f32::NEG_INFINITY);
        for i in instances {
            let m = i.transform;
            let record = CrowdRecord::new(i);
            let b = CullBox::new(&m, character_bounds(&self.clip_bounds, record.anim));
            self.records.push(record);
            self.boxes.push(b);
            let (e, origin) = (b.extent(), m.w_axis.truncate());
            self.min = self.min.min(b.center - e).min(origin);
            self.max = self.max.max(b.center + e).max(origin);
        }
    }
}

impl Graphics {
    /// A crowd of skinned characters, one per instance, drawn in few draws:
    /// culled per view (on the GPU where the device can), each [`CrowdLevel`]
    /// one indirect draw per mesh part and view, every vertex skinned in
    /// the vertex shader from `animation` (a storage buffer the crowd
    /// keeps). Shadows and motion vectors (temporal antialiasing) work as
    /// for models. Devices without compute culling (WebGL2) pose the baked
    /// frames once here, on the CPU, and draw each character in the frame
    /// nearest its time, in one draw per frame shown and view; they draw no
    /// motion vectors for crowds. Update the characters with
    /// [`update_crowd`](Self::update_crowd) (a whole new list, as often as
    /// every frame).
    ///
    /// Meshes made for another skeleton follow `animation`'s joints by
    /// name; a joint it doesn't have stays at rest. Morph targets are not
    /// drawn. See the guide's "Crowds" for the costs.
    ///
    /// # Panics
    ///
    /// For no levels or more than [`MAX_CROWD_LEVELS`], a level whose mesh
    /// is freed or not skinned, a distance band that is not finite,
    /// nonnegative and ordered, influences outside 1 to 4, a bake larger
    /// than a crowd holds (2,097,152 rows: three per mesh joint and frame,
    /// per level), and for the characters
    /// [`update_crowd`](Self::update_crowd) rejects.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationClip, Joint, Skeleton};
    /// use ponko::render::{CrowdAnimation, CrowdInstance, CrowdLevel, CrowdOptions, MeshData};
    /// Game::new("crowd", 1, 1).run_headless(0, |ctx| {
    ///     let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
    ///     let mut body = MeshData::cube(1.0);
    ///     body.joints = vec![[0; 4]; body.positions.len()];
    ///     body.weights = vec![[1.0, 0.0, 0.0, 0.0]; body.positions.len()];
    ///     let material = ctx.gfx.default_material();
    ///     let mesh = ctx.gfx.create_skinned_mesh(&body, material, &skeleton);
    ///     let idle = AnimationClip::new("idle", &skeleton, vec![]);
    ///     let animation = CrowdAnimation::bake(&skeleton, &[idle.into()], 30.0);
    ///     let people: Vec<CrowdInstance> = (0..100)
    ///         .map(|i| CrowdInstance::new(Mat4::from_translation(vec3(i as f32, 0.0, 0.0))))
    ///         .collect();
    ///     let levels = [CrowdLevel::new(mesh, 0.0, f32::INFINITY)];
    ///     let crowd = ctx.gfx.create_crowd(&animation, &levels, &people, CrowdOptions::default());
    ///     assert_eq!(ctx.gfx.crowd_len(crowd), Some(100));
    ///     ctx.free(crowd);
    /// });
    /// ```
    pub fn create_crowd(
        &mut self,
        animation: &CrowdAnimation,
        levels: &[CrowdLevel],
        instances: &[CrowdInstance],
        options: CrowdOptions,
    ) -> Crowd {
        self.try_create_crowd(animation, levels, instances, options)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_crowd`](Self::create_crowd) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::BadHandle`] for a level whose mesh is freed or from
    /// another game; [`ErrorKind::Invalid`] for everything else
    /// `create_crowd` panics on.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationClip, Joint, Skeleton};
    /// use ponko::render::{CrowdAnimation, CrowdInstance, CrowdLevel, CrowdOptions, MeshData};
    /// Game::new("crowd", 1, 1).run_headless(0, |ctx| {
    ///     let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
    ///     let idle = AnimationClip::new("idle", &skeleton, vec![]);
    ///     let animation = CrowdAnimation::bake(&skeleton, &[idle.into()], 30.0);
    ///     let material = ctx.gfx.default_material();
    ///     let plain = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
    ///     let levels = [CrowdLevel::new(plain, 0.0, f32::INFINITY)];
    ///     let people = [CrowdInstance::new(Mat4::IDENTITY)];
    ///     let error = ctx.gfx.try_create_crowd(&animation, &levels, &people, CrowdOptions::default()).unwrap_err();
    ///     assert_eq!(error.kind(), ErrorKind::Invalid); // the cube isn't skinned
    /// });
    /// ```
    pub fn try_create_crowd(
        &mut self,
        animation: &CrowdAnimation,
        levels: &[CrowdLevel],
        instances: &[CrowdInstance],
        options: CrowdOptions,
    ) -> Result<Crowd, Error> {
        let invalid = |message: String| Err(Error::new(ErrorKind::Invalid, message));
        if levels.is_empty() || levels.len() > MAX_CROWD_LEVELS {
            return invalid(format!(
                "crowd: give 1 to {MAX_CROWD_LEVELS} levels, not {}",
                levels.len()
            ));
        }
        let baked = &animation.0;
        let clips = baked.clips.len() as u32;
        if clips == 0 {
            return invalid("crowd: the animation has no clips; bake at least one".into());
        }
        let mut skins = Vec::with_capacity(levels.len());
        for level in levels {
            if !(level.min_distance.is_finite() && level.min_distance >= 0.0 && level.max_distance > level.min_distance)
            {
                return invalid("crowd level: use finite min_distance >= 0 and max_distance > min_distance".into());
            }
            if !(level.fade.is_finite() && level.fade >= 0.0 && level.fade <= level.max_distance - level.min_distance) {
                return invalid(format!(
                    "crowd level: fade must be finite, >= 0 and at most the band's width (max_distance - min_distance), not {}",
                    level.fade
                ));
            }
            if !(1..=4).contains(&level.influences) {
                return invalid(format!(
                    "crowd level: influences must be 1 to 4, not {}",
                    level.influences
                ));
            }
            if self.assets3d.live_bounds(level.mesh.id).is_none() {
                return Err(Error::new(
                    ErrorKind::BadHandle,
                    "crowd level: mesh is freed or foreign; create a live mesh in this game",
                ));
            }
            let Some(skin) = self
                .assets3d
                .skins
                .get(level.mesh.slot() as usize)
                .and_then(Option::as_ref)
            else {
                return invalid(
                    "crowd level: the mesh isn't skinned; make it with create_skinned_mesh or load a skinned glTF mesh"
                        .into(),
                );
            };
            skins.push(skin.clone());
        }
        check_instances(instances, clips)?;
        let frames = baked.clips.iter().map(|c| c.frames).sum::<u32>() as usize;
        let rows = baked.clips.len() + skins.iter().map(|s| frames * s.slots.len() * 3).sum::<usize>();
        if rows > MAX_ANIMATION_ROWS {
            return invalid(format!(
                "crowd: its baked animation needs {rows} rows, more than one crowd holds ({MAX_ANIMATION_ROWS}); bake fewer frames (a lower fps) or fewer clips, or give far levels meshes with fewer joints"
            ));
        }
        let mut texels: Vec<[f32; 4]> = baked
            .clips
            .iter()
            .map(|c| {
                [
                    c.first as f32,
                    c.frames as f32,
                    baked.fps,
                    if c.looping { 1.0 } else { 0.0 },
                ]
            })
            .collect();
        let joints = baked.joints.len();
        let mut sections = Vec::with_capacity(levels.len());
        // Per clip, a box around every level's slots (each a sphere around
        // the corners it moves) in every frame of the clip: skinning blends
        // those frames' matrices, so every pose shown lies inside.
        let mut clip_bounds = vec![(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)); baked.clips.len()];
        for skin in &skins {
            let slots = skin.slots.len();
            let start = texels.len() as u32;
            let joint_of: Vec<Option<usize>> = (0..slots).map(|s| joint_in(skin, &baked.joints, s)).collect();
            texels.reserve(frames * slots * 3);
            // The clips' frames follow each other: frame after frame, as `matrices`.
            for (clip, (lo, hi)) in baked.clips.iter().zip(&mut clip_bounds) {
                for g in clip.first as usize..(clip.first + clip.frames) as usize {
                    let pose = &baked.matrices[g * joints..(g + 1) * joints];
                    for s in 0..slots {
                        let m = joint_of[s].map_or(Mat4::IDENTITY, |j| pose[j] * skin.inverse_binds[s]);
                        texels.extend([m.row(0).to_array(), m.row(1).to_array(), m.row(2).to_array()]);
                        let (c, r) = skin.bounds[s];
                        if r >= 0.0 {
                            let scale = m.x_axis.length().max(m.y_axis.length()).max(m.z_axis.length());
                            let p = m.transform_point3(c);
                            *lo = lo.min(p - r * scale);
                            *hi = hi.max(p + r * scale);
                        }
                    }
                }
            }
            sections.push(Section {
                start,
                slots: slots as u32,
            });
        }
        for b in &mut clip_bounds {
            if !b.0.cmple(b.1).all() {
                *b = (Vec3::ZERO, Vec3::ZERO);
            }
        }
        let mut data = CrowdData {
            levels: levels.to_vec(),
            options,
            texels: texels.into(),
            sections,
            clips,
            clip_bounds,
            records: Vec::new(),
            boxes: Vec::new(),
            min: Vec3::ZERO,
            max: Vec3::ZERO,
            suspended: false,
        };
        data.set_instances(instances);
        let id = self.assets3d.crowds.len() as u32;
        if let Some(r) = self.renderer.as_mut() {
            r.set_crowd(id, Some(&data));
        }
        self.assets3d.crowds.push(Some(data));
        Ok(Crowd { id })
    }

    /// Replaces a crowd's characters (any number, up to
    /// [`MAX_CROWD_INSTANCES`]): as often as every frame for a walking
    /// crowd. Returns false for a freed crowd.
    ///
    /// In a frame that updates it, the motion vectors run from each
    /// character's last record to its new one (by index); in one that
    /// doesn't, from its clips' last frame.
    ///
    /// # Panics
    ///
    /// For a transform that is not finite and affine, a clip past the
    /// animation's clips, a time, blend or speed that is not finite, or
    /// more than [`MAX_CROWD_INSTANCES`] characters.
    ///
    /// See [`create_crowd`](Self::create_crowd) for an example.
    pub fn update_crowd(&mut self, crowd: Crowd, instances: &[CrowdInstance]) -> bool {
        self.try_update_crowd(crowd, instances)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`update_crowd`](Self::update_crowd) with errors as values: `Ok(false)`
    /// for a freed crowd. On an error the crowd keeps its characters.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] for the characters `update_crowd` panics on.
    pub fn try_update_crowd(&mut self, crowd: Crowd, instances: &[CrowdInstance]) -> Result<bool, Error> {
        let Some(Some(data)) = self.assets3d.crowds.get_mut(crowd.id as usize) else {
            return Ok(false);
        };
        check_instances(instances, data.clips)?;
        data.set_instances(instances);
        if !data.suspended
            && let Some(r) = self.renderer.as_mut()
        {
            r.set_crowd(crowd.id, Some(data));
        }
        Ok(true)
    }

    /// How many characters a crowd has, or `None` for a freed one.
    ///
    /// See [`create_crowd`](Self::create_crowd) for an example.
    #[must_use]
    pub fn crowd_len(&self, crowd: Crowd) -> Option<usize> {
        let data = self.assets3d.crowds.get(crowd.id as usize)?.as_ref()?;
        Some(data.records.len())
    }

    /// Frees crowd `id`: the engine side of [`Graphics::free`].
    pub(crate) fn free_crowd_id(&mut self, id: u32) {
        if let Some(slot) = self.assets3d.crowds.get_mut(id as usize)
            && slot.take().is_some()
            && let Some(r) = self.renderer.as_mut()
        {
            r.set_crowd(id, None);
        }
    }

    /// Withdraws crowds whose level meshes were freed from the renderer,
    /// and hands back those whose meshes are live again. Runs before each
    /// frame's 3D scene is collected.
    pub(crate) fn validate_crowds(&mut self) {
        let renderer = &mut self.renderer;
        self.assets3d.validate_crowds(|id, crowd| {
            if let Some(r) = renderer.as_mut() {
                r.set_crowd(id, crowd);
            }
        });
    }
}

impl super::Assets3D {
    /// Checks every crowd's level meshes against the mesh table, as
    /// [`validate_instance_batches`](Self::validate_instance_batches) does
    /// for batches: a crowd with a freed one is withdrawn (`send(id,
    /// None)`), and handed back once they are all live again.
    pub(crate) fn validate_crowds(&mut self, mut send: impl FnMut(u32, Option<&CrowdData>)) {
        for id in 0..self.crowds.len() {
            let Some(crowd) = &self.crowds[id] else {
                continue;
            };
            let live = crowd.levels.iter().all(|l| self.live_bounds(l.mesh.id).is_some());
            if live != crowd.suspended {
                continue;
            }
            if let Some(crowd) = self.crowds[id].as_mut() {
                crowd.suspended = !live;
                send(id as u32, live.then_some(&*crowd));
            }
        }
    }
}

/// The `n` strongest of a corner's four influences, their weights scaled
/// back to a whole (all zero if it has none): a level's
/// [`influences`](CrowdLevel::influences). The crowd vertex shader does the
/// same (`crowd_vertex.wgsl`).
pub(crate) fn top_influences(joints: [u16; 4], weights: [f32; 4], n: u8) -> ([u16; 4], [f32; 4]) {
    let mut order = [0usize, 1, 2, 3];
    // The same stable order as the shader's sorting network: ties keep their place.
    for round in 0..3 {
        for k in 0..3 - round {
            if weights[order[k + 1]] > weights[order[k]] {
                order.swap(k, k + 1);
            }
        }
    }
    let mut j = [0u16; 4];
    let mut w = [0f32; 4];
    for (k, &o) in order.iter().enumerate().take(usize::from(n.min(4))) {
        j[k] = joints[o];
        w[k] = weights[o];
    }
    let total: f32 = w.iter().sum();
    if total > 0.0 {
        w = w.map(|x| x / total);
    }
    (j, w)
}

/// The joint of `joints` that slot `slot` of `skin` follows: by index when
/// the mesh was made for these joints, else by name.
fn joint_in(skin: &MeshSkin, joints: &Arc<[Joint]>, slot: usize) -> Option<usize> {
    let own = usize::from(skin.slots[slot]);
    if Arc::ptr_eq(&skin.joints, joints) {
        return Some(own);
    }
    let name = &skin.joints[own].name;
    joints.iter().position(|j| &j.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{AnimationTrack, Interpolation, JointAnimation};
    use crate::d3::Transform as Transform3D;
    use crate::render::MeshData;
    use glam::{Quat, Vec4, vec3};

    /// A bar on two joints, the top one turning a quarter turn about z over a second.
    pub(crate) fn bend() -> (Skeleton, AnimationClip) {
        let skeleton = Skeleton::new(vec![
            Joint::new("base", None),
            Joint::new("top", 0).with_rest(Transform3D::at(vec3(0.0, 1.0, 0.0))),
        ]);
        let mut turn = JointAnimation::new(1, "top");
        let q = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        turn.rotation = Some(AnimationTrack::new(
            vec![0.0, 1.0],
            vec![Vec4::new(0.0, 0.0, 0.0, 1.0), Vec4::from(q)],
            Interpolation::Linear,
        ));
        let clip = AnimationClip::new("bend", &skeleton, vec![turn]);
        (skeleton, clip)
    }

    #[test]
    fn a_character_is_culled_by_the_box_of_the_clips_it_plays() {
        let boxes = [
            (Vec3::ZERO, vec3(1.0, 2.0, 1.0)),
            (vec3(-1.0, 0.0, 0.0), vec3(1.0, 4.0, 1.0)),
        ];
        let record = |clip: u16, blend_clip: u16, blend: f32| {
            CrowdRecord::new(
                &CrowdInstance::new(Mat4::IDENTITY)
                    .playing(clip, 0.0)
                    .blending(blend_clip, 0.0, blend),
            )
            .anim
        };
        assert_eq!(character_bounds(&boxes, record(0, 1, 0.0)), boxes[0], "its clip alone");
        assert_eq!(
            character_bounds(&boxes, record(0, 1, 1.0)),
            boxes[1],
            "the blend clip alone"
        );
        assert_eq!(
            character_bounds(&boxes, record(0, 1, 0.5)),
            (vec3(-1.0, 0.0, 0.0), vec3(1.0, 4.0, 1.0)),
            "both while blending"
        );
    }

    #[test]
    fn a_cull_box_follows_the_transform() {
        // A box 2 x 4 x 2 turned a quarter turn about z, twice the size.
        let m = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            vec3(10.0, 0.0, 0.0),
        );
        let b = CullBox::new(&m, (vec3(-1.0, 0.0, -1.0), vec3(1.0, 4.0, 1.0)));
        assert!(b.center.distance(vec3(6.0, 0.0, 0.0)) < 1e-4, "{}", b.center);
        assert!(
            b.extent().distance(vec3(4.0, 2.0, 2.0)) < 1e-4,
            "lying down: {}",
            b.extent()
        );
        assert!((b.radius() - vec3(4.0, 2.0, 2.0).length()).abs() < 1e-4);
        // Planes facing +x: behind one at x 11, not at x 9.
        assert!(b.behind(Vec4::new(1.0, 0.0, 0.0, -11.0)));
        assert!(!b.behind(Vec4::new(1.0, 0.0, 0.0, -9.0)));
    }

    #[test]
    fn baking_samples_both_ends_of_every_clip() {
        let (skeleton, clip) = bend();
        let idle = AnimationClip::new("idle", &skeleton, vec![]);
        let a = CrowdAnimation::bake(&skeleton, &[clip.into(), CrowdClip::once(idle)], 10.0);
        assert_eq!(a.frames(), 11 + 1);
        assert_eq!((a.0.clips[1].first, a.0.clips[1].frames), (11, 1));
        assert!(!a.0.clips[1].looping && a.0.clips[0].looping);
        // The top joint at the end: turned a quarter, still 1 above the base.
        let end = a.0.matrices[10 * 2 + 1];
        assert!((end.transform_point3(Vec3::X) - vec3(0.0, 2.0, 0.0)).length() < 1e-4);
        assert_eq!(a.0.matrices[1], Mat4::from_translation(Vec3::Y), "starts at rest");
    }

    #[test]
    fn records_pack_clips_times_blend_and_speed() {
        let i = CrowdInstance::new(Mat4::IDENTITY)
            .playing(3, 1.5)
            .blending(2, 0.25, 0.5)
            .with_speed(-1.5);
        let r = CrowdRecord::new(&i);
        assert_eq!(r.anim[0], 3 | 2 << 16);
        assert_eq!(f32::from_bits(r.anim[1]), 1.5);
        assert_eq!(f32::from_bits(r.anim[2]), 0.25);
        assert_eq!(r.anim[3] & 0xffff, 32768);
        assert_eq!((r.anim[3] >> 16) as u16 as i16, -384);
        assert_eq!(size_of::<CrowdRecord>(), 96, "six vec4s, as the shaders read them");
        let held = CrowdRecord::new(&i.with_speed(500.0));
        assert_eq!((held.anim[3] >> 16) as u16 as i16, 127 * 256, "clamped");
    }

    #[test]
    fn a_crowd_bakes_each_level_with_its_own_joints_and_bounds_every_pose() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let (skeleton, clip) = bend();
            let mut bar = MeshData::cube(1.0);
            for p in &mut bar.positions {
                *p = vec3(p.x * 0.2, (p.y + 0.5) * 2.0, p.z * 0.2);
            }
            bar.joints = bar.positions.iter().map(|p| [u16::from(p.y > 1.0), 0, 0, 0]).collect();
            bar.weights = vec![[1.0, 0.0, 0.0, 0.0]; bar.positions.len()];
            let material = ctx.gfx.default_material();
            let near = ctx.gfx.create_skinned_mesh(&bar, material, &skeleton);
            // A far mesh made for a skeleton of the base joint alone.
            let base = Skeleton::new(vec![Joint::new("base", None)]);
            let mut far = bar.clone();
            far.joints = vec![[0; 4]; far.positions.len()];
            let far = ctx.gfx.create_skinned_mesh(&far, material, &base);
            let animation = CrowdAnimation::bake(&skeleton, &[clip.into()], 4.0);
            let levels = [
                CrowdLevel::new(near, 0.0, 10.0),
                CrowdLevel::new(far, 10.0, f32::INFINITY),
            ];
            let crowd = ctx.gfx.create_crowd(
                &animation,
                &levels,
                &[CrowdInstance::new(Mat4::IDENTITY)],
                CrowdOptions::default(),
            );
            let data = ctx.gfx.assets3d.crowds[crowd.id as usize].as_ref().unwrap();
            assert_eq!(data.sections[0].start, 1, "after the clip table");
            assert_eq!(data.sections[0].slots, 2);
            assert_eq!(data.sections[1].slots, 1);
            assert_eq!(data.sections[1].start, 1 + 5 * 2 * 3);
            assert_eq!(data.texels.len(), 1 + 5 * 2 * 3 + 5 * 3);
            assert_eq!(data.texels[0], [0.0, 5.0, 4.0, 1.0]);
            // The top's tip, at (0, 2) at rest, turns to (-1, 1) at the end:
            // both inside the clip's box, which reaches little further
            // (each slot's sphere around its corners, under its matrix).
            let (lo, hi) = data.clip_bounds[0];
            for p in [vec3(-1.0, 1.0, 0.0), vec3(0.0, 2.0, 0.0), Vec3::ZERO] {
                assert!(p.cmpge(lo).all() && p.cmple(hi).all(), "{p} outside {lo} {hi}");
            }
            assert!(lo.x > -1.5 && hi.y < 2.5, "a tight box: {lo} {hi}");
            // The last frame's top slot: rows of a quarter turn about (0, 1).
            let last = data.sections[0].start as usize + (4 * 2 + 1) * 3;
            let row0 = Vec4::from(data.texels[last]);
            assert!((row0 - Vec4::new(0.0, -1.0, 0.0, 1.0)).length() < 1e-4, "{row0}");
        });
    }

    #[test]
    fn fewer_influences_keep_the_strongest_joints_as_a_whole() {
        let (j, w) = top_influences([4, 5, 6, 7], [0.1, 0.5, 0.15, 0.25], 2);
        assert_eq!(j[..2], [5, 7]);
        assert!((w[0] - 0.5 / 0.75).abs() < 1e-6 && (w[1] - 0.25 / 0.75).abs() < 1e-6);
        assert_eq!(w[2..], [0.0, 0.0]);
        let (j, w) = top_influences([1, 2, 3, 4], [0.5, 0.5, 0.0, 0.0], 1);
        assert_eq!((j[0], w[0]), (1, 1.0), "ties keep their order");
        assert_eq!(top_influences([0; 4], [0.0; 4], 4).1, [0.0; 4], "no weights stay none");
    }

    #[test]
    #[should_panic(expected = "the mesh isn't skinned")]
    fn crowd_levels_must_be_skinned() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let (skeleton, clip) = bend();
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let animation = CrowdAnimation::bake(&skeleton, &[clip.into()], 4.0);
            ctx.gfx.create_crowd(
                &animation,
                &[CrowdLevel::new(mesh, 0.0, 1.0)],
                &[],
                CrowdOptions::default(),
            );
        });
    }

    #[test]
    #[should_panic(expected = "past the animation's 1 clips")]
    fn characters_play_only_baked_clips() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let (skeleton, clip) = bend();
            let mut bar = MeshData::cube(1.0);
            bar.joints = vec![[0; 4]; bar.positions.len()];
            bar.weights = vec![[1.0, 0.0, 0.0, 0.0]; bar.positions.len()];
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_skinned_mesh(&bar, material, &skeleton);
            let animation = CrowdAnimation::bake(&skeleton, &[clip.into()], 4.0);
            let levels = [CrowdLevel::new(mesh, 0.0, 1.0)];
            ctx.gfx.create_crowd(
                &animation,
                &levels,
                &[CrowdInstance::new(Mat4::IDENTITY).playing(1, 0.0)],
                CrowdOptions::default(),
            );
        });
    }

    #[test]
    #[should_panic(expected = "fps must be finite and > 0")]
    fn baking_needs_a_positive_rate() {
        let (skeleton, clip) = bend();
        let _ = CrowdAnimation::bake(&skeleton, &[clip.into()], 0.0);
    }

    #[test]
    fn a_freed_mesh_withdraws_its_crowd_and_the_handle_stays_live() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let (skeleton, clip) = bend();
            let mut bar = MeshData::cube(1.0);
            bar.joints = vec![[0; 4]; bar.positions.len()];
            bar.weights = vec![[1.0, 0.0, 0.0, 0.0]; bar.positions.len()];
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_skinned_mesh(&bar, material, &skeleton);
            let animation = CrowdAnimation::bake(&skeleton, &[clip.into()], 4.0);
            let crowd = ctx.gfx.create_crowd(
                &animation,
                &[CrowdLevel::new(mesh, 0.0, 1.0)],
                &[CrowdInstance::new(Mat4::IDENTITY)],
                CrowdOptions::default(),
            );
            let mut sent = Vec::new();
            ctx.gfx.assets3d.validate_crowds(|id, c| sent.push((id, c.is_some())));
            assert!(sent.is_empty(), "live meshes: nothing to do");
            ctx.gfx.free(mesh);
            ctx.gfx.assets3d.validate_crowds(|id, c| sent.push((id, c.is_some())));
            assert_eq!(sent, [(crowd.id, false)]);
            assert!(ctx.is_live(crowd), "the handle stays live");
            ctx.free(crowd);
            assert!(!ctx.is_live(crowd));
            assert!(!ctx.gfx.update_crowd(crowd, &[]), "a freed crowd isn't updated");
        });
    }
}
