### Added
- Required components: a component names the components an entity with it
  needs, with their starting values, and every spawn and insert that gives
  an entity the component adds the ones it lacks (every kind of spawn,
  commands, loading a save, the dynamic, C and C++ paths), in the same
  structural change. Given values win; requirements go through (the
  nearest one's value wins). Declare them with
  `#[derive(Component)] #[component(requires(Transform, Health = Health(100)))]`
  or by writing `Component::requires` (`Requires::add`, a new provided
  method with an empty default, so existing impls are unchanged). At run
  time: `World::require::<T, R>(make)` for types you don't own,
  `RawWorld::require_dynamic` (a byte template) for dynamic components,
  `World::requires` to ask. Bundles that lack nothing and worlds without
  requirements spawn through the same code as before (measurements in
  `crates/ponko_bench/REQUIRED.md`). See "Required components" in the
  guide's ECS chapter.
- C and C++: `ponko_component_require(world, component, required, value)`
  and `world.require<T>(value)`, copying a plain value's bytes.
- `ponko_bench`: `ecs/*/ponko-requires-elsewhere` and `ecs/*/ponko-required`
  rows.

### Changed
- Physics bodies bring what they need: `d2::RigidBody` and `d3::RigidBody`
  require a transform and a `Velocity` (an entity without them gets one at
  the origin, at rest), and `d2::Collider` and `d3::Collider` require a
  body, `RigidBody::fixed()` unless one is given. A spawn names only what
  differs (`(Model::new(look), Collider::mesh(solid))` is fixed ground);
  the tutorials lost their `Velocity::default()` and `RigidBody::fixed()`
  lines. Every body now has a `Velocity` component, which reads as zero
  where one was missing before, so simulations step the same. An entity
  with a collider and no body, which physics ignored before, is now a
  fixed body (rapier's convention); saves that hold one load as one.
