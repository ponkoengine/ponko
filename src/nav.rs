//! Baking 3D navigation meshes (feature `navmesh`).
//!
#![cfg_attr(feature = "3d", doc = " The mesh, its agents and obstacles are in [`d3`](crate::d3)")]
#![cfg_attr(not(feature = "3d"), doc = " The mesh, its agents and obstacles are in `d3`")]
//! (`d3::NavMesh`, `d3::NavAgent`, `d3::NavObstacle`); this module holds
//! what baking one takes. Grid navigation in 2D is [`flow`](crate::flow);
//! the agent system is `systems::step_navmesh3d`.

#![deny(missing_docs)]

#[cfg(feature = "navmesh")]
pub use crate::nav3d::{NavBakeError, NavBakeSettings, NavCrowdStats, NavSource};
