//! Bundles: tuples of components spawned together.

use std::mem::ManuallyDrop;

use crate::seal::Seal;
use crate::storage::ComponentId;
use crate::world::World;

/// A type that entities can have: a column of the world's archetypes.
///
/// A type opts in, with the derive (feature `derive`, on by default):
///
/// ```
/// use ponko_ecs::{Component, World};
///
/// #[derive(Component)]
/// struct Health(f32);
///
/// let mut world = World::new();
/// let e = world.spawn((Health(10.0),));
/// assert_eq!(world.get::<Health>(e).unwrap().0, 10.0);
/// ```
///
/// or by hand, which is the same thing:
///
/// ```
/// use ponko_ecs::Component;
///
/// struct Health(f32);
/// impl Component for Health {}
/// ```
///
/// A component is `Send + Sync + 'static`: systems read it from several
/// threads. A generic type derives it for the arguments that make it one
/// (`#[derive(Component)] struct Tagged<T>(T);` is a component for every
/// `T: Send + Sync + 'static`).
///
/// Types don't qualify by accident. A tuple is a bundle of several
/// components, not one; a bare number, a `String`, a `Vec` or an [`Entity`]
/// says nothing about what it means to the entity: wrap it in a type that
/// does (`struct Target(Entity);`). Code that uses a type that isn't a
/// component doesn't compile:
///
/// ```compile_fail,E0277
/// use ponko_ecs::World;
///
/// let mut world = World::new();
/// world.spawn((5i32,)); // `i32` is not a component
/// ```
///
/// ```compile_fail,E0277
/// use ponko_ecs::{Component, World};
///
/// #[derive(Component)]
/// struct Pos(f32, f32);
///
/// let mut world = World::new();
/// let e = world.spawn((Pos(0.0, 0.0),));
/// world.insert(e, String::from("player")); // `String` is not a component
/// ```
///
/// ```compile_fail,E0277
/// use ponko_ecs::World;
///
/// struct Pos(f32, f32); // no derive
///
/// let mut world = World::new();
/// world.query::<&Pos>().for_each(|_| {}); // `Pos` is not a component
/// ```
///
/// The derive takes no options yet; `#[component(..)]` is reserved for them
/// and rejected:
///
/// ```compile_fail
/// #[derive(ponko_ecs::Component)]
/// #[component(storage = "sparse")] // unknown component option `storage`
/// struct Pos(f32, f32);
/// ```
///
/// Resources ([`World::insert_resource`]) and events don't need this trait.
///
/// [`Entity`]: crate::Entity
/// [`World::insert_resource`]: crate::World::insert_resource
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a component",
    note = "add `#[derive(Component)]` to the type, or `impl Component for {Self} {{}}`"
)]
pub trait Component: Send + Sync + 'static {}

/// A tuple of components that are spawned together: `(Pos, Vel)`, `(Pos,)`.
///
/// Implemented for tuples of up to 12 components, by this crate only: the
/// trait is sealed.
///
/// ```
/// use ponko_ecs::{Component, World};
/// #[derive(Component)]
/// struct Pos(f32);
/// #[derive(Component)]
/// struct Vel(f32);
/// let mut world = World::new();
/// // A bundle is a tuple of components; one component needs the trailing comma.
/// world.spawn((Pos(0.0), Vel(1.0)));
/// world.spawn((Pos(5.0),));
/// assert_eq!(world.query::<&Pos>().iter().count(), 2);
/// assert_eq!(world.query::<(&Pos, &Vel)>().iter().count(), 1);
/// ```
///
/// # Safety
/// `write` must call `f` exactly once per element, in the order of
/// `component_ids`, with a pointer to a value whose ownership the callee takes.
pub unsafe trait Bundle: Send + Sync + 'static {
    #[doc(hidden)]
    const SEAL: Seal;
    #[doc(hidden)]
    fn component_ids(world: &mut World, out: &mut Vec<ComponentId>);
    #[doc(hidden)]
    unsafe fn write(self, f: &mut impl FnMut(usize, *const u8));
}

macro_rules! bundle_tuple {
    ($($t:ident $i:tt),*) => {
        unsafe impl<$($t: Component),*> Bundle for ($($t,)*) {
            const SEAL: Seal = Seal;
            fn component_ids(world: &mut World, out: &mut Vec<ComponentId>) {
                $(out.push(world.register::<$t>());)*
            }
            unsafe fn write(self, f: &mut impl FnMut(usize, *const u8)) {
                let me = ManuallyDrop::new(self);
                $(f($i, (&me.$i as *const $t).cast());)*
            }
        }
    };
}
bundle_tuple!(A 0);
bundle_tuple!(A 0, B 1);
bundle_tuple!(A 0, B 1, C 2);
bundle_tuple!(A 0, B 1, C 2, D 3);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
