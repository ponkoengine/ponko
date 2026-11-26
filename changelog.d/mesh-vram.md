### Changed
- The shared mesh arena uses less GPU memory. Its buffers grow to a quarter
  more than they need instead of to the next power of two, which could leave
  up to half of each buffer empty; freeing the meshes at the end of the arena
  lowers its high-water mark, and a buffer left far larger than its contents
  (more than about 1.56 times) shrinks again with a GPU copy. In a streaming
  test of 768 live meshes (412 MiB of geometry) the arena's GPU memory fell
  from 542.6 MiB to 423.2 MiB. Drawing is unchanged.
