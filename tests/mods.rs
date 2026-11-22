//! Data-only mods (`ponko::mods`): load order, overrides by path, and the
//! checks that refuse a bad mod whole, naming the mod, the file and the
//! reason, before anything of it is used. Includes hostile packs: damaged,
//! path traversal, oversized entries, deeply nested JSON; and asset
//! references checked at mount. No window.

use ponko::mods::{ModError, ModLimits, ModPack, ModProblem, ModSet, Mods};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Debug, PartialEq)]
struct Slime {
    speed: f32,
    hp: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Rules {
    waves: u32,
}

fn manifest(id: &str, requires: &[&str], after: &[&str]) -> String {
    let list = |ids: &[&str]| ids.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(",");
    format!(
        r#"{{"format":"ponko-mod","version":1,"id":"{id}","mod_version":"1.0","requires":[{}],"after":[{}]}}"#,
        list(requires),
        list(after)
    )
}

fn simple(id: &str, files: &[(&str, &str)]) -> ModPack {
    let mut all: Vec<(String, Vec<u8>)> = vec![("mod.json".into(), manifest(id, &[], &[]).into_bytes())];
    all.extend(files.iter().map(|(n, b)| (n.to_string(), b.as_bytes().to_vec())));
    ModPack::from_files(id, all).expect("a valid manifest")
}

fn needs(id: &str, requires: &[&str], after: &[&str]) -> ModPack {
    ModPack::from_files(id, [("mod.json", manifest(id, requires, after))]).expect("a valid manifest")
}

fn prefab(components: &str) -> String {
    format!(r#"{{"format":"ponko-prefab","version":1,"components":{components}}}"#)
}

/// One pack entry: (name, bytes, codec, the unpacked size the index claims).
type Entry<'a> = (&'a str, &'a [u8], u8, u64);

/// A pack as `ponko pack` writes it (version 1, entries stored as they are),
/// with what the index says left to the test.
fn pack_raw(entries: &[Entry]) -> Vec<u8> {
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let (mut index, mut data) = (Vec::new(), Vec::new());
    for (name, bytes, codec, size) in &entries {
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        index.extend_from_slice(&(name.len() as u16).to_le_bytes());
        index.extend_from_slice(name.as_bytes());
        index.push(*codec);
        index.extend_from_slice(&(data.len() as u64).to_le_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&size.to_le_bytes());
        index.extend_from_slice(&crc.sum().to_le_bytes());
        data.extend_from_slice(bytes);
    }
    let mut out = b"PONKOPAK".to_vec();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    out
}

fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
    let entries: Vec<Entry> = files.iter().map(|(n, b)| (*n, *b, 0, b.len() as u64)).collect();
    pack_raw(&entries)
}

/// Runs `body` in a headless game that reflects `Slime` and `Rules`.
fn game(body: impl FnOnce(&mut Ctx)) {
    Game::new("mods", 1, 1).run_headless(0, |ctx| {
        ctx.world.reflect::<Slime>();
        ctx.world.reflect_resource::<Rules>();
        body(ctx);
    });
}

fn mount_err(set: ModSet) -> ModError {
    let mut err = None;
    game(|ctx| {
        err = set.mount(ctx).err();
        // Refused whole: nothing mounted.
        assert!(!ctx.assets.has_source(ponko::mods::SOURCE_ID));
        assert!(ctx.world.get_resource::<Mods>().is_none());
    });
    err.expect("the set is refused")
}

fn read(ctx: &mut Ctx, name: &str) -> String {
    let bytes = ctx.assets.load_with(name, |b| Ok(b.to_vec())).expect("readable");
    String::from_utf8(bytes).unwrap()
}

// ---- load order and overrides ---------------------------------------------------

#[test]
fn a_later_mod_overrides_an_earlier_one_and_the_games_own_files_by_path() {
    game(|ctx| {
        let original = read(ctx, "audio_mixer.json");
        ModSet::new()
            .with(simple(
                "one",
                &[("rules.json", r#"{"waves":1}"#), ("only-one.txt", "1")],
            ))
            .with(simple(
                "two",
                &[("rules.json", r#"{"waves":2}"#), ("audio_mixer.json", "{}")],
            ))
            .mount(ctx)
            .expect("both mods are fine");
        assert_eq!(read(ctx, "rules.json"), r#"{"waves":2}"#, "the later mod wins");
        assert_eq!(read(ctx, "only-one.txt"), "1", "files only the earlier mod has stay");
        assert_ne!(original, "{}");
        assert_eq!(read(ctx, "audio_mixer.json"), "{}", "a mod replaces the game's file");
        let mods = ctx.world.resource::<Mods>();
        assert_eq!(mods.provider("rules.json"), Some("two"));
        assert_eq!(mods.provider("only-one.txt"), Some("one"));
        assert_eq!(mods.provider("audio_mixer.json"), Some("two"));
        assert_eq!(mods.provider("nowhere.json"), None);
        let ids: Vec<&str> = mods.list().iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["one", "two"]);
    });
}

#[test]
fn a_mod_pack_file_mounts_like_files_in_memory() {
    let dir = std::env::temp_dir().join(format!("ponko-mods-pack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bigger-slimes.pack");
    let manifest = manifest("bigger-slimes", &[], &[]);
    let slime = prefab(r#"{"Slime":{"speed":9.0,"hp":30}}"#);
    std::fs::write(
        &path,
        pack(&[
            ("mod.json", manifest.as_bytes()),
            ("prefabs/slime.json", slime.as_bytes()),
        ]),
    )
    .unwrap();
    let pack = ModPack::open(&path).expect("a mod pack");
    assert_eq!(pack.id(), "bigger-slimes");
    game(|ctx| {
        ModSet::new().with(pack).mount(ctx).expect("fine");
        let e = ctx.spawn_prefab("prefabs/slime.json", vec2(0.0, 0.0)).unwrap();
        assert_eq!(ctx.world.get::<Slime>(e), Some(&Slime { speed: 9.0, hp: 30 }));
        assert_eq!(ctx.world.resource::<Mods>().list()[0].origin, "bigger-slimes.pack");
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn load_order_keeps_the_added_order_and_moves_mods_after_what_they_need() {
    let set = ModSet::new()
        .with(needs("frost", &["caves"], &[]))
        .with(needs("hats", &[], &["frost", "absent-is-fine"]))
        .with(needs("caves", &[], &[]))
        .with(needs("music", &[], &[]));
    assert_eq!(set.order().unwrap(), ["caves", "frost", "hats", "music"]);
    let set = ModSet::new().with(needs("a", &[], &[])).with(needs("b", &[], &[]));
    assert_eq!(set.order().unwrap(), ["a", "b"], "no constraints: as added");
}

#[test]
fn files_under_lists_what_the_mods_added_sorted_and_once() {
    game(|ctx| {
        ModSet::new()
            .with(simple(
                "a",
                &[("enemies/bat.json", "{}"), ("enemies/rat.json", "{}"), ("enemiesx", "")],
            ))
            .with(simple("b", &[("enemies/bat.json", "{}"), ("enemies/ant.json", "{}")]))
            .mount(ctx)
            .unwrap();
        let mods = ctx.world.resource::<Mods>();
        let under: Vec<&str> = mods.files_under("enemies/").collect();
        assert_eq!(under, ["enemies/ant.json", "enemies/bat.json", "enemies/rat.json"]);
        assert_eq!(mods.files_under("zzz/").count(), 0);
    });
}

#[test]
fn the_fingerprint_is_the_same_for_the_same_mods_and_changes_with_order_or_data() {
    let print = |set: ModSet| {
        let mut out = 0;
        game(|ctx| {
            set.mount(ctx).unwrap();
            out = ctx.world.resource::<Mods>().fingerprint();
        });
        out
    };
    let a = || simple("a", &[("rules.json", r#"{"waves":1}"#)]);
    let b = || simple("b", &[("x.txt", "x")]);
    let first = print(ModSet::new().with(a()).with(b()));
    assert_eq!(first, print(ModSet::new().with(a()).with(b())));
    assert_ne!(first, print(ModSet::new().with(b()).with(a())), "order");
    let changed = simple("a", &[("rules.json", r#"{"waves":2}"#)]);
    assert_ne!(first, print(ModSet::new().with(changed).with(b())), "data");
}

#[test]
fn mods_mount_once() {
    game(|ctx| {
        ModSet::new().with(simple("a", &[])).mount(ctx).unwrap();
        let err = ModSet::new().with(simple("b", &[])).mount(ctx).unwrap_err();
        assert_eq!(err.problem(), &ModProblem::AlreadyMounted);
        assert_eq!(ctx.world.resource::<Mods>().list().len(), 1);
    });
}

// ---- refused mods: the mod, the file, the reason --------------------------------

#[test]
fn an_unknown_component_is_refused_naming_the_mod_the_file_and_the_component() {
    let bad = simple(
        "frost",
        &[("prefabs/frost.json", &prefab(r#"{"Frosty":{"cold":true}}"#))],
    );
    let err = mount_err(ModSet::new().with(bad));
    assert_eq!(err.mod_id(), "frost");
    assert_eq!(err.file(), Some("prefabs/frost.json"));
    assert_eq!(err.problem(), &ModProblem::UnknownComponent("Frosty".into()));
    assert!(
        err.to_string().contains("component \"Frosty\" isn't one this game has"),
        "{err}"
    );
}

#[test]
fn a_field_of_the_wrong_type_is_refused_naming_the_component_and_the_field() {
    let bad = simple(
        "frost",
        &[("prefabs/frost.json", &prefab(r#"{"Slime":{"speed":"fast","hp":3}}"#))],
    );
    let err = mount_err(ModSet::new().with(bad));
    assert_eq!(err.file(), Some("prefabs/frost.json"));
    let ModProblem::BadValue { component, message } = err.problem() else {
        panic!("{err}")
    };
    assert_eq!(component, "Slime");
    assert!(
        message.contains("invalid type: string \"fast\", expected f32"),
        "{message}"
    );
}

#[test]
fn a_prefab_tree_that_changes_an_included_prefab_is_checked_by_component_name() {
    // Changes to an included prefab are partial: the names are checked.
    let tree = r#"{"format":"ponko-prefab","version":2,"entities":[
        {"entity":1,"prefab":"prefabs/slime.json","components":{"Slime":{"speed":2.0}},
         "parts":[{"part":0,"components":{"Ghost":{}}}]}]}"#;
    let err = mount_err(ModSet::new().with(simple("tree", &[("prefabs/king.json", tree)])));
    assert_eq!(err.problem(), &ModProblem::UnknownComponent("Ghost".into()));
    let fine = tree.replace("Ghost", "Slime");
    game(|ctx| {
        ModSet::new()
            .with(simple(
                "tree",
                &[
                    ("prefabs/king.json", &fine),
                    ("prefabs/slime.json", &prefab(r#"{"Slime":{"speed":1.0,"hp":3}}"#)),
                ],
            ))
            .mount(ctx)
            .expect("a partial Slime in an included prefab's change is fine");
    });
}

#[test]
fn a_scene_in_a_mod_is_checked_like_a_load_resources_too() {
    let scene = r#"{"format":"ponko-world","version":1,"entities":[
        {"entity":1,"components":{"Slime":{"speed":1.0,"hp":1}}}],
        "resources":{"Weather":{"rain":true}}}"#;
    let err = mount_err(ModSet::new().with(simple("rainy", &[("scenes/level.json", scene)])));
    assert_eq!(err.file(), Some("scenes/level.json"));
    assert_eq!(err.problem(), &ModProblem::UnknownResource("Weather".into()));
    let fine = scene.replace(r#""Weather":{"rain":true}"#, r#""Rules":{"waves":3}"#);
    game(|ctx| {
        ModSet::new()
            .with(simple("rainy", &[("scenes/level.json", &fine)]))
            .mount(ctx)
            .unwrap();
        ctx.load_scene("scenes/level.json").unwrap();
        assert_eq!(ctx.world.resource::<Rules>().waves, 3);
    });
}

#[test]
fn a_json_file_that_does_not_parse_is_refused() {
    let err = mount_err(ModSet::new().with(simple("typo", &[("rules.json", "{\"waves\": 3,}")])));
    assert_eq!((err.mod_id(), err.file()), ("typo", Some("rules.json")));
    assert!(matches!(err.problem(), ModProblem::BadJson(_)), "{err}");
}

#[test]
fn a_missing_dependency_is_refused_naming_the_mod_and_what_it_needs() {
    let err = mount_err(ModSet::new().with(needs("frost", &["deep-caves"], &[])));
    assert_eq!((err.mod_id(), err.file()), ("frost", Some("mod.json")));
    assert_eq!(err.problem(), &ModProblem::MissingDependency("deep-caves".into()));
    assert_eq!(
        err.to_string(),
        "mod \"frost\", file \"mod.json\": requires mod \"deep-caves\", which isn't mounted"
    );
}

#[test]
fn a_cycle_in_the_load_order_is_refused_naming_the_chain() {
    let set = ModSet::new()
        .with(needs("hats", &[], &[]))
        .with(needs("a", &["c"], &[]))
        .with(needs("b", &[], &["a"]))
        .with(needs("c", &["b"], &[]));
    let err = set.order().unwrap_err();
    let ModProblem::Cycle(chain) = err.problem() else {
        panic!("{err}")
    };
    assert_eq!(chain.first(), chain.last(), "{chain:?}");
    assert_eq!(chain.len(), 4, "{chain:?}");
    for id in ["a", "b", "c"] {
        assert!(chain.iter().any(|c| c == id), "{chain:?}");
    }
    assert!(err.to_string().contains("in a cycle"), "{err}");
    assert!(matches!(mount_err(set).problem(), ModProblem::Cycle(_)));
}

#[test]
fn two_mods_with_one_id_are_refused() {
    let err = mount_err(ModSet::new().with(simple("a", &[])).with(simple("a", &[])));
    assert_eq!(err.problem(), &ModProblem::DuplicateId("a".into()));
}

#[test]
fn a_bad_mod_mounts_nothing_not_even_the_good_mods_with_it() {
    let good = simple("good", &[("rules.json", r#"{"waves":9}"#)]);
    let bad = simple("bad", &[("prefabs/x.json", &prefab(r#"{"Nope":{}}"#))]);
    game(|ctx| {
        let err = ModSet::new().with(good).with(bad).mount(ctx).unwrap_err();
        assert_eq!(err.mod_id(), "bad");
        assert!(
            ctx.assets.load_with("rules.json", |b| Ok(b.to_vec())).is_err(),
            "the good mod isn't mounted"
        );
        assert!(ctx.world.get_resource::<Mods>().is_none());
        // The game still mounts a good set afterwards.
        ModSet::new()
            .with(simple("good", &[("rules.json", r#"{"waves":9}"#)]))
            .mount(ctx)
            .unwrap();
        assert_eq!(read(ctx, "rules.json"), r#"{"waves":9}"#);
    });
}

#[test]
fn problems_lists_one_problem_per_bad_mod_for_a_mods_menu() {
    let set = ModSet::new()
        .with(simple("fine", &[]))
        .with(simple("typo", &[("a.json", "{")]))
        .with(needs("lonely", &["nobody"], &[]))
        .with(simple("ghost", &[("p.json", &prefab(r#"{"Ghost":{}}"#))]));
    let mut ids = Vec::new();
    game(|ctx| {
        ids = set
            .problems(&ctx.world)
            .iter()
            .map(|e| e.mod_id().to_string())
            .collect()
    });
    assert_eq!(ids, ["lonely", "typo", "ghost"]);
}

#[test]
fn a_mod_error_becomes_an_engine_error_with_the_file_as_its_path() {
    let err = mount_err(ModSet::new().with(simple("frost", &[("p.json", &prefab(r#"{"Ghost":{}}"#))])));
    let engine: Error = err.into();
    assert_eq!(engine.kind(), ErrorKind::NotFound);
    assert_eq!(engine.path(), Some("p.json"));
    assert!(
        engine.message().starts_with("mod \"frost\", file \"p.json\""),
        "{}",
        engine.message()
    );
}

// ---- the manifest ---------------------------------------------------------------

#[test]
fn a_mod_without_a_good_manifest_is_refused_naming_the_pack() {
    let cases: [(&str, &str); 4] = [
        ("{", "bad manifest"),
        (r#"{"format":"ponko-mod","version":2,"id":"a"}"#, "\"version\": 1"),
        (r#"{"format":"ponko-mod","version":1,"id":"Bad Id"}"#, "is not a mod id"),
        (
            r#"{"format":"ponko-mod","version":1,"id":"a","extra":1}"#,
            "unknown field",
        ),
    ];
    for (manifest, needle) in cases {
        let err = ModPack::from_files("thing.pack", [("mod.json", manifest)]).unwrap_err();
        assert_eq!((err.mod_id(), err.file()), ("thing.pack", Some("mod.json")));
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
    let err = ModPack::from_files("thing.pack", [("readme.txt", "hi")]).unwrap_err();
    assert!(err.to_string().contains("needs a mod.json"), "{err}");
}

// ---- hostile packs --------------------------------------------------------------

#[test]
fn a_malformed_pack_is_refused_not_a_crash() {
    let manifest = manifest("m", &[], &[]);
    let good = pack(&[("mod.json", manifest.as_bytes()), ("rules.json", b"{\"waves\":1}")]);
    let garbage = b"<!doctype html> not a pack at all".to_vec();
    let cut = good[..good.len() - 3].to_vec();
    let mut huge_count = good.clone();
    huge_count[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    for (bytes, needle) in [
        (garbage, "not an asset pack"),
        (cut, "cut short"),
        (huge_count, "max_entries"), // past the pack cap on entries, before the index is read
    ] {
        let err = ModPack::from_pack_bytes("evil.pack", bytes).unwrap_err();
        assert_eq!(err.mod_id(), "evil.pack");
        assert!(matches!(err.problem(), ModProblem::Unreadable(_)), "{err}");
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
    // A damaged file is found when the mod is checked, not when it is used.
    let mut damaged = good.clone();
    let last = damaged.len() - 2;
    damaged[last] ^= 0x10; // inside rules.json, the last file in the data
    let pack = ModPack::from_pack_bytes("evil.pack", damaged).expect("the index is fine");
    let err = mount_err(ModSet::new().with(pack));
    assert_eq!((err.mod_id(), err.file()), ("m", Some("rules.json")));
    assert!(err.to_string().contains("damaged"), "{err}");
}

#[test]
fn names_that_could_leave_the_games_assets_are_refused() {
    for name in [
        "../save.json",
        "a/../../b.png",
        "/etc/passwd",
        "C:/Windows/x.dll",
        "a\\..\\b",
        "a//b",
        "./a",
        "a/",
        "bell\u{7}.txt",
    ] {
        let manifest = manifest("sneaky", &[], &[]);
        let bytes = pack(&[("mod.json", manifest.as_bytes()), (name, b"x")]);
        let pack = ModPack::from_pack_bytes("sneaky.pack", bytes).expect("the pack format allows any name");
        let err = mount_err(ModSet::new().with(pack));
        assert_eq!(err.mod_id(), "sneaky");
        assert_eq!(err.file(), Some(name));
        assert!(matches!(err.problem(), ModProblem::BadName(_)), "{name}: {err}");
    }
}

#[test]
fn oversized_entries_are_refused_before_they_are_read() {
    let big = manifest("big", &[], &[]);
    // A deflated entry whose index claims 8 GiB: refused on its size alone
    // (a terabyte would already be refused when the pack opens, by the 16 GiB pack cap).
    let bomb = pack_raw(&[
        ("mod.json", big.as_bytes(), 0, big.len() as u64),
        ("bomb.json", &[0x03, 0x00], 1, 8 << 30),
    ]);
    let pack = ModPack::from_pack_bytes("big.pack", bomb).expect("the index is fine");
    let err = mount_err(ModSet::new().with(pack));
    assert_eq!(err.file(), Some("bomb.json"));
    assert_eq!(
        err.problem(),
        &ModProblem::TooLarge {
            size: 8 << 30,
            limit: 64 << 20
        }
    );
    // The limits are the game's to set: files, one file, the whole mod.
    let three = || simple("three", &[("a.txt", "12345"), ("b.txt", "12345")]);
    let limits = ModLimits::default();
    let few = ModLimits { max_files: 2, ..limits };
    let err = mount_err(ModSet::new().with(three()).limits(few));
    assert_eq!(err.problem(), &ModProblem::TooManyFiles { count: 3, limit: 2 });
    let small = ModLimits {
        max_file_bytes: 4,
        ..limits
    };
    assert_eq!(
        mount_err(ModSet::new().with(three()).limits(small)).file(),
        Some("a.txt")
    );
    let total = ModLimits {
        max_total_bytes: 9 + manifest("three", &[], &[]).len() as u64,
        ..limits
    };
    let err = mount_err(ModSet::new().with(three()).limits(total));
    assert!(matches!(err.problem(), ModProblem::TooLarge { .. }), "{err}");
}

#[test]
fn deeply_nested_json_is_refused_not_a_stack_overflow() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let err = mount_err(ModSet::new().with(simple("deep", &[("deep.json", &deep)])));
    assert_eq!(err.file(), Some("deep.json"));
    assert!(err.to_string().contains("recursion limit"), "{err}");
}

// ---- asset references -------------------------------------------------------------

#[test]
fn mounting_lists_every_missing_asset_reference_in_one_error_naming_file_and_name() {
    // Two prefabs, each naming an asset no mod and not the game has, and
    // one naming the mod's own sheet and one of the game's files: fine.
    let sprite = |texture: &str| {
        prefab(&format!(
            r#"{{"Transform2D":{{}},"Sprite":{{"texture":"{texture}"}},"Slime":{{"speed":1.0,"hp":1}}}}"#
        ))
    };
    let set = || {
        ModSet::new().with(simple(
            "art",
            &[
                ("enemies/bat.json", &sprite("sprites/bat.png")),
                ("enemies/rat.json", &sprite("sprites/rat.png")),
                ("enemies/own.json", &sprite("sprites/own.png")),
                ("sprites/own.png", "not decoded at mount"),
                ("enemies/games.json", &sprite("ember_hop/sprites/drip.png")),
            ],
        ))
    };
    let err = mount_err(set());
    assert_eq!(err.mod_id(), "art");
    assert_eq!(
        err.file(),
        Some("enemies/bat.json"),
        "the first file that names a missing one"
    );
    assert_eq!(
        err.problem(),
        &ModProblem::MissingAssets(vec![
            ("enemies/bat.json".into(), "sprites/bat.png".into()),
            ("enemies/rat.json".into(), "sprites/rat.png".into()),
        ])
    );
    let text = err.to_string();
    assert!(
        text.contains(r#""sprites/bat.png" (in "enemies/bat.json")"#)
            && text.contains(r#""sprites/rat.png" (in "enemies/rat.json")"#),
        "{text}"
    );
    let engine: ponko::error::Error = err.into();
    assert_eq!(engine.kind(), ponko::error::ErrorKind::NotFound);
}

#[test]
fn an_asset_another_mod_of_the_set_ships_or_an_included_prefab_counts_as_there() {
    let uses = prefab(r#"{"Transform2D":{},"Sprite":{"texture":"shared/sheet.png"}}"#);
    let includes = r#"{"format":"ponko-prefab","version":2,"entities":[
        {"entity":1,"prefab":"enemies/base.json","components":{}},
        {"entity":2,"prefab":"enemies/gone.json","components":{}}]}"#;
    game(|ctx| {
        ModSet::new()
            .with(simple("art", &[("shared/sheet.png", "png bytes")]))
            .with(simple("enemies", &[("enemies/base.json", &uses)]))
            .mount(ctx)
            .expect("the sheet is in the other mod");
    });
    let err = mount_err(ModSet::new().with(simple(
        "tree",
        &[
            ("enemies/king.json", includes),
            ("enemies/base.json", &uses),
            ("shared/sheet.png", "png"),
        ],
    )));
    assert_eq!(
        err.problem(),
        &ModProblem::MissingAssets(vec![("enemies/king.json".into(), "enemies/gone.json".into())])
    );
}

#[test]
fn a_game_registered_asset_field_is_checked_too() {
    #[derive(Component, Serialize, Deserialize)]
    struct ShopItem {
        icon: String,
    }
    let item = prefab(r#"{"ShopItem":{"icon":"icons/gone.png"}}"#);
    let mut err = None;
    Game::new("mods", 1, 1).run_headless(0, |ctx| {
        ctx.world.reflect::<ShopItem>();
        ctx.assets
            .scene_asset_field::<ShopItem>("icon", |ctx, name| ctx.gfx.try_load_texture(name).map(|_| ()));
        err = ModSet::new()
            .with(simple("shop", &[("items/apple.json", &item)]))
            .mount(ctx)
            .err();
    });
    assert_eq!(
        err.expect("refused").problem(),
        &ModProblem::MissingAssets(vec![("items/apple.json".into(), "icons/gone.png".into())])
    );
}

// ---- code without the feature -----------------------------------------------------

#[cfg(not(feature = "mods-wasm"))]
#[test]
fn a_mod_with_code_is_refused_by_a_game_built_without_code_mods() {
    let manifest = r#"{"format":"ponko-mod","version":1,"id":"coded","code":"code.wasm"}"#;
    let wasm = wat::parse_str("(module)").unwrap();
    let pack = ModPack::from_files(
        "coded",
        [("mod.json", manifest.as_bytes().to_vec()), ("code.wasm", wasm)],
    )
    .unwrap();
    let err = mount_err(ModSet::new().with(pack));
    assert_eq!(err.file(), Some("code.wasm"));
    assert!(
        matches!(err.problem(), ModProblem::Code(why) if why.contains("mods-wasm")),
        "{err}"
    );
}
