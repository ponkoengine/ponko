### Changed
- C and C++: the 3D physics queries (`ponko_ctx_cast_ray3d`,
  `ponko_ctx_cast_ray3d_all`, `ponko_ctx_cast_sphere3d`,
  `ponko_ctx_cast_capsule3d`, `ponko_ctx_cast_box3d`, the `ponko_ctx_overlap_*3d`
  calls, `ponko_ctx_closest_point3d`, `ponko_ctx_penetrations3d` and their `_ex`
  forms) work inside a `ponko_query` / `world().each` callback instead of
  returning `PONKO_BUSY`, so a loop over the characters can check the ground
  under each. They stay refused in `ponko_query_par`, and so do the calls that
  change physics. Matches the 2D queries.
- The busy-flag handling of commands and events in a query callback shares one
  helper with debug drawing, sounds and the cameras (no behaviour change; the
  "in use" error text is now the same for all of them).
