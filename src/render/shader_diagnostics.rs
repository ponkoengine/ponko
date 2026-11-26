//! Turns naga's errors about a composed shader (the engine's prelude, the
//! game's code, the engine's entry points) into a [`ShaderDiagnostic`] that
//! points at the line in the game's own WGSL.

use crate::render::{ShaderDiagnostic, ShaderKind};
use wgpu::naga;

mod names;

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
        // What is wrong, by the names in the source: naga's own text names
        // IR handles (`Function [4]`, `Some([10])`) instead.
        let names = names::Names::new(&module, composed, e.as_inner());
        let message = names.message(e.as_inner());
        let spans: Vec<(std::ops::Range<usize>, String)> = e
            .spans()
            .filter_map(|(span, label)| {
                let label = names.label(label).unwrap_or_else(|| label.clone());
                Some((span.to_range()?, label))
            })
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

/// Validation errors name types, functions and expressions as the source
/// does, not by naga's IR handles (`Function [4]`, `Some([10])`).
#[cfg(test)]
mod names_in_messages {
    use super::*;

    /// The diagnostic of sprite shader `user` (the game's code).
    fn sprite(user: &str) -> Box<ShaderDiagnostic> {
        let composed = crate::sprite_material::compose_sprite_shader(user);
        diagnose("glow.wgsl", ShaderKind::Sprite, "sprite", &composed).unwrap_err()
    }

    /// The diagnostic of a whole WGSL module (no engine code around it).
    fn module(wgsl: &str) -> Box<ShaderDiagnostic> {
        diagnose("m.wgsl", ShaderKind::Compute, "compute", wgsl).unwrap_err()
    }

    /// No IR handle is left anywhere in the diagnostic.
    #[track_caller]
    fn no_handles(d: &ShaderDiagnostic) {
        let text = d.to_string();
        assert!(!text.contains("naga::ir"), "{text}");
        assert!(!text.contains("Some(["), "{text}");
        // naga writes a handle as ` [4]`; code like `a[0]` has no space.
        assert!(
            !text.split(" [").skip(1).any(|s| s
                .split(']')
                .next()
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))),
            "a handle like `[4]` is left: {text}"
        );
    }

    #[test]
    fn a_return_type_mismatch_names_both_types() {
        let d = sprite(
            "fn fragment(p: SpritePixel) -> vec4<f32> {\n    let glow = material.params[0].x;\n    return mix(p.color.rgb, vec3<f32>(1.0), glow);\n}",
        );
        assert_eq!(
            d.message, "`fragment` returns `vec3<f32>` but is declared to return `vec4<f32>`",
            "{d}"
        );
        assert_eq!((d.line, d.column, d.in_engine_code), (Some(3), Some(12), false), "{d}");
        assert_eq!(
            d.label, "`mix(p.color.rgb, vec3<f32>(1.0), glow)` is `vec3<f32>`",
            "{d}"
        );
        assert_eq!(d.notes, ["in `fn fragment`"], "{d}");
        assert!(
            d.to_string().starts_with(
                "glow.wgsl:3:12: error: `fragment` returns `vec3<f32>` but is declared to return `vec4<f32>`\n        return mix("
            ),
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn a_struct_returned_for_a_vector_is_named() {
        let d = sprite(
            "struct Glow { amount: f32 }\nfn fragment(p: SpritePixel) -> vec4<f32> {\n    let g = Glow(1.0);\n    return g;\n}",
        );
        assert_eq!(
            d.message, "`fragment` returns `Glow` but is declared to return `vec4<f32>`",
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn a_missing_return_value_says_so() {
        let d = sprite(
            "fn amount() -> f32 {\n}\nfn fragment(p: SpritePixel) -> vec4<f32> {\n    return p.color * amount();\n}",
        );
        assert_eq!(
            d.message, "`amount` is declared to return `f32` but returns without a value",
            "{d}"
        );
        assert_eq!(d.line, Some(1), "{d}");
        assert_eq!(d.label, "in `fn amount`", "{d}");
        no_handles(&d);
    }

    #[test]
    fn an_argument_type_mismatch_names_the_parameter_and_both_types() {
        let d = sprite(
            "fn brighten(c: vec4<f32>, by: f32) -> vec4<f32> { return c * by; }\nfn fragment(p: SpritePixel) -> vec4<f32> {\n    return brighten(p.color.rgb, 2.0);\n}",
        );
        assert_eq!(
            d.message, "argument 1 of `brighten` (`p.color.rgb`) is `vec3<f32>`, but its parameter `c` is `vec4<f32>`",
            "{d}"
        );
        assert_eq!((d.line, d.column), (Some(3), Some(21)), "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_wrong_argument_count_names_the_function() {
        let d = sprite(
            "fn brighten(c: vec4<f32>) -> vec4<f32> { return c * 2.0; }\nfn fragment(p: SpritePixel) -> vec4<f32> {\n    return brighten(p.color, 2.0);\n}",
        );
        assert_eq!(d.message, "`brighten` takes 1 argument, but the call gives 2", "{d}");
        assert_eq!(d.line, Some(3), "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_binary_operation_on_mismatched_vectors_names_both_sides() {
        let d = sprite(
            "fn fragment(p: SpritePixel) -> vec4<f32> {\n    let tinted = p.color.rgb * p.uv;\n    return p.color;\n}",
        );
        assert!(
            d.message.starts_with(
                "`*` can't combine `vec3<f32>` (`p.color.rgb`) and `vec2<f32>` (`p.uv`); the vectors' sizes differ"
            ),
            "{d}"
        );
        assert_eq!((d.line, d.column), (Some(2), Some(18)), "{d}");
        // The operation has no type to show: the message says what is wrong.
        assert_eq!(d.label, "", "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_binary_operation_on_two_number_types_says_to_convert() {
        let d = sprite(
            "fn fragment(p: SpritePixel) -> vec4<f32> {\n    let steps: i32 = 4;\n    return p.color * steps;\n}",
        );
        assert!(
            d.message
                .starts_with("`*` can't combine `vec4<f32>` (`p.color`) and `i32` (`steps`); WGSL doesn't convert"),
            "{d}"
        );
        assert!(d.message.contains("`f32(x)`"), "{d}");
        no_handles(&d);
    }

    #[test]
    fn an_assignment_of_the_wrong_type_names_the_variable() {
        let d =
            sprite("fn fragment(p: SpritePixel) -> vec4<f32> {\n    var c = p.color;\n    c = p.uv;\n    return c;\n}");
        assert_eq!(
            d.message, "can't assign `vec2<f32>` to `c`, which is `vec4<f32>`",
            "{d}"
        );
        assert_eq!(d.line, Some(3), "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_non_bool_condition_names_its_type() {
        let d = sprite(
            "fn fragment(p: SpritePixel) -> vec4<f32> {\n    if (p.uv.x) { return p.color; }\n    return p.color;\n}",
        );
        assert_eq!(
            d.message, "an `if` condition must be `bool`, but `p.uv.x` is `f32`",
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn select_with_two_types_names_both() {
        let d = sprite("fn fragment(p: SpritePixel) -> vec4<f32> {\n    return select(p.color, p.uv, true);\n}");
        assert_eq!(
            d.message,
            "`select(f, t, condition)` needs `f` and `t` of one type, but `f` is `vec4<f32>` and `t` is `vec2<f32>`",
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn a_float_index_names_its_type() {
        let d = sprite(
            "fn fragment(p: SpritePixel) -> vec4<f32> {\n    let a = array<f32, 2>(1.0, 2.0);\n    return vec4<f32>(a[p.uv.x]);\n}",
        );
        assert_eq!(
            d.message, "an index must be `i32` or `u32`, but `p.uv.x` is `f32`",
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn a_write_to_a_read_only_buffer_names_it() {
        let d = module(
            "@group(0) @binding(0) var<storage, read> cells: array<f32>;\n@compute @workgroup_size(1) fn main() {\n    cells[0] = 1.0;\n}",
        );
        assert_eq!(d.message, "`cells[0]` can't be written to: it is read-only", "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_uniform_layout_error_names_the_variable_and_its_type() {
        let d = module(
            "struct Params { weights: array<f32, 4> }\n@group(0) @binding(0) var<uniform> params: Params;\n@compute @workgroup_size(1) fn main() { let w = params.weights[0]; }",
        );
        assert!(
            d.message.starts_with("`params`: `array<f32, 4>` in its type `Params` breaks the uniform address space's layout rules: the array stride 4 is not a multiple of the required alignment 16"),
            "{d}"
        );
        assert!(d.message.contains("array<vec4<f32>, N>"), "{d}");
        assert_eq!(d.label, "`params`", "{d}");
        no_handles(&d);
    }

    #[test]
    fn a_binding_used_twice_names_the_variable() {
        let d = module(
            "@group(0) @binding(0) var<storage> a: array<f32>;\n@group(0) @binding(0) var<storage> b: array<f32>;\n@compute @workgroup_size(1) fn main() { let x = a[0] + b[0]; }",
        );
        assert_eq!(
            d.message, "`b` uses the same @group/@binding as another resource of the @compute entry point `main`",
            "{d}"
        );
        no_handles(&d);
    }

    #[test]
    fn errors_without_a_message_of_their_own_keep_nagas_text_without_handles() {
        // A float literal returned from a function with no return type: naga
        // reports the abstract float, which has no message here.
        let d = module("fn f() {\n    return 1.0;\n}\n@compute @workgroup_size(1) fn main() { f(); }");
        assert_eq!(
            d.message, "in `fn f`: Abstract types may only appear in constant expressions",
            "{d}"
        );
        assert_eq!(d.label, "", "{d}");
        assert_eq!(d.line, Some(2), "{d}");
        no_handles(&d);
    }

    #[test]
    fn handles_come_out_of_any_text() {
        assert_eq!(
            names::stripped("Call to [4] is invalid: Argument 0 value [2] doesn't match the type [0]"),
            "Call to is invalid: Argument 0 value doesn't match the type"
        );
        assert_eq!(
            names::stripped("The `return` expression Some([10]) does not match Some([5])"),
            "The `return` expression does not match"
        );
        assert_eq!(names::stripped("member[2] offset [x] stays"), "member offset [x] stays");
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
