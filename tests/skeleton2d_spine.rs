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
        ("transform constraints", |v| {
            v["transform"] = json!([{ "name": "t", "bones": ["arm"], "target": "root" }])
        }),
        ("path constraints", |v| {
            v["path"] = json!([{ "name": "p", "bones": ["arm"], "target": "arm" }])
        }),
        ("physics constraints", |v| {
            v["physics"] = json!([{ "name": "p", "bone": "arm" }])
        }),
        ("event values (event \"step\")", |v| {
            v["events"]["step"] = json!({ "int": 3 })
        }),
        ("field \"audio2\" (the file)", |v| v["audio2"] = json!({})),
        // Bones.
        ("shear (bone \"arm\")", |v| v["bones"][1]["shearX"] = json!(10)),
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
        ("path attachment (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "path" })
        }),
        ("point attachment (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "point" })
        }),
        ("clipping (attachment \"it\"", |v| {
            v["skins"][0]["attachments"]["arm"]["it"] = json!({ "type": "clipping", "end": "arm" })
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
        ("IK timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["ik"] = json!({ "reach": [{}] })
        }),
        ("transform constraint timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["transform"] = json!({ "t": [{}] })
        }),
        ("path constraint timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["path"] = json!({ "p": {} })
        }),
        ("physics constraint timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["physics"] = json!({ "p": {} })
        }),
        ("deform timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["deform"] = json!({ "default": {} })
        }),
        ("deform and sequence timelines (animation \"wave\")", |v| {
            v["animations"]["wave"]["attachments"] = json!({ "default": {} })
        }),
        ("\"shear\" timeline (animation \"wave\", bone \"arm\")", |v| {
            v["animations"]["wave"]["bones"]["arm"]["shear"] = json!([{ "x": 5 }])
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
        r#"{ "name": "arm", "parent": "root", "length": 10 }"#,
        r#"{ "name": "arm", "parent": "root", "length": 10, "shearX": 1, "shearY": 2 }"#,
    );
    let data = SkeletonData::from_spine_json(&text, "a.json").unwrap();
    assert_eq!(
        data.warnings(),
        ["a.json: Spine shear (bone \"arm\") isn't read; the bone is posed without it"]
    );
}
