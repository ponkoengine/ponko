//! Import settings in the editor: the `cook.json` that `ponko cook` reads
//! (in the assets folder), shown and changed per asset. The editor reads
//! the rules the way the cooker does (an exact path wins, then the longest
//! matching pattern; `*` within a folder name, `**` across folders), and a
//! change writes an exact-path rule for that one asset, keeping the rest of
//! the file. The cooker lives in the `ponko` command, which doesn't depend
//! on the engine, so both read the same shared fixture in their tests.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The settings file's name, in the assets folder.
pub(crate) const FILE: &str = "cook.json";

/// What an asset is, by its file name (the cooker's kinds, and the editor's
/// own documents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AssetKind {
    Texture,
    Model,
    Sound,
    Shader,
    Prefab,
    Scene,
}

impl AssetKind {
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Texture => "texture",
            AssetKind::Model => "model",
            AssetKind::Sound => "sound",
            AssetKind::Shader => "shader",
            AssetKind::Prefab => "prefab",
            AssetKind::Scene => "scene",
        }
    }

    /// The settings `cook.json` has for this kind: (key, the values it
    /// takes; empty for true/false).
    pub fn settings(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            AssetKind::Texture => &[
                ("texture", &["color", "linear", "normal", "mask", "keep"]),
                ("basis", &[]),
                ("mipmaps", &[]),
            ],
            AssetKind::Model => &[("optimize", &[]), ("quantize", &[])],
            // The cooker only converts WAV; other sounds are copied.
            AssetKind::Sound => &[("audio", &["flac", "keep"])],
            AssetKind::Shader => &[(
                "shader",
                &["material", "sprite", "sky", "effect", "compute", "module", "keep"],
            )],
            _ => &[],
        }
    }
}

/// Checks a rule as the cooker does: `"keep"`, `"skip"` or an object of
/// known settings with values they take.
pub(crate) fn check_rule(rule: &Value) -> Result<(), String> {
    if let Some(word) = rule.as_str() {
        return match word {
            "keep" | "skip" => Ok(()),
            other => Err(format!("expected \"keep\", \"skip\" or an object, got \"{other}\"")),
        };
    }
    let object = rule.as_object().ok_or("expected \"keep\", \"skip\" or an object")?;
    for (key, v) in object {
        let allowed = [
            AssetKind::Texture,
            AssetKind::Model,
            AssetKind::Sound,
            AssetKind::Shader,
        ]
        .iter()
        .flat_map(|k| k.settings())
        .find(|(name, _)| name == key);
        match allowed {
            None => {
                return Err(format!(
                    "unknown setting \"{key}\" (the settings are texture, basis, mipmaps, audio, optimize, quantize and shader)"
                ));
            }
            Some((_, [])) if !v.is_boolean() => return Err(format!("\"{key}\" is true or false")),
            Some((_, values)) if !values.is_empty() && !v.as_str().is_some_and(|s| values.contains(&s)) => {
                return Err(format!("\"{key}\" is {}", values.join(", ")));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Whether `path` matches `pattern`: `*` within a folder name, `**`
/// across folders (the cooker's matching).
pub(crate) fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((b'*', rest)) => {
            let (across, rest) = match rest.split_first() {
                Some((b'*', rest)) => (true, rest),
                _ => (false, rest),
            };
            (0..=path.len())
                .take_while(|&i| across || i == 0 || path[i - 1] != b'/')
                .any(|i| glob(rest, &path[i..]))
        }
        Some((c, rest)) => path.first() == Some(c) && glob(rest, &path[1..]),
    }
}

/// Where an asset's settings come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// Its own (exact-path) rule.
    Exact,
    /// A pattern that matches it.
    Pattern(String),
    /// Nothing in the file: the cooker's defaults.
    Defaults,
}

/// A `cook.json`.
#[derive(Debug, Clone, Default)]
pub(crate) struct CookSettings {
    /// The file (`None`: not on disk, nothing is written).
    pub file: Option<PathBuf>,
    /// The file as read, so a write keeps everything else in it.
    raw: Map<String, Value>,
}

impl CookSettings {
    /// Reads `cook.json` in assets folder `root`; a missing file has no
    /// rules.
    pub fn load(root: &Path) -> Result<Self, String> {
        let file = root.join(FILE);
        let raw = match std::fs::read_to_string(&file) {
            Ok(text) => Self::parse(&text)?.raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        Ok(Self { file: Some(file), raw })
    }

    /// Reads a `cook.json`'s text, checking it as the cooker does.
    pub fn parse(text: &str) -> Result<Self, String> {
        let json: Value = serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
        let Value::Object(raw) = json else {
            return Err(format!("{FILE}: expected an object"));
        };
        for (key, value) in &raw {
            if key != "assets" {
                return Err(format!("{FILE}: unknown key \"{key}\" (the key is \"assets\")"));
            }
            let rules = value
                .as_object()
                .ok_or(format!("{FILE}: \"assets\" must be an object"))?;
            for (path, rule) in rules {
                check_rule(rule).map_err(|e| format!("{FILE}: \"{path}\": {e}"))?;
            }
        }
        Ok(Self { file: None, raw })
    }

    fn rules(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.raw.get("assets").and_then(Value::as_object).into_iter().flatten()
    }

    /// The rule for asset `path` (relative, `/`-separated) and where it
    /// comes from; the defaults are an empty object.
    pub fn rule_for(&self, path: &str) -> (Value, Source) {
        if let Some((_, rule)) = self.rules().find(|(p, _)| *p == path) {
            return (rule.clone(), Source::Exact);
        }
        self.rules()
            .filter(|(pattern, _)| pattern.contains('*') && glob(pattern.as_bytes(), path.as_bytes()))
            .max_by_key(|(pattern, _)| pattern.len())
            .map_or((Value::Object(Map::new()), Source::Defaults), |(p, rule)| {
                (rule.clone(), Source::Pattern(p.clone()))
            })
    }

    /// Gives asset `path` its own rule (`None`: none, so a pattern or the
    /// defaults apply again).
    pub fn set(&mut self, path: &str, rule: Option<Value>) -> Result<(), String> {
        if let Some(rule) = &rule {
            check_rule(rule)?;
        }
        let assets = self.raw.entry("assets").or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(assets) = assets else {
            return Err(format!("{FILE}: \"assets\" must be an object"));
        };
        match rule {
            Some(rule) => drop(assets.insert(path.to_string(), rule)),
            None => drop(assets.remove(path)),
        }
        Ok(())
    }

    /// Writes the file.
    pub fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let text = serde_json::to_string_pretty(&Value::Object(self.raw.clone())).map_err(|e| e.to_string())?;
        std::fs::write(file, text + "\n").map_err(|e| format!("{}: {e}", file.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_path_wins_then_the_longest_pattern_then_the_defaults() {
        let s = CookSettings::parse(
            r#"{ "assets": {
                "ui/**": "keep",
                "ui/icons/*.png": { "texture": "mask" },
                "ui/icons/star.png": { "mipmaps": false } } }"#,
        )
        .unwrap();
        assert_eq!(s.rule_for("ui/icons/star.png").1, Source::Exact);
        assert_eq!(
            s.rule_for("ui/icons/heart.png"),
            (
                serde_json::json!({ "texture": "mask" }),
                Source::Pattern("ui/icons/*.png".into())
            )
        );
        assert_eq!(s.rule_for("ui/big/heart.png").0, "keep");
        assert_eq!(s.rule_for("art/tree.png").1, Source::Defaults);
    }

    #[test]
    fn rules_are_checked_as_the_cooker_checks_them() {
        assert!(check_rule(&serde_json::json!({ "texture": "normal", "basis": true })).is_ok());
        assert!(check_rule(&serde_json::json!("skip")).is_ok());
        assert!(
            check_rule(&serde_json::json!({ "texture": "blue" }))
                .unwrap_err()
                .contains("color, linear")
        );
        assert!(
            check_rule(&serde_json::json!({ "mipmaps": "yes" }))
                .unwrap_err()
                .contains("true or false")
        );
        assert!(
            check_rule(&serde_json::json!({ "speed": 1 }))
                .unwrap_err()
                .contains("unknown setting")
        );
        let bad = CookSettings::parse(r#"{ "assets": { "a.png": { "audio": "mp3" } } }"#);
        assert!(bad.unwrap_err().contains("\"a.png\""));
    }

    #[test]
    fn setting_one_assets_rule_keeps_the_rest_of_the_file() {
        let dir = std::env::temp_dir().join(format!("ponko-cook-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), r#"{ "assets": { "raw/**": "skip" } }"#).unwrap();
        let mut s = CookSettings::load(&dir).unwrap();
        s.set("hero.png", Some(serde_json::json!({ "texture": "linear" })))
            .unwrap();
        assert!(s.set("hero.png", Some(serde_json::json!({ "texture": "x" }))).is_err());
        s.save().unwrap();
        let back = CookSettings::load(&dir).unwrap();
        assert_eq!(back.rule_for("hero.png").0, serde_json::json!({ "texture": "linear" }));
        assert_eq!(back.rule_for("raw/a.psd").0, "skip");
        let mut back = back;
        back.set("hero.png", None).unwrap();
        assert_eq!(back.rule_for("hero.png").1, Source::Defaults);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The cooker's tests read the same fixture (`ponko_cli`'s
    /// `the_editor_fixture_gives_the_rules_the_editor_shows`).
    #[test]
    fn the_shared_fixture_gives_the_rules_the_cooker_uses() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cook-rules");
        let s = CookSettings::load(&root).unwrap();
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("expected.json")).unwrap()).unwrap();
        for (path, want) in expected.as_object().unwrap() {
            assert_eq!(&s.rule_for(path).0, want, "{path}");
        }
    }
}
