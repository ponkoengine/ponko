//! Parsing Unity text files: documents, values, references, meshes and
//! project GUIDs. The fixtures are written by hand for these tests.

use ponko_unity::*;

const PREFAB: &str = "%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &100
GameObject:
  m_Component:
  - component: {fileID: 400}
  - component: {fileID: 2300}
  m_Layer: 8
  m_Name: 'Lamp: brass'
  m_IsActive: 1
--- !u!4 &400
Transform:
  m_GameObject: {fileID: 100}
  m_LocalRotation: {x: 0, y: 0.6, z: 0, w: 0.8}
  m_LocalPosition: {x: 1.5, y: -2, z: 3e2}
  m_Children:
  - {fileID: 401}
  m_Father: {fileID: 0}
--- !u!23 &2300
MeshRenderer:
  m_GameObject: {fileID: 100}
  m_Materials:
  - {fileID: 2100000, guid: 6f2a0b9c00000000a000000000000001, type: 2}
  m_LightmapParameters: {fileID: 0}
--- !u!4 &401 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 9001, guid: 77, type: 3}
  m_PrefabInstance: {fileID: 1001}
--- !u!114 &500
MonoBehaviour:
  m_Script: {fileID: 11500000, guid: abc, type: 3}
  hexish: 0000000001
  note: \"two
    lines\"
  list:
  - 1
  - 2
  empty:
";

#[test]
fn documents_keep_their_class_id_and_stripped_flag() {
    let file = UnityFile::parse(PREFAB).unwrap();
    let ids: Vec<(i32, i64, bool)> = file
        .objects
        .iter()
        .map(|o| (o.class_id, o.file_id, o.stripped))
        .collect();
    assert_eq!(
        ids,
        [
            (1, 100, false),
            (4, 400, false),
            (23, 2300, false),
            (4, 401, true),
            (114, 500, false)
        ]
    );
    assert_eq!(file.get(2300).unwrap().type_name, "MeshRenderer");
    assert_eq!(file.of_class(4).count(), 2);
}

#[test]
fn values_read_as_numbers_vectors_and_references() {
    let file = UnityFile::parse(PREFAB).unwrap();
    let go = &file.get(100).unwrap().body;
    assert_eq!(go["m_Name"].as_str(), Some("Lamp: brass"));
    assert_eq!(go["m_Layer"].as_i64(), Some(8));
    assert_eq!(go["m_IsActive"].as_bool(), Some(true));
    assert_eq!(go["m_Component"][1]["component"].obj_ref().unwrap().file_id, 2300);
    let t = &file.get(400).unwrap().body;
    assert_eq!(t["m_LocalPosition"].vec3(), Some([1.5, -2.0, 300.0]));
    assert_eq!(t["m_LocalRotation"].vec4(), Some([0.0, 0.6, 0.0, 0.8]));
    assert!(t["m_Father"].obj_ref().unwrap().is_none());
    let r = file.get(2300).unwrap().body["m_Materials"][0].obj_ref().unwrap();
    assert_eq!(r.guid.as_deref(), Some("6f2a0b9c00000000a000000000000001"));
    assert_eq!(r.kind, Some(2));
}

#[test]
fn scalars_keep_their_exact_text() {
    let file = UnityFile::parse(PREFAB).unwrap();
    let b = &file.get(500).unwrap().body;
    // Hex blobs of digits must not turn into numbers.
    assert_eq!(b["hexish"].as_str(), Some("0000000001"));
    assert_eq!(b["note"].as_str(), Some("two lines"));
    assert_eq!(b["list"].as_seq().unwrap().len(), 2);
    assert_eq!(b["empty"].as_str(), Some(""));
    assert!(b["missing"].is_null() && b["list"][7].is_null());
}

#[test]
fn bad_headers_say_where() {
    let err = UnityFile::parse("--- !u!1 &100\nGameObject: {}\n--- nonsense\nX: 1\n").unwrap_err();
    assert_eq!(err.line(), Some(3), "{err}");
    assert_eq!(err.kind(), ErrorKind::Decode);
    let err = UnityFile::parse("--- !u!1 &100\nGameObject:\n  a: [1, 2\n").unwrap_err();
    assert!(err.line().is_some(), "{err}");
    assert!(err.to_string().starts_with("line "), "{err}");
}

#[test]
fn meshes_decode_streams_and_submeshes() {
    let file = UnityFile::parse(include_str!("fixtures/quad.asset")).unwrap();
    let mesh = UnityMesh::from_object(&file.objects[0]).unwrap();
    assert_eq!(mesh.name, "Quad");
    assert_eq!(mesh.positions[3], [1.0, 0.0, 1.0]);
    assert_eq!(mesh.normals[2], [0.0, 1.0, 0.0]);
    // Stream 1 (half floats) starts 16-byte aligned after stream 0.
    assert_eq!(mesh.uvs[0][3], [1.0, 1.0]);
    assert_eq!(mesh.uvs[0][4], [0.5, 0.5]);
    assert!(mesh.uvs[1].is_empty() && mesh.colors.is_empty() && mesh.tangents.is_empty());
    assert_eq!(
        mesh.submeshes,
        [vec![0, 2, 1, 1, 2, 3], vec![1, 2, 4]],
        "base vertex added"
    );
    // Not a mesh.
    let go = UnityFile::parse(PREFAB).unwrap();
    assert!(UnityMesh::from_object(&go.objects[0]).is_err());
}

#[test]
fn mesh_blend_shapes_keep_all_authored_frames() {
    // "Lift" moves vertex 3 up; "Dent" has an in-between frame (vertex 0
    // down by 0.5) and its full frame (vertex 1 down by 1, normal tilted).
    let shapes = "  m_Shapes:
    vertices:
    - vertex: {x: 0, y: 1, z: 0}
      normal: {x: 0, y: 0, z: 0}
      tangent: {x: 0, y: 0, z: 0}
      index: 3
    - vertex: {x: 0, y: -0.5, z: 0}
      normal: {x: 0, y: 0, z: 0}
      tangent: {x: 0, y: 0, z: 0}
      index: 0
    - vertex: {x: 0, y: -1, z: 0}
      normal: {x: 0.5, y: 0, z: 0}
      tangent: {x: 0, y: 0, z: 0}
      index: 1
    shapes:
    - firstVertex: 0
      vertexCount: 1
      hasNormals: 0
      hasTangents: 0
    - firstVertex: 1
      vertexCount: 1
      hasNormals: 1
      hasTangents: 0
    - firstVertex: 2
      vertexCount: 1
      hasNormals: 1
      hasTangents: 0
    channels:
    - name: Lift
      nameHash: 1
      frameIndex: 0
      frameCount: 1
    - name: Dent
      nameHash: 2
      frameIndex: 1
      frameCount: 2
    fullWeights:
    - 100
    - 50
    - 100
";
    // Windows checkouts may end the fixture's lines with \r\n.
    let text = include_str!("fixtures/quad.asset")
        .replace("\r\n", "\n")
        .replace("  m_MeshCompression: 0\n", &format!("{shapes}  m_MeshCompression: 0\n"));
    let file = UnityFile::parse(&text).unwrap();
    let mesh = UnityMesh::from_object(&file.objects[0]).unwrap();
    let [lift, half, dent] = &mesh.blend_shapes[..] else {
        panic!("{:?}", mesh.blend_shapes);
    };
    assert_eq!(lift.name, "Lift");
    assert_eq!(
        lift.positions,
        [[0.0; 3], [0.0; 3], [0.0; 3], [0.0, 1.0, 0.0], [0.0; 3]]
    );
    assert!(lift.normals.is_empty());
    assert_eq!(dent.name, "Dent");
    assert_eq!(half.channel, "Dent");
    assert_eq!(half.frame_weight, 0.5);
    assert_eq!(half.positions[0], [0., -0.5, 0.]);
    assert_eq!(dent.frame_weight, 1.0);
    assert_eq!(
        dent.positions[0], [0.0; 3],
        "full frame is independent of the in-between"
    );
    assert_eq!(dent.positions[1], [0.0, -1.0, 0.0]);
    assert_eq!(dent.normals[1], [0.5, 0.0, 0.0]);
    assert_eq!(dent.normals.len(), 5);
    // Meshes without shapes have none.
    let plain = UnityFile::parse(include_str!("fixtures/quad.asset")).unwrap();
    assert!(
        UnityMesh::from_object(&plain.objects[0])
            .unwrap()
            .blend_shapes
            .is_empty()
    );
}

#[test]
fn projects_map_guids_to_files() {
    let dir = std::env::temp_dir().join(format!("ponko_unity_project_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Assets/Props")).unwrap();
    std::fs::create_dir_all(dir.join("Library/PackageCache/com.example/Shaders")).unwrap();
    std::fs::write(dir.join("Assets/Props/Lamp.prefab"), PREFAB).unwrap();
    std::fs::write(
        dir.join("Assets/Props/Lamp.prefab.meta"),
        "fileFormatVersion: 2\nguid: aa11\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("Assets/Props.meta"),
        "fileFormatVersion: 2\nguid: ff00\nfolderAsset: yes\n",
    )
    .unwrap();
    let shader = dir.join("Library/PackageCache/com.example/Shaders/Lit.shader");
    std::fs::write(&shader, "Shader \"Lit\" {}").unwrap();
    std::fs::write(shader.with_extension("shader.meta"), "guid: bb22\n").unwrap();
    let project = Project::open(&dir).unwrap();
    assert_eq!(project.len(), 3);
    assert_eq!(project.path_of("bb22"), Some(shader.as_path()));
    let lamp = project.load("aa11").unwrap();
    assert_eq!(lamp.objects.len(), 5);
    assert!(project.load("bb22").is_err(), "a shader isn't Unity YAML");
    assert!(project.load("nope").is_err());
    assert!(Project::open(dir.join("Assets")).is_err(), "no Assets folder inside");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Maps and lists nest at most 128 deep: a file of `- - - - ...` (two
/// bytes a level) or of nested maps can't overflow the stack while it is
/// built or dropped.
#[test]
fn deeply_nested_yaml_is_an_error_not_a_stack_overflow() {
    let deep_list = format!("--- !u!1 &1\nGameObject:\n  a:\n  {}x\n", "- ".repeat(200_000));
    let err = UnityFile::parse(&deep_list).unwrap_err();
    assert!(err.message().contains("nested deeper than 128"), "{err}");
    let mut deep_map = String::from("--- !u!1 &1\nGameObject:\n");
    for level in 1..200 {
        deep_map.push_str(&format!("{}a:\n", " ".repeat(level)));
    }
    let err = UnityFile::parse(&deep_map).unwrap_err();
    assert!(err.message().contains("nested deeper than 128"), "{err}");
    let deep_flow = format!(
        "--- !u!1 &1\nGameObject: {}{}\n",
        "{a: ".repeat(100_000),
        "}".repeat(100_000)
    );
    assert!(UnityFile::parse(&deep_flow).is_err());
    // Well within the limit: fine.
    let fine = format!("--- !u!1 &1\nGameObject:\n  a:\n  {}x\n", "- ".repeat(100));
    let fine = UnityFile::parse(&fine).unwrap();
    let mut v = &fine.objects[0].body["a"];
    for _ in 0..99 {
        v = &v[0];
    }
    assert_eq!(v[0].as_str(), Some("x"));
}

/// Counts and sizes in mesh, texture and terrain assets that their data
/// can't back are errors, not huge allocations or arithmetic overflow.
#[test]
fn counts_without_data_are_errors_not_huge_allocations() {
    let mesh = |vertex_data: &str, shapes: &str| {
        let text = format!(
            "--- !u!43 &1\nMesh:\n  m_Name: M\n  m_VertexData:\n{vertex_data}  m_Shapes:\n{shapes}  m_SubMeshes:\n  - {{topology: 0, firstByte: 0, indexCount: 0, baseVertex: 0}}\n  m_IndexBuffer: ''\n"
        );
        UnityMesh::from_object(&UnityFile::parse(&text).unwrap().objects[0])
    };
    let shapes = "    vertices: []\n    shapes:\n    - {firstVertex: 0, vertexCount: 0, hasNormals: 1}\n    channels:\n    - {name: s, frameIndex: 0, frameCount: 1}\n    fullWeights: [100]\n";
    // A vertex count with no vertex data: no channel reads it, and blend
    // shapes expand to the vertices there are.
    let huge = mesh(
        "    m_VertexCount: 1000000000000\n    m_Channels: []\n    _typelessdata: ''\n",
        shapes,
    )
    .unwrap();
    assert_eq!(huge.blend_shapes[0].positions.len(), 0);
    assert!(
        mesh(
            "    m_VertexCount: -1\n    m_Channels: []\n    _typelessdata: ''\n",
            shapes
        )
        .is_err()
    );
    let stream = "    m_VertexCount: 4\n    m_Channels:\n    - {stream: 1000000000000000, offset: 0, format: 0, dimension: 3}\n    _typelessdata: ''\n";
    assert!(
        mesh(stream, shapes)
            .unwrap_err()
            .message()
            .contains("stream out of range")
    );
    let long = "    m_VertexCount: 4611686018427387904\n    m_Channels:\n    - {stream: 0, offset: 0, format: 0, dimension: 3}\n    _typelessdata: ''\n";
    assert!(mesh(long, shapes).is_err());

    let texture = UnityFile::parse(
        "--- !u!28 &1\nTexture2D:\n  m_Name: T\n  m_TextureFormat: 4\n  m_Width: 4294967295\n  m_Height: 4294967295\n  image data: ''\n",
    )
    .unwrap();
    assert!(UnityTexture::from_object(&texture.objects[0]).is_err());

    let terrain = UnityFile::parse(
        "--- !u!156 &1\nTerrainData:\n  m_Name: T\n  m_Heightmap:\n    m_Resolution: 4294967296\n    m_Heights: []\n    m_Scale: {x: 1, y: 1, z: 1}\n",
    )
    .unwrap();
    assert!(UnityTerrain::from_object(&terrain.objects[0]).is_err());
}

/// Blend shape channels each own a range of frames: many channels naming
/// the same frames (of a mesh with no vertices, so nothing else is
/// charged) would make channels times frames shapes from a small file.
#[test]
fn blend_shape_channels_sharing_frames_are_errors() {
    let mesh = |channels: &str| {
        let text = format!(
            "--- !u!43 &1\nMesh:\n  m_Name: M\n  m_VertexData:\n    m_VertexCount: 0\n    m_Channels: []\n    _typelessdata: ''\n  m_Shapes:\n    vertices: []\n    shapes:\n{}    channels:\n{channels}    fullWeights: []\n  m_SubMeshes: []\n  m_IndexBuffer: ''\n",
            "    - {firstVertex: 0, vertexCount: 0, hasNormals: 0}\n".repeat(2000)
        );
        UnityMesh::from_object(&UnityFile::parse(&text).unwrap().objects[0])
    };
    let apart =
        "    - {name: a, frameIndex: 0, frameCount: 1000}\n    - {name: b, frameIndex: 1000, frameCount: 1000}\n";
    assert_eq!(mesh(apart).unwrap().blend_shapes.len(), 2000);
    let shared = "    - {name: s, frameIndex: 0, frameCount: 2000}\n".repeat(2000);
    let started = std::time::Instant::now();
    let err = mesh(&shared).unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(err.message().contains("share frames"), "{err}");
    let partly = "    - {name: a, frameIndex: 0, frameCount: 10}\n    - {name: b, frameIndex: 5, frameCount: 10}\n";
    assert!(mesh(partly).is_err());
}
