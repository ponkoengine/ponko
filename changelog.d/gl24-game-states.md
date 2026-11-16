### Added
- Game states (`ponko::state`): `Game::state(initial)` gives a game a state
  machine of its own `enum` (title, loading, playing, paused), kept in the
  reflected `State<S>` resource, so world saves, tools and the lockstep
  checksum carry it. `ctx.set_state`, `ctx.push_state` and `ctx.pop_state`
  ask for changes, which apply only between simulation ticks: at the end of
  the tick that asks (or before the next tick, when asked for in setup or
  by a frame system). `Game::on_enter` and `Game::on_exit` run work when a
  state starts and ends; `push_state` puts a pause over play without
  leaving it. Guide: "Game states".
- Run conditions: `IntoSystem::run_if` with `in_state(..)`, `in_stack(..)`,
  a closure `|ctx: &Ctx| -> bool`, and `Condition::not` / `or`. State
  conditions declare a read of the state and keep view systems in parallel
  batches; a closure condition makes a view system run alone
  (`Alone::Condition`, diagnostics reason `condition`). Systems without a
  condition show no measured overhead above the noise (`conditions/` benchmark).
- `Game::has_state`, and `ctx.state::<S>()` to read the current state.
- Timed states: `State::ticks()` counts the ticks run with the current
  state on top (reset by every change, stopped while a state is pushed
  over it, saved with it), and `ctx.seconds_in_state::<S>()` gives it in
  seconds. Every set is a change, the same value included (a restart), so
  a fade or a death animation reads its time here instead of keeping a
  timer in the state value. Loading a world restores the saved state
  without on-exit or on-enter work.
- C and C++: `ponko_game_state`, `ponko_game_on_enter`, `ponko_game_on_exit`,
  `ponko_game_add_system_in_state`, `ponko_game_add_system_in_stack`,
  `ponko_ctx_state`, `ponko_ctx_state_in_stack`, `ponko_ctx_state_ticks`,
  `ponko_ctx_set_state`, `ponko_ctx_push_state`, `ponko_ctx_pop_state`,
  and run conditions as callbacks: `ponko_game_add_system_if` with a
  `PonkoConditionFn`; in C++ `game::state`, `on_enter`, `on_exit`,
  `add_in_state`, `add_in_stack`, `add_if` (any callable) and
  `ctx::state<S>()`, `set_state`, `push_state`, `pop_state`,
  `in_state_stack`, `state_ticks`.
- The Coin Dash and Ember Hop reference games use engine states, including
  a pause pushed over play; Ember Hop's dying and fading screens are timed
  states, and its tests check that on-enter and on-exit work runs once per
  change of screen.
