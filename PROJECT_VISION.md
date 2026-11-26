# Ponko project vision

Ponko aims to provide the power of a serious 2D/3D game engine through a
high-performance, code-first architecture, with minimal unnecessary complexity
and first-class support for AI-assisted development.

The goal is complete games: small 2D games, character-driven 3D games, large
worlds, and complex simulations. Large entity counts are a strength, not a
restriction on what developers should build. The engine should feel small to
the developer even as its internals become sophisticated.

This document sets direction. [ARCHITECTURE.md](ARCHITECTURE.md) describes
current structure and proposed boundaries; [ROADMAP.md](ROADMAP.md) records
verified capability status and prioritized work. It does not declare feature
parity with Bevy or Unity, or production readiness for every genre.

## Simple public APIs, sophisticated internals

Ordinary games should start with `Game::new()`, clear setup code, optional
`.sim_system()` and physics configuration, then `.run()`. A developer should
not need to construct a renderer, assemble a dependency graph, or register a
large collection of plugins to draw a scene. Advanced configuration should be
optional, discoverable, and useful when a real project needs it.

Preserve the custom archetype ECS, fixed-step simulation, headless execution,
wgpu renderer, native APIs and runtime tooling. Evolve working systems through
small, verifiable steps. Learn from other engines without adopting their
architecture merely because it exists elsewhere.

## Modular internals, integrated experience

**Common things should work immediately. Advanced things should be customizable.
Neither should require understanding the entire engine.**

Modularity serves performance, maintainability, compile time, testing,
extensibility, and agent navigation. It must reduce complexity rather than
transfer it to users. Ponko should provide coherent built-in solutions for
common needs while allowing narrow customization where practical.

Three distinct levels guide the work:

1. **Compile-time selection.** Preserve and improve Cargo features. Headless
   servers should omit window/GPU dependencies; 2D projects should omit 3D;
   development tools should be optional. Existing feature flags already cover
   these choices, but not every dependency is independently optional.
2. **Internal boundaries.** Give ECS/scheduling, rendering, assets, physics,
   navigation, networking, audio, UI, scenes and tools clear responsibilities
   and dependency directions. Separate responsibilities within modules before
   creating public crates. Current coupling, especially renderer coordination,
   requires gradual improvement.
3. **Selective extensibility.** Keep gameplay systems, custom shaders and compute
   useful today; develop narrow pass, loader and transport interfaces where
   demonstrated needs justify them. Full renderer, ECS or asset-system
   replacement is not an immediate requirement.

Before adding a crate, trait, plugin system or interface, explain the concrete
problem, why existing modules are insufficient, the measurable or testable
benefit, the contributor/user cost, the smaller alternatives, and compatibility
and platform effects. Avoid dependency cycles, hidden allocations, unnecessary
indirection and speculative framework layers.

## Performance with evidence

Measure before optimizing. Representative workloads should report release-build
configuration, hardware, entity/scene composition, warmup and sample duration,
CPU and GPU cost, memory and allocations, and frame-time percentiles. Preserve
reproduction commands and compare equivalent workloads. Microbenchmarks inform
specific decisions; they do not establish overall engine superiority.

Correctness comes first: randomized/differential ECS tests, Miri, scheduling
access safety, resource lifetimes, GPU synchronization and visual checks matter
more than an isolated timing win. Avoid unnecessary steady-state allocations
on hot paths. Establish baselines before architectural changes and investigate
regressions with traces and representative games.

## ECS and simulation

Keep compact archetype storage, generational entities, typed/filtered queries,
parallel entity iteration and the accessible `Ctx` workflow. Maintain storage
independence from higher-level scheduling where practical.

Existing parallel query iteration is not automatic parallel system execution.
Planned access metadata and an optional dependency-aware scheduler must preserve
ordering, deferred-command/event semantics, safety and applicable determinism
contracts. Never run arbitrary mutable `Ctx` or `World` access concurrently.
The simple `.sim_system()` workflow should remain useful as advanced scheduling
becomes available.

Query caching, change detection, filtering, small-archetype load balancing and
diagnostics should follow the prioritized backlog and measured need. Determinism
is bounded by game code, arithmetic, physics configuration and matching builds;
[STABILITY.md](STABILITY.md#determinism) defines the actual promise.

## Graphics and assets

Continue the existing wgpu 2D/3D renderer. Improve resource management,
pipeline/material caching, scene preparation, culling, lights, shadows and pass
coordination through clearer internal boundaries. Prefer lightweight explicit
pass dependencies to a large render-graph framework introduced by default.
Incremental extraction, persistent state, reduced uploads, streaming budgets,
and broader visual regression coverage are development work, not blanket
claims about today's implementation.

A coherent asset lifecycle should cover import, caching, ownership, loading,
unloading, streaming, hot reload and diagnostics. Ordinary loading should stay
straightforward; game developers should not manage GPU lifetimes by hand.
External tools such as Blender remain practical authoring tools. Importers
provide supported conversions, not a promise that arbitrary projects from
another engine migrate completely.

Preserve supported native and web behavior. Advanced GPU paths need explicit
capability requirements, clear errors and feasible fallbacks. WebGPU/WebGL2
support does not imply every desktop feature works identically on the web.

## AI-assisted development is a product direction

The intended loop is concrete:

1. Discover documented APIs, capabilities and constraints.
2. Generate or modify game code/data, compile and launch.
3. Observe reflected state, logs and screenshots.
4. Control exact ticks and gameplay inputs in repeatable scenarios.
5. Reproduce failures, diagnose simulation/render/memory costs, correct code.
6. Rerun tests and provide state, timing and visual evidence.

Today the MCP server and control endpoint provide launch, entity/resource
inspection and mutation, scenes/prefabs, tick control, logs, screenshots for
windowed instances, profiling and traces. Reflection and headless execution
support reproducible checks. See [the tool contracts and examples](book/src/agents.md).

An [agent acceptance suite](book/src/agents.md#the-agent-acceptance-suite)
scores real tasks done through these tools. Broader capability discovery,
consistent subsystem diagnostics, richer input scenario automation, and
acceptance runs with real agents require further work. Reliable autonomous development of complete games is an objective, not
an existing guarantee. Agent usability must be tested through real tasks,
actionable errors and reproducible evidence. Use the same public interfaces for
humans and agents rather than building a separate engine architecture for AI.

## Code-first does not mean weak tools

No mandatory large visual editor is a design constraint. Optional inspectors,
scene viewers, visual debuggers and focused authoring tools are welcome when
they improve iteration. Good asset workflows, accessible UI, debugging and
profiling remain necessary. Supporting Rust and C/C++ means documenting real
API coverage and gaps, and preserving the C ABI contract.

Bevy is also code-first and supplies parallel scheduling and remote inspection.
Unity provides editor tooling, high-performance Entities/DOTS, and official AI
and MCP workflows. Ponko's intended distinction is an integrated combination
of APIs, tools and architectural choices. See the balanced
[comparison](book/src/comparison.md) for current tradeoffs and official sources.

## Build toward shipping games

Assess complete workflows across scenes/worlds, assets/animation, terrain/LOD,
physics/characters/navigation, input/UI/accessibility, audio, multiplayer/server
operation, save/load, compute/custom graphics, packaging, deployment and supported
platforms. Existing modules are a starting point; an API's presence is not proof
of complete game readiness. The roadmap must label implemented, partial, missing
and unverified areas, link prerequisites, and prioritize concrete shipping needs
over a disconnected wishlist.

Respect existing P0–P3 backlog priorities: foundations first, then high-impact
improvements, useful lower-priority work, and explicitly exploratory research.
No arbitrary release dates or blanket promises for every issue.

## Compatibility and contribution

[STABILITY.md](STABILITY.md) is authoritative for public APIs, semver,
MSRV, C ABI, saved worlds, MCP contracts, platforms and determinism within its
stated scope. Ponko is at 0.x, where a minor release may break things, but
every break is listed with its replacement. When a proposal conflicts with a
contract, document the conflict and prefer an additive design; name any
breaking change explicitly, so it lands in a minor release with its migration.

Start with [CONTRIBUTING.md](CONTRIBUTING.md). Keep changes focused, explain new
abstractions, provide relevant evidence, and keep capability documentation in
sync. The engine should become more capable while ordinary game development
stays understandable.
