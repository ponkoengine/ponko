### Fixed
- Shard Grove: the warden and the crawlers no longer walk backwards. Their
  glTF models look along +Z, and the game turned -Z towards the way they
  moved; `facing_of` now turns the model's forward (`MODEL_FORWARD`) onto
  the direction, and the warden starts looking the way the view looks.
