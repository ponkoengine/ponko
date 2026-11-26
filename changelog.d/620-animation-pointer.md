<!-- area: Rendering -->
### Added
- glTF `KHR_animation_pointer` animations play. A clip's pointer channels load
  as property tracks (`AnimationClip::property_tracks`, `PropertyAnimation`,
  `AnimatedProperty`, `MaterialProperty`) next to its joint tracks:
  - node translation, rotation and scale through a pointer;
  - node visibility (`KHR_node_visibility`), also on nodes hidden at rest;
  - material values: base color, emissive color and strength, alpha cutoff,
    metallic, roughness, normal scale, and the factors of the clearcoat,
    sheen, specular, IOR, anisotropy and iridescence layers;
  - `KHR_texture_transform` offset, rotation and scale of the material's
    texture transform.
  A pointer the engine doesn't play (a camera's, a morph weight, a texture
  other than the one the material's transform follows, an extension it
  doesn't read) is named in the load warnings, one each.
- `Skeleton::property`, `Skeleton::set_property` and
  `Skeleton::joint_visible`, and `Joint::visible` / `Joint::with_visible`: a
  hidden joint isn't drawn, with everything under it, and what hangs from it
  with `AttachToJoint` (a model, a light) is scaled to nothing.
- `ModelLight::joint`: the joint of the file's skeleton a light's node is.
  Attach the light there and it follows its node's animation and visibility.
- `MaterialDesc::texture_transform` (`TextureTransform`: offset, rotation,
  scale) and `MaterialDesc::alpha_cutoff`.

### Changed
- A glTF texture transform is a material value now, applied as the model
  is drawn, instead of being baked into the mesh's texture coordinates at
  load. Static files draw as before.
- Cutout materials cut at `MaterialDesc::alpha_cutoff` (glTF's
  `alphaCutoff`), instead of always at 0.5.
- Material descriptions are kept for every material, not only in editor
  builds.

### Breaking changes
- `ModelLight` has a new field, `joint`.
  A light from a file with skins or animations names the joint its node is.
  Instead: set `joint: None` where you build a `ModelLight` yourself.
  Before:
  ```rust
  let lamp = ModelLight {
      name: "lamp".into(),
      transform: ponko::d3::Transform::at(vec3(0.0, 2.0, 0.0)),
      kind: ModelLightKind::Point(ponko::d3::PointLight::default()),
  };
  ```
  After:
  ```rust
  # use ponko::render::{ModelLight, ModelLightKind};
  let lamp = ModelLight {
      name: "lamp".into(),
      transform: ponko::d3::Transform::at(vec3(0.0, 2.0, 0.0)),
      kind: ModelLightKind::Point(ponko::d3::PointLight::default()),
      joint: None,
  };
  ```
