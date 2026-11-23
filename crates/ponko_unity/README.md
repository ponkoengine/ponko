# ponko_unity

Reads Unity projects saved as text (Force Text serialization) for importing
them into [Ponko](https://github.com/ponkoengine/ponko): scenes, prefabs,
materials and mesh assets, the GUIDs in `.meta` files, and mesh, texture and
animation data.

This crate is independent of the engine. It is not affiliated with or
endorsed by Unity Technologies.

- API reference: <https://docs.rs/ponko_unity/>
- Guide: <https://ponko.dev/guide/unity.html>

The Rust API follows Ponko's 0.x versioning; the files an import writes are
not a stable format, so re-import after upgrading.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
