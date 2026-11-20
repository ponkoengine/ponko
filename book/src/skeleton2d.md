# 2D skeletons: bones, skinned sprites and blending

Frame-by-frame animation (see [Sprite sheets, animation,
tilemaps](tiles.md)) draws every pose. A 2D skeleton draws the parts once
and moves them: a hierarchy of bones carries rigid sprite parts (an arm, a
head) and bends 2D meshes whose vertices follow up to four bones (a tail, a
cape, a torso that squashes). Clips key the bones; the engine blends them,
crossfades between them, runs a state machine over them and solves IK on
top. This is often called cutout or bone animation.

Everything lives in `ponko::skeleton2d`. There is no cargo feature to turn
on, and a game without skeletons pays nothing: no system work beyond an
empty query, no draw calls, no GPU memory.

C and C++ games use them too (see [From C and C++](#from-c-and-c) below).

## The pieces

| Type | What it is |
|---|---|
| `SkeletonData` | The shared description, loaded once: bones and their rest pose, slots, skins and attachments, clips, IK, transform, path and physics constraints, an optional state machine. |
| `Bone` | A name, a parent and a rest `BonePose` (translation, rotation, scale, shear) relative to the parent. |
| `Slot` | A place on a bone that shows one attachment at a time, in draw order. |
| `SpritePart` | A rigid attachment: a texture (or a region of one) with a size, pivot and offset from its bone. |
| `SkinnedMesh` | A bending attachment: vertices with UVs and up to four weighted bones each, and triangles. |
| `ClippingMask` | A polygon on bones that clips what the next slots draw, up to an end slot. |
| `PathShape` | A chain of cubic beziers on bones that draws nothing: path constraints place bones along it. |
| `Clip` | Keys for bones (translate, rotate, scale, shear, or one axis at a time), slots (attachment, visibility, color), mesh deforms, constraint mixes and settings, draw order and events. |
| `TransformConstraint` | Bones that take on some of a target bone's rotation, position, scale and shear. |
| `PathConstraint` | Bones placed one after another along a `PathShape` and turned along it (a snake, a tentacle, a chain on a track). |
| `PhysicsConstraint` | A bone that lags behind its pose and springs back (hair, a tail, a feather), stepped at a fixed rate. |
| `SkeletonGraph` | The state machine: states that play a clip or a 1D blend, with the same parameters, conditions and transitions as the 3D animator (`ponko::anim`). |
| `Skeleton` | The component: what plays, its clock and its pose. |

Coordinates match the rest of 2D: y points down and rotations are radians,
clockwise on screen. Clip keys are offsets from the rest pose: a translate
key adds, a rotate key adds, a scale key multiplies.

## Playing a skeleton

Load a file (the format is below) and put a `Skeleton` on an entity with a
`Transform`. The built-in `update_skeletons2d` system advances it at the
end of `Phase::Animate`, and the sprite pass draws it:

<div class="lang-pair">
<div>

```rust
# use ponko::skeleton2d::Skeleton;
let hero = ctx.load_skeleton2d("hero/hero.json");
let mut puppet = Skeleton::new(hero);
puppet.play("walk");
ctx.world.spawn((ponko::d2::Transform::at(vec2(200.0, 300.0)), puppet));
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:skeleton2d_play}}
```

</div>
</div>

Its parts are drawn in the same batches and `z` order as sprites: set
`puppet.z` to place it among them. A `BlendMode` or `SpriteMaterial` on the
entity applies to every part, and `puppet.color` tints them all.

`play` loops, `play_once` holds the last pose (`finished()` turns true),
`crossfade(clip, seconds)` blends from the current pose to the new clip.
`speed` scales the clock. Set `manual = true` to advance a skeleton
yourself with `update(dt)`.

Events keyed in clips (footsteps, the frame a sword hits) are collected
while the skeleton advances; read them from a system:

<div class="lang-pair">
<div>

```rust
# use ponko::skeleton2d::Skeleton;
ctx.world.query::<&mut Skeleton>().for_each(|s| {
    for event in s.drain_events() {
        if &*event == "step" {
            // play a footstep
        }
    }
});
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:skeleton2d_events}}
```

</div>
</div>

## Blending and the state machine

A file (or `SkeletonData::set_graph`) can give the skeleton a state
machine. Its states play a clip or a 1D blend: clips at thresholds of a
float parameter, the two around its value mixed by distance (a walk at 0
and a run at 1 make a jog at 0.5). Blended clips keep their phase, and the
state lasts their weighted length, so feet stay in step.

Transitions use `ponko::anim`'s `AnimationTransition` and
`AnimationCondition`, the same types as the 3D animator: a condition on a
float, bool or trigger parameter, an exit time, a crossfade duration and an
offset into the next state. Triggers reset when a transition uses them.

<div class="lang-pair">
<div>

```rust
# use ponko::skeleton2d::Skeleton;
# let hero = ctx.load_skeleton2d("hero/hero.json");
let mut puppet = Skeleton::new(hero);
let speed = puppet.parameter_id("speed").expect("the file declares it");
puppet.set_float(speed, 0.5);
let hit = puppet.parameter_id("hit").expect("the file declares it");
puppet.set_trigger(hit, true);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:skeleton2d_graph}}
```

</div>
</div>

`set_float`, `set_bool`, `set_trigger` and `crossfade_state` panic on a
parameter of the wrong kind, a value that isn't a number or a state that
isn't there. With ids from outside (a file, a tool, a script), use
`try_set_float`, `try_set_bool`, `try_set_trigger`, `try_crossfade_state`
and `try_play_state`: they return an `AnimationGraphError` saying why and
change nothing. The C and C++ calls are the checked ones.

The graph only plays what the data says; `play`, `play_once` and
`crossfade` still work and take over until `crossfade_state` (or a new
skeleton) hands control back to the graph.

Crossfades and 1D blends mix everything that comes in amounts: bone
poses, mesh deforms (a clip that doesn't deform a mesh counts as no
offset, so a deform fades in and out rather than snapping), and the
constraint mixes and settings clips key (a side that doesn't key one uses
the constraint's own value). What can't be mixed comes from the stronger
side: attachments, visibility, colors, draw order, an IK chain's bend
direction, events and physics resets.

## Slots, skins and sprite swaps

Each slot shows one attachment, named. Clips swap them (an open hand for a
fist, a blinking head), hide or tint slots, and reorder them. Code can do
the same: `set_attachment(slot, Some(Some("fist")))` overrides the clip
until `set_attachment(slot, None)` hands it back.

A skin is a set of attachments under the same names (a red and a gold
armor): `set_skin("gold")` swaps them all, and an attachment a skin
doesn't have comes from the default skin.

## IK and look-at

An `IkConstraint` with two bones bends the chain so the end of the second
bone reaches a target; with one bone it turns the bone to point at the
target. The target is a bone of the skeleton, or a point the game gives in
the skeleton's own space (before the entity's transform):

<div class="lang-pair">
<div>

```rust
# use ponko::skeleton2d::Skeleton;
# fn aim(puppet: &mut Skeleton, mouse: Vec2, origin: Vec2) {
puppet.set_ik_target(0, Some(mouse - origin)); // constraint 0 reaches the mouse
puppet.set_ik_mix(0, 0.75);                    // three quarters of the way
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:skeleton2d_ik}}
```

</div>
</div>

Constraints apply after the clips every update. For one-off posing,
`look_at(bone, target, mix)` and `two_bone_ik(upper, lower, target,
bend_positive, mix)` change the current pose directly, until the next
update poses it again (call them after it, in a late system, or set
`manual`). A target out of reach straightens the chain toward it.

<div class="lang-pair">
<div>

```rust
# use ponko::skeleton2d::Skeleton;
# fn pose(puppet: &mut Skeleton, mouse: Vec2, origin: Vec2) {
if let (Some(head), Some(upper), Some(lower)) = (puppet.bone("head"), puppet.bone("arm"), puppet.bone("hand")) {
    puppet.look_at(head, mouse - origin, 1.0);                     // the head follows the mouse
    puppet.two_bone_ik(upper, lower, mouse - origin, true, 0.5);   // the arm half way to it
}
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:skeleton2d_one_off}}
```

</div>
</div>

Clips can key the constraints too: an `IkTrack` keys an IK constraint's
mix and bend direction, a `TransformTrack` its mixes, a `PathTrack` a path
constraint's position, spacing and mixes, and a `PhysicsTrack` a physics
constraint's settings (or every one's) and when its springs start over.
While a clip keys a mix it replaces the constraint's own; `set_ik_mix`
sets the mix used while no clip keys it.

## Paths and physics

A `PathShape` is an attachment like a mesh, but it draws nothing: three
vertices per point (the handle before it, the point, the handle after it)
make a chain of cubic beziers that bends with its bones. A
`PathConstraint` places its bones along the path that a slot shows: the
first at `position` (units, or a fraction of the path with
`PathPosition::Percent`), the next ones `spacing` apart (each bone's
length plus `spacing` by default; `PathSpacing` has fixed, percent and
proportional gaps too), turned along the path (`PathRotate::Tangent`), or
each pointing at the next place with the chain keeping its lengths
(`Chain`) or stretching to reach it (`ChainScale`). Positions are measured
by distance along the curves unless the path says otherwise.

A `PhysicsConstraint` makes a bone lag and spring back as the skeleton
moves: its position (`x`, `y`), its turn (`rotate`, `shear_x`) and its
stretch (`scale_x`) each follow a damped spring stepped `1 / step` times a
second whatever the frame rate, pushed by `wind` (+x) and `gravity` (down
the screen), with Spine's `inertia`, `strength`, `damping` and `mass`.
Each `Skeleton` keeps its own springs; `reset_physics()` starts them over
where the bones are (after a teleport, say).

```rust
use ponko::skeleton2d::{BonePose, MeshVertex, PathConstraint, PathRotate, PathShape, PhysicsConstraint, SkeletonData};

let mut data = SkeletonData::new();
let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
let mut tail = Vec::new();
let mut last = root;
for i in 0..4 {
    let at = if i == 0 { Vec2::ZERO } else { vec2(8.0, 0.0) };
    last = data.add_bone(&format!("tail{i}"), Some(last), BonePose::at(at), 8.0);
    tail.push(last);
}
// A wavy track on the root: two points, each with a handle on either side.
let on = |x: f32, y: f32| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(root, 1.0)]);
let wave = PathShape::new(vec![
    on(-10.0, 0.0), on(0.0, 0.0), on(10.0, -10.0),
    on(30.0, 10.0), on(40.0, 0.0), on(50.0, -10.0),
]);
let track = data.add_slot("track", root);
data.add_attachment(track, "track", wave);
let mut slither = PathConstraint::new("slither", &tail, track);
slither.rotate_mode = PathRotate::Chain;
data.add_path_constraint(slither);
// A feather at the tip that swings behind as the tail moves.
let feather = data.add_bone("feather", Some(last), BonePose::at(vec2(8.0, 0.0)), 6.0);
let mut sway = PhysicsConstraint::new("sway", feather);
sway.rotate = 1.0;
sway.gravity = 40.0;
data.add_physics_constraint(sway);
```

Constraints of every kind apply after the clips, in the order they were
added (a file's `order`), each seeing what the ones before it did.

## Building one in code

The `skeleton2d` example builds a puppet with rigid parts, a skinned tail,
a walk/run blend, a jump with a sprite swap, look-at and arm IK (`cargo run
--example skeleton2d`):

```rust
{{#include ../../examples/skeleton2d.rs:skeleton2d}}
```

## The file format

A skeleton file is JSON with `"format": "ponko-skeleton2d"` and
`"version": 1`. `ctx.load_skeleton2d(path)` reads it through the asset
system (any [asset source](asset-sources.md)), loads its images (relative
to the file's folder) and caches it; `try_load_skeleton2d` returns the
error instead of panicking. With hot reload on, saving the file reloads
it, and every skeleton playing it keeps its clip or state, clock,
parameters and skin by name.

```json
{
  "format": "ponko-skeleton2d", "version": 1,
  "bones": [
    { "name": "root" },
    { "name": "thigh", "parent": "root", "y": -10, "rotation": 90, "length": 10 },
    { "name": "shin", "parent": "thigh", "x": 10, "length": 10 },
    { "name": "foot-goal", "parent": "root", "x": 5, "y": 8 }
  ],
  "slots": [
    { "name": "leg", "bone": "thigh", "attachment": "leg" },
    { "name": "body", "bone": "root", "attachment": "body", "color": "#ff000080" }
  ],
  "skins": [
    { "name": "default", "attachments": [
      { "slot": "body", "name": "body", "image": "body.png", "size": [8, 8] },
      { "slot": "leg", "name": "leg", "type": "mesh", "image": "leg.png",
        "vertices": [
          { "position": [-1, -10], "uv": [0, 0], "bones": ["thigh"] },
          { "position": [1, -10], "uv": [1, 0], "bones": ["thigh"] },
          { "position": [0, 10], "uv": [0.5, 1], "bones": ["thigh", "shin"], "weights": [1, 3] }
        ],
        "triangles": [0, 1, 2] }
    ] },
    { "name": "gold", "attachments": [
      { "slot": "body", "name": "body", "image": "gold.png", "region": [0, 0, 2, 2] }
    ] }
  ],
  "ik": [{ "name": "leg", "bones": ["thigh", "shin"], "target": "foot-goal", "mix": 0 }],
  "clips": [
    { "name": "idle", "duration": 1, "events": [{ "time": 0.5, "name": "blink" }] },
    { "name": "walk", "bones": [{ "bone": "thigh", "rotate": [
        { "time": 0, "value": 0 }, { "time": 0.5, "value": 30, "curve": "stepped" }, { "time": 1, "value": 0 } ] }],
      "slots": [{ "slot": "body", "visible": [{ "time": 0.5, "value": false }] }],
      "draw_order": [{ "time": 0.5, "order": ["body", "leg"] }] }
  ],
  "graph": {
    "parameters": [{ "name": "speed", "float": 0 }, { "name": "hit", "trigger": false }],
    "states": [
      { "name": "move", "blend1d": { "parameter": "speed", "clips": [[0, "idle"], [1, "walk"]] } },
      { "name": "hurt", "clip": "idle", "loop": false }
    ],
    "transitions": [{ "to": "hurt", "when": [{ "trigger": "hit" }], "duration": 0.1 }],
    "initial": "move"
  }
}
```

Unknown fields are errors, so a typo doesn't silently do nothing. Names
refer to things by name; angles are degrees (clockwise, y down), and the
engine turns them into radians.

**Top level.** `format`, `version` (1) and `bones` are required; `slots`,
`skins`, `ik`, `transform`, `path`, `physics`, `clips` and `graph` are
optional, and so are `premultiplied` (`true`: the images' colors are
premultiplied by alpha, as a `pma` atlas page's) and `nearest` (images,
relative to the file, sampled without smoothing).

**`bones`**, parents before children:

| Field | Default | Meaning |
|---|---|---|
| `name` | required | Unique. |
| `parent` | none (a root) | The parent bone's name. |
| `x`, `y` | 0 | Rest translation, in the parent's space. |
| `rotation` | 0 | Rest rotation, degrees. |
| `scale` | `[1, 1]` | Rest scale. |
| `shear` | `[0, 0]` | Rest shear of its x and y axes, degrees. |
| `length` | 0 | Its length along +x, for tools, IK, paths and physics. |

**`slots`**, in draw order (first drawn first):

| Field | Default | Meaning |
|---|---|---|
| `name` | required | Unique. |
| `bone` | required | The bone it follows. |
| `attachment` | none | What it shows at rest, from the default skin. |
| `color` | white | Tint, `#rrggbb` or `#rrggbbaa` (or `[r, g, b, a]`, 0 to 1 each, for a color those can't hold). |

**`skins`**: `name` (the first skin is the default one; call it `default`)
and `attachments`, each:

| Field | Default | Meaning |
|---|---|---|
| `slot`, `name` | required | Where it goes and what it is called. |
| `type` | `"sprite"` | `"sprite"` (rigid), `"mesh"` (skinned), `"clipping"` (a mask) or `"path"`. |
| `image` | required for sprites and meshes | The image, relative to the file's folder. |
| `color` | white | Tint. |
| `region` | the whole image | Sprites: `[x, y, width, height]` in image pixels. |
| `size` | the region's size | Sprites: size in world units (one of `size` or `region` is required). |
| `pivot` | `[0.5, 0.5]` | Sprites: which point of the part (0..1 across it) sits at its offset from the bone. |
| `x`, `y`, `rotation`, `scale` | 0, 0, 0, `[1, 1]` | Sprites: offset from the bone. |
| `vertices` | required for meshes, masks and paths | `position` (in the skeleton's rest space), `uv` (0..1, meshes only), `bones` (names) and `weights` (one per bone; optional with one bone). The four strongest bones are kept; weights are normalized. |
| `triangles` | required for meshes | Meshes: three vertex indices per triangle. |
| `end` | none (every slot after it) | Clipping masks: the last slot it clips. |
| `closed` | `false` | Paths: it also runs from its last point back to the first. |
| `constant_speed` | `true` | Paths: positions along it are measured by distance; `false` uses `lengths`. |
| `lengths` | none | Paths not measured by distance: the length from the start to the end of each curve. |

A path's vertices come three per point: the handle before it, the point,
the handle after it.

**`ik`**: `name`, `bones` (one, or two where the second is a child of the
first), `target` (a bone), `bend_positive` (default `true`: the joint turns
clockwise from the line to the target) and `mix` (default 1).

**`transform`**: `name`, `bones`, `target` (a bone), the mixes
`mix_rotate`, `mix_translate` (`[x, y]`), `mix_scale` (`[x, y]`) and
`mix_shear_y` (each default 1), the offsets `rotation` (degrees), `x`, `y`,
`scale` (`[x, y]`, added) and `shear_y` (degrees), all default 0, and
`local` and `relative` (default `false`).

**`path`**: `name`, `bones`, `target` (the slot showing the path),
`position_mode` (`"percent"`, the default, or `"fixed"`), `spacing_mode`
(`"length"`, the default, `"fixed"`, `"percent"` or `"proportional"`),
`rotate_mode` (`"tangent"`, the default, `"chain"` or `"chain_scale"`),
`rotation` (degrees added), `position`, `spacing` (default 0),
`mix_rotate` (default 1) and `mix_translate` (default `[1, 1]`).

**`physics`**: `name`, `bone`, how much each part follows its spring
(`x`, `y`, `rotate`, `scale_x`, `shear_x`, default 0), `limit` (default
5000), `step` (seconds per step, default 1/60), `inertia` (1), `strength`
(100), `damping` (1), `mass` (1), `wind` (0), `gravity` (0), `mix` (1) and
`reference_scale` (100).

Constraints apply in the order of their `order` field (default 0), then IK,
transform, path and physics constraints, each in file order.

**`clips`**: `name`, `duration` (default: the last key's time), and:

- `bones`: `bone` and key lists `translate` (`[x, y]`), `rotate` (degrees),
  `scale` (`[x, y]`) and `shear` (`[x, y]`, degrees), and one axis at a
  time with its own key times: `translate_x`, `translate_y`, `scale_x`,
  `scale_y`, `shear_x` and `shear_y` (degrees), which replace that axis.
- `slots`: `slot` and key lists `attachment` (a name, or `null` to hide it),
  `visible` (`true`/`false`) and `color` (`#rrggbbaa`).
- `deform`: `slot`, `skin` (default `"default"`), `attachment` (a mesh, mask
  or path) and `keys`, each value a list of `[x, y]` offsets: one per
  vertex and bone, in the order the attachment lists its vertices and each
  vertex's bones, in that bone's space at rest.
- `ik`: `constraint` and key lists `mix` and `bend_positive`.
- `transform`: `constraint` and key lists `mix_rotate`, `mix_translate`
  (`[x, y]`), `mix_scale` (`[x, y]`) and `mix_shear_y`.
- `path`: `constraint` and key lists `position`, `spacing`, `mix_rotate`
  and `mix_translate` (`[x, y]`).
- `physics`: `constraint` (default: every physics constraint), key lists
  `inertia`, `strength`, `damping`, `mass`, `wind`, `gravity` and `mix`,
  and `reset`, a list of times at which the springs start over.
- `draw_order`: `{ "time", "order" }` where `order` names every slot once,
  or is `null` for the rest order.
- `events`: `{ "time", "name" }`.

A key is `{ "time", "value", "curve" }`: `time` in seconds (default 0),
and `curve` how the value moves from this key to the next:

- `"linear"` (the default): straight.
- `"stepped"`: hold until the next key.
- `{ "bezier": [x1, y1, x2, y2] }`: a cubic bezier from (0, 0) to (1, 1)
  through two handles, as CSS's `cubic-bezier`: x is the fraction of the
  time between the keys (0 to 1), y the fraction of the value's change (it
  may overshoot). `[0.42, 0, 0.58, 1]` eases in and out.
- `{ "bezier": [[...], [...]] }`: one bezier per channel (x and y of a
  translate or scale, r, g, b and a of a color).

Beziers are evaluated exactly, not approximated by line segments. A
rotation goes straight from one key's value to the next: from 350° to 10°
turns back 340°; key 370° to turn 20° forward.

**`graph`**: `parameters` (each a `name` and one of `"float": 0`,
`"bool": false` or `"trigger": false`), `states`, `transitions` and
`initial` (default: the first state). A state has a `name`, exactly one of
`clip` or `blend1d` (`parameter` and `clips`, a list of `[threshold,
clip]` in increasing order), `speed` (default 1) and `loop` (default
`true`). A transition has `from` (default: any state), `to`, `when` (a list
of `{ "greater": [parameter, value] }`, `{ "less": [...] }`,
`{ "bool": [parameter, value] }` or `{ "trigger": parameter }`, all of
which must hold), `exit_time` (the source's normalized time it waits for),
`duration` (crossfade seconds) and `offset` (the next state's starting
normalized time).

`SkeletonData::to_json(path)` writes a skeleton in this format, to be
saved as asset `path` (images are written relative to its folder). It
writes everything a skeleton holds, so an imported Spine export can be
kept in Ponko's format without losing anything: read back, it is the same
skeleton, every number bit for bit, and it plays the same. An attachment
showing an image made in code (with no file) can't be written.

### Importing Spine JSON

`load_skeleton2d` also reads Spine JSON exports (Spine 4.x, and 3.x files),
recognized by their `"skeleton": { "spine": ... }` header. Spine is y-up
with counter-clockwise angles; the importer mirrors y, so poses look the
same.

Images come from the texture atlas Spine's packer writes next to the
export: `hero.json` reads `hero.atlas` beside it, and its page images
(`hero.png`, `hero2.png`, ...) next to that. Regions packed turned a
quarter (`rotate`) and trimmed regions (whitespace cut off, `offsets`)
draw as they were authored, a page with `pma: true` draws premultiplied
(unless the entity has its own `BlendMode`) and `filter: Nearest` keeps
pixel art sharp. Without an atlas, each attachment is a loose image from
the export's `images` folder (relative to the file) or from next to the
file, as `<attachment path or name>.png`. Editing the atlas or the export
reloads every skeleton playing it. To read an export from memory, use
`SpineAtlas::parse` and `SkeletonData::from_spine_json_with_atlas`.

Read: bones (position, rotation, scale, shear, length), slots (rest
attachment, color), every skin's region, mesh, clipping and path
attachments (weighted meshes keep each vertex's four strongest bones), IK
constraints (with their mix and bend direction), transform constraints
(world or local, absolute or relative, every mix and offset; 4.x and 3.x
names), path constraints (every position, spacing and rotate mode) and
physics constraints (Spine 4.2's springs, with their own step rate),
applied in their `order`, and animations' bone timelines (rotate, translate, scale and shear, and 4.x's one-axis
`translatex`, `translatey`, `scalex`, `scaley`, `shearx` and `sheary`,
each with its own key times), deform timelines (4.x under `attachments`,
3.x under `deform`; offsets of weighted meshes stay with their bones),
slot attachment and color timelines, IK, transform, path and physics
timelines (mixes, bend directions, positions, spacings, physics settings
and resets), draw order and events, with their curves: linear, stepped and bezier, exactly as Spine evaluates them. 3.x
rotations turn the shorter way between keys, as 3.x does.

A clipping attachment clips the slots after its own up to its `end` slot,
regions and meshes alike, to any simple polygon. Deforms and constraint
mixes blend across crossfades and 1D blends like bone poses do. Physics
runs as Spine's does, mirrored to y-down: gravity pulls down the screen.

Not read: inherit modes other than normal, skin-specific bones and
constraints, dark colors, slot blend modes, hidden slots, sequences,
linked meshes, bounding boxes, points, IK softness, compress, stretch and
uniform scale (and their keys), physics settings marked global (an
animation's physics keys for all constraints key every one), event
values, and timelines for inherit, `rgb`, `alpha` and two-color tints and
sequences. A file using any of them still loads, and each one it uses is
a warning naming the feature, where it is and the file:

```text
hero/hero.json: Spine inherit mode "noScale" (bone "arm") isn't read; the bone inherits its parent's rotation and scale as "normal" does
```

`load_skeleton2d` prints the warnings (to the browser console on the web);
`SkeletonData::warnings()` lists them, so a test can assert an export has
none. Fields the importer doesn't know warn the same way. A missing atlas
region or image, or a region packed turned other than 0 or 90 degrees, is
an error.

## From C and C++

C and C++ games load skeleton files and put skeletons on entities:
`ponko_ctx_load_skeleton2d` (which also reports the Spine warnings),
`ponko_ctx_set_skeleton2d` and `ponko_ctx_remove_skeleton2d` (C++:
`ctx.load_skeleton2d`, `ctx.set_skeleton2d`). The skeleton lives on its
entity, and every call names the entity: `ponko_ctx_skeleton2d_play`,
`_crossfade`, `_crossfade_state`, `_state`, `_set_float`, `_set_bool`,
`_set_trigger`, `_next_event`, `_set_ik_target`, `_clear_ik_target`,
`_set_ik_mix`, `_look_at`, `_two_bone_ik` (one-off posing, after the
update), `_bone`, `_set_attachment`, `_reset_attachment`,
`_attachment`, `_set_skin`, `_set_look` (z, tint and speed), `_set_manual`
and `_update`. Names become indices once with `_state_index`,
`_parameter_index`, `_ik_index`, `_bone_index` and `_slot_index`.
Calls with a wrong index, kind or value fail with
`PONKO_INVALID_ARGUMENT` and the reason in `ponko_last_error` (C++ throws
it), changing nothing; a missing clip, skin or name is `PONKO_NOT_FOUND`.
The built-in update is `PONKO_SYSTEM_UPDATE_SKELETONS2D`
(`ponko::systems::update_skeletons2d`).

Building skeletons in code (`SkeletonData::add_bone` and the rest) and
`SpineAtlas` are Rust-only: C and C++ skeletons come from files through
the assets (a memory source for files held in memory).

## Saves, scenes, prefabs and the inspector

A `Skeleton` is reflected as `"Skeleton2D"`: the file it plays, the clip or
state, the normalized time, looping, speed, `z`, color, skin, parameter
values and `manual`. Saves, scenes and prefabs keep an animated skeleton
where it was, and loading a scene loads the skeleton files it names first.
The inspector shows and edits the same fields. A skeleton built in code has
no file to name, so it isn't saved (its entity's other components are).

```json
{ "Skeleton2D": { "skeleton": "hero/hero.json", "state": "move", "time": 0.25,
                  "parameters": { "speed": 0.5 }, "skin": "gold" } }
```

Only `skeleton` is required; the rest default to the skeleton's start.

## Cost

Measured with `cargo run --release -p ponko_bench -- skeleton2d` on an
Apple M4 Max (14 cores), with the machine busy with other builds (load
average about 75, so medians are noisy and the minimum is closer to the
unloaded cost). Each of 1,000 skeletons has 16 bones, 15 rigid parts, two
skinned meshes of 20 vertices and 24 triangles, a walk/run 1D blend keying
every bone, and a two-bone IK constraint; one frame advances them all (on
all cores), draws them, then orders and batches the draw list:

| Benchmark | Median | Minimum | Drawn |
|---|---|---|---|
| `skeleton2d/1k` | 11.1 ms | 3.6 ms | 234,000 vertices, 1 batch |
| `skeleton2d/1k-z` (two layers) | 19.3 ms | 6.8 ms | 234,000 vertices, 1 batch |

Parts on one texture (or atlas) and one look batch into one draw call.

Posing and drawing are timed on their own by `skeleton2d/update-1k`,
`draw-1k`, `update-10k` and `draw-10k` (the same skeletons, 1,000 or 10,000
of them). Posing runs on all cores. Drawing splits the skeletons into runs,
and each thread builds its runs' vertices into its own chunk. The chunks are
then copied into the draw list in draw order, so the result is the same as
drawing them one by one (posing was on all cores already). Three runs
each, before (drawing on one thread) and
after, taken in turns on the same busy machine (load average 50 to 90).
These are **to be repeated on a quiet machine**:

| Benchmark | Before: median | Before: min | After: median | After: min |
|---|---|---|---|---|
| `update-1k` | 3.5 to 7.2 ms | 0.64 to 0.75 ms | 1.1 to 2.8 ms | 0.49 to 0.52 ms |
| `draw-1k` | 3.8 to 6.2 ms | 2.5 to 2.6 ms | 3.1 to 21 ms | 0.63 to 0.87 ms |
| `update-10k` | 7.8 to 30 ms | 5.4 to 6.0 ms | 19 to 30 ms | 4.0 to 5.0 ms |
| `draw-10k` | 29 to 89 ms | 25.6 to 26.6 ms | 33 to 50 ms | 5.6 to 8.4 ms |

The minimums (the closest to an idle machine) show drawing three to four
times faster. The medians are within the noise of the other work on the
machine: a parallel draw waits for cores that other processes hold.
Posing, drawing and clipping allocate nothing per frame once warm, apart
from rayon's job queue taking a new block now and then on all cores.
