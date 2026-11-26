# Well-known content

> **Maturity: not a feature.** How the 3D loader and renderer are checked
> against content other people made.

The offscreen GPU tests ([GPU regression tests](render-tests.md)) draw small
scenes built for them. This chapter checks the loader and the renderer
against content made elsewhere and already known to look right: the
[Khronos glTF Sample Assets](https://github.com/KhronosGroup/glTF-Sample-Assets),
each with a screenshot from a reference viewer, and two public scenes,
Sponza and the Amazon Lumberyard Bistro. Every sample either loads, fails
with an error naming the file, or names the extension it needs; each one
that loads is drawn offscreen and compared with its screenshot; the two
scenes are measured.

Nothing downloaded is committed: the content lives in a git-ignored cache.

## Fetching the content

```sh
python3 scripts/fetch-known-content.py              # samples and Bistro, about 2.8 GB
python3 scripts/fetch-known-content.py --set samples  # only the samples, about 630 MB
python3 scripts/fetch-known-content.py --model Duck   # one sample
python3 scripts/fetch-known-content.py --list         # what is in the cache
```

The tool is plain Python (no packages). It fetches fixed revisions, so a
run today and a run next year get the same files:

| Set | Repository | Revision | What |
|---|---|---|---|
| samples | `KhronosGroup/glTF-Sample-Assets` | `e124e8d8` | every model in its index (153), one variant each (`glTF-Binary`, else `glTF`, else `glTF-Embedded`), with its screenshot and metadata; Sponza is one of them |
| bistro | `zeux/niagara_bistro` | `2bdb6a41` | the Bistro exterior as glTF with BC7 textures in DDS files (`MSFT_texture_dds`) |

The cache is the folder `PONKO_KNOWN_CONTENT` names, else
`.cache/known-content` in the repository. Files already there are kept, so
a second run only fetches what is missing. The tool writes `manifest.json`
(each asset's file, variant, screenshot and licenses) and `LICENSES.md`.

### Licenses

Each asset's license comes from its own `metadata.json`, and the license
texts are fetched into `licenses/`. `LICENSES.md` in the cache lists every
asset with its license and copyright holder. At this revision most samples
are CC0-1.0 (95 entries) or CC-BY-4.0 (73), several carry the Khronos or
another trademark notice, and a few allow less: Sponza is under the
CRYENGINE license agreement, and single samples are CC-BY-NC-4.0,
CC-BY-NC-SA-4.0, a Poser EULA, the Adobe Stock license, a testing-only
license or the SCEA license. The Bistro scene is CC-BY-4.0 (Amazon
Lumberyard) and its glTF conversion MIT (NVIDIA). These files are used
here to test the engine and nothing else, and none of them is shipped.

## The load sweep

```sh
cargo test -p ponko --features 3d,compressed-textures,gltf-meshopt,image-webp --test known_content -- --nocapture
```

Each sample is loaded through `ctx.gfx.try_load_mesh` in a headless game
of its own (`tests/known_content.rs`). The outcome is one of:

- **loaded**: the file loads (warnings for the optional extensions it
  ignores are fine);
- **skipped**: it requires an extension the engine doesn't read, and the
  error names it;
- **failed**: any other error, which must name the file, or a panic.

`tests/fixtures/known-content/outcomes.txt` records each sample's outcome
for the pinned revision, and the test fails when one changes. A fix that
makes another sample load is recorded with
`PONKO_UPDATE_KNOWN_CONTENT=1` (and the table below updated); a sample that
stops loading is a regression. Without the cache the test says so and
passes: nothing is downloaded by a test.

At this revision **all 153 samples load, none are skipped, none fail**.
ScatterAnemone and PotOfCoalsAnimationPointer require `KHR_materials_volume`,
which is approximated (transmission without refraction), with a warning.

## Drawn against the screenshots

```sh
cargo test -p ponko --lib --features 3d,compressed-textures,gltf-meshopt,image-webp,image-gif \
  known_samples_are_drawn -- --nocapture
PONKO_KNOWN_ONLY=Duck,Fox cargo test ...   # only these samples
```

`src/render/known_content_render_test.rs` draws each sample that loads,
offscreen, at the screenshot's aspect (256 pixels on the long side), and
compares the drawing with the screenshot. A screenshot can't be matched
pixel for pixel: the reference viewer has its own lights, environment, tone
curve and often its own camera. So:

- **Camera.** The file's first camera where it has one. Otherwise the
  framing rule, after the glTF Sample Viewer's default view: a 45° vertical
  field of view, the camera on the +Z side of the center of the model's
  bounds, looking down -Z, as far away as the bounding sphere needs to fit
  the narrower field of view.
- **Lights.** The twelve light, emission and environment samples in
  [#597](https://github.com/ponkoengine/ponko/issues/597) use
  `SceneSettings::light_gltf`: the file's lights with EV100 exposure, a neutral
  studio environment and a hue-preserving highlight curve. The harness
  pins EV100 and ambient strength for the four punctual-light shots, and
  lowers picture exposure for EmissiveStrengthTest so its five strengths
  are distinguishable. Other samples retain the comparison rig of a sun
  from over the camera's left shoulder at intensity 2 and a flat grey sky
  (0.6 above, 0.3 below), with file lights where available.
- **Background.** The screenshot's border color, so the outlines compare.
- **Skins** stand in their rest pose with their skeleton, as a game spawns
  them; nothing is animated here (the pointer animations are drawn by a test
  of their own, [below](#animations-through-pointers)).

The comparison finds each picture's foreground (pixels away from its own
border color), crops both to it and scales them to the same size, so a
model framed a little closer or further still compares. Then:

- **overlap**: the intersection over union of the two silhouettes, 1 when
  they coincide;
- **ΔE**: the mean CIE76 color difference over 8x8 blocks where either
  picture has its foreground (about 2 is just visible, 10 a clear shift,
  30 another color).

A drawing **matches** its screenshot when the overlap is at least 0.7 and ΔE
at most 20. The test's own checks (an empty frame, a shape elsewhere, a
recolored drawing) pin what the measure tells apart.

Each run writes the drawing next to its screenshot in
`results/draw/<Name>.png` in the cache, contact sheets of sixteen pairs
(`results/sheet-NN.png`) and `results/draw.json`.
`tests/fixtures/known-content/draws.txt` records which samples match: one
that matched and stops matching fails the test; a sample that starts
matching is recorded with `PONKO_UPDATE_KNOWN_CONTENT=1`.

At this revision **42 of the 153 drawings match** their screenshots. Most
of the others differ for reasons outside the renderer's correctness, which
the table says in a word:

- **view**: the screenshot was taken from another side or angle than the
  framing rule (the file holds no camera); **crop**: the screenshot is a
  close-up; **background**: the screenshot has a photograph or backdrop
  behind the model, so the outlines don't line up;
- **anim**: the screenshot shows a frame of the animation or a posed
  skeleton, the drawing the rest pose;
- **light**: lit brighter or darker than the reference viewer's lights;
  the shapes and materials are right.

The rest are missing features, each with its issue:
[#595](https://github.com/ponkoengine/ponko/issues/595) transmissive
materials vanish, and a material has one texture transform for all its
maps ([#666](https://github.com/ponkoengine/ponko/issues/666)). Points and
lines aren't drawn either (MeshPrimitiveModes, PrimitiveModeNormalsTest).

Fixed while making this list: triangle strips and fans are drawn (they were
left out), `EXT_mesh_gpu_instancing` is read (SimpleInstancing drew one cube
of 125), `KHR_animation_pointer` channels no longer fail the whole file
(AnimatedColorsCube), and the retired specular-glossiness materials load,
approximated (SpecGlossVsMetalRough, and the Bistro scene).

### The table

Overlap and ΔE are after registration; a dash: not drawn.

| Asset | Loaded | Picture | Overlap | ΔE | Difference |
|---|---|---|---:|---:|---|
| ABeautifulGame | yes | differs | 0.12 | 18.7 | view: the screenshot looks across the board from a low corner; the framing rule sees it edge-on |
| AlphaBlendModeTest | yes | matches | 0.93 | 8.2 |  |
| AnimatedColorsCube | yes | differs | 0.91 | 24.0 | anim: drawn at rest (the cube red, at the left); its colors play ([pointer animations](#animations-through-pointers)); the screenshot is an animated GIF of the cube moving over the others |
| AnimatedCube | yes | matches | 0.79 | 18.6 |  |
| AnimatedMorphCube | yes | differs | 0.90 | 49.9 | light: lit from the front, the screenshot shows its shaded side mid-morph |
| AnimatedTriangle | yes | differs | 0.33 | 50.5 | anim: the screenshot shows it turned mid-animation; drawn at rest |
| AnimationPointerUVs | yes | differs | 0.41 | 31.6 | the file's camera; drawn at rest; the base color and other first-texture transforms play, the other maps' don't: a material has one transform ([#666](https://github.com/ponkoengine/ponko/issues/666)) |
| AnisotropyBarnLamp | yes | differs | 0.47 | 19.5 | view; the brushed copper reflects the comparison rig's flat studio sky |
| AnisotropyDiscTest | yes | matches | 0.81 | 12.6 |  |
| AnisotropyRotationTest | yes | differs | 0.64 | 26.8 | environment: the black plastic reflects a flat studio sky of the comparison rig, so the stretched highlights stand out less |
| AnisotropyStrengthTest | yes | differs | 0.36 | 21.1 | environment: the spheres reflect a flat studio sky of the comparison rig, not a dark room; the labels are smaller |
| AntiqueCamera | yes | differs | 0.44 | 9.8 | view: the screenshot is from three quarters, the rule from the front |
| AttenuationTest | yes | differs | 0.06 | 19.3 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| Avocado | yes | differs | 0.80 | 31.3 | light: brighter, the sun lights the side the camera sees |
| BarramundiFish | yes | differs | 0.45 | 17.9 | view: drawn head-on, the screenshot is from the side |
| BoomBox | yes | matches | 0.77 | 12.9 |  |
| BoomBoxWithAxes | yes | matches | 0.75 | 11.4 |  |
| Box | yes | differs | 0.75 | 43.5 | view: the screenshot looks at a corner from above, the rule at one face |
| Box With Spaces | yes | matches | 0.74 | 19.5 |  |
| BoxAnimated | yes | differs | 0.82 | 46.2 | anim: the screenshot shows a moved frame; drawn at rest |
| BoxInterleaved | yes | differs | 0.75 | 23.3 | view: as Box |
| BoxTextured | yes | differs | 0.66 | 30.6 | view: as Box |
| BoxTexturedNonPowerOfTwo | yes | differs | 0.66 | 30.6 | view: as Box |
| BoxVertexColors | yes | differs | 0.75 | 46.0 | view: as Box |
| BrainStem | yes | differs | 0.30 | 12.4 | anim: drawn in its rest pose, the screenshot shows a pose of its animation |
| Cameras | yes | differs | 0.97 | 34.1 | light: the file's camera frames it as the screenshot, but it is lit fully where the screenshot shades it |
| CarbonFibre | yes | matches | 0.78 | 17.5 |  |
| CarConcept | yes | differs | 0.51 | 24.1 | view: the screenshot shows a three-quarter angle, with other reflections |
| CesiumMan | yes | differs | 0.42 | 15.2 | anim: rest pose against a walking pose |
| CesiumMilkTruck | yes | differs | 0.71 | 26.6 | view: the screenshot is from three quarters |
| ChairDamaskPurplegold | yes | differs | 0.74 | 21.1 | view: the screenshot is closer; the fabric's sheen is drawn |
| ChronographWatch | yes | differs | 0.70 | 23.4 | crop: the screenshot is a close-up of the dial |
| ClearCoatCarPaint | yes | differs | 0.83 | 27.8 | environment: the coat reflects a flat studio sky of the comparison rig, not the screenshot's street |
| ClearCoatTest | yes | differs | 0.52 | 18.0 | environment: the comparison rig's flat studio sky; the labels are smaller |
| ClearcoatWicker | yes | matches | 0.94 | 13.9 |  |
| CommercialRefrigerator | yes | differs | 0.70 | 18.3 | view: the screenshot is from three quarters |
| CompareAlphaCoverage | yes | matches | 0.85 | 13.9 |  |
| CompareAmbientOcclusion | yes | matches | 0.79 | 15.6 |  |
| CompareAnisotropy | yes | differs | 0.90 | 21.3 | environment: the comparison rig's flat studio sky; view |
| CompareBaseColor | yes | matches | 0.96 | 13.5 |  |
| CompareClearcoat | yes | matches | 0.93 | 13.9 |  |
| CompareDispersion | yes | differs | 0.51 | 18.9 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| CompareEmissiveStrength | yes | differs | 0.60 | 14.7 | both logos keep their color and the stronger one glows more; crop and reflected surroundings differ |
| CompareIor | yes | differs | 0.31 | 12.6 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| CompareIridescence | yes | matches | 0.92 | 14.7 |  |
| CompareMetallic | yes | matches | 0.95 | 16.2 |  |
| CompareNormal | yes | matches | 0.98 | 15.0 |  |
| CompareRoughness | yes | matches | 0.96 | 13.0 |  |
| CompareSheen | yes | matches | 0.96 | 19.9 | |
| CompareSpecular | yes | matches | 0.95 | 14.5 |  |
| CompareTransmission | yes | differs | 0.44 | 11.1 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| CompareVolume | yes | differs | 0.48 | 13.5 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| Corset | yes | matches | 0.84 | 17.5 |  |
| Cube | yes | matches | 0.79 | 18.2 |  |
| CubeVisibility | yes | differs | 0.15 | 69.5 | view; drawn at rest; the blue cube blinks ([pointer animations](#animations-through-pointers)) |
| DamagedHelmet | yes | differs | 0.43 | 18.1 | view: the screenshot is from three quarters |
| DiffuseTransmissionPlant | yes | differs | 0.41 | 27.4 | the file's camera isn't the screenshot's; diffuse transmission isn't drawn ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| DiffuseTransmissionTeacup | yes | differs | 0.33 | 14.1 | view; diffuse transmission isn't drawn ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| DiffuseTransmissionTest | yes | differs | 0.61 | 24.0 | view: the framing rule sees the plane edge-on; diffuse transmission isn't drawn (`KHR_materials_diffuse_transmission`) |
| DirectionalLight | yes | differs | 0.15 | 35.1 | the spheres pick up the yellow sun; the screenshot’s toolbar enters the silhouette comparison and its specular highlights are broader |
| DispersionTest | yes | differs | 0.23 | 14.8 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| DragonAttenuation | yes | differs | 0.30 | 26.0 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| DragonDispersion | yes | differs | 0.30 | 11.6 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| Duck | yes | differs | 0.75 | 33.6 | light: more saturated and brighter (file camera) |
| EmissiveStrengthTest | yes | differs | 0.12 | 17.6 | the panels stay blue as strength rises; the screenshot is angled down and has brighter bloom and a brighter backdrop |
| EnvironmentTest | yes | differs | 0.13 | 13.3 | the first row reflects the studio panels across roughnesses; the reference has a different environment and a gradient background |
| FlightHelmet | yes | differs | 0.36 | 19.6 | crop: the screenshot is a close-up |
| Fox | yes | differs | 0.21 | 36.7 | view: drawn from the front, the screenshot is from the side |
| GlamVelvetSofa | yes | matches | 0.94 | 9.7 |  |
| GlassBrokenWindow | yes | differs | 0.28 | 19.5 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| GlassHurricaneCandleHolder | yes | differs | 0.39 | 18.5 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| GlassVaseFlowers | yes | differs | 0.16 | 18.7 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| InterpolationTest | yes | matches | 0.93 | 9.1 |  |
| IORTestGrid | yes | differs | 0.80 | 20.9 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): the grid is dark |
| IridescenceAbalone | yes | matches | 0.97 | 15.1 |  |
| IridescenceDielectricSpheres | yes | differs | 0.15 | 7.3 | view; the films are faint on the dielectric spheres in both |
| IridescenceLamp | yes | differs | 0.71 | 20.8 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| IridescenceMetallicSpheres | yes | differs | 0.36 | 12.2 | view |
| IridescenceSuzanne | yes | differs | 0.09 | 14.1 | view; transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| IridescentDishWithOlives | yes | differs | 0.51 | 37.1 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): the jug is missing (file camera) |
| Lantern | yes | differs | 0.55 | 11.2 | view: the screenshot is from three quarters |
| LightsPunctualLamp | yes | differs | 0.35 | 28.6 | the bulb lights up; the transmitted shade is missing ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| LightVisibility | yes | differs | 0.73 | 24.6 | view: the drawing faces the wall; drawn at rest, with the blue light on; it blinks ([pointer animations](#animations-through-pointers)) |
| MandarinOrange | yes | matches | 0.91 | 15.6 |  |
| MaterialsVariantsShoe | yes | differs | 0.56 | 26.0 | view: the screenshot is from the side, above |
| MeshoptCubeTest | yes | differs | 0.11 | 20.7 | view: the cubes are drawn, the screenshot looks from above |
| MeshPrimitiveModes | yes | differs | 0.16 | 46.3 | points and lines aren't drawn (strips and fans are) |
| MetalRoughSpheres | yes | matches | 0.72 | 15.7 |  |
| MetalRoughSpheresNoTextures | yes | matches | 0.81 | 6.9 |  |
| MorphPrimitivesTest | yes | differs | 0.14 | 22.6 | view: both primitives and the morph are right, the screenshot is from an angle |
| MorphStressTest | yes | differs | 0.29 | 30.1 | anim: the screenshot shows other morph weights |
| MosquitoInAmber | yes | differs | 0.35 | 23.9 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): the amber is missing |
| MultipleScenes | yes | matches | 0.75 | 17.8 |  |
| MultiUVTest | yes | matches | 0.74 | 18.7 |  |
| NegativeScaleTest | yes | differs | 0.91 | 22.7 | light: lighter; the mirrored shapes are right |
| NodePerformanceTest | yes | differs | 0.28 | 21.3 | light: brighter (file camera) |
| NodeVisibilityTest | yes | matches | 0.72 | 15.0 | |
| NormalTangentMirrorTest | yes | matches | 0.77 | 11.6 |  |
| NormalTangentTest | yes | matches | 0.91 | 8.6 |  |
| OrientationTest | yes | differs | 0.72 | 31.4 | view: the screenshot is from an angle |
| PlaysetLightTest | yes | matches | 0.80 | 18.6 | the 512 lux sun and 1500 candela point light use EV100 11 (file camera) |
| PointLightIntensityTest | yes | differs | 0.56 | 15.2 | the red, green, blue and neutral pools of light are visible; the reference is a closer crop with unlit labels |
| PotOfCoals | yes | differs | 0.79 | 36.1 | view: the screenshot looks down into the pot and shows hotter coals and different studio reflections |
| PotOfCoalsAnimationPointer | yes | differs | 0.76 | 28.5 | view; transmission ([#595](https://github.com/ponkoengine/ponko/issues/595)): the heat dome whose normal map turns isn't seen ([pointer animations](#animations-through-pointers)) |
| PrimitiveModeNormalsTest | yes | differs | 0.38 | 14.8 | points and lines aren't drawn; view |
| RecursiveSkeletons | yes | differs | 0.13 | 22.0 | anim: rest pose against a pose of its animation; framed differently |
| RiggedFigure | yes | differs | 0.50 | 10.1 | anim: rest pose against a pose of its animation |
| RiggedSimple | yes | differs | 0.85 | 25.3 | anim: drawn straight, the screenshot bent |
| ScatterAnemone | yes | differs | 0.37 | 33.9 | transmission, volume and scattering aren't drawn ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| ScatteringSkull | yes | differs | 0.00 | 26.8 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): nothing is drawn |
| ScatterPlant | yes | differs | 0.28 | 25.2 | the file's camera is far from the screenshot's: drawn small |
| ScatterTest | yes | differs | 0.39 | 24.5 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| SciFiHelmet | yes | differs | 0.46 | 19.9 | background: the screenshot has a photograph behind it, so the outlines don't line up; view |
| SheenChair | yes | differs | 0.51 | 26.0 | view |
| SheenCloth | yes | differs | 0.56 | 20.5 | its occlusion map has a texture transform of its own, which follows the base color's; darker in the screenshot |
| SheenTestGrid | yes | matches | 0.72 | 18.4 | |
| SheenWoodLeatherSofa | yes | differs | 0.60 | 11.4 | crop |
| SimpleInstancing | yes | differs | 0.71 | 22.6 | view: all 125 instances are drawn (since this change), the screenshot is from an angle |
| SimpleMaterial | yes | differs | 0.44 | 20.5 | view |
| SimpleMeshes | yes | differs | 0.93 | 24.4 | light: lighter; both triangles are right |
| SimpleMorph | yes | differs | 0.41 | 48.0 | anim: the screenshot shows other morph weights |
| SimpleSkin | yes | differs | 0.47 | 17.0 | anim: rest pose against a bent frame |
| SimpleSparseAccessor | yes | matches | 0.95 | 14.3 |  |
| SimpleTexture | yes | differs | 0.75 | 42.0 | view: the screenshot is tilted; light |
| SpecGlossVsMetalRough | yes | matches | 0.97 | 14.9 |  |
| SpecularSilkPouf | yes | differs | 0.57 | 19.8 | view: the screenshot is closer |
| SpecularTest | yes | matches | 0.87 | 6.2 |  |
| Sponza | yes | differs | 0.20 | 17.8 | view: the rule frames the building from outside, the screenshot is inside the atrium |
| StainedGlassLamp | yes | differs | 0.49 | 23.6 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): the glass shade is missing |
| SunglassesKhronos | yes | differs | 0.09 | 30.6 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)): the lenses are missing |
| Suzanne | yes | differs | 0.54 | 16.5 | background: the screenshot has a photograph behind it |
| TextureCoordinateTest | yes | matches | 1.00 | 14.7 |  |
| TextureEncodingTest | yes | differs | 0.31 | 23.5 | crop: the screenshot shows part of the panel |
| TextureLinearInterpolationTest | yes | differs | 0.30 | 38.8 | view |
| TextureSettingsTest | yes | matches | 0.96 | 14.8 |  |
| TextureTransformMultiTest | yes | matches | 0.91 | 19.7 |  |
| TextureTransformTest | yes | differs | 0.78 | 21.3 | light: darker; the transforms are right |
| ToyCar | yes | differs | 0.73 | 39.5 | the file's camera is closer than the screenshot's; environment: the comparison rig's flat studio sky |
| TrafficCone | yes | differs | 0.57 | 25.2 | background: the screenshot has a photographed floor (file camera) |
| TransmissionOrderTest | yes | differs | 0.27 | 16.7 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| TransmissionRoughnessTest | yes | differs | 0.37 | 16.9 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| TransmissionTest | yes | matches | 0.72 | 17.8 |  |
| TransmissionThinwallTestGrid | yes | differs | 0.44 | 23.0 | transmissive materials vanish ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| Triangle | yes | differs | 0.98 | 26.6 | light: the sun lights the white triangle fully, the screenshot shows it grey |
| TriangleWithoutIndices | yes | differs | 0.98 | 26.6 | light: as Triangle |
| TwoSidedPlane | yes | differs | 0.00 | 15.0 | view: the plane lies flat and the rule sees it edge-on |
| Unicode❤♻Test | yes | matches | 0.85 | 19.5 |  |
| UnlitTest | yes | differs | 0.87 | 30.2 | view |
| USDShaderBallForGltf | yes | differs | 0.34 | 43.6 | the screenshot combines two views and shows transmission ([#595](https://github.com/ponkoengine/ponko/issues/595)) |
| VertexColorTest | yes | matches | 0.82 | 14.9 |  |
| VirtualCity | yes | differs | 0.17 | 54.6 | the file’s camera looks down onto the city; the reference shows a different view and orange sunset surroundings |
| WaterBottle | yes | differs | 0.35 | 25.3 | background: the screenshot has a backdrop behind it; view |
| XmpMetadataRoundedCube | yes | differs | 0.27 | 27.4 | view; light |

### Animations through pointers

```sh
cargo test -p ponko --lib --features 3d,compressed-textures,gltf-meshopt,image-webp,image-gif \
  known_pointer_animations_play -- --nocapture
```

`known_pointer_animations_play` draws the five samples with
`KHR_animation_pointer` animations at three times each, sampling their clips
into the model's skeleton as a game's animator does, with the file's lights
hung from their nodes' joints (`ModelLight::joint`). Each drawing has its
negative control: the same time with the clip's property tracks taken out,
as they were drawn before they played. The pairs (control left) go to
`results/pointer/<Name>-<time>.png` in the cache, the numbers and the
adapter to `results/pointer/pointer.json`. On an Apple M4 Max (Metal):

| Sample | Times (s) | What the test checks | Measured |
|---|---|---|---|
| CubeVisibility | 0.25, 0.75, 1.25 | the blue cube shows, hides, shows | hidden at 0.75: half the model's pixels go (19264 to 9630), mean difference from the control 8.0; shown: identical to it |
| LightVisibility | 0.25, 0.75, 1.25 | the blue spot light shines, goes out, shines | out at 0.75: less blue, mean difference 1.7; on: identical |
| AnimatedColorsCube | 0.75, 1.75, 2.75 | the moving cube is green over the green cube, blue over the blue one, red back over the red one | the changed pixels are green, then blue, where the control's are red; red again: identical |
| AnimationPointerUVs | 0, 3, 6 | the keyed base color transforms (offset, scale, rotation) take their keyed values, and the textures move from time to time where the control's don't | every value exact; the other maps' channels warn ([#666](https://github.com/ponkoengine/ponko/issues/666)) |
| PotOfCoalsAnimationPointer | 0, 1, 2 | the heat dome's normal map turns 0, a quarter and half a turn | exact; the dome is fully transmissive, drawn invisible until [#595](https://github.com/ponkoengine/ponko/issues/595), so the picture doesn't change |

None of their screenshots matches at any of the times: the screenshots are
taken from other views, and the GIF and WebP ones are animated (the first
frame is compared).

## The two scenes

```sh
cargo test --release -p ponko --lib --features 3d,compressed-textures known_scenes_benchmark -- --ignored --nocapture
rm -rf target/release
```

`known_scenes_benchmark` is ignored by default: it is a measurement, run in
release on purpose. For each scene it loads the file once and times it,
reads the process's resident memory, then at 1920×1080 and 2560×1440 and
for each quality preset draws 20 frames to warm up and times 60, each
waited for on the GPU. It reports the median and 95th percentile frame time
(CPU and GPU, wall clock), the GPU time of each pass (timestamp queries,
averaged over the timed frames) and the renderer's GPU memory. The results
go to `results/scenes-<time>.json` in the cache, with a picture of each
scene.

- **Sponza** (Khronos sample, glTF with PNG and JPEG textures): a camera at
  one end of the atrium at head height, looking down its length. The
  file has no lights; the engine's default sun lights it.
- **Bistro** (about 2 GB, BC7 textures): the file's own camera down the
  street, its 6830 lux sun and 96 point lights placed at EV100 11.
  Set `PONKO_KNOWN_SCENE=Bistro` to measure just this scene, and
  `PONKO_KNOWN_STUDIO=1` to replace the sky IBL with the procedural studio.
  Both settings use the same lighting atlas, sampling and BRDF.

For #597, the Bistro at 1920×1080 Medium had median GPU frame times of
5.45 ms with sky IBL and 5.44 ms with the studio map. Median wall-clock
frame times were 9.43 ms and 8.77 ms, respectively. These are medians of
three offscreen release runs per setting on Apple M4 Max (Metal), with
20 warm-up and 60 timed frames per run. The machine was busy compiling
other work, and GPU times varied more between runs than between sources.
Both settings used 2128 MB of GPU memory.

The original scene measurements from #603 used an Apple M4 Max (Metal),
a release build and the median of three runs. **The machine was busy** (load average 34–62 from other builds), so
these are an upper bound; frame time is CPU and GPU together.

| | Sponza | Bistro |
|---|---|---|
| Load (parse, upload, first frame) | 0.6–0.8 s | 0.9–1.6 s |
| Resident memory added by the load | 420–440 MB | 820–1500 MB |
| GPU memory: textures, meshes | 380 MB, 21 MB | 1581 MB, 230 MB |

The load times are with the files in the operating system's file cache.

| Scene | Size | Preset | Frame median (ms) | p95 (ms) | GPU (ms) | GPU memory (MB) | Slowest passes (GPU ms) |
|---|---|---|---:|---:|---:|---:|---|
| Sponza | 1920×1080 | Low | 1.26 | 1.74 | 0.70 | 498 | opaque 0.52, bloom 0.10, upscale 0.03, post 0.03 |
| Sponza | 1920×1080 | Medium | 1.83 | 4.59 | 1.17 | 620 | opaque 0.91, bloom 0.15, post 0.06, sprites 0.04 |
| Sponza | 1920×1080 | High | 6.69 | 7.35 | 5.62 | 778 | screen reflections 2.25, opaque 1.36, GTAO bilateral 0.90, GTAO horizons 0.40 |
| Sponza | 1920×1080 | Ultra | 10.86 | 14.39 | 8.44 | 994 | screen reflections 2.37, opaque 2.13, gi views 1.36, GTAO bilateral 0.91 |
| Sponza | 2560×1440 | Low | 1.39 | 1.57 | 0.95 | 555 | opaque 0.70, bloom 0.14, upscale 0.05, post 0.05 |
| Sponza | 2560×1440 | Medium | 2.32 | 3.16 | 1.75 | 741 | opaque 1.36, bloom 0.21, post 0.10, sprites 0.07 |
| Sponza | 2560×1440 | High | 9.96 | 10.62 | 8.97 | 1009 | screen reflections 3.64, opaque 2.08, GTAO bilateral 1.63, GTAO horizons 0.52 |
| Sponza | 2560×1440 | Ultra | 14.95 | 16.75 | 12.62 | 1224 | screen reflections 3.67, opaque 3.32, GTAO bilateral 1.62, GTAO horizons 1.61 |
| Bistro | 1920×1080 | Low | 5.78 | 10.25 | 4.24 | 2005 | opaque 4.07, bloom 0.10, upscale 0.03, post 0.03 |
| Bistro | 1920×1080 | Medium | 6.57 | 8.62 | 5.46 | 2128 | opaque 5.18, bloom 0.16, post 0.06, sprites 0.05 |
| Bistro | 1920×1080 | High | 15.41 | 17.00 | 13.90 | 2286 | opaque 5.79, screen reflections 4.95, GTAO bilateral 0.91, depth prepass 0.77 |
| Bistro | 1920×1080 | Ultra | 51.72 | 55.49 | 40.26 | 2501 | gi views 23.80, opaque 7.45, screen reflections 4.46, gi shadows 1.51 |
| Bistro | 2560×1440 | Low | 6.12 | 6.70 | 4.90 | 2063 | opaque 4.64, bloom 0.14, upscale 0.05, post 0.05 |
| Bistro | 2560×1440 | Medium | 8.58 | 10.03 | 7.38 | 2248 | opaque 6.97, bloom 0.21, post 0.10, sprites 0.08 |
| Bistro | 2560×1440 | High | 21.48 | 24.16 | 20.08 | 2516 | opaque 8.42, screen reflections 7.14, GTAO bilateral 1.64, depth prepass 0.83 |
| Bistro | 2560×1440 | Ultra | 58.64 | 68.23 | 47.82 | 2732 | gi views 23.95, opaque 11.00, screen reflections 6.56, GTAO bilateral 1.69 |

GPU memory grows with the preset through the frame targets (the full-size
buffers of the effects a preset turns on) and the shadow maps, not the
scene. Low and Medium draw both scenes in under 10 ms at 1440p; High adds
screen-space reflections and ambient occlusion, the largest passes after
the opaque one. On Ultra the Bistro's global illumination views cost about
24 ms of the GPU's 40–48 ms: the street, its many lights and its depth make
them the pass to look at first.

## Updating after a fix

1. Fix the loader or the renderer, with a test of its own.
2. Run the sweep and the drawings with `PONKO_UPDATE_KNOWN_CONTENT=1`; the
   fixture lists show what changed in the diff.
3. Look at the drawings that changed (`results/draw/`), and update the
   table above and the issue.

To move to a newer revision of the samples, change the revision in
`scripts/fetch-known-content.py`, fetch into an empty cache, and record
both lists again: their `# revision` line says which revision they are
for, and a cache of another revision is not compared.

## The check list

The `known-content` job of `scripts/checks.yml` runs the tool's self-test,
the fetch, the sweep and the drawings. It runs only when
`PONKO_KNOWN_CONTENT` names a cache folder, since the fetch is large:

```sh
PONKO_KNOWN_CONTENT=~/known-content python3 scripts/run-checks.py --job known-content
```
