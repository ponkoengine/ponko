//! A custom material kept in a glTF material's `extras`: which material
//! shader draws it and the values that shader reads, so a model exported
//! with them (or saved by the editor's materials window) loads drawn with
//! its shader and values, with no code. A material's extras:
//!
//! ```json
//! "extras": {
//!   "ponko:shader": "water.wgsl",
//!   "ponko:params": [
//!     { "name": "tint", "type": "color", "value": [0.1, 0.4, 0.8, 1.0] },
//!     { "name": "speed", "type": "float", "value": 2.0 },
//!     { "name": "scroll", "type": "vec2", "value": [1.0, 0.0] }
//!   ]
//! }
//! ```
//!
//! The shader's name is relative to the glTF file. The parameters, at most
//! four, are `material.custom[0]` to `[3]` in the shader, in their order:
//! a `float` is `x`, a `vec2` `xy`, a `vec3` `xyz`, and a `vec4` or a
//! `color` (red, green, blue, alpha; alpha 1 if left out) all four. The
//! rest of a value is 0. The name is the editor's label for the value.
//! Other keys of the extras are the game's own.

use glam::Vec4;
use serde_json::{Value, json};

/// The key of the shader's name.
pub(crate) const SHADER_KEY: &str = "ponko:shader";
/// The key of the parameter list.
pub(crate) const PARAMS_KEY: &str = "ponko:params";

/// What a parameter holds: how many of its four numbers count, and how
/// the editor shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParamKind {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
}

impl ParamKind {
    pub(crate) const ALL: [Self; 5] = [Self::Float, Self::Vec2, Self::Vec3, Self::Vec4, Self::Color];

    /// Its name in the file.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Color => "color",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// How many numbers it has.
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Float => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 | Self::Color => 4,
        }
    }
}

/// One value of a custom material.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Param {
    pub name: String,
    pub kind: ParamKind,
    /// The numbers past the kind's length are 0.
    pub value: Vec4,
}

/// A custom material as its extras keep it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CustomExtras {
    /// The shader's name as written (relative to the glTF file).
    pub shader: Option<String>,
    pub params: Vec<Param>,
}

impl CustomExtras {
    /// Reads them from a material's `extras`; `Ok(None)` when it has
    /// neither key.
    ///
    /// # Errors
    /// What's wrong with them.
    pub(crate) fn read(extras: &Value) -> Result<Option<Self>, String> {
        let (shader, params) = (extras.get(SHADER_KEY), extras.get(PARAMS_KEY));
        if shader.is_none() && params.is_none() {
            return Ok(None);
        }
        let shader = match shader {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(other) => return Err(format!("\"{SHADER_KEY}\" must be a file name (got {other})")),
        };
        let list = match params {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(list)) => list.as_slice(),
            Some(other) => return Err(format!("\"{PARAMS_KEY}\" must be a list (got {other})")),
        };
        if list.len() > 4 {
            return Err(format!(
                "\"{PARAMS_KEY}\" has {} values; a material shader reads four (material.custom[0] to [3])",
                list.len()
            ));
        }
        let params = list
            .iter()
            .enumerate()
            .map(|(i, p)| Param::read(p).map_err(|e| format!("\"{PARAMS_KEY}\"[{i}]: {e}")))
            .collect::<Result<_, _>>()?;
        Ok(Some(Self { shader, params }))
    }

    /// Writes them into `extras` (an object, made one if it isn't),
    /// keeping its other keys; with no shader and no values both keys go.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub(crate) fn write(&self, extras: &mut Value) {
        if !extras.is_object() {
            *extras = json!({});
        }
        let Some(map) = extras.as_object_mut() else { return };
        match &self.shader {
            Some(s) => drop(map.insert(SHADER_KEY.into(), json!(s))),
            None => drop(map.remove(SHADER_KEY)),
        }
        if self.params.is_empty() {
            map.remove(PARAMS_KEY);
        } else {
            map.insert(
                PARAMS_KEY.into(),
                Value::Array(self.params.iter().map(Param::to_json).collect()),
            );
        }
    }

    /// The values as `MaterialDesc::custom` has them.
    pub(crate) fn custom(&self) -> [Vec4; 4] {
        let mut out = [Vec4::ZERO; 4];
        for (to, p) in out.iter_mut().zip(&self.params) {
            *to = p.value;
        }
        out
    }
}

impl Param {
    fn read(p: &Value) -> Result<Self, String> {
        let name = p["name"].as_str().unwrap_or("").to_string();
        let kind_name = p["type"]
            .as_str()
            .ok_or("\"type\" must be float, vec2, vec3, vec4 or color")?;
        let kind = ParamKind::from_name(kind_name)
            .ok_or_else(|| format!("\"type\" must be float, vec2, vec3, vec4 or color (got \"{kind_name}\")"))?;
        let numbers: Vec<f32> = match &p["value"] {
            Value::Number(n) => vec![n.as_f64().unwrap_or(0.0) as f32],
            Value::Array(list) => list
                .iter()
                .map(|v| v.as_f64().map(|x| x as f32).ok_or("\"value\" must hold numbers"))
                .collect::<Result<_, _>>()?,
            other => return Err(format!("\"value\" must be a number or a list of numbers (got {other})")),
        };
        // A colour may leave its alpha out.
        let fits = numbers.len() == kind.len() || (kind == ParamKind::Color && numbers.len() == 3);
        if !fits {
            return Err(format!(
                "a {} value has {} numbers (got {})",
                kind.name(),
                kind.len(),
                numbers.len()
            ));
        }
        let mut value = Vec4::ZERO;
        for (i, x) in numbers.into_iter().enumerate() {
            value[i] = x;
        }
        if kind == ParamKind::Color && p["value"].as_array().is_some_and(|l| l.len() == 3) {
            value.w = 1.0;
        }
        Ok(Self { name, kind, value })
    }

    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    fn to_json(&self) -> Value {
        let numbers = &self.value.to_array()[..self.kind.len()];
        let value = match self.kind {
            ParamKind::Float => json!(numbers[0]),
            _ => json!(numbers),
        };
        json!({ "name": self.name, "type": self.kind.name(), "value": value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_values_survive_a_write_and_read_through_extras_with_name_type_and_value() {
        let extras = CustomExtras {
            shader: Some("water.wgsl".into()),
            params: vec![
                Param {
                    name: "tint".into(),
                    kind: ParamKind::Color,
                    value: Vec4::new(0.1, 0.4, 0.8, 0.5),
                },
                Param {
                    name: "speed".into(),
                    kind: ParamKind::Float,
                    value: Vec4::new(2.5, 0.0, 0.0, 0.0),
                },
                Param {
                    name: "scroll".into(),
                    kind: ParamKind::Vec2,
                    value: Vec4::new(1.0, -0.25, 0.0, 0.0),
                },
                Param {
                    name: "wave".into(),
                    kind: ParamKind::Vec3,
                    value: Vec4::new(3.0, 4.0, 5.0, 0.0),
                },
            ],
        };
        let mut json = json!({ "game": "kept" });
        extras.write(&mut json);
        assert_eq!(json["game"], "kept", "the game's own keys stay");
        assert_eq!(
            json[PARAMS_KEY][1],
            json!({ "name": "speed", "type": "float", "value": 2.5 })
        );
        // Through text, as a file keeps it.
        let back: Value = serde_json::from_str(&json.to_string()).unwrap();
        let read = CustomExtras::read(&back).unwrap().unwrap();
        assert_eq!(read, extras);
        assert_eq!(read.custom()[0], Vec4::new(0.1, 0.4, 0.8, 0.5));
        assert_eq!(read.custom()[3], Vec4::new(3.0, 4.0, 5.0, 0.0));
        // Emptied, both keys go.
        CustomExtras::default().write(&mut json);
        assert_eq!(json, json!({ "game": "kept" }));
        assert_eq!(CustomExtras::read(&json).unwrap(), None);
    }

    #[test]
    fn extras_that_dont_fit_say_what_is_wrong() {
        let read = |params: Value| CustomExtras::read(&json!({ PARAMS_KEY: params })).unwrap_err();
        assert!(read(json!([{ "type": "vec2", "value": [1] }])).contains("2 numbers"));
        assert!(read(json!([{ "type": "mat4", "value": 1 }])).contains("mat4"));
        assert!(read(Value::Array(vec![json!({ "type": "float", "value": 1 }); 5])).contains("four"));
        assert!(
            CustomExtras::read(&json!({ SHADER_KEY: 3 }))
                .unwrap_err()
                .contains("file name")
        );
        let rgb = CustomExtras::read(&json!({ PARAMS_KEY: [{ "type": "color", "value": [1, 0, 0] }] }));
        assert_eq!(rgb.unwrap().unwrap().params[0].value, Vec4::new(1.0, 0.0, 0.0, 1.0));
    }
}
