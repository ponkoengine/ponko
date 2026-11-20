//! The control endpoint's `debug.draw` method: turn debug drawing and the
//! built-in views on and off while the game runs, and see what is drawn.

use serde_json::{Value, json};

use super::DebugView;
use crate::ctx::Ctx;

/// `debug.draw`: params `on?` (bool: debug drawing on or off), `views?`
/// (an object of view name to bool), `toggles?` (an object of the game's
/// debug toggle names to bool), `clear?` (true drops everything drawn).
/// Answers the state after the change.
pub(crate) fn handle(ctx: &mut Ctx, params: &Value) -> Result<Value, String> {
    let on = match params.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or("`on` must be true or false")?),
    };
    let mut views = Vec::new();
    match params.get("views") {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => {
            for (name, v) in map {
                let view = DebugView::from_name(name).ok_or_else(|| {
                    let names: Vec<_> = DebugView::ALL.iter().map(|v| v.name()).collect();
                    format!("unknown view `{name}` (views: {})", names.join(", "))
                })?;
                views.push((view, v.as_bool().ok_or("each view takes true or false")?));
            }
        }
        Some(_) => return Err("`views` must be an object of view names to true or false".into()),
    }
    let mut toggles = Vec::new();
    match params.get("toggles") {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => {
            for (name, v) in map {
                if !ctx.debug.toggles().any(|(n, _)| n == name) {
                    return Err(format!(
                        "unknown debug toggle `{name}` (toggles: {})",
                        ctx.debug.toggle_names()
                    ));
                }
                toggles.push((name.as_str(), v.as_bool().ok_or("each toggle takes true or false")?));
            }
        }
        Some(_) => return Err("`toggles` must be an object of toggle names to true or false".into()),
    }
    let clear = match params.get("clear") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("`clear` must be true or false")?,
    };
    // Checked everything first: a bad parameter changes nothing.
    if let Some(on) = on {
        ctx.debug.set_enabled(on);
    }
    for (view, on) in views {
        ctx.debug.show(view, on);
    }
    for (name, on) in toggles {
        // Checked above: every name is a toggle.
        let _ = ctx.debug.set_toggle(name, on);
    }
    if clear {
        ctx.debug.clear();
    }
    Ok(state(ctx))
}

fn state(ctx: &Ctx) -> Value {
    let views: serde_json::Map<String, Value> = DebugView::ALL
        .iter()
        .map(|v| (v.name().to_string(), json!(ctx.debug.showing(*v))))
        .collect();
    let toggles: serde_json::Map<String, Value> = ctx
        .debug
        .toggles()
        .map(|(name, on)| (name.to_string(), json!(on)))
        .collect();
    json!({
        "on": ctx.debug.enabled(),
        "views": views,
        "toggles": toggles,
        "drawn": ctx.debug.stats(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx::new(1.0 / 60.0, glam::Vec2::ONE)
    }

    #[test]
    fn debug_draw_turns_drawing_and_views_on_and_reports_them() {
        let mut ctx = ctx();
        let answer = handle(
            &mut ctx,
            &json!({ "on": true, "views": { "colliders": true, "lights": true } }),
        )
        .unwrap();
        assert_eq!(answer["on"], true);
        assert_eq!(answer["views"]["colliders"], true);
        assert_eq!(answer["views"]["navmesh"], false);
        assert!(ctx.debug.enabled() && ctx.debug.showing(DebugView::Lights));
        ctx.debug
            .line(glam::Vec3::ZERO, glam::Vec3::X, crate::color::Color::RED);
        let answer = handle(&mut ctx, &json!({})).unwrap();
        assert_eq!(answer["drawn"]["lines_3d"], 1, "{answer}");
        let answer = handle(&mut ctx, &json!({ "clear": true, "views": { "lights": false } })).unwrap();
        assert_eq!(answer["drawn"]["lines_3d"], 0);
        assert_eq!(answer["views"]["lights"], false);
        let answer = handle(&mut ctx, &json!({ "on": false })).unwrap();
        assert_eq!(answer["on"], false);
    }

    #[test]
    fn debug_draw_turns_the_games_toggles_on_and_off() {
        let mut ctx = ctx();
        ctx.debug.set_enabled(true);
        ctx.debug
            .set_toggles(&[("ai.paths".into(), false), ("ai.goals".into(), true)]);
        let answer = handle(&mut ctx, &json!({ "toggles": { "ai.paths": true } })).unwrap();
        assert_eq!(answer["toggles"], json!({ "ai.paths": true, "ai.goals": true }));
        assert!(ctx.debug.toggled("ai.paths"));
        let answer = handle(&mut ctx, &json!({ "toggles": { "ai.goals": false } })).unwrap();
        assert_eq!(answer["toggles"]["ai.goals"], false);
        let unknown = handle(&mut ctx, &json!({ "toggles": { "ai.nope": true } })).unwrap_err();
        assert!(unknown.contains("ai.paths, ai.goals"), "{unknown}");
        // A bad toggle changes nothing else in the same call.
        assert!(handle(&mut ctx, &json!({ "on": false, "toggles": { "ai.paths": 1 } })).is_err());
        assert!(ctx.debug.enabled() && ctx.debug.toggled("ai.paths"));
    }

    #[test]
    fn a_bad_debug_draw_parameter_changes_nothing() {
        let mut ctx = ctx();
        ctx.debug.set_enabled(false);
        for bad in [
            json!({ "on": true, "views": { "nope": true } }),
            json!({ "on": true, "views": { "bounds": 1 } }),
            json!({ "on": true, "views": [] }),
            json!({ "on": "yes" }),
            json!({ "on": true, "clear": 2 }),
        ] {
            assert!(handle(&mut ctx, &bad).is_err(), "{bad}");
            assert!(!ctx.debug.enabled(), "{bad}");
        }
        let unknown = handle(&mut ctx, &json!({ "views": { "nope": true } })).unwrap_err();
        assert!(
            unknown.contains("colliders") && unknown.contains("hierarchy"),
            "{unknown}"
        );
    }
}
