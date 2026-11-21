//! 2D skeletons from Spine: bezier curves evaluated exactly (checked against
//! hand-computed values), texture atlases (rotated and trimmed regions), and
//! a warning or error for every Spine feature the importer doesn't read.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ponko::assets::MemorySource;
use ponko::prelude::*;
use ponko::skeleton2d::{Attachment, Bezier, Curve, Skeleton, SkeletonData, SpineAtlas, SpritePart};

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

fn near2(a: Vec2, b: Vec2) {
    assert!(a.distance(b) < 1e-3, "{a} != {b}");
}

// ---- Curves -------------------------------------------------------------

#[test]
fn beziers_ease_by_their_handles_exactly() {
    // x(s) = s for handles at 1/3 and 2/3, so ease(t) = y(t):
    // y(s) = 3(1-s)²s·1 + 3(1-s)s²·1 + s³.
    let fast_start = Bezier::new(1.0 / 3.0, 1.0, 2.0 / 3.0, 1.0);
    near(fast_start.ease(0.5), 0.875);
    near(fast_start.ease(0.25), 0.578_125);
    near(fast_start.ease(0.0), 0.0);
    near(fast_start.ease(1.0), 1.0);
    // CSS ease-in-out: at s = 1/4, x = 0.274375 and y = 0.15625.
    let ease_in_out = Bezier::new(0.42, 0.0, 0.58, 1.0);
    near(ease_in_out.ease(0.274_375), 0.156_25);
    near(Bezier::LINEAR.ease(0.3), 0.3);
    // A per-channel curve eases each channel by its own bezier.
    let curve = Curve::Channels([Bezier::LINEAR, fast_start, Bezier::LINEAR, Bezier::LINEAR]);
    near(curve.ease(0.5, 0), 0.5);
    near(curve.ease(0.5, 1), 0.875);
    near(Curve::Stepped.ease(0.9, 0), 0.0);
}

/// One bone `b` under `root`, animated by `timelines` in clip `a`.
fn spine(version: &str, timelines: &str) -> Skeleton {
    let text = format!(
        r#"{{ "skeleton": {{ "spine": "{version}" }},
             "bones": [{{ "name": "root" }}, {{ "name": "b", "parent": "root" }}],
             "animations": {{ "a": {{ "bones": {{ "b": {timelines} }} }} }} }}"#
    );
    let data = SkeletonData::from_spine_json(&text, "curves.json").unwrap();
    assert_eq!(data.warnings(), [] as [String; 0]);
    let mut s = Skeleton::new(Arc::new(data));
    s.play("a");
    s.update(0.0);
    s
}

fn at(s: &mut Skeleton, seconds: f32) -> &mut Skeleton {
    s.set_normalized_time(seconds / s.data().clips()[0].duration);
    s.update(0.0);
    s
}

#[test]
fn spine_4_bezier_keys_move_as_authored() {
    // 0 -> 90 degrees over a second, handles (1/3 s, 90) and (2/3 s, 90):
    // 90 · 0.875 = 78.75 degrees at half time (clockwise on screen here).
    let mut s = spine(
        "4.2.11",
        r#"{ "rotate": [ { "value": 0, "curve": [0.3333333, 90, 0.6666667, 90] }, { "time": 1, "value": 90 } ] }"#,
    );
    near(at(&mut s, 0.5).bone_rotation(1), -78.75f32.to_radians());
    near(at(&mut s, 0.25).bone_rotation(1), -(90.0 * 0.578_125f32).to_radians());

    // Per channel: (0, 0) -> (10, 20) over two seconds; x has its handles on
    // the straight line, y the fast start. At 1 s: x 5, y 20 · 0.875.
    let mut s = spine(
        "4.2.11",
        r#"{ "translate": [
            { "x": 0, "y": 0, "curve": [0.6666667, 0, 1.3333333, 10, 0.6666667, 20, 1.3333333, 20] },
            { "time": 2, "x": 10, "y": 20 } ] }"#,
    );
    near2(at(&mut s, 1.0).bone_position(1), vec2(5.0, -17.5));

    // A key to the same value that bulges out and back: 0 -> 0 with both
    // handles at 30 degrees, y(s) = 30 · 3s(1 - s).
    let mut s = spine(
        "4.2.11",
        r#"{ "rotate": [ { "value": 0, "curve": [0.3333333, 30, 0.6666667, 30] }, { "time": 1, "value": 0 } ] }"#,
    );
    near(at(&mut s, 0.5).bone_rotation(1), -22.5f32.to_radians());
    near(at(&mut s, 0.25).bone_rotation(1), -16.875f32.to_radians());
    near(at(&mut s, 1.0).bone_rotation(1), 0.0);
}

#[test]
fn spine_3_curves_and_shortest_way_rotations_import() {
    // 3.8 writes normalized handles as `curve`, `c2`, `c3`, `c4`.
    let mut s = spine(
        "3.8.99",
        r#"{ "rotate": [ { "time": 0, "angle": 0, "curve": 0.3333333, "c2": 1, "c3": 0.6666667, "c4": 1 },
                         { "time": 1, "angle": 80 } ] }"#,
    );
    near(at(&mut s, 0.5).bone_rotation(1), -70f32.to_radians());
    // Older 3.x: a four-number array.
    let mut s = spine(
        "3.6.53",
        r#"{ "rotate": [ { "time": 0, "angle": 0, "curve": [0.3333333, 1, 0.6666667, 1] },
                         { "time": 1, "angle": 80 } ] }"#,
    );
    near(at(&mut s, 0.5).bone_rotation(1), -70f32.to_radians());
    // 3.x turns the shorter way: 350 -> 10 passes through 0, not 180.
    let mut s = spine(
        "3.8.99",
        r#"{ "rotate": [ { "time": 0, "angle": 350 }, { "time": 1, "angle": 10 } ] }"#,
    );
    let r = at(&mut s, 0.5).bone_rotation(1);
    assert!(r.cos() > 0.9999, "{} degrees", r.to_degrees());
    // 4.x goes straight through the values, as Spine 4 does.
    let mut s = spine(
        "4.1.00",
        r#"{ "rotate": [ { "value": 350 }, { "time": 1, "value": 10 } ] }"#,
    );
    near(at(&mut s, 0.5).bone_rotation(1).cos(), -1.0);
}

const NATIVE: &str = r#"{ "format": "ponko-skeleton2d", "version": 1,
  "bones": [{ "name": "root" }, { "name": "b", "parent": "root" }],
  "clips": [{ "name": "a", "bones": [{ "bone": "b",
    "rotate": [ { "time": 0, "value": 0, "curve": CURVE }, { "time": 1, "value": 80 } ],
    "translate": [ { "time": 0, "value": [0, 0], "curve": { "bezier": [[0.25, 0, 0.75, 1], [0.3333333, 1, 0.6666667, 1]] } },
                   { "time": 1, "value": [10, 20] } ] }] }] }"#;

#[test]
fn the_native_format_has_bezier_curves_one_or_per_channel() {
    let text = NATIVE.replace("CURVE", r#"{ "bezier": [0.3333333, 1, 0.6666667, 1] }"#);
    let data = SkeletonData::from_json(&text, "native.json").unwrap();
    let track = &data.clips()[0].bones[0];
    assert_eq!(
        track.rotate[0].curve,
        Curve::Bezier(Bezier::new(0.333_333_3, 1.0, 0.666_666_7, 1.0))
    );
    let mut s = Skeleton::new(Arc::new(data));
    s.play("a");
    s.update(0.0);
    s.set_normalized_time(0.5);
    s.update(0.0);
    near(s.bone_rotation(1), 70f32.to_radians());
    // x: (0.25, 0, 0.75, 1) is symmetric, so 5 at half time; y: 20 · 0.875.
    near2(s.bone_position(1), vec2(5.0, 17.5));

    for (curve, message) in [
        (r#"{ "bezier": [1.5, 0, 0.75, 1] }"#, "bezier handles"),
        (r#"{ "bezier": [0.25, 0, -0.1, 1] }"#, "bezier handles"),
        (r#"{ "bezier": [] }"#, "one to four"),
        (r#""smooth""#, "unknown variant"),
    ] {
        let err = SkeletonData::from_json(&NATIVE.replace("CURVE", curve), "native.json").unwrap_err();
        assert!(err.to_string().contains(message), "{curve}: {err}");
    }
}

// ---- Atlases ------------------------------------------------------------

/// Spine 4.x spelling. `head` is packed turned and trimmed: its 24 x 34
/// image keeps 20 x 30, 1 px in from the left and 2 px up from the bottom.
const ATLAS_4: &str = "
hero.png
size: 64, 32
filter: Nearest, Nearest
pma: true
head
  bounds: 2, 4, 20, 30
  offsets: 1, 2, 24, 34
  rotate: 90
body
  bounds: 30, 0, 10, 10
  offsets: 1, 2, 12, 14
";

/// The same atlas as Spine 3.x writes it.
const ATLAS_3: &str = "
hero.png
size: 64,32
format: RGBA8888
filter: Nearest,Nearest
repeat: none
head
  rotate: true
  xy: 2, 4
  size: 20, 30
  orig: 24, 34
  offset: 1, 2
  index: -1
body
  rotate: false
  xy: 30, 0
  size: 10, 10
  orig: 12, 14
  offset: 1, 2
  index: -1
";

/// A Spine export using the atlas's regions: `head` as a region attachment
/// drawn at twice its image size, `body` as a mesh.
const HERO: &str = r#"{
  "skeleton": { "spine": "4.2.11" },
  "bones": [{ "name": "root" }],
  "slots": [
    { "name": "body", "bone": "root", "attachment": "body" },
    { "name": "head", "bone": "root", "attachment": "head" }
  ],
  "skins": [{ "name": "default", "attachments": {
    "head": { "head": { "width": 48, "height": 68 } },
    "body": { "body": { "type": "mesh", "uvs": [0, 0, 1, 0, 0, 1], "triangles": [0, 1, 2],
                        "vertices": [0, 0, 12, 0, 0, 14], "hull": 3, "width": 12, "height": 14 } }
  } }]
}"#;

fn head(data: &SkeletonData) -> &SpritePart {
    match data.attachment(0, 1, "head") {
        Some(Attachment::Sprite(part)) => part,
        other => panic!("a region attachment, not {other:?}"),
    }
}

#[test]
fn atlases_read_both_spellings_alike() {
    let four = SpineAtlas::parse(ATLAS_4, "hero/hero.atlas").unwrap();
    let three = SpineAtlas::parse(ATLAS_3, "hero/hero.atlas").unwrap();
    assert_eq!(four.images(), ["hero/hero.png"]);
    assert!(four.has_region("head") && four.has_region("body") && !four.has_region("tail"));
    let a = SkeletonData::from_spine_json_with_atlas(HERO, "hero/hero.json", &four).unwrap();
    let b = SkeletonData::from_spine_json_with_atlas(HERO, "hero/hero.json", &three).unwrap();
    assert_eq!(head(&a), head(&b));
    assert_eq!(a.attachment(0, 0, "body"), b.attachment(0, 0, "body"));
    assert!(a.premultiplied() && !b.premultiplied(), "only 4.x writes pma");
    assert_eq!(a.images(), ["hero/hero.png"], "the page, not loose images");
}

#[test]
fn rotated_and_trimmed_regions_keep_their_place_in_the_whole_image() {
    let atlas = SpineAtlas::parse(ATLAS_4, "hero/hero.atlas").unwrap();
    let data = SkeletonData::from_spine_json_with_atlas(HERO, "hero/hero.json", &atlas).unwrap();
    let part = head(&data);
    assert!(part.rotated);
    // Packed turned: 30 wide, 20 tall in the page.
    assert_eq!(part.region, Some(Rect::new(2.0, 4.0, 30.0, 20.0)));
    // Drawn at twice the image's size: the kept 20 x 30, doubled.
    assert_eq!(part.size, vec2(40.0, 60.0));
    // The whole image's center, (12, 17) from its bottom left, in the kept
    // part: (12 - 1) / 20 across, and (2 + 30 - 17) / 30 down from its top.
    near2(part.pivot, vec2(0.55, 0.5));
    assert_eq!(&*part.image, "hero/hero.png");

    // Mesh UVs map through the trim onto the page (64 x 32): the whole
    // image starts 1 px left of and 2 px above the kept part at (30, 0).
    let Some(Attachment::Mesh(mesh)) = data.attachment(0, 0, "body") else {
        panic!("a mesh");
    };
    near2(mesh.vertices[0].uv, vec2(29.0 / 64.0, -2.0 / 32.0));
    near2(mesh.vertices[1].uv, vec2(41.0 / 64.0, -2.0 / 32.0));
    near2(mesh.vertices[2].uv, vec2(29.0 / 64.0, 12.0 / 32.0));
}

#[test]
fn rotated_mesh_regions_turn_their_uvs() {
    // `head` as a mesh: its top-left corner was packed to the bottom left,
    // its top right to the top left (a quarter turn counter-clockwise).
    let hero = HERO.replace(
        r#""head": { "head": { "width": 48, "height": 68 } }"#,
        r#""head": { "head": { "type": "mesh", "uvs": [0, 0, 1, 0, 0, 1], "triangles": [0, 1, 2],
                                "vertices": [0, 0, 24, 0, 0, 34], "hull": 3 } }"#,
    );
    let atlas = SpineAtlas::parse(ATLAS_4, "hero/hero.atlas").unwrap();
    let data = SkeletonData::from_spine_json_with_atlas(&hero, "hero/hero.json", &atlas).unwrap();
    let Some(Attachment::Mesh(mesh)) = data.attachment(0, 1, "head") else {
        panic!("a mesh");
    };
    // The whole 24 x 34 image, turned, starts 2 px left of x = 2 (its top
    // margin) and 3 px above y = 4 (its right margin): 34 wide, 24 tall.
    near2(mesh.vertices[0].uv, vec2(0.0, 25.0 / 32.0));
    near2(mesh.vertices[1].uv, vec2(0.0, 1.0 / 32.0));
    near2(mesh.vertices[2].uv, vec2(34.0 / 64.0, 25.0 / 32.0));
}

#[test]
fn bad_atlases_are_errors_naming_the_file_and_what_is_wrong() {
    for (atlas, message) in [
        (
            ATLAS_4.replace("rotate: 90", "rotate: 180"),
            "hero.atlas, line 9: region \"head\" is packed turned 180",
        ),
        (
            ATLAS_4.replace("bounds: 30, 0, 10, 10", "bounds: 30, x, 10, 10"),
            "line 11: \"bounds\" needs 4",
        ),
    ] {
        let err = SpineAtlas::parse(&atlas, "hero/hero.atlas").unwrap_err();
        assert!(err.to_string().contains(message), "{err}");
    }
    let atlas = SpineAtlas::parse(&ATLAS_4.replace("head\n", "face\n"), "hero/hero.atlas").unwrap();
    let err = SkeletonData::from_spine_json_with_atlas(HERO, "hero/hero.json", &atlas).unwrap_err();
    assert!(err.to_string().contains("head"), "{err}");
    let atlas = SpineAtlas::parse(&ATLAS_4.replace("size: 64, 32\n", ""), "hero/hero.atlas").unwrap();
    let err = SkeletonData::from_spine_json_with_atlas(HERO, "hero/hero.json", &atlas).unwrap_err();
    assert!(err.to_string().contains("needs a \"size\""), "{err}");
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([255; 4]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn spine_exports_load_with_the_atlas_beside_them() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("spine")
                .with("hero/hero.json", HERO)
                .with("hero/hero.atlas", ATLAS_4)
                .with("hero/hero.png", png(64, 32)),
        );
        let data = ctx.load_skeleton2d("hero/hero.json");
        let texture = head(&data).texture;
        assert_eq!((texture.width(), texture.height()), (64, 32), "the page is loaded");
        assert_eq!(
            data.attachment(0, 0, "body").unwrap().texture(),
            texture,
            "one page, one texture"
        );
        assert!(data.premultiplied());
        assert_eq!(data.warnings(), [] as [String; 0]);

        // Without its atlas, a Spine export's regions are loose images.
        ctx.assets
            .add_source(MemorySource::new("loose").with("solo/hero.json", HERO));
        let err = ctx.try_load_skeleton2d("solo/hero.json").unwrap_err();
        assert!(err.to_string().contains("solo/body.png"), "{err}");
    });
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[test]
fn editing_an_atlas_reloads_the_skeletons_using_it() {
    let dir = std::env::temp_dir().join(format!("ponko-skeleton2d-atlas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir.join("hero.json"), HERO);
    write(&dir.join("hero.atlas"), ATLAS_4);
    std::fs::write(dir.join("hero.png"), png(64, 32)).unwrap();
    let atlas = dir.join("hero.atlas");
    let name = dir.join("hero.json").to_str().unwrap().to_string();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    Game::new("hot", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            ponko::systems::update_skeletons2d(ctx);
            for s in ctx.world.query::<&Skeleton>().iter() {
                log.lock().unwrap().push(head(s.data()).region.unwrap().min.x);
            }
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(Duration::from_millis(50));
                write(&atlas, &ATLAS_4.replace("bounds: 2, 4", "bounds: 6, 4"));
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .run_headless(5, move |ctx| {
            let data = ctx.load_skeleton2d(&name);
            ctx.world.spawn((Skeleton::new(data),));
        });
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.first(), Some(&2.0));
    assert_eq!(seen.last(), Some(&6.0), "{seen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- What isn't read ----------------------------------------------------

const BASE: &str = r#"{
  "skeleton": { "spine": "4.2.11" },
  "bones": [{ "name": "root" }, { "name": "arm", "parent": "root", "length": 10 }],
  "slots": [{ "name": "arm", "bone": "arm" }],
  "ik": [{ "name": "reach", "bones": ["arm"], "target": "root" }],
  "skins": [{ "name": "default", "attachments": { "arm": {} } }],
  "events": { "step": {} },
  "animations": { "wave": { "bones": { "arm": { "rotate": [{ "value": 0 }, { "time": 1, "value": 10 }] } } } }
}"#;

#[test]
fn every_spine_feature_not_read_warns_naming_it_and_the_file() {
    use serde_json::json;
    let clean = SkeletonData::from_spine_json(BASE, "spine/hero.json").unwrap();
    assert_eq!(clean.warnings(), [] as [String; 0], "a plain file warns about nothing");

    type Patch = fn(&mut serde_json::Value);
    let cases: &[(&str, Patch)] = &[
        // Whole-file features.
        ("global physics settings (physics constraint \"p\")", |v| {
            v["physics"] = json!([{ "name": "p", "bone": "arm", "windGlobal": true }])
        }),
        ("event values (event \"step\")", |v| {
            v["events"]["step"] = json!({ "int": 3 })
        }),
        ("field \"audio2\" (the file)", |v| v["audio2"] = json!({})),
        // Bones.
        ("inherit mode \"noScale\" (bone \"arm\")", |v| {
            v["bones"][1]["inherit"] = json!("noScale")
        }),
        ("inherit mode \"onlyTranslation\" (bone \"arm\")", |v| {
            v["bones"][1]["transform"] = json!("onlyTranslation")
        }),
        ("skin-required bones (bone \"arm\")", |v| {
            v["bones"][1]["skin"] = json!(true)
        }),
        ("field \"wobble\" (bone \"arm\")", |v| {
            v["bones"][1]["wobble"] = json!(1)
        }),
        // Slots.
        ("dark color (slot \"arm\")", |v| v["slots"][0]["dark"] = json!("000000")),
        ("blend mode \"additive\" (slot \"arm\")", |v| {
            v["slots"][0]["blend"] = json!("additive")
        }),
        ("hidden slots (slot \"arm\")", |v| {
            v["slots"][0]["visible"] = json!(false)
        }),
        // Skins and attachments.
        ("skin bones and constraints (skin \"default\")", |v| {
            v["skins"][0]["bones"] = json!(["arm"])
        }),
        ("sequence (attachment \"it\" in slot \"arm\")", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "width": 1, "height": 1, "sequence": { "count": 3 } })
        }),
        ("linked mesh (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "linkedmesh", "parent": "x" })
        }),
        ("bounding box (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "boundingbox", "vertexCount": 0 })
        }),
        ("point attachment (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "point" })
        }),
        ("attachment type \"hologram\" (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "hologram" })
        }),
        ("field \"glow\" (attachment \"it\" in slot \"arm\")", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "width": 1, "height": 1, "glow": 1 })
        }),
        // IK.
        ("IK softness (IK constraint \"reach\")", |v| {
            v["ik"][0]["softness"] = json!(5)
        }),
        ("IK compress (IK constraint \"reach\")", |v| {
            v["ik"][0]["compress"] = json!(true)
        }),
        ("IK stretch (IK constraint \"reach\")", |v| {
            v["ik"][0]["stretch"] = json!(true)
        }),
        ("IK uniform scale (IK constraint \"reach\")", |v| {
            v["ik"][0]["uniform"] = json!(true)
        }),
        ("skin-required constraints (IK constraint \"reach\")", |v| {
            v["ik"][0]["skin"] = json!(true)
        }),
        // Animations.
        ("IK softness keys (animation \"wave\", IK constraint \"reach\")", |v| {
            v["animations"]["wave"]["ik"] = json!({ "reach": [{ "softness": 4 }] })
        }),
        ("sequence timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["attachments"] = json!({ "default": { "arm": { "it": { "sequence": [{}] } } } })
        }),
        ("deform of an attachment that isn't a mesh (animation \"wave\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "width": 1, "height": 1 });
            v["animations"]["wave"]["attachments"] = json!({ "default": { "arm": { "it": { "deform": [{}] } } } })
        }),
        ("\"inherit\" timeline (animation \"wave\", bone \"arm\")", |v| {
            v["animations"]["wave"]["bones"]["arm"]["inherit"] = json!([{ "inherit": "noScale" }])
        }),
        ("\"rgba2\" timeline (animation \"wave\", slot \"arm\")", |v| {
            v["animations"]["wave"]["slots"] =
                json!({ "arm": { "rgba2": [{ "light": "ffffffff", "dark": "000000" }] } })
        }),
        ("event values (animation \"wave\")", |v| {
            v["animations"]["wave"]["events"] = json!([{ "time": 0.5, "name": "step", "float": 2.5 }])
        }),
        ("field \"sounds\" (animation \"wave\")", |v| {
            v["animations"]["wave"]["sounds"] = json!([])
        }),
    ];
    let base: serde_json::Value = serde_json::from_str(BASE).unwrap();
    for (feature, patch) in cases {
        let mut file = base.clone();
        patch(&mut file);
        let data = SkeletonData::from_spine_json(&file.to_string(), "spine/hero.json")
            .unwrap_or_else(|e| panic!("{feature}: {e}"));
        let warnings = data.warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.starts_with("spine/hero.json: Spine ") && w.contains(feature) && w.contains("isn't read")),
            "{feature}: {warnings:?}"
        );
    }
}

#[test]
fn spine_warnings_are_listed_once_each() {
    let text = BASE.replace(
        r#""animations": { "wave": {"#,
        r#""animations": { "wave": { "events": [{ "name": "step", "int": 1 }, { "time": 1, "name": "step", "int": 2 }],"#,
    );
    let data = SkeletonData::from_spine_json(&text, "a.json").unwrap();
    assert_eq!(
        data.warnings(),
        ["a.json: Spine event values (animation \"wave\") isn't read; the event fires by name only"]
    );
}

// ---- Shear, one-axis timelines, transform constraints --------------------
//
// Spine is y-up with counter-clockwise degrees; Ponko is y-down with
// clockwise radians, so Spine's (x, y) is Ponko's (x, -y) and its angle a
// is Ponko's -a. Every expected value below is worked out by hand in
// Spine's terms first, then mirrored.

fn load(text: &str) -> Skeleton {
    let data = SkeletonData::from_spine_json(text, "features.json").unwrap();
    assert_eq!(data.warnings(), [] as [String; 0]);
    let mut s = Skeleton::new(Arc::new(data));
    if !s.data().clips().is_empty() {
        s.play(&s.data().clips()[0].name.clone());
    }
    s.update(0.0);
    s
}

fn bone(s: &Skeleton, name: &str) -> usize {
    s.bone(name).unwrap_or_else(|| panic!("no bone {name}"))
}

#[test]
fn shear_turns_each_axis_of_a_bone_on_its_own() {
    // shearX 90: the x axis turns a quarter (Spine: from (1, 0) to (0, 1));
    // shearY 30: the y axis turns from 90 to 120 degrees, (-0.5, 0.866).
    let s = load(
        r#"{ "skeleton": { "spine": "4.2.0" },
             "bones": [{ "name": "root" }, { "name": "b", "parent": "root", "shearX": 90, "shearY": 30 }] }"#,
    );
    let b = bone(&s, "b");
    // Spine (10, 0) local lands at (0, 10); Ponko's (10, 0) at (0, -10).
    near2(s.bone_point(b, vec2(10.0, 0.0)), vec2(0.0, -10.0));
    // Spine (0, 10) local lands at 10 (-0.5, 0.866) = (-5, 8.66); Ponko's
    // (0, -10) at (-5, -8.66).
    near2(s.bone_point(b, vec2(0.0, -10.0)), vec2(-5.0, -8.660_254));
    near2(
        s.bone_pose(b).shear,
        vec2(-std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_6),
    );
}

#[test]
fn shear_timelines_key_both_axes_or_one() {
    // `shear` keys y from 0 to 60 over a second; `shearx` keys x from 0 to
    // -20 over two: at 0.5 s y is 30 and x is -5 degrees.
    let mut s = load(
        r#"{ "skeleton": { "spine": "4.2.0" },
             "bones": [{ "name": "root" }, { "name": "b", "parent": "root" }],
             "animations": { "a": { "bones": { "b": {
               "shear": [{ "time": 0, "x": 0, "y": 0 }, { "time": 1, "x": 0, "y": 60 }],
               "shearx": [{ "time": 0, "value": 0 }, { "time": 2, "value": -20 }] } } } } }"#,
    );
    let b = bone(&s, "b");
    at(&mut s, 0.5);
    near(s.bone_pose(b).shear.y, -30f32.to_radians());
    near(s.bone_pose(b).shear.x, 5f32.to_radians());
}

#[test]
fn one_axis_timelines_keep_their_own_key_times() {
    // translatex: 0 -> 10 over 1 s. translatey: 0 -> 20 -> 0, peaking at
    // 0.5 s. scalex: 1 -> 3 over 1 s; scaley stays at the rest scale 2.
    let mut s = load(
        r#"{ "skeleton": { "spine": "4.2.0" },
             "bones": [{ "name": "root" }, { "name": "b", "parent": "root", "x": 1, "scaleY": 2 }],
             "animations": { "a": { "bones": { "b": {
               "translatex": [{ "time": 0, "value": 0 }, { "time": 1, "value": 10 }],
               "translatey": [{ "time": 0, "value": 0 }, { "time": 0.5, "value": 20 }, { "time": 1, "value": 0 }],
               "scalex": [{ "time": 0, "value": 1 }, { "time": 1, "value": 3 }] } } } } }"#,
    );
    let b = bone(&s, "b");
    at(&mut s, 0.25);
    let p = s.bone_pose(b);
    // x: rest 1 plus 2.5; y: 10 up, so -10; scale x: rest 1 times 1.5.
    near2(p.translation, vec2(3.5, -10.0));
    near2(p.scale, vec2(1.5, 2.0));
    at(&mut s, 0.75);
    let p = s.bone_pose(b);
    near2(p.translation, vec2(8.5, -10.0));
    near(p.scale.x, 2.5);
}

/// A root, a `target` and a `follower` (both on the root) and transform
/// constraint `constraint` (a JSON object) moving the follower.
fn constrained(target: &str, follower: &str, constraint: &str) -> Skeleton {
    load(&format!(
        r#"{{ "skeleton": {{ "spine": "4.2.0" }},
             "bones": [{{ "name": "root" }}, {{ "name": "target", "parent": "root", {target} }},
                       {{ "name": "follower", "parent": "root", {follower} }}],
             "transform": [{{ "name": "t", "bones": ["follower"], "target": "target", {constraint} }}] }}"#
    ))
}

/// Where bone `name`'s +x axis points and how long it is.
fn x_axis(s: &Skeleton, name: &str) -> Vec2 {
    let b = bone(s, name);
    s.bone_point(b, vec2(1.0, 0.0)) - s.bone_position(b)
}

#[test]
fn transform_constraints_move_bones_onto_their_target_in_world_space() {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    // The target at (10, 0) points up (90 degrees). Fully: the follower
    // takes its place and its turn, (10, 0) and -pi/2 in Ponko.
    let full = r#""mixScaleX": 0, "mixShearY": 0"#;
    let s = constrained(r#""x": 10, "rotation": 90"#, r#""x": 0"#, full);
    near2(s.bone_position(bone(&s, "follower")), vec2(10.0, 0.0));
    near(s.bone_rotation(bone(&s, "follower")), -FRAC_PI_2);
    // Half way: (5, 0) and an eighth of a turn.
    let s = constrained(
        r#""x": 10, "rotation": 90"#,
        r#""x": 0"#,
        r#""mixRotate": 0.5, "mixX": 0.5, "mixScaleX": 0, "mixShearY": 0"#,
    );
    near2(s.bone_position(bone(&s, "follower")), vec2(5.0, 0.0));
    near(s.bone_rotation(bone(&s, "follower")), -FRAC_PI_4);
    // An offset of (5, 0) in the target's space is 5 up from it: Spine
    // (10, 5), Ponko (10, -5); an offset rotation of -90 cancels its turn.
    let s = constrained(
        r#""x": 10, "rotation": 90"#,
        r#""x": 0"#,
        r#""x": 5, "rotation": -90, "mixScaleX": 0, "mixShearY": 0"#,
    );
    near2(s.bone_position(bone(&s, "follower")), vec2(10.0, -5.0));
    near(s.bone_rotation(bone(&s, "follower")), 0.0);
    // Scale: target 2 wide, follower 1, half way: 1.5; an offset adds.
    let s = constrained(
        r#""scaleX": 2"#,
        r#""x": 0"#,
        r#""mixRotate": 0, "mixX": 0, "mixScaleX": 0.5, "mixScaleY": 0, "mixShearY": 0"#,
    );
    near(x_axis(&s, "follower").length(), 1.5);
    let s = constrained(
        r#""scaleX": 2"#,
        r#""x": 0"#,
        r#""mixRotate": 0, "mixX": 0, "scaleX": 1, "mixScaleY": 0, "mixShearY": 0"#,
    );
    near(x_axis(&s, "follower").length(), 3.0);
    // Shear: the target's y axis leans 30 degrees (Spine (-0.5, 0.866),
    // Ponko (0.5, 0.866) for its +y); the follower's leans the same.
    let s = constrained(
        r#""shearY": 30"#,
        r#""x": 0"#,
        r#""mixRotate": 0, "mixX": 0, "mixScaleX": 0"#,
    );
    let f = bone(&s, "follower");
    near2(
        s.bone_point(f, vec2(0.0, 1.0)) - s.bone_position(f),
        vec2(0.5, 0.866_025_4),
    );
}

#[test]
fn relative_world_transform_constraints_add_the_targets_transform() {
    // Follower turned 10 degrees, target 90: the follower turns another 90
    // (100 in all, -100 in Ponko) and moves by the target's position.
    let s = constrained(
        r#""x": 10, "y": 4, "rotation": 90"#,
        r#""x": 1, "rotation": 10"#,
        r#""relative": true, "mixScaleX": 0, "mixShearY": 0"#,
    );
    let f = bone(&s, "follower");
    near(s.bone_rotation(f), -100f32.to_radians());
    // (1, 0) plus the target's (10, 4): Spine (11, 4), Ponko (11, -4).
    near2(s.bone_position(f), vec2(11.0, -4.0));
    // Scale: the target's 2 adds (2 - 1) x mix 1: twice as wide.
    let s = constrained(
        r#""scaleX": 2"#,
        r#""x": 0"#,
        r#""relative": true, "mixRotate": 0, "mixX": 0, "mixScaleY": 0, "mixShearY": 0"#,
    );
    near(x_axis(&s, "follower").length(), 2.0);
}

#[test]
fn local_transform_constraints_mix_the_poses_relative_to_parents() {
    // Absolute: the follower's local pose moves toward the target's: at
    // mix 0.5, rotation 0 -> 30 is 15 and (0, 0) -> (10, 20) is (5, 10).
    let s = constrained(
        r#""x": 10, "y": 20, "rotation": 30, "scaleX": 3"#,
        r#""x": 0"#,
        r#""local": true, "mixRotate": 0.5, "mixX": 0.5, "mixScaleX": 0.5, "mixScaleY": 0, "mixShearY": 0"#,
    );
    let f = bone(&s, "follower");
    let p = s.bone_pose(f);
    near(p.rotation, -15f32.to_radians());
    near2(p.translation, vec2(5.0, -10.0));
    // Scale 1 -> 3 half way: 2.
    near(p.scale.x, 2.0);
    // Relative: the target's local pose is added: rotation 10 + 30, (1, 0)
    // + (10, 20), scale 1 x (3 - 1 + 1).
    let s = constrained(
        r#""x": 10, "y": 20, "rotation": 30, "scaleX": 3"#,
        r#""x": 1, "rotation": 10"#,
        r#""local": true, "relative": true, "mixScaleY": 0, "mixShearY": 0"#,
    );
    let p = s.bone_pose(bone(&s, "follower"));
    near(p.rotation, -40f32.to_radians());
    near2(p.translation, vec2(11.0, -20.0));
    near(p.scale.x, 3.0);
}

#[test]
fn ik_and_transform_constraints_apply_in_their_order() {
    // A transform constraint (order 0) moves the IK goal from (20, 0) to
    // the mover at (0, 20) up; the IK (order 1) then reaches the goal
    // there: the tip of `lower` at Ponko (0, -20). Had the IK gone first,
    // the tip would be at (20, 0).
    let text = r#"{ "skeleton": { "spine": "4.2.0" },
        "bones": [{ "name": "root" }, { "name": "upper", "parent": "root", "length": 10 },
                  { "name": "lower", "parent": "upper", "x": 10, "length": 10 },
                  { "name": "goal", "parent": "root", "x": 20 }, { "name": "mover", "parent": "root", "y": 20 }],
        "ik": [{ "name": "reach", "order": 1, "bones": ["upper", "lower"], "target": "goal" }],
        "transform": [{ "name": "move", "order": 0, "bones": ["goal"], "target": "mover",
                        "mixRotate": 0, "mixScaleX": 0, "mixShearY": 0 }] }"#;
    let s = load(text);
    let lower = bone(&s, "lower");
    near2(s.bone_point(lower, vec2(10.0, 0.0)), vec2(0.0, -20.0));
    // The other order: the IK reaches (20, 0) first.
    let s = load(
        &text
            .replace(r#""order": 1"#, r#""order": 2"#)
            .replace(r#""order": 0"#, r#""order": 3"#),
    );
    near2(s.bone_point(lower, vec2(10.0, 0.0)), vec2(20.0, 0.0));
    assert_eq!(s.data().transform_constraints()[0].name.as_ref(), "move");
}

#[test]
fn spine_3_transform_constraints_read_their_mix_names() {
    // 3.x: one translate mix for both axes, one scale mix.
    let s = load(
        r#"{ "skeleton": { "spine": "3.8.99" },
             "bones": [{ "name": "root" }, { "name": "target", "parent": "root", "x": 10, "y": 10, "scaleX": 3 },
                       { "name": "follower", "parent": "root" }],
             "transform": [{ "name": "t", "bones": ["follower"], "target": "target",
                             "rotateMix": 0, "translateMix": 0.5, "scaleMix": 0.5, "shearMix": 0 }] }"#,
    );
    let f = bone(&s, "follower");
    near2(s.bone_position(f), vec2(5.0, -5.0));
    near(x_axis(&s, "follower").length(), 2.0);
}

#[test]
fn transform_constraints_built_in_code_check_their_bones() {
    use ponko::skeleton2d::{BonePose, TransformConstraint};
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let leader = data.add_bone("leader", Some(root), BonePose::at(vec2(10.0, 0.0)), 0.0);
    let follower = data.add_bone("follower", Some(root), BonePose::IDENTITY, 0.0);
    let mut follow = TransformConstraint::new("follow", &[follower], leader);
    follow.mix_translate = vec2(0.5, 0.5);
    assert_eq!(data.add_transform_constraint(follow), 0);
    let mut s = Skeleton::new(Arc::new(data));
    s.update(0.0);
    near2(s.bone_position(follower), vec2(5.0, 0.0));
    let r = std::panic::catch_unwind(|| {
        let mut data = SkeletonData::new();
        let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
        data.add_transform_constraint(TransformConstraint::new("self", &[root], root));
    });
    assert!(r.is_err(), "a bone can't follow itself");
}
