//! The first games, the two tutorials and the three reference games, start
//! from `use ponko::prelude::*;` and their dimension module (`ponko::d2` or
//! `ponko::d3`). Everything else they import from the engine is listed in
//! [`EXTRA`], with the reason it is not in the prelude. A new import fails
//! here until it is listed with its reason, or the name moves into the
//! prelude; a listed import the game no longer needs fails too, and so does
//! importing a name the prelude or the dimension module already brings.
//!
//! The games themselves compile in `cargo build --examples` (and their
//! tests in `tests/<game>.rs`), so this file checks their sources' paths.

use std::collections::{BTreeMap, BTreeSet};

/// A game: its name, its dimension module, and its source files.
struct FirstGame {
    name: &'static str,
    dimension: &'static str,
    files: &'static [(&'static str, &'static str)],
}

macro_rules! sources {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../examples/", $path)))),*]
    };
}

const GAMES: &[FirstGame] = &[
    FirstGame {
        name: "tutorial_2d",
        dimension: "d2",
        files: sources!("tutorial_2d/game.rs", "tutorial_2d/main.rs"),
    },
    FirstGame {
        name: "tutorial_3d",
        dimension: "d3",
        files: sources!("tutorial_3d/game.rs", "tutorial_3d/main.rs"),
    },
    FirstGame {
        name: "coin_dash",
        dimension: "d2",
        files: sources!(
            "coin_dash/game.rs",
            "coin_dash/settings.rs",
            "coin_dash/smoke.rs",
            "coin_dash/main.rs"
        ),
    },
    FirstGame {
        name: "ember_hop",
        dimension: "d2",
        files: sources!("ember_hop/game.rs", "ember_hop/smoke.rs", "ember_hop/main.rs"),
    },
    FirstGame {
        name: "shard_grove",
        dimension: "d3",
        files: sources!("shard_grove/game.rs", "shard_grove/smoke.rs", "shard_grove/main.rs"),
    },
];

/// Every engine path a first game names beyond the prelude and its
/// dimension module: (game, module, items, why it stays out of the prelude).
const EXTRA: &[(&str, &str, &[&str], &str)] = &[
    ("tutorial_2d", "ponko::tilemap", &["LevelObject"], LEVEL),
    (
        "tutorial_3d",
        "ponko::render",
        &["MaterialDesc", "Mesh", "MeshData"],
        MESH,
    ),
    ("coin_dash", "ponko::locale", &["Arg", "Strings"], LOCALE),
    ("coin_dash", "ponko::state", &["State"], STATE),
    ("coin_dash", "ponko::storage", &["Storage"], STORAGE),
    (
        "ember_hop",
        "ponko::particles",
        &["ParticleCurve", "ParticleGradient"],
        PARTICLES,
    ),
    ("ember_hop", "ponko::render", &["BlendMode"], BLEND),
    ("ember_hop", "ponko::sheet", &["Animation"], SHEET),
    ("ember_hop", "ponko::state", &["State"], STATE),
    ("ember_hop", "ponko::storage", &["Storage"], STORAGE),
    (
        "ember_hop",
        "ponko::tilemap",
        &[
            "AutoTile",
            "LevelLayer",
            "LevelObject",
            "LevelOptions",
            "LevelRoot",
            "TileMap",
        ],
        LEVEL,
    ),
    (
        "shard_grove",
        "ponko::anim",
        &[
            "AnimationClip",
            "AnimationCondition",
            "AnimationGraph",
            "AnimationParameter",
            "AnimationState",
            "AnimationTransition",
            "Animator",
            "Motion",
            "ParamId",
            "Skeleton",
            "StateId",
        ],
        ANIM,
    ),
    ("shard_grove", "ponko::audio", &["AudioEffects", "AudioRolloff"], AUDIO),
    ("shard_grove", "ponko::nav", &["NavBakeSettings", "NavSource"], NAV),
    (
        "shard_grove",
        "ponko::particles",
        &["ParticleCurve", "ParticleGradient"],
        PARTICLES,
    ),
    ("shard_grove", "ponko::render", &["GraphicsQuality"], QUALITY),
    (
        "shard_grove",
        "ponko::render",
        &["MaterialDesc", "Mesh", "MeshData"],
        MESH,
    ),
    ("shard_grove", "ponko::storage", &["Storage"], STORAGE),
];

const LEVEL: &str = "Level-file loading is one topic with many types (`TileMap`, `LevelRoot`, layers, objects); its module is its home, and only levels made in an editor need it.";
const MESH: &str = "Meshes and materials made in code: `Mesh` is a name games define themselves, and `MaterialDesc` goes with `render::Material`, which stays out for the same reason. `Model` (drawing a loaded glTF) is in the prelude.";
const LOCALE: &str = "Translation is a topic of its own (string tables, arguments, right-to-left text), and `Arg` and `Strings` are names a game may use itself.";
const STATE: &str = "`State` is the name a game most often gives its own state enum; systems read the state with `ctx.state::<S>()` and `in_state`, both without the import. Only helpers that get a bare `&World` (tests, smoke runs) name the resource.";
const STORAGE: &str = "`Storage` is a common game name (chests, warehouses), and only save code that is split into functions names the service; `ctx.storage` needs no import.";
const PARTICLES: &str = "Particle curves and gradients tune an effect beyond its preset settings; the emitters themselves are in the dimension module.";
const BLEND: &str = "Blend modes are a rendering detail most sprites never set.";
const SHEET: &str = "Games often call their own type `Animation`; `SpriteSheet` itself is in the prelude.";
const ANIM: &str = "Skeletal animation graphs are a topic with many types and short generic names (`Motion`, `StateId`, `Skeleton`); the module is their home.";
const AUDIO: &str =
    "Effects and roll-off curves tune sound beyond `ctx.audio.play`; the everyday sound types are in the prelude.";
const NAV: &str = "Baking navigation meshes is an advanced 3D topic, with its own module.";
const QUALITY: &str = "Graphics quality presets belong to settings screens, not to the first hour.";

/// The prelude and the dimension modules, as source, for the names they bring.
const PRELUDE: &str = include_str!("../src/prelude.rs");
const D2: &str = include_str!("../src/d2.rs");
const D3: &str = include_str!("../src/d3.rs");

/// The source without comments (line comments and doc comments).
fn code(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            out.push('\n');
            continue;
        }
        // A trailing comment, `code; // note`, starts at a `//` after a space.
        let line = line.find(" //").map_or(line, |at| &line[..at]);
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits `a, b::{c, d}, e as f` at its top-level commas.
fn split_top(list: &str) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0, 0, Vec::new());
    for (i, c) in list.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(list[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(list[start..].trim());
    out.retain(|s| !s.is_empty());
    out
}

/// The full paths of a path tree: `ponko::a::{B, c::D as E}` is
/// `ponko::a::B` and `ponko::a::c::D`.
fn expand(prefix: &str, tree: &str, out: &mut Vec<String>) {
    let tree = tree.trim();
    if let Some(open) = tree.find('{') {
        let head = tree[..open].trim_end_matches("::");
        let inner = &tree[open + 1..tree.rfind('}').expect("a closed brace")];
        let prefix = if head.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}::{head}")
        };
        for item in split_top(inner) {
            expand(&prefix, item, out);
        }
    } else {
        let name = tree.split(" as ").next().unwrap().trim();
        if name == "self" {
            out.push(prefix.to_string());
        } else {
            out.push(format!("{prefix}::{name}"));
        }
    }
}

/// Every `ponko::...` path in `source`, braces expanded.
fn engine_paths(source: &str) -> Vec<String> {
    let code = code(source);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find("ponko::") {
        let start = from + at;
        from = start + "ponko::".len();
        if code[..start].chars().next_back().is_some_and(is_ident) {
            continue; // `my_ponko::...`
        }
        // The path: identifiers and `::`, then maybe a brace group.
        let chars: Vec<char> = code[from..].chars().collect();
        let mut end = 0;
        while end < chars.len() {
            let c = chars[end];
            if is_ident(c) || c == '*' {
                end += 1;
            } else if c == ':' && chars.get(end + 1) == Some(&':') {
                end += 2;
            } else if c == '{' {
                let mut depth = 0;
                while end < chars.len() {
                    depth += match chars[end] {
                        '{' => 1,
                        '}' => -1,
                        _ => 0,
                    };
                    end += 1;
                    if depth == 0 {
                        break;
                    }
                }
                break;
            } else {
                break;
            }
        }
        let tree: String = chars[..end].iter().collect();
        let tree = tree.trim_end_matches(':');
        expand("ponko", tree, &mut out);
    }
    // `ponko::d2::Transform::at` names `ponko::d2::Transform`: keep paths up
    // to their first item (an uppercase or `*` segment, or a lowercase one
    // after a module that holds functions).
    out.into_iter().map(|path| item_path(&path)).collect()
}

/// `ponko::a::B::method` → `ponko::a::B`; `ponko::a::b` stays.
fn item_path(path: &str) -> String {
    let segments: Vec<&str> = path.split("::").collect();
    let first_item = segments
        .iter()
        .position(|s| s.starts_with(|c: char| c.is_ascii_uppercase()) || *s == "*");
    match first_item {
        Some(i) => segments[..=i].join("::"),
        None => path.to_string(),
    }
}

/// Names a module source brings with `pub use` (aliases as aliased) and
/// `pub` items.
fn exported_names(source: &str) -> BTreeSet<String> {
    let code = code(source);
    let mut names = BTreeSet::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find("pub use ") {
        let stmt = &rest[at + "pub use ".len()..];
        let end = stmt.find(';').expect("a `;` after `pub use`");
        let mut paths = Vec::new();
        expand("", &stmt[..end], &mut paths);
        // `expand` dropped the `as` aliases: read them back from the text.
        for item in split_top(&stmt[..end].replace(['{', '}'], ",")) {
            let name = match item.split_once(" as ") {
                Some((_, alias)) => alias.trim(),
                None => item.rsplit("::").next().unwrap().trim(),
            };
            if !name.is_empty() && name != "*" && name != "self" {
                names.insert(name.to_string());
            }
        }
        rest = &stmt[end..];
    }
    for line in code.lines() {
        let line = line.trim_start();
        for kind in [
            "pub type ",
            "pub struct ",
            "pub enum ",
            "pub trait ",
            "pub fn ",
            "pub mod ",
        ] {
            if let Some(rest) = line.strip_prefix(kind) {
                let name: String = rest.chars().take_while(|c| is_ident(*c)).collect();
                names.insert(name);
            }
        }
    }
    names
}

/// The engine paths each game names beyond the prelude and its dimension.
fn extras() -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut found = BTreeMap::new();
    for game in GAMES {
        let own = format!("ponko::{}", game.dimension);
        let set: &mut BTreeSet<String> = found.entry(game.name).or_default();
        for (_, source) in game.files {
            for path in engine_paths(source) {
                let allowed = path == "ponko::prelude"
                    || path.starts_with("ponko::prelude::")
                    || path == own
                    || path.starts_with(&format!("{own}::"));
                if !allowed {
                    set.insert(path);
                }
            }
        }
    }
    found
}

fn listed() -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut listed: BTreeMap<&str, BTreeSet<String>> = GAMES.iter().map(|g| (g.name, BTreeSet::new())).collect();
    for (game, module, items, why) in EXTRA {
        assert!(!why.trim().is_empty(), "{game}: `{module}` needs its reason");
        assert!(
            listed.contains_key(game),
            "EXTRA names `{game}`, which is not a first game"
        );
        for item in *items {
            listed.get_mut(game).unwrap().insert(format!("{module}::{item}"));
        }
    }
    listed
}

#[test]
fn first_games_import_only_the_listed_names_beyond_the_prelude() {
    let found = extras();
    let listed = listed();
    let mut problems = Vec::new();
    for game in GAMES {
        let (found, listed) = (&found[game.name], &listed[game.name]);
        for path in found.difference(listed) {
            problems.push(format!(
                "{}: imports `{path}`, which is not in EXTRA: list it with the reason it stays out of the prelude, or add it to the prelude",
                game.name
            ));
        }
        for path in listed.difference(found) {
            problems.push(format!(
                "{}: EXTRA lists `{path}`, which the game no longer names: drop it",
                game.name
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn first_games_import_nothing_the_prelude_or_their_dimension_already_brings() {
    let prelude = exported_names(PRELUDE);
    let mut problems = Vec::new();
    for game in GAMES {
        let dimension = exported_names(if game.dimension == "d2" { D2 } else { D3 });
        for path in &extras()[game.name] {
            let name = path.rsplit("::").next().unwrap();
            let module = path.split("::").nth(1).unwrap_or_default();
            if prelude.contains(module) {
                problems.push(format!(
                    "{}: the prelude brings the module `{module}`: write `{}`",
                    game.name,
                    path.trim_start_matches("ponko::")
                ));
            } else if prelude.contains(name) {
                problems.push(format!(
                    "{}: `{path}` is in the prelude: use the prelude's name",
                    game.name
                ));
            } else if dimension.contains(name) {
                problems.push(format!(
                    "{}: `{path}` is in `ponko::{}`: use the dimension module's name",
                    game.name, game.dimension
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_path_reader_expands_braces_aliases_and_item_paths() {
    let source = "use ponko::a::{B, c::{D, E as F}};\n// use ponko::hidden::G;\nlet x = ponko::h::I::new(); // ponko::j::K\nuse my_ponko::L;";
    assert_eq!(
        engine_paths(source),
        ["ponko::a::B", "ponko::a::c::D", "ponko::a::c::E", "ponko::h::I"]
    );
    let names = exported_names("pub use crate::a::{B, C as D};\npub use crate::e::F as G;\npub struct H;\n");
    assert_eq!(names.into_iter().collect::<Vec<_>>(), ["B", "D", "G", "H"]);
}

/// A game's own names next to the prelude's glob: if the prelude still had
/// one of them, its use below would be ambiguous (a compile error). These
/// are what implementing an asset source or the setup-result bound takes;
/// they stay in `ponko::assets` and `ponko::game`.
mod game_names {
    pub struct AssetSource;
    pub struct MemorySource;
    pub struct PackSource;
    pub struct SetupResult;
}

#[test]
fn asset_source_machinery_and_setup_result_are_not_in_the_prelude() {
    use game_names::*;
    use ponko::prelude::*;
    let _ = (AssetSource, MemorySource, PackSource, SetupResult);
    let _ = World::new();
}

/// Names the reference games import from their modules because games
/// define them too (see [`EXTRA`]): kept out of the prelude the same way.
mod own_names {
    pub struct State;
    pub struct Storage;
    pub struct Mesh;
    pub struct Animation;
    pub struct Strings;
    pub struct Arg;
}

#[test]
fn names_games_define_themselves_stay_out_of_the_prelude() {
    use own_names::*;
    use ponko::prelude::*;
    let _ = (State, Storage, Mesh, Animation, Strings, Arg);
    let _ = World::new();
}

#[test]
fn menus_and_gamepads_need_no_import_beyond_the_prelude() {
    use ponko::prelude::*;
    use std::any::TypeId;
    assert_eq!(TypeId::of::<UiEvent>(), TypeId::of::<ponko::ui::UiEvent>());
    assert_eq!(TypeId::of::<UiLayout>(), TypeId::of::<ponko::ui::UiLayout>());
    assert_eq!(
        TypeId::of::<GamepadButton>(),
        TypeId::of::<ponko::input::GamepadButton>()
    );
    let pad = Gamepad::from_slot(0).expect("slot 0 exists");
    let _ = (pad, GamepadAxis::LeftStickX, GamepadStick::Left);
}

#[test]
fn the_prelude_math_types_are_the_math_modules() {
    use std::any::TypeId;
    assert_eq!(TypeId::of::<ponko::prelude::Vec2>(), TypeId::of::<ponko::math::Vec2>());
    assert_eq!(TypeId::of::<ponko::prelude::Quat>(), TypeId::of::<ponko::math::Quat>());
    assert_eq!(ponko::prelude::vec3(1.0, 2.0, 3.0), ponko::math::vec3(1.0, 2.0, 3.0));
    let turn = ponko::math::Quat::from_euler(ponko::math::EulerRot::YXZ, 0.0, 0.0, 0.0);
    assert_eq!(turn, ponko::math::Quat::IDENTITY);
}
