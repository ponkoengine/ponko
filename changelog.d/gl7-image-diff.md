### Added
- CPU-only `ponko_image_diff` library shared by offscreen GPU tests, the `compare_images` MCP tool and `ponko image-diff`: RGBA tolerances, error statistics, global luminance SSIM, differing region boxes and optional PNG heatmaps. CLI thresholds support CI; MCP can compare against a live capture. Existing render-test tolerances and decisions are preserved.
