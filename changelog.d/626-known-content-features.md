### Added

- Place a glTF file's lights with an EV100 exposure from C and C++: `ponko_ctx_light_gltf` (`ctx::light_gltf`) sets up a glTF preview from a file, and `ponko_ctx_place_light_exposed` (`ctx::place_light_exposed`) places one `PonkoModelLight` the same way.
- Light a scene with an HDR or EXR panorama, six cube faces or the neutral studio from C and C++: `ponko_ctx_load_environment`, `ponko_ctx_load_environment_cube`, `ponko_ctx_set_studio_environment`, `ponko_ctx_clear_environment` and `ponko_ctx_has_environment` (`ctx::load_environment` and friends).

### Fixed

- The well-known content load test no longer fails in a workspace test run: a sample that needs a glTF extension from a cargo feature the build lacks is expected to skip naming that extension.
