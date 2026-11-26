//! The load sweep of well-known content, headless: every model of the
//! Khronos glTF Sample Assets that `scripts/fetch-known-content.py` put in
//! the cache loads through `ctx.gfx.try_load_mesh`, fails with an error
//! naming the file, or is skipped because it needs a glTF extension the
//! engine doesn't claim (the error names it). Each sample's outcome must
//! be the one `tests/fixtures/known-content/outcomes.txt` records for the
//! pinned revision, so a loader change that breaks a sample fails here
//! (and one that fixes a sample asks for the list and the guide's table to
//! be updated: `PONKO_UPDATE_KNOWN_CONTENT=1`).
//!
//! Nothing is downloaded here. Without the cache (the default: nothing was
//! fetched) the test says so and passes; the `known-content` job of
//! `scripts/checks.yml` fetches first. The cache is `PONKO_KNOWN_CONTENT`,
//! else `.cache/known-content` in the repository. Each run writes
//! `results/load.json` there.
#![cfg(feature = "3d")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ponko::prelude::*;
use serde_json::{Value, json};

/// Reads asset names as files under a folder (the cache).
struct Folder(PathBuf);

impl ponko::assets::AssetSource for Folder {
    fn id(&self) -> &str {
        "known-content"
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        // Asset names here come from the cache's own files: refuse any
        // that climbs out of it.
        if name.split(['/', '\\']).any(|part| part == "..") {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        std::fs::read(self.0.join(name))
    }
}

fn cache() -> PathBuf {
    std::env::var_os("PONKO_KNOWN_CONTENT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/known-content"),
        PathBuf::from,
    )
}

const OUTCOMES: &str = "tests/fixtures/known-content/outcomes.txt";

/// What loading one file came to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Loaded,
    /// It needs these extensions, which the engine doesn't read.
    Skipped(Vec<String>),
    Failed(String),
}

impl Outcome {
    /// The outcome as the list records it: `loaded`, `skipped EXT ...`,
    /// `failed` (the message changes with the wording; not recorded).
    fn key(&self) -> String {
        match self {
            Self::Loaded => "loaded".into(),
            Self::Skipped(extensions) => format!("skipped {}", extensions.join(" ")),
            Self::Failed(_) => "failed".into(),
        }
    }
}

/// The extension an "isn't read" error names, if it is one.
fn missing_extension(message: &str) -> Option<String> {
    let rest = message.split("needs the glTF extension ").nth(1)?;
    Some(rest.split([',', ' ']).next()?.to_string())
}

/// Loads `file` from the cache in a headless game of its own; a panic in
/// the loader is a failure too.
fn load(root: &Path, file: &str) -> Outcome {
    let root = root.to_path_buf();
    let file = file.to_string();
    let result = std::panic::catch_unwind(move || {
        let mut outcome = None;
        Game::new("known content", 64, 64).run_headless(0, |ctx| {
            ctx.assets.add_source(Folder(root));
            outcome = Some(ctx.gfx.try_load_mesh(&file).map(|_| ()).map_err(|e| e.to_string()));
        });
        outcome.expect("the setup ran")
    });
    match result {
        Ok(Ok(())) => Outcome::Loaded,
        Ok(Err(message)) => match missing_extension(&message) {
            Some(extension) => Outcome::Skipped(vec![extension]),
            None => Outcome::Failed(message),
        },
        Err(panic) => {
            let text = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            Outcome::Failed(format!("panicked: {text}"))
        }
    }
}

/// The recorded outcomes: the revision they are for, and each sample's.
fn recorded() -> (String, BTreeMap<String, String>) {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(OUTCOMES)).unwrap_or_default();
    let mut revision = String::new();
    let mut outcomes = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(rev) = line.strip_prefix("# revision ") {
            revision = rev.trim().to_string();
        } else if !line.starts_with('#')
            && let Some((name, outcome)) = line.split_once(": ")
        {
            outcomes.insert(name.to_string(), outcome.to_string());
        }
    }
    (revision, outcomes)
}

fn write_recorded(revision: &str, outcomes: &BTreeMap<String, String>) {
    let mut text = format!(
        "# What loading each Khronos glTF sample comes to (tests/known_content.rs).\n\
         # Rewritten by PONKO_UPDATE_KNOWN_CONTENT=1 cargo test --features 3d,compressed-textures,\
         gltf-meshopt,image-webp --test known_content\n\
         # revision {revision}\n"
    );
    for (name, outcome) in outcomes {
        text.push_str(&format!("{name}: {outcome}\n"));
    }
    std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join(OUTCOMES), text).unwrap();
}

#[test]
fn every_sample_loads_or_names_the_extension_it_needs() {
    let root = cache();
    let Ok(manifest) = std::fs::read_to_string(root.join("manifest.json")) else {
        eprintln!(
            "skipped: no well-known content in {} (python3 scripts/fetch-known-content.py fetches it)",
            root.display()
        );
        return;
    };
    let manifest: Value = serde_json::from_str(&manifest).expect("the cache's manifest.json is JSON");
    let revision = manifest["samples"]["revision"].as_str().unwrap_or_default().to_string();
    let samples: Vec<&Value> = manifest["assets"]
        .as_array()
        .expect("the manifest lists assets")
        .iter()
        .filter(|a| a["set"] == "samples")
        .collect();
    assert!(!samples.is_empty(), "the manifest lists no samples");

    let mut results = Vec::new();
    let mut found = BTreeMap::new();
    for asset in &samples {
        let name = asset["name"].as_str().unwrap();
        let file = asset["file"].as_str().unwrap();
        let outcome = load(&root, file);
        if let Outcome::Failed(message) = &outcome {
            // A clear error: it says which file.
            assert!(
                message.contains(file.rsplit('/').next().unwrap()) || message.starts_with("panicked"),
                "{name}: the error doesn't name the file: {message}"
            );
        }
        eprintln!("{name:40} {}", outcome.key());
        results.push(json!({
            "name": name,
            "file": file,
            "outcome": match &outcome { Outcome::Loaded => "loaded", Outcome::Skipped(_) => "skipped", Outcome::Failed(_) => "failed" },
            "extensions": match &outcome { Outcome::Skipped(e) => e.clone(), _ => Vec::new() },
            "error": match &outcome { Outcome::Failed(m) => m.clone(), _ => String::new() },
        }));
        found.insert(name.to_string(), outcome);
    }
    let out = root.join("results");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("load.json"),
        serde_json::to_string_pretty(&json!({ "revision": revision, "assets": results })).unwrap(),
    )
    .unwrap();
    let loaded = found.values().filter(|o| **o == Outcome::Loaded).count();
    let skipped = found.values().filter(|o| matches!(o, Outcome::Skipped(_))).count();
    eprintln!(
        "{} samples: {loaded} loaded, {skipped} skipped, {} failed",
        found.len(),
        found.len() - loaded - skipped
    );

    let (recorded_revision, mut recorded) = recorded();
    if std::env::var_os("PONKO_UPDATE_KNOWN_CONTENT").is_some() {
        for (name, outcome) in &found {
            recorded.insert(name.clone(), outcome.key());
        }
        write_recorded(&revision, &recorded);
        return;
    }
    if recorded_revision != revision {
        eprintln!(
            "the cache holds revision {revision}, {OUTCOMES} is for {recorded_revision}: outcomes not compared \
             (fetch with the script of this checkout, or update the list)"
        );
        return;
    }
    let changed: Vec<String> = found
        .iter()
        .filter_map(|(name, outcome)| {
            let was = recorded.get(name).map_or("(not listed)", String::as_str);
            (was != outcome.key()).then(|| match outcome {
                Outcome::Failed(message) => format!("{name}: {was} -> failed: {message}"),
                _ => format!("{name}: {was} -> {}", outcome.key()),
            })
        })
        .collect();
    assert!(
        changed.is_empty(),
        "samples whose outcome changed (a fix: update {OUTCOMES} with PONKO_UPDATE_KNOWN_CONTENT=1 and the \
         guide's table; a regression: fix it):\n{}",
        changed.join("\n")
    );
}

#[test]
fn an_unread_extension_is_told_apart_from_a_failure() {
    let message = "Box.gltf: needs the glTF extension KHR_draco_mesh_compression, which ponko doesn't read (Draco)";
    assert_eq!(
        missing_extension(message).as_deref(),
        Some("KHR_draco_mesh_compression")
    );
    assert_eq!(missing_extension("Box.gltf: buffer 0 is shorter than it says"), None);
}
