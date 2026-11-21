//! Hooks registered by component id (`RawWorld::on_add_dynamic` and its
//! kin): the path C and C++ hooks take.

use std::sync::{Arc, Mutex};

use ponko_ecs::{Component, ComponentId, Entity, HookWorld, World};

#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Hp(i32);
#[derive(Component)]
struct Loot;

type Seen = Arc<Mutex<Vec<(&'static str, Entity, i32, Option<i32>)>>>;

fn recorder(seen: &Seen, what: &'static str) -> ponko_ecs::DynamicHook {
    let seen = seen.clone();
    Arc::new(move |w: &mut HookWorld, e: Entity, id: ComponentId, new: *const u8| {
        let now = w
            .get_dynamic(e, id)
            .map(|p| unsafe { p.cast::<i32>().read() })
            .unwrap_or(-1);
        let new = (!new.is_null()).then(|| unsafe { new.cast::<i32>().read() });
        seen.lock().unwrap().push((what, e, now, new));
    })
}

fn rust_add(w: &mut HookWorld, e: Entity) {
    let v = w.get::<Hp>(e).unwrap().0;
    w.resource_mut::<Vec<(&'static str, i32)>>().push(("rust add", v));
}

#[test]
fn dynamic_hooks_see_the_value_in_the_world_and_the_new_one() {
    let mut world = World::new();
    let id = world.register::<Hp>();
    let seen = Seen::default();
    world.raw().on_add_dynamic(id, recorder(&seen, "add"));
    world.raw().on_replace_dynamic(id, recorder(&seen, "replace"));
    world.raw().on_remove_dynamic(id, recorder(&seen, "remove"));
    let e = world.spawn((Hp(3),));
    world.insert(e, Hp(7));
    world.despawn(e);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("add", e, 3, None), ("replace", e, 3, Some(7)), ("remove", e, 7, None),]
    );
}

#[test]
fn dynamic_and_rust_hooks_run_in_the_order_they_were_registered() {
    let mut world = World::new();
    world.insert_resource(Vec::<(&'static str, i32)>::new());
    let id = world.register::<Hp>();
    let order = Arc::new(Mutex::new(Vec::new()));
    let first = order.clone();
    world.raw().on_add_dynamic(
        id,
        Arc::new(move |w: &mut HookWorld, _, _, _| {
            assert!(
                w.resource::<Vec<(&'static str, i32)>>().is_empty(),
                "runs before the Rust hook"
            );
            first.lock().unwrap().push("dynamic");
        }),
    );
    world.on_add::<Hp>(rust_add);
    world.spawn((Hp(1),));
    assert_eq!(*order.lock().unwrap(), vec!["dynamic"]);
    assert_eq!(*world.resource::<Vec<(&'static str, i32)>>(), vec![("rust add", 1)]);
}

#[test]
fn a_dynamic_hook_writes_in_place_and_records_commands() {
    let mut world = World::new();
    let id = world.register::<Hp>();
    world.raw().on_add_dynamic(
        id,
        Arc::new(|w: &mut HookWorld, e: Entity, id: ComponentId, _| {
            let p = w.get_dynamic_mut(e, id).expect("the entity has it");
            unsafe { p.cast::<i32>().write(p.cast::<i32>().read() * 10) };
            w.commands().spawn((Loot,));
        }),
    );
    let e = world.spawn((Hp(4),));
    assert_eq!(world.get::<Hp>(e), Some(&Hp(40)), "written before spawn returned");
    assert_eq!(world.query::<&Loot>().iter().count(), 1, "the hook's spawn was applied");
}

#[test]
#[should_panic(expected = "isn't registered in this world")]
fn a_dynamic_hook_needs_a_registered_component() {
    let mut world = World::new();
    let other = {
        let mut w = World::new();
        w.register::<Loot>();
        w.register::<Hp>()
    };
    world.raw().on_add_dynamic(other, Arc::new(|_, _, _, _| {}));
}
