### Added
- Humanoid retargeting (`ponko::anim`): `HumanoidMap::detect` maps a
  skeleton's joints to `HumanBone`s from common naming schemes (editable
  with `set`), and `Retargeter` turns a clip made for one humanoid into an
  ordinary clip for another with other proportions, joint names and axes,
  so it plays in blend trees, layers and with root motion. Hips and root
  motion scale with leg length, and foot IK keeps planted feet planted at
  the target's ankle height (under 1 cm of slide in the test walk, about
  0.02 mm measured). `retarget_pose` follows a live pose every frame
  without allocating. Rust only for now; see the guide's "Retargeting
  humanoid animations" for what is and isn't handled.
