//! The project templates, embedded from `templates/<kind>/`.
//!
//! The same folders are cargo-generate templates, so their text uses its
//! placeholders (`{{project-name}}`, `{{crate_name}}`) and the manifest is
//! `Cargo.toml.liquid`; [`crate::new`] fills them in the same way. A test
//! checks that every file of a template folder is listed here.

/// Which template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A 2D game: a window, sprites and shapes, default engine features.
    TwoD,
    /// A 3D game: the `3d` feature, a mesh, a camera and a sun.
    ThreeD,
    /// A simulation without window, GPU or sound (`default-features = false`).
    Headless,
    /// A dedicated game server: headless, `run_server`, a bot client and a
    /// container image recipe.
    Server,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Kind; 4] = [Kind::TwoD, Kind::ThreeD, Kind::Headless, Kind::Server];

    /// The template's folder under `templates/`.
    #[cfg(test)]
    pub fn folder(self) -> &'static str {
        match self {
            Kind::TwoD => "2d",
            Kind::ThreeD => "3d",
            Kind::Headless => "headless",
            Kind::Server => "server",
        }
    }

    /// (path in the generated project, text) of every file.
    pub fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kind::TwoD => TWO_D,
            Kind::ThreeD => THREE_D,
            Kind::Headless => HEADLESS,
            Kind::Server => SERVER,
        }
    }
}

/// Files every template has, under the same names. The manifest is
/// `Cargo.toml.liquid` in the folder (see the module docs).
macro_rules! template {
    ($dir:literal, $test:literal $(, $extra:literal)*) => {
        &[
            (
                "Cargo.toml",
                include_str!(concat!("../templates/", $dir, "/Cargo.toml.liquid")),
            ),
            (
                ".gitignore",
                include_str!(concat!("../templates/", $dir, "/.gitignore")),
            ),
            (".mcp.json", include_str!(concat!("../templates/", $dir, "/.mcp.json"))),
            ("AGENTS.md", include_str!(concat!("../templates/", $dir, "/AGENTS.md"))),
            ("CLAUDE.md", include_str!(concat!("../templates/", $dir, "/CLAUDE.md"))),
            ("README.md", include_str!(concat!("../templates/", $dir, "/README.md"))),
            (
                "assets/settings.json",
                include_str!(concat!("../templates/", $dir, "/assets/settings.json")),
            ),
            (
                "src/lib.rs",
                include_str!(concat!("../templates/", $dir, "/src/lib.rs")),
            ),
            (
                "src/main.rs",
                include_str!(concat!("../templates/", $dir, "/src/main.rs")),
            ),
            ($test, include_str!(concat!("../templates/", $dir, "/", $test))),
            $(($extra, include_str!(concat!("../templates/", $dir, "/", $extra))),)*
        ]
    };
}

const TWO_D: &[(&str, &str)] = template!("2d", "tests/game.rs");
const THREE_D: &[(&str, &str)] = template!("3d", "tests/game.rs");
const HEADLESS: &[(&str, &str)] = template!("headless", "tests/sim.rs");
const SERVER: &[(&str, &str)] = template!(
    "server",
    "tests/server.rs",
    "examples/client.rs",
    "Dockerfile",
    ".dockerignore"
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn files_on_disk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path: PathBuf = entry.unwrap().path();
            if path.is_dir() {
                files_on_disk(&path, base, out);
            } else {
                let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.push(rel);
            }
        }
    }

    #[test]
    fn every_file_of_a_template_folder_is_embedded() {
        for kind in Kind::ALL {
            let base = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("templates")
                .join(kind.folder());
            let mut on_disk = Vec::new();
            files_on_disk(&base, &base, &mut on_disk);
            // Only cargo-generate reads its config; `ponko new` doesn't copy it.
            on_disk.retain(|f| f != "cargo-generate.toml");
            let mut embedded: Vec<String> = kind
                .files()
                .iter()
                .map(|(name, _)| {
                    if *name == "Cargo.toml" {
                        "Cargo.toml.liquid".to_string()
                    } else {
                        name.to_string()
                    }
                })
                .collect();
            on_disk.sort();
            embedded.sort();
            assert_eq!(
                on_disk,
                embedded,
                "templates/{}: list new files in templates.rs",
                kind.folder()
            );
        }
    }

    #[test]
    fn templates_use_only_the_placeholders_ponko_new_fills() {
        for kind in Kind::ALL {
            for (name, text) in kind.files() {
                let rest = text.replace("{{project-name}}", "").replace("{{crate_name}}", "");
                assert!(
                    !rest.contains("{{") && !rest.contains("{%"),
                    "templates/{}/{name} has a Liquid tag `ponko new` would leave as it is",
                    kind.folder()
                );
            }
        }
    }

    #[test]
    fn templates_depend_on_this_release_of_the_engine() {
        // `ponko_cli` is released with the engine, at the same version.
        let version = env!("CARGO_PKG_VERSION");
        let mut parts = version.split('.');
        let (major, minor) = (parts.next().unwrap(), parts.next().unwrap());
        let wanted = if major == "0" {
            format!("{major}.{minor}")
        } else {
            major.to_string()
        };
        for kind in Kind::ALL {
            let manifest = kind.files()[0].1;
            let line = manifest.lines().find(|l| l.starts_with("ponko = ")).unwrap();
            assert!(
                line.contains(&format!("version = \"{wanted}\"")),
                "templates/{}/Cargo.toml.liquid should depend on ponko {wanted} (the version of this release): {line}",
                kind.folder()
            );
        }
    }

    #[test]
    fn no_template_opens_a_window_in_its_tests() {
        for kind in Kind::ALL {
            for (name, text) in kind.files() {
                if name.starts_with("tests/") {
                    assert!(text.contains("run_headless("), "{name} runs headless");
                    assert!(
                        !text.contains(".run()"),
                        "templates/{}/{name} must not open a window",
                        kind.folder()
                    );
                }
            }
        }
    }

    /// The server's dependency contract: the engine without default
    /// features and none turned back on, so no window, GPU, audio or
    /// gamepad library reaches the container. `scripts/feature-deps.sh`
    /// checks the dependency graph this line gives.
    #[test]
    fn the_server_template_builds_the_engine_without_window_gpu_or_audio() {
        let manifest = Kind::Server.files()[0].1;
        let line = manifest.lines().find(|l| l.starts_with("ponko = ")).unwrap();
        assert!(line.contains("default-features = false"), "{line}");
        assert!(!line.contains("features = ["), "no engine features: {line}");
        for (name, text) in Kind::Server.files() {
            assert!(
                !text.contains(".run()") && !text.contains(".run(|"),
                "{name} opens no window"
            );
        }
    }

    #[test]
    fn the_server_image_runs_the_package_as_a_non_root_user() {
        let dockerfile = Kind::Server.files().iter().find(|(n, _)| *n == "Dockerfile").unwrap().1;
        assert!(
            dockerfile.contains("ponko package --format folder"),
            "built with ponko package"
        );
        assert!(dockerfile.contains("USER nonroot"));
        assert!(dockerfile.contains("STOPSIGNAL SIGTERM"));
        assert!(dockerfile.contains("ENTRYPOINT [\"/app/{{project-name}}\"]"));
    }
}
