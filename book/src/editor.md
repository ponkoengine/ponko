# The editor: build scenes and prefabs

The editor is your game, opened in edit mode. Its window becomes a level
editor: a scene tree on the left, the selected entity's components on the
right, the world in the middle with gizmos to move, turn and scale things,
a File menu that opens and saves scenes and prefabs, and undo for every
change.

```sh
ponko editor                          # the game in this folder
ponko editor games/yard --open levels/one.json
ponko editor --example editor         # inside the engine's repository
ponko editor --last                   # the project and file opened last
ponko editor --recent                 # list the recent projects
```

`ponko editor` builds the game with the engine's `editor` feature and runs
it with `PONKO_EDITOR` set. The game's setup runs as usual, then time is
paused and the editor shows the world setup made, or the scene or prefab
named by `--open` (a path in `assets/`). It takes the same target options
as `cargo run` (`--bin`, `--example`, `-p`, `--features`, `--release`), and
what follows `--` goes to the game. `--print` shows the command instead of
running it. Without the `ponko` command it is:

```sh
PONKO_EDITOR=levels/one.json cargo run --features ponko/editor
```

(`PONKO_EDITOR=1` opens the world setup made.)

## Why the editor runs inside the game

The editor could have been a separate program that attaches to a running
game through the [control endpoint](control.md). It isn't, because only
the game's own build knows its components, its systems, its assets and how
it draws them: a separate editor would have to load the game's code or
show a world it can't draw. Built into the game, the editor shows exactly
what the game shows, edits every [reflected](reflection.md) component the
game registers, and runs the game's own systems when you press **Play**.

The control endpoint still reaches it: an agent connected to the editor
makes the same edits through the same undo history (see
[below](#agents-and-the-editor)).

## The window

| Part | What it has |
|---|---|
| Menu bar | **File** (New scene, Open, Open recent, Save, Save as, Save selection as prefab, Reload from disk), **Edit** (Undo, Redo, Duplicate, Rename, Delete, New entity), **Author** (the [authoring windows](#authoring-particles-tiles-animation-graphs-materials-and-terrain)), **Window** (the asset panel on or off), **Help** (Shortcuts), the [play bar](#play-mode), the view (2D or 3D) and the gizmo's mode, axes and snapping |
| Hierarchy (left) | Every entity under what it hangs off. Click to select, Shift or Ctrl+click to add to the selection, drag onto another row to attach (the whole selection moves with the dragged row), drag onto *drop here to detach* to make a root. Double-click or F2 renames; a right click has Rename, Duplicate, New child, Detach and Delete |
| Inspector (right) | A prefab instance's [overrides](#prefab-instances-and-overrides), then the selected entity's components, editable in place, **Add component** and **Remove**; below them resources and performance, as in the [inspector](inspector.md) |
| Assets (bottom) | The [asset panel](#the-asset-panel): the assets folder as a tree, thumbnails, search, and the selected asset's details and import settings |
| Viewport (middle) | The world as the game draws it, with the gizmo on the selection (drawn only in the viewport, never over a panel). The 2D and 3D cameras draw into the free part of the window between the panels (their `rect`), so what the camera looks at is in the middle of the viewport; resizing or hiding a panel moves the view with it, never during a drag. A camera rect the game set itself (a split screen) is left alone |
| Status bar | The document (`untitled scene`, `scene levels/one.json`, `prefab prefabs/crab.json`), *unsaved changes*, and what the last action did |

The window title shows the document's name, with `*` while it has unsaved
changes.

### Keys

| Key | Does |
|---|---|
| Ctrl+N, Ctrl+O | New scene, open a scene or prefab |
| Ctrl+S, Ctrl+Shift+S | Save (an untitled document asks for a file), save as |
| Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y | Undo, redo |
| Ctrl+D, Delete | Duplicate, delete the selection (with what hangs off it) |
| F2 | Rename the selection |
| W, E, R | The move, rotate and scale gizmo |
| F | Frame the selection (3D) |
| Ctrl+P, Ctrl+Shift+P, F10, Ctrl+. | Play (or resume), pause, step one tick, stop |
| Ctrl+F | Search the assets |
| F1 | The shortcuts window |
| Esc | Stop placing a prefab, close a dialog |
| Right or middle drag, wheel | Pan and zoom (2D); in 3D see [Editing in 3D](inspector.md#editing-in-3d) |

On macOS, Ctrl is ⌘. While the editor has the window, keys go to it, not
to the game; **Play** gives them back to the game, except the play keys,
which work while it runs.

**Help → Shortcuts** (F1) lists every shortcut. **Set** next to one takes
the next key you press, with its modifiers, as its only shortcut (Esc
leaves it as it was); **Reset** puts the default back, and **Reset all**
every default. A key bound to two actions shows in red, with the other
action on hover. Camera flying (right mouse with W A S D Q E) can't be
rebound.

## Gizmos and snapping

Select an entity and drag a handle:

- **Move**: an arrow per axis moves along it, the dot in the middle moves
  freely.
- **Rotate**: drag around the ring.
- **Scale**: a box per axis, or the dot to scale evenly. A scale never
  reaches zero.

**World** and **Local** pick the world's axes or the entity's own. With
**Snap** on, moves land on a grid (8 pixels in 2D, 0.5 units in 3D), turns
go in steps (15°) and scales in steps (0.1); the fields next to it change
the steps. A selection of several entities moves and turns about its
middle. Moving a parent carries what is attached to it.

2D games get these 2D gizmos; 3D games the 3D ones, with a handle per plane
as well (see [Editing in 3D](inspector.md#editing-in-3d)).

## Undo, and what counts as unsaved

Every change is a step in one history: a gizmo drag, an inspector edit, an
added or removed component, a rename, a reparent, a spawn, a delete, a
resource edit. A drag is one step however long it lasts, and so is a run
of edits to one field while you drag its number: undo takes the whole drag
back. A multi-selection's delete or reparent is one step too.

The document is unsaved when its history is not where it was at the last
save: undoing back to the saved state makes it saved again, and a change
after undoing past the save makes it unsaved for good (the redo steps are
gone). Opening or reloading a file starts a fresh history.

Closing the window with unsaved changes asks first: **Save**, **Don't
save** or **Cancel**.

## Scenes and prefabs

A document is a [scene](scenes.md) (the whole world) or a
[prefab](scenes.md#prefabs-with-children) (one entity with what hangs off
it). **Open** reads either; a prefab opens on its own in an empty world.
**Save as** writes the document as a scene or as a prefab (a prefab needs
exactly one root in the world); after that, **Save** writes it there.
**Save selection as prefab** saves the selected entity as a prefab file and
leaves the document as it was.

The files are JSON in `assets/`, readable and diffable; the paths are
relative to `assets/` and can't leave it.

### Choosing a file

On macOS, Windows and Linux, **Open** and **Save as** use the system's
own file picker. With unsaved changes, **Open** first asks whether to
drop them. **Save as** first asks whether to save a **Scene** or a
**Prefab**; **Choose file…** then opens the save picker in the
document's folder with its name filled in (**Save** on an untitled
document does the same). Canceling a picker changes nothing: the
document and its unsaved edits stay as they were.

The file has to be inside the project's `assets/` folder. The editor
refuses one outside it, or one that another `assets/` folder or a
registered asset source would shadow, and says why. A name typed without
an extension gets `.json`. On Linux the picker is the desktop's file
chooser portal, with Zenity as the fallback.

Builds without a system picker (the web) ask in the editor's own dialog
instead: **Open** shows a path field (starting at the open document's
file) above the scenes and prefabs it found, and **Save as** a path field
beside **Scene** and **Prefab**, with the default name filled in. Enter
or the button confirms, Esc cancels and changes nothing, and a file that
can't be read or written is reported in the panel like the picker's
errors.

### Saving in a browser

A browser has no `assets/` folder to write to, so the web build keeps what
the editor saves (**Save**, **Save as**, **Save selection as prefab**) in
the page's `localStorage`, one entry per file named after the page's path
and the asset (`ponko-assets:/play/:scenes/level.json`), so two games on
one site keep theirs apart. The saved file is read from there at once,
and when the page is loaded again the saved files replace the fetched
ones before setup runs: the game and the editor find the version you
saved. The status bar says the file stayed in the browser.

**File > Download** saves the document, then hands its file to you as a
browser download, to keep it or to copy it into the project's `assets/`.
A browser keeps a few megabytes per site this way; when it refuses a save
(it is full), the error says so and Download still gives you the file.

A browser may also clear a site's storage when the disk runs low, unless
it keeps that storage *persistent*. Before setup the web editor asks for
that (`navigator.storage.persist()`): Chromium decides from how the site
is used (an installed or bookmarked site gets it), Firefox asks you, and
a page served without HTTPS (other than `localhost`) can't have it. When
the browser refuses, the status bar says **the browser may clear saves**
(its tooltip says why) and **File > Saved in this browser** repeats it:
download what you want to keep. `ctx.assets.browser_saves_persistent()`
is the answer from code: `Some(true)`, `Some(false)`, or `None` until the
browser answers (and in other builds).

**Open** and the asset panel list the web build's file manifest: the
files named in `assets/index.json` (which `web/build.sh` and `ponko
package --web` write), the files in the page's asset pack, and the ones
saved in the browser. A JSON file is listed as a scene or prefab by its
`"format"`, as on the desktop. Their tiles get thumbnails made from those
files in memory (a browser has no folders to read them from): a texture
scaled down, a scene's map, a prefab's sprite, a model's software drawing,
with its buffers found beside it by name. A file you save or revert gets
its thumbnail made again.

**File > Saved in this browser** lists every file the browser keeps for
this page, with its size, and offers **Revert** for a file the build
ships (a scene in `assets/` you saved over) and **Delete** for one only
the browser has. Either removes the browser's copy: a reverted file is
the build's again at once, from the copy kept aside when the save
replaced it, and the open document is opened again if it was that file
and had no unsaved changes. After a reload nothing replaces the shipped
file any more, so the build's version is back. From code,
`ctx.assets.browser_saves()` lists the same names and
`ctx.assets.revert_browser_save(path)` reverts one (both are Rust-only;
in other builds the list is empty and nothing is reverted).

While the system's picker is up it has the keyboard, so the editor never
sees the keys let go meanwhile: the Ctrl (⌘) of Ctrl+O, or the Enter that
pressed **Choose file…**. When the picker closes, and whenever the window
loses or regains focus, the editor forgets every key and modifier it
thought held; a key still down counts again when it is pressed again. The
game's own input (`ctx.input`) already lets everything go when the window
loses focus.

## When the file changes on disk

The editor looks at the open file twice a second. If another program
changed it (a pull, a text editor, a script) and you have no unsaved
changes, the editor opens it again. If you do, the status bar says so and
offers **Reload** (drop yours) or **Keep mine** (the next save writes over
the file).

## The asset panel

Along the bottom of the window, the panel lists what the game can use from
its assets folder: textures (PNG, JPEG), models (glTF), sounds (WAV, OGG,
FLAC, MP3), prefabs, scenes and shaders. The folders are a tree on the
left; a tile per asset on the right, with a thumbnail made on a worker
thread: the image, the model drawn from the front, the sound's waveform, a
prefab's sprite or model, a scene's map.

A model's first thumbnail is a quick software drawing. When the game has a
GPU, the engine's own renderer then draws the model (lit, with its
materials and textures) in a small offscreen view on the same GPU, one
model per frame, and that picture replaces it. Rendered thumbnails are kept
in the user's cache folder (`~/Library/Caches/ponko/thumbnails` on macOS,
`$XDG_CACHE_HOME/ponko/thumbnails` or `~/.cache/ponko/thumbnails` on
Linux, `%LOCALAPPDATA%\ponko\thumbnails` on Windows;
`PONKO_THUMBNAIL_CACHE` names another folder) under a hash of the model's
file and the buffers and images it names: the next run shows them at once,
in any project, and a changed model or texture is drawn again. The web
build keeps the software drawing, made from the page's files in memory
(see [Saving in a browser](#saving-in-a-browser)): a browser can't wait
for the GPU's picture within the frame, and has no cache folder. The search field (Ctrl+F) shows
every asset whose path has all the words typed, in any folder, and
**List again** reads the folder again after files were added.

- **Drag** a texture, model or prefab into the viewport to place it where
  you drop it: a prefab as an [instance](#prefab-instances-and-overrides),
  a model as an entity with that model, a texture as a sprite in 2D. In the
  3D view, a texture dropped on a model paints it: the model gets a
  `ModelMaterial` of that image, saved as `{ "texture": "path" }`, and the
  scene loads the image when it is opened again. Each placement is one
  undo step.
- **Double-click** a scene or prefab to open it (asking first if the
  document has unsaved changes).
- **Click** an asset for its details on the right: its path, its kind and
  size (an image's pixels, a sound's length, a model's triangles, a
  prefab's entities), **Preview**
  and **Stop** for a sound, **Open** for a scene or prefab.

### Import settings

The details end with the asset's import settings: its rule in `cook.json`,
the file [`ponko cook`](packaging.md#cooking-assets-ponko-cook) reads in the assets folder. The editor
shows where the settings come from (the asset's own rule, a pattern such
as `ui/**`, or the cooker's defaults) and offers each setting the cooker
has for that kind of asset: `texture`, `basis` and `mipmaps` for an image,
`optimize` and `quantize` for a model, `audio` for a sound, `shader` for a
shader, or `keep` and `skip` for the whole file. A change writes a rule for
that one asset into `cook.json` and leaves the rest of the file as it was;
**Use the pattern / defaults** removes the asset's own rule, so a pattern or the
defaults apply again. Nothing is cooked until you run `ponko cook`.

## Prefab instances and overrides

A prefab placed in the editor (dragged from the asset panel) remembers its
file: its root gets a `PrefabInstance` component, which the
scene saves. Selecting any part of the instance shows, at the top of the
inspector, the prefab it came from and how this instance differs from it:

- **Overrides**: each component whose value differs from the prefab's,
  with the fields that differ, and a **Revert** button that puts the
  prefab's value back. The root's position, rotation and scale are where
  you placed it, not an override.
- Entities **added** to the instance (attached to its parts) and parts
  **missing** from it.
- **Revert all** makes the instance the prefab again (added entities are
  deleted, missing parts come back).
- **Apply to prefab** writes the instance into the prefab file, so its
  overrides become the prefab's. The other instances in the open scene are
  updated at once, keeping their own overrides; all of it is one undo
  step, but the file stays written.
- **Open prefab** opens the prefab alone, as its own document.

A prefab opened as a document has **Apply to scenes** once it is saved: it
opens every scene in the assets folder that has instances of it, updates
them to the saved prefab (keeping their overrides), saves each and reopens
the prefab.

### Prefabs inside prefabs

An instance of a prefab that includes other prefabs is compared the same
way, part by part. **Apply to prefab** writes the outer file and keeps each
included prefab as the link it is, with how this instance changes it: the
included root's changed components, those it no longer has (`remove`) and
changes to the included prefab's own parts (`parts`), as
[Prefabs that include prefabs](scenes.md#prefabs-that-include-prefabs)
describes. With a part of an included prefab selected, an **Apply to
*name*** button writes that part of the instance into the included prefab's
own file instead; every instance in the scene that uses it, directly or
inside another prefab, follows. A part deleted from an included prefab
can't be written as a link: revert it, or apply to the included prefab.

### glTF scenes

A glTF file spawned as a scene (`GltfRoot::spawn`, or `spawn_prefab` of a
`.glb` in the game's setup) can be edited too: move a node, change or
remove its components, add new ones. How each node differs from what the
file makes of it is its override, kept in the scene's saved record:

- **Exporting the file again** (hot reload) updates the scene from the new
  file and puts the overrides back: a moved node stays where you put it in
  its parent, a changed component keeps the fields you changed, while
  everything you didn't touch follows the file.
- **Saving** the scene writes the overrides; opening it again in the
  editor spawns its glTF scenes where they were, with them, and a game
  that loads the save and spawns the file again gets them too. A glTF
  scene whose file is missing or broken then is left out (its record and
  overrides stay in the scene, for when the file is back), and the status
  bar's error line names each such file and why it didn't load.

Overrides are per component (the changed fields of each), and two more
kinds are per node:

- A node **deleted** in the editor (with the nodes that hang off it) stays
  deleted: re-exports and reopens leave it out, and what you attached to
  it elsewhere before deleting it stays where you put it.
- A node **attached to another parent** (dragged onto another row in the
  hierarchy, or detached) keeps that parent: another node of the scene,
  the scene's root, the root or a node of another glTF scene, or an
  entity the scene file saves. Through a save, a reopen and Play/Stop a
  node of another scene comes back on its parent whichever scene spawns
  first (the scene file numbers each glTF scene's record for it).
  Deleting that parent deletes what hangs off it; undo brings both back,
  linked, at once. If the other scene's file doesn't load when the scene
  is opened, the node waits on its file parent and its overrides list
  the parent as missing: **Revert** forgets the link, or fix the file and
  the link is made when that scene spawns. Builds from before these links
  read such saves too, with the node on its file parent. A parent the
  save leaves out (an entity marked not saved) can't be kept, and the
  editor says so on its console.

Selecting a scene's root or any of its nodes shows, at the top of the
inspector, the file it came from and every override of the scene, one row
each (*node* › *component*, *node* › parent, or the deleted *node*), with a
**Revert** button that puts the file's value, the file's parent or the
deleted node back (one undo step each), and **Revert all**. Undoing a
delete brings the node back whole: what saves leave out of it (its mesh,
collider, light) comes from the file again.

Play and Stop keep glTF scenes: Play records how they were edited, and
Stop spawns them again from their files (saves leave their nodes out) with
those edits. Their nodes are new entities then; the undo history and the
selection follow them, so undoing past Stop still reaches the edits made
before Play.

## Authoring: particles, tiles, animation graphs, materials and terrain

Some things are tuned by eye rather than typed: how a fire's sparks rise,
which tile goes where, when a character blends from walking to running,
how shiny a rock is, where a hill stands. The editor has a tool for each.
They all work the same way:

- every edit is a step in the [one history](#undo-and-what-counts-as-unsaved):
  a drag, a brush stroke or a typed value is one step, and Ctrl+Z takes it
  back wherever it was made;
- what they edit shows at once in the view, while the game is paused;
- they save in the formats the game already loads, so nothing changes in
  the game's code: Ctrl+S (or a window's **Save**) writes every edited file
  with the scene, and a file with unsaved edits marks the document unsaved
  (the close question covers it too).

Particle emitters are components, so their section is in the inspector and
they are saved with the scene. The others edit files of their own: open one
from **Author**, which asks for a path in `assets/` and lists the files the
tool opens. Closing a window drops its unsaved edits. JSON files are written
back with two-space indents and their keys in alphabetical order; the
loaders don't mind the order.

What stays in other tools, on purpose: modelling, rigging and drawing
animation clips (Blender, Spine), tilesets, Wang sets, objects and other map
formats (Tiled, LDtk), texture painting and generated terrain, and shader
code beyond quick edits (your code editor; the [hot reload](3d.md#shader-errors-and-reloads)
picks its saves up).

### Particles

Select an entity with a 2D or 3D `ParticleEmitter`: its **Particles**
section has every setting (rate, burst, duration and looping, lifetime,
speed, shape, gravity, drag, size, rotation, texture and sprite sheet,
blend, the particle limit, local or world space, seed and, in 3D, softness),
the size over a particle's life as a curve and its colour as a gradient.
In the curve, click to add a key, drag one to move it and double-click to
remove it; the gradient's bar works the same way, with each key's time and
colour below it. **Live preview** (on by default) runs the selected
emitter while the game is paused, and an edit keeps the particles already
flying, so a change shows on the cloud you were looking at; **Restart**
starts it over.

### Tilemaps

**Author → Tilemap…** opens a Tiled map (`.tmj`, the JSON kind that
[`spawn_level`](tiles.md#tilemaps) loads). Pick a layer and a tileset, click a
tile in the palette, then paint in the view: **Paint** puts the tile,
**Erase** clears cells and **Autotile** puts the frame each cell's
neighbours call for. The autotile rule is kept as a property of the layer,
`autotile`, with the value `four <first tile>` (16 frames, see
`AutoTile::four`) or `blob <first tile>` (47 frames, `AutoTile::blob`), so
the next session and your own code can read it. The level in the view
follows every stroke; **Show in the view** spawns it when the scene has
none. Infinite maps (chunked) aren't edited here.

### Animation graphs

**Author → Animation graph…** opens a 2D skeleton in Ponko's format
(`"format": "ponko-skeleton2d"`, see [2D skeletons](skeleton2d.md)) and
shows its graph: a node for each state, an arrow for each transition, and
**Any state** for transitions from anywhere. Click a state or an arrow to
edit it below the canvas, drag a state to move it, and right-drag from one
state onto another to add a transition. A state plays one clip or a 1D
blend tree over a float parameter, at a speed, looping or not; a transition
has its conditions (greater, less, bool, trigger), an exit time, a fade and
an offset. **+ State** and **+ Parameter** add them.

The **Preview** runs the graph on the skeleton with test values for its
parameters (they aren't saved): drag a float, tick a bool or **Fire** a
trigger, and the state playing turns green and the pose follows. A graph
the runtime would refuse says why under the canvas, and the preview keeps
the last one that loaded. Where the nodes sit is the editor's own; the
file has no place for it. 3D animation graphs are built in code and have
no file to edit.

### Materials and shaders

**Author → Materials…** opens a glTF file (`.gltf`; export a `.glb` as
`.gltf` to edit it) and edits its materials: base colour, metallic,
roughness, emission, normal strength, alpha mode and double-sidedness, and
each texture slot, which takes one of the file's images or a PNG from
`assets/` (added to the file, by a name relative to it). Every model drawn
with the file's materials changes as you edit; textures are read again only
when a slot changes.

**Author → Shader…** opens a material shader (`.wgsl`, see
[custom shaders](3d.md#custom-shaders)). The line with the first error is marked and
the compiler's message is under the code; while the code compiles, the
game's shader loaded from that file takes it once typing pauses (with an
error, the last good code stays, as with hot reload). Below the code are
the custom values (`material.custom[0]` to `[3]`) and textures of the
materials drawn with the shader, edited live. Those are set in code, with
`MaterialDesc::with_custom`, so **Save** writes the `.wgsl` alone and
**Copy as Rust** copies the values as code to paste where the game makes
its material.

### Terrain

**Author → Terrain…** opens a terrain file, or makes a new flat tile when
the name isn't there yet. The brushes work on the tile in the 3D view:
drag over it to **Raise**, **Lower** or **Smooth** the ground, or **Paint**
a ground layer, under a circle of the brush's radius, at its strength.
**+ layer** adds a layer from a PNG; each layer repeats every *size* world
units. **Show in the view** puts the tile in the middle of the world when
the scene has none (it isn't saved with the scene).

A terrain file is JSON, with its images beside it, and
`TerrainTile::spawn_from_file(ctx, path, corner)` spawns it as a [`TerrainTile`](3d.md#terrain):

```json
{
  "format": "ponko-terrain", "version": 1,
  "size": 64.0,
  "heights": "hill-heights.png", "low": -8.0, "high": 24.0,
  "layers": [{ "texture": "grass.png", "size": 4.0 }, { "texture": "rock.png", "size": 6.0 }],
  "weights": ["hill-weights.png"],
  "roughness": 0.9
}
```

`heights` is a square greyscale PNG of n × n heights (16 bits a pixel; 8
work too), rows along +Z, black at `low` and white at `high`. `weights`
are RGBA PNGs over the whole tile: red, green, blue and alpha of the first
are layers 0 to 3, of the second layers 4 to 7. Without layers the tile is
drawn with the default material. The brushes keep heights between `low`
and `high`; changing either keeps the ground where it is.

## Play mode

The play bar runs the game in the editor:

| Button | Does |
|---|---|
| **Play** (Ctrl+P) | Saves the world as it is and runs the game's systems; **Resume** when paused while playing |
| **Pause** (Ctrl+Shift+P) | Stops time where it is; the panels edit the paused game |
| **Step** (F10) | One tick, then paused (the first step plays too) |
| **Stop** (Ctrl+.) | Puts the world back exactly as it was at Play |
| speed | How fast the game's time runs |

The play bar says whether you are editing, playing or paused, and the
tick. Stop puts back every entity and component, the entity ids and the
resources as they were saved at Play, and the undo history and the unsaved
marker with them: what the game did, and what you changed while it ran,
is gone.

Unless you keep it. If you changed things through the editor while
playing, Stop asks first and lists them (each component of an entity that
was there at Play, each resource); the ticked ones are written back into
the restored world, with the values they have at Stop (your edit, and
whatever the game did to it since), as one undo step. **Stop, keep none**
throws them all away. Entities spawned or deleted while playing are always
put back.

## Layout, recent projects and settings

The editor remembers, across runs, the sizes of its panels, whether the
asset panel is open, the projects and files you opened (ten of each) and
the shortcuts you rebound. They are in `editor.json` in the user's
settings folder: `~/Library/Application Support/ponko` on macOS,
`%APPDATA%\ponko` on Windows, `$XDG_CONFIG_HOME/ponko` (or
`~/.config/ponko`) elsewhere; `PONKO_CONFIG_DIR` puts it somewhere else.
**File → Open recent** has the project's recent files,
`ponko editor --recent` lists the projects
and their files, and `ponko editor --last` opens the project and file you
had open last. Two editors open at once both write the file; each reads it
again before it writes, so neither loses the other's projects.

## Agents and the editor

The [control endpoint](control.md#the-editor-from-an-agent) works in the
editor: `editor.document` opens, reloads or starts a document and reports
it, `editor.save` takes `as` (`scene` or `prefab`), and `editor.transform`
and `editor.drag` work on 2D games as well as 3D ones. An agent's edits are
steps in the same history, so you can undo them, and they mark the document
unsaved like yours. The [MCP tools](agents.md#what-an-agent-can-do) are
`editor_document` and `save_edited_scene`.

## Turning it on

The editor is behind the engine's `editor` cargo feature, which is off by
default: a game built without it has no editor code, and `PONKO_EDITOR`
only prints that the feature is missing. `ponko editor` turns the feature
on for that build only, so the game you ship is unchanged. To keep it on
in a development build:

```toml
[features]
editor = ["ponko/editor"]
```

The feature needs the [inspector](inspector.md) (it turns it on) and a
window. A [web build](web.md) with the feature starts in edit mode from
the page's address instead of the variable: `?editor` (or `?editor=1`)
edits what setup made, `?editor=levels/a.json` opens that file. Open and
Save as use the path field there (see [Choosing a file](#choosing-a-file)).

C and C++ games get edit mode from `ponko_ffi`'s `editor` feature and the
same `PONKO_EDITOR` variable, with the asset panel, prefab overrides and
play mode in their window; there is no C API for the editor itself (its
undo history, prefab comparison, play mode or settings), and
`ponko editor` builds Cargo projects only.

## Tests

Everything behind the panels is tested without a window: the history and
its merged drags, the unsaved marker, scene and prefab round trips, the
file watch, the gizmos' math, and the panels themselves driven by fake
mouse and keyboard input: Ctrl+S, F2, the close question, dragging a hierarchy row onto another, dragging a number
in the inspector, and snapped move, rotate and scale drags in 2D and in a
3D game, each one undo step. The newer parts are tested the same way:
dragging an asset into the view places it (one undo step), an image
dropped on a model paints it and the scene keeps it, a prefab
instance's overrides are listed, reverted one at a time or all together
and applied to the prefab and its other instances (and, for a prefab
inside a prefab, to the outer or the included file with **Apply to
*name***), a glTF scene's node moved with the gizmo keeps its place through
a re-export and a save and reopen, glTF nodes deleted with the Delete key
or dragged onto another row in the hierarchy stay so through both, a glTF
scene's overrides are listed and reverted from its panel, Play then Stop
spawns glTF scenes again with the undo history pointing at their new
nodes, the 2D and 3D cameras centre on the
view between the panels and follow a dragged panel edge, a model tile
shows the thumbnail kept on disk for its content, Stop gives back the
world saved at Play byte for byte (and keeps only the edits ticked), the
shortcuts are rebound by key presses and saved, the layout and recent
projects survive a restart, and the asset panel's thumbnails, search and
import settings match what `ponko cook` reads (both read one shared
fixture). A sound's preview is checked through the audio engine's offline
output, without a speaker. Open and Save as tests inject paths through
a fake picker, and the web build's path field is answered the same way
(typed, Enter or Esc) with the system picker turned off. Other tests hold
Ctrl and Enter down, then close the picker or feed the window's focus
events (as macOS reports them, both "focused"), and check that nothing is
held afterwards and that a plain S doesn't save. Offscreen GPU tests draw the editor's window
over a 2D world and over a frame of the 3D renderer and check where its
panels and gizmo are, and draw the asset panel, a prefab's overrides, the
shortcuts window and the play bar, each against a control; another draws a
model's thumbnail with the renderer, checks the model covers its middle and
not its corners, that it isn't the software drawing, and that the next run
reads it from the cache without drawing.

The authoring tools are tested by mouse the same way: a particle setting
dragged (one undo step, saved with the scene), a curve key added, dragged
and removed, a gradient key moved and removed, the preview running while
paused; tiles painted from the palette along a row (one step, the level
following), autotiled with the right frames and saved into the `.tmj`;
a state, a transition and its condition added in the graph and saved as a
skeleton the runtime loads, the preview following a dragged test value
into the next state, a broken graph explained in place; a material's
metallic dragged with the loaded model's material following and a PNG
picked for its base colour; a shader error shown at its line and undone;
custom values dragged onto the shader's materials; the raise brush held
over a terrain (one step), a layer added, painted and saved as PNGs that
`TerrainTile::spawn_from_file` loads. Offscreen GPU tests check the previews draw: a
material edit recolours its model, a shader edit shows and a broken one
keeps the last good look, a painted terrain layer shows where it was
painted, the particle preview draws particles (none without it), and the
graph's preview poses its skeleton by the test value, each against the
picture without the edit.

## Checks for a person

The tests feed the picker's answers and the focus events; these need a
real desktop session or browser:

1. On macOS, Windows and Linux (with the desktop's portal, and with
   Zenity), **Open** and **Save as** show the system's picker in the
   document's folder, with the name filled in for a save; a file outside
   `assets/` is refused, and canceling changes nothing.
2. On each of the three, open the picker with Ctrl+O (⌘O) and with Enter
   on **Choose file…**, then choose a file and also cancel. Back in the
   editor, press plain O, S and Enter: none of them act as if Ctrl (⌘) or
   Enter were still held (no second picker, no save, no confirmed
   dialog). Switching to another window with Ctrl or Alt held and back
   must leave nothing held either.
3. In a browser, a web build with the feature opened with `?editor`:
   **Open** and **Save as** show the path field, Enter and Esc work, a
   save reports that web builds have no files, and the asset panel's
   model tiles keep their software thumbnails.
