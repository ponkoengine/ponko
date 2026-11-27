<!-- area: Tools: editor, inspector and agents -->
### Security
- The control endpoint's `sim.step` queues any number of ticks at once
  instead of one at a time (four billion ticks took seconds of the game
  thread).
