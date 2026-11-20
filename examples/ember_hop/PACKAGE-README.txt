Ember Hop
=========

A small platformer made with the Ponko engine (https://gitlab.com/ponkoengine/ponko).
Carry an ember spirit through two caves: collect the sparks, burn or stomp
the drips, light the braziers and reach the lantern at the end of each cave.

Run it
------
Start `ember_hop` (`ember_hop.exe` on Windows; on macOS the app) from this
folder or from any other: it finds its `assets` folder next to the program,
wherever you unpacked it. Keep the `assets` folder beside it.

Controls
--------
  Move            arrow keys, A / D, the left stick or the d-pad
  Jump            Space, Up, W, Z or K, or South (A / Cross); hold for higher
  Flare           X, J or Shift, or West (X / Square) or the right bumper
  Pause           Esc or P, or Start (on the title screen: quit)
  New run         Enter or Space, or South, on the title screen
  Continue        C, or North (Y / Triangle), on the title screen
  To the title    Q, or East (B / Circle), while paused

On a touch screen a stick, Jump and Flare buttons and a pause button appear;
a tap starts a new run and a long press continues one.

A lit brazier keeps your progress: losing the ember (spikes, water, a fall or
a drip) starts you again at the last brazier, or at the start of the cave.

Your files
----------
The saved run and your best results are kept in your data folder:

  Windows   %APPDATA%\ember-hop\
  macOS     ~/Library/Application Support/ember-hop/
  Linux     $XDG_DATA_HOME/ember-hop/ (or ~/.local/share/ember-hop/)

`saves/run.json` is the run (a world save), `progress.json` the best time and
the most sparks. Delete the folder to start over. A damaged progress file is
set aside as `progress.json.bad`. `PONKO_DATA_DIR=/some/folder` keeps the
files in `/some/folder/ember-hop/` instead.

Check the package
-----------------
  ember_hop --smoke headless [--report report.json]   plays by itself, no window
  ember_hop --smoke window   [--report report.json]   the same, in a real window

Licenses
--------
Ember Hop and the Ponko engine: MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE,
NOTICE). What the game ships: CONTENT-NOTICES.txt. Every library linked into
the program: THIRD-PARTY-NOTICES.txt.
