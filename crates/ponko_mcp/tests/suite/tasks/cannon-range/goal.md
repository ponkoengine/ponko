A small 2D game is running headless and paused, with its control endpoint
on; reach it with the ponko-mcp tools. A cannon on the ground at x = 40
fires a ball at the speed and angle in `assets/cannon.json`, which the game
reloads while it runs. Gravity pulls the ball down at 400 pixels per second
squared, and the air slows it down. Pressing Space, or setting the
`Trigger` resource's `fire` to true, fires one ball (while none is in the
air); a ball that lands sends a `Landed` event with its x.

Goal: the shot must land on the target, between x = 600 and x = 640. Change
only `muzzle_speed` in `assets/cannon.json`; keep the angle at 45 degrees.
