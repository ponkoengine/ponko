//! The convention for game data in glTF custom properties (a node's
//! `extras`): which keys the engine reads, what their values mean, how
//! they are checked, and the JSON Schema that editors validate against.
//!
//! Keys the engine reads start with `ponko:` (Blender writes an object's
//! custom properties as its node's `extras`, one key each). Keys without
//! the prefix belong to the game or other tools and are kept as they are.
//! The same keys may also sit, without the prefix, in an object under the
//! key `ponko` (what a script or a hand-written file finds easier).

#![deny(missing_docs)]

use serde_json::{Map, Value, json};

/// The prefix of the custom properties the engine reads.
pub(crate) const PREFIX: &str = "ponko:";

/// The shape a node's [`NodeProperties::collider`] is made from its mesh.
///
/// ```
/// use ponko::d3::NodeCollider;
/// assert_eq!(NodeCollider::parse("hull"), Some(NodeCollider::Hull));
/// assert_eq!(NodeCollider::Box.name(), "box");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeCollider {
    /// `"box"`: the box around the mesh's corners.
    Box,
    /// `"sphere"`: the ball around the box's center that holds every corner.
    Sphere,
    /// `"capsule"`: an upright capsule (along the node's Y) filling the box.
    Capsule,
    /// `"hull"`: the convex hull of the mesh's corners, right for moving
    /// bodies.
    Hull,
    /// `"trimesh"`: the mesh's triangles, for level geometry that never moves.
    Trimesh,
}

impl NodeCollider {
    /// Every shape, in the order the guide lists them.
    pub const ALL: [NodeCollider; 5] = [Self::Box, Self::Sphere, Self::Capsule, Self::Hull, Self::Trimesh];

    /// The shape's name in a file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Sphere => "sphere",
            Self::Capsule => "capsule",
            Self::Hull => "hull",
            Self::Trimesh => "trimesh",
        }
    }

    /// The shape named `name` in a file, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// How a node with a collider moves ([`NodeProperties::body`]).
///
/// ```
/// use ponko::d3::NodeBody;
/// assert_eq!(NodeBody::parse("dynamic"), Some(NodeBody::Dynamic));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum NodeBody {
    /// `"fixed"` (the default): never moves.
    #[default]
    Fixed,
    /// `"dynamic"`: falls and is pushed.
    Dynamic,
    /// `"kinematic"`: moved by the game only.
    Kinematic,
}

impl NodeBody {
    /// Every kind.
    pub const ALL: [NodeBody; 3] = [Self::Fixed, Self::Dynamic, Self::Kinematic];

    /// The kind's name in a file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Dynamic => "dynamic",
            Self::Kinematic => "kinematic",
        }
    }

    /// The kind named `name` in a file, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// What one glTF node's custom properties ask the engine for, read and
/// checked by [`NodeProperties::read`]. The guide's "From Blender to the
/// game" chapter lists the keys; [`gltf_properties_schema`] describes
/// them for editors.
///
/// ```
/// use ponko::d3::{NodeCollider, NodeProperties};
/// use ponko::prelude::*;
///
/// let extras = json!({ "ponko:collider": "box", "ponko:body": "dynamic", "note": "the game's own" });
/// let props = NodeProperties::read(&extras, &[]).unwrap();
/// assert_eq!(props.collider, Some(NodeCollider::Box));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct NodeProperties {
    /// `ponko:collider`: a collider made from the node's mesh.
    pub collider: Option<NodeCollider>,
    /// `ponko:body`: how the collider's body moves (fixed when not given).
    pub body: Option<NodeBody>,
    /// `ponko:sensor`: the collider reports overlaps and blocks nothing.
    pub sensor: bool,
    /// `ponko:collision_layers`: the collision groups the collider is in,
    /// as bits (the file gives layer numbers 0 to 31).
    pub collision_layers: Option<u32>,
    /// `ponko:collides_with`: the collision groups it touches, as bits.
    pub collides_with: Option<u32>,
    /// `ponko:layers`: the render layers its model is on, as bits.
    pub layers: Option<u32>,
    /// `ponko:hidden`: its mesh isn't drawn (a collision or navigation proxy).
    pub hidden: bool,
    /// `ponko:spawn`: a named spawn point (`true` names it after the node).
    pub spawn: Option<String>,
    /// `ponko:prefab`: a prefab file spawned in the node's place instead
    /// of its mesh.
    pub prefab: Option<String>,
    /// `ponko:lod`: the screen heights of a level-of-detail group whose
    /// levels are the node's children with meshes, most detailed first.
    pub lod: Vec<f32>,
    /// `ponko:navmesh`: whether its mesh is walkable ground for navigation
    /// meshes. Not given: meshes of fixed or no bodies are, others aren't.
    pub navmesh: Option<bool>,
    /// `ponko:shadows`: whether its light (a point or spot light) casts
    /// shadows.
    pub shadows: Option<bool>,
    /// `ponko:<Component>`: reflected components by name, with their JSON,
    /// in the file's order.
    pub components: Vec<(String, Value)>,
}

/// The keys of the convention, with what each takes (for the schema and
/// for suggestions).
const KEYS: [(&str, &str); 12] = [
    ("collider", "a collider made from the node's mesh"),
    ("body", "how the collider's body moves (default fixed)"),
    ("sensor", "the collider reports overlaps and blocks nothing"),
    ("collision_layers", "collision groups (0 to 31) the collider is in"),
    ("collides_with", "collision groups (0 to 31) the collider touches"),
    ("layers", "render layers (0 to 31) the model is on"),
    ("hidden", "the mesh isn't drawn (a collision or navigation proxy)"),
    ("spawn", "a named spawn point; true names it after the node"),
    ("prefab", "a prefab file spawned in the node's place"),
    (
        "lod",
        "screen heights of a level-of-detail group made of the node's children",
    ),
    ("navmesh", "whether the mesh is walkable ground for navigation meshes"),
    ("shadows", "whether the node's light casts shadows"),
];

impl NodeProperties {
    /// Reads the convention's keys from a node's `extras`; `components`
    /// names the reflected components (`world.reflected_components()`).
    /// Keys without the `ponko:` prefix are left to the game.
    ///
    /// ```
    /// use ponko::d3::NodeProperties;
    /// use ponko::prelude::*;
    ///
    /// let err = NodeProperties::read(&json!({ "ponko:colider": "box" }), &[]).unwrap_err();
    /// assert!(err.contains("did you mean \"ponko:collider\""), "{err}");
    /// ```
    ///
    /// # Errors
    ///
    /// A message naming the key, for an unknown key (with the closest
    /// known ones), a component that isn't reflected or a value of the
    /// wrong kind.
    pub fn read(extras: &Value, components: &[&str]) -> Result<Self, String> {
        let mut props = Self::default();
        let Value::Object(map) = extras else {
            return Ok(props);
        };
        let nested: Option<Map<String, Value>> = match map.get("ponko").map(parsed) {
            None => None,
            Some(Value::Object(inner)) => Some(inner),
            Some(_) => return Err("\"ponko\" must be an object of properties".into()),
        };
        let entries = map
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(PREFIX).map(|name| (name, v)))
            .chain(nested.iter().flat_map(|m| m.iter().map(|(k, v)| (k.as_str(), v))));
        for (name, value) in entries {
            props.set(name, value, components)?;
        }
        if props.lod.windows(2).any(|w| w[1] > w[0]) {
            return Err(format!(
                "\"{PREFIX}lod\": the screen heights must go down, most detailed first"
            ));
        }
        Ok(props)
    }

    fn set(&mut self, name: &str, value: &Value, components: &[&str]) -> Result<(), String> {
        let key = format!("{PREFIX}{name}");
        let wrong = |what: &str| format!("\"{key}\" takes {what}, not {}", short(value));
        match name {
            "collider" => {
                let text = value.as_str().ok_or_else(|| wrong("a shape name"))?;
                let names: Vec<&str> = NodeCollider::ALL.iter().map(|c| c.name()).collect();
                self.collider = Some(NodeCollider::parse(text).ok_or_else(|| unknown_value(&key, text, &names))?);
            }
            "body" => {
                let text = value.as_str().ok_or_else(|| wrong("a body kind"))?;
                let names: Vec<&str> = NodeBody::ALL.iter().map(|c| c.name()).collect();
                self.body = Some(NodeBody::parse(text).ok_or_else(|| unknown_value(&key, text, &names))?);
            }
            "sensor" => self.sensor = flag(value).ok_or_else(|| wrong("true or false"))?,
            "hidden" => self.hidden = flag(value).ok_or_else(|| wrong("true or false"))?,
            "navmesh" => self.navmesh = Some(flag(value).ok_or_else(|| wrong("true or false"))?),
            "shadows" => self.shadows = Some(flag(value).ok_or_else(|| wrong("true or false"))?),
            "collision_layers" => self.collision_layers = Some(layers(value).map_err(|e| wrong(&e))?),
            "collides_with" => self.collides_with = Some(layers(value).map_err(|e| wrong(&e))?),
            "layers" => self.layers = Some(layers(value).map_err(|e| wrong(&e))?),
            "spawn" => {
                self.spawn = Some(match value {
                    Value::String(s) => s.clone(),
                    Value::Bool(true) => String::new(),
                    _ => return Err(wrong("a name, or true")),
                });
            }
            "prefab" => match value {
                Value::String(s) if !s.is_empty() => self.prefab = Some(s.clone()),
                _ => return Err(wrong("a prefab file")),
            },
            "lod" => {
                let list = match parsed(value) {
                    Value::Array(list) => list,
                    _ => return Err(wrong("a list of screen heights")),
                };
                let heights: Option<Vec<f32>> = list
                    .iter()
                    .map(|v| v.as_f64().map(|h| h as f32).filter(|h| (0.0..=1.0).contains(h)))
                    .collect();
                self.lod = heights
                    .filter(|h| !h.is_empty())
                    .ok_or_else(|| wrong("a list of screen heights between 0 and 1"))?;
            }
            _ if components.contains(&name) => self.components.push((name.to_string(), parsed(value))),
            _ => {
                let mut known: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
                known.extend_from_slice(components);
                let what = if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    "no reflected component"
                } else {
                    "no property"
                };
                let mut message = format!("\"{key}\": {what} is called {name:?}");
                let close = suggestions(name, &known);
                if close.is_empty() {
                    message.push_str(" (properties: ");
                    message.push_str(&KEYS.map(|(k, _)| k).join(", "));
                    message.push_str("; components must be reflected with `world.reflect::<T>()`)");
                } else {
                    let quoted: Vec<String> = close.iter().map(|c| format!("\"{PREFIX}{c}\"")).collect();
                    message.push_str(&format!("; did you mean {}?", quoted.join(" or ")));
                }
                return Err(message);
            }
        }
        Ok(())
    }
}

/// `value`, or the JSON in it when it is a string holding JSON (Blender's
/// custom properties are easiest typed as text).
fn parsed(value: &Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| value.clone()),
        _ => value.clone(),
    }
}

/// A boolean: `true`/`false`, or 1/0 as older Blender files write them.
fn flag(value: &Value) -> Option<bool> {
    match parsed(value) {
        Value::Bool(b) => Some(b),
        Value::Number(n) => match n.as_u64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// Layer numbers (one, or a list) as bits.
fn layers(value: &Value) -> Result<u32, String> {
    let what = "a layer number from 0 to 31 or a list of them".to_string();
    let bit = |v: &Value| v.as_u64().filter(|&l| l < 32).map(|l| 1u32 << l);
    match parsed(value) {
        Value::Array(list) => list.iter().try_fold(0, |bits, v| bit(v).map(|b| bits | b)).ok_or(what),
        v => bit(&v).ok_or(what),
    }
}

/// A short form of `value` for messages.
fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 40 {
        format!("{}...", text.chars().take(40).collect::<String>())
    } else {
        text
    }
}

/// The message for `value`, which isn't one of `names`.
fn unknown_value(key: &str, value: &str, names: &[&str]) -> String {
    let close = suggestions(value, names);
    let hint = match close.first() {
        Some(c) => format!(" (did you mean {c:?}?)"),
        None => String::new(),
    };
    format!("\"{key}\" is {value:?}; it takes one of {}{hint}", names.join(", "))
}

/// The names in `known` closest to `name` (ignoring case), at most three,
/// when they are close enough to be what was meant.
pub(crate) fn suggestions<'a>(name: &str, known: &[&'a str]) -> Vec<&'a str> {
    let lower = name.to_lowercase();
    let mut scored: Vec<(usize, &str)> = known
        .iter()
        .map(|k| (edit_distance(&lower, &k.to_lowercase()), *k))
        .filter(|(d, k)| *d <= (k.chars().count().max(name.chars().count()) / 3).max(1) + 1)
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(3).map(|(_, k)| k).collect()
}

/// Edits (insert, delete, change, swap two neighbours) between `a` and `b`.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = d;
        }
    }
    rows[a.len()][b.len()]
}

/// The JSON Schema (draft 2020-12) of a glTF node's `extras` as the engine
/// reads them, with every component reflected in `world` (its own schema
/// where the component is described, any JSON where it isn't): for an
/// editor or a script to check custom properties before exporting. `ponko
/// schema` writes it for a game (see the guide's "From Blender to the
/// game").
///
/// ```
/// use ponko::prelude::*;
///
/// let mut world = World::new();
/// world.reflect::<ponko::d3::Transform>();
/// let schema = ponko::d3::gltf_properties_schema(&world);
/// assert!(schema["properties"]["ponko:collider"].is_object());
/// assert!(schema["properties"]["ponko:Transform3D"].is_object());
/// ```
#[must_use]
pub fn gltf_properties_schema(world: &ponko_ecs::World) -> Value {
    let flag = json!({ "anyOf": [{ "type": "boolean" }, { "enum": [0, 1] }] });
    let layer = json!({ "type": "integer", "minimum": 0, "maximum": 31 });
    let layer_list = json!({ "anyOf": [layer, { "type": "array", "items": layer }, { "type": "string" }] });
    let mut keys = Map::new();
    for (name, about) in KEYS {
        let mut schema = match name {
            "collider" => json!({ "enum": NodeCollider::ALL.map(NodeCollider::name) }),
            "body" => json!({ "enum": NodeBody::ALL.map(NodeBody::name) }),
            "sensor" | "hidden" | "navmesh" | "shadows" => flag.clone(),
            "collision_layers" | "collides_with" | "layers" => layer_list.clone(),
            "spawn" => json!({ "anyOf": [{ "type": "string" }, { "const": true }] }),
            "prefab" => json!({ "type": "string", "minLength": 1 }),
            _ => json!({ "anyOf": [
                { "type": "array", "minItems": 1, "items": { "type": "number", "minimum": 0, "maximum": 1 } },
                { "type": "string" }
            ] }),
        };
        schema["description"] = json!(about);
        keys.insert(name.to_string(), schema);
    }
    let (components, _) = crate::reflect::reflected_schemas(world);
    for c in components {
        let Some(name) = c["name"].as_str() else { continue };
        if keys.contains_key(name) {
            continue;
        }
        let value = match c["schema"].get("json_schema") {
            // Blender properties are often text: JSON in a string is read too.
            Some(schema) => json!({ "anyOf": [schema, { "type": "string" }] }),
            None => json!({}),
        };
        let mut value = value;
        value["description"] = json!(format!("the reflected component {name}, as JSON"));
        keys.insert(name.to_string(), value);
    }
    let names: Vec<String> = keys.keys().map(|k| format!("{PREFIX}{k}")).collect();
    let mut properties: Map<String, Value> = keys.iter().map(|(k, v)| (format!("{PREFIX}{k}"), v.clone())).collect();
    properties.insert(
        "ponko".into(),
        json!({
            "description": "the same properties without the prefix, as one object",
            "type": "object",
            "properties": Value::Object(keys),
            "additionalProperties": false,
        }),
    );
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Ponko glTF node properties",
        "description": "The custom properties (glTF node extras) the engine reads. Keys without the ponko: prefix belong to the game and are not checked.",
        "type": "object",
        "properties": Value::Object(properties),
        "propertyNames": { "anyOf": [
            { "not": { "pattern": "^ponko:" } },
            { "enum": names },
        ] },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_find_typos_and_case_slips_but_not_strangers() {
        let known = ["collider", "collides_with", "Health", "prefab"];
        assert_eq!(suggestions("colider", &known), ["collider"]);
        assert_eq!(suggestions("health", &known), ["Health"]);
        assert_eq!(suggestions("perfab", &known), ["prefab"]);
        assert!(suggestions("velocity", &known).is_empty());
    }

    #[test]
    fn values_in_text_are_read_as_json() {
        let extras = json!({ "ponko:layers": "[1, 3]", "ponko:lod": "[0.5, 0.1]", "ponko:sensor": 1 });
        let props = NodeProperties::read(&extras, &[]).unwrap();
        assert_eq!(props.layers, Some(0b1010));
        assert_eq!(props.lod, [0.5, 0.1]);
        assert!(props.sensor);
    }

    #[test]
    fn a_ponko_object_reads_like_prefixed_keys() {
        let a = NodeProperties::read(
            &json!({ "ponko": { "collider": "sphere", "Health": { "hp": 3 } } }),
            &["Health"],
        );
        let b = NodeProperties::read(
            &json!({ "ponko:collider": "sphere", "ponko:Health": "{\"hp\": 3}" }),
            &["Health"],
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let text = NodeProperties::read(&json!({ "ponko": "{\"collider\": \"box\"}" }), &[]).unwrap();
        assert_eq!(text.collider, Some(NodeCollider::Box));
    }

    #[test]
    fn wrong_values_name_the_key_and_what_it_takes() {
        let err = NodeProperties::read(&json!({ "ponko:collider": "bax" }), &[]).unwrap_err();
        assert!(
            err.contains("\"ponko:collider\" is \"bax\"") && err.contains("did you mean \"box\""),
            "{err}"
        );
        let err = NodeProperties::read(&json!({ "ponko:layers": 40 }), &[]).unwrap_err();
        assert!(err.contains("0 to 31"), "{err}");
        let err = NodeProperties::read(&json!({ "ponko:lod": [0.1, 0.5] }), &[]).unwrap_err();
        assert!(err.contains("go down"), "{err}");
    }

    #[test]
    fn unknown_components_suggest_reflected_ones() {
        let err = NodeProperties::read(&json!({ "ponko:Helth": {} }), &["Health", "Model"]).unwrap_err();
        assert!(
            err.contains("no reflected component") && err.contains("\"ponko:Health\""),
            "{err}"
        );
        let err = NodeProperties::read(&json!({ "ponko:wobble": 1 }), &[]).unwrap_err();
        assert!(err.contains("properties: collider"), "{err}");
    }
}
