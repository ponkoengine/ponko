//! Reflects the 2D [`Skeleton`] as `"Skeleton2D"`: the skeleton file it
//! plays (by name, like a sprite's image), what plays and how far, its
//! parameters, skin and look, so prefabs, scenes, saves and the inspector
//! keep an animated skeleton. A fade in progress isn't kept: a loaded
//! skeleton plays the fade's target.

use std::collections::BTreeMap;
use std::sync::Arc;

use ponko_ecs::World;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Reflected, Registry, SchemaSource, remove};
use crate::anim::AnimationParameter;
use crate::color::Color;
use crate::schema::ValueSchema;
use crate::skeleton2d::{Skeleton, SkeletonLibrary};

/// The reflected name.
pub(crate) const NAME: &str = "Skeleton2D";

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}

/// A [`Skeleton`] as JSON. A file needs only `skeleton`; the rest has
/// defaults (rest pose, or the graph's initial state). Saves write every
/// field.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
struct SkeletonJson {
    skeleton: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(default)]
    time: f32,
    #[serde(default = "yes")]
    looping: bool,
    #[serde(default = "one")]
    speed: f32,
    #[serde(default)]
    z: f32,
    #[serde(default)]
    color: Option<Color>,
    #[serde(default)]
    skin: Option<String>,
    #[serde(default)]
    parameters: BTreeMap<String, Value>,
    #[serde(default)]
    manual: bool,
}

impl SkeletonJson {
    fn check(&self) -> Result<(), String> {
        if self.clip.is_some() && self.state.is_some() {
            return Err("a skeleton plays a clip or a state, not both".into());
        }
        if !(self.time.is_finite() && self.speed.is_finite() && self.z.is_finite()) {
            return Err("time, speed and z must be numbers".into());
        }
        for (name, v) in &self.parameters {
            if !(v.is_boolean() || v.as_f64().is_some_and(f64::is_finite)) {
                return Err(format!("parameter \"{name}\" must be a number or true/false"));
            }
        }
        Ok(())
    }
}

/// `None` for a skeleton built in code: it has no file to name, so it
/// isn't saved (the rest of the entity is).
fn skeleton_json(s: &Skeleton) -> Option<Result<Value, String>> {
    let name = s.data().name()?.to_string();
    let playing = s.playing();
    let mut parameters = BTreeMap::new();
    if let Some(graph) = s.data().graph() {
        for (i, n) in graph.parameter_names().iter().enumerate() {
            let value = match s.parameter(crate::anim::ParamId(i)) {
                Some(AnimationParameter::Float(v)) => Value::from(v),
                Some(AnimationParameter::Bool(v) | AnimationParameter::Trigger(v)) => Value::from(v),
                _ => continue,
            };
            parameters.insert(n.to_string(), value);
        }
    }
    let json = SkeletonJson {
        skeleton: name,
        clip: playing.filter(|p| !p.0).map(|p| p.1.to_string()),
        state: playing.filter(|p| p.0).map(|p| p.1.to_string()),
        time: playing.map_or(0.0, |p| p.2),
        looping: playing.is_none_or(|p| p.3),
        speed: s.speed,
        z: s.z,
        color: Some(s.color),
        skin: Some(s.skin().to_string()),
        parameters,
        manual: s.manual,
    };
    Some(serde_json::to_value(json).map_err(|e| e.to_string()))
}

fn skeleton_from_json(world: &World, value: Value) -> Result<Skeleton, String> {
    let json: SkeletonJson = serde_json::from_value(value).map_err(|e| e.to_string())?;
    json.check()?;
    let data = world
        .get_resource::<SkeletonLibrary>()
        .and_then(|l| l.get(&json.skeleton))
        .cloned()
        .ok_or_else(|| {
            let name = &json.skeleton;
            format!("skeleton \"{name}\" isn't loaded: ctx.load_skeleton2d(\"{name}\") first")
        })?;
    let mut s = Skeleton::new(data);
    if let Some(skin) = &json.skin
        && !s.set_skin(skin)
    {
        return Err(format!("skeleton \"{}\" has no skin \"{skin}\"", json.skeleton));
    }
    for (name, value) in &json.parameters {
        let p = s
            .parameter_id(name)
            .ok_or_else(|| format!("skeleton \"{}\" has no parameter \"{name}\"", json.skeleton))?;
        match (s.parameter(p), value) {
            (Some(AnimationParameter::Float(_)), Value::Number(n)) => s.set_float(p, n.as_f64().unwrap_or(0.0) as f32),
            (Some(AnimationParameter::Bool(_)), Value::Bool(b)) => s.set_bool(p, *b),
            (Some(AnimationParameter::Trigger(_)), Value::Bool(b)) => s.set_trigger(p, *b),
            _ => return Err(format!("parameter \"{name}\" has the wrong kind of value")),
        }
    }
    let restored = match (&json.clip, &json.state) {
        (Some(clip), None) => Some((false, clip)),
        (None, Some(state)) => Some((true, state)),
        _ => None,
    };
    if let Some((state, name)) = restored
        && !s.restore(state, name, json.time, json.looping)
    {
        let what = if state { "state" } else { "clip" };
        return Err(format!("skeleton \"{}\" has no {what} \"{name}\"", json.skeleton));
    }
    s.speed = json.speed;
    s.z = json.z;
    s.color = json.color.unwrap_or(Color::WHITE);
    s.manual = json.manual;
    s.update(0.0);
    Ok(s)
}

fn check(value: &Value) -> Result<(), String> {
    SkeletonJson::deserialize(value).map_err(|e| e.to_string())?.check()
}

/// The skeleton's JSON shape.
pub(super) fn schema() -> ValueSchema {
    let optional = ValueSchema::optional;
    ValueSchema::strict_object([
        ("skeleton", ValueSchema::String),
        ("clip", optional(ValueSchema::String)),
        ("state", optional(ValueSchema::String)),
        ("time", optional(ValueSchema::Number)),
        ("looping", optional(ValueSchema::Bool)),
        ("speed", optional(ValueSchema::Number)),
        ("z", optional(ValueSchema::Number)),
        ("color", optional(ValueSchema::Color)),
        ("skin", optional(ValueSchema::String)),
        ("parameters", optional(ValueSchema::Opaque)),
        ("manual", optional(ValueSchema::Bool)),
    ])
}

/// Reflects [`Skeleton`] as `"Skeleton2D"`.
pub(crate) fn reflect_skeleton2d(world: &mut World) {
    crate::skeleton2d::library(world);
    if world.get_resource::<Registry>().is_none() {
        world.insert_resource(Registry::default());
    }
    let reg = world.resource_mut::<Registry>();
    if reg.by_name.contains_key(NAME) {
        return;
    }
    reg.schemas.insert(NAME.into(), (schema(), SchemaSource::Described));
    reg.by_name.insert(NAME.into(), reg.list.len());
    reg.list.push(Reflected {
        name: NAME.into(),
        type_name: std::any::type_name::<Skeleton>().into(),
        get: Arc::new(|w: &World, e| w.get::<Skeleton>(e).and_then(skeleton_json)),
        set: Arc::new(|w: &mut World, e, v| {
            let s = skeleton_from_json(w, v)?;
            w.insert(e, s);
            Ok(())
        }),
        spawn: Arc::new(|w: &mut World, v| {
            let s = skeleton_from_json(w, v)?;
            Ok(w.spawn((s,)))
        }),
        spawn_at: Arc::new(|w: &mut World, e, v| {
            let s = skeleton_from_json(w, v)?;
            Ok(w.spawn_at(e, (s,)))
        }),
        check: Arc::new(check),
        remove: Arc::new(remove::<Skeleton>),
    });
}
