//! Heightfields, tiled terrain and streaming helpers.
//!
//! **Maturity: tested**, from the guide chapter `3d.md`. Game: Shard Grove, played headless by
//! `tests/shard_grove.rs`; its package is played headless in the list of checks. Tests:
//! `tests/three.rs`, `tests/lights.rs`, `tests/morph3d.rs` and the 3D render tests. Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics:
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found. Missing: Shard Grove's frame-time baseline per GPU
//! tier, and its web build and window played on a real GPU of each OS (#454).

#![deny(missing_docs)]
pub use crate::three::{
    FarTerrain, Heightfield, TerrainFile, TerrainMaterialDesc, TerrainTile, TerrainTileData, TextureArray, TextureKind,
};
