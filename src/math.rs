//! Math types: vectors, quaternions and matrices, with their constructors.
//!
//! These are the [`glam`](https://docs.rs/glam) crate's own types, re-exported
//! as they are: `ponko::math::Vec2` *is* `glam::Vec2`, so values pass between
//! Ponko and other glam code without conversion, and their methods
//! (`length`, `normalize`, `lerp`, `Quat::from_rotation_y`, ...) are glam's
//! and documented there. The prelude re-exports the common ones
//! (`Vec2`, `Vec3`, `Vec4`, `Quat`, `UVec2`, `vec2`, `vec3`, `vec4`, `uvec2`,
//! and `Mat4` with the `3d` feature) as the same items; import this module
//! for the rest (`EulerRot` for [`Quat::from_euler`]).
//!
//! Which glam version is behind them is part of Ponko's compatibility
//! promise (STABILITY.md, "Re-exported crates"): before 1.0 a Ponko minor
//! release may move to an incompatible glam (listed under Breaking changes);
//! from 1.0 on only a Ponko major release does. A patch or minor release of
//! Ponko may take a compatible glam release, which can add methods.
//!
//! ```
//! use ponko::math::{EulerRot, Quat, Vec3, vec3};
//!
//! let turn = Quat::from_euler(EulerRot::YXZ, std::f32::consts::FRAC_PI_2, 0.0, 0.0);
//! let ahead: Vec3 = turn * vec3(0.0, 0.0, -1.0);
//! assert!((ahead - vec3(-1.0, 0.0, 0.0)).length() < 1e-6);
//! // The prelude's names are the same items.
//! let same: ponko::prelude::Vec3 = ahead;
//! assert_eq!(same, ahead);
//! ```

pub use glam::{EulerRot, IVec2, Mat4, Quat, UVec2, Vec2, Vec3, Vec4, uvec2, vec2, vec3, vec4};
