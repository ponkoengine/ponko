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
        ("skins/rock a.png", "glTF model's image"),
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

/// The shared fixture `tests/fixtures/cook/` (at the repository root):
/// `source/` and what `ponko cook` makes of it in `cooked/`. The engine's
/// test `tests/cooked_assets.rs` loads both through the same calls and
/// compares them. `PONKO_UPDATE_COOK_FIXTURE=1` writes them again.
#[cfg(all(feature = "textures", feature = "meshes"))]
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
        ];
        let _ = std::fs::remove_dir_all(&root);
        for (name, bytes) in &source {
            std::fs::create_dir_all(root.join("source")).unwrap();
            std::fs::write(root.join("source").join(name), bytes).unwrap();
        }
        let cooked = cook_files(source, &Config::default(), &settings(None)).unwrap();
        for (name, bytes) in &cooked.files {
            std::fs::create_dir_all(root.join("cooked")).unwrap();
            std::fs::write(root.join("cooked").join(name), bytes).unwrap();
        }
    }
    let source = read_folder(&root.join("source"), None).unwrap();
    let committed = read_folder(&root.join("cooked"), None).unwrap();
    let cooked = cook_files(source, &Config::default(), &settings(None)).unwrap();
    assert_eq!(cooked.counts().0, 3, "{}", cooked.report());
    for ((name, now), (committed_name, then)) in cooked.files.iter().zip(&committed) {
        assert_eq!(name, committed_name);
        // FLAC is integer arithmetic: the same bytes everywhere. Texture
        // and mesh encoders use floats, so another CPU may round a last
        // bit differently: the same format and layout.
        match name.as_str() {
            "beep.wav" => assert_eq!(now, then, "{name}"),
            "tile.png" => assert_eq!(now[..48], then[..48], "{name}: the same KTX2 header"),
            _ => assert_eq!(
                gltf_json(now).unwrap()["accessors"],
                gltf_json(then).unwrap()["accessors"],
                "{name}"
            ),
        }
    }
}
