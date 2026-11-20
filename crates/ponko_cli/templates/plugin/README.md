# {{project-name}}

A plugin for the [Ponko](https://ponko.dev/) game engine.

```rust,ignore
use ponko::prelude::*;

Game::new("My game", 1280, 720)
    .plugin({{crate_name}}::PulsePlugin)
    .setup(|ctx| {
        ctx.world.spawn(({{crate_name}}::Pulse::new(2.0),));
    })
    .run();
```

What it adds to a game:

| What | Name |
|---|---|
| Plugin | `{{crate_name}}::PulsePlugin` |
| Component (reflected) | `Pulse` |
| System | advances every `Pulse`, in `Phase::AfterPhysics` |
| Debug toggle | `{{crate_name}}.pulses` (`ctx.debug.set_toggle`, or the control endpoint's `debug.draw`) |
| Control method | `{{crate_name}}.set_rate` (ponko-mcp: `call_game_method`) |

```sh
cargo test    # the headless tests
cargo doc --open
```

## Versions

This crate follows semver. Each release names the Ponko versions it works
with:

| {{project-name}} | Ponko |
|---|---|
| 0.1 | 0.1 |

The guide's [Plugins for other crates](https://ponko.dev/guide/plugins.html)
says how to name, version and publish a Ponko plugin.
