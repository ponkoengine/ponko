//! The skeleton file format (`"format": "ponko-skeleton2d"`, version 1):
//! reading it, and writing a [`SkeletonData`] back out
//! ([`SkeletonData::to_json`]). The guide's "2D skeletons" chapter
//! documents it field by field. The same structures read and write it, so
//! everything the reader takes the writer gives.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::clip::{
    Bezier, BoneChannel, BoneTrack, ChannelTrack, Clip, Curve, DeformTrack, IkTrack, Key, PathTrack, PhysicsTrack,
    SlotTrack, TransformTrack,
};
use super::data::{
    Attachment, BonePose, ClippingMask, Constraint, IkConstraint, MeshVertex, SkeletonData, SkeletonError, SkinnedMesh,
    SpritePart, TransformConstraint, err,
};
use super::graph::{SkeletonGraph, SkeletonMotion, SkeletonState};
use super::path::{PathConstraint, PathPosition, PathRotate, PathShape, PathSpacing};
use super::physics::PhysicsConstraint;
use crate::anim::{AnimationCondition, AnimationEvent, AnimationParameter, AnimationTransition, StateId};
use crate::camera::Rect;
use crate::color::Color;
use crate::texture::Texture;

/// The `format` field of a skeleton file.
pub(crate) const FORMAT: &str = "ponko-skeleton2d";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileJson {
    format: String,
    version: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    premultiplied: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    nearest: Vec<String>,
    bones: Vec<BoneJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    slots: Vec<SlotJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skins: Vec<SkinJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ik: Vec<IkJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transform: Vec<TransformJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path: Vec<PathJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    physics: Vec<PhysicsJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    clips: Vec<ClipJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graph: Option<GraphJson>,
}

fn one() -> f32 {
    1.0
}
fn ones() -> [f32; 2] {
    [1.0, 1.0]
}
fn yes() -> bool {
    true
}
fn hundred() -> f32 {
    100.0
}
fn default_limit() -> f32 {
    5000.0
}
fn sixtieth() -> f32 {
    1.0 / 60.0
}
fn default_skin() -> String {
    "default".into()
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(v: &bool) -> bool {
    *v
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero64(v: &f64) -> bool {
    *v == 0.0
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(v: &f32) -> bool {
    *v == 1.0
}
fn is_ones(v: &[f32; 2]) -> bool {
    *v == [1.0, 1.0]
}
fn is_zeros(v: &[f32; 2]) -> bool {
    *v == [0.0, 0.0]
}
fn is_zeros64(v: &[f64; 2]) -> bool {
    *v == [0.0, 0.0]
}
fn is_default_skin(v: &str) -> bool {
    v == "default"
}

/// Degrees in the file, radians in the engine. Kept as f64 in the file so
/// radians written and read back come out bit for bit the same.
fn radians(degrees: f64) -> f32 {
    degrees.to_radians() as f32
}

fn degrees(radians: f32) -> f64 {
    f64::from(radians).to_degrees()
}

fn degrees2(v: Vec2) -> [f64; 2] {
    [degrees(v.x), degrees(v.y)]
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BoneJson {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    y: f32,
    /// Degrees, clockwise on screen.
    #[serde(default, skip_serializing_if = "is_zero64")]
    rotation: f64,
    #[serde(default = "ones", skip_serializing_if = "is_ones")]
    scale: [f32; 2],
    /// Degrees, of the x and y axes.
    #[serde(default, skip_serializing_if = "is_zeros64")]
    shear: [f64; 2],
    #[serde(default, skip_serializing_if = "is_zero")]
    length: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SlotJson {
    name: String,
    bone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attachment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<ColorJson>,
}

/// `"#rrggbbaa"`, or `[r, g, b, a]` (0..1 each) for a color hex can't hold.
#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum ColorJson {
    Hex(String),
    Rgba([f32; 4]),
}

impl ColorJson {
    fn color(&self) -> Result<Color, SkeletonError> {
        match self {
            ColorJson::Hex(h) => hex_color(h),
            ColorJson::Rgba([r, g, b, a]) => Ok(Color::rgba(*r, *g, *b, *a)),
        }
    }

    fn of(c: Color) -> Self {
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
        let hex = format!("#{:02x}{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b), byte(c.a));
        if hex_color(&hex).is_ok_and(|h| h == c) {
            ColorJson::Hex(hex)
        } else {
            ColorJson::Rgba([c.r, c.g, c.b, c.a])
        }
    }

    /// `None` for white, the default.
    fn unless_white(c: Color) -> Option<Self> {
        (c != Color::WHITE).then(|| Self::of(c))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SkinJson {
    name: String,
    attachments: Vec<AttachmentJson>,
}

#[derive(Deserialize, Serialize, Default, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Kind {
    #[default]
    Sprite,
    Mesh,
    Clipping,
    Path,
}

impl Kind {
    fn is_sprite(&self) -> bool {
        *self == Kind::Sprite
    }
}

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
struct AttachmentJson {
    slot: String,
    name: String,
    #[serde(default, rename = "type", skip_serializing_if = "Kind::is_sprite")]
    kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<ColorJson>,
    // A sprite's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "is_false")]
    rotated: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    y: f32,
    #[serde(default, skip_serializing_if = "is_zero64")]
    rotation: f64,
    #[serde(default = "ones", skip_serializing_if = "is_ones")]
    scale: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pivot: Option<[f32; 2]>,
    // A mesh's, a clipping mask's and a path's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertices: Vec<VertexJson>,
    // A mesh's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    triangles: Vec<u16>,
    // A clipping mask's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<String>,
    // A path's.
    #[serde(default, skip_serializing_if = "is_false")]
    closed: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    constant_speed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lengths: Vec<f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VertexJson {
    position: [f32; 2],
    /// Meshes only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uv: Option<[f32; 2]>,
    bones: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    weights: Vec<f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IkJson {
    name: String,
    bones: Vec<String>,
    target: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    bend_positive: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mix: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<i64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransformJson {
    name: String,
    bones: Vec<String>,
    target: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mix_rotate: f32,
    #[serde(default = "ones", skip_serializing_if = "is_ones")]
    mix_translate: [f32; 2],
    #[serde(default = "ones", skip_serializing_if = "is_ones")]
    mix_scale: [f32; 2],
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mix_shear_y: f32,
    /// Degrees.
    #[serde(default, skip_serializing_if = "is_zero64")]
    rotation: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    y: f32,
    #[serde(default, skip_serializing_if = "is_zeros")]
    scale: [f32; 2],
    /// Degrees.
    #[serde(default, skip_serializing_if = "is_zero64")]
    shear_y: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    local: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    relative: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<i64>,
}

#[derive(Deserialize, Serialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PositionJson {
    Fixed,
    #[default]
    Percent,
}

#[derive(Deserialize, Serialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SpacingJson {
    #[default]
    Length,
    Fixed,
    Percent,
    Proportional,
}

#[derive(Deserialize, Serialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RotateJson {
    #[default]
    Tangent,
    Chain,
    ChainScale,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PathJson {
    name: String,
    bones: Vec<String>,
    /// A slot.
    target: String,
    #[serde(default, skip_serializing_if = "is_default")]
    position_mode: PositionJson,
    #[serde(default, skip_serializing_if = "is_default")]
    spacing_mode: SpacingJson,
    #[serde(default, skip_serializing_if = "is_default")]
    rotate_mode: RotateJson,
    /// Degrees.
    #[serde(default, skip_serializing_if = "is_zero64")]
    rotation: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    position: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    spacing: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mix_rotate: f32,
    #[serde(default = "ones", skip_serializing_if = "is_ones")]
    mix_translate: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<i64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PhysicsJson {
    name: String,
    bone: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    y: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    rotate: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    scale_x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    shear_x: f32,
    #[serde(default = "default_limit")]
    limit: f32,
    #[serde(default = "sixtieth")]
    step: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    inertia: f32,
    #[serde(default = "hundred")]
    strength: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    damping: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mass: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    wind: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    gravity: f32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    mix: f32,
    #[serde(default = "hundred")]
    reference_scale: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<i64>,
}

/// `"linear"`, `"stepped"`, `{ "bezier": [x1, y1, x2, y2] }` or
/// `{ "bezier": [[x1, y1, x2, y2], ...] }` (one per channel).
#[derive(Deserialize, Serialize, Default, Clone, PartialEq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum CurveJson {
    #[default]
    Linear,
    Stepped,
    Bezier(BezierJson),
}

#[derive(Deserialize, Serialize, Clone, PartialEq)]
#[serde(untagged)]
enum BezierJson {
    One([f32; 4]),
    PerChannel(Vec<[f32; 4]>),
}

impl CurveJson {
    fn to_curve(&self) -> Result<Curve, SkeletonError> {
        let bezier = |[x1, y1, x2, y2]: [f32; 4]| Bezier::new(x1, y1, x2, y2);
        Ok(match self {
            CurveJson::Linear => Curve::Linear,
            CurveJson::Stepped => Curve::Stepped,
            CurveJson::Bezier(BezierJson::One(b)) => Curve::Bezier(bezier(*b)),
            CurveJson::Bezier(BezierJson::PerChannel(list)) => {
                if list.is_empty() || list.len() > 4 {
                    return Err(err("a bezier per channel needs one to four [x1, y1, x2, y2]"));
                }
                let mut channels = [bezier(list[0]); 4];
                for (c, b) in list.iter().enumerate() {
                    channels[c] = bezier(*b);
                }
                Curve::Channels(channels)
            }
        })
    }

    fn of(curve: &Curve) -> Self {
        let b = |b: &Bezier| [b.x1, b.y1, b.x2, b.y2];
        match curve {
            Curve::Stepped => CurveJson::Stepped,
            Curve::Bezier(x) => CurveJson::Bezier(BezierJson::One(b(x))),
            Curve::Channels(cs) => CurveJson::Bezier(BezierJson::PerChannel(cs.iter().map(b).collect())),
            _ => CurveJson::Linear,
        }
    }

    fn is_linear(&self) -> bool {
        *self == CurveJson::Linear
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeyJson<T> {
    #[serde(default)]
    time: f32,
    value: T,
    #[serde(default, skip_serializing_if = "CurveJson::is_linear")]
    curve: CurveJson,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClipJson {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bones: Vec<BoneTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    slots: Vec<SlotTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deform: Vec<DeformJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ik: Vec<IkTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transform: Vec<TransformTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path: Vec<PathTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    physics: Vec<PhysicsTrackJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    draw_order: Vec<DrawOrderJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    events: Vec<EventJson>,
}

type Keys<T> = Vec<KeyJson<T>>;

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
struct BoneTrackJson {
    bone: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    translate: Keys<[f32; 2]>,
    /// Degrees.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rotate: Keys<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scale: Keys<[f32; 2]>,
    /// Degrees.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shear: Keys<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    translate_x: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    translate_y: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scale_x: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scale_y: Keys<f32>,
    /// Degrees.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shear_x: Keys<f64>,
    /// Degrees.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shear_y: Keys<f64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SlotTrackJson {
    slot: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachment: Keys<Option<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    visible: Keys<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    color: Keys<ColorJson>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeformJson {
    slot: String,
    #[serde(default = "default_skin", skip_serializing_if = "is_default_skin")]
    skin: String,
    attachment: String,
    /// Each key: one `[x, y]` offset per vertex and bone, in the order the
    /// attachment lists its vertices and their bones.
    keys: Keys<Vec<[f32; 2]>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IkTrackJson {
    constraint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bend_positive: Keys<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransformTrackJson {
    constraint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_rotate: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_translate: Keys<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_scale: Keys<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_shear_y: Keys<f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PathTrackJson {
    constraint: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    position: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    spacing: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_rotate: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix_translate: Keys<[f32; 2]>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PhysicsTrackJson {
    /// None: every physics constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    constraint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    inertia: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    strength: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    damping: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mass: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wind: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    gravity: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mix: Keys<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reset: Vec<f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DrawOrderJson {
    #[serde(default)]
    time: f32,
    order: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventJson {
    /// Seconds; f64 so a time written and read back is the same.
    time: f64,
    name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphJson {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parameters: Vec<ParameterJson>,
    states: Vec<StateJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transitions: Vec<TransitionJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParameterJson {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    float: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bool: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trigger: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateJson {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blend1d: Option<Blend1DJson>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    speed: f32,
    #[serde(default = "yes", rename = "loop", skip_serializing_if = "is_true")]
    looping: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Blend1DJson {
    parameter: String,
    clips: Vec<(f32, String)>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransitionJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    when: Vec<ConditionJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_time: Option<f32>,
    #[serde(default, skip_serializing_if = "is_zero")]
    duration: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    offset: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum ConditionJson {
    Greater(String, f32),
    Less(String, f32),
    Bool(String, bool),
    Trigger(String),
}

/// Whether `value` is a Spine JSON export (its `skeleton.spine` version).
pub(crate) fn is_spine(value: &serde_json::Value) -> bool {
    value.get("skeleton").and_then(|s| s.get("spine")).is_some()
}

/// `#rrggbb` or `#rrggbbaa` (the `#` optional) as a color.
pub(crate) fn hex_color(text: &str) -> Result<Color, SkeletonError> {
    let digits = text.trim_start_matches('#');
    let value = u32::from_str_radix(digits, 16).ok();
    match (digits.len(), value) {
        (6, Some(v)) => Ok(Color::hex(v)),
        (8, Some(v)) => Ok(Color::hex(v >> 8).with_alpha((v & 0xff) as f32 / 255.0)),
        _ => Err(err(format!("\"{text}\" isn't a color (#rrggbb or #rrggbbaa)"))),
    }
}

/// `image` relative to the folder of the file at `path`.
pub(crate) fn beside(path: &str, image: &str) -> String {
    match path.rfind('/') {
        Some(i) => format!("{}/{image}", &path[..i]),
        None => image.to_string(),
    }
}

/// `image` (an asset name) relative to the folder of the file at `path`:
/// the inverse of [`beside`]. `None` if it isn't in that folder.
fn relative<'a>(path: &str, image: &'a str) -> Option<&'a str> {
    match path.rfind('/') {
        Some(i) => image.strip_prefix(&path[..=i]),
        None => Some(image),
    }
}

/// A file's `name` field lookups with a helpful error.
fn index(found: Option<usize>, what: &str, name: &str) -> Result<usize, SkeletonError> {
    found.ok_or_else(|| err(format!("no {what} named \"{name}\"")))
}

impl SkeletonData {
    /// Reads a skeleton file: Ponko's format (`"format":
    /// "ponko-skeleton2d"`, see the guide's "2D skeletons") or a Spine
    /// JSON export (see [`from_spine_json`](Self::from_spine_json)).
    /// `path` is the file's asset name: images are found next to it. Image
    /// textures aren't loaded; [`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d)
    /// loads the file and its images.
    ///
    /// ```
    /// use ponko::skeleton2d::SkeletonData;
    /// let data = SkeletonData::from_json(
    ///     r#"{ "format": "ponko-skeleton2d", "version": 1,
    ///          "bones": [{ "name": "root" }, { "name": "arm", "parent": "root", "x": 10, "length": 20 }],
    ///          "clips": [{ "name": "wave", "bones": [{ "bone": "arm", "rotate": [
    ///              { "time": 0, "value": 0 }, { "time": 0.5, "value": 45 } ] }] }] }"#,
    ///     "hero/hero.json",
    /// )
    /// .unwrap();
    /// assert_eq!(data.bones().len(), 2);
    /// assert_eq!(data.clips()[0].duration, 0.5); // the last key
    /// ```
    ///
    /// # Errors
    /// If it isn't JSON, isn't either format, or names something that
    /// isn't there; the message says what.
    pub fn from_json(text: &str, path: &str) -> Result<Self, SkeletonError> {
        Self::from_json_with(text, path, None)
    }

    /// [`from_json`](Self::from_json), a Spine export taking its images
    /// from `atlas` when there is one.
    pub(crate) fn from_json_with(
        text: &str,
        path: &str,
        atlas: Option<&super::SpineAtlas>,
    ) -> Result<Self, SkeletonError> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| err(e.to_string()))?;
        if value.get("format").and_then(|f| f.as_str()) == Some(FORMAT) {
            let file: FileJson = serde_json::from_value(value).map_err(|e| err(e.to_string()))?;
            return from_file(file, path);
        }
        if is_spine(&value) {
            return Self::from_spine_value(&value, path, atlas);
        }
        Err(err(format!(
            "not a skeleton file: it needs \"format\": \"{FORMAT}\" (or to be a Spine JSON export)"
        )))
    }

    /// Writes it as a skeleton file in Ponko's own format, to be saved as
    /// asset `path` (its images are written relative to that file's
    /// folder). Everything a skeleton holds is written: bones with shear,
    /// slots, skins with sprites, meshes, clipping masks and paths, every
    /// constraint in its order, clips with all their tracks and curves,
    /// the state machine, and an atlas's premultiplied and nearest images.
    /// Reading the text back with [`from_json`](Self::from_json) at the
    /// same `path` gives the same skeleton, numbers bit for bit, so an
    /// imported Spine export can be kept in Ponko's format and lose
    /// nothing.
    ///
    /// ```
    /// use ponko::skeleton2d::SkeletonData;
    /// let spine = r#"{ "skeleton": { "spine": "4.2.0" },
    ///     "bones": [{ "name": "root" }, { "name": "arm", "parent": "root", "rotation": 30, "shearY": 10 }] }"#;
    /// let data = SkeletonData::from_spine_json(spine, "hero/hero.json").unwrap();
    /// let ours = data.to_json("hero/hero.json").unwrap();
    /// assert!(ours.contains("\"ponko-skeleton2d\""));
    /// let back = SkeletonData::from_json(&ours, "hero/hero.json").unwrap();
    /// assert_eq!(back.bones(), data.bones());
    /// ```
    ///
    /// # Errors
    /// If an attachment shows an image made in code (it has no file) or
    /// one outside `path`'s folder, or a bone track keys one axis twice.
    pub fn to_json(&self, path: &str) -> Result<String, SkeletonError> {
        let file = to_file(self, path)?;
        serde_json::to_string_pretty(&file).map_err(|e| err(e.to_string()))
    }
}

/// Per attachment (skin, slot, name): where each `[x, y]` of its deform
/// keys goes, in the file's order of its vertices and their bones: the
/// offset index (`4 * vertex + k`), or `None` for a bone the vertex
/// dropped (weight 0, or past the strongest four).
type Influences = HashMap<(usize, usize, String), Vec<Option<usize>>>;

fn from_file(file: FileJson, path: &str) -> Result<SkeletonData, SkeletonError> {
    if file.format != FORMAT || file.version != 1 {
        return Err(err(format!(
            "this build reads {FORMAT} version 1, not version {}",
            file.version
        )));
    }
    let mut data = SkeletonData::new();
    for b in &file.bones {
        if data.bone(&b.name).is_some() {
            return Err(err(format!("two bones are named \"{}\"", b.name)));
        }
        let parent = match &b.parent {
            Some(p) => Some(index(data.bone(p), "bone (parents come first)", p)?),
            None => None,
        };
        let rest = BonePose::new(Vec2::new(b.x, b.y), radians(b.rotation), Vec2::from(b.scale))
            .with_shear(Vec2::new(radians(b.shear[0]), radians(b.shear[1])));
        if !(rest.translation.is_finite() && rest.rotation.is_finite() && rest.scale.is_finite() && rest.shear.is_finite())
        {
            return Err(err(format!("bone \"{}\" needs numbers", b.name)));
        }
        data.add_bone(&b.name, parent, rest, b.length);
    }
    for s in &file.slots {
        if data.slot(&s.name).is_some() {
            return Err(err(format!("two slots are named \"{}\"", s.name)));
        }
        let bone = index(data.bone(&s.bone), "bone", &s.bone)?;
        let slot = data.add_slot(&s.name, bone);
        let color = s.color.as_ref().map(ColorJson::color).transpose()?.unwrap_or(Color::WHITE);
        data.set_slot_rest(slot, s.attachment.as_deref(), color);
    }
    let mut influences = Influences::new();
    for skin in &file.skins {
        let index = if skin.name == "default" {
            0
        } else if data.skin(&skin.name).is_some() {
            return Err(err(format!("two skins are named \"{}\"", skin.name)));
        } else {
            data.add_skin(&skin.name)
        };
        for a in &skin.attachments {
            let slot = self::index(data.slot(&a.slot), "slot", &a.slot)?;
            if data.lookup.contains_key(&(index, slot, Arc::from(a.name.as_str()))) {
                return Err(err(format!(
                    "slot \"{}\" has two attachments named \"{}\"",
                    a.slot, a.name
                )));
            }
            let color = a.color.as_ref().map(ColorJson::color).transpose()?.unwrap_or(Color::WHITE);
            let image = || -> Result<Arc<str>, SkeletonError> {
                let image = a
                    .image
                    .as_deref()
                    .ok_or_else(|| err(format!("attachment \"{}\" needs an \"image\"", a.name)))?;
                Ok(beside(path, image).into())
            };
            let attachment: Attachment = match a.kind {
                Kind::Sprite => sprite_part(a, image()?, color)?.into(),
                Kind::Mesh => {
                    let (vertices, map) = vertices(&data, a, true)?;
                    influences.insert((index, slot, a.name.clone()), map);
                    mesh(a, vertices, image()?, color)?.into()
                }
                Kind::Clipping => {
                    let (vertices, map) = vertices(&data, a, false)?;
                    influences.insert((index, slot, a.name.clone()), map);
                    let end = match &a.end {
                        Some(e) => Some(self::index(data.slot(e), "slot", e)?),
                        None => None,
                    };
                    ClippingMask::new(vertices, end).into()
                }
                Kind::Path => {
                    let (vertices, map) = vertices(&data, a, false)?;
                    influences.insert((index, slot, a.name.clone()), map);
                    let mut shape = PathShape::new(vertices);
                    shape.closed = a.closed;
                    shape.constant_speed = a.constant_speed;
                    shape.lengths.clone_from(&a.lengths);
                    shape.check().map_err(|e| err(format!("path \"{}\": {e}", a.name)))?;
                    shape.into()
                }
            };
            data.add_skin_attachment(index, slot, &a.name, attachment);
        }
    }
    // What slots show at rest is what the file says, nothing without it.
    for (i, s) in file.slots.iter().enumerate() {
        if let Some(name) = &s.attachment
            && data.find(0, i, name).is_none()
        {
            return Err(err(format!(
                "slot \"{}\" shows \"{name}\" at rest, which isn't an attachment of the default skin",
                s.name
            )));
        }
        data.slots[i].attachment = s.attachment.as_deref().map(Arc::from);
    }
    constraints(&file, &mut data)?;
    for c in &file.clips {
        let clip = clip(&data, &influences, c)?;
        if data.clip(&clip.name).is_some() {
            return Err(err(format!("two clips are named \"{}\"", clip.name)));
        }
        data.check_clip(&clip)?;
        data.clips.push(clip);
    }
    if let Some(g) = &file.graph {
        let graph = graph(&data, g)?;
        data.set_graph(graph)?;
    }
    data.premultiplied = file.premultiplied;
    data.nearest = file.nearest.iter().map(|n| beside(path, n).into()).collect();
    Ok(data)
}

/// The file's IK, transform, path and physics constraints, in their
/// `order` (a stable sort: equal orders keep that kind order, each kind in
/// file order).
fn constraints(file: &FileJson, data: &mut SkeletonData) -> Result<(), SkeletonError> {
    let mut all: Vec<(i64, u8, usize)> = Vec::new();
    all.extend(file.ik.iter().enumerate().map(|(i, c)| (c.order.unwrap_or(0), 0, i)));
    all.extend(file.transform.iter().enumerate().map(|(i, c)| (c.order.unwrap_or(0), 1, i)));
    all.extend(file.path.iter().enumerate().map(|(i, c)| (c.order.unwrap_or(0), 2, i)));
    all.extend(file.physics.iter().enumerate().map(|(i, c)| (c.order.unwrap_or(0), 3, i)));
    all.sort_by_key(|(order, _, _)| *order);
    let bones = |data: &SkeletonData, names: &[String]| {
        names
            .iter()
            .map(|b| index(data.bone(b), "bone", b))
            .collect::<Result<Vec<_>, _>>()
    };
    for (_, kind, i) in all {
        match kind {
            0 => {
                let ik = &file.ik[i];
                let constraint = IkConstraint {
                    name: ik.name.as_str().into(),
                    bones: bones(data, &ik.bones)?,
                    target: index(data.bone(&ik.target), "bone", &ik.target)?,
                    bend_positive: ik.bend_positive,
                    mix: ik.mix,
                };
                data.check_ik(&constraint)?;
                data.push_ik(constraint);
            }
            1 => {
                let t = &file.transform[i];
                let mut c = TransformConstraint::new(&t.name, &bones(data, &t.bones)?, index(data.bone(&t.target), "bone", &t.target)?);
                c.mix_rotate = t.mix_rotate;
                c.mix_translate = Vec2::from(t.mix_translate);
                c.mix_scale = Vec2::from(t.mix_scale);
                c.mix_shear_y = t.mix_shear_y;
                c.offset_rotation = radians(t.rotation);
                c.offset_translation = Vec2::new(t.x, t.y);
                c.offset_scale = Vec2::from(t.scale);
                c.offset_shear_y = radians(t.shear_y);
                c.local = t.local;
                c.relative = t.relative;
                data.check_transform(&c)?;
                data.push_transform(c);
            }
            2 => {
                let p = &file.path[i];
                let mut c = PathConstraint::new(&p.name, &bones(data, &p.bones)?, index(data.slot(&p.target), "slot", &p.target)?);
                c.position_mode = match p.position_mode {
                    PositionJson::Fixed => PathPosition::Fixed,
                    PositionJson::Percent => PathPosition::Percent,
                };
                c.spacing_mode = match p.spacing_mode {
                    SpacingJson::Length => PathSpacing::Length,
                    SpacingJson::Fixed => PathSpacing::Fixed,
                    SpacingJson::Percent => PathSpacing::Percent,
                    SpacingJson::Proportional => PathSpacing::Proportional,
                };
                c.rotate_mode = match p.rotate_mode {
                    RotateJson::Tangent => PathRotate::Tangent,
                    RotateJson::Chain => PathRotate::Chain,
                    RotateJson::ChainScale => PathRotate::ChainScale,
                };
                c.offset_rotation = radians(p.rotation);
                c.position = p.position;
                c.spacing = p.spacing;
                c.mix_rotate = p.mix_rotate;
                c.mix_translate = Vec2::from(p.mix_translate);
                data.check_path(&c)?;
                data.push_path(c);
            }
            _ => {
                let p = &file.physics[i];
                let mut c = PhysicsConstraint::new(&p.name, index(data.bone(&p.bone), "bone", &p.bone)?);
                c.x = p.x;
                c.y = p.y;
                c.rotate = p.rotate;
                c.scale_x = p.scale_x;
                c.shear_x = p.shear_x;
                c.limit = p.limit;
                c.step = p.step;
                c.inertia = p.inertia;
                c.strength = p.strength;
                c.damping = p.damping;
                c.mass = p.mass;
                c.wind = p.wind;
                c.gravity = p.gravity;
                c.mix = p.mix;
                c.reference_scale = p.reference_scale;
                data.check_physics(&c)?;
                data.push_physics(c);
            }
        }
    }
    Ok(())
}

fn sprite_part(a: &AttachmentJson, image: Arc<str>, color: Color) -> Result<SpritePart, SkeletonError> {
    let region = a.region.map(|[x, y, w, h]| Rect::new(x, y, w, h));
    let packed = |r: Rect| {
        if a.rotated {
            Vec2::new(r.size().y, r.size().x)
        } else {
            r.size()
        }
    };
    let size = a.size.map(Vec2::from).or(region.map(packed)).ok_or_else(|| {
        err(format!(
            "sprite attachment \"{}\" needs a \"size\" or a \"region\"",
            a.name
        ))
    })?;
    let offset = BonePose::new(Vec2::new(a.x, a.y), radians(a.rotation), Vec2::from(a.scale));
    Ok(SpritePart {
        image,
        texture: Texture::new(u32::MAX, 0, 0),
        region,
        rotated: a.rotated && region.is_some(),
        offset,
        size,
        pivot: a.pivot.map_or(Vec2::splat(0.5), Vec2::from),
        color,
    })
}

/// A mesh's, clipping mask's or path's vertices (with a `uv` each when
/// `uvs`), and where each listed bone's deform offset goes.
fn vertices(
    data: &SkeletonData,
    a: &AttachmentJson,
    uvs: bool,
) -> Result<(Vec<MeshVertex>, Vec<Option<usize>>), SkeletonError> {
    let mut out = Vec::with_capacity(a.vertices.len());
    let mut map = Vec::new();
    for (i, v) in a.vertices.iter().enumerate() {
        let bones = v
            .bones
            .iter()
            .map(|b| index(data.bone(b), "bone", b))
            .collect::<Result<Vec<_>, _>>()?;
        let weights = if v.weights.is_empty() && bones.len() == 1 {
            vec![1.0]
        } else {
            v.weights.clone()
        };
        if bones.is_empty()
            || weights.len() != bones.len()
            || !weights.iter().all(|w| w.is_finite())
            || !weights.iter().any(|w| *w > 0.0)
        {
            return Err(err(format!(
                "\"{}\": every vertex needs bones with as many weights, one above 0",
                a.name
            )));
        }
        let uv = match (uvs, v.uv) {
            (true, Some(uv)) => Vec2::from(uv),
            (true, None) => return Err(err(format!("mesh \"{}\": every vertex needs a \"uv\"", a.name))),
            (false, Some(_)) => return Err(err(format!("\"{}\": only mesh vertices have a \"uv\"", a.name))),
            (false, None) => Vec2::ZERO,
        };
        let influences: Vec<(usize, f32)> = bones.into_iter().zip(weights).collect();
        let mut vertex = MeshVertex::new(Vec2::from(v.position), uv, &influences);
        // Where `MeshVertex::new` put each bone: the positive ones,
        // strongest first (a stable sort), the first four.
        let mut order: Vec<usize> = (0..influences.len()).filter(|&j| influences[j].1 > 0.0).collect();
        order.sort_by(|&x, &y| influences[y].1.total_cmp(&influences[x].1));
        order.truncate(4);
        let first = map.len();
        map.resize(first + influences.len(), None);
        // Weights that already add up to 1 (a file this engine wrote) are
        // kept bit for bit rather than divided by their sum again.
        let given: Vec<f32> = order.iter().map(|&j| influences[j].1).collect();
        if given.iter().zip(&vertex.weights).all(|(g, w)| (g - w).abs() <= 1e-6) {
            for (k, g) in given.iter().enumerate() {
                vertex.weights[k] = *g;
            }
        }
        for (k, j) in order.into_iter().enumerate() {
            map[first + j] = Some(4 * i + k);
        }
        out.push(vertex);
    }
    Ok((out, map))
}

fn mesh(a: &AttachmentJson, vertices: Vec<MeshVertex>, image: Arc<str>, color: Color) -> Result<SkinnedMesh, SkeletonError> {
    if !a.triangles.len().is_multiple_of(3) || a.triangles.iter().any(|&i| usize::from(i) >= vertices.len()) {
        return Err(err(format!(
            "mesh \"{}\"'s triangles are three indices each, into its vertices",
            a.name
        )));
    }
    let mut mesh = SkinnedMesh::new(Texture::new(u32::MAX, 0, 0), vertices, a.triangles.clone());
    mesh.image = image;
    mesh.color = color;
    Ok(mesh)
}

fn keys<T, U>(keys: &[KeyJson<T>], f: impl Fn(&T) -> Result<U, SkeletonError>) -> Result<Vec<Key<U>>, SkeletonError> {
    keys.iter()
        .map(|k| {
            Ok(Key {
                time: k.time,
                value: f(&k.value)?,
                curve: k.curve.to_curve()?,
            })
        })
        .collect()
}

fn same<T: Copy>(keys_of: &[KeyJson<T>]) -> Result<Vec<Key<T>>, SkeletonError> {
    keys(keys_of, |v| Ok(*v))
}

fn pairs(keys_of: &[KeyJson<[f32; 2]>]) -> Result<Vec<Key<Vec2>>, SkeletonError> {
    keys(keys_of, |v| Ok(Vec2::from(*v)))
}

/// The last key time of `keys`, or 0.
fn last<T>(keys: &[Key<T>]) -> f32 {
    keys.iter().map(|k| k.time).fold(0.0, f32::max)
}

fn clip(data: &SkeletonData, influences: &Influences, c: &ClipJson) -> Result<Clip, SkeletonError> {
    let mut end = 0.0f32;
    let mut clip = Clip::new(&c.name, 0.0);
    for t in &c.bones {
        let mut track = BoneTrack::new(index(data.bone(&t.bone), "bone", &t.bone)?);
        track.translate = pairs(&t.translate)?;
        track.rotate = keys(&t.rotate, |v| Ok(radians(*v)))?;
        track.scale = pairs(&t.scale)?;
        track.shear = keys(&t.shear, |v| Ok(Vec2::new(radians(v[0]), radians(v[1]))))?;
        let axes = [
            (BoneChannel::TranslateX, same(&t.translate_x)?),
            (BoneChannel::TranslateY, same(&t.translate_y)?),
            (BoneChannel::ScaleX, same(&t.scale_x)?),
            (BoneChannel::ScaleY, same(&t.scale_y)?),
            (BoneChannel::ShearX, keys(&t.shear_x, |v| Ok(radians(*v)))?),
            (BoneChannel::ShearY, keys(&t.shear_y, |v| Ok(radians(*v)))?),
        ];
        for (channel, list) in axes {
            if !list.is_empty() {
                track.channels.push(ChannelTrack::new(channel, list));
            }
        }
        end = end
            .max(last(&track.translate))
            .max(last(&track.rotate))
            .max(last(&track.scale))
            .max(last(&track.shear));
        for ch in &track.channels {
            end = end.max(last(&ch.keys));
        }
        clip.bones.push(track);
    }
    for t in &c.slots {
        let mut track = SlotTrack::new(index(data.slot(&t.slot), "slot", &t.slot)?);
        track.attachment = keys(&t.attachment, |v| Ok(v.as_deref().map(Arc::from)))?;
        track.visible = same(&t.visible)?;
        track.color = keys(&t.color, ColorJson::color)?;
        end = end
            .max(last(&track.attachment))
            .max(last(&track.visible))
            .max(last(&track.color));
        clip.slots.push(track);
    }
    for d in &c.deform {
        let slot = index(data.slot(&d.slot), "slot", &d.slot)?;
        let skin = index(data.skin(&d.skin), "skin", &d.skin)?;
        let place = format!("clip \"{}\", deform of \"{}\"", c.name, d.attachment);
        let (Some(map), Some(a)) = (
            influences.get(&(skin, slot, d.attachment.clone())),
            data.lookup.get(&(skin, slot, Arc::from(d.attachment.as_str()))),
        ) else {
            return Err(err(format!(
                "{place}: slot \"{}\" of skin \"{}\" has no mesh, clipping mask or path by that name",
                d.slot, d.skin
            )));
        };
        let binds = data.attachments[*a].binds.len();
        let mut track = DeformTrack::new(slot, skin, &d.attachment);
        for k in &d.keys {
            if k.value.len() != map.len() {
                return Err(err(format!(
                    "{place}: every key needs {} offsets, one per vertex and bone",
                    map.len()
                )));
            }
            let mut offsets = vec![Vec2::ZERO; binds];
            for (o, to) in k.value.iter().zip(map) {
                if let Some(to) = to {
                    offsets[*to] = Vec2::from(*o);
                }
            }
            track.keys.push(Key::new(k.time, Arc::from(offsets)).with_curve(k.curve.to_curve()?));
        }
        end = end.max(last(&track.keys));
        clip.deform.push(track);
    }
    for t in &c.ik {
        let mut track = IkTrack::new(index(data.ik_constraint(&t.constraint), "IK constraint", &t.constraint)?);
        track.mix = same(&t.mix)?;
        track.bend_positive = same(&t.bend_positive)?;
        end = end.max(last(&track.mix)).max(last(&track.bend_positive));
        clip.ik.push(track);
    }
    for t in &c.transform {
        let what = "transform constraint";
        let mut track = TransformTrack::new(index(data.transform_constraint(&t.constraint), what, &t.constraint)?);
        track.mix_rotate = same(&t.mix_rotate)?;
        track.mix_translate = pairs(&t.mix_translate)?;
        track.mix_scale = pairs(&t.mix_scale)?;
        track.mix_shear_y = same(&t.mix_shear_y)?;
        end = end
            .max(last(&track.mix_rotate))
            .max(last(&track.mix_translate))
            .max(last(&track.mix_scale))
            .max(last(&track.mix_shear_y));
        clip.transforms.push(track);
    }
    for t in &c.path {
        let mut track = PathTrack::new(index(data.path_constraint(&t.constraint), "path constraint", &t.constraint)?);
        track.position = same(&t.position)?;
        track.spacing = same(&t.spacing)?;
        track.mix_rotate = same(&t.mix_rotate)?;
        track.mix_translate = pairs(&t.mix_translate)?;
        end = end
            .max(last(&track.position))
            .max(last(&track.spacing))
            .max(last(&track.mix_rotate))
            .max(last(&track.mix_translate));
        clip.paths.push(track);
    }
    for t in &c.physics {
        let constraint = match &t.constraint {
            Some(n) => Some(index(data.physics_constraint(n), "physics constraint", n)?),
            None => None,
        };
        let mut track = PhysicsTrack::new(constraint);
        track.inertia = same(&t.inertia)?;
        track.strength = same(&t.strength)?;
        track.damping = same(&t.damping)?;
        track.mass = same(&t.mass)?;
        track.wind = same(&t.wind)?;
        track.gravity = same(&t.gravity)?;
        track.mix = same(&t.mix)?;
        track.reset.clone_from(&t.reset);
        for list in [
            &track.inertia,
            &track.strength,
            &track.damping,
            &track.mass,
            &track.wind,
            &track.gravity,
            &track.mix,
        ] {
            end = end.max(last(list));
        }
        end = track.reset.iter().copied().fold(end, f32::max);
        clip.physics.push(track);
    }
    for k in &c.draw_order {
        let order = match &k.order {
            Some(names) => Some(
                names
                    .iter()
                    .map(|n| index(data.slot(n), "slot", n).map(|s| s as u16))
                    .collect::<Result<Arc<[u16]>, _>>()?,
            ),
            None => None,
        };
        end = end.max(k.time);
        clip.draw_order.push(Key::new(k.time, order).with_curve(Curve::Stepped));
    }
    for e in &c.events {
        end = end.max(e.time as f32);
    }
    clip.duration = c.duration.unwrap_or(end);
    // Seconds to the clip's normalized clock, in f64 so a time written and
    // read back is the same.
    let duration = f64::from(clip.duration);
    for e in &c.events {
        let time = if duration > 0.0 {
            ((e.time / duration) as f32).clamp(0.0, 0.999_999)
        } else {
            0.0
        };
        let at = clip.events.partition_point(|x| x.time <= time);
        clip.events.insert(at, AnimationEvent::new(time, e.name.as_str()));
    }
    Ok(clip)
}

fn graph(data: &SkeletonData, g: &GraphJson) -> Result<Arc<SkeletonGraph>, SkeletonError> {
    let mut parameters = Vec::new();
    for p in &g.parameters {
        let value = match (p.float, p.bool, p.trigger) {
            (Some(v), None, None) => AnimationParameter::Float(v),
            (None, Some(v), None) => AnimationParameter::Bool(v),
            (None, None, Some(v)) => AnimationParameter::Trigger(v),
            _ => {
                return Err(err(format!(
                    "parameter \"{}\" needs exactly one of \"float\", \"bool\" or \"trigger\"",
                    p.name
                )));
            }
        };
        parameters.push((p.name.as_str(), value));
    }
    let param = |name: &str| {
        parameters
            .iter()
            .position(|(n, _)| *n == name)
            .map(crate::anim::ParamId)
            .ok_or_else(|| err(format!("no parameter named \"{name}\"")))
    };
    let clip = |name: &str| index(data.clip(name), "clip", name);
    let mut states = Vec::new();
    for s in &g.states {
        let motion = match (&s.clip, &s.blend1d) {
            (Some(c), None) => SkeletonMotion::Clip(clip(c)?),
            (None, Some(b)) => SkeletonMotion::Blend1D {
                parameter: param(&b.parameter)?,
                children: b
                    .clips
                    .iter()
                    .map(|(t, c)| Ok((*t, clip(c)?)))
                    .collect::<Result<_, SkeletonError>>()?,
            },
            _ => {
                return Err(err(format!(
                    "state \"{}\" needs exactly one of \"clip\" or \"blend1d\"",
                    s.name
                )));
            }
        };
        let mut state = SkeletonState::new(&s.name, motion).with_speed(s.speed);
        state.looping = s.looping;
        states.push(state);
    }
    let state = |name: &str| {
        states
            .iter()
            .position(|s: &SkeletonState| &*s.name == name)
            .map(StateId)
            .ok_or_else(|| err(format!("no state named \"{name}\"")))
    };
    let mut transitions = Vec::new();
    for t in &g.transitions {
        let mut transition = match &t.from {
            Some(from) => AnimationTransition::new(state(from)?, state(&t.to)?),
            None => AnimationTransition::from_any(state(&t.to)?),
        }
        .with_exit_time(t.exit_time)
        .with_duration(t.duration)
        .with_offset(t.offset);
        for c in &t.when {
            transition = transition.with_condition(match c {
                ConditionJson::Greater(p, v) => AnimationCondition::Greater(param(p)?, *v),
                ConditionJson::Less(p, v) => AnimationCondition::Less(param(p)?, *v),
                ConditionJson::Bool(p, v) => AnimationCondition::Bool(param(p)?, *v),
                ConditionJson::Trigger(p) => AnimationCondition::Trigger(param(p)?),
            });
        }
        transitions.push(transition);
    }
    let initial = match &g.initial {
        Some(name) => state(name)?,
        None => StateId(0),
    };
    SkeletonGraph::new(parameters, states, transitions, initial).map_err(|e| err(format!("graph: {e}")))
}

// ---- writing ---------------------------------------------------------------------

fn key_json<T, U>(keys: &[Key<T>], f: impl Fn(&T) -> U) -> Keys<U> {
    keys.iter()
        .map(|k| KeyJson {
            time: k.time,
            value: f(&k.value),
            curve: CurveJson::of(&k.curve),
        })
        .collect()
}

fn pair_json(keys: &[Key<Vec2>]) -> Keys<[f32; 2]> {
    key_json(keys, |v| v.to_array())
}

fn to_file(data: &SkeletonData, path: &str) -> Result<FileJson, SkeletonError> {
    let bone = |i: usize| data.bones[i].name.to_string();
    let slot = |i: usize| data.slots[i].name.to_string();
    let image = |image: &str, what: &str| -> Result<String, SkeletonError> {
        if image.is_empty() {
            return Err(err(format!(
                "{what} shows an image made in code, which a file can't name"
            )));
        }
        relative(path, image).map(String::from).ok_or_else(|| {
            err(format!(
                "{what} shows \"{image}\", which isn't in the folder of \"{path}\""
            ))
        })
    };
    let bones = data
        .bones
        .iter()
        .map(|b| BoneJson {
            name: b.name.to_string(),
            parent: b.parent.map(bone),
            x: b.rest.translation.x,
            y: b.rest.translation.y,
            rotation: degrees(b.rest.rotation),
            scale: b.rest.scale.to_array(),
            shear: degrees2(b.rest.shear),
            length: b.length,
        })
        .collect();
    let slots = data
        .slots
        .iter()
        .map(|s| SlotJson {
            name: s.name.to_string(),
            bone: bone(s.bone),
            attachment: s.attachment.as_deref().map(String::from),
            color: ColorJson::unless_white(s.color),
        })
        .collect();
    let mut skins: Vec<SkinJson> = data
        .skins
        .iter()
        .map(|s| SkinJson {
            name: s.to_string(),
            attachments: Vec::new(),
        })
        .collect();
    for named in &data.attachments {
        let what = format!("attachment \"{}\" in slot \"{}\"", named.name, slot(named.slot));
        let vertex_json = |v: &MeshVertex, uv: bool| {
            let used: Vec<usize> = (0..4).filter(|&k| v.weights[k] > 0.0).collect();
            VertexJson {
                position: v.position.to_array(),
                uv: uv.then(|| v.uv.to_array()),
                bones: used.iter().map(|&k| bone(usize::from(v.bones[k]))).collect(),
                weights: if used.len() == 1 && v.weights[used[0]] == 1.0 {
                    Vec::new()
                } else {
                    used.iter().map(|&k| v.weights[k]).collect()
                },
            }
        };
        let mut a = AttachmentJson {
            slot: slot(named.slot),
            name: named.name.to_string(),
            constant_speed: true,
            scale: [1.0, 1.0],
            ..Default::default()
        };
        match &named.attachment {
            Attachment::Sprite(p) => {
                a.image = Some(image(&p.image, &what)?);
                a.color = ColorJson::unless_white(p.color);
                a.region = p.region.map(|r| [r.min.x, r.min.y, r.size().x, r.size().y]);
                a.rotated = p.rotated;
                a.x = p.offset.translation.x;
                a.y = p.offset.translation.y;
                a.rotation = degrees(p.offset.rotation);
                a.scale = p.offset.scale.to_array();
                a.size = Some(p.size.to_array());
                a.pivot = (p.pivot != Vec2::splat(0.5)).then(|| p.pivot.to_array());
            }
            Attachment::Mesh(m) => {
                a.kind = Kind::Mesh;
                a.image = Some(image(&m.image, &what)?);
                a.color = ColorJson::unless_white(m.color);
                a.vertices = m.vertices.iter().map(|v| vertex_json(v, true)).collect();
                a.triangles.clone_from(&m.triangles);
            }
            Attachment::Clipping(c) => {
                a.kind = Kind::Clipping;
                a.vertices = c.vertices.iter().map(|v| vertex_json(v, false)).collect();
                a.end = c.end.map(slot);
            }
            Attachment::Path(p) => {
                a.kind = Kind::Path;
                a.vertices = p.vertices.iter().map(|v| vertex_json(v, false)).collect();
                a.closed = p.closed;
                a.constant_speed = p.constant_speed;
                a.lengths.clone_from(&p.lengths);
            }
        }
        skins[named.skin].attachments.push(a);
    }
    skins.retain(|s| !s.attachments.is_empty() || s.name != "default");
    // Constraint orders, written only when they aren't the default (IK,
    // transform, path, physics, each in its own order).
    let mut kind_order = data.constraints.clone();
    kind_order.sort_by_key(|c| match c {
        Constraint::Ik(i) => (0, *i),
        Constraint::Transform(i) => (1, *i),
        Constraint::Path(i) => (2, *i),
        Constraint::Physics(i) => (3, *i),
    });
    let order_of = |c: Constraint| {
        (kind_order != data.constraints).then(|| data.constraints.iter().position(|x| *x == c).unwrap_or(0) as i64)
    };
    let ik = data
        .ik
        .iter()
        .enumerate()
        .map(|(i, c)| IkJson {
            name: c.name.to_string(),
            bones: c.bones.iter().map(|&b| bone(b)).collect(),
            target: bone(c.target),
            bend_positive: c.bend_positive,
            mix: c.mix,
            order: order_of(Constraint::Ik(i)),
        })
        .collect();
    let transform = data
        .transforms
        .iter()
        .enumerate()
        .map(|(i, c)| TransformJson {
            name: c.name.to_string(),
            bones: c.bones.iter().map(|&b| bone(b)).collect(),
            target: bone(c.target),
            mix_rotate: c.mix_rotate,
            mix_translate: c.mix_translate.to_array(),
            mix_scale: c.mix_scale.to_array(),
            mix_shear_y: c.mix_shear_y,
            rotation: degrees(c.offset_rotation),
            x: c.offset_translation.x,
            y: c.offset_translation.y,
            scale: c.offset_scale.to_array(),
            shear_y: degrees(c.offset_shear_y),
            local: c.local,
            relative: c.relative,
            order: order_of(Constraint::Transform(i)),
        })
        .collect();
    let paths = data
        .paths
        .iter()
        .enumerate()
        .map(|(i, c)| PathJson {
            name: c.name.to_string(),
            bones: c.bones.iter().map(|&b| bone(b)).collect(),
            target: slot(c.target),
            position_mode: match c.position_mode {
                PathPosition::Fixed => PositionJson::Fixed,
                _ => PositionJson::Percent,
            },
            spacing_mode: match c.spacing_mode {
                PathSpacing::Fixed => SpacingJson::Fixed,
                PathSpacing::Percent => SpacingJson::Percent,
                PathSpacing::Proportional => SpacingJson::Proportional,
                _ => SpacingJson::Length,
            },
            rotate_mode: match c.rotate_mode {
                PathRotate::Chain => RotateJson::Chain,
                PathRotate::ChainScale => RotateJson::ChainScale,
                _ => RotateJson::Tangent,
            },
            rotation: degrees(c.offset_rotation),
            position: c.position,
            spacing: c.spacing,
            mix_rotate: c.mix_rotate,
            mix_translate: c.mix_translate.to_array(),
            order: order_of(Constraint::Path(i)),
        })
        .collect();
    let physics = data
        .physics
        .iter()
        .enumerate()
        .map(|(i, c)| PhysicsJson {
            name: c.name.to_string(),
            bone: bone(c.bone),
            x: c.x,
            y: c.y,
            rotate: c.rotate,
            scale_x: c.scale_x,
            shear_x: c.shear_x,
            limit: c.limit,
            step: c.step,
            inertia: c.inertia,
            strength: c.strength,
            damping: c.damping,
            mass: c.mass,
            wind: c.wind,
            gravity: c.gravity,
            mix: c.mix,
            reference_scale: c.reference_scale,
            order: order_of(Constraint::Physics(i)),
        })
        .collect();
    let clips = data
        .clips
        .iter()
        .map(|c| clip_json(data, c))
        .collect::<Result<_, _>>()?;
    let nearest = data
        .nearest
        .iter()
        .map(|n| image(n, "a nearest-filtered image"))
        .collect::<Result<_, _>>()?;
    Ok(FileJson {
        format: FORMAT.into(),
        version: 1,
        premultiplied: data.premultiplied,
        nearest,
        bones,
        slots,
        skins,
        ik,
        transform,
        path: paths,
        physics,
        clips,
        graph: data.graph.as_deref().map(|g| graph_json(data, g)),
    })
}

fn clip_json(data: &SkeletonData, c: &Clip) -> Result<ClipJson, SkeletonError> {
    let mut bones = Vec::new();
    for t in &c.bones {
        let mut b = BoneTrackJson {
            bone: data.bones[t.bone].name.to_string(),
            translate: pair_json(&t.translate),
            rotate: key_json(&t.rotate, |v| degrees(*v)),
            scale: pair_json(&t.scale),
            shear: key_json(&t.shear, |v| degrees2(*v)),
            ..Default::default()
        };
        for ch in &t.channels {
            let (list, angle) = match ch.channel {
                BoneChannel::TranslateX => (&mut b.translate_x, None),
                BoneChannel::TranslateY => (&mut b.translate_y, None),
                BoneChannel::ScaleX => (&mut b.scale_x, None),
                BoneChannel::ScaleY => (&mut b.scale_y, None),
                BoneChannel::ShearX => (&mut Vec::new(), Some(&mut b.shear_x)),
                BoneChannel::ShearY => (&mut Vec::new(), Some(&mut b.shear_y)),
            };
            let taken = match angle {
                Some(angle) => {
                    let empty = angle.is_empty();
                    *angle = key_json(&ch.keys, |v| degrees(*v));
                    !empty
                }
                None => {
                    let empty = list.is_empty();
                    *list = key_json(&ch.keys, |v| *v);
                    !empty
                }
            };
            if taken {
                return Err(err(format!(
                    "clip \"{}\" keys one axis of bone \"{}\" in two channel tracks; a file holds one",
                    c.name, data.bones[t.bone].name
                )));
            }
        }
        bones.push(b);
    }
    let slots = c
        .slots
        .iter()
        .map(|t| SlotTrackJson {
            slot: data.slots[t.slot].name.to_string(),
            attachment: key_json(&t.attachment, |v| v.as_deref().map(String::from)),
            visible: key_json(&t.visible, |v| *v),
            color: key_json(&t.color, |v| ColorJson::of(*v)),
        })
        .collect();
    let deform = c
        .deform
        .iter()
        .map(|t| {
            let vertices = data
                .find(t.skin, t.slot, &t.attachment)
                .and_then(|a| data.attachments[a].attachment.weighted())
                .unwrap_or(&[]);
            DeformJson {
                slot: data.slots[t.slot].name.to_string(),
                skin: data.skins[t.skin].to_string(),
                attachment: t.attachment.to_string(),
                keys: key_json(&t.keys, |offsets| {
                    let mut out = Vec::new();
                    for (v, vertex) in vertices.iter().enumerate() {
                        for k in 0..4 {
                            if vertex.weights[k] > 0.0 {
                                out.push(offsets[4 * v + k].to_array());
                            }
                        }
                    }
                    out
                }),
            }
        })
        .collect();
    let ik = c
        .ik
        .iter()
        .map(|t| IkTrackJson {
            constraint: data.ik[t.constraint].name.to_string(),
            mix: key_json(&t.mix, |v| *v),
            bend_positive: key_json(&t.bend_positive, |v| *v),
        })
        .collect();
    let transform = c
        .transforms
        .iter()
        .map(|t| TransformTrackJson {
            constraint: data.transforms[t.constraint].name.to_string(),
            mix_rotate: key_json(&t.mix_rotate, |v| *v),
            mix_translate: pair_json(&t.mix_translate),
            mix_scale: pair_json(&t.mix_scale),
            mix_shear_y: key_json(&t.mix_shear_y, |v| *v),
        })
        .collect();
    let path = c
        .paths
        .iter()
        .map(|t| PathTrackJson {
            constraint: data.paths[t.constraint].name.to_string(),
            position: key_json(&t.position, |v| *v),
            spacing: key_json(&t.spacing, |v| *v),
            mix_rotate: key_json(&t.mix_rotate, |v| *v),
            mix_translate: pair_json(&t.mix_translate),
        })
        .collect();
    let physics = c
        .physics
        .iter()
        .map(|t| PhysicsTrackJson {
            constraint: t.constraint.map(|i| data.physics[i].name.to_string()),
            inertia: key_json(&t.inertia, |v| *v),
            strength: key_json(&t.strength, |v| *v),
            damping: key_json(&t.damping, |v| *v),
            mass: key_json(&t.mass, |v| *v),
            wind: key_json(&t.wind, |v| *v),
            gravity: key_json(&t.gravity, |v| *v),
            mix: key_json(&t.mix, |v| *v),
            reset: t.reset.clone(),
        })
        .collect();
    let draw_order = c
        .draw_order
        .iter()
        .map(|k| DrawOrderJson {
            time: k.time,
            order: k
                .value
                .as_ref()
                .map(|o| o.iter().map(|&s| data.slots[usize::from(s)].name.to_string()).collect()),
        })
        .collect();
    let events = c
        .events
        .iter()
        .map(|e| EventJson {
            time: f64::from(e.time) * f64::from(c.duration),
            name: e.name.to_string(),
        })
        .collect();
    Ok(ClipJson {
        name: c.name.to_string(),
        duration: Some(c.duration),
        bones,
        slots,
        deform,
        ik,
        transform,
        path,
        physics,
        draw_order,
        events,
    })
}

fn graph_json(data: &SkeletonData, g: &SkeletonGraph) -> GraphJson {
    let param = |p: crate::anim::ParamId| g.names[p.0].to_string();
    let state = |s: StateId| g.states[s.0].name.to_string();
    let clip = |c: usize| data.clips[c].name.to_string();
    GraphJson {
        parameters: g
            .names
            .iter()
            .zip(&g.parameters)
            .map(|(name, p)| {
                let mut json = ParameterJson {
                    name: name.to_string(),
                    float: None,
                    bool: None,
                    trigger: None,
                };
                match *p {
                    AnimationParameter::Float(v) => json.float = Some(v),
                    AnimationParameter::Bool(v) => json.bool = Some(v),
                    AnimationParameter::Trigger(v) => json.trigger = Some(v),
                }
                json
            })
            .collect(),
        states: g
            .states
            .iter()
            .map(|s| {
                let (clip_name, blend1d) = match &s.motion {
                    SkeletonMotion::Clip(c) => (Some(clip(*c)), None),
                    SkeletonMotion::Blend1D { parameter, children } => (
                        None,
                        Some(Blend1DJson {
                            parameter: param(*parameter),
                            clips: children.iter().map(|(t, c)| (*t, clip(*c))).collect(),
                        }),
                    ),
                };
                StateJson {
                    name: s.name.to_string(),
                    clip: clip_name,
                    blend1d,
                    speed: s.speed,
                    looping: s.looping,
                }
            })
            .collect(),
        transitions: g
            .transitions
            .iter()
            .map(|t| TransitionJson {
                from: t.from.map(state),
                to: state(t.to),
                when: t
                    .conditions
                    .iter()
                    .map(|c| match *c {
                        AnimationCondition::Greater(p, v) => ConditionJson::Greater(param(p), v),
                        AnimationCondition::Less(p, v) => ConditionJson::Less(param(p), v),
                        AnimationCondition::Bool(p, v) => ConditionJson::Bool(param(p), v),
                        AnimationCondition::Trigger(p) => ConditionJson::Trigger(param(p)),
                    })
                    .collect(),
                exit_time: t.exit_time,
                duration: t.duration,
                offset: t.offset,
            })
            .collect(),
        initial: (g.initial != 0).then(|| g.states[g.initial].name.to_string()),
    }
}
