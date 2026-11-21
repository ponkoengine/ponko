//! Component hooks from C: `ponko_component_on_add`, `_on_replace` and
//! `_on_remove` run inside spawns, sets, removes and despawns, see the
//! values Rust's hooks see, and do their work through `ponko_hook_*`.
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;
use ponko_ffi::*;

/// What the hooks saw: (kind, entity, value, new value).
#[derive(Default)]
struct Log {
    seen: Vec<(u32, u64, u32, Option<u32>)>,
    /// Inside a hook: what the world's own calls returned.
    busy: Option<PonkoStatus>,
    world: Option<usize>,
}

unsafe extern "C" fn record(_: *mut PonkoHookWorld, event: *const PonkoHookEvent, user: *mut c_void) {
    unsafe {
        let log = &mut *user.cast::<Log>();
        let e = &*event;
        assert_eq!(e.size, 4);
        let value = e.value.cast::<u32>().read();
        let new = (!e.new_value.is_null()).then(|| e.new_value.cast::<u32>().read());
        log.seen.push((e.kind, e.entity, value, new));
        if let Some(w) = log.world {
            let mut n = 0;
            log.busy = Some(ponko_world_len(w as *mut PonkoWorld, &mut n));
        }
    }
}

unsafe fn register(w: *mut PonkoWorld, name: &std::ffi::CStr, size: usize) -> u32 {
    let mut id = 0;
    assert_eq!(
        unsafe { ponko_component_register(w, name.as_ptr(), size, size.max(1), &mut id) },
        PONKO_OK
    );
    id
}

unsafe fn spawn_u32(w: *mut PonkoWorld, id: u32, v: u32) -> u64 {
    let value = PonkoValue {
        component: id,
        data: (&raw const v).cast(),
        size: 4,
    };
    let mut e = 0;
    assert_eq!(unsafe { ponko_spawn(w, &value, 1, &mut e) }, PONKO_OK);
    e
}

#[test]
fn hooks_from_c_run_on_add_replace_and_remove_with_the_values_in_place() {
    unsafe {
        let w = ponko_world_new();
        let hp = register(w, c"Hp", 4);
        let mut log = Log {
            world: Some(w as usize),
            ..Log::default()
        };
        let user = (&raw mut log).cast();
        assert_eq!(ponko_component_on_add(w, hp, Some(record), user, None), PONKO_OK);
        assert_eq!(ponko_component_on_replace(w, hp, Some(record), user, None), PONKO_OK);
        assert_eq!(ponko_component_on_remove(w, hp, Some(record), user, None), PONKO_OK);

        let e = spawn_u32(w, hp, 10);
        let twelve = 12u32;
        assert_eq!(ponko_set(w, e, hp, (&raw const twelve).cast(), 4), PONKO_OK);
        assert_eq!(ponko_despawn(w, e), PONKO_OK);
        let (add, replace, remove) = (
            PonkoHookKind::PONKO_HOOK_ADD as u32,
            PonkoHookKind::PONKO_HOOK_REPLACE as u32,
            PonkoHookKind::PONKO_HOOK_REMOVE as u32,
        );
        assert_eq!(
            log.seen,
            vec![(add, e, 10, None), (replace, e, 10, Some(12)), (remove, e, 12, None)]
        );
        assert_eq!(log.busy, Some(PONKO_BUSY), "the world is busy inside a hook");
        ponko_world_free(w);
    }
}

unsafe extern "C" fn count_free(user: *mut c_void) {
    unsafe { *user.cast::<u32>() += 1 };
}

#[test]
fn freeing_the_world_frees_each_hooks_user_data_once() {
    unsafe {
        let w = ponko_world_new();
        let hp = register(w, c"Hp", 4);
        let mut freed = 0u32;
        let user = (&raw mut freed).cast();
        ponko_component_on_add(w, hp, Some(record_nothing), user, Some(count_free));
        ponko_component_on_remove(w, hp, Some(record_nothing), user, Some(count_free));
        assert_eq!(freed, 0);
        ponko_world_free(w);
        assert_eq!(freed, 2);
    }
}

unsafe extern "C" fn record_nothing(_: *mut PonkoHookWorld, _: *const PonkoHookEvent, _: *mut c_void) {}

/// Ids a hook works with.
struct Ids {
    hp: u32,
    shield: u32,
    loot: u32,
    spawned: u64,
}

/// on_add: doubles the new value in place, gives the entity a shield
/// later, and spawns a loot entity; reads the shield back if it has one.
unsafe extern "C" fn arm(hw: *mut PonkoHookWorld, event: *const PonkoHookEvent, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let e = &*event;
        let mut hp = 0u32;
        assert_eq!(ponko_hook_get(hw, e.entity, ids.hp, (&raw mut hp).cast(), 4), PONKO_OK);
        hp *= 2;
        assert_eq!(
            ponko_hook_write(hw, e.entity, ids.hp, (&raw const hp).cast(), 4),
            PONKO_OK
        );
        let mut alive = false;
        assert_eq!(ponko_hook_is_alive(hw, e.entity, &mut alive), PONKO_OK);
        assert!(alive);
        let mut shield = 0u32;
        assert_eq!(
            ponko_hook_get(hw, e.entity, ids.shield, (&raw mut shield).cast(), 4),
            PONKO_NOT_FOUND,
            "not there yet: structural changes wait"
        );
        let five = 5u32;
        let value = PonkoValue {
            component: ids.shield,
            data: (&raw const five).cast(),
            size: 4,
        };
        assert_eq!(ponko_hook_set_later(hw, e.entity, &value), PONKO_OK);
        let loot = PonkoValue {
            component: ids.loot,
            data: (&raw const five).cast(),
            size: 4,
        };
        assert_eq!(ponko_hook_spawn_later(hw, &loot, 1, &mut ids.spawned), PONKO_OK);
    }
}

/// on_remove of the shield: takes the loot away too.
unsafe extern "C" fn disarm(hw: *mut PonkoHookWorld, event: *const PonkoHookEvent, user: *mut c_void) {
    unsafe {
        let ids = &*user.cast::<Ids>();
        assert_eq!(ponko_hook_despawn_later(hw, ids.spawned), PONKO_OK);
        assert_eq!(ponko_hook_remove_later(hw, (*event).entity, ids.hp), PONKO_OK);
    }
}

#[test]
fn a_hook_writes_in_place_and_its_commands_apply_before_the_call_returns() {
    unsafe {
        let w = ponko_world_new();
        let mut ids = Ids {
            hp: register(w, c"Hp", 4),
            shield: register(w, c"Shield", 4),
            loot: register(w, c"Loot", 4),
            spawned: 0,
        };
        let user = (&raw mut ids).cast();
        ponko_component_on_add(w, ids.hp, Some(arm), user, None);
        ponko_component_on_remove(w, ids.shield, Some(disarm), user, None);
        let e = spawn_u32(w, ids.hp, 21);
        let (mut hp, mut shield, mut loot) = (0u32, 0u32, 0u32);
        assert_eq!(ponko_get(w, e, ids.hp, (&raw mut hp).cast(), 4), PONKO_OK);
        assert_eq!(hp, 42, "the hook doubled it in place");
        assert_eq!(ponko_get(w, e, ids.shield, (&raw mut shield).cast(), 4), PONKO_OK);
        assert_eq!(shield, 5, "the hook's set was applied before ponko_spawn returned");
        assert_eq!(ponko_get(w, ids.spawned, ids.loot, (&raw mut loot).cast(), 4), PONKO_OK);
        assert_eq!(ponko_remove(w, e, ids.shield), PONKO_OK);
        let mut alive = true;
        ponko_is_alive(w, ids.spawned, &mut alive);
        assert!(!alive, "the remove hook despawned the loot");
        assert_eq!(ponko_get(w, e, ids.hp, (&raw mut hp).cast(), 4), PONKO_NOT_FOUND);
        ponko_world_free(w);
    }
}

#[test]
fn hooks_refuse_converted_built_ins_and_bad_arguments() {
    unsafe {
        let w = ponko_world_new();
        let mut sprite = 0;
        assert_eq!(ponko_sprite_component(w, &mut sprite), PONKO_OK);
        assert_eq!(
            ponko_component_on_add(w, sprite, Some(record_nothing), null_mut(), None),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_component_on_add(w, 999, Some(record_nothing), null_mut(), None),
            PONKO_INVALID_ARGUMENT
        );
        let hp = register(w, c"Hp", 4);
        assert_eq!(
            ponko_component_on_add(w, hp, None, null_mut(), None),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_hook_get(null_mut(), 0, hp, null_mut(), 4), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_hook_write(null_mut(), 0, hp, null(), 4), PONKO_INVALID_ARGUMENT);
        ponko_world_free(w);
    }
}

unsafe extern "C" fn count_adds(_: *mut PonkoHookWorld, _: *const PonkoHookEvent, user: *mut c_void) {
    unsafe { *user.cast::<u32>() += 1 };
}

#[test]
fn hooks_from_c_run_for_spawns_made_by_a_game_system() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
        unsafe {
            let w = ponko_ctx_world(ctx);
            let mut id = 0;
            ponko_component_register(w, c"Hp".as_ptr(), 4, 4, &mut id);
            assert_eq!(ponko_component_on_add(w, id, Some(count_adds), user, None), PONKO_OK);
            let one = 1u32;
            let v = PonkoValue {
                component: id,
                data: (&raw const one).cast(),
                size: 4,
            };
            // A command, applied after the system: the hook runs then.
            assert_eq!(ponko_ctx_spawn_later(ctx, &v, 1), PONKO_OK);
        }
    }
    unsafe {
        let mut adds = 0u32;
        let game = ponko_game_new(c"hooks".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 2, Some(setup), (&raw mut adds).cast(), &mut world),
            PONKO_OK
        );
        assert_eq!(adds, 1);
        ponko_world_free(world);
    }
}
