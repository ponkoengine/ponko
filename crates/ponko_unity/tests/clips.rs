//! Animation clips beyond plain YAML curves: editor curves next to baked
//! runtime data, binary serialized clips, and clips holding only the
//! streamed, dense, constant or compressed data Unity bakes.
//! Every fixture is built here, byte by byte.

use glam::{Mat4, Quat, Vec3};
use ponko_unity::{AnimationClip, MeshSkin, SkinNode};

/// Standard CRC-32, the hash Unity binds paths and properties with.
fn crc32(text: &str) -> u32 {
    let mut crc = !0u32;
    for &byte in text.as_bytes() {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn skin(rest: Mat4) -> MeshSkin {
    let node = |name: &str, parent: Option<usize>, matrix: Mat4| SkinNode {
        name: name.into(),
        parent,
        matrix: matrix.to_cols_array(),
    };
    MeshSkin {
        root: Some(0),
        nodes: vec![
            node("Root", None, Mat4::IDENTITY),
            node("Child", Some(0), rest),
            node("Grand", Some(1), Mat4::IDENTITY),
        ],
        joints: vec![0, 1, 2],
        inverse_binds: vec![Mat4::IDENTITY.to_cols_array(); 3],
        ..Default::default()
    }
}

fn sampled(clip: &AnimationClip, rest: Mat4) -> (MeshSkin, Vec<String>) {
    let mut skin = skin(rest);
    let report = clip.append_to_skin(&mut skin, 0);
    (skin, report)
}

/// Scale, rotation and position of `node` at every sample.
fn poses(skin: &MeshSkin, node: usize) -> Vec<(Vec3, Quat, Vec3)> {
    skin.animations[0].nodes[node]
        .iter()
        .map(|m| Mat4::from_cols_array(m).to_scale_rotation_translation())
        .collect()
}

fn near(a: Vec3, b: [f32; 3]) -> bool {
    a.abs_diff_eq(Vec3::from_array(b), 1e-4)
}

// --- Baked clips (YAML) ---

fn floats(values: &[f32]) -> String {
    format!("[{}]", values.iter().map(f32::to_string).collect::<Vec<_>>().join(", "))
}
fn words(values: &[u32]) -> String {
    format!("[{}]", values.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))
}

/// A binding as `(path hash, attribute, class id, custom type, is pptr)`.
type Binding = (u32, u32, i32, u8, u8);

/// The runtime data the baked fixtures hold.
struct Baked {
    bindings: Vec<Binding>,
    streamed: Vec<u32>,
    streamed_curves: u32,
    dense: (u32, u32, f32, f32, Vec<f32>),
    constant: Vec<f32>,
}

/// Appends a streamed frame: its time, then `(curve, [a, b, c, d])` keys.
fn frame(out: &mut Vec<u32>, time: f32, keys: &[(u32, [f32; 4])]) {
    out.push(time.to_bits());
    out.push(keys.len() as u32);
    for (curve, coef) in keys {
        out.push(*curve);
        out.extend(coef.map(f32::to_bits));
    }
}

/// Position, scale and rotation of a small rig:
/// - Child position (streamed): x rises linearly 0 to 2, y steps 1 to 3 at
///   the end, z eases 0 to 1 (`-2 s^3 + 3 s^2`);
/// - Child scale (dense, 2 samples per second): (1,1,1), (2,3,4), (1,1,1);
/// - Root rotation (constant): 90 degrees about y.
fn baked() -> Baked {
    let mut streamed = Vec::new();
    frame(
        &mut streamed,
        f32::MIN,
        &[(0, [0.; 4]), (1, [0., 0., 0., 1.]), (2, [0.; 4])],
    );
    frame(
        &mut streamed,
        0.0,
        &[(0, [0., 0., 2., 0.]), (1, [0., 0., 0., 1.]), (2, [-2., 3., 0., 0.])],
    );
    frame(
        &mut streamed,
        1.0,
        &[(0, [0., 0., 0., 2.]), (1, [0., 0., 0., 3.]), (2, [0., 0., 0., 1.])],
    );
    frame(&mut streamed, f32::INFINITY, &[]);
    let h = std::f32::consts::FRAC_1_SQRT_2;
    Baked {
        bindings: vec![
            (crc32("Child"), 1, 4, 4, 0),
            (crc32("Child"), 3, 4, 4, 0),
            (0, 2, 4, 4, 0),
        ],
        streamed,
        streamed_curves: 3,
        dense: (3, 3, 2.0, 0.0, vec![1., 1., 1., 2., 3., 4., 1., 1., 1.]),
        constant: vec![0., h, 0., h],
    }
}

fn baked_yaml(b: &Baked, editor: &str) -> String {
    let mut text = String::from(
        "--- !u!74 &7400000\nAnimationClip:\n  m_ObjectHideFlags: 0\n  m_Name: Baked\n  m_Legacy: 0\n  m_Compressed: 0\n  m_SampleRate: 4\n",
    );
    text += editor;
    text += "  m_ClipBindingConstant:\n    genericBindings:\n";
    for (path, attribute, class, custom, pptr) in &b.bindings {
        text += &format!(
            "    - serializedVersion: 2\n      path: {path}\n      attribute: {attribute}\n      script: {{fileID: 0}}\n      typeID: {class}\n      customType: {custom}\n      isPPtrCurve: {pptr}\n"
        );
    }
    text += "    pptrCurveMapping: []\n  m_MuscleClip:\n    m_StartTime: 0\n    m_StopTime: 1\n    m_LoopTime: 1\n    m_Clip:\n      data:\n";
    text += &format!(
        "        m_StreamedClip:\n          data: {}\n          curveCount: {}\n",
        words(&b.streamed),
        b.streamed_curves
    );
    let (frames, curves, rate, begin, samples) = &b.dense;
    text += &format!(
        "        m_DenseClip:\n          m_FrameCount: {frames}\n          m_CurveCount: {curves}\n          m_SampleRate: {rate}\n          m_BeginTime: {begin}\n          m_SampleArray: {}\n",
        floats(samples)
    );
    text += &format!("        m_ConstantClip:\n          data: {}\n", floats(&b.constant));
    text
}

fn check_baked(clip: &AnimationClip) {
    let (skin, report) = sampled(clip, Mat4::IDENTITY);
    assert!(report.is_empty(), "{report:?}");
    let a = &skin.animations[0];
    assert_eq!(a.times, [0.0, 0.25, 0.5, 0.75, 1.0]);
    assert!(a.looping);
    let child = poses(&skin, 1);
    // x: linear; y: held, then the step; z: eased.
    let expected = [
        ([1.0, 1.0, 1.0], [0.0, 1.0, 0.0]),
        ([1.5, 2.0, 2.5], [0.5, 1.0, 0.15625]),
        ([2.0, 3.0, 4.0], [1.0, 1.0, 0.5]),
        ([1.5, 2.0, 2.5], [1.5, 1.0, 0.84375]),
        ([1.0, 1.0, 1.0], [2.0, 3.0, 1.0]),
    ];
    for (i, (scale, position)) in expected.iter().enumerate() {
        assert!(near(child[i].0, *scale), "scale {i}: {:?}", child[i].0);
        assert!(near(child[i].2, *position), "position {i}: {:?}", child[i].2);
    }
    let want = Quat::from_rotation_y(90f32.to_radians());
    for (_, rotation, _) in poses(&skin, 0) {
        assert!(rotation.dot(want).abs() > 0.99999, "{rotation:?}");
    }
    // A node nothing is bound to stays untouched.
    assert!(a.nodes[2].is_empty());
}

#[test]
fn baked_clip_decodes_streamed_dense_and_constant_curves_through_bindings() {
    let clip = AnimationClip::parse(&baked_yaml(&baked(), "")).unwrap();
    check_baked(&clip);
}

#[test]
fn editor_curves_win_over_the_baked_data_next_to_them() {
    // The baked part is garbage that would fail to decode; it must not be read.
    let mut b = baked();
    b.streamed = vec![1, 2, 3];
    b.streamed_curves = 1;
    let editor = "  m_EditorCurves:\n  - curve:\n      m_Curve:\n      - {time: 0, value: 0, inSlope: 0, outSlope: 4}\n      - {time: 1, value: 4, inSlope: 4, outSlope: 0}\n    attribute: m_LocalPosition.x\n    path: Child\n    classID: 4\n    script: {fileID: 0}\n  m_EulerEditorCurves:\n  - curve:\n      m_Curve:\n      - {time: 0, value: 0, inSlope: 0, outSlope: 90, tangentMode: 0}\n      - {time: 1, value: 90, inSlope: 90, outSlope: 0, tangentMode: 0}\n    attribute: m_LocalEulerAngles.y\n    path: Child\n    classID: 4\n    script: {fileID: 0}\n";
    let clip = AnimationClip::parse(&baked_yaml(&b, editor)).unwrap();
    let rest = Mat4::from_translation(Vec3::new(9.0, 5.0, 6.0));
    let (skin, report) = sampled(&clip, rest);
    assert!(report.is_empty(), "{report:?}");
    let child = poses(&skin, 1);
    // Only x is animated; y and z keep the rest pose.
    assert!(near(child[0].2, [0.0, 5.0, 6.0]));
    assert!(near(child[2].2, [2.0, 5.0, 6.0]));
    assert!(near(child[4].2, [4.0, 5.0, 6.0]));
    // Only the Euler y angle is animated.
    let want = Quat::from_rotation_y(45f32.to_radians());
    assert!(child[2].1.dot(want).abs() > 0.9999, "{:?}", child[2].1);
    // Nothing was taken from the baked Root rotation.
    assert!(skin.animations[0].nodes[0].is_empty());
}

#[test]
fn plain_transform_curves_win_over_editor_curves_for_the_same_target() {
    let text = "--- !u!74 &1\nAnimationClip:\n  m_Name: Both\n  m_SampleRate: 1\n  m_PositionCurves:\n  - path: Child\n    curve:\n      m_Curve:\n      - {time: 0, value: {x: 7, y: 7, z: 7}}\n  m_EditorCurves:\n  - curve:\n      m_Curve:\n      - {time: 0, value: 1}\n    attribute: m_LocalPosition.x\n    path: Child\n    classID: 4\n";
    let (skin, _) = sampled(&AnimationClip::parse(text).unwrap(), Mat4::IDENTITY);
    assert!(near(poses(&skin, 1)[0].2, [7.0, 7.0, 7.0]));
}

#[test]
fn unresolved_hashed_paths_are_reported() {
    let mut b = baked();
    b.bindings[1].0 = crc32("Missing");
    let clip = AnimationClip::parse(&baked_yaml(&b, "")).unwrap();
    let (skin, report) = sampled(&clip, Mat4::IDENTITY);
    assert!(
        report.iter().any(|d| d.contains("unresolved transform path hash")),
        "{report:?}"
    );
    // The resolved bindings still apply.
    assert!(!skin.animations[0].nodes[1].is_empty());
}

#[test]
fn nested_paths_resolve_by_the_hash_of_their_full_path() {
    let mut b = baked();
    b.bindings[1].0 = crc32("Child/Grand");
    let clip = AnimationClip::parse(&baked_yaml(&b, "")).unwrap();
    let (skin, report) = sampled(&clip, Mat4::IDENTITY);
    assert!(report.is_empty(), "{report:?}");
    // The scale went to Grand now.
    assert!(near(poses(&skin, 2)[2].0, [2.0, 3.0, 4.0]));
    assert!(near(poses(&skin, 1)[2].2, [1.0, 1.0, 0.5]));
}

#[test]
fn baked_blend_shapes_are_named_from_the_mesh() {
    let h = crc32("blendShape.Raise");
    let b = Baked {
        bindings: vec![(0, h, 137, 20, 0)],
        streamed: Vec::new(),
        streamed_curves: 0,
        dense: (2, 1, 1.0, 0.0, vec![0.0, 100.0]),
        constant: Vec::new(),
    };
    let clip = AnimationClip::parse(&baked_yaml(&b, "")).unwrap();
    let (_, report) = sampled(&clip, Mat4::IDENTITY);
    assert!(
        report.iter().any(|d| d.contains("unresolved blend shape")),
        "{report:?}"
    );
    let named = clip.with_blend_shape_names(["Smile", "Raise"]);
    let (skin, report) = sampled(&named, Mat4::IDENTITY);
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(skin.animations[0].morphs[0].0, "Raise");
    // Percent become weights.
    assert_eq!(skin.animations[0].morphs[0].1.last(), Some(&1.0));
}

#[test]
fn humanoid_only_clips_are_rejected_clearly() {
    let b = Baked {
        bindings: vec![(0, 12345, 95, 8, 0), (0, 12346, 95, 8, 0)],
        streamed: Vec::new(),
        streamed_curves: 0,
        dense: (2, 2, 30.0, 0.0, vec![0.0, 0.1, 0.2, 0.3]),
        constant: Vec::new(),
    };
    let message = AnimationClip::parse(&baked_yaml(&b, "")).unwrap_err().to_string();
    assert!(message.contains("humanoid"), "{message}");
}

#[test]
fn humanoid_curves_next_to_generic_ones_are_skipped_and_reported() {
    let mut b = baked();
    // One muscle channel after the generic ones (constants come last).
    b.bindings.push((0, 4242, 95, 8, 0));
    b.constant.push(0.5);
    let clip = AnimationClip::parse(&baked_yaml(&b, "")).unwrap();
    let (skin, report) = sampled(&clip, Mat4::IDENTITY);
    assert!(report.iter().any(|d| d.contains("humanoid")), "{report:?}");
    assert!(!skin.animations[0].nodes[1].is_empty());
}

#[test]
fn bindings_reading_more_curves_than_the_data_holds_are_errors() {
    let mut b = baked();
    b.constant.truncate(2);
    assert!(AnimationClip::parse(&baked_yaml(&b, "")).is_err());
    let mut b = baked();
    b.streamed.truncate(b.streamed.len() - 6);
    assert!(AnimationClip::parse(&baked_yaml(&b, "")).is_err());
}

// --- Compressed rotation curves ---

/// Little-endian bit fields, low bits first.
fn pack(fields: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut at = 0usize;
    for &(value, size) in fields {
        for bit in 0..size {
            if at.is_multiple_of(8) {
                bytes.push(0);
            }
            *bytes.last_mut().unwrap() |= (((value >> bit) & 1) as u8) << (at % 8);
            at += 1;
        }
    }
    bytes
}

/// A unit quaternion in 32 bits: flags (largest component, its sign), then
/// the other three in 9 or 10 bits.
fn pack_quat(q: [f32; 4]) -> Vec<u8> {
    let mut largest = 0;
    for i in 1..4 {
        if q[i].abs() > q[largest].abs() {
            largest = i;
        }
    }
    let mut fields = vec![(largest as u32 | u32::from(q[largest] < 0.0) << 2, 3)];
    for (j, &x) in q.iter().enumerate().filter(|(j, _)| *j != largest) {
        let size = if (largest + 1) % 4 == j { 9 } else { 10 };
        let max = ((1u32 << size) - 1) as f32;
        fields.push((((x + 1.0) * 0.5 * max).round() as u32, size));
    }
    pack(&fields)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn compressed_rotation_curves_decode_their_packed_keys() {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    // Identity, then 90 degrees about y with a negative w (the same turn).
    let mut quats = pack_quat([0.0, 0.0, 0.0, 1.0]);
    quats.extend(pack_quat([0.0, h, 0.0, h]));
    let text = format!(
        "--- !u!74 &1\nAnimationClip:\n  m_Name: Packed\n  m_Compressed: 1\n  m_SampleRate: 2\n  m_CompressedRotationCurves:\n  - m_Path: Child\n    m_Times: {{m_NumItems: 2, m_Data: 0064, m_BitSize: 8}}\n    m_Values: {{m_NumItems: 2, m_Data: {}}}\n    m_Slopes: {{m_NumItems: 16, m_Range: 1, m_Start: 0, m_Data: '', m_BitSize: 0}}\n    m_PreInfinity: 2\n    m_PostInfinity: 2\n  m_AnimationClipSettings: {{m_StartTime: 0, m_StopTime: 1}}\n",
        hex(&quats)
    );
    let clip = AnimationClip::parse(&text).unwrap();
    let (skin, report) = sampled(&clip, Mat4::IDENTITY);
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(skin.animations[0].times, [0.0, 0.5, 1.0]);
    let child = poses(&skin, 1);
    let turn = Quat::from_rotation_y(90f32.to_radians());
    assert!(child[0].1.dot(Quat::IDENTITY).abs() > 0.9995, "{:?}", child[0].1);
    assert!(child[2].1.dot(turn).abs() > 0.9995, "{:?}", child[2].1);
    // Zero tangents ease between the keys.
    let middle = Quat::IDENTITY.lerp(turn, 0.5).normalize();
    assert!(child[1].1.dot(middle).abs() > 0.999, "{:?}", child[1].1);
}

#[test]
fn compressed_curves_with_mismatched_counts_are_errors() {
    let text = "--- !u!74 &1\nAnimationClip:\n  m_Compressed: 1\n  m_CompressedRotationCurves:\n  - m_Path: Child\n    m_Times: {m_NumItems: 2, m_Data: 0064, m_BitSize: 8}\n    m_Values: {m_NumItems: 1, m_Data: 00000000}\n    m_Slopes: {m_NumItems: 0, m_Range: 1, m_Start: 0, m_Data: '', m_BitSize: 0}\n";
    assert!(AnimationClip::parse(text).is_err());
    // Packed data that ends early.
    let short = text.replace("m_Data: 0064", "m_Data: 00");
    assert!(AnimationClip::parse(&short).is_err());
}

// --- Binary serialized clips ---

/// A tiny writer for binary serialized objects: a value tree that knows
/// its type tree and its bytes.
enum N {
    I32(&'static str, i32),
    U32(&'static str, u32),
    U8(&'static str, u8),
    F32(&'static str, f32),
    Str(&'static str, String),
    Obj(&'static str, &'static str, Vec<N>),
    /// A vector: its name, a template of one element (named `data`) and the items.
    List(&'static str, Box<N>, Vec<N>),
}

const ALIGN: i32 = 0x4000;
type Node = (u8, bool, u32, u32, i32, i32);

struct Strings(Vec<u8>);
impl Strings {
    fn add(&mut self, s: &str) -> u32 {
        let at = self.0.len() as u32;
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
        at
    }
}

fn shape(n: &N, level: u8, s: &mut Strings, out: &mut Vec<Node>) {
    let mut leaf = |ty: &str, name: &str, size: i32, s: &mut Strings| {
        out.push((level, false, s.add(ty), s.add(name), size, 0));
    };
    match n {
        N::I32(name, _) => leaf("int", name, 4, s),
        N::U32(name, _) => leaf("unsigned int", name, 4, s),
        N::U8(name, _) => leaf("UInt8", name, 1, s),
        N::F32(name, _) => leaf("float", name, 4, s),
        N::Str(name, _) => {
            leaf("string", name, -1, s);
            out.push((level + 1, true, s.add("Array"), s.add("Array"), -1, ALIGN));
            out.push((level + 2, false, s.add("int"), s.add("size"), 4, 0));
            out.push((level + 2, false, s.add("char"), s.add("data"), 1, 0));
        }
        N::Obj(name, ty, children) => {
            out.push((level, false, s.add(ty), s.add(name), -1, 0));
            for c in children {
                shape(c, level + 1, s, out);
            }
        }
        N::List(name, template, _) => {
            out.push((level, false, s.add("vector"), s.add(name), -1, 0));
            out.push((level + 1, true, s.add("Array"), s.add("Array"), -1, ALIGN));
            out.push((level + 2, false, s.add("int"), s.add("size"), 4, 0));
            shape(template, level + 2, s, out);
        }
    }
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn write(n: &N, out: &mut Vec<u8>) {
    match n {
        N::I32(_, v) => out.extend(v.to_le_bytes()),
        N::U32(_, v) => out.extend(v.to_le_bytes()),
        N::U8(_, v) => out.push(*v),
        N::F32(_, v) => out.extend(v.to_le_bytes()),
        N::Str(_, v) => {
            out.extend((v.len() as i32).to_le_bytes());
            out.extend(v.as_bytes());
            pad(out);
        }
        N::Obj(_, _, children) => children.iter().for_each(|c| write(c, out)),
        N::List(_, _, items) => {
            out.extend((items.len() as i32).to_le_bytes());
            items.iter().for_each(|i| write(i, out));
            pad(out);
        }
    }
}

/// A binary serialized file (format 23, with type trees) of one object.
fn binary_file(class_id: i32, root: &N) -> Vec<u8> {
    let mut local = Strings(Vec::new());
    let mut nodes = Vec::new();
    shape(root, 0, &mut local, &mut nodes);
    let mut tree = Vec::new();
    tree.extend((nodes.len() as i32).to_le_bytes());
    tree.extend((local.0.len() as i32).to_le_bytes());
    for (level, array, ty, name, size, flags) in &nodes {
        tree.extend(1u16.to_le_bytes());
        tree.push(*level);
        tree.push(u8::from(*array));
        tree.extend(ty.to_le_bytes());
        tree.extend(name.to_le_bytes());
        tree.extend(size.to_le_bytes());
        tree.extend(0i32.to_le_bytes());
        tree.extend(flags.to_le_bytes());
        tree.extend(0u64.to_le_bytes());
    }
    tree.extend(&local.0);
    let mut object = Vec::new();
    write(root, &mut object);

    let mut meta = Vec::new();
    meta.extend(b"6000.0.0f1\0");
    meta.extend((-2i32).to_le_bytes());
    meta.push(1); // type trees
    meta.extend(1i32.to_le_bytes()); // one type
    meta.extend(class_id.to_le_bytes());
    meta.push(0);
    meta.extend((-1i16).to_le_bytes());
    meta.extend([0u8; 32]); // the hashes
    meta.extend(((tree.len() + 8) as u32).to_le_bytes());
    meta.extend(b"mhtt");
    meta.extend(23u32.to_le_bytes());
    meta.extend(&tree);
    meta.extend(0i32.to_le_bytes()); // dependencies
    meta.extend(1i32.to_le_bytes()); // one object
    while (48 + meta.len()) % 4 != 0 {
        meta.push(0);
    }
    meta.extend(7400000i64.to_le_bytes()); // path id
    meta.extend(0i64.to_le_bytes()); // start
    meta.extend((object.len() as u32).to_le_bytes());
    meta.extend(0i32.to_le_bytes()); // type 0
    meta.extend(0i32.to_le_bytes()); // scripts
    meta.extend(0i32.to_le_bytes()); // externals
    meta.push(0); // user information

    let data_offset = (48 + meta.len()).next_multiple_of(16);
    let mut out = Vec::new();
    out.extend(0u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend(23u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.push(0); // little endian
    out.extend([0, 0, 0]);
    out.extend((meta.len() as u32).to_be_bytes());
    out.extend(((data_offset + object.len()) as u64).to_be_bytes());
    out.extend((data_offset as u64).to_be_bytes());
    out.extend(0u64.to_be_bytes());
    out.extend(&meta);
    out.resize(data_offset, 0);
    out.extend(&object);
    out
}

fn list_u32(name: &'static str, values: &[u32]) -> N {
    N::List(
        name,
        Box::new(N::U32("data", 0)),
        values.iter().map(|v| N::U32("data", *v)).collect(),
    )
}
fn list_f32(name: &'static str, values: &[f32]) -> N {
    N::List(
        name,
        Box::new(N::F32("data", 0.0)),
        values.iter().map(|v| N::F32("data", *v)).collect(),
    )
}
fn vec3(name: &'static str, v: [f32; 3]) -> N {
    N::Obj(
        name,
        "Vector3f",
        vec![N::F32("x", v[0]), N::F32("y", v[1]), N::F32("z", v[2])],
    )
}

fn binary_baked(b: &Baked) -> Vec<u8> {
    let binding = || {
        N::Obj(
            "data",
            "GenericBinding",
            vec![
                N::U32("path", 0),
                N::U32("attribute", 0),
                N::I32("typeID", 0),
                N::U8("customType", 0),
                N::U8("isPPtrCurve", 0),
            ],
        )
    };
    let bindings = b
        .bindings
        .iter()
        .map(|(path, attribute, class, custom, pptr)| {
            N::Obj(
                "data",
                "GenericBinding",
                vec![
                    N::U32("path", *path),
                    N::U32("attribute", *attribute),
                    N::I32("typeID", *class),
                    N::U8("customType", *custom),
                    N::U8("isPPtrCurve", *pptr),
                ],
            )
        })
        .collect();
    let (frames, curves, rate, begin, samples) = &b.dense;
    let root = N::Obj(
        "Base",
        "AnimationClip",
        vec![
            N::Str("m_Name", "Baked".into()),
            N::F32("m_SampleRate", 4.0),
            N::Obj(
                "m_ClipBindingConstant",
                "AnimationClipBindingConstant",
                vec![N::List("genericBindings", Box::new(binding()), bindings)],
            ),
            N::Obj(
                "m_MuscleClip",
                "ClipMuscleConstant",
                vec![
                    N::F32("m_StartTime", 0.0),
                    N::F32("m_StopTime", 1.0),
                    N::U8("m_LoopTime", 1),
                    N::Obj(
                        "m_Clip",
                        "OffsetPtr",
                        vec![N::Obj(
                            "data",
                            "Clip",
                            vec![
                                N::Obj(
                                    "m_StreamedClip",
                                    "StreamedClip",
                                    vec![list_u32("data", &b.streamed), N::U32("curveCount", b.streamed_curves)],
                                ),
                                N::Obj(
                                    "m_DenseClip",
                                    "DenseClip",
                                    vec![
                                        N::I32("m_FrameCount", *frames as i32),
                                        N::U32("m_CurveCount", *curves),
                                        N::F32("m_SampleRate", *rate),
                                        N::F32("m_BeginTime", *begin),
                                        list_f32("m_SampleArray", samples),
                                    ],
                                ),
                                N::Obj("m_ConstantClip", "ConstantClip", vec![list_f32("data", &b.constant)]),
                            ],
                        )],
                    ),
                ],
            ),
        ],
    );
    binary_file(74, &root)
}

#[test]
fn binary_baked_clip_matches_the_text_clip() {
    let b = baked();
    let from_binary = AnimationClip::parse_binary(&binary_baked(&b)).unwrap();
    check_baked(&from_binary);
    let from_text = AnimationClip::parse(&baked_yaml(&b, "")).unwrap();
    let (a, _) = sampled(&from_binary, Mat4::IDENTITY);
    let (t, _) = sampled(&from_text, Mat4::IDENTITY);
    assert_eq!(a.animations, t.animations);
}

/// One `Keyframe` of a vector curve.
fn key(time: f32, value: [f32; 3], out: [f32; 3]) -> N {
    N::Obj(
        "data",
        "Keyframe",
        vec![
            N::F32("time", time),
            vec3("value", value),
            vec3("inSlope", [0.0; 3]),
            vec3("outSlope", out),
            N::I32("weightedMode", 0),
        ],
    )
}

/// A `Vector3Curve` entry for `m_PositionCurves`.
fn position_entry(path: &str, keys: Vec<N>) -> N {
    N::Obj(
        "data",
        "Vector3Curve",
        vec![
            N::Obj(
                "curve",
                "AnimationCurve",
                vec![N::List("m_Curve", Box::new(key(0.0, [0.0; 3], [0.0; 3])), keys)],
            ),
            N::Str("path", path.into()),
        ],
    )
}

#[test]
fn binary_editor_curves_match_the_text_clip() {
    let root = N::Obj(
        "Base",
        "AnimationClip",
        vec![
            N::Str("m_Name", "Moves".into()),
            N::F32("m_SampleRate", 2.0),
            N::List(
                "m_PositionCurves",
                Box::new(position_entry("", vec![key(0.0, [0.0; 3], [0.0; 3])])),
                vec![position_entry(
                    "Child",
                    vec![
                        key(0.0, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
                        key(1.0, [2.0, 0.0, 4.0], [0.0, 0.0, 0.0]),
                    ],
                )],
            ),
            N::Obj(
                "m_AnimationClipSettings",
                "AnimationClipSettings",
                vec![
                    N::F32("m_StartTime", 0.0),
                    N::F32("m_StopTime", 1.0),
                    N::U8("m_LoopTime", 0),
                ],
            ),
        ],
    );
    let clip = AnimationClip::parse_binary(&binary_file(74, &root)).unwrap();
    let text = "--- !u!74 &1\nAnimationClip:\n  m_Name: Moves\n  m_SampleRate: 2\n  m_AnimationClipSettings: {m_StartTime: 0, m_StopTime: 1, m_LoopTime: 0}\n  m_PositionCurves:\n  - path: Child\n    curve:\n      m_Curve:\n      - {time: 0, value: {x: 0, y: 0, z: 0}, inSlope: {x: 0, y: 0, z: 0}, outSlope: {x: 2, y: 0, z: 0}}\n      - {time: 1, value: {x: 2, y: 0, z: 4}, inSlope: {x: 0, y: 0, z: 0}, outSlope: {x: 0, y: 0, z: 0}}\n";
    let expected = AnimationClip::parse(text).unwrap();
    let (a, _) = sampled(&clip, Mat4::IDENTITY);
    let (b, _) = sampled(&expected, Mat4::IDENTITY);
    assert_eq!(a.animations, b.animations);
    assert!(near(poses(&a, 1)[1].2, [1.25, 0.0, 2.0]), "{:?}", poses(&a, 1)[1].2);
}

#[test]
fn truncated_binary_clips_are_errors_not_panics() {
    let whole = binary_baked(&baked());
    for cut in [60, whole.len() / 2, whole.len() - 3] {
        assert!(AnimationClip::parse_binary(&whole[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn files_are_read_as_text_or_binary() {
    let dir = std::env::temp_dir().join(format!("ponko_unity_clips_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.anim"), binary_baked(&baked())).unwrap();
    std::fs::write(dir.join("b.anim"), baked_yaml(&baked(), "")).unwrap();
    check_baked(&AnimationClip::read(dir.join("a.anim")).unwrap());
    check_baked(&AnimationClip::read(dir.join("b.anim")).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A clip of a few curves can say it lasts hours: sampling it would make a
/// value per frame per curve. Too many samples is an error at parse time.
#[test]
fn clips_too_long_to_sample_are_errors() {
    let clip = |curves: usize| {
        let mut text =
            String::from("--- !u!74 &1\nAnimationClip:\n  m_Name: Long\n  m_SampleRate: 60\n  m_PositionCurves:\n");
        for c in 0..curves {
            text.push_str(&format!(
                "  - path: A{c}\n    curve:\n      m_Curve:\n      - {{time: 0, value: {{x: 0, y: 0, z: 0}}}}\n"
            ));
        }
        text.push_str("  m_AnimationClipSettings:\n    m_StartTime: 0\n    m_StopTime: 16000\n");
        AnimationClip::parse(&text)
    };
    assert!(clip(2).is_ok());
    let err = clip(40).unwrap_err();
    assert!(err.message().contains("too long to sample"), "{err}");
}

/// Packed vectors with no bits per item have no data to check their count
/// against: a huge count is an error, not a huge allocation.
#[test]
fn packed_vectors_without_bits_cant_claim_huge_counts() {
    let text = "--- !u!74 &1\nAnimationClip:\n  m_Name: C\n  m_SampleRate: 30\n  m_CompressedRotationCurves:\n  - m_Path: A\n    m_Times: {m_NumItems: 1000000000000, m_BitSize: 0, m_Data: []}\n    m_Values: {m_NumItems: 0, m_Data: []}\n    m_Slopes: {m_NumItems: 1000000000000, m_BitSize: 0, m_Range: 0, m_Start: 0, m_Data: []}\n";
    let started = std::time::Instant::now();
    let err = AnimationClip::parse(text).unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(err.message().contains("differ in count"), "{err}");
}
