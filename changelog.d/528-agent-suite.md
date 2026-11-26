### Added
- The agent acceptance suite records time: a real agent's result file has
  the seconds to its last submission and when each tool call came, and
  `agent_suite scores FILE...` prints the runs as a Markdown table for a
  release pull request.
- `scripts/mcp-session.py` drives a stdio MCP server one tool call per shell
  command, so an agent with a shell but no MCP client can try the suite.
  The guide's agents chapter has the procedure for a release ("At a
  release"), and the first real-agent run is recorded in the macOS check
  results.

### Changed
- MCP `entity_stats` (control method `entities.stats`): `fields` is
  optional; without it the answer counts the entities that match `with` and
  `where`.
- MCP `run_input_scenario`: its schema lists the key names (`Right`, not
  `ArrowRight`) and shows an `expect_ui` step.
