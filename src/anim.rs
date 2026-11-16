//! Skeletal animation, clips, blending and IK (feature `3d`). Sprite-sheet
//! animation is [`sheet`](crate::sheet); 2D skeletons are
//! [`skeleton2d`](crate::skeleton2d).
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
