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
//! | `get_field` | `(entity: i64, path: i32, len: i32) -> f64` | A number field (`"Health.current"`, `"Transform2D.position.0"`) of a reflected component of an [exposed](ModCode::expose) entity; NaN when refused |
//! | `set_field` | `(entity: i64, path: i32, len: i32, value: f64) -> i32` | Writes that field: 0 when written, else a [`refusal code`](REFUSED_ENTITY) |
//! | `spawn_prefab` | `(path: i32, len: i32, x: f32, y: f32) -> i64` | Spawns an [exposed](ModCode::expose_prefab) prefab at `(x, y)` and exposes its root: the root's bits, or a negative refusal code |
//! | `random` | `() -> f32` | A number in `[0, 1)` from the mod's generator, seeded by [`ModCode::seed_random`] |
//!
//! Text arguments (`path`, `len`) are UTF-8 bytes in the module's exported
//! `memory`, at most 256 bytes. Every refusal also adds a line starting
//! with `refused:` to [`ModCode::take_log`].

use std::collections::HashSet;
use std::fmt;
use std::ptr::NonNull;

use glam::vec2;
use ponko_ecs::Entity;
use serde_json::Value as Json;
use wasmi::{
    Caller, Config, Engine, Extern, Func, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Val,
};

use super::{ModError, ModProblem};
use crate::ctx::Ctx;
use crate::random::Rng;
use crate::reflect::WorldReflect;

/// The module the host functions are imported from.
const HOST_MODULE: &str = "ponko";
/// The longest text one `log` call takes, in bytes.
const MAX_LOG_BYTES: usize = 4096;
/// Lines kept in the log until [`ModCode::take_log`]; later ones are dropped.
const MAX_LOG_LINES: usize = 1024;
/// The most parameters a mod function called by the game may have.
const MAX_PARAMS: usize = 16;
/// The longest field path or prefab path a host function takes, in bytes.
const MAX_PATH_BYTES: usize = 256;
/// The host functions a module may import from `"ponko"`.
const HOST_FUNCTIONS: [&str; 7] = [
    "log",
    "dt",
    "cast_ray",
    "get_field",
    "set_field",
    "spawn_prefab",
    "random",
];

/// `set_field` and `spawn_prefab`'s answer when the entity isn't one the game
/// [exposed](ModCode::expose), or is dead (for `spawn_prefab`: the prefab
/// isn't one the game [exposed](ModCode::expose_prefab)).
pub const REFUSED_ENTITY: i32 = -1;
/// `set_field`'s answer when the entity has no such reflected component or
/// the component no such field (`spawn_prefab`'s: the prefab didn't spawn).
pub const REFUSED_FIELD: i32 = -2;
/// `set_field`'s answer when the field isn't a number or a bool, or the value
/// doesn't fit it (a fraction for an integer field, NaN, out of range); also
/// the answer to text that isn't readable UTF-8 within 256 bytes.
pub const REFUSED_VALUE: i32 = -3;

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
    /// The game during a [`ModCode::call`] made inside the [`with_code`]
    /// call that handed the code out, for the host functions that read and
    /// change it; `None` otherwise.
    ctx: GamePtr,
    /// The entities mods may read and change.
    exposed: HashSet<Entity>,
    /// The prefabs mods may spawn.
    prefabs: HashSet<String>,
    /// Each mod's id and random number generator, by slot.
    rngs: Vec<(String, Rng)>,
}

/// The game, as [`with_code`] handed it to its `ModCode`: the pointer and
/// the number of the `with_code` call that set it. Nothing follows it
/// directly: [`ModCode::call`] checks once that this call is still running
/// on this thread and only then hands the pointer to the host functions
/// ([`GamePtr`]) for the length of that one call.
struct CtxPtr(Option<(NonNull<Ctx>, u64)>);

impl CtxPtr {
    /// The pointer, if the `with_code` call that set it is the one running
    /// on this thread now.
    fn live(&self) -> Option<NonNull<Ctx>> {
        let (ptr, call) = self.0?;
        (RUNNING.get() == call).then_some(ptr)
    }
}

/// The game for the length of one [`ModCode::call`], checked live
/// ([`CtxPtr::live`]) when that call began: what the host functions follow.
struct GamePtr(Option<NonNull<Ctx>>);

thread_local! {
    /// The number of the `with_code` call running on this thread, 0 for none.
    /// A pointer is handed to the host functions only while the call that
    /// set it runs, so a `ModCode` swapped out of its call (it is handed out
    /// as `&mut`) or left behind by a panic holds a pointer nothing follows.
    static RUNNING: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Marks a `with_code` call as the one running on this thread until dropped
/// (also when the call unwinds), then puts back the call around it.
struct Running(u64);

impl Running {
    fn start() -> (Self, u64) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let call = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (Self(RUNNING.replace(call)), call)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.set(self.0);
    }
}

// SAFETY: the pointer is set only for the length of a `with_code` call, on the
// thread that holds the `&mut Ctx` it came from, and followed only through
// `CtxPtr::live`, which checks on the calling thread that this call is the one
// running (`RUNNING` is per thread and names that call). Sending or sharing
// `ModCode` outside that call shares a pointer nothing follows.
unsafe impl Send for CtxPtr {}
// SAFETY: as for `Send`.
unsafe impl Sync for CtxPtr {}
// SAFETY: `ModCode::call` sets it from `CtxPtr::live` on the thread that runs
// the `with_code` call, and host functions follow it only inside that same
// `ModCode::call` on that thread (wasmi runs a host function on the thread that
// called into the module). The next call sets it again before any module code
// runs, so a pointer left by a call that unwound is never followed.
unsafe impl Send for GamePtr {}
// SAFETY: as for `Send`.
unsafe impl Sync for GamePtr {}

impl Host {
    /// Runs `f` with the game, or returns `None` outside a [`ModCode::call`]
    /// made inside the [`with_code`] call that handed the code out.
    fn with_ctx<R>(&mut self, f: impl FnOnce(&mut Ctx, &mut HashSet<Entity>, &HashSet<String>) -> R) -> Option<R> {
        let mut ptr = self.ctx.0?;
        // SAFETY: `ModCode::call` set the pointer for this call only, after
        // `CtxPtr::live` found the `with_code` call that set it from its
        // `&mut Ctx` running on this thread; that call doesn't use `ctx`
        // while `f` (inside it) runs. The `ModCode` this `Host` lives in was
        // taken out of `ctx.world`, so the game and the host don't overlap,
        // and nothing else touches `ctx` while the mod's call runs.
        let ctx = unsafe { ptr.as_mut() };
        Some(f(ctx, &mut self.exposed, &self.prefabs))
    }

    /// Notes a refusal in the log for the game to see.
    fn refuse(&mut self, why: String) {
        if self.log.len() < MAX_LOG_LINES {
            self.log.push((self.current, format!("refused: {why}")));
        }
    }
}

/// A seed mixed with a mod's id: each mod has its own sequence.
fn rng_for(seed: u64, id: &str) -> Rng {
    // FNV-1a: stable on every machine and in every build.
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Rng::new(seed ^ hash)
}

/// Text from the calling module's `memory`: at most [`MAX_PATH_BYTES`], UTF-8.
fn read_text(caller: &Caller<'_, Host>, ptr: i32, len: i32) -> Option<String> {
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return None;
    };
    let len = usize::try_from(len).ok().filter(|&n| n <= MAX_PATH_BYTES)?;
    let mut buf = vec![0; len];
    memory.read(caller, usize::try_from(ptr).ok()?, &mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// Why a field access was refused.
enum FieldRefusal {
    Entity,
    Field,
    Value,
}

impl FieldRefusal {
    fn code(&self) -> i32 {
        match self {
            Self::Entity => REFUSED_ENTITY,
            Self::Field => REFUSED_FIELD,
            Self::Value => REFUSED_VALUE,
        }
    }
}

/// The component of an exposed entity named by `path`'s first part, as
/// JSON, and the rest of the path (at least one part).
fn component_at<'p>(
    world: &ponko_ecs::World,
    exposed: &HashSet<Entity>,
    bits: i64,
    path: &'p str,
) -> Result<(Entity, &'p str, Json, Vec<&'p str>), FieldRefusal> {
    let e = Entity::from_bits(bits as u64);
    if !exposed.contains(&e) || !world.is_alive(e) {
        return Err(FieldRefusal::Entity);
    }
    let mut parts = path.split('.');
    let component = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return Err(FieldRefusal::Field);
    }
    let json = world.get_json(e, component).map_err(|_| FieldRefusal::Field)?;
    Ok((e, component, json, rest))
}

/// The value at `parts` in `json`: object keys, or array indices.
fn walk<'j>(mut json: &'j mut Json, parts: &[&str]) -> Option<&'j mut Json> {
    for part in parts {
        json = match json {
            Json::Object(map) => map.get_mut(*part)?,
            Json::Array(items) => items.get_mut(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(json)
}

/// A JSON number or bool as an `f64`.
fn number(json: &Json) -> Result<f64, FieldRefusal> {
    match json {
        Json::Number(n) => n.as_f64().ok_or(FieldRefusal::Value),
        Json::Bool(b) => Ok(f64::from(u8::from(*b))),
        _ => Err(FieldRefusal::Value),
    }
}

fn get_field(world: &ponko_ecs::World, exposed: &HashSet<Entity>, bits: i64, path: &str) -> Result<f64, FieldRefusal> {
    let (_, _, mut json, rest) = component_at(world, exposed, bits, path)?;
    number(walk(&mut json, &rest).ok_or(FieldRefusal::Field)?)
}

fn set_field(
    world: &mut ponko_ecs::World,
    exposed: &HashSet<Entity>,
    bits: i64,
    path: &str,
    value: f64,
) -> Result<(), FieldRefusal> {
    let (e, component, mut json, rest) = component_at(world, exposed, bits, path)?;
    let slot = walk(&mut json, &rest).ok_or(FieldRefusal::Field)?;
    *slot = match &*slot {
        Json::Bool(_) => Json::Bool(value != 0.0),
        // An integer field takes a whole number in its range (`set_json`
        // checks the range); a float field any finite number.
        Json::Number(n) if n.is_i64() || n.is_u64() => {
            if value.fract() != 0.0 || !value.is_finite() || value.abs() > 9.007_199_254_740_992e15 {
                return Err(FieldRefusal::Value);
            }
            if value < 0.0 {
                Json::from(value as i64)
            } else {
                Json::from(value as u64)
            }
        }
        Json::Number(_) => serde_json::Number::from_f64(value)
            .map(Json::Number)
            .ok_or(FieldRefusal::Value)?,
        _ => return Err(FieldRefusal::Value),
    };
    let top = rest[0];
    let changed = json.get(top).cloned().unwrap_or_default();
    world
        .set_json(e, component, serde_json::json!({ top: changed }))
        .map_err(|_| FieldRefusal::Value)
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
        let known = import.module() == HOST_MODULE && HOST_FUNCTIONS.contains(&import.name());
        if !known {
            return Err(ModProblem::Code(format!(
                "imports \"{}\".\"{}\", which isn't a host function (\"ponko\": {})",
                import.module(),
                import.name(),
                HOST_FUNCTIONS.join(", ")
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
            let hit = caller.data_mut().with_ctx(|ctx, _, _| {
                ctx.physics
                    .cast_ray(vec2(ox, oy), vec2(dx, dy), max, crate::d2::QueryFilter::default())
                    .map(|h| h.distance)
            });
            hit.flatten().unwrap_or(-1.0)
        }
        #[cfg(not(feature = "physics"))]
        {
            let _ = (&mut caller, ox, oy, dx, dy, max);
            -1.0
        }
    };
    let get = |mut caller: Caller<'_, Host>, entity: i64, ptr: i32, len: i32| -> f64 {
        let Some(path) = read_text(&caller, ptr, len) else {
            caller
                .data_mut()
                .refuse("get_field: the path isn't UTF-8 text of at most 256 bytes".into());
            return f64::NAN;
        };
        let host = caller.data_mut();
        match host.with_ctx(|ctx, exposed, _| get_field(&ctx.world, exposed, entity, &path)) {
            Some(Ok(v)) => v,
            Some(Err(why)) => {
                host.refuse(field_refusal("get_field", entity, &path, &why));
                f64::NAN
            }
            None => f64::NAN,
        }
    };
    let set = |mut caller: Caller<'_, Host>, entity: i64, ptr: i32, len: i32, value: f64| -> i32 {
        let Some(path) = read_text(&caller, ptr, len) else {
            caller
                .data_mut()
                .refuse("set_field: the path isn't UTF-8 text of at most 256 bytes".into());
            return REFUSED_VALUE;
        };
        let host = caller.data_mut();
        match host.with_ctx(|ctx, exposed, _| set_field(&mut ctx.world, exposed, entity, &path, value)) {
            Some(Ok(())) => 0,
            Some(Err(why)) => {
                host.refuse(field_refusal("set_field", entity, &path, &why));
                why.code()
            }
            None => REFUSED_ENTITY,
        }
    };
    let spawn = |mut caller: Caller<'_, Host>, ptr: i32, len: i32, x: f32, y: f32| -> i64 {
        let Some(path) = read_text(&caller, ptr, len) else {
            caller
                .data_mut()
                .refuse("spawn_prefab: the path isn't UTF-8 text of at most 256 bytes".into());
            return REFUSED_VALUE.into();
        };
        let host = caller.data_mut();
        let spawned = host.with_ctx(|ctx, exposed, prefabs| {
            if !prefabs.contains(&path) {
                return Err((
                    REFUSED_ENTITY,
                    format!("spawn_prefab: \"{path}\" isn't a prefab the game exposed"),
                ));
            }
            let root = ctx
                .spawn_prefab(&path, vec2(x, y))
                .map_err(|e| (REFUSED_FIELD, format!("spawn_prefab: \"{path}\" didn't spawn: {e}")))?;
            exposed.insert(root);
            Ok(root)
        });
        match spawned {
            Some(Ok(root)) => root.to_bits() as i64,
            Some(Err((code, why))) => {
                host.refuse(why);
                code.into()
            }
            None => REFUSED_ENTITY.into(),
        }
    };
    let random = |mut caller: Caller<'_, Host>| -> f32 {
        let host = caller.data_mut();
        host.rngs.get_mut(host.current).map_or(0.0, |(_, rng)| rng.f32())
    };
    // The names are fixed and the closures' types fit: defining them can't fail.
    let defined = linker
        .func_wrap(HOST_MODULE, "log", log)
        .and_then(|l| l.func_wrap(HOST_MODULE, "dt", dt))
        .and_then(|l| l.func_wrap(HOST_MODULE, "cast_ray", cast_ray))
        .and_then(|l| l.func_wrap(HOST_MODULE, "get_field", get))
        .and_then(|l| l.func_wrap(HOST_MODULE, "set_field", set))
        .and_then(|l| l.func_wrap(HOST_MODULE, "spawn_prefab", spawn))
        .and_then(|l| l.func_wrap(HOST_MODULE, "random", random));
    debug_assert!(defined.is_ok(), "host functions defined twice");
    linker
}

/// The log line of a refused field access.
fn field_refusal(call: &str, bits: i64, path: &str, why: &FieldRefusal) -> String {
    let e = Entity::from_bits(bits as u64);
    match why {
        FieldRefusal::Entity => format!("{call}: entity {e:?} isn't one the game exposed, or is gone"),
        FieldRefusal::Field => format!("{call}: entity {e:?} has no reflected field \"{path}\""),
        FieldRefusal::Value => {
            format!("{call}: \"{path}\" of entity {e:?} isn't a number field, or the value doesn't fit it")
        }
    }
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
    /// The game as [`with_code`] handed it over; checked once per
    /// [`call`](Self::call), not once per host function.
    ctx: CtxPtr,
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
            ctx: GamePtr(None),
            exposed: HashSet::new(),
            prefabs: HashSet::new(),
            rngs: modules.iter().map(|(id, _)| (id.clone(), rng_for(0, id))).collect(),
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
            ctx: CtxPtr(None),
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
        let host = self.store.data_mut();
        host.current = f.slot;
        // Once per call: the game, if the `with_code` call that handed this
        // code out is still the one running here.
        host.ctx = GamePtr(self.ctx.live());
        let fueled = self.store.set_fuel(self.fuel_per_call);
        debug_assert!(fueled.is_ok(), "fuel is on in `engine()`");
        let called = f.func.call(&mut self.store, &self.args, &mut self.results);
        self.store.data_mut().ctx = GamePtr(None);
        if let Err(e) = called {
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

    /// Lets the mods read and write `e`'s reflected components with
    /// `get_field` and `set_field` (the game hands them `e.to_bits()`). A mod
    /// can't reach an entity the game didn't expose, apart from the roots of
    /// the prefabs it spawns, which are exposed for it.
    pub fn expose(&mut self, e: Entity) {
        self.store.data_mut().exposed.insert(e);
    }

    /// Takes `e` back from the mods; returns whether it was exposed.
    pub fn hide(&mut self, e: Entity) -> bool {
        self.store.data_mut().exposed.remove(&e)
    }

    /// Whether mods may reach `e`.
    pub fn is_exposed(&self, e: Entity) -> bool {
        self.store.data().exposed.contains(&e)
    }

    /// Takes every entity back from the mods.
    pub fn hide_all(&mut self) {
        self.store.data_mut().exposed.clear();
    }

    /// Lets the mods spawn the prefab at `path` (`"prefabs/frost.json"`, the
    /// name they pass to `spawn_prefab`, the one [`Ctx::spawn_prefab`]
    /// takes; a mod's own prefab once its pack is mounted).
    pub fn expose_prefab(&mut self, path: impl Into<String>) {
        self.store.data_mut().prefabs.insert(path.into());
    }

    /// Seeds every mod's `random`: from here, a mod's numbers depend only on
    /// `seed`, its id and how many it took (same seed, same sequence; mods
    /// don't share a sequence). The seed is 0 until the game sets one.
    pub fn seed_random(&mut self, seed: u64) {
        for (id, rng) in &mut self.store.data_mut().rngs {
            *rng = rng_for(seed, id);
        }
    }
}

/// Runs `f` with the mounted code mods, their host functions reaching this
/// tick's `dt`, the game's 2D physics and, for the entities and prefabs the
/// game exposed, its world; `None` (and `f` isn't called) when no mounted
/// mod has code.
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
    let (running, call) = Running::start();
    code.ctx = CtxPtr(Some((NonNull::from(&mut *ctx), call)));
    let out = f(&mut code);
    drop(running);
    code.ctx = CtxPtr(None);
    ctx.world.insert_resource(code);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The host functions reach the game only through `with_code`'s pointer:
    /// a mod writes an exposed entity's field, and the code is back in the
    /// world, its pointer cleared, after the call. (Miri runs this.)
    #[test]
    fn host_functions_reach_the_game_only_during_with_code() {
        let wasm = wat::parse_str(
            r#"(module
                (import "ponko" "set_field" (func $set (param i64 i32 i32 f64) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Transform2D.rotation")
                (func (export "turn") (param i64) (result i32)
                    local.get 0 i32.const 0 i32.const 20 f64.const 0.5 call $set))"#,
        )
        .expect("valid WebAssembly text");
        let engine = engine();
        let module = compile(&engine, &wasm).expect("compiles");
        let code = ModCode::start(&engine, vec![("turner".into(), module)], CodeLimits::default()).expect("starts");
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(8.0, 8.0));
        ctx.world.reflect::<crate::d2::Transform>();
        ctx.world.insert_resource(code);
        let e = ctx.world.spawn((crate::d2::Transform::default(),));
        let answer = with_code(&mut ctx, |code| {
            code.expose(e);
            let turn = code.function("turner", "turn").expect("exported");
            code.call(turn, &[ModValue::I64(e.to_bits() as i64)])
        });
        assert_eq!(answer, Some(Ok(Some(ModValue::I32(0)))));
        assert_eq!(ctx.world.get::<crate::d2::Transform>(e).map(|t| t.rotation), Some(0.5));
        let code = ctx.world.get_resource::<ModCode>().expect("back in the world");
        assert!(code.ctx.0.is_none(), "no pointer kept after with_code");
        assert!(code.store.data().ctx.0.is_none(), "no pointer kept after the call");
        // ModCode is a resource: Send and Sync.
        fn shared<T: Send + Sync>(_: &T) {}
        shared(code);
    }

    /// `with_code` hands the code out as `&mut`, so a game can swap another
    /// `ModCode` in and keep the one that holds the pointer: its host
    /// functions must not follow the pointer once that call is over.
    #[test]
    fn code_swapped_out_of_with_code_cannot_reach_the_game_afterwards() {
        let wasm = wat::parse_str(
            r#"(module
                (import "ponko" "set_field" (func $set (param i64 i32 i32 f64) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Transform2D.rotation")
                (func (export "turn") (param i64) (result i32)
                    local.get 0 i32.const 0 i32.const 20 f64.const 0.5 call $set))"#,
        )
        .expect("valid WebAssembly text");
        let engine = engine();
        let start = || {
            let module = compile(&engine, &wasm).expect("compiles");
            ModCode::start(&engine, vec![("turner".into(), module)], CodeLimits::default()).expect("starts")
        };
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(8.0, 8.0));
        ctx.world.reflect::<crate::d2::Transform>();
        ctx.world.insert_resource(start());
        let e = ctx.world.spawn((crate::d2::Transform::default(),));
        let mut kept = start();
        with_code(&mut ctx, |code| {
            code.expose(e);
            std::mem::swap(code, &mut kept);
        });
        // `kept` is the code the call gave its pointer to.
        let turn = kept.function("turner", "turn").expect("exported");
        let answer = kept.call(turn, &[ModValue::I64(e.to_bits() as i64)]);
        assert_ne!(answer, Ok(Some(ModValue::I32(0))), "the write was refused");
        assert_eq!(ctx.world.get::<crate::d2::Transform>(e).map(|t| t.rotation), Some(0.0));
    }

    /// The game is checked once per `ModCode::call`, not per host function:
    /// code swapped into a `with_code` call (it was never handed the game)
    /// is refused for every host call it makes, before and after the code
    /// that was handed the game makes its own; and no call leaves the
    /// pointer behind for the host functions.
    #[test]
    fn code_swapped_into_with_code_cannot_reach_the_game() {
        let wasm = wat::parse_str(
            r#"(module
                (import "ponko" "set_field" (func $set (param i64 i32 i32 f64) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "Transform2D.rotation")
                (func (export "turn") (param i64 f64) (result i32)
                    local.get 0 i32.const 0 i32.const 20 local.get 1 call $set
                    local.get 0 i32.const 0 i32.const 20 local.get 1 call $set
                    i32.add))"#,
        )
        .expect("valid WebAssembly text");
        let engine = engine();
        let start = || {
            let module = compile(&engine, &wasm).expect("compiles");
            ModCode::start(&engine, vec![("turner".into(), module)], CodeLimits::default()).expect("starts")
        };
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::new(8.0, 8.0));
        ctx.world.reflect::<crate::d2::Transform>();
        ctx.world.insert_resource(start());
        let e = ctx.world.spawn((crate::d2::Transform::default(),));
        let bits = ModValue::I64(e.to_bits() as i64);
        let mut outside = start();
        outside.expose(e);
        // A function belongs to its code's store: one for each.
        let turn_outside = outside.function("turner", "turn").expect("exported");
        let answers = with_code(&mut ctx, |code| {
            code.expose(e);
            let turn = code.function("turner", "turn").expect("exported");
            std::mem::swap(code, &mut outside);
            // `code` is now the one the call never handed the game.
            let refused = code.call(turn_outside, &[bits, ModValue::F64(0.25)]);
            assert!(code.store.data().ctx.0.is_none(), "nothing kept after a call");
            std::mem::swap(code, &mut outside);
            let written = code.call(turn, &[bits, ModValue::F64(0.5)]);
            assert!(code.store.data().ctx.0.is_none(), "nothing kept after a call");
            std::mem::swap(code, &mut outside);
            let refused_again = code.call(turn_outside, &[bits, ModValue::F64(0.75)]);
            std::mem::swap(code, &mut outside);
            (refused, written, refused_again)
        })
        .expect("the mod has code");
        // Both host calls of each refused call were refused; both written.
        assert_eq!(answers.0, Ok(Some(ModValue::I32(2 * REFUSED_ENTITY))));
        assert_eq!(answers.1, Ok(Some(ModValue::I32(0))));
        assert_eq!(answers.2, Ok(Some(ModValue::I32(2 * REFUSED_ENTITY))));
        assert_eq!(ctx.world.get::<crate::d2::Transform>(e).map(|t| t.rotation), Some(0.5));
    }
}
