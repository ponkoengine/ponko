//! Importing Spine JSON exports (Spine 4.x, and the 3.x spellings that
//! differ). What is and isn't read is listed on
//! [`SkeletonData::from_spine_json`].

use std::sync::Arc;

use glam::Vec2;
use serde_json::{Map, Value};

use super::atlas::SpineAtlas;
use super::clip::{Bezier, BoneTrack, Clip, Curve, Key, SlotTrack};
use super::data::{BonePose, IkConstraint, MeshVertex, SkeletonData, SkeletonError, SkinnedMesh, SpritePart, err};
use super::json::beside;
use crate::color::Color;
use crate::texture::Texture;

fn num(v: &Value, key: &str, default: f32) -> f32 {
    v.get(key).and_then(Value::as_f64).map_or(default, |n| n as f32)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn object<'a>(v: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    v.get(key).and_then(Value::as_object)
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Whether `v[key]` holds something: a non-empty list or object, a true,
/// a non-zero number, a string.
fn present(v: &Value, key: &str) -> bool {
    match v.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(_)) => true,
    }
}

/// Spine's `RRGGBBAA`.
fn color(v: &str) -> Result<Color, SkeletonError> {
    super::json::hex_color(v)
}

/// Spine is y-up with counter-clockwise angles in degrees; Ponko's 2D is
/// y-down with clockwise radians. Mirroring y maps one onto the other.
fn point(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, -y)
}

fn angle(degrees: f32) -> f32 {
    -degrees.to_radians()
}

/// One import: the file, its Spine version, its atlas and what it left out.
struct Import<'a> {
    path: &'a str,
    /// Spine's major version (3 or 4): curves and rotations differ.
    major: u32,
    atlas: Option<&'a SpineAtlas>,
    warnings: Vec<String>,
}

impl Import<'_> {
    /// Records that `feature` isn't read, and what the skeleton does instead.
    fn warn(&mut self, feature: &str, instead: &str) {
        let message = format!("{}: Spine {feature} isn't read; {instead}", self.path);
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    /// Warns about every field of `v` not in `known`.
    fn unknown(&mut self, v: &Value, known: &[&str], what: &str) {
        if let Some(map) = v.as_object() {
            for key in map.keys() {
                if !known.contains(&key.as_str()) {
                    self.warn(&format!("field \"{key}\" ({what})"), "it is left out");
                }
            }
        }
    }
}

impl SkeletonData {
    /// Imports a Spine JSON export (`.json`, Spine 4.x; 3.x files read
    /// too). `path` is its asset name: images are found in the export's
    /// `skeleton.images` folder (relative to the file), or next to the
    /// file, as `<attachment path or name>.png`, one image per attachment.
    /// With a texture atlas, use
    /// [`from_spine_json_with_atlas`](Self::from_spine_json_with_atlas)
    /// ([`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d) finds
    /// the atlas by itself).
    ///
    /// Read: bones (position, rotation, scale, length), slots (rest
    /// attachment and color), every skin's region and mesh attachments
    /// (weighted meshes keep each vertex's four strongest bones), IK
    /// constraints (two bones, or one as a look-at), and animations'
    /// bone rotate/translate/scale timelines, slot attachment and color
    /// timelines, draw order and events. Curves are exact: stepped,
    /// linear, and bezier (4.x handles in seconds and values; 3.x
    /// normalized ones), per channel; 3.x rotations keep turning the
    /// shorter way, as 3.x plays them.
    ///
    /// Not read, each reported in [`warnings`](Self::warnings) with the
    /// file and where (the file still loads, without it): shear, bone
    /// inherit modes other than `normal`, skin-required bones, slot blend
    /// modes and dark colors, linked meshes, sequences, bounding boxes,
    /// paths, points, clipping, transform/path/physics constraints, skin
    /// constraints, IK softness/compress/stretch, deform timelines,
    /// separate x/y timelines, shear and inherit timelines, `rgb`,
    /// `alpha` and two-color timelines, IK and other constraint timelines,
    /// event values (events fire by name), and any field this importer
    /// doesn't know.
    ///
    /// ```
    /// use ponko::skeleton2d::SkeletonData;
    /// let data = SkeletonData::from_spine_json(
    ///     r#"{ "skeleton": { "spine": "4.2.0" },
    ///          "bones": [{ "name": "root" }, { "name": "head", "parent": "root", "y": 40, "shearX": 5 }] }"#,
    ///     "hero.json",
    /// )
    /// .unwrap();
    /// // Spine's y-up becomes Ponko's y-down.
    /// assert_eq!(data.bones()[1].rest.translation.y, -40.0);
    /// assert!(data.warnings()[0].starts_with("hero.json: Spine shear"));
    /// ```
    ///
    /// # Errors
    /// If it isn't JSON or names something that isn't there.
    pub fn from_spine_json(text: &str, path: &str) -> Result<Self, SkeletonError> {
        let value: Value = serde_json::from_str(text).map_err(|e| err(e.to_string()))?;
        Self::from_spine_value(&value, path, None)
    }

    /// [`from_spine_json`](Self::from_spine_json) with its attachments'
    /// images taken from the regions of `atlas` (Spine's packer output):
    /// each region and mesh attachment uses the region named by its
    /// `path` (or name), rotated or trimmed regions included, so it draws
    /// as Spine draws it.
    ///
    /// ```
    /// use ponko::skeleton2d::{SkeletonData, SpineAtlas};
    /// let atlas = SpineAtlas::parse("hero.png\nsize: 32, 32\nhead\n  bounds: 0, 0, 10, 12\n", "hero.atlas").unwrap();
    /// let data = SkeletonData::from_spine_json_with_atlas(
    ///     r#"{ "bones": [{ "name": "root" }], "slots": [{ "name": "head", "bone": "root", "attachment": "head" }],
    ///          "skins": [{ "name": "default", "attachments": { "head": { "head": { "width": 10, "height": 12 } } } }] }"#,
    ///     "hero.json",
    ///     &atlas,
    /// )
    /// .unwrap();
    /// assert_eq!(data.images(), ["hero.png"]);
    /// ```
    ///
    /// # Errors
    /// As `from_spine_json`, and an attachment whose region isn't in the
    /// atlas, or a mesh on a page whose size the atlas doesn't give.
    pub fn from_spine_json_with_atlas(text: &str, path: &str, atlas: &SpineAtlas) -> Result<Self, SkeletonError> {
        let value: Value = serde_json::from_str(text).map_err(|e| err(e.to_string()))?;
        Self::from_spine_value(&value, path, Some(atlas))
    }

    pub(crate) fn from_spine_value(v: &Value, path: &str, atlas: Option<&SpineAtlas>) -> Result<Self, SkeletonError> {
        let version = v.get("skeleton").and_then(|s| text(s, "spine")).unwrap_or("4");
        let major = version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(4);
        let mut import = Import {
            path,
            major,
            atlas,
            warnings: Vec::new(),
        };
        let mut data = read(v, &mut import)?;
        data.warnings = import.warnings;
        Ok(data)
    }
}

fn read(v: &Value, import: &mut Import) -> Result<SkeletonData, SkeletonError> {
    let path = import.path;
    import.unknown(
        v,
        &[
            "skeleton",
            "bones",
            "slots",
            "ik",
            "transform",
            "path",
            "physics",
            "skins",
            "events",
            "animations",
        ],
        "the file",
    );
    for (key, kind) in [("transform", "transform"), ("path", "path"), ("physics", "physics")] {
        if present(v, key) {
            import.warn(
                &format!("{kind} constraints"),
                "the bones they would move keep their animated pose",
            );
        }
    }
    for (name, e) in v.get("events").and_then(Value::as_object).into_iter().flatten() {
        if ["int", "float", "string", "audio"].iter().any(|k| present(e, k)) {
            import.warn(
                &format!("event values (event \"{name}\")"),
                "the event fires by name only",
            );
        }
    }
    let mut data = SkeletonData::new();
    let images = v
        .get("skeleton")
        .and_then(|s| text(s, "images"))
        .map(|dir| dir.trim_start_matches("./").trim_end_matches('/').to_string());
    let image = |name: &str| -> Arc<str> {
        let file = format!("{name}.png");
        match &images {
            Some(dir) if !dir.is_empty() => beside(path, &format!("{dir}/{file}")),
            _ => beside(path, &file),
        }
        .into()
    };
    for b in array(v, "bones") {
        let name = text(b, "name").ok_or_else(|| err("a bone has no name"))?;
        import.unknown(
            b,
            &[
                "name",
                "parent",
                "length",
                "x",
                "y",
                "rotation",
                "scaleX",
                "scaleY",
                "shearX",
                "shearY",
                "inherit",
                "transform",
                "skin",
                "color",
                "icon",
                "visible",
            ],
            &format!("bone \"{name}\""),
        );
        if present(b, "shearX") || present(b, "shearY") {
            import.warn(&format!("shear (bone \"{name}\")"), "the bone is posed without it");
        }
        if let Some(mode) = text(b, "inherit").or(text(b, "transform"))
            && mode != "normal"
        {
            import.warn(
                &format!("inherit mode \"{mode}\" (bone \"{name}\")"),
                "the bone inherits its parent's rotation and scale as \"normal\" does",
            );
        }
        if present(b, "skin") {
            import.warn(
                &format!("skin-required bones (bone \"{name}\")"),
                "the bone is active in every skin",
            );
        }
        let parent = match text(b, "parent") {
            Some(p) => Some(data.bone(p).ok_or_else(|| err(format!("no bone named \"{p}\"")))?),
            None => None,
        };
        if data.bone(name).is_some() {
            return Err(err(format!("two bones are named \"{name}\"")));
        }
        let rest = BonePose {
            translation: point(num(b, "x", 0.0), num(b, "y", 0.0)),
            rotation: angle(num(b, "rotation", 0.0)),
            scale: Vec2::new(num(b, "scaleX", 1.0), num(b, "scaleY", 1.0)),
        };
        data.add_bone(name, parent, rest, num(b, "length", 0.0));
    }
    let mut rest_attachments = Vec::new();
    for s in array(v, "slots") {
        let name = text(s, "name").ok_or_else(|| err("a slot has no name"))?;
        import.unknown(
            s,
            &["name", "bone", "color", "dark", "attachment", "blend", "visible"],
            &format!("slot \"{name}\""),
        );
        if present(s, "dark") {
            import.warn(
                &format!("dark color (slot \"{name}\")"),
                "the slot is tinted by its color alone",
            );
        }
        if let Some(blend) = text(s, "blend")
            && blend != "normal"
        {
            import.warn(
                &format!("blend mode \"{blend}\" (slot \"{name}\")"),
                "the slot draws with its entity's blend mode",
            );
        }
        if s.get("visible").and_then(Value::as_bool) == Some(false) {
            import.warn(&format!("hidden slots (slot \"{name}\")"), "the slot is shown");
        }
        let bone = text(s, "bone").ok_or_else(|| err(format!("slot \"{name}\" has no bone")))?;
        let bone = data
            .bone(bone)
            .ok_or_else(|| err(format!("no bone named \"{bone}\"")))?;
        if data.slot(name).is_some() {
            return Err(err(format!("two slots are named \"{name}\"")));
        }
        let slot = data.add_slot(name, bone);
        let c = text(s, "color").map(color).transpose()?.unwrap_or(Color::WHITE);
        data.set_slot_rest(slot, None, c);
        rest_attachments.push(text(s, "attachment").map(str::to_string));
    }
    // 4.x: a list of {name, attachments}; 3.x: an object by skin name.
    let mut skins: Vec<(&str, &Map<String, Value>)> = Vec::new();
    match v.get("skins") {
        Some(Value::Array(list)) => {
            for s in list {
                let name = text(s, "name").unwrap_or("default");
                import.unknown(
                    s,
                    &[
                        "name",
                        "attachments",
                        "bones",
                        "ik",
                        "transform",
                        "path",
                        "physics",
                        "color",
                    ],
                    &format!("skin \"{name}\""),
                );
                if ["bones", "ik", "transform", "path", "physics"]
                    .iter()
                    .any(|k| present(s, k))
                {
                    import.warn(
                        &format!("skin bones and constraints (skin \"{name}\")"),
                        "they are active in every skin",
                    );
                }
                if let Some(a) = object(s, "attachments") {
                    skins.push((name, a));
                }
            }
        }
        Some(Value::Object(map)) => {
            for (name, a) in map {
                if let Some(a) = a.as_object() {
                    skins.push((name, a));
                }
            }
        }
        _ => {}
    }
    for (skin_name, slots) in skins {
        let skin = if skin_name == "default" {
            0
        } else {
            data.skin(skin_name).unwrap_or_else(|| data.add_skin(skin_name))
        };
        for (slot_name, attachments) in slots {
            let slot = data.slot(slot_name).ok_or_else(|| {
                err(format!(
                    "skin \"{skin_name}\" names slot \"{slot_name}\", which isn't there"
                ))
            })?;
            let Some(attachments) = attachments.as_object() else {
                continue;
            };
            for (name, a) in attachments {
                let kind = text(a, "type").unwrap_or("region");
                let place = format!("attachment \"{name}\" in slot \"{slot_name}\"");
                if present(a, "sequence") {
                    import.warn(&format!("sequence ({place})"), "its first image shows throughout");
                }
                let source = text(a, "path").or(text(a, "name")).unwrap_or(name);
                let c = text(a, "color").map(color).transpose()?.unwrap_or(Color::WHITE);
                match kind {
                    "region" => {
                        import.unknown(
                            a,
                            &[
                                "type", "name", "path", "x", "y", "rotation", "scaleX", "scaleY", "width", "height",
                                "color", "sequence",
                            ],
                            &place,
                        );
                        let size = Vec2::new(num(a, "width", 0.0), num(a, "height", 0.0));
                        let mut part = SpritePart::new(Texture::new(u32::MAX, 0, 0))
                            .with_size(size)
                            .with_offset(BonePose {
                                translation: point(num(a, "x", 0.0), num(a, "y", 0.0)),
                                rotation: angle(num(a, "rotation", 0.0)),
                                scale: Vec2::new(num(a, "scaleX", 1.0), num(a, "scaleY", 1.0)),
                            });
                        match import.atlas {
                            Some(atlas) => {
                                let r = region(atlas, source, &place)?;
                                let page = &atlas.pages[r.page];
                                let whole = Vec2::new(r.original_width, r.original_height);
                                let size = if size == Vec2::ZERO { whole } else { size };
                                let k = size / whole;
                                part.size = Vec2::new(r.width, r.height) * k;
                                part.pivot = Vec2::new(
                                    (r.original_width / 2.0 - r.offset_x) / r.width,
                                    (r.offset_y + r.height - r.original_height / 2.0) / r.height,
                                );
                                part.region = Some(r.packed());
                                part.rotated = r.rotated;
                                part.image = page.image.clone();
                            }
                            None => part.image = image(source),
                        }
                        part.color = c;
                        data.add_skin_attachment(skin, slot, name, part);
                    }
                    "mesh" => {
                        import.unknown(
                            a,
                            &[
                                "type",
                                "name",
                                "path",
                                "uvs",
                                "triangles",
                                "vertices",
                                "hull",
                                "edges",
                                "width",
                                "height",
                                "color",
                                "sequence",
                            ],
                            &place,
                        );
                        let mut mesh = spine_mesh(&data, slot, name, a)?;
                        match import.atlas {
                            Some(atlas) => {
                                let r = region(atlas, source, &place)?;
                                let page = &atlas.pages[r.page];
                                let size = page.size.ok_or_else(|| {
                                    err(format!(
                                        "{}: page {} needs a \"size\" for mesh {place}",
                                        atlas.path(),
                                        page.image
                                    ))
                                })?;
                                for v in &mut mesh.vertices {
                                    v.uv = r.mesh_uv(v.uv, size);
                                }
                                mesh.image = page.image.clone();
                            }
                            None => mesh.image = image(source),
                        }
                        mesh.color = c;
                        data.add_skin_attachment(skin, slot, name, mesh);
                    }
                    "linkedmesh" => import.warn(
                        &format!("linked mesh ({place})"),
                        "the slot shows nothing for this attachment",
                    ),
                    "boundingbox" => import.warn(&format!("bounding box ({place})"), "it has no shape in Ponko"),
                    "path" => import.warn(&format!("path attachment ({place})"), "it has no shape in Ponko"),
                    "point" => import.warn(&format!("point attachment ({place})"), "it has no position in Ponko"),
                    "clipping" => import.warn(&format!("clipping ({place})"), "the slots it would clip draw whole"),
                    other => import.warn(
                        &format!("attachment type \"{other}\" ({place})"),
                        "the slot shows nothing for this attachment",
                    ),
                }
            }
        }
    }
    for (slot, name) in rest_attachments.into_iter().enumerate() {
        data.slots[slot].attachment = name.filter(|n| data.find(0, slot, n).is_some()).map(Arc::from);
    }
    let mut constraints: Vec<&Value> = array(v, "ik").iter().collect();
    constraints.sort_by_key(|ik| ik.get("order").and_then(Value::as_i64).unwrap_or(0));
    for ik in constraints {
        let name = text(ik, "name").unwrap_or("ik");
        let place = format!("IK constraint \"{name}\"");
        import.unknown(
            ik,
            &[
                "name",
                "order",
                "skin",
                "bones",
                "target",
                "mix",
                "softness",
                "bendPositive",
                "compress",
                "stretch",
                "uniform",
            ],
            &place,
        );
        for (key, what) in [
            ("softness", "softness"),
            ("compress", "compress"),
            ("stretch", "stretch"),
            ("uniform", "uniform scale"),
        ] {
            if present(ik, key) {
                import.warn(&format!("IK {what} ({place})"), "the chain bends without it");
            }
        }
        if present(ik, "skin") {
            import.warn(
                &format!("skin-required constraints ({place})"),
                "it applies in every skin",
            );
        }
        let bones = array(ik, "bones")
            .iter()
            .map(|b| {
                let b = b.as_str().unwrap_or("");
                data.bone(b).ok_or_else(|| err(format!("no bone named \"{b}\"")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target = text(ik, "target").unwrap_or("");
        let target = data
            .bone(target)
            .ok_or_else(|| err(format!("no bone named \"{target}\"")))?;
        let mut constraint = IkConstraint::new(name, &bones, target).with_mix(num(ik, "mix", 1.0));
        // Spine's positive bend is counter-clockwise (y-up), which is the
        // negative way on a y-down screen.
        constraint.bend_positive = !ik.get("bendPositive").and_then(Value::as_bool).unwrap_or(true);
        data.check_ik(&constraint)?;
        data.ik.push(constraint);
    }
    if let Some(animations) = object(v, "animations") {
        for (name, a) in animations {
            let clip = spine_clip(&data, name, a, import)?;
            clip.check(data.bones.len(), data.slots.len())?;
            data.clips.push(clip);
        }
    }
    if let Some(atlas) = import.atlas {
        let used: Vec<String> = data.images().into_iter().map(String::from).collect();
        for page in &atlas.pages {
            if used.iter().any(|u| *u == *page.image) {
                data.premultiplied |= page.premultiplied;
                if page.nearest {
                    data.nearest.push(page.image.clone());
                }
            }
        }
    }
    Ok(data)
}

fn region<'a>(atlas: &'a SpineAtlas, name: &str, place: &str) -> Result<&'a super::atlas::AtlasRegion, SkeletonError> {
    atlas
        .region(name)
        .ok_or_else(|| err(format!("{} has no region \"{name}\" for {place}", atlas.path())))
}

fn spine_mesh(data: &SkeletonData, slot: usize, name: &str, a: &Value) -> Result<SkinnedMesh, SkeletonError> {
    let floats = |key: &str| -> Vec<f32> {
        array(a, key)
            .iter()
            .filter_map(Value::as_f64)
            .map(|n| n as f32)
            .collect()
    };
    let uvs = floats("uvs");
    let raw = floats("vertices");
    let triangles: Vec<u16> = array(a, "triangles")
        .iter()
        .filter_map(Value::as_u64)
        .map(|n| n as u16)
        .collect();
    let count = uvs.len() / 2;
    let bad = || err(format!("mesh \"{name}\" has vertices that don't match its uvs"));
    let mut vertices = Vec::with_capacity(count);
    if raw.len() == uvs.len() {
        // Unweighted: positions in the slot's bone's space.
        let bone = data.slots[slot].bone;
        let rest = data.rest_world[bone].0;
        for i in 0..count {
            let at = rest.transform_point2(point(raw[2 * i], raw[2 * i + 1]));
            vertices.push(MeshVertex::new(
                at,
                Vec2::new(uvs[2 * i], uvs[2 * i + 1]),
                &[(bone, 1.0)],
            ));
        }
    } else {
        // Weighted: per vertex, a bone count, then (bone, x, y, weight).
        let mut at = 0;
        for i in 0..count {
            let n = *raw.get(at).ok_or_else(bad)? as usize;
            at += 1;
            let mut position = Vec2::ZERO;
            let mut influences = Vec::with_capacity(n);
            for _ in 0..n {
                let entry = raw.get(at..at + 4).ok_or_else(bad)?;
                at += 4;
                let bone = entry[0] as usize;
                if bone >= data.bones.len() {
                    return Err(bad());
                }
                position += data.rest_world[bone].0.transform_point2(point(entry[1], entry[2])) * entry[3];
                influences.push((bone, entry[3]));
            }
            if !influences.iter().any(|(_, w)| *w > 0.0) {
                return Err(bad());
            }
            vertices.push(MeshVertex::new(
                position,
                Vec2::new(uvs[2 * i], uvs[2 * i + 1]),
                &influences,
            ));
        }
    }
    if !triangles.len().is_multiple_of(3) || triangles.iter().any(|&t| usize::from(t) >= vertices.len()) {
        return Err(err(format!("mesh \"{name}\"'s triangles don't match its vertices")));
    }
    Ok(SkinnedMesh::new(Texture::new(u32::MAX, 0, 0), vertices, triangles))
}

// ---- curves ---------------------------------------------------------------------

/// How one Spine key goes to the next, before it is normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Segment<const N: usize> {
    Linear,
    Stepped,
    /// Spine 3.x: one normalized bezier for every channel.
    Normalized([f32; 4]),
    /// Spine 4.x, and 3.x once converted: per channel, the handles
    /// (time, value, time, value) in seconds and the channel's units.
    Absolute([[f64; 4]; N]),
}

/// A Spine key: its time, its channels' values (Spine's units) and curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame<const N: usize> {
    time: f64,
    values: [f64; N],
    segment: Segment<N>,
}

/// A key's curve as Spine writes it: `"stepped"`, 4.x's array of four
/// handle numbers per channel, or 3.x's `curve` number with `c2`..`c4`
/// (or its older four-number array), normalized.
fn segment<const N: usize>(key: &Value, major: u32) -> Result<Segment<N>, SkeletonError> {
    let bad = || err(format!("a curve needs {} numbers", 4 * N));
    Ok(match key.get("curve") {
        None | Some(Value::Null) => Segment::Linear,
        Some(Value::String(s)) if s == "stepped" => Segment::Stepped,
        Some(Value::String(s)) if s == "linear" => Segment::Linear,
        Some(Value::Number(n)) => {
            let cx1 = n.as_f64().unwrap_or(0.0) as f32;
            Segment::Normalized([cx1, num(key, "c2", 0.0), num(key, "c3", 1.0), num(key, "c4", 1.0)])
        }
        Some(Value::Array(list)) => {
            let n: Vec<f64> = list.iter().filter_map(Value::as_f64).collect();
            if major < 4 {
                if n.len() != 4 {
                    return Err(bad());
                }
                Segment::Normalized([n[0] as f32, n[1] as f32, n[2] as f32, n[3] as f32])
            } else {
                // One set of handles per channel; one set for all is read too.
                if n.len() != 4 * N && n.len() != 4 {
                    return Err(bad());
                }
                let mut handles = [[0.0; 4]; N];
                for (c, h) in handles.iter_mut().enumerate() {
                    let at = if n.len() == 4 { 0 } else { 4 * c };
                    h.copy_from_slice(&n[at..at + 4]);
                }
                Segment::Absolute(handles)
            }
        }
        Some(_) => return Err(bad()),
    })
}

/// A cubic from `a` to `d` through `b` and `c`, at `s`.
fn cubic(a: f64, b: f64, c: f64, d: f64, s: f64) -> f64 {
    let r = 1.0 - s;
    r * r * r * a + 3.0 * r * r * s * b + 3.0 * r * s * s * c + s * s * s * d
}

/// Spine's frames as Ponko keys: normalized beziers, segments whose
/// channel doesn't move but bulges split where it has moved (so the bulge
/// survives as two curves). `map` turns Spine's values into Ponko's (a
/// linear map per channel, which leaves normalized curves as they are).
fn keys<const N: usize, T>(
    mut frames: Vec<Frame<N>>,
    import: &mut Import,
    place: &str,
    map: impl Fn([f64; N]) -> T,
) -> Vec<Key<T>> {
    // 3.x: handles relative to each segment, made absolute.
    for i in 0..frames.len() {
        if let Segment::Normalized([x1, y1, x2, y2]) = frames[i].segment {
            frames[i].segment = match frames.get(i + 1) {
                None => Segment::Linear,
                Some(next) => {
                    let (t0, t1) = (frames[i].time, next.time);
                    let mut handles = [[0.0; 4]; N];
                    for (c, h) in handles.iter_mut().enumerate() {
                        let (v0, v1) = (frames[i].values[c], next.values[c]);
                        *h = [
                            t0 + f64::from(x1) * (t1 - t0),
                            v0 + f64::from(y1) * (v1 - v0),
                            t0 + f64::from(x2) * (t1 - t0),
                            v0 + f64::from(y2) * (v1 - v0),
                        ];
                    }
                    Segment::Absolute(handles)
                }
            };
        }
    }
    // Split where a channel with equal ends bulges.
    let mut i = 0;
    let mut splits = 0;
    while i + 1 < frames.len() {
        if let Some(split) = bulge(&frames[i], &frames[i + 1])
            && splits < 256
        {
            let (left, middle) = split;
            frames[i].segment = Segment::Absolute(left);
            frames.insert(i + 1, middle);
            splits += 1;
            continue;
        }
        i += 1;
    }
    if splits >= 256 {
        import.warn(
            &format!("a bezier curve this importer couldn't split ({place})"),
            "parts of it are read as straight",
        );
    }
    let mut out = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let curve = match (f.segment, frames.get(i + 1)) {
            (Segment::Stepped, _) => Curve::Stepped,
            (Segment::Absolute(handles), Some(next)) => normalized(f, next, &handles),
            _ => Curve::Linear,
        };
        out.push(Key::new(f.time as f32, map(f.values)).with_curve(curve));
    }
    out
}

/// A segment's absolute handles as a normalized [`Curve`].
fn normalized<const N: usize>(from: &Frame<N>, to: &Frame<N>, handles: &[[f64; 4]; N]) -> Curve {
    let span = to.time - from.time;
    if span <= 0.0 {
        return Curve::Linear;
    }
    let mut channels = [Bezier::LINEAR; 4];
    for (c, h) in handles.iter().enumerate() {
        let (v0, v1) = (from.values[c], to.values[c]);
        let change = v1 - v0;
        let y = |cy: f64| {
            if change != 0.0 {
                ((cy - v0) / change) as f32
            } else {
                0.0
            }
        };
        channels[c] = Bezier::new(
            ((h[0] - from.time) / span).clamp(0.0, 1.0) as f32,
            y(h[1]),
            ((h[2] - from.time) / span).clamp(0.0, 1.0) as f32,
            y(h[3]),
        );
    }
    if channels[..N].iter().all(|b| *b == channels[0]) {
        Curve::Bezier(channels[0])
    } else {
        Curve::Channels(channels)
    }
}

/// If a channel of the segment from `a` to `b` starts and ends at the same
/// value but its handles leave it (a normalized curve can't hold that),
/// the segment split where that channel has moved: the left part's
/// handles and the key in the middle (with the right part's handles).
#[allow(clippy::type_complexity)]
fn bulge<const N: usize>(a: &Frame<N>, b: &Frame<N>) -> Option<([[f64; 4]; N], Frame<N>)> {
    let Segment::Absolute(handles) = a.segment else {
        return None;
    };
    let (t0, t1) = (a.time, b.time);
    if t1 <= t0 {
        return None;
    }
    let flat = |c: usize| {
        let (v0, v1) = (a.values[c], b.values[c]);
        let h = handles[c];
        let scale = v0.abs().max(v1.abs()).max(h[1].abs()).max(h[3].abs()).max(1e-6);
        let eps = 1e-9 * scale;
        (v1 - v0).abs() <= eps && ((h[1] - v0).abs() > eps || (h[3] - v0).abs() > eps)
    };
    let c = (0..N).find(|&c| flat(c))?;
    let h = handles[c];
    let v0 = a.values[c];
    let moved = |s: f64| cubic(v0, h[1], h[3], b.values[c], s);
    let s = [0.5, 1.0 / 3.0, 2.0 / 3.0]
        .into_iter()
        .find(|&s| (moved(s) - v0).abs() > 1e-9 * v0.abs().max(1e-6))?;
    let time = cubic(t0, h[0], h[2], t1, s);
    if !(time > t0 && time < t1) {
        return None;
    }
    let mut left = [[0.0; 4]; N];
    let mut right = [[0.0; 4]; N];
    let mut values = [0.0; N];
    for k in 0..N {
        let h = handles[k];
        // Where this channel's own curve reaches `time`.
        let x = Bezier::new(
            ((h[0] - t0) / (t1 - t0)) as f32,
            0.0,
            ((h[2] - t0) / (t1 - t0)) as f32,
            0.0,
        );
        let sk = if k == c {
            s
        } else {
            f64::from(x.parameter_at(((time - t0) / (t1 - t0)) as f32))
        };
        // De Casteljau: the same curve as two curves, exactly.
        let p = [(t0, a.values[k]), (h[0], h[1]), (h[2], h[3]), (t1, b.values[k])];
        let lerp = |p: (f64, f64), q: (f64, f64)| (p.0 + (q.0 - p.0) * sk, p.1 + (q.1 - p.1) * sk);
        let (ab, bc, cd) = (lerp(p[0], p[1]), lerp(p[1], p[2]), lerp(p[2], p[3]));
        let (abc, bcd) = (lerp(ab, bc), lerp(bc, cd));
        let m = lerp(abc, bcd);
        left[k] = [ab.0, ab.1, abc.0, abc.1];
        right[k] = [bcd.0, bcd.1, cd.0, cd.1];
        values[k] = m.1;
    }
    Some((
        left,
        Frame {
            time,
            values,
            segment: Segment::Absolute(right),
        },
    ))
}

/// The frames of one Spine timeline.
fn frames<const N: usize>(
    keys: &[Value],
    major: u32,
    value: impl Fn(&Value) -> Result<[f64; N], SkeletonError>,
) -> Result<Vec<Frame<N>>, SkeletonError> {
    keys.iter()
        .map(|k| {
            Ok(Frame {
                time: f64::from(num(k, "time", 0.0)),
                values: value(k)?,
                segment: segment(k, major)?,
            })
        })
        .collect()
}

fn channel(k: &Value, key: &str, default: f32) -> f64 {
    f64::from(num(k, key, default))
}

fn rgba(k: &Value) -> Result<[f64; 4], SkeletonError> {
    let c = color(text(k, "color").unwrap_or("ffffffff"))?;
    Ok([c.r, c.g, c.b, c.a].map(f64::from))
}

// ---- animations -----------------------------------------------------------------

fn spine_clip(data: &SkeletonData, name: &str, a: &Value, import: &mut Import) -> Result<Clip, SkeletonError> {
    let major = import.major;
    let mut clip = Clip::new(name, 0.0);
    let anim = format!("animation \"{name}\"");
    import.unknown(
        a,
        &[
            "bones",
            "slots",
            "drawOrder",
            "draworder",
            "events",
            "ik",
            "transform",
            "path",
            "physics",
            "deform",
            "attachments",
        ],
        &anim,
    );
    for (key, what, instead) in [
        ("ik", "IK timelines", "the constraints keep their mix"),
        ("transform", "transform constraint timelines", "they are left out"),
        ("path", "path constraint timelines", "they are left out"),
        ("physics", "physics constraint timelines", "they are left out"),
        ("deform", "deform timelines", "meshes keep their bound shape"),
        (
            "attachments",
            "deform and sequence timelines",
            "meshes keep their bound shape and sequences their first image",
        ),
    ] {
        if present(a, key) {
            import.warn(&format!("{what} ({anim})"), instead);
        }
    }
    // The clip lasts until its last key, whatever its kind.
    let mut last = 0.0f32;
    let mut times = |keys: &[Value]| {
        for k in keys {
            last = last.max(num(k, "time", 0.0));
        }
    };
    if let Some(bones) = object(a, "bones") {
        for (bone, timelines) in bones {
            let index = data
                .bone(bone)
                .ok_or_else(|| err(format!("{anim}: no bone named \"{bone}\"")))?;
            let place = format!("{anim}, bone \"{bone}\"");
            for (key, _) in timelines.as_object().into_iter().flatten() {
                if !["rotate", "translate", "scale"].contains(&key.as_str()) {
                    import.warn(
                        &format!("\"{key}\" timeline ({place})"),
                        "the bone isn't animated by it",
                    );
                }
            }
            let mut track = BoneTrack::new(index);
            let rotate = array(timelines, "rotate");
            times(rotate);
            // 4.x `value`, 3.x `angle`.
            let mut rotations = frames::<1>(rotate, major, |k| {
                Ok([f64::from(
                    k.get("value").or(k.get("angle")).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                )])
            })?;
            if major < 4 {
                // 3.x turns the shorter way between keys: the same turns,
                // as values that go straight.
                for i in 1..rotations.len() {
                    let previous = rotations[i - 1].values[0];
                    let delta = rotations[i].values[0] - previous;
                    rotations[i].values[0] = previous + (delta + 180.0).rem_euclid(360.0) - 180.0;
                }
            }
            track.rotate = keys(rotations, import, &place, |[d]| angle(d as f32));
            let translate = array(timelines, "translate");
            times(translate);
            let f = frames::<2>(translate, major, |k| Ok([channel(k, "x", 0.0), channel(k, "y", 0.0)]))?;
            track.translate = keys(f, import, &place, |[x, y]| point(x as f32, y as f32));
            let scale = array(timelines, "scale");
            times(scale);
            let f = frames::<2>(scale, major, |k| Ok([channel(k, "x", 1.0), channel(k, "y", 1.0)]))?;
            track.scale = keys(f, import, &place, |[x, y]| Vec2::new(x as f32, y as f32));
            clip.bones.push(track);
        }
    }
    if let Some(slots) = object(a, "slots") {
        for (slot, timelines) in slots {
            let index = data
                .slot(slot)
                .ok_or_else(|| err(format!("{anim}: no slot named \"{slot}\"")))?;
            let place = format!("{anim}, slot \"{slot}\"");
            for (key, _) in timelines.as_object().into_iter().flatten() {
                if !["attachment", "rgba", "color"].contains(&key.as_str()) {
                    import.warn(
                        &format!("\"{key}\" timeline ({place})"),
                        "the slot isn't animated by it",
                    );
                }
            }
            let mut track = SlotTrack::new(index);
            let attachment = array(timelines, "attachment");
            times(attachment);
            for k in attachment {
                track
                    .attachment
                    .push(Key::new(num(k, "time", 0.0), text(k, "name").map(Arc::from)));
            }
            // 4.x `rgba`, 3.x `color`.
            let colors = if timelines.get("rgba").is_some() {
                array(timelines, "rgba")
            } else {
                array(timelines, "color")
            };
            times(colors);
            let f = frames::<4>(colors, major, rgba)?;
            track.color = keys(f, import, &place, |[r, g, b, a]| {
                Color::rgba(r as f32, g as f32, b as f32, a as f32)
            });
            clip.slots.push(track);
        }
    }
    for k in array(a, "drawOrder").iter().chain(array(a, "draworder")) {
        last = last.max(num(k, "time", 0.0));
        let offsets = array(k, "offsets");
        let order = if offsets.is_empty() {
            None
        } else {
            Some(draw_order(data, offsets).map_err(|e| err(format!("{anim}: {e}")))?)
        };
        clip.draw_order
            .push(Key::new(num(k, "time", 0.0), order).with_curve(Curve::Stepped));
    }
    let mut events = Vec::new();
    for k in array(a, "events") {
        last = last.max(num(k, "time", 0.0));
        if ["int", "float", "string", "volume", "balance"]
            .iter()
            .any(|f| present(k, f))
        {
            import.warn(&format!("event values ({anim})"), "the event fires by name only");
        }
        events.push((num(k, "time", 0.0), text(k, "name").unwrap_or("").to_string()));
    }
    clip.duration = last;
    for (t, e) in events {
        clip = clip.with_event(t, &e);
    }
    Ok(clip)
}

/// Spine's draw order key: slots moved by offsets from the rest order, the
/// others keeping their relative order in the remaining places.
fn draw_order(data: &SkeletonData, offsets: &[Value]) -> Result<Arc<[u16]>, SkeletonError> {
    let n = data.slots.len();
    let mut order: Vec<Option<usize>> = vec![None; n];
    let mut unchanged = Vec::with_capacity(n);
    let mut original = 0;
    for o in offsets {
        let slot = text(o, "slot").unwrap_or("");
        let index = data
            .slot(slot)
            .ok_or_else(|| err(format!("no slot named \"{slot}\"")))?;
        while original != index {
            if original >= n {
                return Err(err("draw order offsets out of order"));
            }
            unchanged.push(original);
            original += 1;
        }
        let to = original as i64 + o.get("offset").and_then(Value::as_i64).unwrap_or(0);
        let place = order
            .get_mut(usize::try_from(to).map_err(|_| err("a draw order offset leaves the slots"))?)
            .ok_or_else(|| err("a draw order offset leaves the slots"))?;
        *place = Some(original);
        original += 1;
    }
    unchanged.extend(original..n);
    for i in (0..n).rev() {
        if order[i].is_none() {
            order[i] = unchanged.pop();
        }
    }
    order
        .into_iter()
        .map(|s| s.map(|s| s as u16).ok_or_else(|| err("draw order offsets overlap")))
        .collect()
}
