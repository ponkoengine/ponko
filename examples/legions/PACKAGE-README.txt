Legions
=======

A battle of 100,000 units made with the Ponko engine (https://github.com/ponkoengine/ponko).
Two armies of soldiers and archers march across a map of rocks, lakes and
mud and fight until one is gone. You command red; blue attacks. Every
battle can be recorded and played again, bit for bit.

Run it
------
Start `legions` (`legions.exe` on Windows; on macOS the app). It needs no
files next to it.

  legions --units 20000            a smaller battle, for slower computers
  legions --seed 7                 another map

Controls
--------
  Left click      send red to that point
  A               red attacks the blue army
  H               red holds where it stands
  R               a new round
  Right drag      pan          Mouse wheel   zoom
  Space           pause        1 / 2 / 3     speed 1x, 2x, 4x
  Esc             quit

Replays
-------
  legions --record my-battle.json         play and record your orders
  legions --replay my-battle.json --watch watch it again
  legions --replay skirmish.json          play it again without a window and
                                          check it ends exactly as recorded

A replay holds the map's seed, the army size, your orders with the tick
each one took effect, and a hash of the whole battle at the end. Played
again, on any computer and any number of cores, it must end with the same
hash; `--replay` says whether it did. The recording is written when you
quit with Esc, and every ten seconds while you play.

Check the package
-----------------
  legions --replay skirmish.json
  legions --headless --ticks 600          time 600 ticks of the battle

Licenses
--------
Legions and the Ponko engine: MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE,
NOTICE). What the game ships: CONTENT-NOTICES.txt. Every library linked into
the program: THIRD-PARTY-NOTICES.txt.
