# Loading screens: asset groups with progress

A level needs its tiles, sounds and data before it can start, and a loading
screen should keep drawing while they arrive. Ponko already has the pieces
for loading one asset at a time:

- `load_texture`, `ctx.audio.load`, `ctx.assets.load_with` and the other
  loads by name read and decode the file **on the game thread**, in the
  call, and their `try_` forms return an `Error` instead of panicking.
  Right for what is small or needed this frame; a frame that loads a level
  this way stops until every file is decoded.
- `stream_texture` and `stream_mesh` ([Streaming](streaming.md)) never wait:
  they decode on worker threads and upload within a per-frame budget, under
  a memory budget. Right for a world that loads around the player.
- [`Job`](https://docs.rs/ponko/latest/ponko/streaming/struct.Job.html)
  runs any work of your own on a worker thread.

An **asset group** puts these together for a loading screen: declare the
assets a level needs, start the group, and it loads them in the background,
reports how far it got by count and by bytes, says when it is ready or which
file failed, and can be cancelled. Once it is ready, the usual loads return
its assets at once, without reading them again.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/asset_groups.rs:loading_screen}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:loading_screen}}
```

</div>
</div>

## Declaring a group

`AssetGroup::new(name)` starts an empty group; each method adds one asset by
its name, as the loads take it:

| Method | Loaded as | Afterwards |
|---|---|---|
| `texture(path)` | decoded on a worker, uploaded to the GPU | `ctx.gfx.load_texture(path)` returns it |
| `streamed_texture(path)` | a stream, under the texture [residency budget](streaming.md#budgets-staying-in-memory) | `ctx.gfx.stream_texture(path)` returns it |
| `mesh(path)` (`3d` feature) | a stream, under the mesh residency budget | `ctx.gfx.stream_mesh(path)` returns it |
| `sound(path)` | decoded on a worker | `ctx.audio.load(path)` returns it |
| `with(path, decode)` | `decode(bytes)` on a worker: your own format | `ctx.assets.take_group_value::<T>(id, path)` |
| `bytes(path)` | the file's bytes | `take_group_value::<Vec<u8>>(id, path)` |

`with` takes the same shape as `ctx.assets.load_with`: a function from the
file's bytes to your value or an `Error` (usually `ErrorKind::Decode`), so
any loader written that way plugs in, a level format, a tile map or a sprite
sheet's data. A value is taken once, by its type; `take_group_value` returns
`None` before the group loaded it, after it was taken, or for another type.

`ctx.assets.start_group(group)` starts it and returns a `GroupId`; the work
begins at the start of the next frame. `ctx.assets.group(name)` finds the
last group started with that name, so systems don't need to keep the id.
Starting a group with the same name again (after a failure, say) makes a new
group. It reads its files again, but an asset the old one already loaded
keeps its first handle: the copy just read is dropped.

## Progress, ready, failed, cancelled

`ctx.assets.group_progress(id)` says how far the group got:

- `loaded` of `total` assets, and `state` (`Loading`, `Ready`, `Failed` or
  `Cancelled`, also from `group_state(id)`).
- `bytes_read` of `bytes_total`: the bytes read from the files so far.
  `bytes_known` says whether every file's size was known before it was read:
  then `bytes_total` is the whole group from the first frame, and
  `fraction()` is weighted by bytes, so one large file counts for more than
  many small ones. Otherwise `bytes_total` grows as files are read and
  `fraction()` counts assets.
- `fraction()` never goes back while the group loads and is `1.0` once it is
  ready (an empty group is ready at once).

Sizes are known up front when every source can say them without reading:
the `assets/` folder, [packs](asset-sources.md#packs), `MemorySource`, and
your own sources that implement `AssetSource::size`.

The **first failure** ends the group: its state is `Failed` and
`group_error(id)` returns the error, with the path of the asset that failed
(`error.path()`) and its kind: `NotFound` for a missing file, `Decode` for a
broken one, or what your `with` loader returned. The assets the group
already finished stay loaded; the rest are not started.

`ctx.assets.cancel_group(id)` stops a group: the files not yet started are
never read, the ones on workers are dropped when they finish, and streams
that haven't uploaded anything are removed. Its state is `Cancelled`.

## Never stalling a frame

At the start of each frame, before your systems, the engine's **group
step** takes what the workers finished, uploads it, and starts more reads
and decodes. It stops for the frame once it has run for the **frame budget**
(`ctx.assets.set_group_frame_budget(ms)`, 4 ms by default), so a loading
screen keeps its frame rate; at least one piece of work runs every frame, so
a group always gets somewhere. Uploads also count against the [upload
budget](streaming.md#streaming-never-waiting) (`ctx.gfx.set_upload_budget`),
shared with streaming: the group step and streamed assets together send at
most that many bytes per frame (one upload bigger than the budget still goes,
alone).

`ctx.assets.group_stats()` reports what the step cost: the last and the
longest step in milliseconds, how many steps went over the budget, and the
bytes uploaded in the last frame. `ctx.gfx.upload_stats()` includes the
group's uploads in the frame's totals.

## Where files are read

Files from `assets/` and packs are read on worker threads. [Asset
sources](asset-sources.md) added with `ctx.assets.add_source` are read on
the game thread, since their trait may not be safe to call from another
one; the group step reads them within its frame budget, one file per piece
of work. A source that is safe to read anywhere can be added with
`ctx.assets.add_background_source(source)` instead (it needs `Send + Sync`;
`MemorySource` and `PackSource` are): then groups and streams read it on
workers too. Both kinds are asked in the order they were added, as always,
so a source added later never hides one added earlier: when a game-thread
source was added after a background one, files are looked up on the game
thread, so add background sources last.

`ctx.finish_uploads()` waits until every group is done (and every stream
uploaded), without the budgets: for tests and tools, never in a game's frame.
A game that calls it at a checkpoint (after streaming a level's cells, say)
while a group is loading waits for that group too; without groups it waits
only for streams, as it always did.

## A loading state

`Game::loading_state(loading, group, then)` ties a group to a [game
state](states.md): entering `loading` calls `group(ctx)` and starts the
group it returns, and once it is ready the game moves to `then` (the change
applies at the end of that tick, like any other). Leaving `loading` early,
to go back to the title, cancels the group. A failed group keeps the game in
`loading`, so the loading screen can show `group_error` and offer a retry
(`set_state(loading)` enters it again and starts a new group).

## Measured

`cargo run --release -p ponko_bench -- streaming/` includes a group case,
`streaming/group-16x1k`: a headless game that starts a group of sixteen
1024 × 1024 noise PNGs written to disk first, beside the `load_texture` and
`stream_texture` cases of the same files. It measures the game thread's CPU
time spent loading in each frame, as the [streaming numbers](streaming.md#measured)
do. On an Apple M4 Max (macOS, release build, default budgets: 4 ms per
frame, 16 MiB uploads), three runs with the machine under heavy other load
(load average 100 to 140):

| | Worst frame's loading work | Median | Usable after |
|---|---|---|---|
| `load_texture` | 12.1 to 12.8 ms | 0.23 to 0.25 ms | the same frame |
| `stream_texture` | 0.14 to 0.27 ms | 0.002 to 0.004 ms | 5 to 11 frames (32-pixel stand-ins) |
| asset group | 0.03 to 0.08 ms | 0.001 ms | 9 to 10 frames (full size) |

The group never came near its 4 ms frame budget: decoding runs on workers,
and the game thread only takes the results and hands them to the GPU. The
streaming chapter's caveats apply: headless runs don't include the GPU's
share of uploads (the upload budget is what bounds that), noise PNGs may
decode more cheaply than real art, and one set of runs on a loaded machine
is for comparing the cases within a run.

## In a browser

In an ordinary web build there are no worker threads, so a group's reads
and decodes run in the group step, one piece of work at a time, and it
stops for the frame once the budget is spent: a loading screen keeps
drawing between files. A build with threads, on a page that allows them,
reads and decodes on job workers as on the desktop ([Threads on the
web](web-threads.md)); the browser test checks a group from `assets/` there. One file
that takes longer than the budget to decode still makes that frame long.
Files come from what the page fetched (`assets/index.json`) and from
sources, as for every load in a browser. The browser smoke test
(`web/smoke/`) loads a group from an in-memory source and checks that it is
ready.

## C and C++

From C, `ponko_asset_group_new(name)` makes a group,
`ponko_asset_group_add(group, kind, path)` adds an asset (`PONKO_GROUP_TEXTURE`,
`PONKO_GROUP_STREAMED_TEXTURE`, `PONKO_GROUP_MESH`, `PONKO_GROUP_SOUND` or
`PONKO_GROUP_BYTES`), and `ponko_ctx_start_asset_group` starts it (it takes
the group; free one you don't start with `ponko_asset_group_free`). Group ids
are `uint32_t`, never 0. `ponko_ctx_asset_group_state`,
`ponko_ctx_asset_group_progress`, `ponko_ctx_find_asset_group`,
`ponko_ctx_cancel_asset_group` and `ponko_ctx_set_asset_group_frame_budget`
match the Rust calls; `ponko_ctx_asset_group_error` returns the failure's
status and sets the last error, its kind and its path.
`ponko_ctx_take_asset_group_bytes` hands a `PONKO_GROUP_BYTES` asset's bytes
to your decode callback once, and `ponko_game_loading_state` makes a loading
state whose callback adds the group's assets.
`ponko_ctx_add_background_memory_source` adds a memory source that workers
read.

In C++, `ponko::asset_group` is the builder, `ctx.start_asset_group`,
`ctx.asset_group_progress` and the rest are `ponko::ctx` methods,
`ctx.asset_group_error(id)` returns an `std::optional<ponko::error>`, and
`ctx.take_asset_group_bytes(id, path, decode)` returns what your
`decode(data, len)` returns. There is no C or C++ form of `with`: take the
bytes and decode them yourself.
