### Changed
- Global illumination: batches of distance levels on the CPU path keep
  their 4 bytes per instance for the probes' size cut only while global
  illumination is on: built the first frame it is on, dropped once it has
  been off for 120 frames. Off, it costs nothing per instance. The batch's
  bounding spheres they are made from are shared with the renderer, not
  copied.
- Global illumination: on the GPU culling path, the probes' face views
  leave out instances narrower than one probe pixel from every probe
  updated that frame, as the CPU path does per probe. On a field of
  160,000 small instances at 1920x1080 (16 probes a frame), the instances
  drawn in those views fell from 240,578 to none and the GI passes' GPU
  time from about 53 ms to about 2 ms (measured on a busy machine).
