<!-- area: Tools: editor, inspector and agents -->
### Changed
- In a web build with threads, a hierarchy update (attachments) pass worth
  sharing out runs on the worker threads, through the same runner as
  parallel queries, instead of calling the thread pool from the page's
  thread (where it ran alone).
- `for_each` and the parallel loops with two to four `Option` fetches
  vectorize for every combination of components, not only when an
  archetype has none of them: two `Option`s whose values the closure
  combines went from 14.0 to 2.8 instructions per entity (none: 2.0), three
  from 17.1 to 3.1 and four from 19.7 to 3.3, about half the cycles.
