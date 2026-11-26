//! Importing a small hand-written Unity project into Ponko scene files.

use ponko_unity::*;
use serde_json::Value as Json;

/// A Unity project in a temp dir of its own (tests run in parallel), and
/// an output folder in it.
struct Fixture {
    dir: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const MATERIAL: &str = "%YAML 1.1
--- !u!21 &2100000
Material:
  m_Name: Bark
  m_Shader: {fileID: 4800000, guid: lit00000, type: 3}
  m_SavedProperties:
    m_TexEnvs:
    - _BaseMap:
        m_Texture: {fileID: 2800000, guid: bark0000, type: 3}
        m_Scale: {x: 2, y: 1}
        m_Offset: {x: 0, y: 0}
    - _MetallicGlossMap:
        m_Texture: {fileID: 2800000, guid: gloss000, type: 3}
        m_Scale: {x: 1, y: 1}
        m_Offset: {x: 0, y: 0}
    m_Floats:
    - _Smoothness: 0.5
    - _AlphaClip: 1
    - _Cutoff: 0.3
    - _Cull: 0
    m_Colors:
    - _BaseColor: {r: 1, g: 0.5, b: 0, a: 1}
";

/// A prefab: a root at (1, 2, 3) turned 90° around Y, with a child at
/// (0, 0, 1) drawing the quad; a LOD 1 child, an inactive one and one
/// that only casts shadows that draw it too, and a model instance.
const PREFAB: &str = "%YAML 1.1
--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 2}
  - component: {fileID: 9}
  m_Name: Root
  m_IsActive: 1
--- !u!4 &2
Transform:
  m_GameObject: {fileID: 1}
  m_LocalRotation: {x: 0, y: 0.70710677, z: 0, w: 0.70710677}
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 4}
  - {fileID: 14}
  - {fileID: 24}
  - {fileID: 34}
  m_Father: {fileID: 0}
--- !u!205 &9
LODGroup:
  m_GameObject: {fileID: 1}
  m_LODs:
  - renderers:
    - renderer: {fileID: 6}
  - renderers:
    - renderer: {fileID: 16}
--- !u!1 &3
GameObject:
  m_Component:
  - component: {fileID: 4}
  - component: {fileID: 5}
  - component: {fileID: 6}
  m_Name: Near
  m_IsActive: 1
--- !u!4 &4
Transform:
  m_GameObject: {fileID: 3}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 1}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!33 &5
MeshFilter:
  m_GameObject: {fileID: 3}
  m_Mesh: {fileID: 4300000, guid: quad0000, type: 2}
--- !u!23 &6
MeshRenderer:
  m_GameObject: {fileID: 3}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
--- !u!1 &13
GameObject:
  m_Component:
  - component: {fileID: 14}
  - component: {fileID: 15}
  - component: {fileID: 16}
  m_Name: Far
  m_IsActive: 1
--- !u!4 &14
Transform:
  m_GameObject: {fileID: 13}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!33 &15
MeshFilter:
  m_GameObject: {fileID: 13}
  m_Mesh: {fileID: 4300000, guid: quad0000, type: 2}
--- !u!23 &16
MeshRenderer:
  m_GameObject: {fileID: 13}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
--- !u!1 &23
GameObject:
  m_Component:
  - component: {fileID: 24}
  - component: {fileID: 25}
  - component: {fileID: 26}
  m_Name: Hidden
  m_IsActive: 0
--- !u!4 &24
Transform:
  m_GameObject: {fileID: 23}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!33 &25
MeshFilter:
  m_GameObject: {fileID: 23}
  m_Mesh: {fileID: 4300000, guid: quad0000, type: 2}
--- !u!23 &26
MeshRenderer:
  m_GameObject: {fileID: 23}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
--- !u!1 &33
GameObject:
  m_Component:
  - component: {fileID: 34}
  - component: {fileID: 35}
  - component: {fileID: 36}
  m_Name: Shadow
  m_IsActive: 1
--- !u!4 &34
Transform:
  m_GameObject: {fileID: 33}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!33 &35
MeshFilter:
  m_GameObject: {fileID: 33}
  m_Mesh: {fileID: 4300000, guid: quad0000, type: 2}
--- !u!23 &36
MeshRenderer:
  m_GameObject: {fileID: 33}
  m_Enabled: 1
  m_CastShadows: 3
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
";

fn fixture(name: &str) -> (Fixture, Project) {
    fixture_with(name, PREFAB)
}

fn fixture_with(name: &str, prefab: &str) -> (Fixture, Project) {
    let dir = std::env::temp_dir().join(format!("ponko_unity_import_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = dir.join("Assets");
    std::fs::create_dir_all(&assets).unwrap();
    let write = |file: &str, text: &[u8], guid: &str| {
        std::fs::write(assets.join(file), text).unwrap();
        std::fs::write(assets.join(format!("{file}.meta")), format!("guid: {guid}\n")).unwrap();
    };
    write("Quad.asset", include_bytes!("fixtures/quad.asset"), "quad0000");
    write("Bark.mat", MATERIAL.as_bytes(), "bark0mat");
    write("Tree.prefab", prefab.as_bytes(), "tree0000");
    write("Lit.shader", b"Shader \"Lit\" {}", "lit00000");
    let png = |pixel: [u8; 4], size: u32| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(size, size, image::Rgba(pixel))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    write("Bark.png", &png([200, 100, 50, 255], 8), "bark0000");
    // Metalness 1 in red, smoothness 0.8 in alpha.
    write("Gloss.png", &png([255, 0, 0, 204], 4), "gloss000");
    // Textures over the size limit their settings give are made smaller.
    std::fs::write(
        assets.join("Bark.png.meta"),
        "guid: bark0000\nTextureImporter:\n  maxTextureSize: 4\n",
    )
    .unwrap();
    let project = Project::open(&dir).unwrap();
    (Fixture { dir }, project)
}

/// The JSON chunk of a `.glb`.
fn glb_json(bytes: &[u8]) -> Json {
    assert_eq!(&bytes[..4], b"glTF");
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}

fn import(name: &str) -> (Fixture, Json, Report) {
    let (fixture, project) = fixture(name);
    let out = fixture.dir.join("out");
    let file = UnityFile::read(project.root().join("Assets/Tree.prefab")).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, &out, "trees");
    importer.lod_groups(false);
    let scene = importer.scene(&hierarchy).unwrap();
    let report = importer.finish().unwrap();
    (fixture, scene, report)
}

/// A renderer child of the group's root: object `id` (components `id + 1`
/// to `id + 3`), at local `position`, with Cast Shadows `cast`.
fn renderer_object(id: i64, position: [f32; 3], cast: i64) -> String {
    format!(
        "--- !u!1 &{id}
GameObject:
  m_Component:
  - component: {{fileID: {t}}}
  - component: {{fileID: {f}}}
  - component: {{fileID: {r}}}
  m_Name: Part{id}
  m_IsActive: 1
--- !u!4 &{t}
Transform:
  m_GameObject: {{fileID: {id}}}
  m_LocalRotation: {{x: 0, y: 0, z: 0, w: 1}}
  m_LocalPosition: {{x: {}, y: {}, z: {}}}
  m_LocalScale: {{x: 1, y: 1, z: 1}}
  m_Children: []
  m_Father: {{fileID: 2}}
--- !u!33 &{f}
MeshFilter:
  m_GameObject: {{fileID: {id}}}
  m_Mesh: {{fileID: 4300000, guid: quad0000, type: 2}}
--- !u!23 &{r}
MeshRenderer:
  m_GameObject: {{fileID: {id}}}
  m_Enabled: 1
  m_CastShadows: {cast}
  m_Materials:
  - {{fileID: 2100000, guid: bark0mat, type: 2}}
",
        position[0],
        position[1],
        position[2],
        t = id + 1,
        f = id + 2,
        r = id + 3,
    )
}

/// A prefab of a synthetic LOD group: a root at (10, 0, 5) scaled by 2 with
/// four levels. LOD 0 (renderer 13) and 3 (43) cast their own shadows; LOD 1
/// has a part with shadows off (23) and a shadows-only stand-in (33).
/// `group` is what follows `m_GameObject` in the LODGroup.
fn lod_prefab(group: &str) -> String {
    let mut text = format!(
        "%YAML 1.1
--- !u!1 &1
GameObject:
  m_Component:
  - component: {{fileID: 2}}
  - component: {{fileID: 9}}
  m_Name: Root
  m_IsActive: 1
--- !u!4 &2
Transform:
  m_GameObject: {{fileID: 1}}
  m_LocalRotation: {{x: 0, y: 0, z: 0, w: 1}}
  m_LocalPosition: {{x: 10, y: 0, z: 5}}
  m_LocalScale: {{x: 2, y: 2, z: 2}}
  m_Children:
  - {{fileID: 11}}
  - {{fileID: 21}}
  - {{fileID: 31}}
  - {{fileID: 41}}
  m_Father: {{fileID: 0}}
--- !u!205 &9
LODGroup:
  m_GameObject: {{fileID: 1}}
{group}"
    );
    text += &renderer_object(10, [0.0, 1.0, 1.0], 1);
    text += &renderer_object(20, [0.0, 0.0, 0.0], 0);
    text += &renderer_object(30, [1.0, 0.0, 0.0], 3);
    text += &renderer_object(40, [0.0, 0.0, 0.0], 2);
    text
}

const LOD_GROUP: &str = "  m_Size: 4
  m_LocalReferencePoint: {x: 0, y: 1, z: 2}
  m_FadeMode: 1
  m_AnimateCrossFading: 0
  m_LODs:
  - screenRelativeHeight: 0.6
    fadeTransitionWidth: 0.25
    renderers:
    - renderer: {fileID: 13}
  - screenRelativeHeight: 0.3
    fadeTransitionWidth: 0
    renderers:
    - renderer: {fileID: 23}
    - renderer: {fileID: 33}
  - screenRelativeHeight: 0.1
    fadeTransitionWidth: 0.5
    renderers: []
  - screenRelativeHeight: 0.02
    fadeTransitionWidth: 0.4
    renderers:
    - renderer: {fileID: 43}
";

fn import_lod(name: &str, group: &str, configure: impl FnOnce(&mut Importer)) -> (Fixture, Json, Report) {
    let (fixture, project) = fixture_with(name, &lod_prefab(group));
    let out = fixture.dir.join("out");
    let file = UnityFile::read(project.root().join("Assets/Tree.prefab")).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, &out, "trees");
    configure(&mut importer);
    let scene = importer.scene(&hierarchy).unwrap();
    let report = importer.finish().unwrap();
    (fixture, scene, report)
}

fn numbers(v: &Json) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect()
}

fn close(got: &[f64], want: &[f64]) {
    assert!(
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5),
        "{got:?} vs {want:?}"
    );
}

#[test]
fn a_lod_group_keeps_every_level_under_one_entity() {
    let (_f, scene, report) = import_lod("lod", LOD_GROUP, |_| {});
    let entities = scene["entities"].as_array().unwrap();
    assert_eq!(
        entities.len(),
        1,
        "one group; no renderer is an entity of its own: {scene}"
    );
    assert_eq!((report.lod_groups, report.entities), (1, 1));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let c = &entities[0]["components"];
    assert!(c["Model"].is_null() && c["ShadowMesh"].is_null());
    // The root, mirrored along Z.
    close(&numbers(&c["Transform3D"]["position"]), &[10.0, 0.0, -5.0]);
    close(&numbers(&c["Transform3D"]["scale"]), &[2.0, 2.0, 2.0]);
    let g = &c["LodGroup"];
    assert_eq!(g["size"], 4.0);
    close(&numbers(&g["center"]), &[0.0, 1.0, -2.0]);
    let levels = g["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 4);
    let heights: Vec<f64> = levels.iter().map(|l| l["screen_height"].as_f64().unwrap()).collect();
    close(&heights, &[0.6, 0.3, 0.1, 0.02]);
    let widths: Vec<f64> = levels.iter().map(|l| l["fade_width"].as_f64().unwrap()).collect();
    close(&widths, &[0.25, 0.0, 0.5, 0.4]);
    // LOD 0's part is at (0, 1, 1) in the root's space, world (10, 2, 7)
    // in Unity: in the group's own space it's (0, 1, -1) again, mirrored.
    let part = &levels[0]["renderers"][0];
    assert!(part["mesh"].as_str().unwrap().starts_with("trees/meshes/"));
    close(&numbers(&part["offset"]["position"]), &[0.0, 1.0, -1.0]);
    close(&numbers(&part["offset"]["scale"]), &[1.0, 1.0, 1.0]);
    // An empty level stays: the group draws nothing there.
    assert!(levels[2]["renderers"].as_array().unwrap().is_empty());
    assert_eq!(levels[3]["renderers"].as_array().unwrap().len(), 1);
}

#[test]
fn lod_levels_cast_the_shadows_their_renderers_ask_for() {
    let (_f, scene, _) = import_lod("lodshadow", LOD_GROUP, |_| {});
    let levels = scene["entities"][0]["components"]["LodGroup"]["levels"]
        .as_array()
        .unwrap();
    assert_eq!(levels[0]["shadow"], "Same");
    assert_eq!(levels[3]["shadow"], "Same", "Two Sided casts too");
    // LOD 1: the part with shadows off is drawn but casts none; the
    // shadows-only one casts without being drawn.
    let shown = levels[1]["renderers"].as_array().unwrap();
    assert_eq!(shown.len(), 1);
    close(&numbers(&shown[0]["offset"]["position"]), &[0.0, -0.0, 0.0]);
    let casters = levels[1]["shadow"]["Casters"].as_array().unwrap();
    assert_eq!(casters.len(), 1);
    // Unity (1, 0, 0) in the root's space, scaled by the root's 2, then
    // divided by it again in the group's space.
    close(&numbers(&casters[0]["offset"]["position"]), &[1.0, 0.0, 0.0]);
}

#[test]
fn unfaded_groups_switch_at_once() {
    let none = LOD_GROUP.replace("m_FadeMode: 1", "m_FadeMode: 0");
    let (_f, scene, _) = import_lod("lodnofade", &none, |_| {});
    let c = &scene["entities"][0]["components"];
    let levels = c["LodGroup"]["levels"].as_array().unwrap();
    assert!(
        levels.iter().all(|l| l["fade_width"] == 0.0),
        "no fade mode: switches at once"
    );
    // Animate Cross-fading without a fade mode fades nothing either.
    let still = none.replace("m_AnimateCrossFading: 0", "m_AnimateCrossFading: 1");
    let (_f, scene, _) = import_lod("lodnofadeanim", &still, |_| {});
    assert!(scene["entities"][0]["components"]["LodFadeTime"].is_null());
}

#[test]
fn animated_cross_fades_fade_over_time() {
    let animated = LOD_GROUP.replace("m_AnimateCrossFading: 0", "m_AnimateCrossFading: 1");
    let (_f, scene, report) = import_lod("lodanim", &animated, |_| {});
    let c = &scene["entities"][0]["components"];
    // Unity's default duration, and no band fades: the switch is what fades.
    assert_eq!(c["LodFadeTime"], 0.5);
    let levels = c["LodGroup"]["levels"].as_array().unwrap();
    assert!(levels.iter().all(|l| l["fade_width"] == 0.0), "{levels:?}");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    // The duration a game's script set for the project.
    let (_f, scene, _) = import_lod("lodanimtime", &animated, |i| i.lod_fade_time(0.25));
    assert_eq!(scene["entities"][0]["components"]["LodFadeTime"], 0.25);
    // Not animated: no fade time.
    let (_f, scene, _) = import_lod("lodnotanim", LOD_GROUP, |_| {});
    assert!(scene["entities"][0]["components"]["LodFadeTime"].is_null());
}

#[test]
fn lod_groups_can_be_turned_off_to_keep_lod_0_only() {
    let (_f, scene, report) = import_lod("lodoff", LOD_GROUP, |i| i.lod_groups(false));
    let entities = scene["entities"].as_array().unwrap();
    // Only LOD 0's renderer; the other three levels' are left out.
    assert_eq!(entities.len(), 1, "{scene}");
    assert!(entities[0]["components"]["Model"]["mesh"].is_string());
    assert!(entities[0]["components"]["LodGroup"].is_null());
    assert_eq!(report.skipped["a far level of detail (LOD 1+)"], 3);
    assert_eq!(report.lod_groups, 0);
}

#[test]
fn groups_are_on_by_default() {
    let (fixture, project) = fixture("lodon");
    let file = UnityFile::read(project.root().join("Assets/Tree.prefab")).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, fixture.dir.join("out"), "trees");
    let scene = importer.scene(&hierarchy).unwrap();
    let entities = scene["entities"].as_array().unwrap();
    // The shadow-only part outside the group, and the group of the two renderers.
    assert_eq!(entities.len(), 2, "{scene}");
    let group = &entities[1]["components"]["LodGroup"];
    assert_eq!(group["levels"].as_array().unwrap().len(), 2);
    assert_eq!(importer.report().lod_groups, 1);
    assert!(!importer.report().skipped.contains_key("a far level of detail (LOD 1+)"));
}

#[test]
fn drawn_meshes_become_models_placed_in_ponkos_space() {
    let (_f, scene, report) = import("place");
    assert_eq!(scene["format"], "ponko-world");
    let entities = scene["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 2, "the far LOD and the inactive child are left out");
    assert_eq!(report.skipped["a far level of detail (LOD 1+)"], 1);
    assert_eq!(report.skipped["inactive"], 1);
    // Shadows Only: a shadow with no model.
    let shadow = &entities[1]["components"];
    assert!(shadow["Model"].is_null(), "{shadow}");
    assert!(
        shadow["ShadowMesh"]["mesh"].as_str().unwrap().ends_with(".glb"),
        "{shadow}"
    );
    assert!(
        entities[0]["components"]["ShadowMesh"].is_null(),
        "it casts its own shadow"
    );
    let t = &entities[0]["components"]["Transform3D"];
    // In Unity the child is at (1, 2, 3) + the root's turn applied to
    // (0, 0, 1) = (2, 2, 3); mirrored along Z that's (2, 2, -3).
    let p: Vec<f64> = t["position"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    for (got, want) in p.iter().zip([2.0, 2.0, -3.0]) {
        assert!((got - want).abs() < 1e-5, "{p:?}");
    }
    // The turn goes the other way: y and w of opposite signs (q and -q
    // are the same rotation).
    let r: Vec<f64> = t["rotation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert!((r[1] * r[3] + 0.5).abs() < 1e-5, "{r:?}");
    assert_eq!(
        entities[0]["components"]["Model"]["mesh"]
            .as_str()
            .unwrap()
            .split('/')
            .next(),
        Some("trees")
    );
    assert_eq!(report.shaders["Assets/Lit.shader"], 1);
}

#[test]
fn meshes_are_mirrored_and_their_materials_converted() {
    let (f, scene, report) = import("mesh");
    assert_eq!((report.meshes, report.textures), (1, 2));
    let name = scene["entities"][0]["components"]["Model"]["mesh"].as_str().unwrap();
    let glb = std::fs::read(f.dir.join("out").join(name.strip_prefix("trees/").unwrap())).unwrap();
    let doc = glb_json(&glb);
    let unity =
        UnityMesh::from_object(&UnityFile::parse(include_str!("fixtures/quad.asset")).unwrap().objects[0]).unwrap();
    // Two submeshes, both drawn with the one material.
    let primitives = doc["meshes"][0]["primitives"].as_array().unwrap();
    assert_eq!(primitives.len(), unity.submeshes.len());
    assert!(primitives.iter().all(|p| p["material"] == 0 || p["material"] == 1));
    let position = &doc["accessors"][primitives[0]["attributes"]["POSITION"].as_u64().unwrap() as usize];
    let unity_min_z = unity.positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
    let unity_max_z = unity.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
    assert_eq!(position["min"][2].as_f64().unwrap() as f32, -unity_max_z);
    assert_eq!(position["max"][2].as_f64().unwrap() as f32, -unity_min_z);

    // The material: linear color, cut out, two-sided, with an ORM image.
    let m = &doc["materials"][0];
    let color = m["pbrMetallicRoughness"]["baseColorFactor"].as_array().unwrap();
    assert!(
        (color[1].as_f64().unwrap() - 0.214).abs() < 1e-3,
        "sRGB 0.5 is linear 0.214"
    );
    assert_eq!(m["alphaMode"], "MASK");
    assert!((m["alphaCutoff"].as_f64().unwrap() - 0.3).abs() < 1e-6);
    assert_eq!(m["doubleSided"], true);
    assert_eq!(m["pbrMetallicRoughness"]["metallicFactor"], 1.0);
    let images: Vec<&str> = doc["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["uri"].as_str().unwrap())
        .collect();
    assert_eq!(images[0], "textures/bark0000.png");
    let textures = f.dir.join("out/meshes");
    let orm = image::open(textures.join(images[1])).unwrap().to_rgba8();
    // No occlusion: 1; roughness 1 - 0.8 * 0.5 = 0.6; metalness 1.
    assert_eq!(orm.get_pixel(0, 0).0, [255, 153, 255, 255]);
    let bark = image::open(textures.join(images[0])).unwrap();
    assert_eq!(bark.width(), 4, "made as small as its settings say");
}

#[test]
fn shared_meshes_and_materials_are_written_once() {
    let (fixture, project) = fixture("shared");
    let file = UnityFile::read(project.root().join("Assets/Tree.prefab")).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, fixture.dir.join("out"), "trees/");
    let a = importer.scene(&hierarchy).unwrap();
    let b = importer.scene(&hierarchy).unwrap();
    assert_eq!(
        a["entities"][0]["components"]["Model"],
        b["entities"][0]["components"]["Model"]
    );
    let report = importer.finish().unwrap();
    assert_eq!((report.entities, report.meshes, report.textures), (4, 1, 2));
}

/// A prefab drawing Unity's built-in Cube and Sphere.
const BUILTINS: &str = "%YAML 1.1
--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 2}
  - component: {fileID: 3}
  - component: {fileID: 4}
  m_Name: Box
  m_IsActive: 1
--- !u!4 &2
Transform:
  m_GameObject: {fileID: 1}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 12}
  m_Father: {fileID: 0}
--- !u!33 &3
MeshFilter:
  m_GameObject: {fileID: 1}
  m_Mesh: {fileID: 10202, guid: 0000000000000000e000000000000000, type: 0}
--- !u!23 &4
MeshRenderer:
  m_GameObject: {fileID: 1}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
--- !u!1 &11
GameObject:
  m_Component:
  - component: {fileID: 12}
  - component: {fileID: 13}
  - component: {fileID: 14}
  m_Name: Ball
  m_IsActive: 1
--- !u!4 &12
Transform:
  m_GameObject: {fileID: 11}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 1, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!33 &13
MeshFilter:
  m_GameObject: {fileID: 11}
  m_Mesh: {fileID: 10207, guid: 0000000000000000e000000000000000, type: 0}
--- !u!23 &14
MeshRenderer:
  m_GameObject: {fileID: 11}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: bark0mat, type: 2}
";

#[test]
fn built_in_cubes_are_made_and_spheres_reported() {
    let (fixture, project) = fixture("builtin");
    let file = UnityFile::parse(BUILTINS).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, fixture.dir.join("out"), "box/");
    let scene = importer.scene(&hierarchy).unwrap();
    let report = importer.finish().unwrap();
    let entities = scene["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 1);
    assert_eq!(report.skipped["built-in mesh Sphere"], 1);
    let name = entities[0]["components"]["Model"]["mesh"].as_str().unwrap();
    let glb = std::fs::read(fixture.dir.join("out").join(name.strip_prefix("box/").unwrap())).unwrap();
    let doc = glb_json(&glb);
    let position = &doc["accessors"][doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
        .as_u64()
        .unwrap() as usize];
    assert_eq!(position["count"], 24);
    assert_eq!(position["min"], serde_json::json!([-0.5, -0.5, -0.5]));
}

#[test]
fn custom_shader_materials_are_described_for_the_game() {
    let (fixture, project) = fixture("custom");
    let out = fixture.dir.join("out");
    let file = UnityFile::read(project.root().join("Assets/Tree.prefab")).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, &out, "trees");
    importer.custom_shader("Assets/Lit.shader");
    let scene = importer.scene(&hierarchy).unwrap();
    importer.finish().unwrap();
    let described: Json = serde_json::from_str(&std::fs::read_to_string(out.join("materials.json")).unwrap()).unwrap();
    let bark = &described["Bark"];
    assert_eq!(bark["shader"], "Assets/Lit.shader");
    assert_eq!(
        bark["floats"]["_Cutoff"].as_f64().map(|f| (f * 10.0).round()),
        Some(3.0)
    );
    assert_eq!(bark["colors"]["_BaseColor"], serde_json::json!([1.0, 0.5, 0.0, 1.0]));
    // Every texture, written where the file says, with its tiling.
    let base = &bark["textures"]["_BaseMap"];
    assert_eq!(base["scale"], serde_json::json!([2.0, 1.0]));
    let file = base["file"].as_str().unwrap();
    assert!(out.join(file.strip_prefix("trees/").unwrap()).is_file(), "{file}");
    assert!(bark["textures"]["_MetallicGlossMap"]["file"].is_string());
    // The glTF material has the name materials.json uses.
    let name = scene["entities"][1]["components"]["LodGroup"]["levels"][0]["renderers"][0]["mesh"]
        .as_str()
        .unwrap();
    let glb = std::fs::read(out.join(name.strip_prefix("trees/").unwrap())).unwrap();
    assert_eq!(glb_json(&glb)["materials"][0]["name"], "Bark");
}

#[test]
fn without_custom_shaders_there_is_no_materials_file() {
    let (f, _, _) = import("plain");
    assert!(!f.dir.join("out/materials.json").exists());
}

/// A wall: a box collider on an object at (0, 1, 2) scaled (2, 1, 1),
/// with a mesh collider on a child and a trigger on another.
const WALL: &str = "%YAML 1.1
--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 2}
  - component: {fileID: 3}
  m_Name: Wall
  m_IsActive: 1
--- !u!4 &2
Transform:
  m_GameObject: {fileID: 1}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 1, z: 2}
  m_LocalScale: {x: 2, y: 1, z: 1}
  m_Children:
  - {fileID: 12}
  m_Father: {fileID: 0}
--- !u!65 &3
BoxCollider:
  m_GameObject: {fileID: 1}
  m_IsTrigger: 0
  m_Enabled: 1
  m_Size: {x: 1, y: 2, z: 3}
  m_Center: {x: 0.5, y: 0, z: 1}
--- !u!1 &11
GameObject:
  m_Component:
  - component: {fileID: 12}
  - component: {fileID: 13}
  - component: {fileID: 14}
  m_Name: Rock
  m_IsActive: 1
--- !u!4 &12
Transform:
  m_GameObject: {fileID: 11}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 2}
--- !u!64 &13
MeshCollider:
  m_GameObject: {fileID: 11}
  m_IsTrigger: 0
  m_Enabled: 1
  m_Convex: 1
  m_Mesh: {fileID: 4300000, guid: quad0000, type: 2}
--- !u!135 &14
SphereCollider:
  m_GameObject: {fileID: 11}
  m_IsTrigger: 1
  m_Enabled: 1
  m_Radius: 3
  m_Center: {x: 0, y: 0, z: 0}
";

#[test]
fn colliders_become_fixed_bodies_with_their_objects_scale() {
    let (fixture, project) = fixture("colliders");
    let file = UnityFile::parse(WALL).unwrap();
    let hierarchy = Hierarchy::build(&file, &project).unwrap();
    let mut importer = Importer::new(&project, fixture.dir.join("out"), "wall/");
    let without = importer.scene(&hierarchy).unwrap();
    assert!(without["entities"].as_array().unwrap().is_empty(), "off unless asked");
    importer.colliders(true);
    let scene = importer.scene(&hierarchy).unwrap();
    let entities = scene["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 2, "the trigger is left out");
    let wall = &entities[0]["components"];
    assert_eq!(wall["RigidBody3D"]["kind"], "Fixed");
    assert_eq!(wall["Transform3D"]["position"], serde_json::json!([0.0, 1.0, -2.0]));
    assert_eq!(wall["Transform3D"]["scale"], serde_json::json!([1.0, 1.0, 1.0]));
    assert_eq!(
        wall["Collider3D"]["shape"],
        serde_json::json!({ "Cuboid": { "size": [2.0, 2.0, 3.0] } }),
        "scaled by the object"
    );
    assert_eq!(wall["Collider3D"]["offset"], serde_json::json!([1.0, 0.0, -1.0]));
    let rock = &entities[1]["components"]["Collider3D"]["shape"]["Mesh"];
    assert_eq!(rock["convex"], true);
    assert_eq!(rock["scale"], serde_json::json!([2.0, 1.0, 1.0]));
    let mesh = rock["file"].as_str().unwrap();
    assert!(mesh.starts_with("wall/meshes/"), "{mesh}");
    assert!(
        fixture
            .dir
            .join("out")
            .join(mesh.strip_prefix("wall/").unwrap())
            .exists()
    );
    let report = importer.finish().unwrap();
    assert_eq!(report.colliders, 2);
    assert_eq!(report.skipped["a trigger collider"], 1);
}
