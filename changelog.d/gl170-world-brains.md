### Added
- World brains inside C and C++ loops: `Brain` and `Blackboard` are query
  columns (`ponko_ai_brain_component`, `ponko_ai_blackboard_component`), so
  a `ponko_query` visits each entity's brain and blackboard with its other
  components, and `ponko_chunk_ai_brain` / `ponko_chunk_ai_blackboard` reach
  a row's. `ponko_ai_brain_action`, `_started`, `_finish`, `_param_num`,
  `ponko_ai_blackboard_set_*` and the `ponko_ai_blackboard_*` getters work
  on a row inside `ponko_query` and from `ponko_query_par` threads: a C
  system needs no list of entities first. C++: `ai::each<Ts...>(ctx, fn)`
  with `ai::brain_row` and `ai::blackboard_row`, `ai::query`,
  `ai::query_par` and `ai::rows`; `world_view::component_id<T>()` gives the
  C id of a component type.
- Blackboard getters for entities and 2D and 3D points: world brains
  `ponko_ctx_ai_entity`, `_vec2`, `_vec3` (C++ `ai::get_entity`,
  `get_vec2`, `get_vec3`), native brains `ponko_brain_entity`, `_vec2`,
  `_vec3` (C++ `brain::get_entity`, `get_vec2`, `get_vec3`).
