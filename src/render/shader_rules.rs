//! The rules a game's shader hooks follow, one table for every kind with
//! rules: which hooks it defines and their signatures, and that it declares
//! no entry points and no bindings of its own (the engine owns them).
//!
//! The engine checks a shader with them before compiling it
//! (`shader_diagnostics`), and `ponko cook` checks shader files with them
//! before a game ships: its copy, `crates/ponko_cli/src/cook/shaders/rules.rs`,
//! is this file, which the engine's test
//! `the_cooks_shader_rules_are_the_engines` keeps so. This file uses only
//! `std`, so both crates compile it as it is.

use std::ops::Range;

/// One hook: `fn <name>(<any name>: <param>) -> <ret>`, as WGSL tokens.
pub(crate) struct Hook {
    /// Its function name.
    pub name: &'static str,
    /// Its parameter's type.
    pub param: &'static [&'static str],
    /// Its return type.
    pub ret: &'static [&'static str],
    /// The message for a wrong signature.
    pub wrong: &'static str,
}

/// The rules of one kind of shader.
pub(crate) struct Rules {
    /// The kind's name: `material`, `sprite` or `sky`.
    pub kind: &'static str,
    /// Its hooks.
    pub hooks: &'static [Hook],
    /// The message when it defines none of them, if one is required.
    pub missing: Option<&'static str>,
    /// The message for a `@vertex`, `@fragment` or `@compute` entry point.
    pub entry_point: &'static str,
    /// The message for a `@group` or `@binding`.
    pub bindings: &'static str,
}

const VEC3: &[&str] = &["vec3", "<", "f32", ">"];
const VEC4: &[&str] = &["vec4", "<", "f32", ">"];

/// Every kind with rules. Screen effects and compute passes have none
/// beyond compiling.
pub(crate) const RULES: &[Rules] = &[
    Rules {
        kind: "material",
        hooks: &[
            Hook {
                name: "vertex",
                param: &["Vertex"],
                ret: &["Vertex"],
                wrong: "material hook `vertex` has the wrong signature; expected `fn vertex(value: Vertex) -> Vertex`; replace its parameters and return type with this signature (or remove the hook for the pass-through default)",
            },
            Hook {
                name: "surface",
                param: &["Surface"],
                ret: &["Surface"],
                wrong: "material hook `surface` has the wrong signature; expected `fn surface(value: Surface) -> Surface`; replace its parameters and return type with this signature (or remove the hook for the pass-through default)",
            },
        ],
        missing: None,
        entry_point: "material hooks `vertex`/`surface` are ordinary functions, not shader-stage entry points; expected `fn vertex(value: Vertex) -> Vertex` or `fn surface(value: Surface) -> Surface`; remove the @vertex/@fragment/@compute attribute and let the engine provide the entry points",
        bindings: "material hooks `vertex`/`surface` may not declare bindings: every @group/@binding is engine-owned; remove the declaration and use material.custom, custom_texture0..1, custom_array0..2 or items through the engine API",
    },
    Rules {
        kind: "sprite",
        hooks: &[Hook {
            name: "fragment",
            param: &["SpritePixel"],
            ret: VEC4,
            wrong: "sprite hook `fragment` has the wrong signature; expected `fn fragment(p: SpritePixel) -> vec4<f32>`",
        }],
        missing: Some(
            "a sprite shader needs a `fragment` hook: `fn fragment(p: SpritePixel) -> vec4<f32> { return p.color; }`",
        ),
        entry_point: "the sprite hook `fragment` is an ordinary function, not a shader-stage entry point; expected `fn fragment(p: SpritePixel) -> vec4<f32>`; remove the @vertex/@fragment/@compute attribute and let the engine provide the entry points",
        bindings: "sprite shaders may not declare bindings: every @group/@binding is the engine's; use material.params, material_sample0/1 (the material's textures) and sprite_sample instead",
    },
    Rules {
        kind: "sky",
        hooks: &[Hook {
            name: "sky",
            param: VEC3,
            ret: VEC3,
            wrong: "sky hook `sky` has the wrong signature; expected `fn sky(dir: vec3<f32>) -> vec3<f32>`",
        }],
        missing: Some(
            "a sky shader needs a `sky` hook: `fn sky(dir: vec3<f32>) -> vec3<f32> { return vec3<f32>(0.4, 0.6, 1.0); }`",
        ),
        entry_point: "the sky hook `sky` is an ordinary function, not a shader-stage entry point; expected `fn sky(dir: vec3<f32>) -> vec3<f32>`; remove the @vertex/@fragment/@compute attribute and let the engine provide the entry points",
        bindings: "sky shaders may not declare bindings: every @group/@binding is the engine's; pass values through `Sky::shader_params` (`frame.sky_params`)",
    },
];

/// Checks a game's shader code (`user`, alone) of kind `kind` against its
/// rules: the first problem, as a byte range of `user` and a message.
/// `None` when it follows them, or the kind has no rules.
pub(crate) fn check(kind: &str, user: &str) -> Option<(Range<usize>, String)> {
    let rules = RULES.iter().find(|r| r.kind == kind)?;
    let tokens = tokens(user);
    let mut found = false;
    for (i, (token, span)) in tokens.iter().enumerate() {
        let next = tokens.get(i + 1).map(|t| t.0);
        if *token == "@" && matches!(next, Some("vertex" | "fragment" | "compute")) {
            return Some((span.clone(), rules.entry_point.into()));
        }
        if *token == "@" && matches!(next, Some("group" | "binding")) {
            return Some((span.clone(), rules.bindings.into()));
        }
        if *token != "fn" {
            continue;
        }
        let Some(hook) = rules.hooks.iter().find(|h| Some(h.name) == next) else {
            continue;
        };
        let signature: Vec<&str> = tokens
            .iter()
            .skip(i + 2)
            .take_while(|t| t.0 != "{")
            .map(|t| t.0)
            .collect();
        if !signature_is(&signature, hook) {
            return Some((span.clone(), hook.wrong.into()));
        }
        found = true;
    }
    match rules.missing {
        Some(message) if !found => Some((0..user.len().min(1), message.into())),
        _ => None,
    }
}

/// Whether `signature` (the tokens from `(` to before `{`) is `hook`'s:
/// `(name: param) -> ret`. Parameter names are the game's; a trailing
/// comma is WGSL.
fn signature_is(signature: &[&str], hook: &Hook) -> bool {
    let Some(rest) = signature
        .strip_prefix(&["("][..])
        .and_then(|s| s.get(1..))
        .and_then(|s| s.strip_prefix(&[":"][..]))
        .and_then(|s| s.strip_prefix(hook.param))
    else {
        return false;
    };
    let rest = rest.strip_prefix(&[","][..]).unwrap_or(rest);
    rest.strip_prefix(&[")", "->"][..]) == Some(hook.ret)
}

/// WGSL tokens with byte spans, without line and (nested) block comments.
pub(crate) fn tokens(source: &str) -> Vec<(&str, Range<usize>)> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source[i..].starts_with("//") {
            i += source[i..].find('\n').unwrap_or(source.len() - i);
            continue;
        }
        if source[i..].starts_with("/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if source[i..].starts_with("/*") {
                    depth += 1;
                    i += 2;
                } else if source[i..].starts_with("*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += source[i..].chars().next().map_or(1, char::len_utf8);
                }
            }
            continue;
        }
        let start = i;
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        } else if source[i..].starts_with("->") {
            i += 2;
        } else {
            i += source[i..].chars().next().map_or(1, char::len_utf8);
        }
        tokens.push((&source[start..i], start..i));
    }
    tokens
}
