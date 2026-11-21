### Changed
- `par_for_each`, `par_for_each_indexed` and `par_collect_into` decide
  between the calling thread and all cores by the work they measure, not
  by the size of their archetypes: every loop of more than one chunk times
  its first rows and stays on the calling thread if the whole loop would
  take under about 50 µs. A cheap loop over a big archetype no longer wakes
  the pool (a 16,000-entity `pos += vel * dt` loop: 0.1× the cycles); a
  heavy loop goes parallel after 64 timed rows.
- Hierarchy updates (attachments) run each pass on all cores only when its
  measured work is worth it, so a small update stays on the calling thread
  and doesn't wait for busy worker threads (2,000 moving roots with four
  children each: 0.27× the cycles).
- `par_collect_into` into a type that needs `Drop` keeps the closure's
  captured values out of the loop and vectorizes: 0.36× the instructions
  per entity.
- `for_each` and the parallel loops with three or four `Option` fetches
  pick the loop per archetype, as with one or two: about 0.6× the cycles
  of testing each row.
