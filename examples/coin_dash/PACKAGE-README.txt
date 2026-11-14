Coin Dash
=========

A small game made with the Ponko engine (https://github.com/ponkoengine/ponko).
Collect every coin before the clock runs out.

Run it
------
Start `coin_dash` (`coin_dash.exe` on Windows) from this folder or from any
other: it finds its `assets` folder next to the program, wherever you unpacked
it and whatever the current folder is. Keep the `assets` folder beside it.

Controls (change them in Settings, press O or the gamepad's North button)
-----------------------------------------------------------------------
  Move            arrow keys, W A S D, or the left stick
  Confirm         Enter, Space, or South (A / Cross)
  Restart round   R, or West
  Pause           Esc, P, or Start (on the title screen: quit)
  Back to title   Q, or East
  Settings        O, or North

Languages
---------
English and Arabic: the Language button in Settings switches between them,
and the choice is saved with the other settings.

Your files
----------
The best time and the settings are saved in your data folder:

  Windows   %APPDATA%\coin-dash\
  macOS     ~/Library/Application Support/coin-dash/
  Linux     $XDG_DATA_HOME/coin-dash/ (or ~/.local/share/coin-dash/)

Delete that folder to start over. A damaged file is set aside as `<name>.bad`
and the game starts from scratch. `PONKO_DATA_DIR=/some/folder` keeps the
files in `/some/folder/coin-dash/` instead.

Check the package
-----------------
  coin_dash --smoke headless [--report report.json]   plays by itself, no window
  coin_dash --smoke window   [--report report.json]   the same, in a real window

Licenses
--------
Coin Dash and the Ponko engine: MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE,
NOTICE). What the game ships: CONTENT-NOTICES.txt. Every library linked into
the program: THIRD-PARTY-NOTICES.txt.
