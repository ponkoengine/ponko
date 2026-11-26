# When something fails: reading the message

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/errors.rs`,
> `tests/error_messages.rs`, `tests/error_messages_files.rs`,
> `tests/startup_errors.rs`, `tests/loader_convention.rs`. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks). Graphics: not involved.
> Missing: a reference game that uses it.

Every failure the engine reports says two things: what your code did, and
what to change. This chapter shows the messages for the mistakes people make
in their first hour, and how to choose between a panic and an `Error`.

## The common mistakes at a glance

| You did | The message says | Section |
|---|---|---|
| Misspelled an asset, or put `assets/` in its name | the folders searched, near names, the name without the folder | [A missing asset](#a-missing-asset) |
| Gave a loader the wrong kind of file, or an empty one | what the bytes are, and the call that loads them | [A file that isn't what its name says](#a-file-that-isnt-what-its-name-says) |
| Used a format or subsystem the build leaves out | the cargo feature to turn on | [A feature the build leaves out](#a-feature-the-build-leaves-out) |
| Loaded a save slot that is missing, damaged or newer | which, the slots there are, how to get on | [A save that won't load](#a-save-that-wont-load) |
| Named a component the game never registered | the entity, the call to register it, near names | [A component not registered for reflection](#a-component-not-registered-for-reflection) |
| Opened a scene as a prefab, or the other way round | what the file is and which call loads it | [A scene, save or prefab of the wrong kind](#a-scene-save-or-prefab-of-the-wrong-kind) |
| Ran on a machine with no usable GPU | the GPU's words, the driver, backend and headless fixes | [No GPU to draw with](#no-gpu-to-draw-with) |
| Queried or ordered systems in a way the schedule refuses | the system, the component, the call to make instead | [Systems the schedule refuses](#systems-the-schedule-refuses) |

## Panic or `Error`

Loaders and constructors come in pairs. The plain name (`load_texture`,
`create_shader`) panics, for files the game can't run without: a typo there
is a bug to fix now. The `try_` twin returns `Result<_, Error>` for files
that may legitimately be missing, such as mods or user content. Both carry
the same message. Match on `err.kind()`, never on the text; the text is for
people and may get better between versions.

```rust
fn icon(ctx: &mut Ctx, name: &str) -> Texture {
    match ctx.gfx.try_load_texture(name) {
        Ok(texture) => texture,
        // A missing optional icon draws nothing.
        Err(err) if err.kind() == ErrorKind::NotFound => Texture::NONE,
        Err(err) => panic!("{err}"),
    }
}
# let _ = icon(ctx, "icons/coin.png");
```

C and C++ get the same message, kind and path through `ponko_last_error()`,
`ponko_last_error_kind()` and `ponko_last_error_path()` after a call that
fails.

## A missing asset

The message lists every folder it searched, then the names in the requested
folder that look close: the same name in another case, the same name with
another extension, or a name with the same start.

```text
ponko: "crab.pgn" not found in assets/. Looked in:
  /home/me/game/assets
Check spelling and case, or place the file under one of these assets/ folders. Nearby names: crab.png
```

Only the requested folder is listed, and at most eight names, so a typo in a
big asset folder stays quick to read. Asset sources added with
`AssetSource` can't list their files; their ids appear among the searched
places.

Two slips that a spelling check can't see get a line of their own: a name
that starts with the `assets/` folder itself, and one written with
Windows separators.

```text
ponko: "assets/crab.png" not found in assets/. Looked in:
  /home/me/game/assets
Check spelling and case, or place the file under one of these assets/ folders. Nearby names: (none)
Hint: names are relative to the assets/ folder, so drop the leading `assets/`: try "crab.png" (it exists there)
```

## A file that isn't what its name says

A decoder's own words ("Invalid PNG signature") don't say what you did. The
loaders for images, fonts and sounds look at the file's first bytes, so the
message says what the file is, and which call loads it. A renamed sound
given to `load_texture`:

```text
ponko: "/home/me/game/assets/crab.png" is not a valid image: its bytes are an Ogg sound, not an image; load it with `ctx.audio.load` instead
```

The other cases have their own fix:

```text
"hit.wav" is not a valid sound: the file is empty (0 bytes); a failed export or copy leaves one, so export or copy it again
"crab.png" is not a valid image: it is a Git LFS pointer, the small text file a checkout without the real file leaves; run `git lfs pull` to fetch the file itself
"crab.png" is not a valid image: it is a text file, not an image; check that the file name points to the real image
"crab.png" is not a valid image: it starts like a PNG image but can't be read (cut off or damaged?); export or copy it again, and open it in an editor to check: unexpected end of file
"hit.ogg" is not a valid sound: its first bytes (de ad be ef 01 02 03 04) aren't those of any sound format ponko reads: ...; ponko reads Ogg Vorbis, MP3, FLAC, WAV and AIFF
```

The error's kind is `ErrorKind::Decode`, and `path()` is the asset's name.

## A component not registered for reflection

Scenes, prefabs, save slots, data files and the inspector see only
reflected components. A file that names one that isn't registered says which
call is missing, where the file names it, and the registered names closest to
it, so a typo shows:

```text
ponko: levels/one.json: no reflected component named `Helth`: register the type with `world.reflect::<Helth>()` in setup, or fix the name in the file (names are case-sensitive); entity 9 is the first in the file to name it; did you mean `Health`?
```

Call `ctx.world.reflect::<Health>()` (or `reflect_resource::<T>()` for a
resource) in setup, or correct the name. When no registered name is close,
the message points to `world.reflected_components()`. A value that doesn't fit
its type names the entity too:

```text
ponko: levels/one.json: bad value for `Health` on entity 2: invalid type: string "lots", expected i32
```

See [Reflection](reflection.md).

## A scene, save or prefab of the wrong kind

A scene and a save slot are the same format; a prefab is another. Giving one
to the other's loader says what the file is and which call reads it:

```text
ponko: levels/crab.json: not a world save (expected "format": "ponko-world"): it is a prefab: spawn it with `ctx.spawn_prefab` instead. A scene or save slot is written by `save_scene`, `Storage::save_world` or the editor
ponko: levels/one.json: not a prefab (expected "format": "ponko-prefab"): it is a scene or save: load it with `ctx.load_scene`, or `Ctx::load_world` for a save slot. A prefab is written by `save_prefab` or the editor
```

A file from a newer ponko says to update the engine (`unsupported save
version 99 (this engine reads 1): the save was written by a newer ponko: ...`,
and the same for prefabs), and one with no `format` field at all says it isn't
a file ponko wrote.

## A save that won't load

`Ctx::load_world(slot)` names the slot and the file in every message, and the
world is unchanged after a failure. A slot nobody saved lists the slots there
are:

```text
ponko: there is no save in slot "slot2" (looked for saves/slot2.json); saved slots: slot1
```

A damaged or empty file says so, and what to do: restore the previous copy
(`Storage::read_backup` returns the file as it was before its last write, if
there is one) or delete the slot with `Storage::delete_world`:

```text
ponko: save slot "torn" (saves/torn.json) is damaged or isn't a ponko save: EOF while parsing a string at line 1 column 22; restore its previous copy if there is one (`Storage::read_backup("saves/torn.json")`) or delete the slot with `Storage::delete_world`
ponko: save slot "none" (saves/none.json) is empty (0 bytes); restore its previous copy if there is one (...) or delete the slot with `Storage::delete_world`
```

A save from a newer version, one that names an unregistered component and
one that isn't a save use the messages above, with the slot in front. Reading
a stored file that isn't there names the folder it looked in
(`no stored file "settings.json" in /home/me/.local/share/crabs`), so a wrong
app id shows; `read_json` of a file of the wrong shape names the type it
didn't fit. With no app id or folder, storage is off and says to give the game
one with `Game::app_id` (or `Game::data_dir`).

## The same component queried mutably twice

A query that reads and writes one component in the same item would alias
it, so it panics before it runs:

```text
query `(&mut my_game::Speed, &my_game::Speed)` accesses `my_game::Speed` twice, once mutably; remove the duplicate access or split it into sequential queries
```

Read the value through the `&mut` you already have, or run two queries one
after the other.

## A feature the build leaves out

Formats and subsystems you don't use stay out of the build. Using one names
the Cargo feature that turns it on:

```text
ponko: "pixel.qoi" is a QOI image, but this build of ponko doesn't read QOI: turn on its `image-qoi` cargo feature (or `image-formats` for all of them)
```

The same holds for KTX2 textures (`compressed-textures`), glTF extensions,
system fonts (`system-fonts`), code mods (`mods-wasm`), and `Game::run` in a
build without the `window` feature, which returns `ErrorKind::Unsupported`
from `try_run` and says to use `Game::run_headless`.

## No GPU to draw with

Opening the window's GPU can fail in three steps, and each message names the
step, the GPU's own words in parentheses, and what to try. On a desktop:

```text
ponko: no compatible GPU found (...): install or update the graphics driver (Vulkan on Linux, Direct3D 12 or Vulkan on Windows, Metal on macOS), turn on GPU acceleration in a virtual machine, or on a machine without a GPU use `Game::run_headless`; `WGPU_BACKEND` (`vulkan`, `dx12`, `metal` or `gl`) picks the backend to try
ponko: could not open the GPU device (...): the GPU lacks a limit or feature ponko needs, or its driver failed; update the graphics driver, and try another adapter or backend (`WGPU_BACKEND`)
ponko: could not create a GPU surface (...): the window was refused a drawing surface; update the graphics driver, and on Linux check that a Wayland or X11 session is running
ponko: the GPU can't draw to this window: its surface offers no format this adapter can show (a software fallback adapter, such as a virtual machine's, may be all there is); update the graphics driver, or try another backend (`WGPU_BACKEND`)
```

They reach the game as the `Error` of `try_run` (kind `ErrorKind::Unsupported`)
and as the panic of `run`, before any of your systems run. In a browser the
messages say to use a current browser with hardware acceleration turned on;
a WebGPU adapter that opens a device but can't present (a software fallback
one) is not an error there: the engine falls back to WebGL2 and logs one
console line (see [WebGPU first, WebGL2 as the fallback](web.md#webgpu-first-webgl2-as-the-fallback)).

## Systems the schedule refuses

The schedule checks its own rules when the game starts or the system is
added, not at the frame where the mistake would show:

```text
Game::add: system `spawn_wave` has an interval, and only fixed-tick phases take one
Game::order: `second` already runs before `first` (an earlier phase, or registered first in the same phase); order can keep two systems apart, not swap them: to run `second` first, register it before `first` or put it in an earlier Phase
system `mover` went beyond its declared access: it writes component `Velocity`. Declare it in its SystemAccess, or register the system without one (exclusive).
`Changed<Health>` needs `Health`'s changes tracked; call `world.track_changes::<Health>()` first (in a game: in setup or a `&mut Ctx` system)
no resource of type `my_game::Score`; add one first with `world.insert_resource(...)`
Game::state: the game already has a state of type `my_game::Mode`; register each state type once
```

Each names the system or type, and the call to make instead; move an
interval system to a fixed-tick phase (for example `Phase::SimLast`), or drop the interval.

## A freed handle

Drawing with a freed handle is defined and quiet: it draws the default (see
[Handles](handles.md)). Calls that need the resource itself panic and say
what to make instead, also after a new resource has taken the freed one's
slot:

```text
ponko: GPU buffer was freed or belongs to another game; create a live buffer with create_gpu_buffer
instance batch: mesh is freed or foreign; create a live mesh in this game
ponko: create_mesh: that material wasn't made by this game; create it with create_material in this game
```

Check `ctx.is_live(handle)` before such a call when a handle may be stale.

## A shader that doesn't compile

The message gives the file, line and column, the line itself and a caret
under the problem, then the variant of the engine's shader that was
compiled:

```text
ponko: red.wgsl:3:12: error: automatic conversions cannot convert `{AbstractFloat}` to `Surface`
        return 1.0;
               ^ this expression has type {AbstractFloat}
  = variant: material shader with decals, global illumination and instance storage
```

The message names the types, functions and variables as your WGSL writes
them. A sprite shader whose `fragment` hook returns three components
instead of four:

```text
glow.wgsl:3:12: error: `fragment` returns `vec3<f32>` but is declared to return `vec4<f32>`
        return mix(p.color.rgb, vec3<f32>(1.0), glow);
               ^ `mix(p.color.rgb, vec3<f32>(1.0), glow)` is `vec3<f32>`
  = note: in `fn fragment`
  = variant: sprite
```

The usual type mistakes read the same way: a call's argument against the
parameter it is passed to (``argument 1 of `brighten` (`p.color.rgb`) is
`vec3<f32>`, but its parameter `c` is `vec4<f32>` ``), an operation on two
types that don't go together (`` `*` can't combine `vec4<f32>` (`p.color`)
and `i32` (`steps`) ``, with the conversion to write), an assignment, an
`if` condition, an index, a uniform buffer's layout. A rarer error keeps the
compiler's own words, without its internal numbering.

It reaches the terminal as the panic of `create_shader` or `load_shader`, as
the notice of a watched file that draws with a stand-in until it compiles,
and as the notice of a hot reload that kept the previous version. A `try_`
twin returns it as the `Error` message instead of printing it.
`ctx.gfx.shader_status()` keeps the same diagnostic, with the line and
column as numbers, for tools and in-game overlays (C:
`ponko_ctx_shader_status`). An error in the engine's code around yours has
no line of yours to point at, and says so instead of guessing.

## Offline image comparison

`ponko_image_diff`, the library behind the image-diff command and MCP tool,
returns `ponko_image_diff::Error`: match `kind()` (`Invalid`, `Io`,
`Decode`, with a wildcard arm), read `path()` and `message()`, and use
`std::error::Error::source()` for the file or codec error underneath. It has
no C or C++ binding; native tools call the command or the MCP tool.
