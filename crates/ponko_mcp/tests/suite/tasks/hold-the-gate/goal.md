A small tower-defense game is running headless and paused, with its control
endpoint on; reach it with the ponko-mcp tools. A wave of walkers
(`Walker`, `Health`) follows a winding road to a gate; the `Gate` resource
counts those that reach it (`breaches`) and those the towers stop
(`stopped`). Towers (`Tower`) stand on pads (`Pad`, with a name and a
place) and shoot the walker in reach that is furthest along the road.
`assets/defense.json` lists the pads that get a tower; the game builds them
when the file changes, and reloads it while it runs. The wave starts when
the game's simulation does, and comes only once.

Goal: with at most two towers, no walker may reach the gate. Write the
pads in `assets/defense.json`. The checker replays the wave from its start
with your towers (it keeps that state in `checker/`; leave it there).
