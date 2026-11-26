A small 2D game is running headless and paused, with its control endpoint
on; reach it with the ponko-mcp tools. It loads the cave level
`levels/cave.json` (a scene in `assets/`) with the player and four bats
(`Bat`, `Health`, `Transform2D`). While it plays, a spawner adds more bats
as `assets/spawner.json` says. The game reloads the data files under
`assets/` while it runs.

Playtest notes: "the bats die far too fast, and new ones come too often."

Goal: every bat must start with 30 health: the bats placed in the level
and every bat the spawner adds. The spawner must add bats half as often as
it does now. Keep the level's bats where they are, the spawner's prefab
and its limit on bats as they are.
