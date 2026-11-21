### Added
- `RUSTFLAGS="--cfg ponko_web_thread_heaps"` gives a web build with
  threads one heap per thread (talc, through `orx-parallel-wasm-allocator`)
  instead of the standard allocator's one lock, and builds world saves on
  the workers. Experimental and not the default: in headless Chromium it
  made a JSON save on the workers 18 times faster (38 ms instead of 700
  ms) and a parallel query that allocates on every item 40 times faster,
  but allocation on the page's thread 10 to 50% slower (the save there
  56 ms instead of 41 ms). The ordinary web build and native builds ignore
  the flag. The guide's "Threads on the web" has the numbers.
- `web/smoke/threads.mjs --case thresholds` times the hierarchy's update,
  the 2D skeleton drawing and the 3D characters' moves at several sizes,
  on the page's thread and shared out, and checks each on both sides of
  its web threshold (the same result as on the page's thread alone);
  `--case allocator` compares the standard allocator with one heap per
  thread. Both are in the check list.

### Changed
- In a web build with threads, the small parallel passes are shared out
  to the workers only from sizes where that pays in a browser: a hierarchy
  level from 0.4 ms projected on the page's thread (about 14,000 attached
  children; 50 µs natively), 2D skeletons from 512 (64 natively) and 3D
  characters from 512 (16 natively). Below those sizes they were slower on
  the workers than on the page's thread. Native thresholds are unchanged.
