//! The material pipeline cache: typed keys, shared pipeline sets for
//! identical shaders, and the counters behind [`crate::render::PipelineStats`].
//!
//! A material shader's pipelines depend on:
//!
//! | Part of the key | From | Changes |
//! |---|---|---|
//! | [`ModuleKey::source`] | the game's WGSL | on a hot reload |
//! | [`ModuleKey::features`] | decals, global illumination, instance storage | fixed per device |
//! | [`TargetKey`] | HDR, depth and motion formats, samples per pixel | fixed per renderer |
//! | [`ShaderVariant`](super::ShaderVariant) | sides and instancing of a draw (`variants.rs`) | per draw |
//! | [`PipelineRole`](super::PipelineRole) | the pass and the material's blend | per pass |
//!
//! [`MaterialCache`] maps a [`SetKey`] (module and targets) to the one
//! [`ShaderPipelines`](super::ShaderPipelines) built for it: two shaders
//! made from the same code share it, and a shader reloaded back to code
//! another shader still uses takes that one instead of compiling. Inside a
//! set each [`ShaderVariant`](super::ShaderVariant) is built once (plain ones with the shader,
//! the others on first use or by warming) and kept while any shader uses
//! the set. Replacing a shader drops only its own set, and only when no
//! other shader shares it; the other shaders' pipelines stay as they are.
//!
//! Every other pipeline of the renderer is fixed: built once when the 3D
//! renderer starts (post, FXAA, SMAA, TAA, SSR, AO, sky, particles, decals
//! off/on, skinning and culling compute passes), or once per screen effect
//! and compute pass the game makes (rebuilt only when that one is
//! reloaded). See `variants.rs` for which draws use which variant.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::ShaderPipelines;
#[cfg(test)]
use super::ShaderVariant;
use crate::render::PipelineStats;

/// The optional parts of the engine's code a material shader is composed
/// with: whichever the device has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Features {
    pub decals: bool,
    pub gi: bool,
    /// Per-model storage buffers (`items` in `vertex`).
    pub storage: bool,
    /// The opaque pass reads the reflections' spread image itself (see
    /// `GpuLimits::reflection_spread`).
    pub spread: bool,
}

/// Which shader module a set is built from: the game's code and the
/// engine parts composed around it. Equal keys compose equal modules.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ModuleKey {
    /// The game's WGSL as given (empty for the built-in shader).
    pub source: Arc<str>,
    pub features: Features,
}

/// What a set's pipelines draw into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TargetKey {
    pub color: wgpu::TextureFormat,
    pub depth: wgpu::TextureFormat,
    pub velocity: wgpu::TextureFormat,
    pub samples: u32,
}

/// One pipeline set: a module drawn into one kind of target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SetKey {
    pub module: ModuleKey,
    pub targets: TargetKey,
}

/// One variant of one set: everything that decides a material pipeline,
/// with the pass's [`PipelineRole`](super::PipelineRole). Names a built
/// variant when listing the cache ([`MaterialCache::built_keys`]).
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
    pub set: SetKey,
    pub variant: ShaderVariant,
}

/// Counters the sets and the renderer add to, from any recording thread.
#[derive(Default)]
pub(crate) struct Counters {
    /// Variant sets built while drawing, since the renderer last took them
    /// (`RenderStats::pipelines_built`).
    pub frame_misses: AtomicU64,
    lookups: AtomicU64,
    misses: AtomicU64,
    shared: AtomicU64,
    builds: AtomicU64,
    build_ns: AtomicU64,
    slowest_build_ns: AtomicU64,
    compiles: AtomicU64,
    compile_ns: AtomicU64,
    slowest_compile_ns: AtomicU64,
    last_compile_ns: AtomicU64,
    invalidated: AtomicU64,
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

fn ms(ns: &AtomicU64) -> f32 {
    ns.load(Ordering::Relaxed) as f32 / 1e6
}

impl Counters {
    /// `n` draws looked their pipelines up (hits and misses).
    pub fn looked_up(&self, n: u64) {
        if n > 0 {
            self.lookups.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// A draw found its variant unbuilt and built it.
    pub fn missed(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
        self.frame_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// A shader got another shader's set instead of compiling.
    pub fn shared(&self) {
        self.shared.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds the builds counted in `other` (a set that is now in use).
    pub fn add_builds(&self, other: &Counters) {
        self.builds
            .fetch_add(other.builds.load(Ordering::Relaxed), Ordering::Relaxed);
        self.build_ns
            .fetch_add(other.build_ns.load(Ordering::Relaxed), Ordering::Relaxed);
        self.slowest_build_ns
            .fetch_max(other.slowest_build_ns.load(Ordering::Relaxed), Ordering::Relaxed);
    }

    /// A variant set took `took` to build.
    pub fn built(&self, took: Duration) {
        let ns = nanos(took);
        self.builds.fetch_add(1, Ordering::Relaxed);
        self.build_ns.fetch_add(ns, Ordering::Relaxed);
        self.slowest_build_ns.fetch_max(ns, Ordering::Relaxed);
    }

    /// A game shader took `took` to compile (worked or not).
    pub fn compiled(&self, took: Duration) {
        let ns = nanos(took);
        self.compiles.fetch_add(1, Ordering::Relaxed);
        self.compile_ns.fetch_add(ns, Ordering::Relaxed);
        self.slowest_compile_ns.fetch_max(ns, Ordering::Relaxed);
        self.last_compile_ns.store(ns, Ordering::Relaxed);
    }

    /// `sets` built variant sets were thrown away.
    pub fn invalidated(&self, sets: u64) {
        self.invalidated.fetch_add(sets, Ordering::Relaxed);
    }
}

/// The material shaders' pipeline sets by [`SetKey`].
pub(crate) struct MaterialCache {
    /// Sets in use; an entry whose set every shader let go of is dead and
    /// pruned on the next insert.
    sets: HashMap<SetKey, Weak<ShaderPipelines>>,
    pub counters: Arc<Counters>,
    pub targets: TargetKey,
}

impl MaterialCache {
    pub fn new(targets: TargetKey) -> Self {
        Self {
            sets: HashMap::new(),
            counters: Arc::default(),
            targets,
        }
    }

    /// The key of `module` drawn into this renderer's targets.
    pub fn key(&self, module: ModuleKey) -> SetKey {
        SetKey {
            module,
            targets: self.targets,
        }
    }

    /// The set some shader already built for `key`. The caller counts it
    /// as shared ([`Counters::shared`]) unless it is the asking shader's own.
    pub fn get(&self, key: &SetKey) -> Option<Arc<ShaderPipelines>> {
        self.sets.get(key)?.upgrade()
    }

    /// Keeps `set` for shaders made from `key` later.
    pub fn insert(&mut self, key: SetKey, set: &Arc<ShaderPipelines>) {
        self.sets.retain(|_, s| s.strong_count() > 0);
        self.sets.insert(key, Arc::downgrade(set));
    }

    /// The live sets.
    pub(super) fn live(&self) -> impl Iterator<Item = (&SetKey, Arc<ShaderPipelines>)> {
        self.sets.iter().filter_map(|(k, s)| Some((k, s.upgrade()?)))
    }

    /// The pipeline keys built so far, in no particular order.
    #[cfg(test)]
    pub fn built_keys(&self) -> Vec<PipelineKey> {
        let mut keys = Vec::new();
        for (key, set) in self.live() {
            for variant in ShaderVariant::ALL {
                if set.built(variant) {
                    keys.push(PipelineKey {
                        set: key.clone(),
                        variant,
                    });
                }
            }
        }
        keys
    }

    /// The counters, with what is alive now.
    pub fn stats(&self) -> PipelineStats {
        let c = &self.counters;
        let (mut shaders, mut sets, mut pipelines, mut key_bytes) = (0, 0, 0, 0);
        for (key, set) in self.live() {
            shaders += 1;
            key_bytes += key.module.source.len();
            let built = set.built_count();
            sets += built;
            pipelines += built * set.pipelines_per_set();
        }
        let lookups = c.lookups.load(Ordering::Relaxed);
        let misses = c.misses.load(Ordering::Relaxed);
        PipelineStats {
            material_shaders: shaders,
            variant_sets: sets,
            pipelines,
            hits: lookups.saturating_sub(misses),
            misses,
            shared: c.shared.load(Ordering::Relaxed),
            builds: c.builds.load(Ordering::Relaxed),
            build_ms: ms(&c.build_ns),
            slowest_build_ms: ms(&c.slowest_build_ns),
            compiles: c.compiles.load(Ordering::Relaxed),
            compile_ms: ms(&c.compile_ns),
            slowest_compile_ms: ms(&c.slowest_compile_ns),
            last_compile_ms: ms(&c.last_compile_ns),
            invalidated: c.invalidated.load(Ordering::Relaxed),
            key_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str) -> ModuleKey {
        ModuleKey {
            source: source.into(),
            features: Features {
                decals: true,
                gi: false,
                storage: true,
                spread: false,
            },
        }
    }

    const TARGETS: TargetKey = TargetKey {
        color: wgpu::TextureFormat::Rgba16Float,
        depth: wgpu::TextureFormat::Depth32Float,
        velocity: wgpu::TextureFormat::Rg16Float,
        samples: 4,
    };

    #[test]
    fn keys_differ_in_every_part_that_changes_a_pipeline() {
        let base = SetKey {
            module: module("fn surface(s: Surface) -> Surface { return s; }"),
            targets: TARGETS,
        };
        assert_eq!(base, base.clone());
        let mut other = base.clone();
        other.module.source = "fn surface(s: Surface) -> Surface { var o = s; return o; }".into();
        assert_ne!(base, other, "the code");
        let mut other = base.clone();
        other.module.features.gi = true;
        assert_ne!(base, other, "the composed engine parts");
        let mut other = base.clone();
        other.targets.samples = 1;
        assert_ne!(base, other, "the sample count");
        let mut other = base.clone();
        other.targets.color = wgpu::TextureFormat::Rgba8Unorm;
        assert_ne!(base, other, "the target format");
        let plain = PipelineKey {
            set: base.clone(),
            variant: ShaderVariant::ALL[0],
        };
        let faded = PipelineKey {
            set: base,
            variant: ShaderVariant::ALL[2],
        };
        assert_ne!(plain, faded, "the variant");
    }

    #[test]
    fn counters_add_up_into_stats() {
        let cache = MaterialCache::new(TARGETS);
        let c = &cache.counters;
        c.looked_up(10);
        c.missed();
        c.missed();
        c.built(Duration::from_millis(3));
        c.built(Duration::from_millis(1));
        c.compiled(Duration::from_millis(5));
        c.compiled(Duration::from_millis(2));
        c.invalidated(4);
        let s = cache.stats();
        assert_eq!((s.hits, s.misses, s.builds, s.compiles, s.invalidated), (8, 2, 2, 2, 4));
        assert!((s.build_ms - 4.0).abs() < 1e-3 && (s.slowest_build_ms - 3.0).abs() < 1e-3);
        assert!((s.compile_ms - 7.0).abs() < 1e-3 && (s.slowest_compile_ms - 5.0).abs() < 1e-3);
        assert!((s.last_compile_ms - 2.0).abs() < 1e-3);
        assert_eq!((s.material_shaders, s.variant_sets, s.pipelines), (0, 0, 0));
    }
}
