### Changed

- Cache required column numbers in indexed ECS queries when matching
  layouts have expensive lookups. Use the cached numbers in typed,
  view, chunk, and parallel queries.
