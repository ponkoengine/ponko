//! Turns naga's errors about a composed shader (the engine's prelude, the
//! game's code, the engine's entry points) into a [`ShaderDiagnostic`] that
//! points at the line in the game's own WGSL.

use crate::render::{ShaderDiagnostic, ShaderKind};
use wgpu::naga;

/// The line before the game's code in every composed shader.
const GAME_MARKER: &str = "\n// ---- the game's ";
/// The line after it.
const ENGINE_MARKER: &str = "\n// ---- the engine's ";

/// Where the game's code sits in a composed shader: byte range.
fn game_range(composed: &str) -> Option<std::ops::Range<usize>> {
    let marker = composed.find(GAME_MARKER)?;
    // The game's code starts on the line after the marker.
    let start = marker + 1 + composed[marker + 1..].find('\n')? + 1;
    let end = composed[start..]
        .find(ENGINE_MARKER)
        .map_or(composed.len(), |at| start + at);
    Some(start..end)
}

/// Checks composed shader `composed` made from the game's `name`. On an
/// error: where it is in the game's code, and why.
pub(crate) fn diagnose(
    name: &str,
    kind: ShaderKind,
    variant: &str,
    composed: &str,
) -> Result<(), Box<ShaderDiagnostic>> {
    let rules = match kind {
        ShaderKind::Material => "material",
        ShaderKind::Sprite => "sprite",
        ShaderKind::Sky => "sky",
        _ => "",
    };
    if let Some(game) = game_range(composed)
        && let Some((range, message)) = super::shader_rules::check(rules, &composed[game.clone()])
    {
        let range = range.start + game.start..range.end + game.start;
        return Err(build(
            name,
            kind,
            variant,
            composed,
            message,
            Some((
                range,
                if kind == ShaderKind::Sprite {
                    "sprite shader contract"
                } else if kind == ShaderKind::Sky {
                    "sky shader contract"
                } else {
                    "material contract"
                }
                .into(),
            )),
            Vec::new(),
        ));
    }
    let module = match naga::front::wgsl::parse_str(composed) {
        Ok(module) => module,
        Err(e) => {
            let spans: Vec<(std::ops::Range<usize>, String)> = e
                .labels()
                .filter_map(|(span, label)| Some((span.to_range()?, label.to_string())))
                .collect();
            // A parse error's first label is its main one.
            let mut notes: Vec<String> = e.notes().map(str::to_string).collect();
            notes.extend(spans.iter().skip(1).filter(|s| !s.1.is_empty()).map(|s| s.1.clone()));
            let primary = spans.first().cloned();
            return Err(build(
                name,
                kind,
                variant,
                composed,
                e.message().to_string(),
                primary,
                notes,
            ));
        }
    };
    // Sprite shaders run on every GPU the engine supports, WebGL2
    // included: check them against what all of them have, so a shader
    // that would fail in a browser fails on the developer's machine too.
    let capabilities = if kind == ShaderKind::Sprite {
        naga::valid::Capabilities::empty()
    } else {
        naga::valid::Capabilities::all()
    };
    let result = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities).validate(&module);
    if let Err(e) = result {
        // The error, then what caused it, from the outside in.
        let mut message = e.as_inner().to_string();
        let mut source = std::error::Error::source(e.as_inner());
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        let spans: Vec<(std::ops::Range<usize>, String)> = e
            .spans()
            .filter_map(|(span, label)| Some((span.to_range()?, label.clone())))
            .collect();
        // Validation adds the narrowest span first and wraps it in wider
        // ones (the function, ...): point at the narrowest.
        let primary = spans.iter().min_by_key(|(range, _)| range.len()).cloned();
        let notes = spans
            .iter()
            .filter(|s| Some(&s.0) != primary.as_ref().map(|p| &p.0) && !s.1.is_empty())
            .map(|s| s.1.clone())
            .collect();
        return Err(build(name, kind, variant, composed, message, primary, notes));
    }
    Ok(())
}

/// WGSL tokens with byte spans, without comments (the shader rules' tokens).
#[cfg(feature = "3d")]
pub(crate) use super::shader_rules::tokens as contract_tokens;

/// An unsupported material/storage combination, located in the user hook.
/// Comments do not count as reads of the engine's `items` buffer.
#[cfg(feature = "3d")]
pub(crate) fn require_material_storage(name: &str, variant: &str, user: &str) -> Result<(), Box<ShaderDiagnostic>> {
    let Some((_, span)) = contract_tokens(user).into_iter().find(|t| t.0 == "items") else {
        return Ok(());
    };
    let composed = crate::render::compose_shader(user);
    let game = game_range(&composed).expect("material source marker");
    Err(build(
        name, ShaderKind::Material, variant, &composed,
        "material hooks `vertex`/`surface` may read `items` only with vertex storage buffers; this GPU has none; use create_instance_batch or Model entities with a shader that does not read items, or select a storage-capable GPU".into(),
        Some((span.start + game.start..span.end + game.start, "unsupported instance storage read".into())), Vec::new(),
    ))
}

/// A diagnostic for an error that isn't the compiler's (pipeline creation),
/// with no place in the source.
pub(crate) fn without_location(name: &str, kind: ShaderKind, variant: &str, message: String) -> Box<ShaderDiagnostic> {
    Box::new(ShaderDiagnostic {
        shader: name.to_string(),
        kind,
        variant: variant.to_string(),
        line: None,
        column: None,
        in_engine_code: false,
        message,
        label: String::new(),
        notes: Vec::new(),
        source_line: None,
    })
}

fn build(
    name: &str,
    kind: ShaderKind,
    variant: &str,
    composed: &str,
    message: String,
    primary: Option<(std::ops::Range<usize>, String)>,
    notes: Vec<String>,
) -> Box<ShaderDiagnostic> {
    let mut diagnostic = without_location(name, kind, variant, message);
    diagnostic.notes = notes;
    let Some((range, label)) = primary else {
        return diagnostic;
    };
    diagnostic.label = label;
    let at = range.start.min(composed.len());
    let line_start = composed[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = composed[at..].find('\n').map_or(composed.len(), |i| at + i);
    let column = composed[line_start..at].chars().count() as u32 + 1;
    diagnostic.source_line = Some(composed[line_start..line_end].trim_end().to_string());
    match game_range(composed) {
        Some(game) if game.contains(&at) || (at == game.end && !game.is_empty()) => {
            let line = composed[game.start..line_start].matches('\n').count() as u32 + 1;
            diagnostic.line = Some(line);
            diagnostic.column = Some(column);
        }
        _ => {
            // In the engine's code: often the game's code broke what came
            // after it (an unclosed brace), or a hook has the wrong type.
            diagnostic.in_engine_code = true;
            diagnostic.line = Some(composed[..line_start].matches('\n').count() as u32 + 1);
            diagnostic.column = Some(column);
        }
    }
    diagnostic
}

#[cfg(test)]
mod cook_rules {
    /// `ponko cook` checks shader files with the engine's rule table: its copy
    /// (`crates/ponko_cli/src/cook/shaders/rules.rs`) is `shader_rules.rs`.
    /// `PONKO_UPDATE_COOK_SHADERS=1` writes it again.
    #[test]
    fn the_cooks_shader_rules_are_the_engines() {
        let engine = include_str!("shader_rules.rs");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/ponko_cli/src/cook/shaders/rules.rs");
        if std::env::var_os("PONKO_UPDATE_COOK_SHADERS").is_some() {
            std::fs::write(&path, engine).unwrap();
        }
        assert!(
            std::fs::read_to_string(&path).ok().as_deref() == Some(engine),
            "ponko cook's copy of the shader rules is out of date: run \
             PONKO_UPDATE_COOK_SHADERS=1 cargo test --lib the_cooks_shader_rules_are_the_engines"
        );
    }
}

#[cfg(all(test, feature = "3d"))]
mod tests {
    use super::*;
    use crate::render::{compose_compute, compose_effect, compose_shader, compose_sky};

    #[test]
    fn parse_errors_point_at_the_games_line_and_column() {
        let user = "fn surface(s: Surface) -> Surface {\n    var out = s;\n    out.albedo = vec3<f32>(1.0, 0.0, 0.0)\n    return out;\n}";
        let d = diagnose("red.wgsl", ShaderKind::Material, "material", &compose_shader(user)).unwrap_err();
        assert_eq!(d.shader, "red.wgsl");
        assert!(!d.in_engine_code, "{d}");
        // The missing `;`: naga points at what follows it.
        assert!(matches!(d.line, Some(3 | 4)), "{d}");
        assert!(d.column.is_some(), "{d}");
        assert!(!d.message.is_empty());
        assert!(
            d.to_string().starts_with(&format!("red.wgsl:{}:", d.line.unwrap())),
            "{d}"
        );
    }

    #[test]
    fn validation_errors_point_at_the_games_line_with_the_cause() {
        let user = "// a comment\n\nfn surface(s: Surface) -> Surface {\n    return 1.0;\n}";
        let d = diagnose("water.wgsl", ShaderKind::Material, "material", &compose_shader(user)).unwrap_err();
        assert!(!d.in_engine_code, "{d}");
        assert_eq!(d.line, Some(4), "{d}");
        assert_eq!(d.column, Some(12), "{d}");
        assert_eq!(d.source_line.as_deref(), Some("    return 1.0;"));
        assert!(d.message.contains("Surface"), "{d}");
        assert!(
            d.to_string().contains("\n        return 1.0;\n               ^ "),
            "{d}"
        );
    }

    #[test]
    fn validator_errors_have_their_causes() {
        // Parses, but the validator rejects it: a function that never returns.
        let user = "fn helper() -> f32 {\n}\nfn surface(s: Surface) -> Surface { return s; }";
        let d = diagnose("v.wgsl", ShaderKind::Material, "material", &compose_shader(user)).unwrap_err();
        assert!(!d.message.is_empty(), "{d}");
        assert!(d.message.contains(": "), "the cause follows: {d}");
        assert!(d.line.is_some(), "{d}");
    }

    #[test]
    fn effects_and_compute_passes_map_to_their_own_lines() {
        let effect = "fn effect(p: Pixel) -> vec4<f32> {\n    return p.color + nope;\n}";
        let d = diagnose("fx.wgsl", ShaderKind::Effect, "effect", &compose_effect(effect)).unwrap_err();
        assert_eq!((d.line, d.in_engine_code), (Some(2), false), "{d}");
        let compute = "\nfn compute(c: Cell) -> vec4<f32> {\n    return undefined_thing;\n}";
        let d = diagnose("sim.wgsl", ShaderKind::Compute, "compute", &compose_compute(compute)).unwrap_err();
        assert_eq!((d.line, d.in_engine_code), (Some(3), false), "{d}");
    }

    #[test]
    fn a_sky_shader_needs_its_sky_hook_with_the_right_signature() {
        let sky = |user: &str| diagnose("sky.wgsl", ShaderKind::Sky, "sky", &compose_sky(user));
        sky("fn sky(dir: vec3<f32>) -> vec3<f32> { return frame.sky_params[0].rgb * max(dir.y, 0.0); }").unwrap();
        sky("fn sky(d: vec3<f32>,) -> vec3<f32> { return d; }").unwrap();
        let missing = sky("fn sky_colour(d: vec3<f32>) -> vec3<f32> { return d; }").unwrap_err();
        assert!(missing.message.contains("needs a `sky` hook"), "{missing}");
        let wrong = sky("fn sky(d: vec3<f32>) -> vec4<f32> { return vec4<f32>(d, 1.0); }").unwrap_err();
        assert!(wrong.message.contains("wrong signature"), "{wrong}");
        assert_eq!(wrong.line, Some(1), "{wrong}");
        let bound =
            sky("@group(0) @binding(9) var t: texture_2d<f32>;\nfn sky(d: vec3<f32>) -> vec3<f32> { return d; }")
                .unwrap_err();
        assert!(bound.message.contains("bindings"), "{bound}");
        let broken = sky("fn sky(d: vec3<f32>) -> vec3<f32> {\n    return 1.0;\n}").unwrap_err();
        assert!(!broken.in_engine_code, "{broken}");
        assert_eq!(broken.line, Some(2), "{broken}");
    }

    #[test]
    fn valid_shaders_have_no_diagnostic() {
        diagnose("ok", ShaderKind::Material, "material", &compose_shader("")).unwrap();
    }

    #[test]
    fn the_game_range_is_exactly_the_games_code() {
        let user = "fn surface(s: Surface) -> Surface { return s; }";
        let composed = compose_shader(user);
        let range = game_range(&composed).unwrap();
        assert_eq!(&composed[range], user);
    }
}
