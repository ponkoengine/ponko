//! Shader cooking: WGSL files checked the way the engine compiles them.
//!
//! A game's shader file is a part: the engine puts its own code (bindings,
//! types, helpers) before it and its entry points after it, and the result
//! is what the GPU compiles. [`validate`] does the same with naga, the WGSL
//! compiler the engine uses, and reports an error at the line of the game's
//! file. The engine's parts are in `shaders/` next to this file: the engine's
//! test `cook_shader_parts_match_the_engine` writes them from the engine's
//! own composition, and fails when they are out of date.
//!
//! Before compiling, [`validate`] checks the engine's rules for the kind
//! (a hook's signature, no entry points or bindings of its own) from the
//! engine's own rule table: `shaders/rules.rs` is the engine's
//! `src/render/shader_rules.rs`, kept so by the engine's test
//! `the_cooks_shader_rules_are_the_engines`. So a shader that cooks also
//! loads.
//!
//! The kind comes from the hook the file defines (`vertex`/`surface`: a
//! material, `fragment`: a sprite shader, `sky`, `effect`, `compute` or
//! `update`), or from `cook.json`; a file with its own `@vertex`,
//! `@fragment` or `@compute` entry points is checked on its own.

#![deny(missing_docs)]

use super::ShaderKind;

#[path = "shaders/rules.rs"]
mod rules;

/// Where the engine's mesh prelude goes in a part file.
const PRELUDE: &str = "//@@ponko-mesh-prelude@@\n";
/// Where the game's code goes in a part file.
const GAME: &str = "//@@ponko-game@@\n";

const MESH_PRELUDE: &str = include_str!("shaders/mesh_prelude.wgsl");
const MATERIAL: &str = include_str!("shaders/material.wgsl");
const SPRITE: &str = include_str!("shaders/sprite.wgsl");
const SKY: &str = include_str!("shaders/sky.wgsl");
const EFFECT: &str = include_str!("shaders/effect.wgsl");
const COMPUTE: &str = include_str!("shaders/compute.wgsl");
const COMPUTE_UPDATE: &str = include_str!("shaders/compute_update.wgsl");

/// The pass-through hooks the engine adds to a material without them.
const DEFAULT_VERTEX: &str = "fn vertex(v: Vertex) -> Vertex { return v; }\n";
const DEFAULT_SURFACE: &str = "fn surface(s: Surface) -> Surface { return s; }\n";

/// The code before and after the game's in a part file.
fn split(part: &str) -> (String, &str) {
    let (before, after) = part.split_once(GAME).expect("a part file has the game's marker");
    (before.replace(PRELUDE, MESH_PRELUDE), after)
}

/// `source` without its comments (block comments nest, as in WGSL), with
/// each comment replaced by a space.
fn without_comments(source: &str) -> String {
    let s = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(b"//") {
            while i < s.len() && s[i] != b'\n' {
                i += 1;
            }
            out.push(' ');
        } else if s[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < s.len() {
                if s[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if s[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else {
            let end = source[i..].find(['/']).map_or(s.len(), |n| i + n.max(1));
            out.push_str(&source[i..end]);
            i = end;
        }
    }
    out
}

/// Whether `code` (comments removed) defines function `name`.
fn defines(code: &str, name: &str) -> bool {
    let words: Vec<&str> = code
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '('))
        .filter(|w| !w.is_empty())
        .collect();
    words
        .windows(2)
        .any(|w| w[0] == "fn" && (w[1] == name || w[1].strip_prefix(name).is_some_and(|r| r.starts_with('('))))
}

/// The kind of a shader file, from the hooks it defines; `None` when it
/// defines none.
pub fn kind_of(source: &str) -> Option<ShaderKind> {
    let code = without_comments(source);
    if ["@vertex", "@fragment", "@compute"].iter().any(|e| code.contains(e)) {
        return Some(ShaderKind::Module);
    }
    if defines(&code, "vertex") || defines(&code, "surface") {
        Some(ShaderKind::Material)
    } else if defines(&code, "fragment") {
        Some(ShaderKind::Sprite)
    } else if defines(&code, "sky") {
        Some(ShaderKind::Sky)
    } else if defines(&code, "effect") {
        Some(ShaderKind::Effect)
    } else if defines(&code, "compute") || defines(&code, "update") {
        Some(ShaderKind::Compute)
    } else {
        None
    }
}

/// The whole module the engine compiles for a game's `user` code of
/// `kind`, and the byte offset where the game's code starts in it.
pub fn compose(kind: ShaderKind, user: &str) -> (String, usize) {
    let code = without_comments(user);
    let part = match kind {
        ShaderKind::Material => MATERIAL,
        ShaderKind::Sprite => SPRITE,
        ShaderKind::Sky => SKY,
        ShaderKind::Effect => EFFECT,
        ShaderKind::Compute if defines(&code, "update") => COMPUTE_UPDATE,
        ShaderKind::Compute => COMPUTE,
        ShaderKind::Module | ShaderKind::Keep => return (user.to_string(), 0),
    };
    let (before, after) = split(part);
    let mut after = after.to_string();
    if kind == ShaderKind::Material {
        if defines(&code, "vertex") {
            after = after.replacen(DEFAULT_VERTEX, "", 1);
        }
        if defines(&code, "surface") {
            after = after.replacen(DEFAULT_SURFACE, "", 1);
        }
    }
    let start = before.len();
    (format!("{before}{user}\n{after}"), start)
}

/// Where byte `at` of the composed module is, as `name:line:column` of the
/// game's file, or `None` in the engine's code.
fn at_line(name: &str, user: &str, start: usize, at: usize) -> Option<String> {
    let offset = at.checked_sub(start).filter(|&o| o <= user.len())?;
    let before = &user[..offset];
    let line = before.matches('\n').count() + 1;
    let column = offset - before.rfind('\n').map_or(0, |n| n + 1) + 1;
    Some(format!("{name}:{line}:{column}"))
}

/// Checks a shader file as the engine compiles it; returns its kind.
pub fn validate(name: &str, source: &str, kind: Option<ShaderKind>) -> Result<ShaderKind, String> {
    let kind = match kind.or_else(|| kind_of(source)) {
        Some(kind) => kind,
        None => {
            return Err(format!(
                "{name}: defines no shader hook (vertex, surface, fragment, sky, effect, compute or update) \
                 and no entry point; say its kind in cook.json (\"shader\": \"material\", ...) or \"keep\""
            ));
        }
    };
    if let Some((range, message)) = rules::check(kind.name(), source) {
        let place = at_line(name, source, 0, range.start).unwrap_or_else(|| name.to_string());
        return Err(format!("{place}: {message}"));
    }
    let (composed, start) = compose(kind, source);
    let place = |at: Option<usize>| match at.and_then(|at| at_line(name, source, start, at)) {
        Some(place) => place,
        None => format!("{name} (as a {} shader, in the engine's code around it)", kind.name()),
    };
    let module = match naga::front::wgsl::parse_str(&composed) {
        Ok(module) => module,
        Err(e) => {
            let at = e.labels().next().and_then(|(span, _)| span.to_range()).map(|r| r.start);
            return Err(format!("{}: {}", place(at), e.message()));
        }
    };
    // Sprite shaders run on every GPU the engine supports, WebGL2
    // included, as the engine checks them.
    let capabilities = if kind == ShaderKind::Sprite {
        naga::valid::Capabilities::empty()
    } else {
        naga::valid::Capabilities::all()
    };
    if let Err(e) = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities).validate(&module) {
        let mut message = e.as_inner().to_string();
        let mut cause = std::error::Error::source(e.as_inner());
        while let Some(c) = cause {
            message.push_str(": ");
            message.push_str(&c.to_string());
            cause = c.source();
        }
        let at = e
            .spans()
            .filter_map(|(span, _)| span.to_range())
            .min_by_key(|r| r.len())
            .map(|r| r.start);
        return Err(format!("{}: {message}", place(at)));
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_shader(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/shaders")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_engines_example_shaders_validate_as_their_kind() {
        for (file, kind) in [
            ("water.wgsl", ShaderKind::Material),
            ("wind.wgsl", ShaderKind::Material),
            ("sprite_outline.wgsl", ShaderKind::Sprite),
            ("sprite_dissolve.wgsl", ShaderKind::Sprite),
            ("stylized_sky.wgsl", ShaderKind::Sky),
        ] {
            assert_eq!(validate(file, &repo_shader(file), None), Ok(kind), "{file}");
        }
    }

    #[test]
    fn every_kind_validates_a_minimal_hook() {
        for (source, kind) in [
            ("fn surface(s: Surface) -> Surface { return s; }", ShaderKind::Material),
            (
                "fn fragment(p: SpritePixel) -> vec4<f32> { return p.color; }",
                ShaderKind::Sprite,
            ),
            ("fn sky(dir: vec3<f32>) -> vec3<f32> { return dir; }", ShaderKind::Sky),
            (
                "fn effect(p: Pixel) -> vec3<f32> { return p.color.rgb; }",
                ShaderKind::Effect,
            ),
            ("@compute @workgroup_size(1) fn main() {}", ShaderKind::Module),
        ] {
            assert_eq!(validate("s.wgsl", source, None), Ok(kind), "{source}");
        }
    }

    #[test]
    fn an_error_points_at_the_line_of_the_games_file() {
        let source = "// a material\nfn surface(s: Surface) -> Surface {\n    return s.missing;\n}\n";
        let err = validate("bad.wgsl", source, None).unwrap_err();
        assert!(err.starts_with("bad.wgsl:3:"), "{err}");
        let err = validate("typo.wgsl", "fn surface(s: Surface) -> Surface { return s }", None).unwrap_err();
        assert!(err.starts_with("typo.wgsl:1:"), "{err}");
    }

    #[test]
    fn a_file_without_a_hook_needs_its_kind() {
        let err = validate("lib.wgsl", "const k = 1.0;", None).unwrap_err();
        assert!(err.contains("cook.json"), "{err}");
        assert_eq!(
            validate(
                "lib.wgsl",
                "fn helper() -> f32 { return 1.0; }",
                Some(ShaderKind::Material)
            ),
            Ok(ShaderKind::Material)
        );
    }

    #[test]
    fn the_engines_rules_fail_the_cook_where_the_engine_would_fail_the_load() {
        // Each compiles as WGSL, and the engine refuses it at load.
        for (source, kind, says) in [
            (
                "@group(2) @binding(9) var t: texture_2d<f32>;\nfn surface(s: Surface) -> Surface { return s; }",
                ShaderKind::Material,
                "may not declare bindings",
            ),
            (
                "fn surface(s: Surface, extra: f32) -> Surface { return s; }",
                ShaderKind::Material,
                "wrong signature",
            ),
            (
                "@group(0) @binding(9) var t: texture_2d<f32>;\nfn fragment(p: SpritePixel) -> vec4<f32> { return p.color; }",
                ShaderKind::Sprite,
                "may not declare bindings",
            ),
            (
                "fn sky(d: vec3<f32>) -> vec4<f32> { return vec4<f32>(d, 1.0); }",
                ShaderKind::Sky,
                "wrong signature",
            ),
            (
                "fn helper() -> f32 { return 1.0; }",
                ShaderKind::Sky,
                "needs a `sky` hook",
            ),
        ] {
            let err = validate("s.wgsl", source, Some(kind)).unwrap_err();
            assert!(err.starts_with("s.wgsl:") && err.contains(says), "{err}");
        }
        // The place is the line of the game's file.
        let err = validate(
            "b.wgsl",
            "// fine\n@group(2) @binding(9) var t: texture_2d<f32>;\nfn surface(s: Surface) -> Surface { return s; }",
            None,
        )
        .unwrap_err();
        assert!(err.starts_with("b.wgsl:2:1:"), "{err}");
    }

    #[test]
    fn hooks_in_comments_dont_count() {
        assert_eq!(
            kind_of("// fn vertex(v: Vertex)\nfn sky(d: vec3<f32>) -> vec3<f32> { return d; }"),
            Some(ShaderKind::Sky)
        );
        assert_eq!(kind_of("/* fn fragment( /* nested */ */ const a = 1;"), None);
    }
}
