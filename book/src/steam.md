# Shipping on Steam

> **Maturity: [tested](maturity.md).** Game: none. Tests:
> `tests/steam_services.rs` and `crates/ponko_ffi/tests/steam_services.rs`,
> against the fake backend and without Steam; the real Steam calls are a check
> a person runs with two accounts. Platforms: Linux, macOS and Windows,
> headless (the Steam job in the list of checks). Graphics: not involved.
> Missing: a recorded run against a real Steam client, and a released game
> that uses it.

A game on Steam needs more than a build: an app id, achievements and stats,
saves that follow the player to another computer, what friends see the player
doing, a pause when the Steam overlay opens, and typing on a Steam Deck. With
the `steam` feature, `ponko::steam::Steam` does those, and `ponko package`
builds what goes up to Steam. Multiplayer over Steam (lobbies, invites,
relays) is in [Lockstep, rollback and your own protocol](networking.md#over-steam).

```toml
ponko = { version = "0.1", features = ["steam"] }
```

## The app id

Valve gives each game an app id when you sign up on the Steamworks site. Until
then, 480 (Valve's test app "Spacewar") works for development: its
achievements and stats are Valve's, so yours are refused, but everything else
runs.

`Steam::start(app_id)` starts Steam for the game; `Steam::start_launched()`
takes the id Steam launched the game with (or the one in a `steam_appid.txt`
beside the program, while developing; don't ship that file). A game that
also plays online passes its `SteamClient` to `Steam::with_client`, so both
share one client.

**Without Steam, the same build runs.** When the Steam client isn't running
(a player who bought the game elsewhere, a build for another store, a test
machine), `Steam::start` returns an absent Steam instead of failing: every call
does nothing and returns `false` or `None`, and the log says so once
(`Steam is not running (...): achievements, stats, cloud saves, rich presence
and the Steam text input do nothing in this run`). `Steam::absent()` makes one
on purpose. A name Steam refuses, such as an achievement that isn't set up for
the app, is also said once, so a typo shows up in the log the first time.

## A whole game

One `Steam` value, cloned into the systems that need it:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/steam_services.rs:steam_services}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:steam_services}}
```

</div>
</div>

### Achievements and stats

Set them up on the Steamworks site first; the game uses their API names.
`unlock_achievement(name)` shows the achievement to the player at once, and
`clear_achievement` locks it again (for testing). `set_stat_i32` and
`set_stat_f32` set a stat of that type; `achievement`, `stat_i32` and
`stat_f32` read them back. `store_stats()` sends what changed to Steam's
servers: call it at moments that matter (a level done, the game quitting), not
every frame, since Steam limits how often.

### Cloud saves

The game keeps its files in `ctx.storage` as always ([Saves and
settings](storage.md)): that folder stays the game's copy, and the only one
without Steam. `steam.sync_cloud(&mut ctx.storage, &["saves/", "settings.json"])`
keeps Steam Cloud in step with it for the files named (a name ending in `/`
covers a folder):

- A file on one side only is copied to the other.
- A file that differs is copied from the side written last, so the newest
  copy wins: a save made on another computer comes down, and one made while
  Steam was off goes up.

Call it at launch and after saving (or when the player quits). It returns the
files it copied each way. `steam.delete_cloud_file(&mut ctx.storage, name)`
deletes a file from both, so the next sync doesn't bring it back. On the
Steamworks site, give the app a Steam Cloud quota (bytes and files) larger
than the saves; a full cloud is an error naming the file. A player who turned
Steam Cloud off in Steam's settings (`cloud_enabled()` is `false`) keeps
their saves in the folder only.

Steam's other way, Auto-Cloud (the Steamworks site copies a folder with no
code), also works with Ponko's data folder: point it at the folder of
[the app id](storage.md) on each system. Use one way, not both.

### Rich presence

`set_rich_presence("status", "Level 3")` is the line friends see in their
friends list. `"steam_display"` names a localized string set up on the
Steamworks site, which can use the other keys as `%key%`. An empty value
removes a key; `clear_rich_presence()` removes them all.

### The overlay

Shift+Tab (or the Steam button) opens the Steam overlay over the game.
`overlay_open()` says whether it's open, as of the last `update()`: pause the
game while it is (`ctx.time.pause()`). `update()` runs Steam's callbacks;
call it once a frame. The [Steam transports](networking.md#over-steam) run
the same callbacks, and calling both is fine.

### Steam Deck and Big Picture text input

A Deck or a TV with a gamepad has no keyboard. Two ways to type:

- `show_floating_keyboard(mode, x, y, width, height)`: the Deck's on-screen
  keyboard, placed so it doesn't cover the text field at that rectangle
  (pixels from the window's top left). Its keys arrive as ordinary typed text
  (`ctx.input`), so the game's own text field works unchanged.
- `show_text_input(mode, description, existing, max_chars)`: Steam's
  full-screen text dialog, in Big Picture mode and on a Deck. When the player
  closes it, `take_text_input()` (after `update()`) returns
  `SteamText::Submitted(text)` or `SteamText::Cancelled`, once.

`SteamTextMode` picks one line, several lines, a password, an email address or
digits. Both return `false` when they didn't open (not on a Deck, not in Big
Picture, Steam absent): show the game's own keyboard then. `is_steam_deck()`
and `is_big_picture()` say where the game runs.

### Testing a game's Steam logic

`Steam::fake()` returns a `Steam` backed by an in-memory Steam and the
`FakeSteam` that controls it, so a game's own Steam code runs in headless
tests: set up achievements and stats (`define_achievements`, `define_stats`;
other names are refused, as Steam would), open the overlay (`set_overlay`),
play on a Deck (`set_steam_deck`, `set_big_picture`), type and close the text
input (`close_text_input`), put cloud files written on "another computer"
(`put_cloud_file`), fill the cloud (`set_cloud_full`), and read back what the
game did (`achieved`, `stat`, `stores`, `rich_presence`, `cloud_file`). The
test above, `tests/steam_services.rs`, is one.

## Depots from `ponko package`

Steam ships a game as depots: one per system, each a folder that SteamPipe
uploads. `ponko package` makes those folders ([A complete game,
packaged](packaging.md)): run it on each system (or with `--target`) and the
folder `target/package/<name>-<version>-<os>-<arch>/` is a depot's content,
next to the archive made from it.

For a game built with `steam`, the package holds Valve's library beside the
program: `steam_api64.dll` on Windows, `libsteam_api.so` on Linux, and
`libsteam_api.dylib` in the app's `Contents/MacOS` on macOS. `ponko package`
finds it where the `steamworks` build put it and says so (`beside the program:
...`). macOS finds it there by itself. **On Linux**, the program finds it
only when linked to look beside itself; add this to the game's `build.rs`:

```rust
// build.rs, beside Cargo.toml
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
```

A SteamPipe build script names the depots and the folders; with Valve's
`steamcmd`:

```text
"AppBuild"
{
    "AppID" "<your app id>"
    "Desc" "0.3.0"
    "ContentRoot" "target/package"
    "BuildOutput" "target/steam-build"
    "Depots"
    {
        "<windows depot id>" { "FileMapping" { "LocalPath" "my-game-0.3.0-windows-x86_64/*" "DepotPath" "." "Recursive" "1" } }
        "<macos depot id>"   { "FileMapping" { "LocalPath" "my-game-0.3.0-macos-aarch64/*" "DepotPath" "." "Recursive" "1" } }
        "<linux depot id>"   { "FileMapping" { "LocalPath" "my-game-0.3.0-linux-x86_64/*" "DepotPath" "." "Recursive" "1" } }
    }
}
```

```sh
steamcmd +login <build account> +run_app_build "$PWD/app_build.vdf" +quit
```

On the Steamworks site, each launch option names the program inside its depot:
`my_game.exe`, `my_game`, or `My Game.app` on macOS. A macOS build that
players download needs signing and notarizing ([the hook](packaging.md#signing-and-notarizing-the-hook)).
The Linux depot is also what a Steam Deck runs.

## Before Steam's review

Check these on a real Steam client, from the build Steam installed (not from
`cargo run`), on each system you ship:

- The game starts from the Steam library, with no `steam_appid.txt`, and the
  log has no "Steam is not running" line.
- An achievement unlocks with Steam's pop-up and shows on the game's page;
  stats survive a restart.
- A save made on one computer is there on another (or after deleting the
  local folder), and the newest copy wins.
- Friends see the rich presence; the overlay opens with Shift+Tab and the
  game pauses.
- On a Steam Deck: the game is playable with the controls alone, text fields
  bring up a keyboard, and it fits 1280×800. Steam's Deck compatibility review
  checks the same.
- With Steam closed (or a build for another store), the game still starts and
  plays, saving to its folder.

The engine's own list of these runs, for a person with two Steam accounts, is
issue #484.

## From C and C++

The C functions are `ponko_steam_*` (one Steam per process, started by
`ponko_steam_start`) and `ponko_ctx_steam_sync_cloud`; C++ has the
`ponko::steam` namespace and `ctx.steam_sync_cloud`. Build ponko_ffi with
`--features steam`; without it every call does nothing, as without Steam. The
fake (`Steam::fake`, `FakeSteam`) is Rust-only: C and C++ tests run against
an absent Steam.

## Not covered

Workshop, inventory, purchases and leaderboards aren't wrapped yet. Until
they are, `steam.client()` gives the `SteamClient`, and its
`unstable_steamworks()` the `steamworks` crate's client, which is
[unstable](https://github.com/ponkoengine/ponko/blob/main/STABILITY.md#unstable).
