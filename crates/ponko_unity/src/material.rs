//! Unity materials (class 21) read as PBR settings: what URP's Lit,
//! Simple Lit and Unlit shaders (and most others, which follow their
//! property names) mean by their textures, colors and numbers.

use std::collections::BTreeMap;

use crate::{ObjRef, UnityObject, Value};

/// A texture property of a material: the texture, its tiling and offset.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureProperty {
    /// The texture.
    pub texture: ObjRef,
    /// Its tiling (`m_Scale`).
    pub scale: [f32; 2],
    /// Its offset (`m_Offset`).
    pub offset: [f32; 2],
}

/// How a material covers what's behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    /// Solid.
    Opaque,
    /// Cut out where the base alpha is below the threshold.
    Cutout(f32),
    /// Blended by the base alpha.
    Transparent,
}

/// A Unity material as physically based settings.
///
/// ```
/// let text = "%YAML 1.1\n--- !u!21 &2100000\nMaterial:\n  m_Name: Bark\n  m_Shader: {fileID: 4800000, guid: 933532a4fcc9baf4fa0491de14d08ed7, type: 3}\n  m_SavedProperties:\n    m_TexEnvs:\n    - _BaseMap:\n        m_Texture: {fileID: 2800000, guid: 0123abcd, type: 3}\n        m_Scale: {x: 2, y: 2}\n        m_Offset: {x: 0, y: 0}\n    m_Floats:\n    - _Smoothness: 0.25\n    m_Colors:\n    - _BaseColor: {r: 1, g: 0.5, b: 0.5, a: 1}\n";
/// let file = ponko_unity::UnityFile::parse(text).unwrap();
/// let m = ponko_unity::UnityMaterial::from_object(&file.objects[0]).unwrap();
/// assert_eq!(m.base_map.unwrap().guid.as_deref(), Some("0123abcd"));
/// assert_eq!(m.tiling, [2.0, 2.0]);
/// assert_eq!(m.smoothness, 0.25);
/// assert_eq!(m.color, [1.0, 0.5, 0.5, 1.0]);
/// assert_eq!(m.floats["_Smoothness"], 0.25, "every property is kept too");
/// assert_eq!(m.textures["_BaseMap"].scale, [2.0, 2.0]);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UnityMaterial {
    /// `m_Name`.
    pub name: String,
    /// The shader's GUID (`None` for a built-in shader).
    pub shader: Option<String>,
    /// The shader's id in its file (built-in shaders differ only by it).
    pub shader_file_id: i64,
    /// Base color (`_BaseColor` or `_Color`), sRGB, with alpha.
    pub color: [f32; 4],
    /// Base color texture (`_BaseMap` or `_MainTex`).
    pub base_map: Option<ObjRef>,
    /// The base map's tiling and offset, which the other maps share in URP.
    pub tiling: [f32; 2],
    /// The base map's texture offset, in UV units, applied after `tiling`.
    pub offset: [f32; 2],
    /// Tangent-space normal map (`_BumpMap`) and its strength.
    pub normal_map: Option<ObjRef>,
    /// The normal map's strength (`_BumpScale`); 1 leaves it as drawn.
    pub normal_scale: f32,
    /// `_Metallic`, 0 to 1 (ignored by Unity when there's a metallic map).
    pub metallic: f32,
    /// `_Smoothness` (`_Glossiness`), 0 to 1: one minus roughness. With a
    /// metallic map it scales the map's smoothness.
    pub smoothness: f32,
    /// `_MetallicGlossMap`: metalness in red, smoothness in alpha.
    pub metallic_map: Option<ObjRef>,
    /// Smoothness comes from the base map's alpha instead
    /// (`_SmoothnessTextureChannel` 1).
    pub smoothness_in_base_alpha: bool,
    /// `_OcclusionMap` (green is used) and `_OcclusionStrength`.
    pub occlusion_map: Option<ObjRef>,
    /// How much the occlusion map darkens (`_OcclusionStrength`), 0 to 1.
    pub occlusion_strength: f32,
    /// Emitted light (`_EmissionColor`), when emission is on; HDR, sRGB.
    pub emission: [f32; 3],
    /// `_EmissionMap`.
    pub emission_map: Option<ObjRef>,
    /// `_Surface`, `_AlphaClip` and `_Cutoff`.
    pub surface: Surface,
    /// `_Cull` 0: both sides drawn.
    pub double_sided: bool,
    /// Every number the material sets (`m_Floats`), by property name:
    /// what a custom shader reads.
    pub floats: BTreeMap<String, f32>,
    /// Every color it sets (`m_Colors`), as saved (sRGB for colors).
    pub colors: BTreeMap<String, [f32; 4]>,
    /// Every texture it sets (`m_TexEnvs`, empty slots left out).
    pub textures: BTreeMap<String, TextureProperty>,
}

impl UnityMaterial {
    /// Reads a `Material` object.
    ///
    /// # Errors
    ///
    /// If it isn't a material.
    pub fn from_object(object: &UnityObject) -> Result<Self, crate::Error> {
        if object.class_id != 21 {
            return Err(crate::Error::invalid(format!(
                "object {} is a {}, not a Material",
                object.file_id, object.type_name
            )));
        }
        let body = &object.body;
        let props = &body["m_SavedProperties"];
        let texture = |names: &[&str]| -> Option<(ObjRef, [f32; 2], [f32; 2])> {
            names.iter().find_map(|n| {
                let entry = find(&props["m_TexEnvs"], n)?;
                let r = entry["m_Texture"]
                    .obj_ref()
                    .filter(|r| !r.is_none() && r.guid.is_some())?;
                let scale = entry["m_Scale"].vec2().unwrap_or([1.0; 2]);
                let offset = entry["m_Offset"].vec2().unwrap_or([0.0; 2]);
                Some((r, scale, offset))
            })
        };
        let float = |names: &[&str], default: f32| {
            names
                .iter()
                .find_map(|n| find(&props["m_Floats"], n).and_then(Value::as_f32))
                .unwrap_or(default)
        };
        let color = |names: &[&str]| {
            names
                .iter()
                .find_map(|n| find(&props["m_Colors"], n).and_then(Value::color))
        };
        let keywords: Vec<&str> = ["m_ValidKeywords", "m_ShaderKeywords"]
            .iter()
            .flat_map(|k| match &body[*k] {
                Value::Seq(items) => items.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
                Value::Scalar(s) => s.split_whitespace().collect(),
                _ => Vec::new(),
            })
            .collect();
        let shader = body["m_Shader"].obj_ref().unwrap_or(ObjRef {
            file_id: 0,
            guid: None,
            kind: None,
        });
        let base = texture(&["_BaseMap", "_MainTex", "_BaseColorMap", "_Albedo"]);
        let emission_on = keywords.contains(&"_EMISSION")
            || body["m_LightmapFlags"]
                .as_i64()
                .is_some_and(|f| f & 2 != 0 && f & 4 == 0);
        let emission = if emission_on {
            color(&["_EmissionColor"]).map_or([0.0; 3], |c| [c[0], c[1], c[2]])
        } else {
            [0.0; 3]
        };
        let cutout = float(&["_AlphaClip"], 0.0) >= 0.5 || keywords.contains(&"_ALPHATEST_ON");
        let surface = if float(&["_Surface"], 0.0) >= 0.5 {
            Surface::Transparent
        } else if cutout {
            Surface::Cutout(float(&["_Cutoff"], 0.5))
        } else {
            Surface::Opaque
        };
        let mut floats = BTreeMap::new();
        for (k, v) in entries(&props["m_Floats"]) {
            if let Some(f) = v.as_f32() {
                floats.insert(k.to_string(), f);
            }
        }
        let mut colors = BTreeMap::new();
        for (k, v) in entries(&props["m_Colors"]) {
            if let Some(c) = v.color() {
                colors.insert(k.to_string(), c);
            }
        }
        let mut textures = BTreeMap::new();
        for (k, v) in entries(&props["m_TexEnvs"]) {
            if let Some(r) = v["m_Texture"].obj_ref().filter(|r| !r.is_none() && r.guid.is_some()) {
                textures.insert(
                    k.to_string(),
                    TextureProperty {
                        texture: r,
                        scale: v["m_Scale"].vec2().unwrap_or([1.0; 2]),
                        offset: v["m_Offset"].vec2().unwrap_or([0.0; 2]),
                    },
                );
            }
        }
        Ok(Self {
            name: body["m_Name"].as_str().unwrap_or("").to_string(),
            shader: shader.guid.clone(),
            shader_file_id: shader.file_id,
            color: color(&["_BaseColor", "_Color"]).unwrap_or([1.0; 4]),
            base_map: base.as_ref().map(|b| b.0.clone()),
            tiling: base.as_ref().map_or([1.0; 2], |b| b.1),
            offset: base.as_ref().map_or([0.0; 2], |b| b.2),
            normal_map: texture(&["_BumpMap", "_NormalMap"]).map(|t| t.0),
            normal_scale: float(&["_BumpScale", "_NormalScale"], 1.0),
            metallic: float(&["_Metallic"], 0.0),
            smoothness: float(&["_Smoothness", "_Glossiness"], 0.5),
            metallic_map: texture(&["_MetallicGlossMap"]).map(|t| t.0),
            smoothness_in_base_alpha: float(&["_SmoothnessTextureChannel"], 0.0) >= 0.5,
            occlusion_map: texture(&["_OcclusionMap"]).map(|t| t.0),
            occlusion_strength: float(&["_OcclusionStrength"], 1.0),
            emission,
            emission_map: if emission_on {
                texture(&["_EmissionMap"]).map(|t| t.0)
            } else {
                None
            },
            surface,
            double_sided: float(&["_Cull"], 2.0) < 0.5,
            floats,
            colors,
            textures,
        })
    }
}

/// Every property of a list (as [`find`] reads them), in order.
fn entries(list: &Value) -> Vec<(&str, &Value)> {
    match list {
        Value::Seq(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::Map(entries) => entries.first().map(|(k, v)| (k.as_str(), v)),
                _ => None,
            })
            .collect(),
        Value::Map(entries) => entries.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        _ => Vec::new(),
    }
}

/// Unity keeps properties as a list of one-key maps: `- _Name: value`.
fn find<'a>(list: &'a Value, name: &str) -> Option<&'a Value> {
    match list {
        Value::Seq(items) => items.iter().find_map(|item| match item {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }),
        // Older files: a map of name to value.
        Value::Map(entries) => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v),
        _ => None,
    }
}
