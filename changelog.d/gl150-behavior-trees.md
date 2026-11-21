### Added
- Behaviour trees: a brain can run several actions at once. A `parallel`
  node may now hold `do` nodes in several children (move *and* aim); their
  actions run side by side and each ends on its own. `Brain::actions`,
  `started_action`, `action_param`, `succeed_action` and `fail_action`
  name one action; the one-action calls (`action`, `started`, `status`,
  `param`, `succeed`, `fail`) are about the first, so existing trees and
  systems behave as before. Two children of one `parallel` that could run
  an action of the same name at once are refused when the tree compiles.
  A brain's JSON lists `"actions"` when it does several, and its saved
  memory keeps the extra ones (`"also"`).
- C and C++ world brains and signals: `ponko_ctx_ai_signal` (with `_num`,
  `_entity`, `_vec2`, `_vec3` values; `PONKO_AI_EVERY_BRAIN` for all)
  sends `ai::Signal` events to the brains of entities;
  `ponko_ctx_ai_load_tree`, `ponko_ctx_ai_add_tree`,
  `ponko_ctx_ai_add_brain`, `ponko_ctx_ai_think`, `ponko_ctx_ai_action`,
  `_started`, `_finish`, `_param_num` and the blackboard calls
  (`ponko_ctx_ai_set_num` and friends, `ponko_ctx_ai_num`,
  `ponko_ctx_ai_flag`). C++: `ponko::ai::signal`, `think(ctx)`,
  `add_brain` and the rest in `namespace ai`.
- C and C++ native brains with several actions: `ponko_brain_action_at`,
  `ponko_brain_action_started`, `ponko_brain_finish_action`,
  `ponko_brain_action_param_num` (`brain::action_at`, `started_action`,
  `action_param`, `succeed_action`, `fail_action`).

### Changed
- A `parallel` node with `do` nodes in more than one child, refused
  before, now compiles and runs the actions together.
- C++ `ai::brain::doing` is true for any of the brain's actions, not only
  the first.
