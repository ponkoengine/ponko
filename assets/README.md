# Sample assets

The files the examples, tests and guide load (`ctx.gfx.load_texture("crab.png")`
reads `assets/crab.png`). The examples and these assets ship in the `ponko`
crate; `branding/` does not.

| File | What it is | Where it comes from |
|---|---|---|
| `crab.png` | The sprite of the examples and the guide | Added with the first sprite renderer (#3); no separate provenance note, see git history |
| `models/house.glb` | A small house, walls and a roof with two materials | Added as the sample model of the first 3D support (#51); no separate provenance note, see git history |
| `coin_dash/coin.png`, `coin_dash/player.png`, `coin_dash/field.png` | The coin, the player and the grass field of the `coin_dash` example | Tiles from [Tiny Town](https://kenney.nl/assets/tiny-town) and [Tiny Dungeon](https://kenney.nl/assets/tiny-dungeon) by Kenney, CC0; see `examples/coin_dash/CONTENT-NOTICES.txt` |
| `coin_dash/sounds/*.ogg` | Its sound effects | From [RPG Audio](https://kenney.nl/assets/rpg-audio), [Music Jingles](https://kenney.nl/assets/music-jingles) and [Interface Sounds](https://kenney.nl/assets/interface-sounds) by Kenney, CC0 |
| `coin_dash/music/dash.ogg` | Its music | "Stage Select" from [4 Chiptunes (Adventure)](https://opengameart.org/content/4-chiptunes-adventure) by Juhani Junkala (SubspaceAudio), CC0 |
| `ember_hop/sprites/*.png`, `ember_hop/levels/cave.png` | The sprite sheets and the tileset of the `ember_hop` platformer | Tiles from [1-Bit Pack](https://kenney.nl/assets/1-bit-pack) and [1-Bit Platformer Pack](https://kenney.nl/assets/1-bit-platformer-pack) by Kenney, CC0; `ember.png` and `glow.png` from [Particle Pack](https://kenney.nl/assets/particle-pack) by Kenney, CC0; see `examples/ember_hop/CONTENT-NOTICES.txt` |
| `ember_hop/levels/*.tmx`, `cave.tsx` | Its two levels and the tileset, in the Tiled map editor's format | Made for that example (MIT or Apache-2.0) |
| `ember_hop/prefabs/*.json`, `ember_hop/scenes/title.json` | Its prefabs and its title scene | Written for that example (MIT or Apache-2.0) |
| `ember_hop/sounds/*.ogg` | Its sound effects | From [Digital Audio](https://kenney.nl/assets/digital-audio), [Impact Sounds](https://kenney.nl/assets/impact-sounds), [Music Jingles](https://kenney.nl/assets/music-jingles) and [Interface Sounds](https://kenney.nl/assets/interface-sounds) by Kenney, CC0 |
| `ember_hop/music/cave.ogg` | Its music loop | [8-Bit Cave Loop](https://opengameart.org/content/8-bit-cave-loop) by Wolfgang_, CC0 |
| `shard_grove/grove.glb` | The glTF level of the `shard_grove` 3D reference game: ground, walls, broken walls, pillars, a platform with a ramp, the gate's arch, with PBR materials (base color, normal and ambient occlusion / roughness / metalness maps) | Geometry made for that example (MIT or Apache-2.0); textures from Poly Haven, CC0: [Forest Ground 01](https://polyhaven.com/a/forrest_ground_01), [Castle Wall Slates](https://polyhaven.com/a/castle_wall_slates) and [Mossy Rock](https://polyhaven.com/a/mossy_rock) by Rob Tuytel, [Stone Brick Wall 001](https://polyhaven.com/a/stone_brick_wall_001) by Dimitrios Savva and Rico Cilliers; see `examples/shard_grove/CONTENT-NOTICES.txt` |
| `shard_grove/warden.glb`, `shard_grove/crawler.glb` | Its two skinned characters with their animation clips | [Hooded Adventurer](https://poly.pizza/m/y9KWOVG21R) and [Crab Enemy](https://poly.pizza/m/Gs3yfsV5lB) by [Quaternius](https://quaternius.com), CC0 |
| `shard_grove/sounds/*.ogg` | Its sound effects, the crawlers' hum and the wind loop | From [Digital Audio](https://kenney.nl/assets/digital-audio), [Sci-Fi Sounds](https://kenney.nl/assets/sci-fi-sounds), [Impact Sounds](https://kenney.nl/assets/impact-sounds) and [Interface Sounds](https://kenney.nl/assets/interface-sounds) by Kenney, CC0; the wind is [Wind](https://opengameart.org/content/wind) by IgnasD, CC0 |
| `shard_grove/music/grove.ogg` | Its music | [Forest Whisper Theme](https://opengameart.org/content/forest-whisper-theme) by Cleyton Kauffman, CC0 |
| `CC0-1.0.txt` | The text of Creative Commons CC0 1.0 Universal, which the reference games' CC0 content is under; shipped in their packages | [Creative Commons](https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt) |
| `levels/beach.json`, `prefabs/*.json` | A scene and prefabs for the scenes chapter | Written for the engine (#50) |
| `levels/meadow.tmx`, `levels/meadow.tsx`, `levels/meadow.png` | A Tiled map, its tileset and the tileset's image, for the `level` example and the tilemaps chapter | Written for the engine (#381); the image drawn by a script |
| `tutorial_2d/cove.tmx`, `tutorial_2d/cove.tsx`, `tutorial_2d/tiles.png`, `tutorial_2d/crab.png` | The level, tileset and crab of the 2D tutorial (`tutorial_2d` example) | Made for the engine (#372); the images drawn and the level laid out by a script |
| `tutorial_2d/coin.wav`, `tutorial_2d/flag.wav` | The 2D tutorial's two sound effects | Made for the engine (#372): square and sine tones synthesized by a script |
| `levels/courtyard.gltf` | A small glTF scene for the "From Blender to the game" chapter: one cube mesh placed as ground, a crate, a pillar and a door, a spawn point and a lantern, with the engine's custom properties in the nodes' extras | Written by hand for the engine (#41); the cube's buffer is embedded as base64 |
| `tutorial_3d/yard.glb` | The glTF scene of the 3D tutorial (`tutorial_3d` example): ground, walls, a platform with a ramp, stairs and a pillar | Made for the engine (#372): boxes and a wedge written by a script |
| `terrain/bump.json`, `terrain/bump-heights.png` | A 3 × 3 terrain file with one bump in the middle, for the terrain-file tests of the C and C++ API | Written for the engine (#227); the 16-bit height image made by a script |
| `shaders/*.wgsl` | Custom material shaders for the 3D examples | Written for the engine (#54) |
| `fonts/NotoSansArabic-Subset.ttf`, `fonts/NotoSansDevanagari-Subset.ttf`, `fonts/NotoSansJP-Subset.otf` | Arabic, Devanagari and Japanese fonts for the text-shaping tests and reference images, cut down with fontTools to the characters those tests draw (a few KB each) | Subsets of Noto Sans Arabic 2.013, Noto Sans Devanagari 2.007 and Noto Sans JP 2.004 by the Noto Project Authors and Adobe, under the SIL Open Font License 1.1 (not the repository's license); copyright lines and the license text in `fonts/OFL.txt` (#409) |

Files written for the engine are under the repository's license (MIT or
Apache-2.0, at your option; see `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE`
at the root).

The reference games' pictures, models, textures, sounds and music
(`coin_dash/`, `ember_hop/`, `shard_grove/`, as the table says) are by the
artists named there, under Creative Commons CC0 1.0 Universal
(`CC0-1.0.txt`): free to use, change and share without conditions. Each
game's `CONTENT-NOTICES.txt` credits them file by file. Artists are welcome
to make more for these games.

The fonts in `fonts/` are another exception: they stay under the SIL Open Font
License 1.1 (`fonts/OFL.txt`), which lets them be used, subset and
redistributed, bundled with software too, but not sold on their own.

The logo in `branding/` has its own note in `branding/README.md`.
