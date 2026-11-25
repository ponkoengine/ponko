//! Skeletal animation, clips, blending and IK (feature `3d`). Sprite-sheet
//! animation is [`sheet`](crate::sheet); 2D skeletons are
//! [`skeleton2d`](crate::skeleton2d).
//!
//! **Maturity: tested**, from the guide chapter `3d.md`. Game: Shard Grove, played headless by
//! `tests/shard_grove.rs`; its package is not played in the list of checks. Tests:
//! `tests/three.rs`, `tests/lights.rs`, `tests/morph3d.rs` and the 3D render tests. Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics:
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found. Missing: Shard Grove's package played on each OS.
//!
//! The state machine's indices, parameters, conditions, transitions and
//! events are here in every build: the 3D `AnimationGraph` and the 2D
//! [`SkeletonGraph`](crate::skeleton2d::SkeletonGraph) share them.

#![deny(missing_docs)]
pub(crate) mod graph;
#[cfg(feature = "3d")]
pub use crate::three::{
    AnimationClip, AnimationGraph, AnimationLayer, AnimationState, AnimationTrack, Animator, AttachToJoint, HumanBone,
    HumanoidMap, Interpolation, Joint, JointAnimation, JointMask, LayerBlend, LayerId, MorphAnimation, MorphFrame,
    MorphTarget, MorphWeights, Motion, MotionChild, RetargetError, Retargeter, RootMotion, Skeleton, SkinOf,
    look_at_joint, two_bone_ik,
};
pub use graph::{
    AnimationCondition, AnimationEvent, AnimationGraphError, AnimationParameter, AnimationTransition, ParamId, StateId,
};
