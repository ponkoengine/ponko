//! Loader naming is checked against every feature set in the public API snapshot.
use std::collections::BTreeMap;

fn loaders(snapshot: &str) -> BTreeMap<String, String> {
    snapshot
        .lines()
        .filter_map(|line| {
            let signature = ["pub fn ", "pub unsafe fn ", "pub async fn ", "pub const fn "]
                .iter()
                .find_map(|prefix| line.strip_prefix(prefix))?;
            let end = signature.find(['<', '('])?;
            let path = &signature[..end];
            let name = path.rsplit("::").next()?;
            if !name.starts_with("load_") && !name.starts_with("try_load_") {
                return None;
            }
            let result = signature.rsplit_once(" -> ").map_or("()", |(_, result)| result);
            Some((path.to_owned(), result.to_owned()))
        })
        .collect()
}

/// Runtime operations: they fail in normal play, so they return `Result`
/// under their plain name and have no panicking twin (STABILITY.md, "Errors
/// and panics"). A new entry here is a design decision, made with a table row.
const RUNTIME_OPERATIONS: &[(&str, &str)] = &[
    ("ponko::assets::Assets", "load_with"),
    ("ponko::assets::Assets", "load_level"),
    ("ponko::assets::Assets", "load_reflected_resource_json"),
    ("ponko::assets::Assets", "load_reflected_component_json"),
    ("ponko::prelude::Ctx", "load_scene"),
    ("ponko::prelude::Ctx", "load_scene_lockstep"),
    ("ponko::prelude::Ctx", "load_world"),
];

/// Deprecated forwarding shortcuts on `Ctx`: panicking, and their twins are on
/// the service they forward to. Removed by the next minor.
const DEPRECATED_SHORTCUTS: &[(&str, &str)] = &[
    ("ponko::prelude::Ctx", "load_texture"),
    ("ponko::prelude::Ctx", "load_sound"),
];

fn violations(snapshot: &str) -> Vec<String> {
    let api = loaders(snapshot);
    let mut errors = Vec::new();
    for (path, result) in &api {
        let (owner, name) = path.rsplit_once("::").unwrap();
        let is_result = result.starts_with("core::result::Result<") && result.ends_with(", ponko::error::Error>");
        let runtime = RUNTIME_OPERATIONS.contains(&(owner, name));
        if name.starts_with("try_load_") || runtime {
            if !is_result {
                errors.push(format!("{path} must return Result<_, ponko::error::Error>"));
            }
        } else if !DEPRECATED_SHORTCUTS.contains(&(owner, name)) {
            if is_result {
                errors.push(format!(
                    "{path}: a setup loader panics (and has a try_ twin that returns Result); \
                     a runtime operation goes in RUNTIME_OPERATIONS"
                ));
            }
            let twin = format!("{owner}::try_{name}");
            let expected = format!("core::result::Result<{result}, ponko::error::Error>");
            if api.get(&twin) != Some(&expected) {
                errors.push(format!("{path}: missing matching fallible twin {twin} -> {expected}"));
            }
        }
    }
    errors
}

#[test]
fn every_public_load_follows_the_failure_convention_in_every_snapshot() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("api");
    let mut checked = 0;
    for krate in std::fs::read_dir(root).unwrap() {
        let krate = krate.unwrap().path();
        if !krate.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(krate).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|ext| ext == "txt") {
                let snapshot = std::fs::read_to_string(&file).unwrap();
                checked += loaders(&snapshot).len();
                assert!(
                    violations(&snapshot).is_empty(),
                    "{}: {:?}",
                    file.display(),
                    violations(&snapshot)
                );
            }
        }
    }
    assert!(checked > 20, "must actually walk the engine API snapshots");
}

#[test]
fn new_conforming_loaders_are_accepted_and_policy_violations_are_rejected() {
    let good = "pub fn ponko::anim2d::Library::load_skeleton(&mut self, &str) -> ponko::anim2d::Skeleton\n\
                pub fn ponko::anim2d::Library::try_load_skeleton(&mut self, &str) -> core::result::Result<ponko::anim2d::Skeleton, ponko::error::Error>";
    assert!(violations(good).is_empty());
    assert!(!violations(good.lines().next().unwrap()).is_empty());
    assert!(
        !violations(
            "pub fn ponko::assets::Assets::load_surprise(&self) -> core::result::Result<(), ponko::error::Error>"
        )
        .is_empty()
    );
    assert!(!violations("pub fn ponko::assets::Assets::try_load_surprise(&self)").is_empty());
    // The old Result-returning plain names are no longer a thing to add back.
    let old = "pub fn ponko::prelude::Ctx::load_collision_mesh(&mut self, &str) -> core::result::Result<ponko::d3::CollisionMesh, ponko::error::Error>\n\
               pub fn ponko::prelude::Ctx::try_load_collision_mesh(&mut self, &str) -> core::result::Result<ponko::d3::CollisionMesh, ponko::error::Error>";
    assert!(!violations(old).is_empty());
}

#[test]
fn stability_and_data_files_publish_the_same_loader_inventory() {
    fn table(text: &str) -> &str {
        text.split_once("<!-- loader-policy-table:start -->")
            .unwrap()
            .1
            .split_once("<!-- loader-policy-table:end -->")
            .unwrap()
            .0
    }
    assert_eq!(
        table(include_str!("../STABILITY.md")),
        table(include_str!("../book/src/data-files.md"))
    );
}

#[test]
fn the_published_inventory_names_every_public_load_in_the_snapshots() {
    let text = include_str!("../STABILITY.md");
    let table = text
        .split_once("<!-- loader-policy-table:start -->")
        .unwrap()
        .1
        .split_once("<!-- loader-policy-table:end -->")
        .unwrap()
        .0;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("api");
    let mut missing = std::collections::BTreeSet::new();
    for krate in std::fs::read_dir(root).unwrap() {
        let krate = krate.unwrap().path();
        if !krate.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(krate).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|ext| ext == "txt") {
                for path in loaders(&std::fs::read_to_string(&file).unwrap()).keys() {
                    let name = path.rsplit("::").next().unwrap();
                    if !name.starts_with("try_")
                        && !table.contains(&format!("::{name}`"))
                        && !table.contains(&format!("`{name}`"))
                    {
                        missing.insert(path.clone());
                    }
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "add these loaders to the table in STABILITY.md and book/src/data-files.md: {missing:?}"
    );
}
