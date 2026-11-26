### Fixed
- On Windows, `load_skeleton2d` with a file path that uses backslashes
  (`C:\game\hero.json`) finds the atlas pages and images next to the file
  instead of looking for them in `assets/`.
