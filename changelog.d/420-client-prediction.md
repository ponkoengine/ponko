### Added
- Client prediction and reconciliation helpers in `ponko::net`: `Prediction`
  applies the client's own input at once and keeps the inputs the server
  hasn't confirmed, numbered with a wrapping 16-bit sequence; `reconcile`
  compares the server's state for an input with the prediction and, only if
  they differ, replays the newer inputs on top of it. `InputQueue` is the
  server's side (each input once, in order, with the number to acknowledge),
  and `Smoothing` (with the `Smooth` trait, for `f32`, `Vec2`, `Vec3`, `Quat`
  and `Pose`) slides the drawn position over a correction instead of jumping.
  The wire format stays the game's. Example: `net_prediction` (150 ms round
  trip, packet loss); guide: "Your own player at once: client prediction".
- `Physics3D::move_character`: one tick of a `Character3D`'s walk from any
  feet position, the same as the physics step bit for bit, for predicting and
  replaying a walking character.
