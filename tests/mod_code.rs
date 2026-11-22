//! Code mods (`ponko::mods`, feature `mods-wasm`): a mod's WebAssembly
//! module, run by the interpreter in a sandbox. Calls and results, the host
//! functions, fuel and memory limits that stop hostile modules, and modules
//! refused at mount. No window.
#![cfg(feature = "mods-wasm")]

use ponko::mods::{CodeLimits, ModError, ModPack, ModProblem, ModSet, ModValue, Mods, with_code};
use ponko::prelude::*;

/// A mod with id `id` whose code is the module `wat`.
fn coded(id: &str, wat: &str) -> ModPack {
    let manifest = format!(r#"{{"format":"ponko-mod","version":1,"id":"{id}","code":"code/{id}.wasm"}}"#);
    let wasm = wat::parse_str(wat).expect("valid WebAssembly text");
    ModPack::from_files(
        id,
        [
            ("mod.json".to_string(), manifest.into_bytes()),
            (format!("code/{id}.wasm"), wasm),
        ],
    )
    .expect("a valid manifest")
}

fn mount(set: ModSet, body: impl FnOnce(&mut Ctx)) {
    Game::new("code mods", 1, 1).run_headless(0, |ctx| {
        set.mount(ctx).expect("the mods mount");
        body(ctx);
    });
}

fn mount_err(set: ModSet) -> ModError {
    let mut err = None;
    Game::new("code mods", 1, 1).run_headless(0, |ctx| {
        err = set.mount(ctx).err();
        assert!(ctx.world.get_resource::<Mods>().is_none(), "nothing mounted");
        assert!(
            ctx.world.get_resource::<ponko::mods::ModCode>().is_none(),
            "no code running"
        );
    });
    err.expect("the set is refused")
}

const ADD: &str = r#"(module
    (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add)
    (func (export "nothing")))"#;

/// Spins forever.
const SPIN: &str = r#"(module (func (export "spin") (loop $l br $l)))"#;

/// Grows its memory a page at a time until growing fails; returns the
/// pages it got.
const HOARD: &str = r#"(module (memory 1)
    (func (export "hoard") (result i32)
        (block $done (loop $l
            (br_if $done (i32.eq (memory.grow (i32.const 1)) (i32.const -1)))
            br $l))
        memory.size))"#;

#[test]
fn a_code_mod_function_is_called_with_arguments_and_returns_its_result() {
    mount(ModSet::new().with(coded("adder", ADD)), |ctx| {
        let out = with_code(ctx, |code| {
            assert_eq!(code.mods().collect::<Vec<_>>(), ["adder"]);
            let add = code.function("adder", "add").expect("exported");
            let nothing = code.function("adder", "nothing").expect("exported");
            assert!(code.function("adder", "missing").is_none());
            assert!(code.function("nobody", "add").is_none());
            (
                code.call(add, &[ModValue::I32(40), ModValue::I32(2)]),
                code.call(nothing, &[]),
            )
        });
        let (sum, none) = out.expect("a mod has code");
        assert_eq!(sum, Ok(Some(ModValue::I32(42))));
        assert_eq!(none, Ok(None));
    });
}

#[test]
fn with_code_is_none_when_no_mod_has_code() {
    let data = ModPack::from_files(
        "data",
        [("mod.json", r#"{"format":"ponko-mod","version":1,"id":"data"}"#)],
    )
    .unwrap();
    mount(ModSet::new().with(data), |ctx| {
        assert!(with_code(ctx, |_| ()).is_none());
    });
}

#[test]
fn arguments_that_do_not_fit_are_an_error_naming_the_mod_not_a_crash() {
    mount(ModSet::new().with(coded("adder", ADD)), |ctx| {
        let err = with_code(ctx, |code| {
            let add = code.function("adder", "add").unwrap();
            code.call(add, &[ModValue::F32(1.0)]).unwrap_err()
        })
        .unwrap();
        assert_eq!(err.mod_id(), "adder");
        assert!(matches!(err.problem(), ModProblem::Trap(_)), "{err}");
    });
}

#[test]
fn every_mods_hook_is_found_in_load_order() {
    let answer = |n: i32| format!(r#"(module (func (export "hook") (result i32) i32.const {n}))"#);
    let set = ModSet::new()
        .with(coded("first", &answer(1)))
        .with(coded("silent", "(module)"))
        .with(coded("second", &answer(2)));
    mount(set, |ctx| {
        let answers = with_code(ctx, |code| {
            let hooks: Vec<_> = code.exporting("hook").map(|(id, f)| (id.to_string(), f)).collect();
            hooks
                .into_iter()
                .map(|(id, f)| (id, code.call(f, &[]).unwrap()))
                .collect::<Vec<_>>()
        })
        .unwrap();
        assert_eq!(
            answers,
            [
                ("first".to_string(), Some(ModValue::I32(1))),
                ("second".to_string(), Some(ModValue::I32(2)))
            ]
        );
    });
}

#[test]
fn an_endless_loop_is_stopped_by_fuel_and_the_mod_stays_callable() {
    let set = ModSet::new()
        .with(coded("spinner", SPIN))
        .with(coded("adder", ADD))
        .code_limits(CodeLimits {
            fuel_per_call: 100_000,
            ..CodeLimits::default()
        });
    mount(set, |ctx| {
        with_code(ctx, |code| {
            let spin = code.function("spinner", "spin").unwrap();
            let err = code.call(spin, &[]).unwrap_err();
            assert_eq!(err.mod_id(), "spinner");
            assert!(
                matches!(err.problem(), ModProblem::Trap(why) if why.contains("fuel")),
                "{err}"
            );
            assert_eq!(code.fuel_left(), 0);
            // Each call gets its fuel again; other mods are untouched.
            assert!(code.call(spin, &[]).is_err());
            let add = code.function("adder", "add").unwrap();
            assert_eq!(code.call(add, &[1.into(), 2.into()]), Ok(Some(ModValue::I32(3))));
        })
        .unwrap();
    });
}

#[test]
fn memory_grows_only_to_the_limit() {
    let set = ModSet::new().with(coded("hoarder", HOARD)).code_limits(CodeLimits {
        memory_bytes: 4 * 65536,
        ..CodeLimits::default()
    });
    mount(set, |ctx| {
        let pages = with_code(ctx, |code| {
            let hoard = code.function("hoarder", "hoard").unwrap();
            code.call(hoard, &[])
        })
        .unwrap();
        assert_eq!(
            pages,
            Ok(Some(ModValue::I32(4))),
            "four 64 KiB pages, then memory.grow fails"
        );
    });
}

#[test]
fn a_module_asking_for_more_memory_than_the_limit_is_refused_at_mount() {
    let big = "(module (memory 512))"; // 32 MiB, over the default 16 MiB
    let err = mount_err(ModSet::new().with(coded("big", big)));
    assert_eq!(err.mod_id(), "big");
    assert!(
        matches!(err.problem(), ModProblem::Code(why) if why.contains("start")),
        "{err}"
    );
}

#[test]
fn a_start_function_that_spins_is_refused_at_mount() {
    let err = mount_err(ModSet::new().with(coded("busy", "(module (func $s (loop $l br $l)) (start $s))")));
    assert!(
        matches!(err.problem(), ModProblem::Code(why) if why.contains("fuel")),
        "{err}"
    );
}

#[test]
fn a_module_that_imports_anything_but_the_host_functions_is_refused() {
    let wasi = r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#;
    let err = mount_err(ModSet::new().with(coded("sneaky", wasi)));
    assert_eq!((err.mod_id(), err.file()), ("sneaky", Some("code/sneaky.wasm")));
    assert!(
        matches!(err.problem(), ModProblem::Code(why) if why.contains("fd_write")),
        "{err}"
    );
    // A host function with the wrong type can't be linked either.
    let wrong = r#"(module (import "ponko" "dt" (func (result i64))))"#;
    assert!(matches!(
        mount_err(ModSet::new().with(coded("wrong", wrong))).problem(),
        ModProblem::Code(_)
    ));
}

#[test]
fn bytes_that_are_not_webassembly_or_a_missing_module_are_refused() {
    let manifest = r#"{"format":"ponko-mod","version":1,"id":"junk","code":"code.wasm"}"#;
    let junk = ModPack::from_files("junk", [("mod.json", manifest), ("code.wasm", "not wasm")]).unwrap();
    assert!(matches!(
        mount_err(ModSet::new().with(junk)).problem(),
        ModProblem::Code(_)
    ));
    let gone = ModPack::from_files("junk", [("mod.json", manifest)]).unwrap();
    let err = mount_err(ModSet::new().with(gone));
    assert_eq!(err.file(), Some("code.wasm"));
}

#[test]
fn a_bad_module_mounts_nothing_not_even_the_good_mods_with_it() {
    let set = ModSet::new()
        .with(coded("adder", ADD))
        .with(coded("big", "(module (memory 512))"));
    let err = mount_err(set);
    assert_eq!(err.mod_id(), "big");
}

#[test]
fn the_host_functions_log_and_dt_reach_the_mod() {
    let talker = r#"(module
        (import "ponko" "log" (func $log (param i32 i32)))
        (import "ponko" "dt" (func $dt (result f32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "hello from a mod")
        (func (export "talk") (result f32) (call $log (i32.const 0) (i32.const 16)) (call $dt)))"#;
    mount(ModSet::new().with(coded("talker", talker)), |ctx| {
        let dt = ctx.time.dt();
        let (out, log) = with_code(ctx, |code| {
            let talk = code.function("talker", "talk").unwrap();
            (code.call(talk, &[]), code.take_log())
        })
        .unwrap();
        assert_eq!(out, Ok(Some(ModValue::F32(dt))));
        assert_eq!(log, [("talker".to_string(), "hello from a mod".to_string())]);
    });
}

#[test]
fn the_code_is_part_of_the_fingerprint() {
    let fingerprint = |wat: &str| {
        let mut f = 0;
        mount(ModSet::new().with(coded("adder", wat)), |ctx| {
            f = ctx.world.resource::<Mods>().fingerprint();
        });
        f
    };
    assert_ne!(fingerprint(ADD), fingerprint(&ADD.replace("i32.add", "i32.sub")));
}
