# When something fails: reading the message

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/errors.rs`,
> `tests/error_messages.rs`, `tests/startup_errors.rs`,
> `tests/loader_convention.rs`. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks). Graphics: not involved.
> Missing: a reference game that uses it.

Every failure the engine reports says two things: what your code did, and
what to change. This chapter shows the messages for the mistakes people make
in their first hour, and how to choose between a panic and an `Error`.

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

## A component not registered for reflection

Scenes, prefabs, data files and the inspector see only reflected
components. Loading one that isn't registered says which call is missing:

```text
no reflected component named `Health` (register it with `world.reflect::<T>()`)
```

Call `ctx.world.reflect::<Health>()` (or `reflect_resource::<T>()` for a
resource) in setup. See [Reflection](reflection.md).

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
and `Game::run` in a build without the `window` feature.

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
