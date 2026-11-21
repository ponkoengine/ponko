### Changed
- A rich text label that doesn't wrap keeps its laid-out glyphs when it
  moves sideways (a sibling in its row resized) or its box's width changes:
  its lines are aligned where they are placed, not when its text is laid
  out, like plain text's. What is drawn is unchanged.
- The UI layout keeps each label's measured size: unwrapped, narrowest, and
  wrapped at the last four widths it was offered (plain and rich text). A
  wrapped label in a row whose sibling resizes back and forth is measured
  once per width, not each time. Rich text that doesn't wrap is measured
  once, as plain text already was.
- `ponko_bench` has `ui/draw-resize`: a panel in a row of the settings
  screen switches between two widths each frame, moving a rich label
  sideways and rewrapping a paragraph. Its frame went from 0.103–0.106 ms
  to 0.067–0.068 ms (medians of three interleaved runs each, on a very busy
  machine); the other `ui/draw-...` frames are unchanged within the noise
  (about 2 µs).
