### Fixed
- A target camera that draws only some frames (a reflection refreshed every
  other frame) no longer collects its whole view from nothing each time it
  draws: its frame is kept between draws. In a scene of tens of thousands of
  models, its collect went from about 15 ms to about 1.4 ms a frame.
