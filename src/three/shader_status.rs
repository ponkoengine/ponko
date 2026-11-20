//! The 3D side of shader status: stand-ins for effects and compute passes,
//! and GPU rejections that arrive after a shader was put in use.

use crate::gfx::Graphics;
#[cfg(feature = "window")]
use crate::render::{ShaderDiagnostic, ShaderOutcome};

/// What a screen effect that can't be built draws instead: the frame as it is.
pub(crate) const EFFECT_STAND_IN: &str = "fn effect(p: Pixel) -> vec3<f32> { return p.color; }";
/// The compute pass a file that doesn't compile gets (it is skipped).
pub(crate) const COMPUTE_STAND_IN: &str = "fn compute(c: Cell) -> vec4<f32> { return c.value; }";

impl Graphics {
    /// Associate the diagnostic with a live owner. Different resources
    /// can share the same diagnostic name.
    pub(crate) fn keep_shader_name(&mut self, kind: super::ShaderKind, id: u32, name: &str) {
        self.assets3d.shader_names.insert((kind, id), name.to_owned());
    }

    /// Diagnostics for a freed resource don't accumulate during churn.
    pub(crate) fn forget_shader_name(&mut self, kind: super::ShaderKind, id: u32) {
        if let Some(name) = self.assets3d.shader_names.remove(&(kind, id))
            && !self
                .assets3d
                .shader_names
                .iter()
                .any(|(&(k, _), n)| k == kind && *n == name)
        {
            self.shader_statuses.retain(|s| s.kind != kind || s.shader != name);
        }
    }

    /// Records shaders whose pipelines the GPU rejected after they were
    /// put in use (a browser's WebGPU answers a frame or so later): the
    /// renderer has put the previous ones back.
    #[cfg(feature = "window")]
    pub(crate) fn apply_shader_checks(&mut self) {
        if self.headless {
            return;
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        // Allocates only when something was rejected.
        let mut late = Vec::new();
        renderer.poll_shader_checks(&mut late);
        for error in late {
            eprintln!("ponko: {} (keeping the previous version)", error.diagnostic);
            self.record_rejected(error.reload, error.superseded, *error.diagnostic);
        }
    }

    /// A version of a shader recorded as compiled turned out to be rejected
    /// by the GPU. `reload`: it was a hot reload, whose count is undone.
    /// `superseded`: newer code of that shader was put in use since, and
    /// its status stands. Otherwise the previous version is back: the
    /// status is a failure, with this error (after any newer compiler
    /// error that already made it one).
    #[cfg(feature = "window")]
    pub(crate) fn record_rejected(&mut self, reload: bool, superseded: bool, diagnostic: ShaderDiagnostic) {
        let frame = self.frame;
        let Some(status) = self
            .shader_statuses
            .iter_mut()
            .find(|s| s.shader == diagnostic.shader && s.kind == diagnostic.kind)
        else {
            return;
        };
        if reload {
            status.reloads = status.reloads.saturating_sub(1);
        }
        if superseded {
            return;
        }
        status.frame = frame;
        match status.outcome {
            ShaderOutcome::Failed | ShaderOutcome::ReloadFailed => status.diagnostics.push(diagnostic),
            _ => {
                status.outcome = if reload {
                    ShaderOutcome::ReloadFailed
                } else {
                    ShaderOutcome::Failed
                };
                status.diagnostics = vec![diagnostic];
            }
        }
    }
}

#[cfg(all(test, feature = "window"))]
mod tests {
    use super::*;
    use crate::game::Game;
    use crate::render::ShaderKind;

    fn rejected(name: &str) -> ShaderDiagnostic {
        *crate::render::without_location(name, ShaderKind::Material, "v", "the GPU rejected its pipelines".into())
    }

    #[test]
    fn late_gpu_rejections_turn_compiles_into_failures_and_undo_the_reload_count() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.gfx.create_shader("new.wgsl", "");
            ctx.gfx.create_shader("reloaded.wgsl", "");
            ctx.gfx
                .record_shader("reloaded.wgsl", ShaderKind::Material, true, Ok(()))
                .unwrap();
            assert_eq!(ctx.gfx.shader_status()[1].reloads, 1);

            ctx.gfx.record_rejected(false, false, rejected("new.wgsl"));
            ctx.gfx.record_rejected(true, false, rejected("reloaded.wgsl"));
            let s = ctx.gfx.shader_status();
            assert_eq!(s[0].outcome, ShaderOutcome::Failed);
            assert_eq!(s[1].outcome, ShaderOutcome::ReloadFailed);
            assert_eq!(s[1].reloads, 0, "the reload didn't take effect after all");
            assert!(s[1].diagnostics[0].message.contains("rejected"));
            // Unknown names are ignored.
            ctx.gfx.record_rejected(true, false, rejected("gone.wgsl"));
            assert_eq!(ctx.gfx.shader_status().len(), 2);
        });
    }

    #[test]
    fn a_failed_reload_of_a_shader_that_never_compiled_stays_failed() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            assert!(ctx.gfx.try_create_shader("x.wgsl", "fn surface(").is_err());
            let again = ctx
                .gfx
                .record_shader("x.wgsl", ShaderKind::Material, true, Err(Box::new(rejected("x.wgsl"))));
            assert!(again.is_err());
            assert_eq!(ctx.gfx.shader_status()[0].outcome, ShaderOutcome::Failed);
        });
    }

    #[test]
    fn a_rejected_reload_behind_a_compiler_failure_undoes_its_count_and_adds_its_error() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.gfx.create_shader("r.wgsl", "");
            // A reload put in use, waiting on the GPU; then an edit that doesn't compile.
            ctx.gfx
                .record_shader("r.wgsl", ShaderKind::Material, true, Ok(()))
                .unwrap();
            let compile =
                ctx.gfx
                    .record_shader("r.wgsl", ShaderKind::Material, true, Err(Box::new(rejected("r.wgsl"))));
            assert!(compile.is_err());
            assert_eq!(ctx.gfx.shader_status()[0].reloads, 1);
            ctx.gfx.record_rejected(true, false, rejected("r.wgsl"));
            let s = &ctx.gfx.shader_status()[0];
            assert_eq!(s.outcome, ShaderOutcome::ReloadFailed);
            assert_eq!(s.reloads, 0, "the reload never took effect");
            assert_eq!(s.diagnostics.len(), 2, "the compiler's error and the GPU's");
        });
    }

    #[test]
    fn a_superseded_rejection_only_undoes_its_reload_count() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            ctx.gfx.create_shader("s.wgsl", "");
            ctx.gfx
                .record_shader("s.wgsl", ShaderKind::Material, true, Ok(()))
                .unwrap();
            ctx.gfx
                .record_shader("s.wgsl", ShaderKind::Material, true, Ok(()))
                .unwrap();
            ctx.gfx.record_rejected(true, true, rejected("s.wgsl"));
            let s = &ctx.gfx.shader_status()[0];
            assert_eq!((s.outcome, s.reloads), (ShaderOutcome::Reloaded, 1));
            assert!(s.diagnostics.is_empty(), "the newer code runs");
        });
    }
}
