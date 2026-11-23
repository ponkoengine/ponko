# Cookbook

Fifty small recipes for the things most games do. Each one is a few lines to copy, and each is compiled and run by a test (headless, for a few ticks, with an assertion that it did its job), so none of them can go out of date without a test failing. The text on these pages is included from those tests, not retyped.

Every recipe is shown in Rust and C++ side by side. The C++ halves are in `examples/cpp/cookbook.cpp`, and the native check builds and runs them.

A recipe is a system or two and the components they use. Add the system with `.sim_system(...)` (or `.system(...)` for work that belongs to the frame, like cameras and UI) and the components to your own entities. Names such as `Player` are made up for the recipe: use yours.

## [Moving things](moving.md)

- [Walk with the keyboard](moving.md#walk-with-the-keyboard)
- [Walk with a gamepad too](moving.md#walk-with-a-gamepad-too)
- [Jump](moving.md#jump)
- [Dash with a cooldown](moving.md#dash-with-a-cooldown)
- [Click to move](moving.md#click-to-move)
- [Drag a sprite with the mouse](moving.md#drag-a-sprite-with-the-mouse)
- [Face the mouse](moving.md#face-the-mouse)
- [Stay on the screen](moving.md#stay-on-the-screen)
- [Let the player rebind a key](moving.md#let-the-player-rebind-a-key)

## [Fighting and collecting](gameplay.md)

- [Shoot bullets](gameplay.md#shoot-bullets)
- [Bullets that fly and expire](gameplay.md#bullets-that-fly-and-expire)
- [Pick up coins](gameplay.md#pick-up-coins)
- [Damage and death](gameplay.md#damage-and-death)
- [Draw a health bar](gameplay.md#draw-a-health-bar)
- [Waves of enemies](gameplay.md#waves-of-enemies)
- [An enemy that chases the player](gameplay.md#an-enemy-that-chases-the-player)
- [A guard that patrols and gives chase](gameplay.md#a-guard-that-patrols-and-gives-chase)
- [Drop loot when something dies](gameplay.md#drop-loot-when-something-dies)

## [Time and timers](time.md)

- [A fuse that goes off](time.md#a-fuse-that-goes-off)
- [Do something every few seconds](time.md#do-something-every-few-seconds)
- [Slide something with easing](time.md#slide-something-with-easing)
- [A day and a night](time.md#a-day-and-a-night)
- [Slow motion](time.md#slow-motion)

## [Menus and screens](menus.md)

- [A pause menu](menus.md#a-pause-menu)
- [A title screen](menus.md#a-title-screen)
- [Game over and play again](menus.md#game-over-and-play-again)
- [A heads-up display](menus.md#a-heads-up-display)
- [A volume slider](menus.md#a-volume-slider)
- [A panel that slides in](menus.md#a-panel-that-slides-in)

## [Cameras](camera.md)

- [Follow the player](camera.md#follow-the-player)
- [Shake the screen](camera.md#shake-the-screen)
- [Zoom toward the cursor](camera.md#zoom-toward-the-cursor)
- [Pixel-perfect pixel art](camera.md#pixel-perfect-pixel-art)
- [A minimap](camera.md#a-minimap)

## [Bodies and collisions](physics.md)

- [Damage on contact](physics.md#damage-on-contact)
- [Knock something back](physics.md#knock-something-back)
- [Line of sight](physics.md#line-of-sight)
- [Click a body to select it](physics.md#click-a-body-to-select-it)
- [A platformer player](physics.md#a-platformer-player)

## [In 3D](three-d.md)

- [Click a 3D object](three-d.md#click-a-3d-object)
- [Walk with a third-person camera](three-d.md#walk-with-a-third-person-camera)

## [Levels and objects](world.md)

- [Load a level from a Tiled map](world.md#load-a-level-from-a-tiled-map)
- [Spawn a prefab](world.md#spawn-a-prefab)
- [Attach one thing to another](world.md#attach-one-thing-to-another)
- [Draw what the AI is thinking](world.md#draw-what-the-ai-is-thinking)

## [Sound and saves](sound-and-saves.md)

- [Footsteps](sound-and-saves.md#footsteps)
- [A sound for an event](sound-and-saves.md#a-sound-for-an-event)
- [Save and load the game](sound-and-saves.md#save-and-load-the-game)
- [Keep settings and a best score](sound-and-saves.md#keep-settings-and-a-best-score)
- [Save and quit](sound-and-saves.md#save-and-quit)
