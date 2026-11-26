//! Animation tracks and allocation-free steady-state pose sampling.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::{Joint, Material, Skeleton};
use crate::error::{Error, ErrorKind};
use glam::{Quat, Vec4};

/// Interpolation between keys. Cubic values are stored as incoming tangent,
/// value, outgoing tangent for each key; tangents are derivatives per second.
/// ```
/// use ponko::anim::Interpolation;
/// let interpolation = ponko::anim::Interpolation::Linear;
/// assert_ne!(interpolation, ponko::anim::Interpolation::Step);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Interpolation {
    /// Hold the earlier value until the next key.
    Step,
    /// Lerp vectors; slerp rotations along the shortest path.
    Linear,
    /// glTF cubic Hermite interpolation; normalize the resulting quaternion.
    CubicSpline,
}

/// A vector track (translation or scale), or a quaternion track stored as xyzw.
/// Use [`AnimationTrack::new`] to validate times and value counts.
/// ```
/// use ponko::prelude::*;
/// let track = ponko::anim::AnimationTrack::new(vec![0.0], vec![Vec4::ZERO], ponko::anim::Interpolation::Step);
/// assert_eq!(track.times(), &[0.0]);
/// ```
#[derive(Debug, Clone)]
pub struct AnimationTrack {
    times: Vec<f32>,
    values: Vec<Vec4>,
    interpolation: Interpolation,
}
impl AnimationTrack {
    /// Makes a track. Vector values use xyz (w is ignored); rotations use xyzw.
    /// [`try_new`](Self::try_new) returns the failures as an error instead.
    ///
    /// # Panics
    ///
    /// Unless times are finite, nonnegative, strictly increasing and
    /// nonempty, and values are finite with one per key (three for cubic).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let t = ponko::anim::AnimationTrack::new(vec![0.0, 1.0], vec![Vec4::ZERO, Vec4::ONE], ponko::anim::Interpolation::Linear);
    /// assert_eq!(t.values().len(), 2);
    /// ```
    pub fn new(times: Vec<f32>, values: Vec<Vec4>, interpolation: Interpolation) -> Self {
        Self::try_new(times, values, interpolation).unwrap_or_else(|e| panic!("AnimationTrack::new: {}", e.message()))
    }
    /// Makes a track like [`new`](Self::new), or says why the keys are
    /// unusable. For data from files or tools.
    ///
    /// # Errors
    ///
    /// An [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) error
    /// when times are empty, not finite, negative or not strictly
    /// increasing, or when a value is not finite or the value count is not
    /// one per key (three per key for cubic).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationTrack, Interpolation};
    /// let err = AnimationTrack::try_new(vec![1.0, 0.0], vec![Vec4::ZERO; 2], Interpolation::Step).unwrap_err();
    /// assert_eq!(err.kind(), ErrorKind::Invalid);
    /// assert!(AnimationTrack::try_new(vec![0.0], vec![Vec4::ONE], Interpolation::Step).is_ok());
    /// ```
    pub fn try_new(times: Vec<f32>, values: Vec<Vec4>, interpolation: Interpolation) -> Result<Self, Error> {
        if times.is_empty()
            || times.iter().any(|t| !t.is_finite() || *t < 0.0)
            || times.windows(2).any(|t| t[0] >= t[1])
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                "key times must be nonempty, finite, nonnegative and strictly increasing",
            ));
        }
        let count = if interpolation == Interpolation::CubicSpline {
            3
        } else {
            1
        };
        if values.len() != times.len() * count || values.iter().any(|v| !v.is_finite()) {
            return Err(Error::new(
                ErrorKind::Invalid,
                "invalid animation value count or non-finite value",
            ));
        }
        Ok(Self {
            times,
            values,
            interpolation,
        })
    }
    /// Key times in seconds.
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::anim::AnimationTrack::new(vec![0.0], vec![Vec4::ZERO], ponko::anim::Interpolation::Step);
    /// assert_eq!(t.times(), &[0.0]);
    /// ```
    pub fn times(&self) -> &[f32] {
        &self.times
    }
    /// Values, including tangent triples for cubic tracks.
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::anim::AnimationTrack::new(vec![0.0], vec![Vec4::ONE], ponko::anim::Interpolation::Step);
    /// assert_eq!(t.values(), &[Vec4::ONE]);
    /// ```
    pub fn values(&self) -> &[Vec4] {
        &self.values
    }
    /// How keys are interpolated.
    /// ```
    /// # use ponko::prelude::*;
    /// let t = ponko::anim::AnimationTrack::new(vec![0.0], vec![Vec4::ZERO], ponko::anim::Interpolation::Step);
    /// assert_eq!(t.interpolation(), ponko::anim::Interpolation::Step);
    /// ```
    pub fn interpolation(&self) -> Interpolation {
        self.interpolation
    }
    fn value(&self, i: usize) -> Vec4 {
        self.values[if self.interpolation == Interpolation::CubicSpline {
            i * 3 + 1
        } else {
            i
        }]
    }
    fn sample(&self, time: f32, rotation: bool) -> Vec4 {
        let end = self.times.partition_point(|t| *t <= time);
        if end == 0 {
            return self.value(0);
        }
        if end == self.times.len() {
            return self.value(end - 1);
        }
        let i = end - 1;
        let dt = self.times[end] - self.times[i];
        let t = (time - self.times[i]) / dt;
        let a = self.value(i);
        let b = self.value(end);
        match self.interpolation {
            Interpolation::Step => a,
            Interpolation::Linear if rotation => Vec4::from_array(quaternion(a).slerp(quaternion(b), t).to_array()),
            Interpolation::Linear => a.lerp(b, t),
            Interpolation::CubicSpline => {
                let t2 = t * t;
                let t3 = t2 * t;
                let v = a * (2.0 * t3 - 3.0 * t2 + 1.0)
                    + self.values[i * 3 + 2] * (dt * (t3 - 2.0 * t2 + t))
                    + b * (-2.0 * t3 + 3.0 * t2)
                    + self.values[end * 3] * (dt * (t3 - t2));
                if rotation {
                    Vec4::from_array(quaternion(v).to_array())
                } else {
                    v
                }
            }
        }
    }
}
fn quaternion(v: Vec4) -> Quat {
    if v.length_squared() > 1e-20 {
        Quat::from_array(v.normalize().to_array())
    } else {
        Quat::IDENTITY
    }
}

/// Tracks for a joint, identified both by its source index and name.
/// Missing tracks leave the corresponding pose property untouched.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) and the `with_*` methods.
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimationTrack, Interpolation, JointAnimation};
/// let bob = AnimationTrack::new(vec![0.0, 0.5], vec![Vec4::ZERO, Vec4::Y], Interpolation::Linear);
/// let root = JointAnimation::new(0, "root").with_translation(bob);
/// assert_eq!(root.name, "root");
/// assert!(root.rotation.is_none());
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct JointAnimation {
    /// Index in the source skeleton.
    pub joint: usize,
    /// Name used for other skeletons (first matching joint wins).
    pub name: String,
    /// Local translation keys, xyz.
    pub translation: Option<AnimationTrack>,
    /// Local rotation keys, xyzw quaternions.
    pub rotation: Option<AnimationTrack>,
    /// Local scale keys, xyz.
    pub scale: Option<AnimationTrack>,
}
impl JointAnimation {
    /// No tracks yet for joint `joint` (its index in the source skeleton)
    /// called `name`; add them with the `with_*` methods.
    /// ```
    /// let hip = ponko::anim::JointAnimation::new(0, "hip");
    /// assert_eq!((hip.joint, hip.name.as_str()), (0, "hip"));
    /// ```
    #[must_use]
    pub fn new(joint: usize, name: impl Into<String>) -> Self {
        Self {
            joint,
            name: name.into(),
            translation: None,
            rotation: None,
            scale: None,
        }
    }

    with_fields! {
        /// Sets [`translation`](Self::translation), the local position keys
        /// (xyz; `None`: untouched).
        translation: Option<AnimationTrack> => with_translation;
        /// Sets [`rotation`](Self::rotation), the local rotation keys (xyzw
        /// quaternions; `None`: untouched).
        rotation: Option<AnimationTrack> => with_rotation;
        /// Sets [`scale`](Self::scale), the local scale keys (xyz; `None`:
        /// untouched).
        scale: Option<AnimationTrack> => with_scale;
    }
}
/// A named morph channel. Values use the track's x component, in units where
/// 1 is full weight. Cubic tangents use weight units per second.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new).
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimationTrack, Interpolation, MorphAnimation};
/// let channel = MorphAnimation::new("smile", AnimationTrack::new(vec![0.0], vec![Vec4::ONE], Interpolation::Step));
/// assert_eq!(channel.name, "smile");
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MorphAnimation {
    /// Mesh target name; unrelated targets are untouched.
    pub name: String,
    /// Scalar keys in x.
    pub track: AnimationTrack,
}
impl MorphAnimation {
    /// Keys `track` (weights in x) for the morph target called `name`.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationTrack, Interpolation, MorphAnimation};
    /// let blink = MorphAnimation::new("blink", AnimationTrack::new(vec![0.0], vec![Vec4::X], Interpolation::Step));
    /// assert_eq!(blink.track.times(), &[0.0]);
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, track: AnimationTrack) -> Self {
        Self {
            name: name.into(),
            track,
        }
    }
}
/// A value of a material a clip can animate ([`AnimatedProperty::Material`]).
/// Each is read from the track's x, or xy, xyz, xyzw as noted. Colors are
/// linear RGB(A), as glTF's factors are (so they blend in linear light),
/// not the sRGB [`Color`](crate::color::Color) of a
/// [`MaterialDesc`](crate::render::MaterialDesc).
///
/// ```
/// use ponko::anim::MaterialProperty;
/// assert_ne!(MaterialProperty::Color, MaterialProperty::Emissive);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MaterialProperty {
    /// The base color, linear RGBA (xyzw): `color` (glTF's
    /// `baseColorFactor`).
    Color,
    /// The light it gives off, linear RGB (xyz), before
    /// [`EmissiveStrength`](Self::EmissiveStrength): `emissive` (glTF's
    /// `emissiveFactor`).
    Emissive,
    /// Multiplies [`Emissive`](Self::Emissive) (x), glTF's
    /// `KHR_materials_emissive_strength`.
    EmissiveStrength,
    /// `roughness` (x), 0 to 1.
    Roughness,
    /// `metallic` (x), 0 to 1.
    Metallic,
    /// `normal_strength` (x).
    NormalStrength,
    /// `alpha_cutoff` (x).
    AlphaCutoff,
    /// The texture transform's `offset` (xy).
    TextureOffset,
    /// The texture transform's `rotation` (x), radians.
    TextureRotation,
    /// The texture transform's `scale` (xy).
    TextureScale,
    /// One map's texture offset (xy).
    MapTextureOffset(super::TextureMap),
    /// One map's texture rotation (x), radians.
    MapTextureRotation(super::TextureMap),
    /// One map's texture scale (xy).
    MapTextureScale(super::TextureMap),
    /// The clear coat's amount (x), 0 to 1.
    Clearcoat,
    /// The clear coat's roughness (x), 0 to 1.
    ClearcoatRoughness,
    /// The sheen's color, linear RGB (xyz).
    SheenColor,
    /// The sheen's roughness (x), 0 to 1.
    SheenRoughness,
    /// The specular strength (x), 0 to 1.
    Specular,
    /// The specular color, linear RGB (xyz).
    SpecularColor,
    /// The index of refraction (x), at least 1.
    Ior,
    /// The anisotropy's strength (x), -1 to 1.
    Anisotropy,
    /// The anisotropy's rotation (x), radians.
    AnisotropyRotation,
    /// The iridescence's amount (x), 0 to 1.
    Iridescence,
    /// The iridescent film's index of refraction (x), at least 1.
    IridescenceIor,
    /// The iridescent film's thinnest thickness (x), in nanometers.
    IridescenceThinnest,
    /// The iridescent film's thickest thickness (x), in nanometers.
    IridescenceThickest,
}

/// What a [`PropertyAnimation`] changes: something other than a joint's
/// place, which [`JointAnimation`] keys.
///
/// ```
/// use ponko::anim::AnimatedProperty;
/// let blink = AnimatedProperty::JointVisible(3);
/// assert_eq!(blink, AnimatedProperty::JointVisible(3));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnimatedProperty {
    /// Whether joint `.0` of the skeleton shows (x at least 0.5) or is
    /// hidden with everything under it, as glTF's `KHR_node_visibility`
    /// has it: what hangs from it isn't drawn, and lights and models
    /// attached to it with [`AttachToJoint`](crate::anim::AttachToJoint)
    /// go out. See [`Skeleton::joint_visible`].
    JointVisible(usize),
    /// A value of a material. Materials are shared: every model drawn with
    /// the material shows the change, whichever skeleton the clip plays on.
    Material(Material, MaterialProperty),
}

/// Keys for one [`AnimatedProperty`], the value in the track's components
/// that property names. glTF's `KHR_animation_pointer` channels load as
/// these.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new).
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimatedProperty, AnimationTrack, Interpolation, PropertyAnimation};
/// let off = AnimationTrack::new(vec![0.0, 1.0], vec![Vec4::X, Vec4::ZERO], Interpolation::Step);
/// let blink = PropertyAnimation::new(AnimatedProperty::JointVisible(2), off);
/// assert_eq!(blink.target, AnimatedProperty::JointVisible(2));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PropertyAnimation {
    /// What it changes.
    pub target: AnimatedProperty,
    /// The values, in the components `target` reads.
    pub track: AnimationTrack,
}

impl PropertyAnimation {
    /// Keys `track` for `target`.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimatedProperty, AnimationTrack, Interpolation, PropertyAnimation};
    /// let p = PropertyAnimation::new(AnimatedProperty::JointVisible(0), AnimationTrack::new(vec![0.0], vec![Vec4::X], Interpolation::Step));
    /// assert_eq!(p.track.times(), &[0.0]);
    /// ```
    #[must_use]
    pub fn new(target: AnimatedProperty, track: AnimationTrack) -> Self {
        Self { target, track }
    }
}

/// How many live clips animate a material: while none does, drawing a
/// frame doesn't look for animated materials at all.
static MATERIAL_CLIPS: AtomicUsize = AtomicUsize::new(0);

/// Whether a live clip animates a material.
pub(crate) fn any_material_clips() -> bool {
    MATERIAL_CLIPS.load(Ordering::Relaxed) > 0
}

#[derive(Debug)]
struct ClipData {
    name: String,
    duration: f32,
    tracks: Vec<JointAnimation>,
    morphs: Vec<MorphAnimation>,
    properties: Vec<PropertyAnimation>,
    /// Whether `properties` has a material's (counted in `MATERIAL_CLIPS`).
    materials: bool,
    joints: Arc<[Joint]>,
    bindings: Mutex<Vec<Binding>>,
}

impl ClipData {
    /// The last key time of every track.
    fn last_key(&self) -> f32 {
        self.tracks
            .iter()
            .flat_map(|t| [&t.translation, &t.rotation, &t.scale])
            .flatten()
            .chain(self.morphs.iter().map(|m| &m.track))
            .chain(self.properties.iter().map(|p| &p.track))
            .filter_map(|t| t.times.last().copied())
            .fold(0.0, f32::max)
    }

    /// Replaces the property tracks, keeping `MATERIAL_CLIPS` up to date.
    fn set_properties(&mut self, properties: Vec<PropertyAnimation>) {
        let materials = properties
            .iter()
            .any(|p| matches!(p.target, AnimatedProperty::Material(..)));
        match (self.materials, materials) {
            (false, true) => {
                MATERIAL_CLIPS.fetch_add(1, Ordering::Relaxed);
            }
            (true, false) => {
                MATERIAL_CLIPS.fetch_sub(1, Ordering::Relaxed);
            }
            _ => {}
        }
        self.materials = materials;
        self.properties = properties;
    }

    /// A copy with no cached bindings.
    fn copy(&self) -> Self {
        let mut copy = Self {
            name: self.name.clone(),
            duration: self.duration,
            tracks: self.tracks.clone(),
            morphs: self.morphs.clone(),
            properties: Vec::new(),
            materials: false,
            joints: self.joints.clone(),
            bindings: Mutex::new(Vec::new()),
        };
        copy.set_properties(self.properties.clone());
        copy
    }
}

impl Drop for ClipData {
    fn drop(&mut self) {
        self.set_properties(Vec::new());
    }
}
#[derive(Debug, Clone)]
struct Binding {
    joints: Arc<[Joint]>,
    indices: Vec<Option<usize>>,
}

/// Cheaply cloned, shared animation data. Sampling clamps time; for looping
/// use `time.rem_euclid(clip.duration())` when duration is positive.
/// The first sample on each distinct joint list allocates a name mapping;
/// later samples (including skeleton clones) allocate nothing. Source skeletons
/// use indices directly. Missing joints and shortened pose entries are skipped.
/// ```
/// use ponko::prelude::*;
/// let s = ponko::anim::Skeleton::new(vec![]);
/// let clip = ponko::anim::AnimationClip::new("idle", &s, vec![]);
/// assert_eq!(clip.name(), "idle");
/// ```
#[derive(Debug, Clone)]
pub struct AnimationClip(Arc<ClipData>);
impl AnimationClip {
    pub(crate) fn source_joints(&self) -> &Arc<[Joint]> {
        &self.0.joints
    }
    /// Makes a clip from tracks for `source`. Duration is the last key time.
    /// [`try_new`](Self::try_new) returns the failure as an error instead.
    ///
    /// # Panics
    ///
    /// If a track's source index and name disagree with the skeleton.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let s = ponko::anim::Skeleton::new(vec![]);
    /// let clip = ponko::anim::AnimationClip::new("idle", &s, vec![]);
    /// assert_eq!(clip.duration(), 0.0);
    /// ```
    pub fn new(name: impl Into<String>, source: &Skeleton, tracks: Vec<JointAnimation>) -> Self {
        Self::try_new(name, source, tracks).unwrap_or_else(|e| panic!("AnimationClip::new: {}", e.message()))
    }
    /// Makes a clip like [`new`](Self::new). For clips built from files or
    /// tools.
    ///
    /// # Errors
    ///
    /// An [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) error naming the
    /// first track whose joint index is not in `source` or whose name is not
    /// that joint's name.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationClip, JointAnimation, Skeleton};
    /// let s = Skeleton::new(vec![]);
    /// let stray = JointAnimation::new(0, "hip");
    /// let err = AnimationClip::try_new("walk", &s, vec![stray]).unwrap_err();
    /// assert_eq!(err.kind(), ErrorKind::Invalid);
    /// ```
    pub fn try_new(name: impl Into<String>, source: &Skeleton, tracks: Vec<JointAnimation>) -> Result<Self, Error> {
        if let Some(t) = tracks
            .iter()
            .find(|t| source.joints().get(t.joint).is_none_or(|j| j.name != t.name))
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "joint index and name must match source skeleton (track for joint {} \"{}\")",
                    t.joint, t.name
                ),
            ));
        }
        let duration = tracks
            .iter()
            .flat_map(|t| [&t.translation, &t.rotation, &t.scale])
            .flatten()
            .map(|t| *t.times.last().unwrap())
            .fold(0.0, f32::max);
        Ok(Self(Arc::new(ClipData {
            name: name.into(),
            duration,
            tracks,
            morphs: Vec::new(),
            properties: Vec::new(),
            materials: false,
            joints: source.shared_joints().clone(),
            bindings: Mutex::new(Vec::new()),
        })))
    }
    /// Replaces named morph tracks on a clip.
    /// [`try_with_morph_tracks`](Self::try_with_morph_tracks) returns the
    /// failure as an error instead.
    ///
    /// # Panics
    ///
    /// On duplicate track names.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let clip = ponko::anim::AnimationClip::new("face", &ponko::anim::Skeleton::new(vec![]), vec![])
    ///     .with_morph_tracks(vec![]);
    /// assert!(clip.morph_tracks().is_empty());
    /// ```
    pub fn with_morph_tracks(self, tracks: Vec<MorphAnimation>) -> Self {
        self.try_with_morph_tracks(tracks)
            .unwrap_or_else(|e| panic!("AnimationClip::with_morph_tracks: {}", e.message()))
    }
    /// Replaces named morph tracks like
    /// [`with_morph_tracks`](Self::with_morph_tracks).
    ///
    /// # Errors
    ///
    /// An [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) error naming the
    /// first repeated track name (the clip is dropped).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationClip, AnimationTrack, Interpolation, MorphAnimation, Skeleton};
    /// let smile = MorphAnimation::new("smile", AnimationTrack::new(vec![0.0], vec![Vec4::ONE], Interpolation::Step));
    /// let clip = AnimationClip::new("face", &Skeleton::new(vec![]), vec![]);
    /// let err = clip.try_with_morph_tracks(vec![smile.clone(), smile]).unwrap_err();
    /// assert_eq!(err.kind(), ErrorKind::Invalid);
    /// ```
    pub fn try_with_morph_tracks(mut self, tracks: Vec<MorphAnimation>) -> Result<Self, Error> {
        if let Some(t) = tracks
            .iter()
            .enumerate()
            .find_map(|(i, t)| tracks[..i].iter().any(|p| p.name == t.name).then_some(t))
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!("duplicate morph track name \"{}\"", t.name),
            ));
        }
        let data = self.data_mut();
        data.morphs = tracks;
        data.duration = data.last_key();
        Ok(self)
    }

    /// The clip's data, copied first when another clone shares it.
    fn data_mut(&mut self) -> &mut ClipData {
        if Arc::get_mut(&mut self.0).is_none() {
            self.0 = Arc::new(self.0.copy());
        }
        Arc::get_mut(&mut self.0).expect("invariant: the clip's data was just copied, so no other clone shares it")
    }

    /// Replaces the clip's property tracks: values other than joints'
    /// places, such as a joint's visibility or a material's color (glTF's
    /// `KHR_animation_pointer` channels load as these). They are sampled
    /// with the rest of the clip into the [`Skeleton`] it plays on (see
    /// [`Skeleton::property`]); the engine applies them as it draws.
    /// [`try_with_property_tracks`](Self::try_with_property_tracks) returns
    /// the failure as an error instead.
    ///
    /// # Panics
    ///
    /// When two tracks have the same target.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimatedProperty, AnimationClip, AnimationTrack, Interpolation, PropertyAnimation, Skeleton};
    /// let s = Skeleton::new(vec![ponko::anim::Joint::new("lamp", None)]);
    /// let off = AnimationTrack::new(vec![0.0, 1.0], vec![Vec4::X, Vec4::ZERO], Interpolation::Step);
    /// let blink = AnimationClip::new("blink", &s, vec![])
    ///     .with_property_tracks(vec![PropertyAnimation::new(AnimatedProperty::JointVisible(0), off)]);
    /// assert_eq!(blink.duration(), 1.0);
    /// ```
    pub fn with_property_tracks(self, tracks: Vec<PropertyAnimation>) -> Self {
        self.try_with_property_tracks(tracks)
            .unwrap_or_else(|e| panic!("AnimationClip::with_property_tracks: {}", e.message()))
    }

    /// Replaces the property tracks like
    /// [`with_property_tracks`](Self::with_property_tracks).
    ///
    /// # Errors
    ///
    /// An [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) error naming the
    /// first target with two tracks (the clip is dropped).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimatedProperty, AnimationClip, AnimationTrack, Interpolation, PropertyAnimation, Skeleton};
    /// let on = PropertyAnimation::new(AnimatedProperty::JointVisible(0), AnimationTrack::new(vec![0.0], vec![Vec4::X], Interpolation::Step));
    /// let clip = AnimationClip::new("blink", &Skeleton::new(vec![]), vec![]);
    /// let err = clip.try_with_property_tracks(vec![on.clone(), on]).unwrap_err();
    /// assert_eq!(err.kind(), ErrorKind::Invalid);
    /// ```
    pub fn try_with_property_tracks(mut self, tracks: Vec<PropertyAnimation>) -> Result<Self, Error> {
        if let Some(t) = tracks
            .iter()
            .enumerate()
            .find_map(|(i, t)| tracks[..i].iter().any(|p| p.target == t.target).then_some(t))
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!("two property tracks for {:?}", t.target),
            ));
        }
        let data = self.data_mut();
        data.set_properties(tracks);
        data.duration = data.last_key();
        Ok(self)
    }

    /// The property tracks: a joint's visibility, a material's values.
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]).property_tracks().is_empty());
    /// ```
    pub fn property_tracks(&self) -> &[PropertyAnimation] {
        &self.0.properties
    }

    /// Samples the property tracks into `skeleton` at `time`, by `weight`.
    fn sample_properties(&self, time: f32, weight: f32, skeleton: &mut Skeleton) {
        for p in &self.0.properties {
            skeleton
                .morphs
                .set_property(p.target, p.track.sample(time, false), weight);
        }
    }
    /// Named morph tracks, in weight units (1 is full weight).
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]).morph_tracks().is_empty());
    /// ```
    pub fn morph_tracks(&self) -> &[MorphAnimation] {
        &self.0.morphs
    }
    pub(crate) fn sample_morphs(&self, time: f32, weight: f32, skeleton: &mut Skeleton) {
        for t in &self.0.morphs {
            skeleton.morphs.set(&t.name, t.track.sample(time, false).x, weight);
        }
    }
    /// Clip name (unnamed glTF clips use `animation N`, with zero-based N).
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]).name(), "idle");
    /// ```
    pub fn name(&self) -> &str {
        &self.0.name
    }
    /// Length in seconds; zero for an empty clip or a single key at zero.
    /// ```
    /// # use ponko::prelude::*;
    /// assert_eq!(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]).duration(), 0.0);
    /// ```
    pub fn duration(&self) -> f32 {
        self.0.duration
    }
    /// Animated joints and their tracks.
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]).tracks().is_empty());
    /// ```
    pub fn tracks(&self) -> &[JointAnimation] {
        &self.0.tracks
    }
    /// Sets animated properties at clamped `time` (NaN means zero).
    /// ```
    /// # use ponko::prelude::*;
    /// let mut s = ponko::anim::Skeleton::new(vec![]);
    /// ponko::anim::AnimationClip::new("idle", &s, vec![]).sample(0.5, &mut s);
    /// ```
    pub fn sample(&self, time: f32, skeleton: &mut Skeleton) {
        self.sample_blended(time, 1.0, skeleton);
    }
    /// Blends from the current pose by `weight`, clamped to [0, 1] (NaN
    /// means zero). For a crossfade, sample A then blend B by the fade amount.
    /// ```
    /// # use ponko::prelude::*;
    /// let mut s = ponko::anim::Skeleton::new(vec![]);
    /// let clip = ponko::anim::AnimationClip::new("idle", &s, vec![]);
    /// clip.sample_blended(0.5, 0.5, &mut s);
    /// ```
    pub fn sample_blended(&self, time: f32, weight: f32, skeleton: &mut Skeleton) {
        let weight = if weight.is_nan() { 0.0 } else { weight.clamp(0.0, 1.0) };
        if weight == 0.0 {
            return;
        }
        let time = if time.is_nan() {
            0.0
        } else {
            time.clamp(0.0, self.duration())
        };
        self.sample_morphs(time, weight, skeleton);
        self.sample_properties(time, weight, skeleton);
        if skeleton.shares_joints(&self.0.joints) {
            self.apply(time, weight, skeleton, None);
        } else {
            let mut bindings = self.0.bindings.lock().unwrap();
            let at = bindings
                .iter()
                .position(|b| skeleton.shares_joints(&b.joints))
                .unwrap_or_else(|| {
                    let indices = self.0.tracks.iter().map(|t| skeleton.find(&t.name)).collect();
                    bindings.push(Binding {
                        joints: skeleton.shared_joints().clone(),
                        indices,
                    });
                    bindings.len() - 1
                });
            self.apply(time, weight, skeleton, Some(&bindings[at].indices));
        }
    }
    /// One joint's local transform at clamped `time`, starting from its rest
    /// pose (root motion). Tracks bind by index on the clip's own skeleton,
    /// otherwise by joint name.
    pub(crate) fn sample_joint(&self, time: f32, skeleton: &Skeleton, joint: usize) -> crate::d3::Transform {
        let Some(j) = skeleton.joints().get(joint) else {
            return crate::d3::Transform::default();
        };
        let mut pose = j.rest;
        let time = if time.is_nan() {
            0.0
        } else {
            time.clamp(0.0, self.duration())
        };
        let own = skeleton.shares_joints(&self.0.joints);
        for track in &self.0.tracks {
            if !(if own {
                track.joint == joint
            } else {
                *track.name == *j.name
            }) {
                continue;
            }
            if let Some(t) = &track.translation {
                pose.position = t.sample(time, false).truncate();
            }
            if let Some(t) = &track.rotation {
                pose.rotation = quaternion(t.sample(time, true));
            }
        }
        pose
    }
    fn apply(&self, time: f32, weight: f32, skeleton: &mut Skeleton, mapping: Option<&[Option<usize>]>) {
        for (i, track) in self.0.tracks.iter().enumerate() {
            let Some(joint) = mapping.map_or(Some(track.joint), |m| m[i]) else {
                continue;
            };
            let Some(pose) = skeleton.pose.get_mut(joint) else {
                continue;
            };
            if let Some(t) = &track.translation {
                pose.position = pose.position.lerp(t.sample(time, false).truncate(), weight);
            }
            if let Some(t) = &track.scale {
                pose.scale = pose.scale.lerp(t.sample(time, false).truncate(), weight);
            }
            if let Some(t) = &track.rotation {
                pose.rotation = pose.rotation.slerp(quaternion(t.sample(time, true)), weight);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::Transform as Transform3D;
    use glam::Vec3;
    fn skeleton(names: &[&str]) -> Skeleton {
        Skeleton::new(names.iter().map(|n| Joint::new(*n, None)).collect())
    }
    fn track(interpolation: Interpolation, values: Vec<Vec4>) -> AnimationTrack {
        AnimationTrack::new(vec![0.0, 2.0], values, interpolation)
    }
    fn clip(s: &Skeleton) -> AnimationClip {
        AnimationClip::new(
            "move",
            s,
            vec![
                JointAnimation::new(0, "root")
                    .with_translation(track(Interpolation::Linear, vec![Vec4::ZERO, Vec4::splat(4.0)])),
            ],
        )
    }
    #[test]
    fn step_holds_and_linear_clamps() {
        let step = track(Interpolation::Step, vec![Vec4::ZERO, Vec4::ONE]);
        assert_eq!(step.sample(1.99, false), Vec4::ZERO);
        assert_eq!(step.sample(2.0, false), Vec4::ONE);
        let mut s = skeleton(&["root"]);
        let c = clip(&s);
        c.sample(-1.0, &mut s);
        assert_eq!(s.pose[0].position, Vec3::ZERO);
        c.sample(1.0, &mut s);
        assert_eq!(s.pose[0].position, Vec3::splat(2.0));
        c.sample(5.0, &mut s);
        assert_eq!(s.pose[0].position, Vec3::splat(4.0));
    }
    #[test]
    fn cubic_tangents_are_scaled_by_key_interval() {
        let t = track(
            Interpolation::CubicSpline,
            vec![
                Vec4::ZERO,
                Vec4::ZERO,
                Vec4::splat(4.0),
                Vec4::ZERO,
                Vec4::ZERO,
                Vec4::ZERO,
            ],
        );
        assert_eq!(t.sample(1.0, false), Vec4::ONE);
        let q = track(
            Interpolation::CubicSpline,
            vec![Vec4::ZERO, Vec4::W, Vec4::ZERO, Vec4::ZERO, Vec4::Z, Vec4::ZERO],
        );
        assert!((q.sample(1.0, true).length() - 1.0).abs() < 1e-6);
    }
    #[test]
    fn rotations_take_shortest_path() {
        let q = Quat::from_rotation_y(0.4);
        let t = track(
            Interpolation::Linear,
            vec![Vec4::from_array(q.to_array()), -Vec4::from_array(q.to_array())],
        );
        assert!(quaternion(t.sample(1.0, true)).abs_diff_eq(q, 1e-6));
    }
    #[test]
    fn blending_and_name_binding_preserve_other_properties() {
        let source = skeleton(&["root"]);
        let c = clip(&source);
        let mut target = skeleton(&["other", "root"]);
        for weight in [0.0, 0.5, 1.0] {
            target.reset_pose();
            c.sample_blended(2.0, weight, &mut target);
            assert_eq!(target.pose[0], Transform3D::default());
            assert_eq!(target.pose[1].position, Vec3::splat(4.0 * weight));
            assert_eq!(target.pose[1].scale, Vec3::ONE);
        }
        assert_eq!(c.0.bindings.lock().unwrap().len(), 1);
        c.clone().sample(1.0, &mut target.clone());
        assert_eq!(c.0.bindings.lock().unwrap().len(), 1);
    }
    #[test]
    fn blended_scale_and_rotation_use_current_pose() {
        let mut s = skeleton(&["root"]);
        let q = Quat::from_rotation_z(1.0);
        let c = AnimationClip::new(
            "turn",
            &s,
            vec![
                JointAnimation::new(0, "root")
                    .with_rotation(AnimationTrack::new(
                        vec![0.0],
                        vec![Vec4::from_array(q.to_array())],
                        Interpolation::Linear,
                    ))
                    .with_scale(AnimationTrack::new(
                        vec![0.0],
                        vec![Vec4::splat(3.0)],
                        Interpolation::Step,
                    )),
            ],
        );
        for w in [0.0, 0.5, 1.0] {
            s.reset_pose();
            c.sample_blended(0.0, w, &mut s);
            assert!(s.pose[0].rotation.abs_diff_eq(Quat::from_rotation_z(w), 1e-6));
            assert_eq!(s.pose[0].scale, Vec3::splat(1.0 + 2.0 * w));
        }
        let mut missing = skeleton(&["other"]);
        c.sample(0.0, &mut missing);
        assert_eq!(missing.pose[0], Transform3D::default());
    }

    fn visible(joint: usize, values: Vec<Vec4>) -> PropertyAnimation {
        PropertyAnimation::new(
            AnimatedProperty::JointVisible(joint),
            AnimationTrack::new(vec![0.0, 1.0], values, Interpolation::Step),
        )
    }

    #[test]
    fn property_tracks_sample_into_the_skeleton_and_blend_by_weight() {
        let mut s = skeleton(&["root", "lamp"]);
        let blink = clip(&s).with_property_tracks(vec![visible(1, vec![Vec4::X, Vec4::ZERO])]);
        assert_eq!(blink.duration(), 2.0, "the joint track is longer");
        assert_eq!(blink.property_tracks().len(), 1);
        let target = AnimatedProperty::JointVisible(1);
        assert_eq!(s.property(target), None, "nothing keyed yet");
        blink.sample(0.5, &mut s);
        assert_eq!(s.property(target), Some(Vec4::X));
        assert!(s.joint_visible(1));
        s.reset_pose();
        blink.sample(1.5, &mut s);
        assert_eq!(s.property(target), Some(Vec4::ZERO));
        assert!(!s.joint_visible(1) && s.joint_visible(0));
        // Half of a hidden key over a shown rest: the value is the key, the
        // rest makes up the other half, and 0.5 still shows.
        s.reset_pose();
        blink.sample_blended(1.5, 0.5, &mut s);
        assert_eq!(s.property(target), Some(Vec4::ZERO));
        assert!(s.joint_visible(1));
        s.reset_pose();
        blink.sample_blended(1.5, 0.75, &mut s);
        assert!(!s.joint_visible(1));
        // Reset forgets it.
        s.reset_pose();
        assert_eq!(s.property(target), None);
        assert!(s.joint_visible(1));
    }

    #[test]
    fn property_tracks_alone_set_the_duration() {
        let s = skeleton(&["lamp"]);
        let blink = AnimationClip::new("blink", &s, vec![]).with_property_tracks(vec![visible(0, vec![Vec4::X; 2])]);
        assert_eq!(blink.duration(), 1.0);
    }

    #[test]
    fn two_tracks_for_one_property_are_refused() {
        let s = skeleton(&["lamp"]);
        let err = AnimationClip::new("blink", &s, vec![])
            .try_with_property_tracks(vec![visible(0, vec![Vec4::X; 2]), visible(0, vec![Vec4::ZERO; 2])])
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(err.message().contains("JointVisible(0)"), "{}", err.message());
    }

    #[test]
    fn replacing_tracks_on_a_shared_clip_leaves_the_other_clone_alone() {
        let s = skeleton(&["lamp"]);
        let color = PropertyAnimation::new(
            AnimatedProperty::Material(Material { id: 3 }, MaterialProperty::Color),
            AnimationTrack::new(vec![0.0], vec![Vec4::ONE], Interpolation::Step),
        );
        let tinted = AnimationClip::new("tint", &s, vec![]).with_property_tracks(vec![color]);
        assert!(tinted.0.materials && any_material_clips());
        let plain = tinted.clone().with_property_tracks(vec![visible(0, vec![Vec4::X; 2])]);
        assert!(!plain.0.materials);
        assert_eq!(tinted.property_tracks().len(), 1);
        assert!(matches!(
            tinted.property_tracks()[0].target,
            AnimatedProperty::Material(..)
        ));
        // The copy has its own name binding cache, still empty.
        let mut other = skeleton(&["other", "lamp"]);
        plain.sample(0.0, &mut other);
        assert!(tinted.0.bindings.lock().unwrap().is_empty());
    }

    #[test]
    #[should_panic(expected = "key times must")]
    fn unordered_times_are_rejected() {
        AnimationTrack::new(vec![1.0, 0.0], vec![Vec4::ZERO; 2], Interpolation::Step);
    }
    #[test]
    #[should_panic(expected = "joint index and name")]
    fn mismatched_source_joint_is_rejected() {
        AnimationClip::new("bad", &skeleton(&["root"]), vec![JointAnimation::new(0, "missing")]);
    }
}
