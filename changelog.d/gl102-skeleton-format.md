### Added
- 2D skeletons: path constraints (`PathConstraint` on a `PathShape`
  attachment, with every position, spacing and rotate mode) place a chain
  of bones along a bezier path, and physics constraints
  (`PhysicsConstraint`) make a bone lag, sway and spring back, stepped at
  their own fixed rate with springs kept per `Skeleton`
  (`Skeleton::reset_physics` starts them over). Both are imported from
  Spine, mirrored to y-down.
- 2D skeletons: clips key constraint mixes and settings with `IkTrack`,
  `TransformTrack`, `PathTrack` and `PhysicsTrack`, read from Spine's IK,
  transform, path and physics timelines.
- `SkeletonData::to_json` writes a skeleton in the engine's own format.
  That format now also stores shear, transform, path and physics
  constraints (in their order), clipping masks, paths, deform keys,
  one-axis and shear bone keys and every constraint timeline, so an
  imported Spine export kept in it reads back bit for bit and plays the
  same.

### Changed
- 2D skeletons: deforms and constraint mixes blend across crossfades and
  1D blends, where deforms used to switch at the half-way point.
- Spine import evaluates path and physics constraints, path attachments
  and constraint timelines instead of leaving them out with a warning.
- `Skeleton::set_ik_mix` applies while no playing clip keys that
  constraint's mix.
