### Changed
- Wrapped text, plain or rich, keeps its laid-out glyphs while its width
  breaks its lines the same way: laying it out also finds the range of
  widths that would end each line in the same place. A wrapped label that
  moves sideways (its width changing in its last bits) or whose box grows
  or shrinks a little isn't laid out again. A label whose width goes back
  and forth keeps the layouts of its last three widths. What is drawn is
  unchanged.
- The UI layout keeps a wrapped label's height for each of the last four
  ranges of widths that wrap it the same way, not for four exact widths.
- `ponko_bench`'s `ui/draw-resize` frame went from 0.064–0.065 ms to
  0.027–0.029 ms (medians of five interleaved runs each, on a busy
  machine); the other `ui/draw-...` frames are unchanged within the noise
  (about 2 µs).
