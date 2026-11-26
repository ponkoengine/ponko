### Changed
- Crowds on the CPU path (WebGL2's limits) group their characters' draws by
  counting them per view, level and posed frame instead of sorting: for
  10,000 characters the frame's prepare and record went from 0.55 to 0.47 ms
  on an Apple M4 Max, drawing exactly as before.
- The attachments benchmark has a `+100 ticks` row, so the steady tick can
  be read apart from the first update, which links every new attachment to
  its parent.
- `bench/compare/scoreboard.py --only` keeps the date and revision of the
  rows it doesn't measure again, and the table names them.
