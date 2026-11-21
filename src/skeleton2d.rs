//! 2D skeletal (cutout) animation: a hierarchy of bones that moves sprite
//! parts and bends 2D meshes, animated by clips, crossfades, 1D blends and
//! a state machine, with two-bone IK and look-at.
//!
//! - [`SkeletonData`]: the shared description: [`Bone`]s with their rest
//!   [`BonePose`], [`Slot`]s that show one [`Attachment`] at a time (a rigid
//!   [`SpritePart`] or a [`SkinnedMesh`] whose vertices follow up to four
//!   bones), skins, [`Clip`]s, [`IkConstraint`]s and an optional
//!   [`SkeletonGraph`]. Build it in code, or load a file with
//!   [`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d): Ponko's own
//!   JSON format (the guide's "2D skeletons" chapter) or a Spine JSON
//!   export ([`SkeletonData::from_spine_json`] says what is read).
//! - [`Skeleton`]: the component on an entity with a
//!   [`Transform`](crate::d2::Transform): what plays, its clock and pose.
//!   The engine advances it at the end of
//!   [`Phase::Animate`](crate::game::Phase::Animate), and the built-in
//!   sprite drawing draws it in the same batches and `z` order as sprites
//!   (a [`BlendMode`](crate::render::BlendMode) or
//!   [`SpriteMaterial`](crate::render::SpriteMaterial) on the entity applies
//!   to all its parts).
//! - Clips key bone translation, rotation, scale and shear (relative to the
//!   rest pose, both axes at once or one at a time), mesh vertex offsets
//!   (deforms), slot attachments (sprite swaps), visibility and color, draw
//!   order, constraint mixes and settings, and events. [`IkConstraint`]s,
//!   [`TransformConstraint`]s, [`PathConstraint`]s (bones along a
//!   [`PathShape`]) and [`PhysicsConstraint`]s (springs that lag and sway)
//!   adjust the pose, and a [`ClippingMask`] hides what lies outside it.
//!   Crossfades and 1D blends mix bone poses, deforms and constraint values
//!   alike. The state machine reuses [`anim`](crate::anim)'s parameters,
//!   conditions and transitions.
//! - [`SkeletonData::to_json`] writes a skeleton (an imported Spine export,
//!   say) in Ponko's own format, which holds everything it can hold.
//!
//! A game with no skeleton pays nothing: no system work beyond an empty
//! query, no draw calls, no GPU memory. Saves, scenes, prefabs and the
//! inspector keep a skeleton as `"Skeleton2D"`: the file it plays, the clip
//! or state, the clock, the parameters and the skin.
//!
//! ```
//! use ponko::prelude::*;
//! use ponko::skeleton2d::{BonePose, BoneTrack, Clip, Key, Skeleton, SkeletonData, SpritePart};
//! use std::sync::Arc;
//!
//! Game::new("puppet", 320, 240).run_headless(0, |ctx| {
//!     let pixel = ctx.gfx.create_texture(1, 1, &[255; 4]);
//!     let mut data = SkeletonData::new();
//!     let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
//!     let arm = data.add_bone("arm", Some(root), BonePose::at(vec2(8.0, 0.0)), 24.0);
//!     let slot = data.add_slot("arm", arm);
//!     let part = SpritePart::new(pixel).with_size(vec2(24.0, 6.0)).with_pivot(vec2(0.0, 0.5));
//!     data.add_attachment(slot, "arm", part);
//!     let mut wave = BoneTrack::new(arm);
//!     wave.rotate = vec![Key::new(0.0, -0.5), Key::new(0.5, 0.5), Key::new(1.0, -0.5)];
//!     data.add_clip(Clip::new("wave", 1.0).with_bone_track(wave));
//!
//!     let mut puppet = Skeleton::new(Arc::new(data));
//!     puppet.play("wave");
//!     ctx.world.spawn((ponko::d2::Transform::at(vec2(160.0, 120.0)), puppet));
//! });
//! ```
//!
//! C and C++ games load, attach and drive skeletons through
//! `ponko_ctx_load_skeleton2d` and the `ponko_skeleton2d_*` functions
//! (`ponko::Skeleton2DData` and `Ctx` methods in `ponko.hpp`).

#![deny(missing_docs)]

mod atlas;
mod clip;
mod data;
pub(crate) mod draw;
mod graph;
mod json;
mod library;
mod path;
mod physics;
mod skeleton;
mod spine;

pub use atlas::SpineAtlas;
pub use clip::{
    Bezier, BoneChannel, BoneTrack, ChannelTrack, Clip, Curve, DeformTrack, IkTrack, Key, PathTrack, PhysicsTrack,
    SlotTrack, TransformTrack,
};
pub use data::{
    Attachment, Bone, BonePose, ClippingMask, IkConstraint, MeshVertex, SkeletonData, SkeletonError, SkinnedMesh, Slot,
    SpritePart, TransformConstraint,
};
pub(crate) use draw::draw_skeletons;
pub use graph::{SkeletonGraph, SkeletonMotion, SkeletonState};
pub(crate) use library::{SkeletonLibrary, library};
pub use path::{PathConstraint, PathPosition, PathRotate, PathShape, PathSpacing};
pub use physics::PhysicsConstraint;
pub use skeleton::Skeleton;
