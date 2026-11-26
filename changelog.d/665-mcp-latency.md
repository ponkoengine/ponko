<!-- area: Tools: editor, inspector and agents -->
### Changed
- A tool call to a running game takes a fraction of a millisecond instead
  of about 25 ms. The control endpoint accepts connections the moment they
  arrive instead of looking for them every 20 ms (closing it still stops at
  once), and writes each reply in one piece. `ponko-mcp` keeps one
  connection per game and reuses it across calls; it opens a new one,
  without an error, when the game closed the connection or restarted.
  Replies are matched to requests by their `id`.
