//! What happened to each of the game's shaders: compiled, reloaded or
//! rejected, and where in its WGSL an error is ([`Graphics::shader_status`](crate::gfx::Graphics::shader_status)).
//! Sprite shaders use it in every build; 3D shaders with the `3d` feature.

use crate::gfx::Graphics;

/// Which kind of WGSL the game gave the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShaderKind {
    /// A material shader (`Graphics::create_shader`, `Graphics::load_shader`).
    Material,
    /// A screen effect (`Graphics::create_effect`, `Graphics::load_effect`).
    Effect,
    /// A compute pass (`Graphics::create_compute`, `Graphics::load_compute`).
    Compute,
    /// A sprite shader ([`Graphics::create_sprite_shader`](crate::gfx::Graphics::create_sprite_shader), [`Graphics::load_sprite_shader`](crate::gfx::Graphics::load_sprite_shader)).
    Sprite,
    /// A sky shader (`Graphics::create_sky_shader`, `Graphics::load_sky_shader`).
    Sky,
}

/// A compiler error in one of the game's shaders, pointing at its own WGSL
/// (not at the engine's code the shader is composed with).
///
/// Its `Display` reads like a compiler's: `water.wgsl:4:5: error: ...`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShaderDiagnostic {
    /// The shader's name: the path it was loaded from, or the name given to
    /// `create_shader`.
    pub shader: String,
    /// Material shader, screen effect, compute pass, sprite or sky shader.
    pub kind: ShaderKind,
    /// The variant the engine compiled when it failed: which optional parts
    /// (decals, global illumination, instance storage) the GPU has.
    pub variant: String,
    /// 1-based line of the error in the game's WGSL; in the composed
    /// shader's if `in_engine_code`. `None` if the error has no place.
    pub line: Option<u32>,
    /// 1-based column (in characters) on `line`.
    pub column: Option<u32>,
    /// The error is in the engine's code around the game's: usually the
    /// game's code broke it (an unclosed brace, or a `vertex` / `surface`
    /// hook with the wrong signature).
    pub in_engine_code: bool,
    /// What is wrong, with its causes.
    pub message: String,
    /// The compiler's note on the spot it points at (may be empty).
    pub label: String,
    /// More notes from the compiler.
    pub notes: Vec<String>,
    /// The line of code the error is on.
    pub source_line: Option<String>,
}

impl std::fmt::Display for ShaderDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.shader)?;
        if self.in_engine_code {
            write!(f, " (in the engine's code around it")?;
            if let Some(line) = self.line {
                write!(f, ", composed line {line}")?;
            }
            write!(f, ")")?;
        } else if let Some(line) = self.line {
            write!(f, ":{line}")?;
            if let Some(column) = self.column {
                write!(f, ":{column}")?;
            }
        }
        write!(f, ": error: {}", self.message)?;
        if let Some(code) = &self.source_line {
            write!(f, "\n    {code}")?;
            let pad = self.column.map_or(0, |c| c.saturating_sub(1) as usize);
            write!(f, "\n    {:pad$}^ {}", "", self.label)?;
        }
        for note in &self.notes {
            write!(f, "\n  = note: {note}")?;
        }
        if !self.variant.is_empty() {
            write!(f, "\n  = variant: {}", self.variant)?;
        }
        Ok(())
    }
}

/// How the last compile of a shader went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShaderOutcome {
    /// Made, and in use.
    Compiled,
    /// Its file changed and the new code is in use.
    Reloaded,
    /// It could not be made (`create_*` returned an error or panicked).
    /// With hot reload on, a shader file that doesn't compile when loaded
    /// still gets its handle: a stand-in (the built-in look for a material
    /// shader, the frame as it is for a screen effect, skipped for a
    /// compute pass) is used, and its file is watched; it stays `Failed`
    /// until a saved fix compiles, which makes it `Reloaded`.
    Failed,
    /// Its file changed, but the new code has errors: the last good
    /// version stays in use.
    ReloadFailed,
}

/// The latest result of compiling one of the game's shaders, from
/// [`Graphics::shader_status`](crate::gfx::Graphics::shader_status).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShaderStatus {
    /// Its name: the path it was loaded from, or the name given to `create_*`.
    pub shader: String,
    /// Material shader, screen effect, compute pass, sprite or sky shader.
    pub kind: ShaderKind,
    /// How it went.
    pub outcome: ShaderOutcome,
    /// How many times an edit of its file was reloaded successfully.
    pub reloads: u32,
    /// The frame ([`Time::frame`](crate::time::Time::frame)) this happened in.
    pub frame: u64,
    /// The errors of the last compile; empty when it worked.
    pub diagnostics: Vec<ShaderDiagnostic>,
}

impl ShaderStatus {
    /// Whether the code in the file is what's in use.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self.outcome, ShaderOutcome::Compiled | ShaderOutcome::Reloaded)
    }
}

impl Graphics {
    /// What happened to each of the game's shaders (sprite shaders, and
    /// with the `3d` feature material shaders, screen effects and compute
    /// passes) the last time it was compiled: one
    /// entry per name and kind, oldest first. A coding agent or tool can
    /// check here whether an edit took effect, and where an error is,
    /// without reading logs.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// for status in ctx.gfx.shader_status() {
    ///     for error in &status.diagnostics {
    ///         eprintln!("{error}"); // water.wgsl:4:5: error: ...
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn shader_status(&self) -> &[ShaderStatus] {
        &self.shader_statuses
    }

    /// [`shader_status`](Self::shader_status) as JSON, the same as the `shaders.status`
    /// control method returns (for the C API).
    #[doc(hidden)]
    #[must_use]
    pub fn shader_status_json(&self) -> String {
        crate::control::shader_status_json(self).to_string()
    }

    /// Records how compiling shader `name` went; `reload`: a changed file.
    pub(crate) fn record_shader(
        &mut self,
        name: &str,
        kind: ShaderKind,
        reload: bool,
        result: Result<(), Box<ShaderDiagnostic>>,
    ) -> Result<(), crate::error::Error> {
        let frame = self.frame;
        let list = &mut self.shader_statuses;
        let at = match list.iter().position(|s| s.shader == name && s.kind == kind) {
            Some(at) => at,
            None => {
                list.push(ShaderStatus {
                    shader: name.to_string(),
                    kind,
                    outcome: ShaderOutcome::Compiled,
                    reloads: 0,
                    frame,
                    diagnostics: Vec::new(),
                });
                list.len() - 1
            }
        };
        let status = &mut list[at];
        status.frame = frame;
        match result {
            Ok(()) => {
                status.diagnostics.clear();
                status.outcome = if reload {
                    status.reloads += 1;
                    ShaderOutcome::Reloaded
                } else {
                    ShaderOutcome::Compiled
                };
                Ok(())
            }
            Err(diagnostic) => {
                let message = diagnostic.to_string();
                // A file that never compiled has no good version to keep.
                status.outcome = if reload && status.outcome != ShaderOutcome::Failed {
                    ShaderOutcome::ReloadFailed
                } else {
                    ShaderOutcome::Failed
                };
                status.diagnostics = vec![*diagnostic];
                Err(crate::error::Error::new(crate::error::ErrorKind::Shader, message))
            }
        }
    }

    /// Whether a shader file that fails when loaded from `origin` gets a
    /// stand-in and stays watched (hot reload on, a file on disk).
    pub(crate) fn keeps_failed_load(&self, origin: &crate::assets::Origin) -> bool {
        self.watcher.on() && matches!(origin, crate::assets::Origin::File(_))
    }

    /// Says that `path` failed to compile and uses a stand-in until fixed.
    pub(crate) fn report_stand_in(path: &str, error: &crate::error::Error) {
        eprintln!("{error}");
        eprintln!("ponko: {path} uses a stand-in until it compiles; hot reload is watching it");
    }
}

#[cfg(test)]
mod message_tests {
    #[test]
    fn shader_display_keeps_file_location_and_snippet() {
        use ponko::render::{ShaderDiagnostic, ShaderKind};
        let diagnostic = ShaderDiagnostic {
            shader: "water.wgsl".into(),
            kind: ShaderKind::Sprite,
            variant: String::new(),
            line: Some(4),
            column: Some(5),
            in_engine_code: false,
            message: "unknown name".into(),
            label: "declare this name".into(),
            notes: vec![],
            source_line: Some("    bad_name".into()),
        };
        let terminal = diagnostic.to_string();
        assert!(terminal.contains("water.wgsl:4:5: error: unknown name"));
        assert!(terminal.contains("    bad_name"));
        assert!(terminal.contains("^ declare this name"));
    }
}
