//! What the renderer's pipeline cache did since the game started:
//! [`Graphics::pipeline_stats`](crate::gfx::Graphics::pipeline_stats).

use crate::gfx::Graphics;

/// The render pipeline cache's counters and timings, for debugging hitches
/// and shader reloads ([`Graphics::pipeline_stats`](crate::gfx::Graphics::pipeline_stats)).
///
/// Material shaders are built into a set of pipelines per variant (one-
/// or double-sided, plain, crossfading or buffer-driven instances), and in
/// each set one pipeline per pass (depth prepass, opaque, transparent,
/// shadow, motion vectors, and the alpha-to-coverage ones with
/// multisampling). The cache keys them by the shader's code, the GPU
/// features it is composed with, the target formats and the sample count:
/// shaders with the same code share one set, a frame that needs a variant
/// not built yet builds it once (a miss, and a hitch inside that frame),
/// and a hot reload replaces only the reloaded shader's sets.
///
/// Counts are totals since the renderer started; times are milliseconds
/// of CPU time spent in the calls that build pipelines (drivers that
/// compile later, on first use, hide part of their cost from it). All zero
/// in headless runs and without the `3d` feature.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct PipelineStats {
    /// Material shaders with pipelines now: the built-in one and the
    /// game's, those with the same code counted once.
    pub material_shaders: usize,
    /// Pipeline variant sets built and in use now, across those shaders.
    pub variant_sets: usize,
    /// Render pipelines in those sets.
    pub pipelines: usize,
    /// Draws whose pipelines were already built.
    pub hits: u64,
    /// Draws that had to build their variant first, inside a frame (see
    #[cfg_attr(feature = "3d", doc = " [`Graphics::warm_render_pipelines`](crate::gfx::Graphics::warm_render_pipelines)).")]
    #[cfg_attr(not(feature = "3d"), doc = " `Graphics::warm_render_pipelines`).")]
    pub misses: u64,
    /// Material shaders made or reloaded with the same code (and variant)
    /// as one already built: they share its pipelines, nothing is compiled.
    pub shared: u64,
    /// Variant sets built: with their shader, by warming, or on a miss.
    pub builds: u64,
    /// Total time building them, in ms.
    pub build_ms: f32,
    /// The slowest one, in ms.
    pub slowest_build_ms: f32,
    /// The game's shaders compiled (material shaders, screen effects and
    /// compute passes; made or reloaded, failed ones too).
    pub compiles: u64,
    /// Total time of those compiles, in ms: checking the WGSL, making the
    /// module and the pipelines built with it.
    pub compile_ms: f32,
    /// The slowest compile, in ms: the longest stall a create or a hot
    /// reload caused.
    pub slowest_compile_ms: f32,
    /// The last compile, in ms.
    pub last_compile_ms: f32,
    /// Variant sets thrown away because the shader they belonged to was
    /// reloaded (and no other shader shared them).
    pub invalidated: u64,
    /// Bytes of game WGSL the cache keeps to recognise identical shaders.
    /// GPU memory of the pipelines themselves is not visible to the engine.
    pub key_bytes: usize,
}

impl PipelineStats {
    /// Share of draws whose pipelines were already built, from 0 to 1 (1
    /// before anything is drawn).
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let lookups = self.hits + self.misses;
        if lookups == 0 {
            1.0
        } else {
            self.hits as f64 / lookups as f64
        }
    }
}

impl Graphics {
    /// What the render pipeline cache did since the game started: shaders
    /// and pipelines alive, hits and misses, how long building them took,
    /// and how many a hot reload threw away. All zero in headless runs.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let p = ctx.gfx.pipeline_stats();
    /// if p.misses > 0 {
    ///     eprintln!("{} pipeline builds inside frames; warm them while loading", p.misses);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn pipeline_stats(&self) -> PipelineStats {
        match &self.renderer {
            Some(renderer) if !self.headless => renderer.pipeline_stats(),
            _ => PipelineStats::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rate_is_one_before_any_lookup() {
        assert_eq!(PipelineStats::default().hit_rate(), 1.0);
        let p = PipelineStats {
            hits: 3,
            misses: 1,
            ..Default::default()
        };
        assert_eq!(p.hit_rate(), 0.75);
    }
}
