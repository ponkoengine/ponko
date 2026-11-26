### Added
- `Ctx::try_load_collision_mesh` and `try_load_collision_mesh_with` return the error for a missing or broken file, as `try_load_mesh` does.
- `Assets::try_load_resource` and `try_load_strings` are the fallible twins of `load_resource` and `load_strings`.
- C++: `ctx.try_load_strings`, `ctx.try_load_collision_mesh` and `ctx.try_load_json<T>` return a `ponko::result` instead of throwing, and `ponko::result<void>` exists for loaders that return nothing. C already returns a status with `ponko_last_error_kind()` and the path for these calls.
- A table of every `load_*` and what it does on failure in STABILITY.md and the data-files chapter, and a test (`tests/loader_convention.rs`) that walks the API snapshots and fails when a new `load_*` has no `try_` twin (or is a `Result`-returning plain name that is not a documented runtime operation), or is missing from the table.

### Breaking changes
- The plain collision-mesh loaders panic instead of returning a `Result`.
  Instead: call `try_load_collision_mesh` or `try_load_collision_mesh_with` where you handled the error, or drop the `.unwrap()` / `.expect(..)` where a missing file is a bug.
  Before:
  ```rust
  let solid = ctx.load_collision_mesh("yard.glb").expect("the yard loads");
  let hull = ctx.load_collision_mesh_with("rock.glb", options)?;
  ```
  After:
  ```rust
  # let options = ponko::d3::CollisionMeshOptions::default();
  let solid = ctx.load_collision_mesh("yard.glb"); // panics with a `ponko:` message if it fails
  let hull = ctx.try_load_collision_mesh_with("rock.glb", options)?;
  ```
- `Assets::load_strings` panics instead of returning a `Result`.
  Instead: call `try_load_strings` where you handled the error, or drop the `?` / `.unwrap()` where a missing table is a bug.
  Before:
  ```rust
  ctx.assets.load_strings(&mut ctx.world, "lang/en.json")?;
  ```
  After:
  ```rust
  ctx.assets.load_strings(&mut ctx.world, "lang/en.json");
  // or, for a table that may be missing:
  ctx.assets.try_load_strings(&mut ctx.world, "lang/fr.json")?;
  ```
- `Assets::load_resource_json` and `load_component_json` are `load_reflected_resource_json` and `load_reflected_component_json`.
  One name per loader: they still return a `Result` and merge a file's
  fields into the reflected value, where `load_resource` replaces the whole
  resource. The old names are removed without a deprecated alias.
  Instead: rename the calls; arguments and results are the same.
  Before:
  ```rust
  ctx.assets.load_resource_json::<Rules>(&mut ctx.world, "rules.json")?;
  ctx.assets.load_component_json::<Stats>(&mut ctx.world, "stats.json", entity)?;
  ```
  After:
  ```rust
  # #[derive(Default)] struct Rules; #[derive(Default)] struct Stats;
  # let entity: Entity = todo!();
  ctx.assets.load_reflected_resource_json::<Rules>(&mut ctx.world, "rules.json")?;
  ctx.assets.load_reflected_component_json::<Stats>(&mut ctx.world, "stats.json", entity)?;
  ```
