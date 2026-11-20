//! `#[derive(Component)]` in a crate that depends on `ponko_ecs` under
//! another name (`ecs` here): the generated impl must use that name.

use ecs::{Component, World};

#[derive(Component, Debug, PartialEq)]
struct Speed(f32);

#[derive(ecs::Component)]
struct Tagged<T>(T);

#[test]
fn a_renamed_dependency_derives_components() {
    let mut world = World::new();
    let e = world.spawn((Speed(2.0), Tagged(1u8)));
    assert_eq!(world.get::<Speed>(e), Some(&Speed(2.0)));
    assert_eq!(world.get::<Tagged<u8>>(e).map(|t| t.0), Some(1));
}

#[derive(Component, Debug, PartialEq, Default)]
struct Mass(f32);

#[derive(Component)]
#[component(requires(Mass, Speed = Speed(1.0)))]
struct Ball;

#[test]
fn a_renamed_dependency_derives_requirements() {
    let mut world = World::new();
    let e = world.spawn((Ball,));
    assert_eq!(world.get::<Mass>(e), Some(&Mass(0.0)));
    assert_eq!(world.get::<Speed>(e), Some(&Speed(1.0)));
}
