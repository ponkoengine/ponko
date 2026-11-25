//! Unity AnimationClip transform and blend-shape curves: the editor's
//! plain curves, or the runtime data Unity bakes into a clip.
use crate::baked::{self, SKey};
use crate::{Error, MeshSkin, SkinAnimation, UnityFile, Value};
use glam::{EulerRot, Mat4, Quat};
use std::collections::{HashMap, HashSet};

/// The most samples [`AnimationClip::append_to_skin`] may make: sample
/// times (frames and keys) times curves. About ten minutes at 60 frames a
/// second of 400 curves.
const MAX_SAMPLES: usize = 1 << 24;

/// A Unity `.anim` clip (text or binary serialized) with transform,
/// blend-shape and humanoid curves. Editor curves are used when present; a
/// clip with only baked runtime data (streamed, dense, constant or
/// compressed curves) is decoded from that. Humanoid muscle and body curves
/// become bone rotations through a [`HumanoidAvatar`](crate::HumanoidAvatar)
/// ([`append_to_skin_with_avatar`](Self::append_to_skin_with_avatar)).
#[derive(Debug, Clone)]
pub struct AnimationClip {
    name: String,
    rate: f32,
    start: f32,
    stop: f32,
    looping: bool,
    curves: Vec<Curve>,
    skipped: Vec<String>,
    morphs: Vec<Morph>,
    /// Humanoid curves (class 95) by attribute: muscles, `RootT.x`, ...
    humanoid: Vec<(String, Vec<SKey>)>,
}
/// What a curve targets: a transform path by name, or by Unity's CRC-32.
#[derive(Debug, Clone, PartialEq)]
enum PathRef {
    Name(String),
    Hash(u32),
}
#[derive(Debug, Clone)]
struct Curve {
    path: PathRef,
    kind: usize,
    /// Which of the four components the clip animates; the rest keep the
    /// rest pose.
    mask: [bool; 4],
    keys: Vec<Key>,
}
/// A blend-shape weight curve, named, or only by the hash of its property.
#[derive(Debug, Clone)]
struct Morph {
    name: String,
    hash: Option<u32>,
    curve: Curve,
}
#[derive(Debug, Clone)]
struct Key {
    time: f32,
    value: [f32; 4],
    incoming: [f32; 4],
    outgoing: [f32; 4],
}
fn number(v: &Value) -> f32 {
    match v.as_str().unwrap_or("") {
        ".inf" | "Infinity" => f32::INFINITY,
        "-.inf" | "-Infinity" => f32::NEG_INFINITY,
        s => s.parse().unwrap_or(0.0),
    }
}
fn vector(v: &Value) -> [f32; 4] {
    ["x", "y", "z", "w"].map(|k| number(&v[k]))
}
const HUMANOID_ONLY: &str = "animation clip holds only baked humanoid muscle curves, which name their muscles by index: resave it from the editor so it keeps its muscle curves by name";

impl AnimationClip {
    /// Parses a text YAML `.anim`; weighted tangents are approximated as
    /// unweighted Hermite.
    ///
    /// Editor curves (`m_PositionCurves`, `m_EditorCurves`, ...) are used when
    /// the clip has any, and the baked runtime data next to them is ignored.
    /// Without editor curves the baked streamed, dense and constant curves
    /// (and compressed rotation curves) are decoded.
    ///
    /// ```
    /// let clip = ponko_unity::AnimationClip::parse("--- !u!74 &1\nAnimationClip:\n  m_Name: Idle\n  m_SampleRate: 30\n").unwrap();
    /// ```
    /// # Errors
    /// Returns an error for binary text, malformed YAML or data, invalid timing,
    /// or a clip that holds only humanoid muscle curves.
    pub fn parse(text: &str) -> Result<Self, Error> {
        if !text.trim_start().starts_with(['%', '-']) || text.contains('\0') {
            return Err(Error::decode(
                "animation clip is not Unity text YAML; binary clips go through parse_binary or read",
            ));
        }
        Self::from_file(&UnityFile::parse(text)?)
    }

    /// Parses a binary serialized `.anim` (Unity 2022 or later, saved by the
    /// editor with type trees) into the same fields as the text format.
    ///
    /// ```
    /// assert!(ponko_unity::AnimationClip::parse_binary(b"not a Unity file").is_err());
    /// ```
    /// # Errors
    /// As [`AnimationClip::parse`], and for data that is not a binary serialized file.
    pub fn parse_binary(data: &[u8]) -> Result<Self, Error> {
        Self::from_file(&UnityFile::parse_binary(data)?)
    }

    fn from_file(file: &UnityFile) -> Result<Self, Error> {
        let m = &file
            .of_class(74)
            .next()
            .ok_or_else(|| Error::invalid("expected AnimationClip"))?
            .body;
        let mut curves = Vec::new();
        let mut morphs: Vec<Morph> = Vec::new();
        // The blend shape curves already in `morphs`: (name, path).
        let mut morph_names: HashSet<(String, String)> = HashSet::new();
        let mut skipped = Vec::new();
        if has_data(&m["m_PPtrCurves"]) {
            skipped.push("skipped m_PPtrCurves".into());
        }
        for (kind, field) in ["m_PositionCurves", "m_RotationCurves", "m_ScaleCurves", "m_EulerCurves"]
            .iter()
            .enumerate()
        {
            for entry in m[*field].as_seq().unwrap_or_default() {
                let keys = vector_keys(&entry["curve"], &mut skipped)?;
                if !keys.is_empty() {
                    curves.push(Curve {
                        path: PathRef::Name(entry["path"].as_str().unwrap_or("").into()),
                        kind,
                        mask: [true, true, true, kind == 1],
                        keys,
                    });
                }
            }
        }
        for entry in m["m_CompressedRotationCurves"].as_seq().unwrap_or_default() {
            let keys: Vec<Key> = baked::compressed_rotation(entry)?
                .into_iter()
                .map(|k| Key {
                    time: k.time,
                    value: k.value,
                    incoming: k.incoming,
                    outgoing: k.outgoing,
                })
                .collect();
            if keys.iter().any(|k| k.value.iter().any(|v| !v.is_finite())) {
                return Err(Error::invalid("non-finite animation key"));
            }
            if !keys.is_empty() {
                curves.push(Curve {
                    path: PathRef::Name(entry["m_Path"].as_str().unwrap_or("").into()),
                    kind: 1,
                    mask: [true; 4],
                    keys,
                });
            }
        }
        // Humanoid curves by attribute (the editor lists them twice too).
        let mut humanoid: Vec<(String, Vec<SKey>)> = Vec::new();
        for entry in m["m_FloatCurves"].as_seq().unwrap_or_default() {
            if !blend_shape_curve(entry, &mut morphs, &mut morph_names, &mut skipped)?
                && !humanoid_curve(entry, &mut humanoid, &mut skipped)?
            {
                skipped.push("skipped m_FloatCurves property".into());
            }
        }
        // The editor's own per-property curves: `m_LocalPosition.x`, ...
        let mut groups: Vec<Group> = Vec::new();
        let mut group_of: HashMap<(String, usize), usize> = HashMap::new();
        for field in ["m_EditorCurves", "m_EulerEditorCurves"] {
            for entry in m[field].as_seq().unwrap_or_default() {
                if blend_shape_curve(entry, &mut morphs, &mut morph_names, &mut skipped)?
                    || humanoid_curve(entry, &mut humanoid, &mut skipped)?
                {
                    continue;
                }
                let attribute = entry["attribute"].as_str().unwrap_or("");
                let Some((kind, part)) = transform_property(entry["classID"].as_i64(), attribute) else {
                    skipped.push(format!("skipped {field} property"));
                    continue;
                };
                let keys = scalar_keys(&entry["curve"], &mut skipped)?;
                let path = entry["path"].as_str().unwrap_or("");
                let at = *group_of.entry((path.to_owned(), kind)).or_insert_with(|| {
                    groups.push(Group {
                        path: path.into(),
                        kind,
                        parts: Default::default(),
                    });
                    groups.len() - 1
                });
                groups[at].parts[part] = keys;
            }
        }
        let plain: HashSet<(String, usize)> = curves
            .iter()
            .filter_map(|c| match &c.path {
                PathRef::Name(n) => Some((n.clone(), c.kind)),
                PathRef::Hash(_) => None,
            })
            .collect();
        for g in groups {
            // The plain transform curves win over the editor's for the same target.
            if plain.contains(&(g.path.clone(), g.kind)) {
                continue;
            }
            let named = PathRef::Name(g.path);
            if let Some((mask, keys)) = combine(&g.parts) {
                curves.push(Curve {
                    path: named,
                    kind: g.kind,
                    mask,
                    keys,
                });
            }
        }
        // Only a clip without any curve of its own falls back to the baked
        // runtime data; with them, Unity's baked copy is redundant.
        if curves.is_empty() && morphs.is_empty() && humanoid.is_empty() {
            baked_curves(m, &mut curves, &mut morphs, &mut skipped)?;
        }
        // Each humanoid curve is sampled once per bone it turns: about one.
        let human_keys = humanoid.iter().flat_map(|(_, keys)| keys.iter().map(|k| k.time));
        let settings = [&m["m_AnimationClipSettings"], &m["m_MuscleClip"]]
            .into_iter()
            .find(|s| !s["m_StopTime"].is_null())
            .unwrap_or(&m["m_AnimationClipSettings"]);
        let start = number(&settings["m_StartTime"]);
        let stop = settings["m_StopTime"].as_f32().unwrap_or_else(|| {
            curves
                .iter()
                .chain(morphs.iter().map(|m| &m.curve))
                .flat_map(|c| c.keys.iter().map(|k| k.time))
                .chain(human_keys.clone())
                .fold(start, f32::max)
        });
        let rate = m["m_SampleRate"].as_f32().unwrap_or(60.0);
        if !start.is_finite()
            || !stop.is_finite()
            || stop < start
            || !rate.is_finite()
            || rate <= 0.0
            || (stop - start) * rate > 1_000_000.0
        {
            return Err(Error::invalid("invalid animation clip timing/sample rate"));
        }
        // Sampling makes a value per sample time (each frame and each key)
        // for every curve: bounded, so a short file can't ask for gigabytes.
        let frames = ((stop - start) * rate).ceil() as usize + 1;
        // Key times between the frames, as `append_to_skin` adds them: a
        // key on a frame's time is that frame's sample.
        let on_frame = |t: f32| {
            let i = (t * rate).round();
            i >= 0.0 && (i / rate).min(stop - start) == t
        };
        let mut key_times: Vec<u32> = curves
            .iter()
            .chain(morphs.iter().map(|m| &m.curve))
            .flat_map(|c| c.keys.iter().map(|k| k.time))
            .chain(human_keys)
            .filter(|&t| t >= start && t <= stop)
            // `+ 0.0` makes -0 the same time as 0.
            .map(|t| t - start + 0.0)
            .filter(|&t| !on_frame(t))
            .map(f32::to_bits)
            .collect();
        key_times.sort_unstable();
        key_times.dedup();
        let count = curves.len() + morphs.len() + humanoid.len();
        let samples = (frames + key_times.len()).saturating_mul(count);
        if samples > MAX_SAMPLES {
            return Err(Error::invalid(format!(
                "animation clip too long to sample: {count} curves over {frames} frames and {} key times (at most {MAX_SAMPLES} samples)",
                key_times.len()
            )));
        }
        Ok(Self {
            name: m["m_Name"].as_str().unwrap_or("Clip").into(),
            rate,
            start,
            stop,
            looping: settings["m_LoopTime"].as_bool().unwrap_or(false),
            curves,
            skipped,
            morphs,
            humanoid,
        })
    }

    /// Reads a `.anim` asset, text or binary serialized.
    ///
    /// ```no_run
    /// let clip = ponko_unity::AnimationClip::read("Assets/Walk.anim")?;
    /// # Ok::<(), ponko_unity::Error>(())
    /// ```
    /// # Errors
    /// Reports file errors and what [`AnimationClip::parse`] does.
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| Error::io(path.as_ref(), e))?;
        if crate::binary::is_binary(&bytes) {
            return Self::parse_binary(&bytes);
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::decode("animation clip is neither Unity text YAML nor a binary serialized file"))?;
        Self::parse(text)
    }

    /// Names the clip's blend-shape curves that Unity's runtime data only
    /// identifies by hash, using the mesh's blend-shape names. Curves whose
    /// hash matches none of the names stay unresolved and are reported by
    /// `append_to_skin`.
    ///
    /// ```
    /// # use ponko_unity::AnimationClip;
    /// let clip = AnimationClip::parse("--- !u!74 &1\nAnimationClip: {}\n").unwrap();
    /// let named = clip.with_blend_shape_names(["Smile", "Blink"]);
    /// ```
    #[must_use]
    pub fn with_blend_shape_names<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Self {
        let hashes: Vec<(u32, &str)> = names
            .into_iter()
            .map(|n| (baked::crc32(&format!("blendShape.{n}")), n))
            .collect();
        let mut clip = self.clone();
        for morph in &mut clip.morphs {
            if let (true, Some(hash)) = (morph.name.is_empty(), morph.hash)
                && let Some((_, name)) = hashes.iter().find(|(h, _)| *h == hash)
            {
                morph.name = (*name).into();
            }
        }
        clip
    }

    /// Appends sampled local poses in Unity space, returning skipped/unresolved diagnostics.
    /// `root` is the node animated by an empty path. Other paths walk child names
    /// (or match the CRC-32 of the path, for runtime data).
    /// Export through `Importer::animation_clips` to apply the normal Z mirror.
    ///
    /// ```
    /// # use ponko_unity::{AnimationClip, MeshSkin};
    /// # let clip = AnimationClip::parse("--- !u!74 &1\nAnimationClip: {}\n").unwrap();
    /// let mut skin = MeshSkin::default();
    /// let diagnostics = clip.append_to_skin(&mut skin, 0);
    /// ```
    pub fn append_to_skin(&self, skin: &mut MeshSkin, root: usize) -> Vec<String> {
        self.append(skin, root, None)
    }

    /// Whether the clip has humanoid curves (muscles, body position and
    /// rotation): it then needs an avatar,
    /// [`append_to_skin_with_avatar`](Self::append_to_skin_with_avatar).
    ///
    /// ```
    /// # use ponko_unity::AnimationClip;
    /// let clip = AnimationClip::parse("--- !u!74 &1\nAnimationClip: {}\n").unwrap();
    /// assert!(!clip.is_humanoid());
    /// ```
    #[must_use]
    pub fn is_humanoid(&self) -> bool {
        !self.humanoid.is_empty()
    }

    /// [`append_to_skin`](Self::append_to_skin), turning the clip's humanoid
    /// curves into rotations of the bones `avatar` names (and the hips'
    /// position): each muscle's value between its limits, measured from
    /// the rest pose (taken as a T-pose), about axes the rest pose gives.
    /// Muscles without a curve are 0, as in Unity, which isn't the rest
    /// pose for every bone. Transform curves on the same nodes win.
    ///
    /// ```
    /// # use ponko_unity::{AnimationClip, HumanoidAvatar, MeshSkin};
    /// # let clip = AnimationClip::parse("--- !u!74 &1\nAnimationClip: {}\n").unwrap();
    /// let mut skin = MeshSkin::default();
    /// let avatar = HumanoidAvatar::detect(&skin);
    /// let diagnostics = clip.append_to_skin_with_avatar(&mut skin, 0, &avatar);
    /// ```
    pub fn append_to_skin_with_avatar(
        &self,
        skin: &mut MeshSkin,
        root: usize,
        avatar: &crate::HumanoidAvatar,
    ) -> Vec<String> {
        self.append(skin, root, Some(avatar))
    }

    fn append(&self, skin: &mut MeshSkin, root: usize, avatar: Option<&crate::HumanoidAvatar>) -> Vec<String> {
        let mut diagnostics = self.skipped.clone();
        let count = ((self.stop - self.start) * self.rate).ceil() as usize;
        let mut times: Vec<_> = (0..=count)
            .map(|i| (i as f32 / self.rate).min(self.stop - self.start))
            .collect();
        // Keep source keys, including discontinuities, alongside regular samples.
        times.extend(
            self.curves
                .iter()
                .chain(self.morphs.iter().map(|m| &m.curve))
                .flat_map(|c| c.keys.iter().map(|k| k.time))
                .chain(self.humanoid.iter().flat_map(|(_, keys)| keys.iter().map(|k| k.time)))
                .filter(|&t| t >= self.start && t <= self.stop)
                .map(|t| t - self.start),
        );
        times.sort_by(f32::total_cmp);
        times.dedup();
        let by_hash = self.path_hashes(skin, root);
        let children = child_index(skin);
        let mut morphs = Vec::new();
        for m in &self.morphs {
            let c = &m.curve;
            if m.name.is_empty() {
                diagnostics.push(format!(
                    "{}: unresolved blend shape hash {:#010x}",
                    self.name,
                    m.hash.unwrap_or(0)
                ));
            } else if resolve(skin, root, &c.path, &by_hash, &children).is_some() {
                morphs.push((
                    m.name.clone(),
                    times.iter().map(|t| c.sample(*t + self.start)[0]).collect(),
                    c.keys
                        .windows(2)
                        .all(|w| w[0].outgoing[0].is_infinite() || w[1].incoming[0].is_infinite()),
                ));
            } else {
                diagnostics.push(format!("{}: unresolved morph path {}", self.name, c.path.describe()));
            }
        }
        let mut animation = SkinAnimation {
            morphs,
            name: self.name.clone(),
            looping: self.looping,
            times,
            nodes: vec![Vec::new(); skin.nodes.len()],
            step: vec![[false; 3]; skin.nodes.len()],
        };
        let mut resolved = Vec::new();
        for curve in &self.curves {
            if let Some(node) = resolve(skin, root, &curve.path, &by_hash, &children) {
                resolved.push((node, curve));
            } else {
                diagnostics.push(format!(
                    "{}: unresolved transform path {}",
                    self.name,
                    curve.path.describe()
                ));
            }
        }
        let mut tracks_of: HashMap<usize, Vec<&Curve>> = HashMap::new();
        for (node, curve) in &resolved {
            tracks_of.entry(*node).or_default().push(curve);
        }
        for (node, rest) in skin.nodes.iter().enumerate() {
            let Some(tracks) = tracks_of.get(&node) else {
                continue;
            };
            for c in tracks {
                animation.step[node][if c.kind == 3 { 1 } else { c.kind }] = c.keys.windows(2).all(|w| {
                    (0..if c.kind == 1 { 4 } else { 3 })
                        .filter(|i| c.mask[*i])
                        .all(|i| w[0].outgoing[i].is_infinite() || w[1].incoming[i].is_infinite())
                });
            }
            let (rest_scale, rest_rotation, rest_position) =
                Mat4::from_cols_array(&rest.matrix).to_scale_rotation_translation();
            let (ey, ex, ez) = rest_rotation.to_euler(EulerRot::YXZ);
            let rest_euler = [ex.to_degrees(), ey.to_degrees(), ez.to_degrees()];
            for &time in &animation.times {
                let (mut s, mut r, mut t) = (rest_scale, rest_rotation, rest_position);
                for c in tracks {
                    let v = c.sample(time + self.start);
                    match c.kind {
                        0 => {
                            for i in (0..3).filter(|i| c.mask[*i]) {
                                t[i] = v[i];
                            }
                        }
                        1 => {
                            let mut q = rest_rotation.to_array();
                            for i in (0..4).filter(|i| c.mask[*i]) {
                                q[i] = v[i];
                            }
                            let q = Quat::from_array(q);
                            r = if q.length_squared() > 0.0 {
                                q.normalize()
                            } else {
                                Quat::IDENTITY
                            };
                        }
                        2 => {
                            for i in (0..3).filter(|i| c.mask[*i]) {
                                s[i] = v[i];
                            }
                        }
                        _ => {
                            let mut e = rest_euler;
                            for i in (0..3).filter(|i| c.mask[*i]) {
                                e[i] = v[i];
                            }
                            r = Quat::from_rotation_y(e[1].to_radians())
                                * Quat::from_rotation_x(e[0].to_radians())
                                * Quat::from_rotation_z(e[2].to_radians())
                        }
                    }
                }
                animation.nodes[node].push(Mat4::from_scale_rotation_translation(s, r, t).to_cols_array());
            }
        }
        if !self.humanoid.is_empty() {
            self.append_humanoid(skin, avatar, &mut animation, &mut diagnostics);
        }
        if !animation.morphs.is_empty() || animation.nodes.iter().any(|n| !n.is_empty()) {
            skin.animations.push(animation);
        }
        diagnostics.sort();
        diagnostics.dedup();
        diagnostics
    }

    /// Samples the humanoid curves into `animation` through `avatar`: every
    /// mapped bone gets a track, except nodes transform curves animate.
    fn append_humanoid(
        &self,
        skin: &MeshSkin,
        avatar: Option<&crate::HumanoidAvatar>,
        animation: &mut SkinAnimation,
        diagnostics: &mut Vec<String>,
    ) {
        let Some(avatar) = avatar else {
            diagnostics.push(format!(
                "{}: humanoid curves need an avatar (the model's Rig set to Humanoid, or Importer::humanoid_avatar)",
                self.name
            ));
            return;
        };
        let missing = avatar.missing();
        let bound = avatar.bind(skin);
        let unbound: Vec<&str> = missing
            .iter()
            .copied()
            .chain(
                crate::humanoid::REQUIRED
                    .into_iter()
                    .filter(|b| avatar.bone(b).is_some_and(|n| !skin.nodes.iter().any(|x| x.name == n))),
            )
            .collect();
        if !unbound.is_empty() {
            diagnostics.push(format!(
                "{}: humanoid curves need the avatar's {} on the skeleton",
                self.name,
                unbound.join(", ")
            ));
            return;
        }
        let find = |name: &str| self.humanoid.iter().find(|(n, _)| n == name).map(|(_, k)| k.as_slice());
        let value = |keys: Option<&[SKey]>, time: f32| keys.map(|k| scalar_at(k, time).0);
        let root_t = ["RootT.x", "RootT.y", "RootT.z"].map(find);
        let root_q = ["RootQ.x", "RootQ.y", "RootQ.z", "RootQ.w"].map(find);
        let muscles: Vec<(&str, &[SKey])> = bound
            .muscles
            .iter()
            .filter_map(|m| find(&m.name).map(|k| (m.name.as_str(), k)))
            .collect();
        let taken: HashSet<usize> = (0..animation.nodes.len())
            .filter(|&n| !animation.nodes[n].is_empty())
            .collect();
        let mut pose = Vec::new();
        for &time in &animation.times {
            let t = time + self.start;
            let position = root_t
                .iter()
                .any(Option::is_some)
                .then(|| glam::Vec3::from(root_t.map(|k| value(k, t).unwrap_or(0.0))));
            let rotation = root_q.iter().any(Option::is_some).then(|| {
                let q = Quat::from_array(root_q.map(|k| value(k, t).unwrap_or(0.0)));
                if q.length_squared() > 1e-8 {
                    q.normalize()
                } else {
                    Quat::IDENTITY
                }
            });
            let muscle = |name: &str| {
                muscles
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or(0.0, |(_, keys)| scalar_at(keys, t).0)
            };
            bound.pose(muscle, (position, rotation), &mut pose);
            for &(node, matrix) in &pose {
                if !taken.contains(&node) {
                    animation.nodes[node].push(matrix.to_cols_array());
                }
            }
        }
    }

    /// Nodes under `root` by the CRC-32 of their slash-separated path, when
    /// some curve is addressed that way.
    fn path_hashes(&self, skin: &MeshSkin, root: usize) -> HashMap<u32, Vec<usize>> {
        let hashed = |c: &Curve| matches!(c.path, PathRef::Hash(_));
        let mut map: HashMap<u32, Vec<usize>> = HashMap::new();
        if root >= skin.nodes.len() || !(self.curves.iter().any(hashed) || self.morphs.iter().any(|m| hashed(&m.curve)))
        {
            return map;
        }
        // Each node's path hash from its parent's, once: a CRC-32 extends
        // over the text after it. `None`: not under `root`.
        let mut state: Vec<Option<Option<u32>>> = vec![None; skin.nodes.len()];
        state[root] = Some(Some(!0));
        for node in 0..skin.nodes.len() {
            // Up to the first node already known (or out of the tree).
            let mut chain = Vec::new();
            let mut at = node;
            let mut base = loop {
                if let Some(known) = state[at] {
                    break known;
                }
                chain.push(at);
                match skin.nodes[at].parent {
                    Some(p) if p < skin.nodes.len() && chain.len() <= skin.nodes.len() => at = p,
                    _ => break None,
                }
            };
            for &n in chain.iter().rev() {
                // A loop of parents comes back to a node on this chain:
                // none of it is under the root.
                let parent = skin.nodes[n].parent;
                base = base.filter(|_| state[n].is_none()).map(|crc| {
                    let crc = if parent == Some(root) {
                        crc
                    } else {
                        baked::crc32_extend(crc, b"/")
                    };
                    baked::crc32_extend(crc, skin.nodes[n].name.as_bytes())
                });
                state[n] = Some(base);
            }
        }
        for (node, crc) in state.iter().enumerate() {
            if let Some(Some(crc)) = crc {
                map.entry(!crc).or_default().push(node);
            }
        }
        map
    }
}
impl PathRef {
    fn describe(&self) -> String {
        match self {
            PathRef::Name(n) => format!("{n:?}"),
            PathRef::Hash(h) => format!("hash {h:#010x}"),
        }
    }
}
/// Each node's children by name: `None` where two share a name.
fn child_index(skin: &MeshSkin) -> HashMap<(usize, &str), Option<usize>> {
    let mut children: HashMap<(usize, &str), Option<usize>> = HashMap::new();
    for (i, n) in skin.nodes.iter().enumerate() {
        if let Some(parent) = n.parent {
            children
                .entry((parent, n.name.as_str()))
                .and_modify(|c| *c = None)
                .or_insert(Some(i));
        }
    }
    children
}

/// The one node a path leads to from `root`, if it's unambiguous.
fn resolve(
    skin: &MeshSkin,
    root: usize,
    path: &PathRef,
    by_hash: &HashMap<u32, Vec<usize>>,
    children: &HashMap<(usize, &str), Option<usize>>,
) -> Option<usize> {
    match path {
        PathRef::Hash(h) => match by_hash.get(h).map(Vec::as_slice) {
            Some([only]) => Some(*only),
            _ => None,
        },
        PathRef::Name(path) => {
            let mut node = (root < skin.nodes.len()).then_some(root);
            for part in path.split('/').filter(|s| !s.is_empty()) {
                node = node.and_then(|parent| children.get(&(parent, part)).copied().flatten());
            }
            node
        }
    }
}

/// Keys of an `AnimationCurve` of vectors or quaternions.
fn vector_keys(curve: &Value, skipped: &mut Vec<String>) -> Result<Vec<Key>, Error> {
    let mut keys = Vec::new();
    for k in curve["m_Curve"].as_seq().unwrap_or_default() {
        if number(&k["weightedMode"]) != 0.0 {
            skipped.push("weighted tangents approximated as unweighted Hermite".into());
        }
        let key = Key {
            time: number(&k["time"]),
            value: vector(&k["value"]),
            incoming: vector(&k["inSlope"]),
            outgoing: vector(&k["outSlope"]),
        };
        if !key.time.is_finite() || key.value.iter().any(|v| !v.is_finite()) {
            return Err(Error::invalid("non-finite animation key"));
        }
        keys.push(key);
    }
    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    if keys.windows(2).any(|w| w[0].time == w[1].time) {
        return Err(Error::invalid("duplicate animation key time"));
    }
    Ok(keys)
}
/// Keys of an `AnimationCurve` of floats.
fn scalar_keys(curve: &Value, skipped: &mut Vec<String>) -> Result<Vec<SKey>, Error> {
    let mut keys = Vec::new();
    for k in curve["m_Curve"].as_seq().unwrap_or_default() {
        if number(&k["weightedMode"]) != 0.0 {
            skipped.push("weighted tangents approximated as unweighted Hermite".into());
        }
        let key = SKey {
            time: number(&k["time"]),
            value: number(&k["value"]),
            incoming: number(&k["inSlope"]),
            outgoing: number(&k["outSlope"]),
        };
        if !key.time.is_finite() || !key.value.is_finite() || key.incoming.is_nan() || key.outgoing.is_nan() {
            return Err(Error::invalid("non-finite animation key"));
        }
        keys.push(key);
    }
    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    if keys.windows(2).any(|w| w[0].time == w[1].time) {
        return Err(Error::invalid("duplicate animation key time"));
    }
    Ok(keys)
}
/// Takes a `blendShape.<name>` float curve into `morphs` (weights are
/// percentages in Unity). Returns whether the entry was one.
fn blend_shape_curve(
    entry: &Value,
    morphs: &mut Vec<Morph>,
    names: &mut HashSet<(String, String)>,
    skipped: &mut Vec<String>,
) -> Result<bool, Error> {
    let Some(name) = entry["attribute"]
        .as_str()
        .and_then(|a| a.strip_prefix("blendShape."))
        .filter(|_| entry["classID"].as_i64() == Some(137))
    else {
        return Ok(false);
    };
    let path = entry["path"].as_str().unwrap_or("");
    let keys = scalar_keys(&entry["curve"], skipped)?;
    // The same curve can be listed as an editor curve and a float curve.
    if !keys.is_empty() && names.insert((name.to_owned(), path.to_owned())) {
        morphs.push(Morph {
            name: name.into(),
            hash: None,
            curve: percent_curve(PathRef::Name(path.into()), &keys),
        });
    }
    Ok(true)
}
/// Takes a humanoid curve (class 95: a muscle, or the body's `RootT` and
/// `RootQ`) into `humanoid` by attribute. IK goal, motion and translation
/// curves are skipped and reported. Returns whether the entry was one.
fn humanoid_curve(
    entry: &Value,
    humanoid: &mut Vec<(String, Vec<SKey>)>,
    skipped: &mut Vec<String>,
) -> Result<bool, Error> {
    if entry["classID"].as_i64() != Some(95) {
        return Ok(false);
    }
    let attribute = entry["attribute"].as_str().unwrap_or("");
    let body = ["RootT.", "RootQ."].iter().any(|p| attribute.starts_with(p));
    if !body && !crate::humanoid::is_muscle(attribute) {
        skipped.push("skipped humanoid IK goal, motion and translation curves".into());
        return Ok(true);
    }
    let keys = scalar_keys(&entry["curve"], skipped)?;
    if !keys.is_empty() && !humanoid.iter().any(|(n, _)| n == attribute) {
        humanoid.push((attribute.to_owned(), keys));
    }
    Ok(true)
}
fn percent_curve(path: PathRef, keys: &[SKey]) -> Curve {
    Curve {
        path,
        kind: 0,
        mask: [true, false, false, false],
        keys: keys
            .iter()
            .map(|k| Key {
                time: k.time,
                value: [k.value / 100.; 4],
                incoming: [k.incoming / 100.; 4],
                outgoing: [k.outgoing / 100.; 4],
            })
            .collect(),
    }
}
/// The curve kind (0 position, 1 rotation, 2 scale, 3 Euler) and component
/// an editor curve's property animates.
fn transform_property(class_id: Option<i64>, attribute: &str) -> Option<(usize, usize)> {
    if class_id != Some(4) {
        return None;
    }
    let (base, part) = attribute.rsplit_once('.')?;
    let part = ["x", "y", "z", "w"].iter().position(|p| *p == part)?;
    let kind = match base {
        "m_LocalPosition" => 0,
        "m_LocalRotation" => 1,
        "m_LocalScale" => 2,
        "m_LocalEulerAngles" | "localEulerAnglesRaw" | "localEulerAngles" | "localEulerAnglesBaked" => 3,
        _ => return None,
    };
    (part < 3 || kind == 1).then_some((kind, part))
}
/// The editor's per-component curves of one transform property.
struct Group {
    path: String,
    kind: usize,
    parts: [Vec<SKey>; 4],
}

/// A float curve's value and tangent at a time that carries no key of its
/// own; a step segment gives an infinite tangent.
fn scalar_at(keys: &[SKey], time: f32) -> (f32, f32) {
    let right = keys.partition_point(|k| k.time <= time);
    if right == 0 {
        return (keys[0].value, 0.0);
    }
    let a = &keys[right - 1];
    let Some(b) = keys.get(right) else {
        return (a.value, 0.0);
    };
    if a.outgoing.is_infinite() || b.incoming.is_infinite() {
        return (a.value, f32::INFINITY);
    }
    let dt = b.time - a.time;
    let u = (time - a.time) / dt;
    let value = (2.0 * u.powi(3) - 3.0 * u * u + 1.0) * a.value
        + (u.powi(3) - 2.0 * u * u + u) * dt * a.outgoing
        + (-2.0 * u.powi(3) + 3.0 * u * u) * b.value
        + (u.powi(3) - u * u) * dt * b.incoming;
    let slope = ((6.0 * u * u - 6.0 * u) * a.value
        + (3.0 * u * u - 4.0 * u + 1.0) * dt * a.outgoing
        + (-6.0 * u * u + 6.0 * u) * b.value
        + (3.0 * u * u - 2.0 * u) * dt * b.incoming)
        / dt;
    (value, slope)
}
/// Joins per-component float curves into one vector curve over the union of
/// their key times, with the components that have curves.
fn combine(parts: &[Vec<SKey>; 4]) -> Option<([bool; 4], Vec<Key>)> {
    let mask = parts.each_ref().map(|p| !p.is_empty());
    if !mask.iter().any(|m| *m) {
        return None;
    }
    let mut times: Vec<f32> = parts.iter().flatten().map(|k| k.time).collect();
    times.sort_by(f32::total_cmp);
    times.dedup();
    let keys = times
        .into_iter()
        .map(|time| {
            let mut key = Key {
                time,
                value: [0.0; 4],
                incoming: [0.0; 4],
                outgoing: [0.0; 4],
            };
            for (i, keys) in parts.iter().enumerate().filter(|(i, _)| mask[*i]) {
                match keys.binary_search_by(|k| k.time.total_cmp(&time)) {
                    Ok(at) => {
                        key.value[i] = keys[at].value;
                        key.incoming[i] = keys[at].incoming;
                        key.outgoing[i] = keys[at].outgoing;
                    }
                    Err(_) => {
                        let (value, slope) = scalar_at(keys, time);
                        key.value[i] = value;
                        key.incoming[i] = slope;
                        key.outgoing[i] = slope;
                    }
                }
            }
            key
        })
        .collect();
    Some((mask, keys))
}

/// Decodes the clip's baked runtime data (`m_MuscleClip`), mapped to targets
/// through `m_ClipBindingConstant`.
fn baked_curves(
    m: &Value,
    curves: &mut Vec<Curve>,
    morphs: &mut Vec<Morph>,
    skipped: &mut Vec<String>,
) -> Result<(), Error> {
    // Unity wraps the clip in an `OffsetPtr`: a single `data` field.
    let node = &m["m_MuscleClip"]["m_Clip"];
    let clip = node.get("data").unwrap_or(node);
    if !(has_data(&clip["m_StreamedClip"]["data"])
        || has_data(&clip["m_DenseClip"]["m_SampleArray"])
        || has_data(&clip["m_ConstantClip"]["data"]))
    {
        return Ok(());
    }
    let channels = baked::channels(clip)?;
    let bindings = baked::bindings(&m["m_ClipBindingConstant"]);
    let needed: usize = bindings.iter().map(|b| b.width).sum();
    if needed > channels.len() {
        return Err(Error::invalid(format!(
            "animation clip bindings read {needed} curves but its baked data holds {}",
            channels.len()
        )));
    }
    let mut humanoid = 0;
    for b in &bindings {
        let parts = &channels[b.first..b.first + b.width];
        match b.target {
            baked::Target::Transform { path, attribute } if b.width > 1 => {
                let mut split: [Vec<SKey>; 4] = Default::default();
                for (slot, keys) in split.iter_mut().zip(parts) {
                    slot.clone_from(keys);
                }
                if let Some((mask, keys)) = combine(&split) {
                    curves.push(Curve {
                        path: PathRef::Hash(path),
                        kind: attribute as usize - 1,
                        mask,
                        keys,
                    });
                }
            }
            baked::Target::BlendShape { path, hash } if !parts[0].is_empty() => {
                morphs.push(Morph {
                    name: String::new(),
                    hash: Some(hash),
                    curve: percent_curve(PathRef::Hash(path), &parts[0]),
                });
            }
            baked::Target::BlendShape { .. } => {}
            baked::Target::Humanoid => humanoid += 1,
            baked::Target::Other(-1) => skipped.push("skipped m_PPtrCurves".into()),
            baked::Target::Other(class) => skipped.push(format!("skipped baked curves of class {class}")),
            baked::Target::Transform { .. } => skipped.push("skipped baked transform property".into()),
        }
    }
    if humanoid > 0 || bindings.is_empty() {
        if curves.is_empty() && morphs.is_empty() {
            return Err(Error::unsupported(HUMANOID_ONLY));
        }
        skipped.push("skipped baked humanoid muscle curves (named by index; resave the clip from the editor)".into());
    }
    Ok(())
}
fn has_data(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Seq(s) => !s.is_empty(),
        Value::Map(m) => m.iter().any(|(_, v)| has_data(v)),
        Value::Scalar(s) => !s.is_empty() && s != "0",
        Value::Bytes(b) => !b.is_empty(),
        Value::Numbers(n) => !n.is_empty(),
    }
}
impl Curve {
    fn sample(&self, time: f32) -> [f32; 4] {
        let right = self.keys.partition_point(|k| k.time <= time);
        if right == 0 {
            return self.keys[0].value;
        }
        let a = &self.keys[right - 1];
        let Some(b) = self.keys.get(right) else {
            return a.value;
        };
        let dt = b.time - a.time;
        let u = (time - a.time) / dt;
        std::array::from_fn(|i| {
            if a.outgoing[i].is_infinite() || b.incoming[i].is_infinite() {
                return a.value[i];
            }
            (2.0 * u.powi(3) - 3.0 * u * u + 1.0) * a.value[i]
                + (u.powi(3) - 2.0 * u * u + u) * dt * a.outgoing[i]
                + (-2.0 * u.powi(3) + 3.0 * u * u) * b.value[i]
                + (u.powi(3) - u * u) * dt * b.incoming[i]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    #[test]
    fn yaml_curves_round_trip_sampled_hermite_and_zxy() {
        let mut text = String::from(
            "--- !u!74 &1\nAnimationClip:\n  m_Name: Motion\n  m_SampleRate: 2\n  m_AnimationClipSettings: {m_StartTime: 1, m_StopTime: 2, m_LoopTime: 1}\n  m_FloatCurves: [{attribute: ignored}]\n",
        );
        for (field, path, a, b, outgoing, incoming) in [
            (
                "m_PositionCurves",
                "",
                "{x: 0, y: 0, z: 0}",
                "{x: 2, y: 0, z: 4}",
                "{x: 4, y: 0, z: 0}",
                "{x: 0, y: 0, z: 0}",
            ),
            (
                "m_ScaleCurves",
                "Child",
                "{x: 1, y: 1, z: 1}",
                "{x: 3, y: 3, z: 3}",
                "{x: .inf, y: .inf, z: .inf}",
                "{x: 0, y: 0, z: 0}",
            ),
            (
                "m_RotationCurves",
                "",
                "{x: 0, y: 0, z: 0, w: 1}",
                "{x: 0, y: 1, z: 0, w: 0}",
                "{}",
                "{}",
            ),
            (
                "m_EulerCurves",
                "Child",
                "{x: 30, y: 40, z: 50}",
                "{x: 30, y: 40, z: 50}",
                "{}",
                "{}",
            ),
        ] {
            text.push_str(&format!("  {field}:\n  - path: '{path}'\n    curve:\n      m_Curve:\n      - time: 1\n        value: {a}\n        inSlope: {{}}\n        outSlope: {outgoing}\n      - time: 2\n        value: {b}\n        inSlope: {incoming}\n        outSlope: {{}}\n"));
        }
        text.push_str("  m_PositionCurves_UNUSED: []\n");
        // A second entry in an existing curve list exercises unresolved paths.
        text = text.replace(
            "  m_ScaleCurves:",
            "  - path: Missing\n    curve:\n      m_Curve:\n      - {time: 1, value: {x: 9}}\n  m_ScaleCurves:",
        );
        let clip = AnimationClip::parse(&text).unwrap();
        let mut skin = MeshSkin {
            root: Some(0),
            nodes: vec![
                crate::SkinNode {
                    name: "Root".into(),
                    parent: None,
                    matrix: Mat4::IDENTITY.to_cols_array(),
                },
                crate::SkinNode {
                    name: "Child".into(),
                    parent: Some(0),
                    matrix: Mat4::IDENTITY.to_cols_array(),
                },
            ],
            joints: vec![0, 1],
            inverse_binds: vec![Mat4::IDENTITY.to_cols_array(); 2],
            ..Default::default()
        };
        let report = clip.append_to_skin(&mut skin, 0);
        assert!(report.iter().any(|s| s.contains("Missing")));
        assert!(report.iter().any(|s| s.contains("m_FloatCurves")));
        let mirror = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
        for n in &mut skin.animations[0].nodes {
            for m in n {
                *m = (mirror * Mat4::from_cols_array(m) * mirror).to_cols_array();
            }
        }
        let vertices = crate::glb::Vertices {
            skin: Some(skin),
            positions: vec![[0.0; 3]; 3],
            joints: vec![[0; 4]; 3],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            ..Default::default()
        };
        let bytes = crate::glb::write(
            "Model",
            &vertices,
            &[crate::glb::Primitive {
                indices: vec![0, 1, 2],
                material: 0,
            }],
            vec![serde_json::json!({})],
            &[],
        );
        let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
        for channel in gltf.animations().next().unwrap().channels() {
            let reader = channel.reader(|_| gltf.blob.as_deref());
            assert_eq!(reader.read_inputs().unwrap().collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
            let child = channel.target().node().name() == Some("Child");
            match reader.read_outputs().unwrap() {
                gltf::animation::util::ReadOutputs::Translations(v) if !child => assert_eq!(
                    v.collect::<Vec<_>>(),
                    [[0.0, 0.0, 0.0], [1.5, 0.0, -2.0], [2.0, 0.0, -4.0]]
                ),
                gltf::animation::util::ReadOutputs::Scales(v) if child => {
                    for (actual, expected) in v.zip([[1.0; 3], [1.0; 3], [3.0; 3]]) {
                        assert!(actual.into_iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5));
                    }
                    assert_eq!(channel.sampler().interpolation(), gltf::animation::Interpolation::Step);
                }
                gltf::animation::util::ReadOutputs::Rotations(v) => {
                    let values: Vec<_> = v.into_f32().collect();
                    let expected = if child {
                        Quat::from_rotation_y(40f32.to_radians())
                            * Quat::from_rotation_x(30f32.to_radians())
                            * Quat::from_rotation_z(50f32.to_radians())
                    } else {
                        Quat::from_xyzw(0.0, 0.5, 0.0, 0.5).normalize()
                    };
                    let expected = Quat::from_mat4(&(mirror * Mat4::from_quat(expected) * mirror));
                    assert!(Quat::from_array(values[1]).dot(expected).abs() > 0.99999);
                }
                _ => {}
            }
        }
    }
    #[test]
    fn blend_shape_curves_scale_percent_and_keep_sparse_names() {
        let clip = AnimationClip::parse("--- !u!74 &1\nAnimationClip:\n  m_Name: Face\n  m_SampleRate: 2\n  m_FloatCurves:\n  - attribute: blendShape.Raise\n    classID: 137\n    path: ''\n    curve:\n      m_Curve:\n      - {time: 0, value: 0, inSlope: 100, outSlope: 100}\n      - {time: 1, value: 100, inSlope: 100, outSlope: 100}\n").unwrap();
        let mut skin = MeshSkin {
            nodes: vec![crate::SkinNode {
                name: "Root".into(),
                parent: None,
                matrix: Mat4::IDENTITY.to_cols_array(),
            }],
            ..Default::default()
        };
        assert!(clip.append_to_skin(&mut skin, 0).is_empty());
        assert_eq!(skin.animations[0].times, [0., 0.5, 1.]);
        assert_eq!(skin.animations[0].morphs, [("Raise".into(), vec![0., 0.5, 1.], false)]);
        let vertices = crate::glb::Vertices {
            skin: Some(skin),
            positions: vec![[0.; 3]; 3],
            joints: vec![[0; 4]; 3],
            weights: vec![[1., 0., 0., 0.]; 3],
            targets: vec![
                crate::glb::Target {
                    name: "Raise".into(),
                    positions: vec![[0.; 3]; 3],
                    ..Default::default()
                },
                crate::glb::Target {
                    name: "Untouched".into(),
                    positions: vec![[0.; 3]; 3],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let bytes = crate::glb::write(
            "Face",
            &vertices,
            &[crate::glb::Primitive {
                indices: vec![0, 1, 2],
                material: 0,
            }],
            vec![serde_json::json!({})],
            &[],
        );
        let g = gltf::Gltf::from_slice(&bytes).unwrap();
        let a = g.animations().next().unwrap();
        let ch = a
            .channels()
            .find(|c| c.target().property() == gltf::animation::Property::MorphTargetWeights)
            .unwrap();
        let gltf::animation::util::ReadOutputs::MorphTargetWeights(v) =
            ch.reader(|_| g.blob.as_deref()).read_outputs().unwrap()
        else {
            panic!()
        };
        assert_eq!(v.into_f32().collect::<Vec<_>>(), [0., 0., 0.5, 0., 1., 0.]);
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert_eq!(
            doc["animations"][0]["extras"]["ponkoAnimatedTargets"],
            serde_json::json!(["Raise"])
        );
    }
    #[test]
    fn text_parse_rejects_binary_and_empty_baked_flags_are_not_data() {
        assert!(
            AnimationClip::parse("binary")
                .unwrap_err()
                .message()
                .contains("parse_binary")
        );
        // A zero `m_Compressed` and empty baked containers are an empty clip.
        let clip = AnimationClip::parse(
            "--- !u!74 &1\nAnimationClip:\n  m_Compressed: 0\n  m_MuscleClip:\n    m_Clip:\n      data:\n        m_StreamedClip: {data: [], curveCount: 0}\n",
        )
        .unwrap();
        assert!(clip.curves.is_empty());
    }
}
