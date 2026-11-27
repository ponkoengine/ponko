//! Tests of `ponko cook`: settings, the cache, determinism, every kind.

use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-cook-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An RGBA PNG of `w` x `h` with a gradient.
#[cfg(feature = "textures")]
fn png(w: u32, h: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 90, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Without the image crate: a 1 x 1 PNG (images are kept without `textures`).
#[cfg(not(feature = "textures"))]
fn png(_: u32, _: u32) -> Vec<u8> {
    crate::pack::DOT_PNG.to_vec()
}

fn wav() -> Vec<u8> {
    let samples = flac::tests::tone(2_000, 1, 16);
    flac::tests::wav(22_050, 1, 16, &samples)
}

#[cfg(feature = "meshes")]
fn model() -> Vec<u8> {
    let (json, bin) = gltf::tests::grid(6);
    gltf::tests::glb(&json, &bin)
}

const SHADER: &[u8] = b"fn surface(s: Surface) -> Surface { var o = s; o.color = vec4<f32>(1.0); return o; }\n";

/// One file of every kind, and a settings file.
fn sample() -> Vec<(String, Vec<u8>)> {
    #[cfg_attr(not(feature = "meshes"), allow(unused_mut))]
    let mut files = vec![
        ("textures/bricks.png".to_string(), png(32, 16)),
        ("sounds/jump.wav".to_string(), wav()),
        ("shaders/glow.wgsl".to_string(), SHADER.to_vec()),
        ("rules.json".to_string(), br#"{"speed": 3}"#.to_vec()),
    ];
    #[cfg(feature = "meshes")]
    files.push(("models/ground.glb".to_string(), model()));
    files
}

fn settings(cache: Option<PathBuf>) -> Settings {
    Settings {
        platform: Platform::Desktop,
        reads: Reads::ALL,
        cache,
    }
}

fn file<'a>(cooked: &'a Cooked, name: &str) -> &'a [u8] {
    &cooked
        .files
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name}"))
        .1
}

fn outcome(cooked: &Cooked, name: &str) -> Outcome {
    cooked.entries.iter().find(|e| e.name == name).unwrap().outcome.clone()
}

/// The `vkFormat` of a KTX2 file (0 for Basis Universal UASTC).
#[cfg(feature = "textures")]
fn vk_format(ktx2: &[u8]) -> u32 {
    assert!(
        ktx2.starts_with(&[0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB]),
        "not KTX2"
    );
    u32::from_le_bytes(ktx2[12..16].try_into().unwrap())
}

#[test]
fn options_parse_the_folder_platform_and_cache() {
    let o = Options::parse(&args(&["art", "--out", "dist", "--platform", "mobile", "--cache", "c"])).unwrap();
    assert_eq!(o.folder, Some(PathBuf::from("art")));
    assert_eq!(o.out, Some(PathBuf::from("dist")));
    assert_eq!(o.platform, Platform::Mobile);
    assert_eq!(o.cache, Some(PathBuf::from("c")));
    assert_eq!(Options::parse(&[]).unwrap().platform, Platform::Desktop);
    for (bad, needle) in [
        (&["--platform", "console"][..], "unknown platform"),
        (&["--out", "a", "--pack", "b"], "not both"),
        (&["--cache", "a", "--no-cache"], "not both"),
        (&["--bogus"], "unknown option"),
        (&["a", "b"], "unexpected argument"),
    ] {
        let err = Options::parse(&args(bad)).unwrap_err();
        assert!(err.contains(needle), "{bad:?}: {err}");
    }
}

#[test]
fn rules_pick_an_exact_path_then_the_longest_pattern() {
    let config = Config::parse(
        r#"{"assets": {
            "ui/**": "keep",
            "ui/icons/*.png": { "texture": "mask", "mipmaps": false },
            "ui/icons/big.png": "skip",
            "music/*.wav": { "audio": "keep" },
            "*.glb": { "quantize": false },
            "fx.wgsl": { "shader": "effect" }
        }}"#,
    )
    .unwrap();
    assert!(config.rule_for("ui/panel.png").keep);
    let icon = config.rule_for("ui/icons/heart.png");
    assert_eq!(
        (icon.texture, icon.mipmaps, icon.keep),
        (Some(TextureKind::Mask), Some(false), false)
    );
    assert!(config.rule_for("ui/icons/big.png").skip);
    assert_eq!(config.rule_for("music/theme.wav").flac, Some(false));
    assert_eq!(config.rule_for("hero.glb").quantize, Some(false));
    assert_eq!(config.rule_for("fx.wgsl").shader, Some(ShaderKind::Effect));
    assert_eq!(config.rule_for("other.png"), Rule::default());
}

#[test]
fn bad_settings_name_the_asset_and_the_setting() {
    for (text, needle) in [
        (
            r#"{"assets": {"a.png": {"texture": "shiny"}}}"#,
            "\"a.png\": \"texture\" is color",
        ),
        (r#"{"assets": {"a.png": {"size": 2}}}"#, "unknown setting \"size\""),
        (r#"{"assets": {"a.png": "maybe"}}"#, "expected \"keep\", \"skip\""),
        (r#"{"textures": {}}"#, "unknown key \"textures\""),
        (r#"{"assets": {"a.wgsl": {"shader": "vertex"}}}"#, "unknown shader kind"),
        ("[1]", "expected an object"),
    ] {
        let err = Config::parse(text).unwrap_err();
        assert!(err.starts_with("cook.json: ") && err.contains(needle), "{text}: {err}");
    }
}

#[cfg(feature = "textures")]
#[test]
fn cooked_files_keep_their_names_and_load_by_their_first_bytes() {
    let mut files = sample();
    files.push(("cook.json".into(), br#"{"assets": {"rules.json": "keep"}}"#.to_vec()));
    let cooked = cook_files(files.clone(), &config_of(&files).unwrap(), &settings(None)).unwrap();
    let names: Vec<&str> = cooked.files.iter().map(|(n, _)| n.as_str()).collect();
    let mut expected: Vec<&str> = files
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| *n != "cook.json")
        .collect();
    expected.sort();
    assert_eq!(names, expected, "same names; the settings file stays behind");
    assert_eq!(vk_format(file(&cooked, "textures/bricks.png")), 146, "BC7 sRGB");
    assert!(file(&cooked, "sounds/jump.wav").starts_with(b"fLaC"));
    assert_eq!(file(&cooked, "shaders/glow.wgsl"), SHADER);
    assert_eq!(file(&cooked, "rules.json"), br#"{"speed": 3}"#);
    #[cfg(feature = "meshes")]
    assert!(file(&cooked, "models/ground.glb").starts_with(b"glTF"));
    let texture = cooked.entries.iter().find(|e| e.name == "textures/bricks.png").unwrap();
    let (before, after) = texture.gpu.unwrap();
    // BC7 is a quarter of RGBA8 (a little more: the smallest mips are whole 4 x 4 blocks).
    assert!(after * 4 >= before && after * 3 < before, "{before} -> {after}");
    assert!(cooked.report().contains("textures: "), "{}", cooked.report());
}

#[test]
fn the_same_files_cook_to_the_same_bytes() {
    let a = cook_files(sample(), &Config::default(), &settings(None)).unwrap();
    let b = cook_files(sample(), &Config::default(), &settings(None)).unwrap();
    assert_eq!(a.files, b.files);
}

#[test]
fn a_second_cook_reads_every_cooked_file_from_the_cache() {
    let cache = temp("cache");
    let first = cook_files(sample(), &Config::default(), &settings(Some(cache.clone()))).unwrap();
    let (cooked, cached) = first.counts();
    assert!(cooked >= 1 && cached == 0, "{}", first.report());
    let second = cook_files(sample(), &Config::default(), &settings(Some(cache.clone()))).unwrap();
    assert_eq!(second.counts(), (0, cooked), "{}", second.report());
    assert_eq!(first.files, second.files, "the cache gives the same bytes");
    for (a, b) in first.entries.iter().zip(&second.entries) {
        assert_eq!((a.gpu, &a.note), (b.gpu, &b.note), "{}", a.name);
    }
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn a_changed_file_or_setting_is_cooked_again() {
    let cache = temp("changes");
    cook_files(sample(), &Config::default(), &settings(Some(cache.clone()))).unwrap();
    let mut files = sample();
    files.iter_mut().find(|(n, _)| n == "shaders/glow.wgsl").unwrap().1 =
        b"fn sky(d: vec3<f32>) -> vec3<f32> { return d; }".to_vec();
    let config =
        Config::parse(r#"{"assets": {"sounds/*.wav": {"audio": "flac"}, "textures/*": {"mipmaps": false}}}"#).unwrap();
    let again = cook_files(files, &config, &settings(Some(cache.clone()))).unwrap();
    #[cfg(feature = "shaders")]
    assert_eq!(outcome(&again, "shaders/glow.wgsl"), Outcome::Cooked, "new bytes");
    #[cfg(feature = "textures")]
    assert_eq!(outcome(&again, "textures/bricks.png"), Outcome::Cooked, "new setting");
    // The same cooking as before, said out loud: still cached.
    assert_eq!(outcome(&again, "sounds/jump.wav"), Outcome::Cached);
    let _ = std::fs::remove_dir_all(cache);
}

#[test]
fn keep_and_skip_rules_copy_or_leave_out() {
    let config = Config::parse(r#"{"assets": {"sounds/**": "keep", "shaders/**": "skip"}}"#).unwrap();
    let cooked = cook_files(sample(), &config, &settings(None)).unwrap();
    assert_eq!(file(&cooked, "sounds/jump.wav"), wav().as_slice());
    assert!(cooked.files.iter().all(|(n, _)| n != "shaders/glow.wgsl"));
    assert_eq!(outcome(&cooked, "shaders/glow.wgsl"), Outcome::Skipped);
}

#[cfg(all(feature = "textures", feature = "meshes"))]
#[test]
fn images_the_engine_reads_as_rgba8_anyway_are_kept() {
    let mut odd = std::io::Cursor::new(Vec::new());
    image::ImageBuffer::<image::Rgba<u16>, _>::from_pixel(8, 8, image::Rgba([1000, 2000, 3000, 65535]))
        .write_to(&mut odd, image::ImageFormat::Png)
        .unwrap();
    let (mut json, bin) = gltf::tests::grid(2);
    json["images"] = serde_json::json!([{"uri": "../skins/rock%20a.png"}]);
    let files = vec![
        ("heights.png".to_string(), odd.into_inner()),
        ("odd.png".to_string(), png(10, 10)),
        ("skins/rock a.png".to_string(), png(8, 8)),
        ("models/rock.glb".to_string(), gltf::tests::glb(&json, &bin)),
    ];
    let cooked = cook_files(files.clone(), &Config::default(), &settings(None)).unwrap();
    for (name, why) in [
        ("heights.png", "16 bits"),
        ("odd.png", "not a multiple of 4"),
        ("skins/rock a.png", "no material uses"),
    ] {
        match outcome(&cooked, name) {
            Outcome::Kept(Some(reason)) => assert!(reason.contains(why), "{name}: {reason}"),
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(
            file(&cooked, name),
            files.iter().find(|(n, _)| n == name).unwrap().1.as_slice()
        );
    }
}

#[cfg(feature = "textures")]
#[test]
fn texture_formats_follow_the_platform_the_settings_and_the_game() {
    let files = vec![
        ("a.png".to_string(), png(8, 8)),
        ("n_normal.png".to_string(), png(8, 8)),
        (
            "textures.json".to_string(),
            br#"{"textures": {"*_normal.png": "normal"}}"#.to_vec(),
        ),
    ];
    let config = Config::parse(r#"{"assets": {"m.png": {"texture": "mask"}}}"#).unwrap();
    let mut with_mask = files.clone();
    with_mask.push(("m.png".to_string(), png(8, 8)));
    let desktop = cook_files(with_mask.clone(), &config, &settings(None)).unwrap();
    assert_eq!(vk_format(file(&desktop, "a.png")), 146, "BC7 sRGB");
    assert_eq!(vk_format(file(&desktop, "n_normal.png")), 141, "BC5 from textures.json");
    assert_eq!(vk_format(file(&desktop, "m.png")), 139, "BC4");
    let mobile = Settings {
        platform: Platform::Mobile,
        ..settings(None)
    };
    let cooked = cook_files(files.clone(), &Config::default(), &mobile).unwrap();
    assert_eq!(vk_format(file(&cooked, "a.png")), 0, "UASTC");
    let no_basis = Settings {
        reads: Reads {
            ktx2: true,
            uastc: false,
        },
        ..mobile
    };
    let cooked = cook_files(files.clone(), &Config::default(), &no_basis).unwrap();
    assert_eq!(
        vk_format(file(&cooked, "a.png")),
        146,
        "BC7 when the game can't transcode UASTC"
    );
    let no_ktx2 = Settings {
        reads: Reads::from_features(&["audio", "3d"]),
        ..settings(None)
    };
    let cooked = cook_files(files.clone(), &Config::default(), &no_ktx2).unwrap();
    assert_eq!(
        file(&cooked, "a.png"),
        png(8, 8).as_slice(),
        "a game without KTX2 gets its PNG"
    );
    assert!(matches!(outcome(&cooked, "a.png"), Outcome::Kept(Some(r)) if r.contains("compressed-textures")));
}

#[test]
fn engine_features_decide_what_the_game_reads() {
    assert_eq!(
        Reads::from_features(&["audio"]),
        Reads {
            ktx2: false,
            uastc: false
        }
    );
    assert_eq!(
        Reads::from_features(&["compressed-textures"]),
        Reads {
            ktx2: true,
            uastc: false
        }
    );
    assert_eq!(
        Reads::from_features(&["basis-universal", "compressed-textures"]),
        Reads::ALL
    );
}

#[cfg(feature = "shaders")]
#[test]
fn a_shader_that_doesnt_compile_fails_the_cook_naming_it() {
    let mut files = sample();
    files.push((
        "shaders/broken.wgsl".to_string(),
        b"fn surface(s: Surface) -> Surface {\n  return s.nope;\n}\n".to_vec(),
    ));
    files.push(("bad.wav".to_string(), b"RIFF\0\0\0\0WAVE".to_vec()));
    let err = cook_files(files, &Config::default(), &settings(None)).unwrap_err();
    assert!(err.starts_with("2 assets can't be cooked"), "{err}");
    assert!(err.contains("shaders/broken.wgsl:2:"), "{err}");
    assert!(err.contains("bad.wav: it has no data chunk"), "{err}");
}

#[test]
fn cook_command_writes_a_folder_it_owns_and_a_pack() {
    let dir = temp("command");
    let assets = dir.join("assets");
    for (name, bytes) in sample() {
        let path = assets.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let report = cook_command(&Options::default(), &dir).unwrap();
    assert!(report.contains("wrote "), "{report}");
    let out = dir.join("target/cooked/desktop");
    assert!(out.join("sounds/jump.wav").is_file() && out.join(MARKER).is_file());
    // A second run reads the cache and replaces the folder.
    std::fs::write(out.join("stale.txt"), "old").unwrap();
    let report = cook_command(&Options::default(), &dir).unwrap();
    assert!(report.contains("cooked 0 files"), "{report}");
    assert!(!out.join("stale.txt").exists());
    // A folder it didn't make is refused.
    let foreign = dir.join("mine");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("notes.txt"), "keep me").unwrap();
    let options = Options {
        out: Some("mine".into()),
        ..Options::default()
    };
    assert!(cook_command(&options, &dir).unwrap_err().contains("didn't write"));
    assert!(foreign.join("notes.txt").is_file());
    // A pack of the cooked files.
    let options = Options {
        pack: Some("target/game.pack".into()),
        ..Options::default()
    };
    cook_command(&options, &dir).unwrap();
    let pack = std::fs::read(dir.join("target/game.pack")).unwrap();
    let (entries, start) = crate::pack::read_index(&pack).unwrap();
    let jump = entries.iter().find(|e| e.name == "sounds/jump.wav").unwrap();
    assert!(crate::pack::unpack(&pack, start, jump).unwrap().starts_with(b"fLaC"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cook_pack_refuses_a_pack_the_engine_would_refuse_unless_told_the_caps() {
    let dir = temp("pack-caps");
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("a.txt"), vec![b'a'; 3000]).unwrap();
    std::fs::write(assets.join("b.txt"), vec![b'b'; 3000]).unwrap();
    let args = |extra: &[&str]| {
        let mut args: Vec<String> = ["--pack", "target/g.pack", "--max-pack-size", "4000"]
            .map(String::from)
            .to_vec();
        args.extend(extra.iter().map(|s| s.to_string()));
        Options::parse(&args).unwrap()
    };
    let err = cook_command(&args(&[]), &dir).unwrap_err();
    assert!(err.contains("cap of 4000"), "{err}");
    assert!(
        err.contains("--max-pack-size") && err.contains("Game::pack_limits"),
        "{err}"
    );
    assert!(!dir.join("target/g.pack").exists(), "nothing is written");
    // Told the caps, it packs.
    let mut options = args(&[]);
    options.limits.max_pack_size = 8000;
    cook_command(&options, &dir).unwrap();
    assert!(dir.join("target/g.pack").is_file());
    let _ = std::fs::remove_dir_all(dir);
}

/// A `.gltf` file reading `models/<bin>` as its buffer.
#[cfg(feature = "meshes")]
fn gltf_with_buffer(bin: &str) -> (Vec<u8>, Vec<u8>) {
    let (mut json, data) = gltf::tests::grid(3);
    json["buffers"][0]["uri"] = serde_json::json!(bin);
    (serde_json::to_vec(&json).unwrap(), data)
}

#[cfg(feature = "meshes")]
#[test]
fn a_cooked_gltf_leaves_out_the_buffers_its_glb_holds() {
    let (text, data) = gltf_with_buffer("grid.bin");
    let files = vec![
        ("models/grid.gltf".to_string(), text.clone()),
        ("models/grid.bin".to_string(), data.clone()),
    ];
    let cooked = cook_files(files, &Config::default(), &settings(None)).unwrap();
    assert!(file(&cooked, "models/grid.gltf").starts_with(b"glTF"));
    assert!(
        !cooked.files.iter().any(|(n, _)| n == "models/grid.bin"),
        "{}",
        cooked.report()
    );
    assert_eq!(
        outcome(&cooked, "models/grid.bin"),
        Outcome::Embedded("models/grid.gltf".into())
    );
    assert!(
        cooked
            .report()
            .contains("models/grid.bin: left out, inside models/grid.gltf")
    );

    // A model that stays as it is still reads it; so does one a rule keeps.
    let (mut kept, _) = gltf_with_buffer("grid.bin");
    let mut json: serde_json::Value = serde_json::from_slice(&kept).unwrap();
    json["extensionsUsed"] = serde_json::json!(["EXT_unknown"]);
    kept = serde_json::to_vec(&json).unwrap();
    let files = vec![
        ("models/grid.gltf".to_string(), text.clone()),
        ("models/other.gltf".to_string(), kept),
        ("models/grid.bin".to_string(), data.clone()),
    ];
    let cooked = cook_files(files, &Config::default(), &settings(None)).unwrap();
    assert_eq!(file(&cooked, "models/grid.bin"), data.as_slice());
    let files = vec![
        ("models/grid.gltf".to_string(), text),
        ("models/grid.bin".to_string(), data.clone()),
    ];
    let config = Config::parse(r#"{"assets": {"models/*.bin": "keep"}}"#).unwrap();
    let cooked = cook_files(files, &config, &settings(None)).unwrap();
    assert_eq!(file(&cooked, "models/grid.bin"), data.as_slice());
}

/// The shared fixture `tests/fixtures/cook/` (at the repository root):
/// `source/` and what `ponko cook` makes of it in `cooked/`. The engine's
/// test `tests/cooked_assets.rs` loads both through the same calls and
/// compares them. `PONKO_UPDATE_COOK_FIXTURE=1` writes them again.
#[cfg(all(feature = "textures", feature = "meshes", feature = "aseprite"))]
#[test]
fn the_shared_cook_fixture_is_what_cook_makes_of_its_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cook");
    if std::env::var_os("PONKO_UPDATE_COOK_FIXTURE").is_some() {
        let (json, bin) = gltf::tests::grid(4);
        let source = vec![
            ("tile.png".to_string(), png(16, 16)),
            (
                "beep.wav".to_string(),
                flac::tests::wav(22_050, 1, 16, &flac::tests::tone(2_205, 1, 16)),
            ),
            ("grid.glb".to_string(), gltf::tests::glb(&json, &bin)),
            ("panel.glb".to_string(), {
                let (json, bin) = gltf::tests::textured(2, 32);
                gltf::tests::glb(&json, &bin)
            }),
            ("level.glb".to_string(), {
                let (json, bin) = gltf::tests::level(4, 12.0, 4);
                gltf::tests::glb(&json, &bin)
            }),
            ("hero.aseprite".to_string(), super::aseprite::tests::hero()),
        ];
        let mut source = source;
        source.extend(model_with_image_files(32));
        let _ = std::fs::remove_dir_all(&root);
        for (name, bytes) in &source {
            let path = root.join("source").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let cooked = cook_files(source, &Config::default(), &settings(None)).unwrap();
        for (name, bytes) in &cooked.files {
            let path = root.join("cooked").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
    let source = read_folder(&root.join("source"), None).unwrap();
    let committed = read_folder(&root.join("cooked"), None).unwrap();
    let cooked = cook_files(source, &Config::default(), &settings(None)).unwrap();
    assert_eq!(cooked.counts().0, 10, "{}", cooked.report());
    assert_eq!(cooked.files.len(), committed.len(), "{}", cooked.report());
    for ((name, now), (committed_name, then)) in cooked.files.iter().zip(&committed) {
        assert_eq!(name, committed_name);
        // FLAC is integer arithmetic: the same bytes everywhere. Texture
        // and mesh encoders use floats, so another CPU may round a last
        // bit differently: the same format and layout.
        match name.as_str() {
            // So are the Aseprite file's sheet (kept as PNG: 14 x 10
            // isn't a multiple of 4) and its JSON.
            // The occlusion image merged into the crate's ORM is kept.
            "beep.wav" | "hero.png" | "maps/crate_ao.png" => assert_eq!(now, then, "{name}"),
            "hero.json" => {
                let (now, then) = (unstamped_sheet(now), unstamped_sheet(then));
                assert_eq!(now, then, "{name}, apart from the version that cooked it");
            }
            _ if name.ends_with(".png") => assert_eq!(now[..48], then[..48], "{name}: the same KTX2 header"),
            _ => assert_eq!(
                gltf_json(now).unwrap()["accessors"],
                gltf_json(then).unwrap()["accessors"],
                "{name}"
            ),
        }
    }
}

/// A cooked Aseprite sheet's JSON without `meta.version`, the version of
/// the cook that wrote it (checked here to be this one's): a committed
/// fixture keeps the version it was made with.
fn unstamped_sheet(bytes: &[u8]) -> serde_json::Value {
    let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let version = json["meta"]["version"].take();
    assert!(
        version.is_string(),
        "the sheet names the version that cooked it: {version}"
    );
    json
}

/// A glTF's buffer views, with only the `buffer` of those that hold images.
#[cfg(all(feature = "textures", feature = "meshes"))]
fn views_apart_from_images(json: &serde_json::Value) -> Vec<serde_json::Value> {
    let images: Vec<u64> = json["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|image| image["bufferView"].as_u64().unwrap())
        .collect();
    let views = json["bufferViews"].as_array().unwrap();
    (0..views.len() as u64)
        .zip(views)
        .map(|(i, view)| {
            if images.contains(&i) {
                view["buffer"].clone()
            } else {
                view.clone()
            }
        })
        .collect()
}

/// A rule as `cook.json` writes it (only what it sets).
fn rule_json(rule: &Rule) -> serde_json::Value {
    if rule.keep {
        return "keep".into();
    }
    if rule.skip {
        return "skip".into();
    }
    let mut m = serde_json::Map::new();
    if let Some(t) = rule.texture {
        let name = match t {
            TextureKind::Color => "color",
            TextureKind::Linear => "linear",
            TextureKind::Normal => "normal",
            TextureKind::Mask => "mask",
            TextureKind::Keep => "keep",
        };
        m.insert("texture".into(), name.into());
    }
    let flags = [
        ("basis", rule.basis),
        ("mipmaps", rule.mipmaps),
        ("optimize", rule.optimize),
        ("quantize", rule.quantize),
    ];
    for (key, v) in flags {
        if let Some(v) = v {
            m.insert(key.into(), v.into());
        }
    }
    if let Some(flac) = rule.flac {
        m.insert("audio".into(), if flac { "flac" } else { "keep" }.into());
    }
    if let Some(s) = rule.shader {
        m.insert("shader".into(), s.name().into());
    }
    serde_json::Value::Object(m)
}

/// The editor shows and writes these settings; its tests read the same
/// fixture (`tests/fixtures/cook-rules/` at the repository root).
#[test]
fn the_editor_fixture_gives_the_rules_the_editor_shows() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cook-rules");
    let config = Config::parse(&std::fs::read_to_string(root.join(CONFIG)).unwrap()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("expected.json")).unwrap()).unwrap();
    for (path, want) in expected.as_object().unwrap() {
        assert_eq!(&rule_json(&config.rule_for(path)), want, "{path}");
    }
}

/// A `.gltf` (the grid) whose material reads its images from files: a
/// color image, a normal map, and metallic-roughness and occlusion images
/// in a folder of their own, `size` x `size` each.
#[cfg(all(feature = "textures", feature = "meshes"))]
fn model_with_image_files(size: u32) -> Vec<(String, Vec<u8>)> {
    let (mut json, bin) = gltf::tests::grid(2);
    json["materials"] = serde_json::json!([{
        "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicRoughnessTexture": {"index": 2}},
        "normalTexture": {"index": 1},
        "occlusionTexture": {"index": 3}
    }]);
    json["meshes"][0]["primitives"][0]["material"] = serde_json::json!(0);
    json["textures"] = serde_json::json!([{"source": 0}, {"source": 1}, {"source": 2}, {"source": 3}]);
    json["images"] = serde_json::json!([
        {"uri": "crate.png"}, {"uri": "crate_n.png"}, {"uri": "../maps/crate_mr.png"}, {"uri": "../maps/crate_ao.png"}
    ]);
    let solid = |rgba: [u8; 4]| {
        let image = image::RgbaImage::from_pixel(size, size, image::Rgba(rgba));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    vec![
        ("models/crate.glb".to_string(), gltf::tests::glb(&json, &bin)),
        ("models/crate.png".to_string(), png(size, size)),
        ("models/crate_n.png".to_string(), solid([128, 128, 255, 255])),
        ("maps/crate_mr.png".to_string(), solid([0, 200, 30, 255])),
        ("maps/crate_ao.png".to_string(), solid([90, 90, 90, 255])),
    ]
}

/// The image files a model reads are compressed for their use, whether the
/// model itself is cooked or kept: color BC7 sRGB, the normal map BC5, and
/// the metallic-roughness image with the occlusion merged into its red as
/// the engine merges it (BC7, linear), carrying the `ponkoOrm` key that
/// names both files, so the engine keeps it compressed. The occlusion file
/// stays as it is.
#[cfg(all(feature = "textures", feature = "meshes"))]
#[test]
fn a_models_image_files_are_compressed_for_their_use_with_the_occlusion_merged() {
    use crate::textures::tests::{decode, read};
    for config in [
        Config::default(),
        Config::parse(r#"{"assets": {"models/crate.glb": "keep"}}"#).unwrap(),
    ] {
        let cooked = cook_files(model_with_image_files(8), &config, &settings(None)).unwrap();
        let format = |name: &str| read(file(&cooked, name)).0.format;
        assert_eq!(format("models/crate.png"), Some(ktx2::Format::BC7_SRGB_BLOCK));
        assert_eq!(format("models/crate_n.png"), Some(ktx2::Format::BC5_UNORM_BLOCK));
        let orm = file(&cooked, "maps/crate_mr.png");
        let (header, levels) = read(orm);
        assert_eq!(header.format, Some(ktx2::Format::BC7_UNORM_BLOCK));
        let note: Vec<(String, Vec<u8>)> = ktx2::Reader::new(orm)
            .unwrap()
            .key_value_data()
            .map(|(k, v)| (k.to_string(), v.to_vec()))
            .collect();
        assert!(
            note.iter()
                .any(|(k, v)| k == "ponkoOrm" && v == b"crate_mr.png|crate_ao.png\0"),
            "{note:?}"
        );
        let pixels = decode(ktx2::Format::BC7_UNORM_BLOCK, &levels[0], 8, 8);
        for p in pixels.chunks(4) {
            assert!((i32::from(p[0]) - 90).abs() <= 2, "occlusion in red: {p:?}");
            assert!((i32::from(p[1]) - 200).abs() <= 2, "roughness in green: {p:?}");
            assert!((i32::from(p[2]) - 30).abs() <= 2, "metalness in blue: {p:?}");
        }
        match outcome(&cooked, "maps/crate_ao.png") {
            Outcome::Kept(Some(why)) => assert!(why.contains("merged into maps/crate_mr.png"), "{why}"),
            other => panic!("{other:?}"),
        }
        let gpu: Vec<(u64, u64)> = cooked
            .entries
            .iter()
            .filter(|e| e.name.ends_with(".png"))
            .filter_map(|e| e.gpu)
            .collect();
        let (before, after) = gpu.iter().fold((0, 0), |(a, b), g| (a + g.0, b + g.1));
        eprintln!("the crate's image files: {before} bytes of video memory as RGBA8, {after} cooked");
        assert!(after * 3 < before, "{before} -> {after}");
    }
}

/// A model that reads the same metallic-roughness image with two occlusion
/// images can't have both merged into it: it stays as it is, and the
/// engine merges each pair.
#[cfg(all(feature = "textures", feature = "meshes"))]
#[test]
fn a_metallic_roughness_image_with_two_occlusions_is_kept() {
    let mut files = model_with_image_files(8);
    let other = {
        let (mut json, bin) = gltf::tests::grid(2);
        json["materials"] = serde_json::json!([{
            "pbrMetallicRoughness": {"metallicRoughnessTexture": {"index": 0}},
            "occlusionTexture": {"index": 1}
        }]);
        json["textures"] = serde_json::json!([{"source": 0}, {"source": 1}]);
        json["images"] = serde_json::json!([{"uri": "maps/crate_mr.png"}, {"uri": "maps/crate_ao2.png"}]);
        gltf::tests::glb(&json, &bin)
    };
    files.push(("barrel.glb".to_string(), other));
    files.push(("maps/crate_ao2.png".to_string(), png(8, 8)));
    let cooked = cook_files(files, &Config::default(), &settings(None)).unwrap();
    match outcome(&cooked, "maps/crate_mr.png") {
        Outcome::Kept(Some(why)) => assert!(why.contains("two uses"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// An Aseprite file is cooked into the export the engine reads, in its
/// place; an export already beside it (from Aseprite, for hot reload while
/// developing) is replaced by the one made from the file, and `"keep"`
/// keeps the Aseprite file and its export as they are.
#[cfg(all(feature = "aseprite", feature = "textures"))]
#[test]
fn an_aseprite_file_is_cooked_into_its_export_replacing_an_old_one() {
    let hero = super::aseprite::tests::hero();
    let fresh = cook_files(
        vec![("art/hero.aseprite".into(), hero.clone())],
        &Config::default(),
        &settings(None),
    )
    .unwrap();
    let names: Vec<&str> = fresh.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["art/hero.json", "art/hero.png"]);
    let old = vec![
        ("art/hero.aseprite".to_string(), hero),
        ("art/hero.json".to_string(), b"{}".to_vec()),
        ("art/hero.png".to_string(), png(4, 4)),
    ];
    let cooked = cook_files(old.clone(), &Config::default(), &settings(None)).unwrap();
    assert_eq!(cooked.files, fresh.files, "the old export is replaced");
    let entry = cooked.entries.iter().find(|e| e.name == "art/hero.aseprite").unwrap();
    assert!(
        entry
            .note
            .contains("replacing the art/hero.json and art/hero.png beside it"),
        "{}",
        entry.note
    );
    let kept = cook_files(
        old,
        &Config::parse(r#"{"assets": {"art/hero.aseprite": "keep"}}"#).unwrap(),
        &settings(None),
    )
    .unwrap();
    let names: Vec<&str> = kept.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["art/hero.aseprite", "art/hero.json", "art/hero.png"]);
    assert_eq!(file(&kept, "art/hero.json"), b"{}");
}

#[cfg(feature = "aseprite")]
#[test]
fn aseprite_conversion_is_cached_by_source_content_and_split_mode() {
    let cache = temp("aseprite-cache");
    let settings = settings(Some(cache.clone()));
    let mut hero = super::aseprite::tests::hero();
    let run = |bytes: &[u8], config: &Config| {
        cook_files(vec![("art/hero.aseprite".into(), bytes.to_vec())], config, &settings).unwrap()
    };
    let default = Config::default();
    let first = run(&hero, &default);
    let warm = run(&hero, &default);
    assert_eq!(warm.counts(), (0, 1), "{}", warm.report());
    assert_eq!(first.files, warm.files);
    let split = Config::parse(r#"{"assets":{"art/hero.aseprite":{"layers":true}}}"#).unwrap();
    let split_first = run(&hero, &split);
    assert_ne!(split_first.files, first.files);
    assert_eq!(run(&hero, &split).counts(), (0, 1));
    hero[136..138].copy_from_slice(&42u16.to_le_bytes());
    let edited = run(&hero, &default);
    assert_eq!(edited.counts(), (1, 0));
    assert_ne!(edited.files, first.files);
    std::fs::remove_dir_all(cache).unwrap();
}

#[cfg(feature = "aseprite")]
#[test]
fn a_linked_tileset_is_read_from_the_assets_and_its_edits_cook_the_map_again() {
    use super::aseprite::tests::{linked_map, tileset_file};
    let cache = temp("aseprite-linked");
    let settings = settings(Some(cache.clone()));
    // The tileset's own file is left out of the cooked files.
    let config = Config::parse(r#"{"assets":{"art/tiles/ground.aseprite":"skip"}}"#).unwrap();
    let run = |color: [u8; 4]| {
        let files = vec![
            ("art/map.aseprite".into(), linked_map("tiles/ground.aseprite", 3, 2)),
            ("art/tiles/ground.aseprite".into(), tileset_file(3, color)),
        ];
        cook_files(files, &config, &settings).unwrap()
    };
    let corner = |cooked: &Cooked| {
        let png = file(cooked, "art/map.png");
        image::load_from_memory(png).unwrap().into_rgba8().get_pixel(3, 1).0
    };
    let first = run([0, 255, 0, 255]);
    let names: Vec<&str> = first.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["art/map.json", "art/map.png"], "{}", first.report());
    assert_eq!(corner(&first), [0, 255, 0, 255]);
    assert_eq!(run([0, 255, 0, 255]).counts(), (0, 1), "nothing changed");
    let edited = run([0, 0, 255, 255]);
    assert_eq!(edited.counts(), (1, 0), "{}", edited.report());
    assert_eq!(corner(&edited), [0, 0, 255, 255]);
    std::fs::remove_dir_all(cache).unwrap();
}

#[cfg(feature = "aseprite")]
#[test]
fn a_tileset_file_others_link_to_gets_no_sheet_without_a_skip_rule() {
    use super::aseprite::tests::{linked_map, tileset_file};
    let cache = temp("aseprite-tileset-only");
    let settings = settings(Some(cache.clone()));
    let run = |color: [u8; 4], extra: Vec<(String, Vec<u8>)>| {
        let mut files = vec![
            (
                "art/map.aseprite".to_string(),
                linked_map("tiles/ground.aseprite", 3, 2),
            ),
            ("art/tiles/ground.aseprite".to_string(), tileset_file(3, color)),
        ];
        files.extend(extra);
        cook_files(files, &Config::default(), &settings).unwrap()
    };
    let names = |cooked: &Cooked| cooked.files.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    let corner = |cooked: &Cooked| {
        let png = file(cooked, "art/map.png");
        image::load_from_memory(png).unwrap().into_rgba8().get_pixel(3, 1).0
    };
    let first = run([0, 255, 0, 255], Vec::new());
    assert_eq!(names(&first), ["art/map.json", "art/map.png"], "{}", first.report());
    // The map still reads the tiles from the file that has no sheet.
    assert_eq!(corner(&first), [0, 255, 0, 255]);
    let entry = first
        .entries
        .iter()
        .find(|e| e.name == "art/tiles/ground.aseprite")
        .unwrap();
    assert_eq!(entry.outcome, Outcome::Skipped);
    assert!(entry.note.contains("linked from art/map.aseprite"), "{}", entry.note);
    assert!(
        first
            .report()
            .contains("art/tiles/ground.aseprite: left out, only tilesets")
    );
    // Nothing changed: read back; the tileset is still part of the map's key.
    assert_eq!(run([0, 255, 0, 255], Vec::new()).counts(), (0, 1));
    let edited = run([0, 0, 255, 255], Vec::new());
    assert_eq!(edited.counts(), (1, 0), "{}", edited.report());
    assert_eq!(corner(&edited), [0, 0, 255, 255]);
    // The control: a file of only tilesets that nothing links to has a sheet.
    let alone = run(
        [0, 255, 0, 255],
        vec![("art/tiles/unused.aseprite".into(), tileset_file(3, [9; 4]))],
    );
    assert!(
        names(&alone).contains(&"art/tiles/unused.png".to_string()),
        "{}",
        alone.report()
    );
    // And a `skip` rule on the tileset file works as before.
    let config = Config::parse(r#"{"assets":{"art/tiles/ground.aseprite":"skip"}}"#).unwrap();
    let skipped = cook_files(
        vec![
            ("art/map.aseprite".into(), linked_map("tiles/ground.aseprite", 3, 2)),
            ("art/tiles/ground.aseprite".into(), tileset_file(3, [0, 255, 0, 255])),
        ],
        &config,
        &settings,
    )
    .unwrap();
    assert_eq!(names(&skipped), ["art/map.json", "art/map.png"]);
    std::fs::remove_dir_all(cache).unwrap();
}

#[cfg(feature = "aseprite")]
#[test]
fn a_kept_map_that_links_to_a_tileset_file_counts_as_its_linker() {
    use super::aseprite::tests::{hero, linked_map, tileset_file};
    let config = Config::parse(r#"{"assets":{"art/map.aseprite":"keep"}}"#).unwrap();
    let tileset = || {
        (
            "art/tiles/ground.aseprite".to_string(),
            tileset_file(3, [0, 255, 0, 255]),
        )
    };
    let names = |cooked: &Cooked| cooked.files.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    let cook = |map: Vec<u8>| {
        cook_files(
            vec![("art/map.aseprite".to_string(), map), tileset()],
            &config,
            &settings(None),
        )
        .unwrap()
    };
    // The kept map is copied as it is, and the file it links to has no sheet.
    let map = linked_map("tiles/ground.aseprite", 3, 2);
    let linked = cook(map.clone());
    assert_eq!(names(&linked), ["art/map.aseprite"], "{}", linked.report());
    assert_eq!(file(&linked, "art/map.aseprite"), map.as_slice());
    let entry = linked
        .entries
        .iter()
        .find(|e| e.name == "art/tiles/ground.aseprite")
        .unwrap();
    assert_eq!(entry.outcome, Outcome::Skipped);
    assert!(entry.note.contains("linked from art/map.aseprite"), "{}", entry.note);
    // The control: a kept map that does not link to it changes nothing, so
    // the tileset file still gets its sheet.
    let unlinked = cook(hero());
    assert_eq!(
        names(&unlinked),
        ["art/map.aseprite", "art/tiles/ground.json", "art/tiles/ground.png"],
        "{}",
        unlinked.report()
    );
    // And a map a `skip` rule leaves out does not count as a linker either.
    let skip = Config::parse(r#"{"assets":{"art/map.aseprite":"skip"}}"#).unwrap();
    let skipped = cook_files(
        vec![("art/map.aseprite".to_string(), map), tileset()],
        &skip,
        &settings(None),
    )
    .unwrap();
    assert_eq!(
        names(&skipped),
        ["art/tiles/ground.json", "art/tiles/ground.png"],
        "{}",
        skipped.report()
    );
}

#[cfg(all(feature = "meshes", feature = "textures"))]
#[test]
fn image_only_cooking_keeps_external_geometry_buffers_through_cache_hits() {
    let cache = temp("kept-model");
    let settings = settings(Some(cache.clone()));
    let (mut json, original) = gltf::tests::textured(2, 16);
    let ext = "EXT_meshopt_compression";
    json["extensionsUsed"] = serde_json::json!([ext]);
    json["buffers"][0]["uri"] = serde_json::json!("geometry.bin");
    // Two models read the buffer, so neither can tell what in it is unused.
    let files = vec![
        ("models/model.gltf".into(), serde_json::to_vec(&json).unwrap()),
        ("models/geometry.bin".into(), original.clone()),
        ("models/second.gltf".into(), serde_json::to_vec(&json).unwrap()),
    ];
    for expected in [Outcome::Cooked, Outcome::Cached] {
        let cooked = cook_files(files.clone(), &Config::default(), &settings).unwrap();
        assert_eq!(outcome(&cooked, "models/model.gltf"), expected);
        assert_eq!(outcome(&cooked, "models/geometry.bin"), Outcome::Kept(None));
        let output = cooked
            .files
            .iter()
            .find(|(name, _)| name == "models/model.gltf")
            .unwrap();
        let output = gltf_json(&output.1).unwrap();
        assert_eq!(output["buffers"][0], json["buffers"][0]);
        assert_eq!(output["accessors"], json["accessors"]);
        assert!(output["buffers"][1]["uri"].as_str().unwrap().starts_with("data:"));
        assert_eq!(output["images"][0]["mimeType"], "image/ktx2");
        assert_eq!(cooked.files[0], ("models/geometry.bin".into(), original.clone()));
    }
    std::fs::remove_dir_all(cache).unwrap();
}

/// The files of a `.gltf` model with meshopt streams reading `geometry.bin`.
#[cfg(all(feature = "meshes", feature = "textures"))]
fn meshopt_in_a_bin() -> (serde_json::Value, Vec<u8>, Companions) {
    let (json, bin) = gltf::tests::external_meshopt(32);
    let files = vec![
        ("models/model.gltf".to_string(), serde_json::to_vec(&json).unwrap()),
        ("models/geometry.bin".to_string(), bin.clone()),
    ];
    (json, bin, files)
}

#[cfg(all(feature = "meshes", feature = "textures"))]
#[test]
fn a_buffer_file_only_one_model_reads_loses_the_images_that_model_replaced() {
    let cache = temp("sole-reader");
    let settings = settings(Some(cache.clone()));
    let (json, bin, files) = meshopt_in_a_bin();
    let mut sizes = Vec::new();
    for expected in [Outcome::Cooked, Outcome::Cached] {
        let cooked = cook_files(files.clone(), &Config::default(), &settings).unwrap();
        // The shortened file is the cook's own output, cached with the model.
        assert_eq!(outcome(&cooked, "models/model.gltf"), expected);
        assert_eq!(outcome(&cooked, "models/geometry.bin"), expected);
        let now = file(&cooked, "models/geometry.bin");
        let model = gltf_json(file(&cooked, "models/model.gltf")).unwrap();
        assert!(now.len() + 1000 < bin.len(), "{} -> {}", bin.len(), now.len());
        assert_eq!(model["buffers"][0]["byteLength"], now.len());
        assert_eq!(model["accessors"], json["accessors"]);
        gltf::tests::assert_streams_byte_for_byte((&json, &bin), (&model, now));
        let entry = cooked.entries.iter().find(|e| e.name == "models/geometry.bin").unwrap();
        assert_eq!(
            (entry.source_bytes, entry.cooked_bytes),
            (bin.len() as u64, now.len() as u64)
        );
        assert!(entry.note.contains("models/model.gltf"), "{}", entry.note);
        sizes.push(cooked.files.iter().map(|(_, b)| b.len()).sum::<usize>());
    }
    assert_eq!(sizes[0], sizes[1], "a cache hit writes the same files");
    std::fs::remove_dir_all(cache).unwrap();
}

#[cfg(all(feature = "meshes", feature = "textures"))]
#[test]
fn a_buffer_file_another_model_a_rule_or_a_skip_covers_stays_as_it_is() {
    let (json, bin, files) = meshopt_in_a_bin();
    let untouched = |files: Vec<(String, Vec<u8>)>, rules: &str| {
        let config = Config::parse(rules).unwrap();
        let cooked = cook_files(files, &config, &settings(None)).unwrap();
        assert_eq!(file(&cooked, "models/geometry.bin"), bin, "{rules}");
        assert_eq!(outcome(&cooked, "models/geometry.bin"), Outcome::Kept(None), "{rules}");
        let model = gltf_json(file(&cooked, "models/model.gltf")).unwrap();
        assert_eq!(model["buffers"][0], json["buffers"][0], "{rules}");
    };
    // The control: with nothing else, it is shortened.
    let cooked = cook_files(files.clone(), &Config::default(), &settings(None)).unwrap();
    assert!(file(&cooked, "models/geometry.bin").len() < bin.len());
    // A second model reads it, even one the cook leaves alone or leaves out.
    for rules in [
        "{}",
        r#"{"assets": {"models/second.gltf": "keep"}}"#,
        r#"{"assets": {"models/second.gltf": "skip"}}"#,
    ] {
        let mut shared = files.clone();
        shared.push(("models/second.gltf".to_string(), files[0].1.clone()));
        untouched(shared, rules);
    }
    // The buffer's own rule keeps it as it is.
    untouched(files, r#"{"assets": {"models/geometry.bin": "keep"}}"#);
}

#[cfg(all(feature = "meshes", feature = "textures"))]
#[test]
fn a_buffer_file_read_from_two_folders_by_one_name_is_one_reader() {
    // Two models in different folders that name one file: two readers.
    let (_, bin, mut files) = meshopt_in_a_bin();
    let mut other = gltf_json(&files[0].1).unwrap();
    other["buffers"][0]["uri"] = serde_json::json!("../models/geometry.bin");
    files.push(("other/model.gltf".to_string(), serde_json::to_vec(&other).unwrap()));
    let cooked = cook_files(files, &Config::default(), &settings(None)).unwrap();
    assert_eq!(file(&cooked, "models/geometry.bin"), bin);
}

/// The kept-geometry fixture: `tests/fixtures/cook-kept/source/packed.glb`,
/// a model with meshopt-compressed geometry and its own images, and what
/// `ponko cook` makes of it in `cooked/`. The engine's offscreen GPU test
/// (`src/render/model_textures_render_test.rs`) decompresses both and
/// checks the cooked images stay GPU blocks. `PONKO_UPDATE_COOK_FIXTURE=1`
/// writes them again.
#[cfg(all(feature = "textures", feature = "meshes"))]
#[test]
fn the_kept_geometry_fixture_is_what_cook_makes_of_its_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cook-kept");
    if std::env::var_os("PONKO_UPDATE_COOK_FIXTURE").is_some() {
        let (json, bin) = gltf::tests::meshopt_textured(32);
        let source = vec![("packed.glb".to_string(), gltf::tests::glb(&json, &bin))];
        let cooked = cook_files(source.clone(), &Config::default(), &settings(None)).unwrap();
        for (folder, files) in [("source", &source), ("cooked", &cooked.files)] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            for (name, bytes) in files {
                std::fs::write(root.join(folder).join(name), bytes).unwrap();
            }
        }
    }
    let source = read_folder(&root.join("source"), None).unwrap();
    let committed = read_folder(&root.join("cooked"), None).unwrap();
    let cooked = cook_files(source.clone(), &Config::default(), &settings(None)).unwrap();
    assert_eq!(outcome(&cooked, "packed.glb"), Outcome::Cooked, "{}", cooked.report());
    let (now, then) = (&cooked.files[0].1, &committed[0].1);
    let (now_json, then_json) = (gltf_json(now).unwrap(), gltf_json(then).unwrap());
    // Texture encoders use floats, so another CPU may round a last bit
    // differently: the same layout and geometry.
    // Each image's KTX2 file names the cook that wrote it (`KTXwriter`),
    // version included, so its length follows the release: the image views
    // are compared by buffer only, every other view whole.
    assert_eq!(views_apart_from_images(&now_json), views_apart_from_images(&then_json));
    assert_eq!(now_json["images"], then_json["images"]);
    let stamp = concat!("ponko textures ", env!("CARGO_PKG_VERSION"));
    let stamps = now.windows(stamp.len()).filter(|w| *w == stamp.as_bytes()).count();
    assert_eq!(
        stamps,
        now_json["images"].as_array().unwrap().len(),
        "each image says {stamp:?}"
    );
    assert_eq!(now_json["accessors"], gltf_json(&source[0].1).unwrap()["accessors"]);
    // The replaced PNGs are dropped, not only zeroed: the cooked file is
    // smaller on disk than its source (6,128 bytes against 7,116).
    assert!(
        now.len() < source[0].1.len(),
        "{} -> {} bytes",
        source[0].1.len(),
        now.len()
    );
}
