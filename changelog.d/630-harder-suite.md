### Added
- Five harder tasks in the agent acceptance suite: a bug with two causes
  where fixing the visible one leaves the symptom (`lost-points`), a value
  that has to be measured in the game (`cannon-range`), one change across a
  scene, a prefab and a data file (`tough-bats`), a choice that needs
  `save_world` and `load_world` to try alternatives (`hold-the-gate`), and a
  code task, a Rust game crate rebuilt once per attempt (`stuck-guard`).
  Each has negative controls, among them "fixed only the visible cause".
- Each suite task names the tools it leans on (`agent_suite list`), and
  `agent_suite mcp TASK --hide TOOL,...` takes tools away from a real
  agent's run, to compare tool surfaces.

### Changed
- MCP `subscribe_events` (control method `events.subscribe`): the reply
  lists the event types collected (all of the game's when none were named)
  and whether that is all, and names a type the game doesn't have under
  `unknown`, with a hint listing those it has. Such names are still
  accepted.
