# Ponko roadmap

Ponko aims to support complete 2D and 3D games through an integrated, high-performance, code-first engine with useful AI development tools. The direction is broad capability with low incidental complexity, preserving the custom archetype ECS, wgpu renderer, headless simulation and Rust/C++ workflows. See [PROJECT_VISION.md](PROJECT_VISION.md) and the current [ARCHITECTURE.md](ARCHITECTURE.md).

This roadmap is directional. Ordering changes with correctness evidence, measurements and real developer needs; a release is defined by its scope, and any date here is a target. An issue describes intended work, not a delivery promise. P1 issues define a release; P2 issues ship when ready and otherwise move to the next one; P3 is research or lower-priority work that never holds a release and may end in rejection. [STABILITY.md](STABILITY.md#releases) says how releases are cut; the index issue (#433) lists every planned item, and the [`0.2.0` milestone](https://github.com/ponkoengine/ponko/milestones) holds the live state.

## Current capability inventory

**Implemented** means the mechanism exists in source, not that every use case is production proven. **Partial** means useful building blocks exist with important integration or coverage gaps. **Missing** means the named workflow was not found in the inspected implementation. **Requires verification** marks claims needing platform/runtime evidence beyond source inspection. Per feature, the guide's [maturity levels](book/src/maturity.md) say how proven each one is (proven, tested or experimental, as [STABILITY.md](STABILITY.md#maturity-levels) defines them).

| Complete-game area | Status | Evidence and remaining work |
|---|---|---|
| ECS and fixed-step simulation | Implemented | [ECS](crates/ponko_ecs/src), [game loop](src/game.rs), [determinism tests](tests/determinism.rs). Systems are sequential by default; view systems can opt into parallel batches ([schedule](src/schedule.rs)). |
| World, scene and prefab management | Implemented | [scene.rs](src/scene.rs), [reflection](src/reflect.rs), [scene tests](tests/scenes.rs). Scene editing in 3D, with picking, gizmos, a hierarchy and an asset browser, is in the [editor](book/src/editor.md) (#415; [source](src/edit3d.rs)). [Ember Hop](examples/ember_hop) spawns its levels' objects as prefabs (one with children) and its title as a scene. Advanced world composition and complete service-state restoration need use-case validation. |
| Asset importing and processing | Partial | [glTF](src/three/gltf.rs), with extensions and compressed geometry (#404; [guide](book/src/3d.md#gltf-extensions-lights-and-cameras), [tests](tests/gltf_extensions.rs)); asset packs and `ponko pack` (#405; [guide](book/src/packaging.md#assets-in-one-pack-ponko-pack), [tests](tests/asset_packs.rs)); [Unity tooling](crates/ponko_unity), with humanoid clips (#431; [guide](book/src/unity.md#humanoid-clips)); [assets](src/assets.rs). Import support varies by format and feature; it is not source-engine runtime compatibility. glTF's material layers (clear coat, sheen, specular, transmission and volume, iridescence, anisotropy), unlit materials, node visibility, texture transforms and physical light units are drawn (#595–#597). |
| Asset streaming and residency | Partial | [Job/CellStreamer](src/streaming.rs), [stream tests](tests/streaming.rs). Residency budgets for texture mips and meshes shipped in 0.1.0. Instance batches stream by cell (#396) and textures can be GPU-compressed (#403). |
| Meshes, materials, animation and skinning | Implemented | [mesh](src/three/mesh.rs), [clips](src/three/clip.rs), [animator](src/three/animator.rs), [skin](src/three/skin.rs), [morph tests](tests/morph3d.rs). Humanoid retargeting (#408; [guide](book/src/3d.md#retargeting-humanoid-animations), [tests](tests/retarget.rs)) and GPU-driven batches of skinned characters, the [crowds](book/src/crowds.md) (#406; [tests](tests/crowds.rs)), are in. Verify particular imported animation workflows; presence is not arbitrary asset parity. Skinned models of one mesh are posed by one compute dispatch: 1,000 figures with a sun shadow went from 6.0 to 2.9 ms of GPU time on an Apple M4 Max (#488, closed); on a Radeon RX Vega Bevy is still slightly faster on Vulkan and slower on DirectX 12 ([scoreboard](book/src/performance.md#windows-desktop-ryzen-5-3600-radeon-rx-vega-directx-12-and-vulkan)). |
| Terrain, vegetation, LOD and large worlds | Partial | [terrain](src/three/terrain.rs), [far terrain](src/three/far_terrain.rs), [instances](src/three/instances.rs), [LOD](src/three/lod.rs), [streaming example](examples/streaming.rs). Budgeted residency and incremental extraction shipped in 0.1.0, and instance batches stream by cell (see the streaming row). |
| Characters, collision, physics and navigation | Implemented | [physics3d](src/physics3d), [navigation](src/nav3d), [vehicle tests](tests/vehicle3d.rs), [navigation tests](tests/navmesh3d.rs). Navmesh carving with rotated and non-box obstacles (#407; [guide](book/src/navigation3d.md#dynamic-carving)) is in. Features are optional; real game integration and target performance still need evidence. |
| UI, text, actions and gamepads | Implemented | [retained UI](src/ui), [text](src/text.rs), [actions](src/actions.rs), [gamepads](src/gamepad.rs), [UI tests](tests/ui_retained.rs). Input backend and native API coverage vary by target. Text shaping, right-to-left text, IME, localization and rich text (#409–#412) are in. |
| Accessibility | Requires verification | UI and input building blocks exist, and screen readers are supported on macOS, Windows and Linux (#413); no comprehensive assistive-technology, navigation or accessibility conformance claim is established here, and the Windows and Linux readers need runs on real machines (#484). Audit with a playable reference workflow before defining additions. |
| Audio and spatial audio | Implemented | [audio](src/audio.rs), [audio3d example](examples/audio3d.rs), [audio tests](tests/audio.rs). Audio on the web (#419: streamed music, sounds requested before the first click; and #458: WebKit on Linux reporting 0 output channels) is in the [guide](book/src/audio.md#in-a-browser); the browser checks that cover it are in the [macOS record](book/src/check-results/macos.md). Live output/device behavior requires runtime checks. |
| Multiplayer, synchronization and servers | Partial | [lockstep](src/net.rs), [snapshots](src/net/snapshots.rs), [messages](src/net/messages.rs), [replication tests](tests/net_replication.rs), headless runner. Client prediction, rollback, browser multiplayer and dedicated servers shipped (#420–#424). Live transport/relay behavior, sessions over the relay (#486), Steam sockets (#477) and a complete deployment/authority workflow require verification on real networks (#484). |
| Save/load and serialization | Implemented | [reflection](src/reflect.rs), [scenes](src/scene.rs), [stability contract](STABILITY.md). [Ember Hop](examples/ember_hop) saves and loads its runs as world save slots ([tests](tests/ember_hop.rs)). Applications own custom component migrations and external service persistence. |
| GPU compute and custom graphics | Implemented | [compute](src/three/compute.rs), [effects](src/three/effect.rs), [shader example](examples/shaders.rs). Named-stage custom passes shipped in 0.1.0. Stencil state on materials (#397), a custom sky hook (#398), [post effects](book/src/3d.md#post-processing) (#401; [render tests](src/render/post_effects_render_test.rs)), transparency sorting and [particles](src/three/particles.rs) (#400), and [motion vectors](src/three/motion.rs) for skinned meshes, morph targets and custom vertex functions (#391, #392) are in. Rendering gaps found since are closed (#491, #595–#597); how each effect holds up on each GPU is in the render tests' records, not here. |
| Build tooling, packaging and deployment | Partial | [Cargo manifest](Cargo.toml), [web tooling](web), [`ponko new`](book/src/getting-started.md) (#371), [`ponko package`](book/src/packaging.md#packaging-any-game-ponko-package) (#366), [release steps](CONTRIBUTING.md#releasing) with release candidates and patch branches (#358). Trusted publishing (#357) is closed, but the repository has no CI, so a maintainer publishes with a crate owner's token. The project doesn't sign or notarize its own release downloads (it ships checksums); games sign their packages with `ponko package`'s signing hook. The release steps and scripts are not proof that a complete reference game has shipped through every path. |
| Desktop and browser portability | Requires verification | Target-specific implementation and the [list of checks](scripts/checks.yml) exist; see [platform policy](STABILITY.md#platforms). Recorded runs of the list: [macOS on Apple silicon](book/src/check-results/macos.md); on Windows (AMD) the render tests, [Shard Grove's baseline](bench/shard_grove/windows-ryzen5-3600-rx-vega/README.md) and the [scoreboard](book/src/performance.md#windows-desktop-ryzen-5-3600-radeon-rx-vega-directx-12-and-vulkan); Linux is pending (#500). Full visual/runtime parity and individual GPU feature fallbacks need adapter evidence. WebGPU on a real GPU and 3D on WebGL2 (#427, closed: the `web-gpu` job; its run is in the macOS record) and [threads on the web](book/src/web-threads.md) (#425, closed, experimental) are in. HDR output (#429, [guide](book/src/hdr.md)) is still open: nothing was looked at on an HDR display. Until a row of #484 is ticked, none of these is a validated commitment. |
| Mobile | Partial | iOS (#476, closed) builds, handles touch, lifecycle, the keyboard inset and the audio session, and packages: [iOS chapter](book/src/ios.md), [simulator check](scripts/ios-simulator.sh) and the `ios` job of the [list of checks](scripts/checks.yml). It has run on the simulator only; a device run (touch, typing, interruptions, memory warnings) is a check for a person (#484). Android (#475, closed) builds for arm64, runs with Vulkan or OpenGL ES and packages as an `.apk` or an app bundle ([Android chapter](book/src/android.md)); it has run on the emulator only, and a phone run is a check for a person (#484). [Touch](book/src/touch.md) controls are tested headless. Mobile is not a validated platform. |
| Debugging, profiling and automated testing | Implemented | [tests](tests), [GPU tests](src/render), [performance reports](src/perf_report.rs), including GPU memory (#393; [guide](book/src/profiling.md#gpu-memory)), [inspector](src/inspector.rs). Broader reproducible matrices and diagnostics remain below. |
| AI development and runtime control | Partial | [MCP](crates/ponko_mcp/src/lib.rs), [control](src/control.rs), [agent guide](book/src/agents.md): state, stepping, launch, screenshots, logs and profiling exist. The [agent acceptance suite](book/src/agents.md#the-agent-acceptance-suite) (#416; [tests](crates/ponko_mcp/tests/agent_suite.rs)) replays six game-development tasks through the tools and scores them; its baseline is recorded, and a first run of a real agent (6/6 tasks, 2026-11-26) is in the [macOS record](book/src/check-results/macos.md). Reliable autonomous complete-game production is not established. |
| End-to-end shipping reference workflow | Partial | Two packaged, smoke-tested 2D reference games. [Coin Dash](examples/coin_dash): menus, rebinding, sound and its own saves ([packaging](book/src/packaging.md)). [Ember Hop](examples/ember_hop), a platformer: Tiled levels with autotiled ground and tile colliders, 2D physics with sensors as its rules, prefabs with children, a title scene, world save slots at checkpoints, sound files and streamed music, particles, a following camera, keyboard and gamepad ([its chapter](book/src/ember-hop.md)). Both are packaged by `ponko package`, the list of checks plays each package, and both build for the web. [Shard Grove](examples/shard_grove), the 3D reference: a glTF level that is drawn, solid and baked into a navigation mesh, skinned glTF characters with animation state machines, a character controller, enemies on the navmesh, positional sound, menus, a world-space sign, quality presets and saves ([its chapter](book/src/shard-grove.md)); it packages and builds for the web; its package is played headless by the list of checks (#593), and its GPU baseline is recorded on Windows with an AMD GPU ([DirectX 12 and Vulkan](bench/shard_grove/windows-ryzen5-3600-rx-vega/README.md)); opening it in a window and a WebGPU browser on each OS, and the baselines for other GPUs, are open and need a person (#454). The web gallery that publishes the examples (#373), the 3D reference game (#375) and the gaps the platformer found (#437–#439) are closed; #440 (flipping sprites without a negative scale) is closed. |

## After 0.1.0: one release, 0.2.0

0.1.0 closed the foundations backlog (#132 to #207): benchmarks and
differential tests, the access-checked parallel scheduler, change detection,
the renderer split into subsystems, incremental extraction, GPU tiers,
residency budgets, custom passes, and the API reshaped around services and
one prelude. What a review of 0.1.0 found missing is mostly what turns a
game into a shipped product.

That was first planned as four releases (0.2 for 2D, 0.3 for 3D and text,
0.4 for multiplayer and platforms, and a "Later" list). On 2026-11-25 they
were merged: **0.2.0 carries everything after 0.1.0 except the 1.0 freeze
work.** A release candidate tests it as a whole, so what shipped in the
middle of the plan is not left waiting for a second tag. Breaking changes are
still allowed, each with its migration note, and 0.2.0 keeps the right to
change the API: the freeze review comes after it.

0.2.0 is meant to be a finished product, not a preview. Besides the scope
below, the release is held to a bar (tracker #528): no open issue labelled
`bug`; every check in [scripts/checks.yml](scripts/checks.yml) run and green
on Windows, macOS and Linux; the numbers measured; the guide matching the
crate; and the two 2D reference games, the 3D one, and localized text shown
working from an empty project to a package and a web build. Tagging and
publishing are done by a maintainer ([release steps](CONTRIBUTING.md#releasing)),
and the mechanical cut is #474. Target: early 2027, when the scope is done.
Each outcome below names its issues; the GitHub milestone has their state.

### Ship a small game

A complete 2D game goes from an empty project to a packaged download and a
web build without engine-level code, and releases become routine.

| Outcome | Issues |
|---|---|
| Start and learn | Project template and `new` command #371; 2D and 3D tutorials #372; a guide per release #246; web gallery #373 |
| 2D essentials | Image formats #377; blend modes and sprite shaders #378; 2D particles #379; tilemap import, animation and colliders #381; flipping sprites without a negative scale #440; 2D lights and shadows #380 |
| Scenes and saves | Prefabs with children #367; user data folder, atomic saves, browser storage #365; pressed-this-frame input for rebinding #369; save migrations #368; a binary save format #430 |
| Test and ship | Headless UI and frame systems #370; `package` command #366; a second 2D reference game #374; agent acceptance suite #416; web audio #419 and #458; soak runs of the reference games #509 |
| API | One path per type #383; `Ctx` shortcuts #384; typed handles, one type per handle kind (render targets, sprite shaders and materials included) #386; remaining C and C++ bindings #387; immediate UI scale and theme #414; third-party types in the API #385; dependency-based system batches #388 |
| Releases | Semver checks #355; API snapshots #356; trusted publishing #357; release candidates and patch branches #358; migration notes #359; dependency audit #360; benchmark tracking #361 |

### 3D production and text

A complete 3D game with imported content, text that can be localized, and
scene editing in 3D.

| Outcome | Issues |
|---|---|
| Content | Compressed textures #403; glTF extensions #404; asset packs #405; animation retargeting #408; Unity importer humanoid clips and cross-fade groups #431 |
| Rendering | Motion vectors for animated meshes #391; and for custom vertex functions #392; GI coverage #394; stencil #397; custom sky #398; probe refresh #399; transparency and particles #400; post effects #401; instance batch streaming #396; GPU memory reports #393; contact-hardening soft shadows #402; screen-space reflections with cone blur, normal maps and transparents #395; GPU-driven skinned batches #406; skinned characters at the other engine's frame time #488 |
| Text and UI | Shaping, fallback and right-to-left #409; IME #410; localization #411; rich text #412; screen readers on Windows and Linux #413 |
| Tools | 3D scene editing #415; a 3D reference game #375, packaged and played with its GPU baseline recorded #454; WebGPU checked on a real GPU #427 |
| Simulation | Navmesh carving #407 |

### Multiplayer and platforms

Prediction, browser multiplayer, servers, the mixer, and the platforms that
are not desktop:

- Client prediction #420, rollback evaluation #421, multiplayer in the
  browser #422, Steam sockets #423 and the Networking Sockets backend #477,
  dedicated servers #424, relay and WebRTC sessions, TURN on desktop and the
  client's real address behind a proxy #486.
- The audio mixer #418.
- Threads on the web #425, HDR display output #429, inspecting and
  controlling web builds #417, C and C++ games on the web #428, C library
  downloads for Intel Mac, Linux arm64 and Windows arm64 #363, and a
  reference game ported to C++ #376.
- iOS #476 and Android #475 (both shipped, checked on the simulator and
  the emulator; device runs are #484): build, touch, lifecycle and packaging. The
  evaluation #426 is closed.
- The multiplayer lines of #484 (a browser joining a server, two machines with
  a cable pulled, WebRTC across a real NAT, a Steam match between two
  accounts) hold this release too. The maintainers run them on their own
  devices and accounts.

Rollback is decided: **shipped**. `ponko::net::Rollback` (input delay,
prediction, re-simulation, packets over any transport, desync checks) runs
games that save and load their own state, and `GameRollback` rolls back a
whole `Game` from the engine's loop: a typed world snapshot that keeps
entity ids (10,000 entities save and restore in about 0.005 ms each),
`save_state` / `restore_state` for both physics services (bit for bit), the
event store, and re-running the simulation systems from a saved tick, with
sounds of re-run ticks dropped. With physics, rapier's state is cloned
every tick, so about a thousand bodies fit a 60 Hz frame. The numbers and
what is captured are in the guide's "Rollback" section
(`book/src/networking.md`).

### Research and lower-priority work, now in

What was parked as "Later", and is now closed or in scope: fuzzing the loaders #362,
2D skeletal animation #382 (evaluated), batched structural moves #349 and
cached query columns #350, and the platform and rendering items listed above
(#363, #376, #395, #402, #417, #429, #431, #440). P3 issues still never hold
the release: one that is not done moves out of 0.2.0 in writing, in #528,
with the reason.

### 0.3.0

Removing what 0.2.0 deprecated (#485; 140 items are marked `since = "0.2.0"`).
Nothing else is planned for 0.3.0 until 0.2.0 is out, so what a real game
finds goes into its plan then.

### 1.0: the API freeze

1.0 is declared when the readiness checklist is complete, not on a date: the
API has gone two minor releases with only mechanical breaks, reference
games in 2D and 3D and games by other people have shipped on it, every named
platform passes the list of checks, and the compatibility policy for after
1.0 is written. These stay out of 0.2.0 because 0.2.0 keeps the right to change the API: the
C ABI decision #390, the second API freeze review #478, the compatibility
policy after 1.0 #479 and the readiness checklist #480. The last 0.x release carries every remaining break; 1.0.0
breaks nothing.

### Not scheduled

Ray tracing and mesh shaders were evaluated for 0.1.0 and stay research.

VR and XR are decided: **later**, not a goal of any release yet.
The renderer already draws two eyes as two views (shadows, skinning and
global illumination are shared; culling and every camera pass run per eye,
so the CPU side of a stereo frame costs about twice one eye's). A headset
still needs per-eye off-axis projections, drawing into a runtime's
swapchain images, the runtime's frame pacing, and tracked input through
OpenXR; that work moves into a release when a game needs it. The report,
the prototype's numbers and the steps are in the guide's "VR and XR"
chapter (`book/src/xr.md`).

## How work is chosen

- Complete games decide. A gap found by a reference game or by a game built
  on the engine outranks a feature nobody is waiting for.
- Foundations stay measured: benchmarks, differential tests and render
  regressions come before a redesign, and a measured rejection is a valid
  result.
- The simple path stays simple: `Game::new()`, systems, `.run()`. New power
  is optional and costs nothing when unused; 2D and headless builds do not
  pay for 3D.
- Compatibility is kept as [STABILITY.md](STABILITY.md) describes; a break
  lands in a minor release with its migration note.
- [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md) define the
  review and validation workflow. Rust-only contributions are welcome;
  maintainers own guide and C/C++ follow-ups.
