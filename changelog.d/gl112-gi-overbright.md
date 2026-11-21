### Fixed
- Realtime global illumination no longer fills closed and nearly closed
  rooms with the sky's light. The probes' own views took the sky's ambient
  light wherever their probes hadn't looked around yet or the grid faded
  out, and a room kept that outdoor light for hundreds of bounces (white
  walls pass almost all of it on). Probe views now count such probes as
  dark and use the outermost probes' light past the grid's edges, so light
  gets into the probes only from what they see. A closed white box under
  the sky now stays dark inside (its floor 15 instead of 237, out of 765);
  in the red-walled room lit through a roof opening, the settled error to
  a fully converged path-traced reference fell from 0.33 to 0.08 (flat
  ambient light: 0.69). The GPU cost is unchanged.
