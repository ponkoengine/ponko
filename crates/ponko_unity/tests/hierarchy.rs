//! Flattening scenes with nested prefabs: overrides, removed and added
//! objects, stripped stand-ins, and imported model instances.

use ponko_unity::*;

/// A lamp prefab: "Lamp" (light component 108) with a child "Bulb".
const LAMP: &str = "%YAML 1.1
--- !u!1 &10
GameObject:
  m_Name: Lamp
  m_IsActive: 1
  m_Layer: 0
--- !u!4 &11
Transform:
  m_GameObject: {fileID: 10}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 21}
  m_Father: {fileID: 0}
--- !u!108 &12
Light:
  m_GameObject: {fileID: 10}
  m_Intensity: 1
--- !u!1 &20
GameObject:
  m_Name: Bulb
  m_IsActive: 1
--- !u!4 &21
Transform:
  m_GameObject: {fileID: 20}
  m_LocalPosition: {x: 0, y: 2, z: 0}
  m_Father: {fileID: 11}
";

/// A scene: a "Street" with a lamp instance under it (moved, renamed,
/// light removed) and a "Moth" added under the lamp's bulb; plus a
/// model instance.
const SCENE: &str = "%YAML 1.1
--- !u!1 &1
GameObject:
  m_Name: Street
  m_IsActive: 1
--- !u!4 &2
Transform:
  m_GameObject: {fileID: 1}
  m_LocalPosition: {x: 100, y: 0, z: 0}
  m_Father: {fileID: 0}
--- !u!1001 &500
PrefabInstance:
  m_Modification:
    m_TransformParent: {fileID: 2}
    m_Modifications:
    - target: {fileID: 11, guid: lamp0000, type: 3}
      propertyPath: m_LocalPosition.x
      value: 5
      objectReference: {fileID: 0}
    - target: {fileID: 10, guid: lamp0000, type: 3}
      propertyPath: m_Name
      value: Lamp (corner)
      objectReference: {fileID: 0}
    m_RemovedComponents:
    - {fileID: 12, guid: lamp0000, type: 3}
    m_RemovedGameObjects: []
  m_SourcePrefab: {fileID: 100100000, guid: lamp0000, type: 3}
--- !u!4 &517 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 21, guid: lamp0000, type: 3}
  m_PrefabInstance: {fileID: 500}
--- !u!1 &30
GameObject:
  m_Name: Moth
  m_IsActive: 0
--- !u!4 &31
Transform:
  m_GameObject: {fileID: 30}
  m_LocalPosition: {x: 0, y: 0.5, z: 0}
  m_Father: {fileID: 517}
--- !u!1001 &600
PrefabInstance:
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications:
    - target: {fileID: -800, guid: tree0000, type: 3}
      propertyPath: m_LocalPosition.x
      value: 7
      objectReference: {fileID: 0}
    - target: {fileID: -800, guid: tree0000, type: 3}
      propertyPath: m_LocalScale.y
      value: 3
      objectReference: {fileID: 0}
    - target: {fileID: 919, guid: tree0000, type: 3}
      propertyPath: m_Name
      value: Oak
      objectReference: {fileID: 0}
    - target: {fileID: -55, guid: tree0000, type: 3}
      propertyPath: m_Materials.Array.data[0]
      value:
      objectReference: {fileID: 2100000, guid: bark0000, type: 2}
  m_SourcePrefab: {fileID: 100100000, guid: tree0000, type: 3}
--- !u!4 &700 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: -900, guid: tree0000, type: 3}
  m_PrefabInstance: {fileID: 600}
--- !u!1 &40
GameObject:
  m_Name: Nest
  m_IsActive: 1
--- !u!4 &41
Transform:
  m_GameObject: {fileID: 40}
  m_Father: {fileID: 700}
";

/// A small project in its own temp dir: tests run in parallel, so each
/// passes its own `name`.
fn project(name: &str) -> (Project, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ponko_unity_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Assets")).unwrap();
    std::fs::write(dir.join("Assets/Lamp.prefab"), LAMP).unwrap();
    std::fs::write(dir.join("Assets/Lamp.prefab.meta"), "guid: lamp0000\n").unwrap();
    std::fs::write(dir.join("Assets/Tree.fbx"), "").unwrap();
    std::fs::write(dir.join("Assets/Tree.fbx.meta"), "guid: tree0000\n").unwrap();
    (Project::open(&dir).unwrap(), dir)
}

#[test]
fn nested_prefabs_expand_with_their_overrides() {
    let (project, dir) = project("nested");
    let scene = Hierarchy::build(&UnityFile::parse(SCENE).unwrap(), &project).unwrap();
    let by_name = |n: &str| {
        scene
            .objects
            .iter()
            .position(|o| o.name == n)
            .unwrap_or_else(|| panic!("no {n}"))
    };
    let (street, lamp, bulb, moth) = (
        by_name("Street"),
        by_name("Lamp (corner)"),
        by_name("Bulb"),
        by_name("Moth"),
    );
    let o = &scene.objects;
    assert_eq!(
        o[lamp].parent,
        Some(street),
        "the instance hangs under its transform parent"
    );
    assert_eq!(o[lamp].local_position(), [5.0, 0.0, 0.0]);
    assert!(o[lamp].component(108).is_none(), "the removed light is gone");
    assert_eq!(o[bulb].parent, Some(lamp));
    assert_eq!(o[bulb].local_position(), [0.0, 2.0, 0.0]);
    assert_eq!(
        o[moth].parent,
        Some(bulb),
        "added under the stripped stand-in of the bulb"
    );
    assert!(!o[moth].active);
    // Parents come before children.
    for (i, g) in o.iter().enumerate() {
        assert!(g.parent.is_none_or(|p| p < i), "{} before its parent", g.name);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn model_instances_keep_the_model_and_its_inner_overrides() {
    let (project, dir) = project("model");
    let scene = Hierarchy::build(&UnityFile::parse(SCENE).unwrap(), &project).unwrap();
    let oak = scene.objects.iter().find(|o| o.model.is_some()).unwrap();
    assert_eq!(oak.name, "Oak");
    assert_eq!(oak.model.as_ref().unwrap().guid.as_deref(), Some("tree0000"));
    assert_eq!(oak.local_position(), [7.0, 0.0, 0.0]);
    assert_eq!(oak.local_scale(), [1.0, 3.0, 1.0]);
    assert!(oak.parent.is_none());
    // Hung from a branch inside the model: under the model's root, with
    // the branch kept.
    let oak_index = scene.objects.iter().position(|o| o.model.is_some()).unwrap();
    let nest = scene.objects.iter().find(|o| o.name == "Nest").unwrap();
    assert_eq!(nest.parent, Some(oak_index));
    assert_eq!(nest.model_attach.as_ref().map(|r| r.file_id), Some(-900));
    let (target, path, value) = &oak.model_overrides[0];
    assert_eq!((target.file_id, path.as_str()), (-55, "m_Materials.Array.data[0]"));
    assert_eq!(value.obj_ref().unwrap().guid.as_deref(), Some("bark0000"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn prefabs_nesting_in_a_loop_are_an_error() {
    let dir = std::env::temp_dir().join(format!("ponko_unity_loop_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Assets")).unwrap();
    let selfish = "%YAML 1.1\n--- !u!1001 &1\nPrefabInstance:\n  m_Modification:\n    m_TransformParent: {fileID: 0}\n    m_Modifications: []\n  m_SourcePrefab: {fileID: 100100000, guid: loop0000, type: 3}\n";
    std::fs::write(dir.join("Assets/Loop.prefab"), selfish).unwrap();
    std::fs::write(dir.join("Assets/Loop.prefab.meta"), "guid: loop0000\n").unwrap();
    let project = Project::open(&dir).unwrap();
    let err = Hierarchy::load(&project, "loop0000").unwrap_err();
    assert!(err.message().contains("loop"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}
