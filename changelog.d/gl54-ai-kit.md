### Added
- `ponko::ai`, an optional AI kit (guide: "Game AI"): behaviour trees
  written as JSON (`sequence`, `selector`, `utility`, `invert`, `succeed`,
  `check`, `set`, `do`, `wait`, `cooldown`) that read and write a reflected
  per-agent `Blackboard` and pick an action that the game's systems carry
  out (`Brain::doing`, `Brain::succeed`, `Brain::fail`).
  `Assets::load_behavior_tree` (and `try_load_behavior_tree`) loads a tree
  with hot reload; `ai::think`
  runs every due brain on all cores, deterministically, at a per-brain
  think period spread over the ticks (`Brain::with_period`,
  `ai::lod_period`). Utility helpers for plain systems too: `Curve`,
  `Consideration`, `normalize`, `combine`, `best`. The inspector shows the
  selected agent's tree with what each node did at the last think. A tree of
  15 nodes decides for 10,000 agents in about 0.8 ms on one core
  (`ponko_bench ai/10k`). New `ai_guards` example.
- C and C++: `ponko_behavior_tree_new`, `ponko_brain_*`, `ponko_ai_key`,
  `ponko_ai_curve`, `ponko_ai_combine` and `ponko::ai::behavior_tree`,
  `ponko::ai::brain` for brains a native game keeps in its own storage.
