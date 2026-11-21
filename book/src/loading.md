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
| `with_sliced(path, start)` | your own format, a bounded piece at a time ([below](#loaders-that-work-in-slices)) | `ctx.assets.take_group_value::<T>(id, path)` |
| `bytes(path)` | the file's bytes | `take_group_value::<Vec<u8>>(id, path)` |

`with` takes the same shape as `ctx.assets.load_with`: a function from the
file's bytes to your value or an `Error` (usually `ErrorKind::Decode`), so
any loader written that way plugs in, a level format, a tile map or a sprite
sheet's data. It is an `Fn`, not an `FnOnce`: a [restart](#trying-again)
runs it again for a file that failed. A value is taken once, by its type; `take_group_value` returns
`None` before the group loaded it, after it was taken, or for another type.

### Loaders that work in slices

A large file of your own format (a level of a hundred thousand records, a
navigation grid to build) can be loaded a bounded piece at a time, so that
where there are no worker threads it never takes one long frame. Write a
`SliceLoader`: `step` does the next piece of the work and returns
`Ok(true)` once everything is done, and `finish` hands over the value.
`with_sliced(path, start)` adds it: `start` gets the file's bytes (owned,
so the loader can keep them) and returns the loader.

```rust
{{#include ../../tests/asset_groups.rs:sliced_loader}}
```

Where there are no workers (a browser build without threads), the group
step calls `step` while its frame budget lasts and goes on in the next
frame, checking the budget between calls; `group_stats().pieces` counts the
pieces of work a step ran. With workers, a worker calls `step` until it is
done, as `with` runs on a worker. Keep each `step` short, well under a
millisecond on the slowest device you target: a long one still takes its
whole time. An error from `step` fails the group with the asset's path, and
a restart calls `start` again.

`ctx.assets.start_group(group)` starts it and returns a `GroupId`; the work
begins at the start of the next frame. `ctx.assets.group(name)` finds the
last group started with that name, so systems don't need to keep the id.
Starting a group with the same name again makes a new group. It reads its
files again, but an asset the old one already loaded keeps its first handle:
the copy just read is dropped. To try a failed group again, restart it
instead ([Trying again](#trying-again)): that reads only what had not
loaded.

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

The **first failure** stops the group: its state is `Failed` and
`group_error(id)` returns the error, with the path of the asset that failed
(`error.path()`) and its kind: `NotFound` for a missing file, `Decode` for a
broken one, or what your `with` loader returned. The assets the group
already finished stay loaded, and no more are started. The ones already
being read when it failed go on: the following frames take them as they
finish, and `loaded` counts them, so a restart doesn't read them again.
Which of them had finished in the frame the failure came back depends on
the workers, but `ctx.finish_uploads()` waits for all of them, so a test
sees the same `loaded` after it every run.

`ctx.assets.cancel_group(id)` stops a group: the files not yet started are
never read, the ones on workers are dropped when they finish, and streams
that haven't uploaded anything are removed. Its state is `Cancelled`.
Cancelling a failed group also drops what it kept for a restart.

## Trying again

`ctx.assets.restart_group(id)` loads the rest of a failed or cancelled
group. It is `Loading` again at once, and from the next frame it reads
**only the assets that had not loaded**: the one that failed and the ones
never started. What loaded keeps its handle and isn't read again, values
you haven't taken yet stay, and the files that were being read when it
failed are taken as they finish. Restarting a group that is loading or ready
does nothing. It's for a loading screen's "try again" after a dropped
connection, or after fixing a broken file:

```rust
# use ponko::prelude::*;
# use ponko::assets::GroupState;
fn retry_button(ctx: &mut Ctx) {
    let Some(id) = ctx.assets.group("level 1") else { return };
    if ctx.assets.group_state(id) == GroupState::Failed && ctx.input.key_pressed(Key::R) {
        ctx.assets.restart_group(id); // reads only what failed
    }
}
```

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

## What uploads cost

Making a texture on the GPU is the part of loading that has to run on the
game thread. The group measures it, so a loading screen (or a profiling
overlay) can show what its uploads cost on the player's GPU:

- `group_stats()`: `upload_ms`, the milliseconds the last step spent
  uploading (part of its `step_ms`), `max_upload_ms`, the most any step
  spent, and `total_upload_ms` and `total_upload_bytes` since the game
  started.
- `group_progress(id)`: `upload_ms` and `bytes_uploaded` of that group
  alone.

The time is the game thread's: making each texture and handing the GPU its
pixels, measured around those calls. The GPU copies the pixels in its own
time. Where the GPU has **timestamp queries** (most desktop GPUs; browsers
only with WebGPU's `timestamp-query`), the group measures that too:

- `group_stats()`: `gpu_upload_ms`, the GPU's own time copying the textures
  of the latest step it timed, and `total_gpu_upload_ms` since the game
  started. They arrive a frame or a few after the step, as the GPU's
  results are read back without waiting.
- `gpu_upload_timing` says whether this GPU times uploads. Where it doesn't
  (headless games, GPUs and browsers without timestamp queries, WebGL),
  it is `false` and `gpu_upload_ms` stays `None`: only the game thread's
  time is measured.

To time a copy, the step makes each texture write itself, as the GPU
driver layer otherwise does (through a staging buffer), with the copy
between two timestamps, and submits it at once. The time is the sum of
those copies: the GPU's work alone, without the time it sat idle between
them. Textures of other uploads (made by systems with `create_texture`, or
glTF files' buffers) aren't timed. Streamed textures and meshes upload in
the streaming step and count in `ctx.gfx.upload_stats()` instead, which has
the same `gpu_upload_ms` and `gpu_upload_timing` for that step's textures.
Some drivers now and then leave a timestamp unwritten (on an Apple GPU
about one copy in three, in our tests): such a copy is left out of the sum,
and a step whose copies all lost one keeps the previous step's time.
A headless game has no GPU, so there the time is only the engine's
bookkeeping.

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
`loading`, so the loading screen can show `group_error` and offer a retry:
`restart_group` on the group (found by its name) loads what failed, and the
game moves on once it is ready. (`set_state(loading)` would enter the state
again and start a new group, reading every file again.)

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

Repeated after restarts and sliced decodes were added (same machine and
command, three runs, load average 117 to 145, so again not a quiet
machine): `load_texture` 11.0 to 16.0 ms worst and 0.20 to 0.21 ms median,
`stream_texture` 0.12 to 0.22 ms and 0.001 to 0.003 ms, the asset group
0.04 to 0.06 ms and 0.001 ms, usable after 6 to 9 frames. The ordering and
the group's distance from its budget hold; a repeat on a quiet machine is
still to be done (the same command).

**Uploads on a real GPU.** The offscreen timing test
`group_upload_cost_on_the_gpu` (`cargo test --release -p ponko --lib
--features 3d group_upload_cost_on_the_gpu -- --ignored --nocapture`)
loads sixteen 1024 × 1024 noise PNGs (64 MiB of RGBA) as a group on the
production renderer, drawing and reading back a frame after each step.
On the same M4 Max (Metal, release, load average 145 to 210), three runs:
the game thread spent 0.28 to 0.99 ms per MiB uploading (18 to 63 ms for
the 64 MiB), a step uploading 16 MiB (the default upload budget) about 4
ms of it, and that step's frame took 0.2 to 2 ms longer to finish on the
GPU than a frame with nothing uploaded. Single steps of 14 to 32 ms and
frames 14 to 29 ms longer showed up in every run, where the busy machine
paused the thread; on a quiet machine expect the low ends. So with the
default budgets a frame that uploads pays about 4 ms on the game thread
plus a millisecond or two on the GPU; a lower upload budget
(`ctx.gfx.set_upload_budget`) spreads that thinner.

With the GPU's own upload time (`gpu_upload_ms`, [What uploads
cost](#what-uploads-cost)), the same test also prints what the timestamps
say. Two runs on the M4 Max (Metal, release, load average 140 to 170): the
GPU copied the 64 MiB in 2.0 to 5.3 ms of its own time (0.03 to 0.08 ms
per MiB), against 28 to 56 ms of the game thread's. So on this GPU the
copies themselves are cheap; the longer frames above are mostly the busy
machine and the frame's other work, which the frame time can't tell apart
and the timestamps can.

## In a browser

In an ordinary web build there are no worker threads, so a group's reads
and decodes run in the group step, one piece of work at a time, and it
stops for the frame once the budget is spent: a loading screen keeps
drawing between files. A build with threads, on a page that allows them,
reads and decodes on job workers as on the desktop ([Threads on the
web](web-threads.md)); the browser test checks a group from `assets/` there.

Without workers, a large file is **decoded in slices**: a PNG texture a few
rows at a time (about 64 KiB of pixels a slice), a sound a few packets at a
time, and your own [sliced loaders](#loaders-that-work-in-slices) a `step`
at a time, with the frame budget checked between slices. So one large
image, a long piece of music or a large level file is spread over as many
frames as it needs instead of taking one long frame, and the pixels and
samples are exactly those of the one-piece decode. Uploading it waits for a
frame with budget left. A group's streamed textures and meshes are decoded
in slices by the streaming step, after the group step and within what it
left of the same budget ([Streaming](streaming.md#browsers-and-asset-sources)).
What can't be split still runs in one piece: other image formats (JPEG and
the rest), interlaced PNGs, your own `with` loaders and `bytes`, and the
geometry of a glTF file (its PNG images are sliced).

The tests run this path natively, decoding on the game thread as a browser
does, and check the slicing by counting rather than timing: each piece of
work counts as a fixed time, so how many a step runs follows from the
budget alone, whatever else the machine is doing. A 2048 × 2048 PNG is 256
slices of 8 rows; at a 0.5 ms budget with pieces counted as 0.125 ms, every
step runs at most four pieces and the image takes 65 steps. On real time,
measured when slicing was added (release, M4 Max, busy machine), the same
PNG decodes in one piece in 2.9 to 4.6 ms and took 7 or 8 steps of a median
0.50 to 0.51 ms at a 0.5 ms budget, 3.4 to 3.7 ms in all. A browser decodes
more slowly, so there a large file spreads over more frames.
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
`ponko_ctx_cancel_asset_group`, `ponko_ctx_restart_asset_group`,
`ponko_ctx_asset_group_stats` (with the upload fields; the GPU's own
upload time and the step's pieces aren't in it yet) and
`ponko_ctx_set_asset_group_frame_budget` match the Rust calls; `ponko_ctx_asset_group_error` returns the failure's
status and sets the last error, its kind and its path.
`ponko_ctx_take_asset_group_bytes` hands a `PONKO_GROUP_BYTES` asset's bytes
to your decode callback once, and `ponko_game_loading_state` makes a loading
state whose callback adds the group's assets.
`ponko_ctx_add_background_memory_source` adds a memory source that workers
read.

In C++, `ponko::asset_group` is the builder, `ctx.start_asset_group`,
`ctx.asset_group_progress`, `ctx.restart_asset_group` and the rest are
`ponko::ctx` methods,
`ctx.asset_group_error(id)` returns an `std::optional<ponko::error>`, and
`ctx.take_asset_group_bytes(id, path, decode)` returns what your
`decode(data, len)` returns. There is no C or C++ form of `with`: take the
bytes and decode them yourself.
