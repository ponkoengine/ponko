A small 2D game is running headless and paused, with its control endpoint
on; reach it with the ponko-mcp tools. It loads the level
`levels/run.json` (a scene in `assets/`): the player (`Player`,
`Transform2D`) starts at (100, 300) and the arrow keys move it right along
a row of five coins worth 1 each. Touching a coin collects it into the
`Score` resource. Its numbers are in `assets/dash.json`, which the game
reloads while it runs.

Bug report: "I hold Right through the whole level and still end with 0
points."

Goal: holding Right from the start of the level for 2.5 seconds (150
ticks) must end with 5 points. Find out why it doesn't and fix it in the
game's files, so the level loaded fresh plays right. Keep the coins where
they are, the speed as it is, and the pickup radius at most 32 pixels
(the player's size).
