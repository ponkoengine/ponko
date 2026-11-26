### Added
- `NetMessages::steam_host_with(client, visibility, max_clients)`: the
  Steam Networking Messages host in a lobby of the given visibility, as
  `steam_sockets_host` takes it. A `SteamLobbyVisibility::Private` lobby is
  invite-only. `steam_host` stays friends-only.
- `SteamLobby::invite_user(SteamUserId)` invites one Steam user without the
  overlay, for a lobby screen with its own friend list; the friend joins as
  from an overlay invite (`steam_wait_for_invite`, `steam_lobby_from_args`).
  Private lobbies and single invites no longer need `unstable_steamworks`
  or `unsafe` code. Rust-only for now; C and C++ invite through the overlay.
- Unstable `From` conversions between `SteamLobbyId` and
  `unstable_steamworks::LobbyId`, and between `SteamUserId` and
  `unstable_steamworks::SteamId`, both ways, for calls through
  `SteamClient::unstable_steamworks()` (friends, matchmaking, rich
  presence) without `raw()`/`from_raw` round trips. They follow the
  `steamworks` version like the rest of the unstable Steam surface
  (STABILITY.md, "Unstable").

### Deprecated
- Paths that 0.2.0 moved keep building, with a warning that names the new
  path, until 0.3.0 removes them. An alias can't build or match a tuple
  struct (`ShadowMesh(None)`, `LayerId(0)`): write the new path there.
  Everything else (types, associated functions, struct literals, enum
  variants) goes through. The `ponko::physics3d` module is back for this:
  a game with a crate of that name and `use ponko::*;` writes
  `::physics3d::...` for the crate.
  - `ponko::physics3d`: `Character3D`, `Collider3D`, `ContactModifier3D`,
    `ContactPair3D`, `Hit3D`, `Joint3D`, `JointAxis3D`, `JointDrive3D`,
    `JointMotion3D`, `Physics3D`, `QueryFilter3D`, `Ragdoll3D`,
    `RigidBody3D`, `Shape3D`, `Vehicle3D`, `Velocity3D` and `Wheel3D` are
    `ponko::d3`'s types without the `3D` (`d3::Collider`, ...);
    `ColliderFocus`, `CollisionGroups`, `CollisionMesh`, `CollisionMeshData`,
    `FrictionCurve`, `RagdollBone` and `TerrainCollider` keep their names in
    `d3`; `PHYSICS3D_HZ` is `d3::Physics::HZ`; `step_physics3d`,
    `interpolate_physics3d` and their `_access` functions are in
    `ponko::systems`. Added by hand under the old path, the step systems
    are still the built-in ones (`Systems::PHYSICS3D`), listed under the
    same names as before.
  - `ponko::terrain::CellStreamer` is `ponko::streaming::CellStreamer`.
  - `ponko::d3`'s copies of render, animation, particle and terrain types:
    `AmbientOcclusion`, `AntiAliasing`, `BatchCulling`, `Blend`, `Compute`,
    `ComputePass`, `Effect`, `GlobalIllumination`, `GpuBuffer`,
    `GraphicsQuality`, `InstanceBatch`, `InstanceBatchOptions`,
    `InstanceLevel`, `Lighting`, `LodGroup`, `LodHysteresis`, `LodLevel`,
    `LodPick`, `LodRenderer`, `LodSettings`, `LodShadow`, `Material`,
    `MaterialDesc`, `Mesh`, `MeshCost`, `MeshData`, `ModelMaterial`,
    `ModelMaterials`, `Post`, `ReceivesDecals`, `RenderPasses`,
    `SceneSettings`, `ScreenEffect`, `Shader`, `ShaderDiagnostic`,
    `ShaderKind`, `ShaderOutcome`, `ShaderStatus`, `ShadowFilter`,
    `ShadowMesh`, `Sky`, `StreamedLod`, `TargetCamera`, `TargetEffect` and
    `Tonemap` are in `ponko::render`; `AnimationClip`, `AnimationCondition`,
    `AnimationEvent`, `AnimationGraph`, `AnimationGraphError`,
    `AnimationLayer`, `AnimationParameter`, `AnimationState`,
    `AnimationTrack`, `AnimationTransition`, `Animator`, `AttachToJoint`,
    `Interpolation`, `JointAnimation`, `JointMask`, `LayerBlend`, `LayerId`,
    `MorphAnimation`, `MorphFrame`, `MorphTarget`, `MorphWeights`, `Motion`,
    `MotionChild`, `ParamId`, `RootMotion`, `Skeleton`, `SkinOf` and
    `StateId` in `ponko::anim`; `ParticleCurve`, `ParticleGradient` and
    `ParticleSpace` in `ponko::particles`; `FarTerrain`, `Heightfield`,
    `TerrainMaterialDesc`, `TerrainTile`, `TerrainTileData`, `TextureArray`
    and `TextureKind` in `ponko::terrain`. `d3::Model` and `d3`'s collision
    types have no alias: the prelude brings them, and an alias would make
    `use ponko::prelude::*; use ponko::d3::*;` ambiguous. For the same
    reason, a module that glob-imports `d3` together with one of these
    topic modules imports these names by name until 0.3.0.
- To be removed in 0.3.0 (with a breaking entry each): the `ponko::physics3d`
  module, `ponko::terrain::CellStreamer`, and the 83 type aliases in
  `ponko::d3` listed above, together with 0.2.0's other deprecated items
  (`ponko::stream`, the prelude's `Transform2D`, `Camera2D`, `Transform3D`
  and `Camera3D`, ...).
