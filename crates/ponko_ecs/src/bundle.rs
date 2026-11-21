//! Bundles: tuples of components spawned together.

use std::mem::ManuallyDrop;

use crate::required::Requires;
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
/// # Required components
///
/// A component can name components an entity with it needs, with the
/// values they start with: spawn the thing and get what it needs. Every
/// spawn and insert that gives an entity the component adds those it
/// lacks (and what they require in turn); values given in the bundle win.
///
/// ```
/// use ponko_ecs::{Component, World};
///
/// #[derive(Component, Default, Debug, PartialEq)]
/// struct Position(f32, f32);
/// #[derive(Component, Debug, PartialEq)]
/// struct Health(u32);
///
/// #[derive(Component)]
/// #[component(requires(Position, Health = Health(100)))]
/// struct Enemy;
///
/// let mut world = World::new();
/// let e = world.spawn((Enemy,));
/// assert_eq!(world.get::<Position>(e), Some(&Position(0.0, 0.0))); // `Default`
/// assert_eq!(world.get::<Health>(e), Some(&Health(100)));
/// let tough = world.spawn((Enemy, Health(500))); // the bundle's value wins
/// assert_eq!(world.get::<Health>(tough), Some(&Health(500)));
/// ```
///
/// By hand, [`requires`](Component::requires) says the same. See
/// [`Requires`] for the rules, and [`World::require`] to add a
/// requirement to a type you don't own.
///
/// The derive takes no other options; unknown ones are rejected:
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
/// [`World::require`]: crate::World::require
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a component",
    note = "add `#[derive(Component)]` to the type, or `impl Component for {Self} {{}}`"
)]
pub trait Component: Send + Sync + 'static {
    /// The components an entity with this one needs, and how to make each
    /// one's value: called once per world, when the world first sees this
    /// type. The default requires nothing. `#[component(requires(..))]`
    /// writes it for the derive.
    ///
    /// ```
    /// use ponko_ecs::{Component, Requires, World};
    ///
    /// #[derive(Component, Default)]
    /// struct Velocity(f32, f32);
    /// struct Body;
    /// impl Component for Body {
    ///     fn requires(r: &mut Requires<'_>) {
    ///         r.add(Velocity::default);
    ///     }
    /// }
    ///
    /// let mut world = World::new();
    /// let e = world.spawn((Body,));
    /// assert!(world.has::<Velocity>(e));
    /// ```
    fn requires(_required: &mut Requires<'_>) {}

    /// How a world snapshot copies this type's values: written by
    /// `#[derive(Component)]` for a type that is `Clone`. Not part of the
    /// API: a hand-written impl leaves it alone and calls
    /// [`World::register_clone`] instead.
    #[doc(hidden)]
    fn __snapshot_clone() -> Option<crate::snapshot::CloneFn> {
        None
    }
}

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
/// `component_ids`, with a pointer to a value whose ownership the callee
/// takes and that value's size in bytes (`size_of` of its type).
pub unsafe trait Bundle: Send + Sync + 'static {
    #[doc(hidden)]
    const SEAL: Seal;
    #[doc(hidden)]
    fn component_ids(world: &mut World, out: &mut Vec<ComponentId>);
    #[doc(hidden)]
    unsafe fn write(self, f: &mut impl FnMut(usize, *const u8, usize));
}

macro_rules! bundle_tuple {
    ($($t:ident $i:tt),*) => {
        unsafe impl<$($t: Component),*> Bundle for ($($t,)*) {
            const SEAL: Seal = Seal;
            fn component_ids(world: &mut World, out: &mut Vec<ComponentId>) {
                $(out.push(world.register::<$t>());)*
            }
            unsafe fn write(self, f: &mut impl FnMut(usize, *const u8, usize)) {
                let me = ManuallyDrop::new(self);
                // Each size is a constant here, so the copies `f` makes are
                // plain moves of known width, not calls to `memcpy`.
                $(f($i, (&me.$i as *const $t).cast(), std::mem::size_of::<$t>());)*
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
