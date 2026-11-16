//! The skeleton file format (`"format": "ponko-skeleton2d"`, version 1).
//! The guide's "2D skeletons" chapter documents it field by field.

use std::sync::Arc;

use glam::Vec2;
use serde::Deserialize;

use super::clip::{Bezier, BoneTrack, Clip, Curve, Key, SlotTrack};
use super::data::{BonePose, IkConstraint, MeshVertex, SkeletonData, SkeletonError, SkinnedMesh, SpritePart, err};
use super::graph::{SkeletonGraph, SkeletonMotion, SkeletonState};
use crate::anim::{AnimationCondition, AnimationParameter, AnimationTransition, StateId};
use crate::camera::Rect;
use crate::color::Color;
use crate::texture::Texture;

/// The `format` field of a skeleton file.
pub(crate) const FORMAT: &str = "ponko-skeleton2d";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileJson {
    format: String,
    version: u32,
    bones: Vec<BoneJson>,
    #[serde(default)]
    slots: Vec<SlotJson>,
    #[serde(default)]
    skins: Vec<SkinJson>,
    #[serde(default)]
    ik: Vec<IkJson>,
    #[serde(default)]
    clips: Vec<ClipJson>,
    #[serde(default)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoneJson {
    name: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    /// Degrees, clockwise on screen.
    #[serde(default)]
    rotation: f32,
    #[serde(default = "ones")]
    scale: [f32; 2],
    #[serde(default)]
    length: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotJson {
    name: String,
    bone: String,
    #[serde(default)]
    attachment: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkinJson {
    name: String,
    attachments: Vec<AttachmentJson>,
}

#[derive(Deserialize, Default, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Kind {
    #[default]
    Sprite,
    Mesh,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentJson {
    slot: String,
    name: String,
    #[serde(default, rename = "type")]
    kind: Kind,
    image: String,
    #[serde(default)]
    color: Option<String>,
    // A sprite's.
    #[serde(default)]
    region: Option<[f32; 4]>,
    #[serde(default)]
    rotated: bool,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    rotation: f32,
    #[serde(default = "ones")]
    scale: [f32; 2],
    #[serde(default)]
    size: Option<[f32; 2]>,
    #[serde(default)]
    pivot: Option<[f32; 2]>,
    // A mesh's.
    #[serde(default)]
    vertices: Vec<VertexJson>,
    #[serde(default)]
    triangles: Vec<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VertexJson {
    position: [f32; 2],
    uv: [f32; 2],
    bones: Vec<String>,
    #[serde(default)]
    weights: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IkJson {
    name: String,
    bones: Vec<String>,
    target: String,
    #[serde(default = "yes")]
    bend_positive: bool,
    #[serde(default = "one")]
    mix: f32,
}

/// `"linear"`, `"stepped"`, `{ "bezier": [x1, y1, x2, y2] }` or
/// `{ "bezier": [[x1, y1, x2, y2], ...] }` (one per channel).
#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum CurveJson {
    #[default]
    Linear,
    Stepped,
    Bezier(BezierJson),
}

#[derive(Deserialize, Clone)]
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyJson<T> {
    #[serde(default)]
    time: f32,
    value: T,
    #[serde(default)]
    curve: CurveJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipJson {
    name: String,
    #[serde(default)]
    duration: Option<f32>,
    #[serde(default)]
    bones: Vec<BoneTrackJson>,
    #[serde(default)]
    slots: Vec<SlotTrackJson>,
    #[serde(default)]
    draw_order: Vec<DrawOrderJson>,
    #[serde(default)]
    events: Vec<EventJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoneTrackJson {
    bone: String,
    #[serde(default)]
    translate: Vec<KeyJson<[f32; 2]>>,
    /// Degrees.
    #[serde(default)]
    rotate: Vec<KeyJson<f32>>,
    #[serde(default)]
    scale: Vec<KeyJson<[f32; 2]>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotTrackJson {
    slot: String,
    #[serde(default)]
    attachment: Vec<KeyJson<Option<String>>>,
    #[serde(default)]
    visible: Vec<KeyJson<bool>>,
    #[serde(default)]
    color: Vec<KeyJson<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DrawOrderJson {
    #[serde(default)]
    time: f32,
    order: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventJson {
    time: f32,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphJson {
    #[serde(default)]
    parameters: Vec<ParameterJson>,
    states: Vec<StateJson>,
    #[serde(default)]
    transitions: Vec<TransitionJson>,
    #[serde(default)]
    initial: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterJson {
    name: String,
    #[serde(default)]
    float: Option<f32>,
    #[serde(default)]
    bool: Option<bool>,
    #[serde(default)]
    trigger: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateJson {
    name: String,
    #[serde(default)]
    clip: Option<String>,
    #[serde(default)]
    blend1d: Option<Blend1DJson>,
    #[serde(default = "one")]
    speed: f32,
    #[serde(default = "yes", rename = "loop")]
    looping: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Blend1DJson {
    parameter: String,
    clips: Vec<(f32, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionJson {
    #[serde(default)]
    from: Option<String>,
    to: String,
    #[serde(default)]
    when: Vec<ConditionJson>,
    #[serde(default)]
    exit_time: Option<f32>,
    #[serde(default)]
    duration: f32,
    #[serde(default)]
    offset: f32,
}

#[derive(Deserialize)]
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
}

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
        let rest = BonePose {
            translation: Vec2::new(b.x, b.y),
            rotation: b.rotation.to_radians(),
            scale: Vec2::from(b.scale),
        };
        if !(rest.translation.is_finite() && rest.rotation.is_finite() && rest.scale.is_finite()) {
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
        let color = s.color.as_deref().map(hex_color).transpose()?.unwrap_or(Color::WHITE);
        data.set_slot_rest(slot, s.attachment.as_deref(), color);
    }
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
            let image: Arc<str> = beside(path, &a.image).into();
            let color = a.color.as_deref().map(hex_color).transpose()?.unwrap_or(Color::WHITE);
            let attachment: super::Attachment = match a.kind {
                Kind::Sprite => sprite_part(a, image, color)?.into(),
                Kind::Mesh => mesh(&data, a, image, color)?.into(),
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
    for ik in &file.ik {
        let bones = ik
            .bones
            .iter()
            .map(|b| index(data.bone(b), "bone", b))
            .collect::<Result<Vec<_>, _>>()?;
        let constraint = IkConstraint {
            name: ik.name.as_str().into(),
            bones,
            target: index(data.bone(&ik.target), "bone", &ik.target)?,
            bend_positive: ik.bend_positive,
            mix: ik.mix,
        };
        data.check_ik(&constraint)?;
        data.ik.push(constraint);
    }
    for c in &file.clips {
        let clip = clip(&data, c)?;
        if data.clip(&clip.name).is_some() {
            return Err(err(format!("two clips are named \"{}\"", clip.name)));
        }
        clip.check(data.bones.len(), data.slots.len())?;
        data.clips.push(clip);
    }
    if let Some(g) = &file.graph {
        let graph = graph(&data, g)?;
        data.set_graph(graph)?;
    }
    Ok(data)
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
    let offset = BonePose {
        translation: Vec2::new(a.x, a.y),
        rotation: a.rotation.to_radians(),
        scale: Vec2::from(a.scale),
    };
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

fn mesh(data: &SkeletonData, a: &AttachmentJson, image: Arc<str>, color: Color) -> Result<SkinnedMesh, SkeletonError> {
    let mut vertices = Vec::with_capacity(a.vertices.len());
    for v in &a.vertices {
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
        if bones.is_empty() || weights.len() != bones.len() || !weights.iter().any(|w| *w > 0.0) {
            return Err(err(format!(
                "mesh \"{}\": every vertex needs bones with as many weights, one above 0",
                a.name
            )));
        }
        let influences: Vec<(usize, f32)> = bones.into_iter().zip(weights).collect();
        vertices.push(MeshVertex::new(Vec2::from(v.position), Vec2::from(v.uv), &influences));
    }
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

fn clip(data: &SkeletonData, c: &ClipJson) -> Result<Clip, SkeletonError> {
    let mut last = 0.0f32;
    let mut clip = Clip::new(&c.name, 0.0);
    for t in &c.bones {
        let mut track = BoneTrack::new(index(data.bone(&t.bone), "bone", &t.bone)?);
        track.translate = keys(&t.translate, |v| Ok(Vec2::from(*v)))?;
        track.rotate = keys(&t.rotate, |v| Ok(v.to_radians()))?;
        track.scale = keys(&t.scale, |v| Ok(Vec2::from(*v)))?;
        for k in track
            .translate
            .iter()
            .map(|k| k.time)
            .chain(track.rotate.iter().map(|k| k.time))
        {
            last = last.max(k);
        }
        for k in &track.scale {
            last = last.max(k.time);
        }
        clip.bones.push(track);
    }
    for t in &c.slots {
        let mut track = SlotTrack::new(index(data.slot(&t.slot), "slot", &t.slot)?);
        track.attachment = keys(&t.attachment, |v| Ok(v.as_deref().map(Arc::from)))?;
        track.visible = keys(&t.visible, |v| Ok(*v))?;
        track.color = keys(&t.color, |v| hex_color(v))?;
        for time in track
            .attachment
            .iter()
            .map(|k| k.time)
            .chain(track.visible.iter().map(|k| k.time))
            .chain(track.color.iter().map(|k| k.time))
        {
            last = last.max(time);
        }
        clip.slots.push(track);
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
        last = last.max(k.time);
        clip.draw_order.push(Key::new(k.time, order).with_curve(Curve::Stepped));
    }
    for e in &c.events {
        last = last.max(e.time);
    }
    clip.duration = c.duration.unwrap_or(last);
    for e in &c.events {
        clip = clip.with_event(e.time, &e.name);
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
