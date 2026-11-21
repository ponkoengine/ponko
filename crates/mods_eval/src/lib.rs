//! Throwaway prototype for issue 70: the "frost drip" enemy's crawl run as a
//! WASM module (wasmtime, wasmi) and as a script (rhai, Lua), with the
//! engine functions it needs (two ray casts) as host functions. Not shipped.

/// The engine side the mod calls: is there a wall `8` px ahead, a floor `7` px ahead?
/// A fixed corridor (walls at 0 and 100, a gap at 40..44) stands in for physics.
pub fn wall_ahead(x: f32, heading: f32) -> bool {
    let ahead = x + heading * 8.0;
    ahead <= 0.0 || ahead >= 100.0
}

pub fn floor_ahead(x: f32, heading: f32) -> bool {
    let ahead = x + heading * 7.0;
    !(40.0..44.0).contains(&ahead)
}

/// The same crawl in Rust: the baseline.
pub fn native(x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
    let mut heading = heading;
    if wall_ahead(x, heading) || !floor_ahead(x, heading) {
        heading = -heading;
    }
    (x + heading * speed * dt, heading)
}

/// A mod runtime with the drip's `update` loaded.
pub trait Runtime {
    fn name(&self) -> &'static str;
    fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32);
    /// Runs the hostile mod's endless loop; `Err` with how it was stopped.
    fn spin(&mut self) -> Result<(), String>;
    /// Runs the hostile mod's endless allocation; `Err` with how it was stopped.
    fn grow(&mut self) -> Result<(), String>;
}

pub struct Native;

impl Runtime for Native {
    fn name(&self) -> &'static str {
        "native Rust"
    }
    fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
        native(x, heading, speed, dt)
    }
    fn spin(&mut self) -> Result<(), String> {
        Err("not applicable".into())
    }
    fn grow(&mut self) -> Result<(), String> {
        Err("not applicable".into())
    }
}

pub const DRIP_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/drip.wasm"));
pub const HOSTILE_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hostile.wasm"));
/// Instructions a mod may run per call before it is stopped.
pub const FUEL: u64 = 10_000_000;
/// Memory a mod may hold.
pub const MEMORY: usize = 16 << 20;

#[cfg(feature = "wasmtime")]
pub mod wt {
    use wasmtime::*;

    pub struct Data {
        limits: StoreLimits,
    }

    pub struct Wasmtime {
        store: Store<Data>,
        update: TypedFunc<(f32, f32, f32, f32), (f32, f32)>,
        spin: TypedFunc<(), ()>,
        grow: TypedFunc<(), i32>,
    }

    impl Wasmtime {
        pub fn new() -> Self {
            let mut config = Config::new();
            config.consume_fuel(true);
            config.cranelift_nan_canonicalization(true);
            let engine = Engine::new(&config).unwrap();
            let mut store = Store::new(
                &engine,
                Data {
                    limits: StoreLimitsBuilder::new().memory_size(super::MEMORY).build(),
                },
            );
            store.limiter(|d| &mut d.limits);
            let mut linker = Linker::new(&engine);
            linker
                .func_wrap("ponko", "wall_ahead", |x: f32, h: f32| super::wall_ahead(x, h) as i32)
                .unwrap();
            linker
                .func_wrap("ponko", "floor_ahead", |x: f32, h: f32| super::floor_ahead(x, h) as i32)
                .unwrap();
            let drip = Module::new(&engine, super::DRIP_WASM).unwrap();
            let hostile = Module::new(&engine, super::HOSTILE_WASM).unwrap();
            let a = linker.instantiate(&mut store, &drip).unwrap();
            let b = linker.instantiate(&mut store, &hostile).unwrap();
            Self {
                update: a.get_typed_func(&mut store, "update").unwrap(),
                spin: b.get_typed_func(&mut store, "spin").unwrap(),
                grow: b.get_typed_func(&mut store, "grow").unwrap(),
                store,
            }
        }
    }

    impl super::Runtime for Wasmtime {
        fn name(&self) -> &'static str {
            "wasmtime 51 (Cranelift JIT)"
        }
        fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
            self.store.set_fuel(super::FUEL).unwrap();
            self.update.call(&mut self.store, (x, heading, speed, dt)).unwrap()
        }
        fn spin(&mut self) -> Result<(), String> {
            self.store.set_fuel(super::FUEL).unwrap();
            self.spin.call(&mut self.store, ()).map_err(|e| format!("{e:?}"))
        }
        fn grow(&mut self) -> Result<(), String> {
            self.store.set_fuel(super::FUEL).unwrap();
            let pages = self.grow.call(&mut self.store, ()).map_err(|e| format!("{e:?}"))?;
            Err(format!("memory.grow refused at {pages} pages (64 KiB each)"))
        }
    }
}

#[cfg(feature = "wasmi")]
pub mod wi {
    use wasmi::*;

    pub struct Data {
        limits: StoreLimits,
    }

    pub struct Wasmi {
        store: Store<Data>,
        update: TypedFunc<(f32, f32, f32, f32), (f32, f32)>,
        spin: TypedFunc<(), ()>,
        grow: TypedFunc<(), i32>,
    }

    impl Wasmi {
        pub fn new() -> Self {
            let mut config = Config::default();
            config.consume_fuel(true);
            let engine = Engine::new(&config);
            let mut store = Store::new(
                &engine,
                Data {
                    limits: StoreLimitsBuilder::new().memory_size(super::MEMORY).build(),
                },
            );
            store.limiter(|d| &mut d.limits);
            let mut linker = <Linker<Data>>::new(&engine);
            linker
                .func_wrap("ponko", "wall_ahead", |x: f32, h: f32| super::wall_ahead(x, h) as i32)
                .unwrap();
            linker
                .func_wrap("ponko", "floor_ahead", |x: f32, h: f32| super::floor_ahead(x, h) as i32)
                .unwrap();
            let drip = Module::new(&engine, super::DRIP_WASM).unwrap();
            let hostile = Module::new(&engine, super::HOSTILE_WASM).unwrap();
            let a = linker.instantiate_and_start(&mut store, &drip).unwrap();
            let b = linker.instantiate_and_start(&mut store, &hostile).unwrap();
            Self {
                update: a.get_typed_func(&store, "update").unwrap(),
                spin: b.get_typed_func(&store, "spin").unwrap(),
                grow: b.get_typed_func(&store, "grow").unwrap(),
                store,
            }
        }
    }

    impl super::Runtime for Wasmi {
        fn name(&self) -> &'static str {
            "wasmi 2 (interpreter)"
        }
        fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
            self.store.set_fuel(super::FUEL).unwrap();
            self.update.call(&mut self.store, (x, heading, speed, dt)).unwrap()
        }
        fn spin(&mut self) -> Result<(), String> {
            self.store.set_fuel(super::FUEL).unwrap();
            self.spin.call(&mut self.store, ()).map_err(|e| format!("{e}"))
        }
        fn grow(&mut self) -> Result<(), String> {
            self.store.set_fuel(super::FUEL).unwrap();
            let pages = self.grow.call(&mut self.store, ()).map_err(|e| format!("{e}"))?;
            Err(format!("memory.grow refused at {pages} pages (64 KiB each)"))
        }
    }
}

#[cfg(feature = "rhai")]
pub mod rh {
    pub struct Rhai {
        engine: rhai::Engine,
        ast: rhai::AST,
        hostile: rhai::AST,
        scope: rhai::Scope<'static>,
    }

    impl Rhai {
        pub fn new() -> Self {
            let mut engine = rhai::Engine::new();
            engine.register_fn("wall_ahead", |x: f64, h: f64| super::wall_ahead(x as f32, h as f32));
            engine.register_fn("floor_ahead", |x: f64, h: f64| super::floor_ahead(x as f32, h as f32));
            engine.set_max_operations(super::FUEL);
            engine.set_max_array_size(1 << 16);
            engine.set_max_string_size(1 << 20);
            let ast = engine.compile(include_str!("drip.rhai")).unwrap();
            let hostile = engine
                .compile("fn spin() { loop {} } fn grow() { let a = []; loop { a.push(a.len()); } }")
                .unwrap();
            Self {
                engine,
                ast,
                hostile,
                scope: rhai::Scope::new(),
            }
        }
    }

    impl super::Runtime for Rhai {
        fn name(&self) -> &'static str {
            "rhai 1.27 (AST interpreter)"
        }
        fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
            let out: rhai::Array = self
                .engine
                .call_fn_with_options(
                    rhai::CallFnOptions::new().eval_ast(false).rewind_scope(true),
                    &mut self.scope,
                    &self.ast,
                    "update",
                    (x as f64, heading as f64, speed as f64, dt as f64),
                )
                .unwrap();
            (out[0].as_float().unwrap() as f32, out[1].as_float().unwrap() as f32)
        }
        fn spin(&mut self) -> Result<(), String> {
            self.engine
                .call_fn::<()>(&mut self.scope, &self.hostile, "spin", ())
                .map_err(|e| e.to_string())
        }
        fn grow(&mut self) -> Result<(), String> {
            self.engine
                .call_fn::<()>(&mut self.scope, &self.hostile, "grow", ())
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(feature = "lua")]
pub mod lu {
    use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, VmState};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct LuaRt {
        _lua: Lua,
        update: Function,
        spin: Function,
        grow: Function,
        budget: Arc<AtomicU64>,
    }

    impl LuaRt {
        pub fn new() -> Self {
            // Only the libraries a sandboxed mod may have: no io, os, package, debug.
            let lua = Lua::new_with(StdLib::MATH | StdLib::STRING | StdLib::TABLE, LuaOptions::default()).unwrap();
            lua.set_memory_limit(super::MEMORY).unwrap();
            let budget = Arc::new(AtomicU64::new(0));
            let b = budget.clone();
            // Every 1000 instructions: stop the mod once its budget is spent.
            lua.set_hook(HookTriggers::new().every_nth_instruction(1000), move |_, _| {
                if b.fetch_sub(1000.min(b.load(Ordering::Relaxed)), Ordering::Relaxed) <= 1000 {
                    return Err(mlua::Error::runtime("instruction budget spent"));
                }
                Ok(VmState::Continue)
            })
            .unwrap();
            let g = lua.globals();
            g.set(
                "wall_ahead",
                lua.create_function(|_, (x, h): (f32, f32)| Ok(super::wall_ahead(x, h)))
                    .unwrap(),
            )
            .unwrap();
            g.set(
                "floor_ahead",
                lua.create_function(|_, (x, h): (f32, f32)| Ok(super::floor_ahead(x, h)))
                    .unwrap(),
            )
            .unwrap();
            lua.load(include_str!("drip.lua")).exec().unwrap();
            lua.load("function spin() while true do end end function grow() local t = {} while true do t[#t + 1] = string.rep('x', 64) .. #t end end")
                .exec()
                .unwrap();
            Self {
                update: g.get("update").unwrap(),
                spin: g.get("spin").unwrap(),
                grow: g.get("grow").unwrap(),
                budget,
                _lua: lua,
            }
        }
    }

    impl super::Runtime for LuaRt {
        fn name(&self) -> &'static str {
            "Lua 5.4 via mlua 0.12 (interpreter)"
        }
        fn update(&mut self, x: f32, heading: f32, speed: f32, dt: f32) -> (f32, f32) {
            self.budget.store(super::FUEL, Ordering::Relaxed);
            self.update.call((x, heading, speed, dt)).unwrap()
        }
        fn spin(&mut self) -> Result<(), String> {
            self.budget.store(super::FUEL, Ordering::Relaxed);
            self.spin
                .call::<()>(())
                .map_err(|e| e.to_string().lines().next().unwrap_or("").to_string())
        }
        fn grow(&mut self) -> Result<(), String> {
            self.budget.store(u64::MAX, Ordering::Relaxed);
            self.grow
                .call::<()>(())
                .map_err(|e| e.to_string().lines().next().unwrap_or("").to_string())
        }
    }
}

/// Every runtime this build has.
pub fn runtimes() -> Vec<Box<dyn Runtime>> {
    #[allow(unused_mut)]
    let mut out: Vec<Box<dyn Runtime>> = vec![Box::new(Native)];
    #[cfg(feature = "wasmtime")]
    out.push(Box::new(wt::Wasmtime::new()));
    #[cfg(feature = "wasmi")]
    out.push(Box::new(wi::Wasmi::new()));
    #[cfg(feature = "rhai")]
    out.push(Box::new(rh::Rhai::new()));
    #[cfg(feature = "lua")]
    out.push(Box::new(lu::LuaRt::new()));
    out
}
