### Added
- `d3::NavAgent` builds with struct-update syntax, like other components:
  `NavAgent { speed: 4.0, destination: Some(goal), ..Default::default() }`.
  Its path, crowd slot and speed reached moved into the new public
  `NavAgent::state` field, of the opaque type `d3::NavAgentState`. That type
  has no public constructor, so a literal always ends with
  `..Default::default()` (or `..other_agent`) and new settings can arrive
  without breaking it. Setting fields on a `NavAgent::default()` keeps
  working; clippy's `field_reassign_with_default` lint now suggests the
  literal there. `d3::NavObstacle` is still built with its constructors
  (`NavObstacle::new`, `::cylinder`, `::convex`), as its shape needs them.
