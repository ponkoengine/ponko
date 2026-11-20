//! Methods a game and its plugins add to the control endpoint
//! ([`Game::control_method`](crate::game::Game::control_method)), and the
//! names of the plugins it was built with, as the endpoint reports them.

use serde_json::{Value, json};

use super::METHODS;
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};

/// A game method's handler: the params in, the answer or an error out.
pub(crate) type MethodFn = Box<dyn FnMut(&mut Ctx, &Value) -> Result<Value, Error>>;

/// One added method.
struct GameMethod {
    name: String,
    about: String,
    /// Taken out while it runs, so it can have the whole `Ctx`.
    run: Option<MethodFn>,
}

/// What a game added to the endpoint, moved into `Ctx` when it starts.
#[derive(Default)]
pub(crate) struct GameExtensions {
    methods: Vec<GameMethod>,
    /// The plugins the game was built with, by name, in the order added.
    pub(crate) plugins: Vec<String>,
}

impl GameExtensions {
    /// Why `name` can't be a game method, if it can't: the shape
    /// `namespace.method` (lowercase ASCII letters, digits and `_`), a
    /// namespace of its own (not one of the engine's) and not taken yet.
    pub(crate) fn check_name(&self, name: &str) -> Result<(), Error> {
        let invalid = |why: String| Err(Error::new(ErrorKind::Invalid, why));
        let well_formed = name.contains('.')
            && name.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            });
        if !well_formed {
            return invalid(format!(
                "control method `{name}`: a name is `namespace.method`, in lowercase letters, digits and `_` (like `weather.set_rain`)"
            ));
        }
        let namespace = name.split('.').next().unwrap_or_default();
        if namespace == "ponko" || METHODS.iter().any(|(m, _)| m.split('.').next() == Some(namespace)) {
            return invalid(format!(
                "control method `{name}`: `{namespace}` is one of the engine's namespaces; name yours after your game or crate"
            ));
        }
        if self.methods.iter().any(|m| m.name == name) {
            return invalid(format!(
                "control method `{name}` was added twice; each method is added once"
            ));
        }
        Ok(())
    }

    /// Adds a method whose name passed [`check_name`](Self::check_name).
    pub(crate) fn add(&mut self, name: String, about: String, run: MethodFn) {
        self.methods.push(GameMethod {
            name,
            about,
            run: Some(run),
        });
    }

    /// The added methods for `methods` and `capabilities`, in the order added.
    pub(crate) fn listed(&self) -> impl Iterator<Item = Value> + '_ {
        self.methods
            .iter()
            .map(|m| json!({ "name": m.name, "about": m.about, "added": true }))
    }
}

/// Runs game method `method`, or says it is unknown.
pub(crate) fn call(ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, String> {
    let Some(i) = ctx.extensions.methods.iter().position(|m| m.name == method) else {
        return Err(format!("unknown method `{method}` (call `methods` for the list)"));
    };
    let Some(mut run) = ctx.extensions.methods[i].run.take() else {
        return Err(format!("`{method}` is already running"));
    };
    let result = run(ctx, params);
    // The list doesn't change once the game runs, so `i` is still this method.
    ctx.extensions.methods[i].run = Some(run);
    result.map_err(|e| e.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &mut Ctx, _: &Value) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    #[test]
    fn method_names_need_a_namespace_of_their_own() {
        let mut added = GameExtensions::default();
        for good in ["weather.set_rain", "acme_ai.paths.show", "w2.x"] {
            assert!(added.check_name(good).is_ok(), "{good}");
        }
        for bad in [
            "rain",
            "Weather.rain",
            "weather.",
            ".rain",
            "weather..rain",
            "weather.set-rain",
            "",
        ] {
            let e = added.check_name(bad).unwrap_err();
            assert!(e.message().contains("namespace.method"), "{bad}: {e}");
        }
        for engine in [
            "entity.blink",
            "debug.mine",
            "ponko.x",
            "methods.extra",
            "screenshot.now",
        ] {
            let e = added.check_name(engine).unwrap_err();
            assert!(e.message().contains("engine's namespaces"), "{engine}: {e}");
        }
        added.add("weather.set_rain".into(), "rain".into(), Box::new(none));
        let e = added.check_name("weather.set_rain").unwrap_err();
        assert!(e.message().contains("added twice"), "{e}");
    }

    #[test]
    fn a_game_method_gets_the_ctx_and_its_params() {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.extensions.add(
            "counter.add".into(),
            "adds `by` to the counter".into(),
            Box::new(|ctx, params| {
                let by = params["by"]
                    .as_u64()
                    .ok_or_else(|| Error::new(ErrorKind::Invalid, "`by` must be a whole number"))?;
                ctx.world.insert_resource(by);
                Ok(json!({ "counter": by }))
            }),
        );
        assert_eq!(
            call(&mut ctx, "counter.add", &json!({ "by": 3 })).unwrap(),
            json!({ "counter": 3 })
        );
        assert_eq!(*ctx.world.resource::<u64>(), 3);
        assert_eq!(
            call(&mut ctx, "counter.add", &json!({})).unwrap_err(),
            "`by` must be a whole number"
        );
        assert!(
            call(&mut ctx, "counter.nope", &json!({}))
                .unwrap_err()
                .contains("unknown method")
        );
    }
}
