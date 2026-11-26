A small 2D game, a Rust crate (`Cargo.toml`, `src/main.rs`), is built and
running headless and paused, with its control endpoint on; reach it with
the ponko-mcp tools. A guard (`Guard`, `Transform2D`) walks the route in
`assets/patrol.json`: from waypoint to waypoint, back to the first after
the last, at the speed given there (pixels per second). `Guard.laps`
counts the laps it finished; reaching a waypoint sends a `Reached` event.

Bug report: "the guard stops at the first corner and never finishes a lap."

Goal: fix the game's code so the guard walks the route lap after lap,
turning only at the waypoints and never moving faster than its speed.
Don't change `assets/patrol.json`. After changing the code, rebuild and
restart the game: `quit_instance`, then `launch` with `cwd` the game's
folder and `release: false` (a debug build takes seconds; a release build
would compile the engine again). `submit_task` builds and starts the game
from your files itself.
