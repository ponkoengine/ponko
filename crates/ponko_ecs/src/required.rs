//! Required components: a component names the components an entity with it
//! needs, with their starting values, and every spawn and insert that gives
//! an entity the component adds those it lacks. The user-facing rules are
//! on [`Requires`]; the decision and its measurements in `ARCHITECTURE.md`
//! and `crates/ponko_bench/REQUIRED.md`.
//!
//! Where it hooks in: requirements live with the component hooks (in the
//! world's boxed `Hooks`) and set the same `World::hooked` flag. A bundle
//! that lacks a component it requires is cached with the archetype of its
//! components and the required ones, and tagged like a bundle with
//! `on_add` hooks, so it spawns through the same cold path, which makes the
//! missing values first. Bundles that lack nothing, and worlds without
//! requirements, spawn as before. An insert tests the `hooked` flag once
//! more, before it moves anything, and only a hooked world looks up
//! whether the component requires something; the dynamic (C) spawns test
//! the archetype's flag byte they already tested for hooks.

use crate::bundle::Component;
use crate::hooks::{Make, Template};
use crate::raw::RawWorld;
use crate::storage::ComponentId;
use crate::world::World;

/// What a component requires: passed to [`Component::requires`], which
/// calls [`add`](Self::add) for each component an entity with it needs.
///
/// The rules:
///
/// - **When.** Every kind of spawn (typed, batch, [`Commands`](crate::Commands),
///   saved-world, dynamic and C) and every insert that gives an entity the
///   component adds the required components the entity lacks, in the same
///   structural change: the entity never exists without them, and `on_add`
///   hooks see them all.
/// - **What's given wins.** A component in the bundle, or one the entity
///   already has, keeps its value: requirements only fill gaps.
/// - **Through.** Required components bring what they require in turn.
///   The nearest requirement's value wins: one the spawned component names
///   itself beats one a required component names; of two at the same
///   depth, the one of the component earlier in the bundle. Cycles are
///   fine (each component is added once).
/// - **Only arriving.** Removing a required component later is allowed
///   (it is a normal component), and a requirement registered after
///   entities exist doesn't change them: it applies from their next spawn
///   or insert of the component on.
///
/// ```
/// use ponko_ecs::{Component, Requires, World};
///
/// #[derive(Component, Default)]
/// struct Transform(f32, f32);
/// #[derive(Component)]
/// struct Visible(bool);
/// // A sprite needs a transform and starts visible.
/// struct Sprite(u32);
/// impl Component for Sprite {
///     fn requires(r: &mut Requires<'_>) {
///         r.add(Transform::default).add(|| Visible(true));
///     }
/// }
///
/// let mut world = World::new();
/// let e = world.spawn((Sprite(7),));
/// assert!(world.has::<Transform>(e) && world.get::<Visible>(e).unwrap().0);
/// ```
///
/// The derive writes the same with `#[component(requires(Transform,
/// Visible = Visible(true)))]`: a type alone uses its `Default`.
///
/// **Cost.** Nothing for bundles and worlds that don't use it: a spawn of
/// a bundle that lacks nothing runs the same code as without requirements.
/// A spawn that fills gaps runs a constructor per missing component, much
/// like spawning them in the bundle. Numbers: `crates/ponko_bench/REQUIRED.md`.
pub struct Requires<'w> {
    world: &'w mut World,
    of: ComponentId,
}

impl<'w> Requires<'w> {
    pub(crate) fn new(world: &'w mut World, of: ComponentId) -> Self {
        Requires { world, of }
    }

    /// Requires component `R`, made with `make` where an entity lacks it:
    /// `R::default` for its default, or a closure for another value
    /// (`|| Health(100)`). Requiring the component itself does nothing;
    /// requiring `R` again replaces its constructor.
    pub fn add<R: Component>(&mut self, make: fn() -> R) -> &mut Self {
        let r = self.world.register::<R>();
        self.world.add_requirement(self.of, r, Make::rust(make), None);
        self
    }
}

impl World {
    /// Makes component `T` require component `R`, made with `make`, in
    /// this world: from now on every spawn and insert that gives an entity
    /// a `T` also gives it an `R` if it lacks one. For types you don't own
    /// (an engine or library component); your own say it with
    /// `#[component(requires(..))]` or [`Component::requires`], which
    /// every world uses. The rules are on [`Requires`]; entities that have
    /// a `T` already are left as they are.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component)]
    /// struct Enemy;
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Loot(u32);
    ///
    /// let mut world = World::new();
    /// world.require::<Enemy, _>(|| Loot(5));
    /// let e = world.spawn((Enemy,));
    /// assert_eq!(world.get::<Loot>(e), Some(&Loot(5)));
    /// ```
    pub fn require<T: Component, R: Component>(&mut self, make: fn() -> R) {
        let of = self.register::<T>();
        let r = self.register::<R>();
        self.add_requirement(of, r, Make::rust(make), None);
    }

    /// Whether component `of` requires `required` (directly; not through
    /// another component). For tools and tests.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component, Default)]
    /// struct Position(f32);
    /// #[derive(Component)]
    /// #[component(requires(Position))]
    /// struct Walker;
    ///
    /// let mut world = World::new();
    /// let walker = world.register::<Walker>();
    /// let position = world.register::<Position>();
    /// assert!(world.requires(walker, position));
    /// assert!(!world.requires(position, walker));
    /// ```
    #[must_use]
    pub fn requires(&self, of: ComponentId, required: ComponentId) -> bool {
        self.hooks.has_requirement(of, required)
    }

    /// Records a requirement and places what the world cached again.
    pub(crate) fn add_requirement(
        &mut self,
        of: ComponentId,
        required: ComponentId,
        make: Make,
        bytes: Option<Template>,
    ) {
        if of == required {
            return;
        }
        self.hooks.add_requirement(of, required, make, bytes);
        self.hooks_changed();
    }
}

impl RawWorld<'_> {
    /// Makes component `of` require component `required` in this world,
    /// with a copy of the bytes at `default` as its starting value: the
    /// dynamic and C path of [`World::require`] (C: `ponko_component_require`).
    /// The bytes are copied now; each entity that gets `required` this way
    /// gets its own copy of them. The rules are on [`Requires`].
    ///
    /// # Safety
    /// Both ids are of this world, and `default` points to a valid value of
    /// component `required` (its size in bytes; any pointer for size 0).
    ///
    /// # Panics
    /// If `required` has a destructor (a Rust type that needs `Drop`):
    /// copies of its bytes would be dropped once each. Use
    /// [`World::require`] with a constructor for those.
    pub unsafe fn require_dynamic(&mut self, of: ComponentId, required: ComponentId, default: *const u8) {
        let info = self.0.component_info(required);
        assert!(
            !info.needs_drop(),
            "require_dynamic: component `{}` has a destructor, so it can't be copied from bytes; use World::require with a constructor",
            info.name()
        );
        let size = info.layout().size();
        let bytes: Box<[u8]> = if size == 0 {
            Box::new([])
        } else {
            // SAFETY: the caller's value is `size` readable bytes.
            unsafe { std::slice::from_raw_parts(default, size) }.into()
        };
        let bytes = Template::new(bytes);
        let make = Make::bytes(bytes.as_ptr());
        self.0.add_requirement(of, required, make, Some(bytes));
    }
}
