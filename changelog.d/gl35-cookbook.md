### Added
- A cookbook in the guide: fifty small recipes (moving, fighting and collecting,
  timers, menus, cameras, 2D and 3D physics, levels, sound and saves), each in
  Rust and, for 48 of them, in C++ (slow motion and zoom need C++ calls that do
  not exist yet). Every recipe is compiled and run headless by a test
  (`tests/cookbook_*.rs`, `examples/cpp/cookbook.cpp`), and the pages include
  the tested code, so none of it can drift. An index by task is in the guide
  and in the README.
