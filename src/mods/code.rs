//! Code mods: a mod's WebAssembly module, run by an interpreter
//! ([wasmi](https://github.com/wasmi-labs/wasmi)) in a sandbox with fuel and
//! memory limits (feature `mods-wasm`).
//!
//! A mod names its module in its manifest (`"code": "code/frost.wasm"`).
//! [`ModSet::mount`](super::ModSet::mount) compiles and starts every mod's
//! module before it mounts anything, and inserts [`ModCode`]; the game then
//! calls the functions the modules export, when and with what it decides,
//! through [`with_code`]. A module reaches only the host functions below,
//! imported from module `"ponko"`:
//!
//! | Import | Type | What it does |
//! |---|---|---|
//! | `log` | `(ptr: i32, len: i32)` | Adds UTF-8 text from the module's exported `memory` to [`ModCode::take_log`] |
//! | `dt` | `() -> f32` | The tick's step in seconds |
//! | `cast_ray` | `(ox, oy, dx, dy, max: f32) -> f32` | A 2D physics ray cast (feature `physics`): the distance to the first collider hit, or -1 |

use std::fmt;

use wasmi::{
    Caller, Config, Engine, Extern, Func, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Val,
};

use super::{ModError, ModProblem};
use crate::ctx::Ctx;

/// The module the host functions are imported from.
const HOST_MODULE: &str = "ponko";
/// The longest text one `log` call takes, in bytes.
const MAX_LOG_BYTES: usize = 4096;
/// Lines kept in the log until [`ModCode::take_log`]; later ones are dropped.
const MAX_LOG_LINES: usize = 1024;
/// The most parameters a mod function called by the game may have.
const MAX_PARAMS: usize = 16;

/// What a code mod may use: the same for every call and every mod.
///
/// ```
/// use ponko::mods::CodeLimits;
/// let strict = CodeLimits { fuel_per_call: 1_000_000, ..CodeLimits::default() };
/// assert_eq!(strict.memory_bytes, 16 << 20);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeLimits {
    /// Fuel one call gets (and starting a module, at mount): about one unit
    /// per WebAssembly instruction. A call that runs out stops with a
    /// [`ModProblem::Trap`]. Default 10 000 000 (a few milliseconds).
    pub fuel_per_call: u64,
    /// Bytes a module's linear memory may grow to; a module that asks for
    /// more is refused at mount, and `memory.grow` past it fails. Default
    /// 16 MiB.
    pub memory_bytes: u64,
}

impl Default for CodeLimits {
    fn default() -> Self {
        Self {
            fuel_per_call: 10_000_000,
            memory_bytes: 16 << 20,
        }
    }
}

/// A number passed to or returned from a mod's function: WebAssembly's four
/// number types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModValue {
    /// `i32`.
    I32(i32),
    /// `i64`, such as an [`Entity`](ponko_ecs::Entity)'s bits.
    I64(i64),
    /// `f32`.
    F32(f32),
    /// `f64`.
    F64(f64),
}

impl From<i32> for ModValue {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}
impl From<i64> for ModValue {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<f32> for ModValue {
    fn from(v: f32) -> Self {
        Self::F32(v)
    }
}
impl From<f64> for ModValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl ModValue {
    /// The value if it is an `f32`.
    pub fn as_f32(self) -> Option<f32> {
        if let Self::F32(v) = self { Some(v) } else { None }
    }

    /// The value if it is an `i32`.
    pub fn as_i32(self) -> Option<i32> {
        if let Self::I32(v) = self { Some(v) } else { None }
    }

    fn to_val(self) -> Val {
        match self {
            Self::I32(v) => Val::I32(v),
            Self::I64(v) => Val::I64(v),
            Self::F32(v) => Val::F32(v.into()),
            Self::F64(v) => Val::F64(v.into()),
        }
    }

    fn from_val(v: &Val) -> Option<Self> {
        match v {
            Val::I32(v) => Some(Self::I32(*v)),
            Val::I64(v) => Some(Self::I64(*v)),
            Val::F32(v) => Some(Self::F32(f32::from(*v))),
            Val::F64(v) => Some(Self::F64(f64::from(*v))),
            _ => None,
        }
    }
}

/// A function a code mod exports, found with [`ModCode::function`]: keep it
/// and call it with [`ModCode::call`] (no name lookup per call).
#[derive(Clone, Copy)]
pub struct ModFn {
    func: Func,
    slot: usize,
}

impl fmt::Debug for ModFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModFn")
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

/// What the host functions reach.
struct Host {
    limits: StoreLimits,
    /// The mod being called, for the log.
    current: usize,
    dt: f32,
    log: Vec<(usize, String)>,
    /// The game's physics during [`with_code`], a spare one otherwise (it is
    /// swapped with `ctx.physics`, never allocated per call).
    #[cfg(feature = "physics")]
    physics: std::sync::Mutex<crate::d2::Physics>,
}

/// The engine every code mod is compiled with: fuel counted.
pub(super) fn engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    Engine::new(&config)
}

/// Compiles a mod's module and checks what it imports, without running it.
pub(super) fn compile(engine: &Engine, bytes: &[u8]) -> Result<Module, ModProblem> {
    let module =
        Module::new(engine, bytes).map_err(|e| ModProblem::Code(format!("not a valid WebAssembly module: {e}")))?;
    for import in module.imports() {
        let known = import.module() == HOST_MODULE && ["log", "dt", "cast_ray"].contains(&import.name());
        if !known {
            return Err(ModProblem::Code(format!(
                "imports \"{}\".\"{}\", which isn't a host function (\"ponko\": log, dt, cast_ray)",
                import.module(),
                import.name()
            )));
        }
    }
    Ok(module)
}

fn linker(engine: &Engine) -> Linker<Host> {
    let mut linker = Linker::new(engine);
    let log = |mut caller: Caller<'_, Host>, ptr: i32, len: i32| {
        let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
            return;
        };
        let len = (len.max(0) as usize).min(MAX_LOG_BYTES);
        let mut buf = vec![0; len];
        if memory.read(&caller, ptr.max(0) as usize, &mut buf).is_err() {
            return;
        }
        let host = caller.data_mut();
        if host.log.len() < MAX_LOG_LINES {
            host.log
                .push((host.current, String::from_utf8_lossy(&buf).into_owned()));
        }
    };
    let dt = |caller: Caller<'_, Host>| caller.data().dt;
    let cast_ray = |mut caller: Caller<'_, Host>, ox: f32, oy: f32, dx: f32, dy: f32, max: f32| -> f32 {
        #[cfg(feature = "physics")]
        {
            let physics = caller.data_mut().physics.get_mut().unwrap_or_else(|e| e.into_inner());
            let hit = physics.cast_ray(
                glam::vec2(ox, oy),
                glam::vec2(dx, dy),
                max,
                crate::d2::QueryFilter::default(),
            );
            hit.map_or(-1.0, |h| h.distance)
        }
        #[cfg(not(feature = "physics"))]
        {
            let _ = (&mut caller, ox, oy, dx, dy, max);
            -1.0
        }
    };
    // The names are fixed and the closures' types fit: defining them can't fail.
    let defined = linker
        .func_wrap(HOST_MODULE, "log", log)
        .and_then(|l| l.func_wrap(HOST_MODULE, "dt", dt))
        .and_then(|l| l.func_wrap(HOST_MODULE, "cast_ray", cast_ray));
    debug_assert!(defined.is_ok(), "host functions defined twice");
    linker
}

/// The running code mods: a resource [`ModSet::mount`](super::ModSet::mount)
/// inserts when a mod has code. Reach it with [`with_code`].
pub struct ModCode {
    store: Store<Host>,
    /// Each code mod's id and instance, in load order.
    mods: Vec<(String, Instance)>,
    fuel_per_call: u64,
    args: Vec<Val>,
    results: Vec<Val>,
}

impl fmt::Debug for ModCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModCode")
            .field("mods", &self.mods.iter().map(|(id, _)| id).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl ModCode {
    /// Starts every module (`(id, module)`, in load order) in one store:
    /// each module's start function runs with one call's fuel.
    pub(super) fn start(engine: &Engine, modules: Vec<(String, Module)>, limits: CodeLimits) -> Result<Self, ModError> {
        let memory = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);
        let host = Host {
            limits: StoreLimitsBuilder::new()
                .memory_size(memory)
                .instances(modules.len())
                .memories(modules.len())
                .tables(modules.len())
                .table_elements(100_000)
                .build(),
            current: 0,
            dt: 0.0,
            log: Vec::new(),
            #[cfg(feature = "physics")]
            physics: std::sync::Mutex::new(crate::d2::Physics::default()),
        };
        let mut store = Store::new(engine, host);
        store.limiter(|host| &mut host.limits);
        let linker = linker(engine);
        let mut mods = Vec::with_capacity(modules.len());
        for (slot, (id, module)) in modules.into_iter().enumerate() {
            store.data_mut().current = slot;
            let fueled = store.set_fuel(limits.fuel_per_call);
            debug_assert!(fueled.is_ok(), "fuel is on in `engine()`");
            let instance = linker
                .instantiate_and_start(&mut store, &module)
                .map_err(|e| ModError::new(&id, None, ModProblem::Code(format!("can't start: {e}"))))?;
            mods.push((id, instance));
        }
        Ok(Self {
            store,
            mods,
            fuel_per_call: limits.fuel_per_call,
            args: Vec::with_capacity(MAX_PARAMS),
            results: Vec::with_capacity(4),
        })
    }

    /// The ids of the mods with code, in load order.
    pub fn mods(&self) -> impl Iterator<Item = &str> {
        self.mods.iter().map(|(id, _)| id.as_str())
    }

    /// The function `name` that mod `mod_id` exports, or `None` if it has no
    /// code or no such function.
    pub fn function(&self, mod_id: &str, name: &str) -> Option<ModFn> {
        let slot = self.mods.iter().position(|(id, _)| id == mod_id)?;
        let func = self.mods[slot].1.get_func(&self.store, name)?;
        Some(ModFn { func, slot })
    }

    /// Every code mod's function `name`, in load order, with its mod's id:
    /// a hook each mod may answer (`"on_spawn"`).
    pub fn exporting<'a>(&'a self, name: &'a str) -> impl Iterator<Item = (&'a str, ModFn)> + 'a {
        self.mods.iter().enumerate().filter_map(move |(slot, (id, instance))| {
            let func = instance.get_func(&self.store, name)?;
            Some((id.as_str(), ModFn { func, slot }))
        })
    }

    /// Calls `f` with `args` and one call's fuel; returns its result (a
    /// function may return one value or none). No allocation per call.
    ///
    /// # Errors
    /// A [`ModProblem::Trap`] naming the mod: the arguments don't fit the
    /// function's parameters, it returns more than one value, or it stopped:
    /// out of fuel, out of memory, a WebAssembly trap. The mod stays usable;
    /// what a game does with a mod that fails (turn it off, report it) is
    /// its choice.
    pub fn call(&mut self, f: ModFn, args: &[ModValue]) -> Result<Option<ModValue>, ModError> {
        let trap = |code: &Self, why: String| ModError::new(&code.mods[f.slot].0, None, ModProblem::Trap(why));
        if args.len() > MAX_PARAMS {
            return Err(trap(
                self,
                format!("{} arguments, more than the {MAX_PARAMS} a call takes", args.len()),
            ));
        }
        let ty = f.func.ty(&self.store);
        if ty.results().len() > 1 {
            return Err(trap(self, "returns more than one value".into()));
        }
        self.args.clear();
        self.args.extend(args.iter().map(|a| a.to_val()));
        self.results.clear();
        self.results
            .extend(ty.results().iter().map(|t| Val::default_for_ty(*t)));
        self.store.data_mut().current = f.slot;
        let fueled = self.store.set_fuel(self.fuel_per_call);
        debug_assert!(fueled.is_ok(), "fuel is on in `engine()`");
        if let Err(e) = f.func.call(&mut self.store, &self.args, &mut self.results) {
            return Err(trap(self, e.to_string()));
        }
        Ok(self.results.first().and_then(ModValue::from_val))
    }

    /// Takes the lines the mods logged since the last call, each with its
    /// mod's id. At most 1024 lines are kept in between, 4096 bytes each.
    pub fn take_log(&mut self) -> Vec<(String, String)> {
        let lines = std::mem::take(&mut self.store.data_mut().log);
        lines
            .into_iter()
            .map(|(slot, line)| (self.mods[slot].0.clone(), line))
            .collect()
    }

    /// Fuel left after the last call: how close a mod comes to its limit.
    pub fn fuel_left(&self) -> u64 {
        self.store.get_fuel().unwrap_or(0)
    }
}

/// Runs `f` with the mounted code mods, their host functions reaching this
/// tick's `dt` and the game's 2D physics; `None` (and `f` isn't called)
/// when no mounted mod has code.
///
/// ```
/// use ponko::mods::{ModPack, ModSet, ModValue, with_code};
/// use ponko::prelude::*;
/// // A module that doubles a number (a real mod is built from Rust, C, ...).
/// let wasm = wat::parse_str(r#"(module (func (export "double") (param f32) (result f32)
///     local.get 0 local.get 0 f32.add))"#).unwrap();
/// let manifest = r#"{"format":"ponko-mod","version":1,"id":"doubler","code":"doubler.wasm"}"#;
/// let pack = ModPack::from_files("doubler", [("mod.json", manifest.as_bytes().to_vec()), ("doubler.wasm", wasm)]).unwrap();
/// Game::new("modded", 1, 1).run_headless(0, |ctx| {
///     ModSet::new().with(pack).mount(ctx).unwrap();
///     let out = with_code(ctx, |code| {
///         let double = code.function("doubler", "double").expect("exported");
///         code.call(double, &[ModValue::F32(21.0)])
///     });
///     assert_eq!(out, Some(Ok(Some(ModValue::F32(42.0)))));
/// });
/// ```
pub fn with_code<R>(ctx: &mut Ctx, f: impl FnOnce(&mut ModCode) -> R) -> Option<R> {
    let mut code = ctx.world.remove_resource::<ModCode>()?;
    code.store.data_mut().dt = ctx.time.dt();
    #[cfg(feature = "physics")]
    std::mem::swap(
        code.store
            .data_mut()
            .physics
            .get_mut()
            .unwrap_or_else(|e| e.into_inner()),
        &mut ctx.physics,
    );
    let out = f(&mut code);
    #[cfg(feature = "physics")]
    std::mem::swap(
        code.store
            .data_mut()
            .physics
            .get_mut()
            .unwrap_or_else(|e| e.into_inner()),
        &mut ctx.physics,
    );
    ctx.world.insert_resource(code);
    Some(out)
}
