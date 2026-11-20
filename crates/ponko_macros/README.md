# ponko_macros

Derive macros for [Ponko](https://ponko.dev/). Don't depend on this crate
directly: use them through `ponko` (`ponko::prelude::*`) or `ponko_ecs`,
which re-export each derive next to the trait it implements.

```rust
use ponko::prelude::*;

#[derive(Component)]
struct Health(f32);
```

- `#[derive(Component)]` writes `impl Component for Type {}`. The type must
  be `Send + Sync + 'static`; a generic type is a component for the
  arguments that make it one. `#[component(requires(Transform, Health =
  Health(100.0)))]` names the components an entity with it needs: a type
  alone starts at its `Default`, `Type = expression` at that value.

The generated code names the trait through whichever of `ponko` and
`ponko_ecs` the crate depends on, under the name its `Cargo.toml` gives it.

Licensed under either of MIT or Apache-2.0, at your option.
