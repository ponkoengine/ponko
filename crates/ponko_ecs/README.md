# ponko_ecs

The entity-component-system behind [Ponko](https://github.com/ponkoengine/ponko),
built for simulations with millions of entities: archetype storage, queries
that run as fast as a loop over arrays (and on all cores with the default
`parallel` feature), resources, commands, change detection and
runtime-registered components for other languages.

Most games use it through the `ponko` crate (`ctx.world`). Use this crate on
its own when you want only the ECS.

- API reference: <https://docs.rs/ponko_ecs/>
- Guide: <https://ponko.dev/guide/>
- Features: `parallel` (default, rayon), `derive` (default, `#[derive(Component)]`
  from `ponko_macros`), `serde` (`Entity` as serde data).

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
