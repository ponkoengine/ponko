### Added

- First-person held items (`d3::ViewModel`, `d3::HeldItem`) write motion
  vectors of their own under temporal antialiasing: an item held still stays
  put on screen while the eye turns and walks, one that sways or recoils
  moves as far as it does, instead of smearing with the world behind it.
- Cut-out held items (`Blend::Cutout`) are drawn by the view model, their
  holes cut, with and without multisampling (a pipeline built the first time
  one is drawn).
- The cookbook's "Follow the player" recipe puts `CameraTarget` on the
  player and keeps the rig as a resource: no camera system.

### Fixed

- A wall nearer than a hundredth of the near plane's distance beyond it no
  longer cuts into held items: the view model pushes it behind them first.
