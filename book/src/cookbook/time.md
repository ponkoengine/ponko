# Time and timers

Fuses, repeating work, easing, a clock for the day and slow motion. Every one of them counts simulation time, not wall-clock time, so they behave the same on a slow machine, in a replay and in a test.

## A fuse that goes off

A component holds the seconds left and a system counts them down. When it reaches zero the fuse sends an event and removes itself, so one explosion is exactly one event.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_time.rs:fuse}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:fuse}}
```

</div>
</div>

## Do something every few seconds

Spawn on a timer. The `while` makes a slow tick lose nothing: the leftover time carries over to the next tick. For a plain "every N ticks", give the system a period instead (the C++ half does).

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_time.rs:repeat}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:repeat}}
```

</div>
</div>

## Slide something with easing

Progress from 0 to 1 over a time, bent by an easing curve, then a linear blend between the two places. Swap `ease_out_cubic` for any function from 0..1 to 0..1.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_time.rs:tween}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:tween}}
```

</div>
</div>

## A day and a night

A cosine of the simulation clock gives the amount of light, 0 at midnight and 1 at noon; the world's tint is a mix of night blue and white. Because it reads the simulation clock, the time of day is the same in a replay.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_time.rs:day_night}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:day_night}}
```

</div>
</div>

## Slow motion

Hold a key and the simulation runs at a quarter speed. Each tick is as long as ever, only fewer of them run each second, so the game plays out exactly as it would at full speed, later.

<div class="lang-pair">
<div>

```rust
{{#include ../../../tests/cookbook_time.rs:slow_motion}}
```

</div>
<div>

```cpp
{{#include ../../../examples/cpp/cookbook.cpp:slow_motion}}
```

</div>
</div>
