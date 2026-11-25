### Added
- Resolve-once handles where calls took names, following the action id
  pattern: a name is looked up once, and a typo is an error
  (`ErrorKind::NotFound`) whose message lists the names there are, for
  example `unknown audio bus "musci" (known: music, sfx)`.
  - Audio: `Audio::find_bus(name)` finds a bus that exists without making
    one (`bus(name)` still makes it). `Audio::snapshot_id(name)` gives a
    `SnapshotId`; `start_snapshot`, `stop_snapshot` and `snapshot_weight`
    take it (no lookup) as well as the name (`SnapshotRef`). An id stays
    valid when the snapshot is replaced.
  - 2D skeletons: `skeleton2d::ClipId`, from `SkeletonData::clip_id` or
    `Skeleton::clip_id`; `Skeleton::play`, `play_once` and `crossfade` take
    it as well as the name (`ClipRef`). `Skeleton::try_parameter_id`,
    `SkeletonGraph::try_parameter` and `SkeletonGraph::try_state` are the
    checked lookups.
  - `Ui::try_find(name)` for a node that must be there, and
    `CrowdAnimation::try_clip(name)`.
  - `Input::set_touch_control_shown` and `touch_control_center` take an
    `ActionId` as well as the action's name.
- C: `ponko_ctx_audio_find_bus`, `ponko_ctx_audio_snapshot_id`,
  `ponko_ctx_start_audio_snapshot_id`, `ponko_ctx_stop_audio_snapshot_id`,
  `ponko_ctx_audio_snapshot_weight_id`, `ponko_ctx_skeleton2d_clip_index`,
  `ponko_ctx_skeleton2d_play_index`, `ponko_ctx_skeleton2d_crossfade_index`,
  `ponko_ctx_ui_try_find`, `ponko_ctx_set_touch_control_shown_id` and
  `ponko_ctx_touch_control_center_id`. The ABI version is unchanged: only
  functions were added.
- C++: `ctx.find_audio_bus`, `ponko::audio_snapshot_id` with
  `ctx.audio_snapshot_id` and overloads of the snapshot calls taking it,
  `ctx.skeleton2d_clip_index`, `skeleton2d_play_index`,
  `skeleton2d_crossfade_index`, `tree.try_find`, and
  `set_touch_control_shown`/`touch_control_center` taking a
  `ponko::action_id`.

### Changed
- Name lookups that fail now say which names exist: the C calls
  `ponko_ctx_skeleton2d_play`, `ponko_ctx_skeleton2d_crossfade`,
  `ponko_ctx_skeleton2d_state_index` and
  `ponko_ctx_skeleton2d_parameter_index` (and their C++ wrappers) put the
  clip, state or parameter names in `ponko_last_error`.
- `ActionRef` (what the action queries take) accepts any reference to a
  string type (`&Arc<str>`, `&Box<str>`, `&Cow<str>`, ...), not only
  `&str` and `&String`. `SnapshotRef` and `ClipRef` do the same, so the
  calls that took `&str` before still take everything they took.
- The examples, the reference games and the guide's recipes look no names
  up in code that runs every frame: they resolve actions, buses, snapshots,
  animation parameters and UI nodes once and keep the ids (or look UI
  names up only when a click arrives).
