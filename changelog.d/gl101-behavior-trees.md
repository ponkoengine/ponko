### Added
- Behaviour trees (`ponko::ai`, guide: "Game AI"):
  - `{"subtree": "ai/patrol.json"}` runs another tree by name, with its
    blackboard defaults. `BehaviorTrees` links trees in any order and links
    again every tree that uses one that changed; `Assets::load_behavior_tree`
    loads and hot-reloads the files a tree names. A tree that contains
    itself is refused with every name on the cycle
    (`BehaviorTrees::try_insert`; `insert` panics). `BehaviorTree::subtrees`.
  - `{"parallel": [..], "success": "any", "failure": "all"}`: all children
    at once with a stated policy (`"all"`, `"any"` or a count); it stops
    what still runs when it ends. One child at most may hold `do` nodes.
  - `{"random": [..]}` and `{"chance": p}`, drawn from the game's seed, the
    brain's tick and its entity (`Brain::with_seed` / `set_seed` for brains
    run by hand), so the same seed and inputs give the same decisions.
  - `{"wait_for": "signal", "timeout": s, "store": "key"}` waits for an
    `ai::Signal` event (`Signal::to`, `Signal::broadcast`, `.with(value)`)
    instead of polling; the signal wakes the brain whatever its period.
    `ai::forward` turns any event type into signals; `Brain::signal`,
    `BehaviorTree::signals`.
  - A brain's JSON holds its memory (running nodes, waits, cooldowns), so a
    saved world carries on at the same running node, counting ticks on from
    the save; a changed tree (`BehaviorTree::fingerprint`) starts over.
- C and C++: `ponko_behavior_tree_new_linked` (`behavior_tree(json,
  subtrees)`), `ponko_brain_signal`, `ponko_brain_set_seed`,
  `ponko_brain_save_json` and `ponko_brain_load_json` (`brain::signal`,
  `set_seed`, `save_json`, `brain::load_json`).

### Changed
- `Brain`'s JSON has a `"memory"` field once it has thought; saves without
  it still load (the brain starts its tree over, as before). A brain is
  128 bytes instead of 72.
