### Changed
- Error messages on the common mistakes name the mistake, where it is and the
  fix; the error types, their variants and `ErrorKind`s are unchanged (match
  on `kind()`, not on the text), and the guide's "When something fails"
  chapter lists them:
  - A file given to the wrong loader (`load_texture`, `load_font`,
    `Audio::load`, `load_stream`) says what its first bytes are and which
    call loads it; an empty file and a Git LFS pointer say so and how to
    get the real file. The sound loaders name the formats they read.
  - A missing asset name that starts with `assets/` or uses backslashes gets
    a hint with the corrected name.
  - `Ctx::load_world` names the slot and its file. A slot nobody saved lists
    the slots there are; a damaged, empty or non-save file says so and points
    at `Storage::read_backup` and `Storage::delete_world`. A stored file that
    isn't there names the folder it looked in, and `Storage::read_json` names
    the type the file didn't fit.
  - A scene, save or prefab that names a component the game didn't register
    gives the call to register it (`world.reflect::<Name>()`), the first entity
    that names it and the closest registered names. A value that doesn't fit
    names the entity too. A prefab given to the scene loader (or the other way
    round) says what the file is and which call reads it, and a file from a
    newer ponko says to update. The `ReflectError` texts for an unknown
    component or resource, and for a bad save as a whole, are reworded.
  - GPU startup failures (`no compatible GPU found`, `could not open the GPU
    device`, `could not create a GPU surface`, a surface with no format) say
    what to try: the driver, `WGPU_BACKEND`, `Game::run_headless`.
  - `Game::order`'s refusal says which system to move and how. The code-mod
    message names the `mods-wasm` feature to turn on.

### Added
- The guide's "When something fails" chapter has a list of the common
  mistakes with their messages: wrong asset, wrong kind of file, a feature the
  build leaves out, a save that won't load, an unregistered component, a scene
  of the wrong kind, no GPU, and systems the schedule refuses.
