//! Making the game's sky shaders ([`SkyShader`]): from WGSL source or a
//! file, checked against the sky hook's contract, hot reloaded.

use super::SkyShader;
use crate::gfx::Graphics;

/// What a sky shader file that doesn't compile shows until a saved fix
/// does (hot reload): a plain grey-blue sky.
const SKY_STAND_IN: &str = "fn sky(dir: vec3<f32>) -> vec3<f32> { return vec3<f32>(0.35, 0.42, 0.5); }";

impl Graphics {
    /// Loads a sky shader (WGSL) from an [added asset
    /// source](crate::assets::Assets::add_source) or `assets/`. It defines
    /// `fn sky(dir: vec3<f32>) -> vec3<f32>`, the sky's light in each
    /// direction; set it as [`Sky::shader`](crate::render::Sky::shader) to
    /// show it. See [`SkyShader`] for what it can read. With hot reload on,
    /// saving the file in `assets/` updates the game (an error keeps the
    /// old one). Loading the same file again returns the same shader.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.sky.shader = Some(ctx.gfx.load_sky_shader("shaders/cartoon_sky.wgsl"));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the file can't be read or doesn't compile (the message says where).
    /// With hot reload on, a file that doesn't compile doesn't panic: it
    /// gets a stand-in and stays watched until a saved fix compiles
    /// ([`ShaderOutcome::Failed`](crate::render::ShaderOutcome::Failed), [`Graphics::shader_status`]).
    pub fn load_sky_shader(&mut self, path: &str) -> SkyShader {
        self.try_load_sky_shader(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_sky_shader`](Self::load_sky_shader) with errors as values:
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file is
    /// missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't UTF-8 text and
    /// [`Shader`](crate::error::ErrorKind::Shader) if it doesn't compile (the
    /// message has the compiler's log). With hot reload on, a file that
    /// doesn't compile still returns a stand-in, as `load_sky_shader` does.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_sky_shader("missing.wgsl").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_sky_shader(&mut self, path: &str) -> Result<SkyShader, crate::error::Error> {
        let found = match self
            .asset_sources
            .lookup(path, |origin| self.assets3d.loaded_sky_shaders.get(origin).copied())?
        {
            crate::assets::Lookup::Cached(_, shader) => return Ok(shader),
            crate::assets::Lookup::Found(found) => found,
        };
        let origin = found.origin.clone();
        let source = found.into_text(path)?;
        let shader = match self.try_create_sky_shader(path, &source) {
            Ok(shader) => shader,
            // With hot reload on, it shows a plain sky until a saved fix
            // compiles; its status says what's wrong.
            Err(e) if self.keeps_failed_load(&origin) => {
                let e = e.with_path(path);
                Self::report_stand_in(path, &e);
                self.compile_sky_shader(path, SKY_STAND_IN, None).map_err(|_| e)?;
                let shader = SkyShader {
                    id: self.assets3d.sky_shaders,
                };
                self.assets3d.sky_shaders += 1;
                shader
            }
            Err(e) => return Err(e.with_path(path)),
        };
        self.assets3d.loaded_sky_shaders.insert(origin.clone(), shader);
        if let crate::assets::Origin::File(file) = origin {
            self.watcher
                .watch(path, file, crate::hot_reload::Asset::SkyShader(shader.id));
        }
        Ok(shader)
    }

    /// Makes a sky shader from WGSL source; `name` is for error messages.
    /// See [`SkyShader`] for what it defines and can read.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let dusk = ctx.gfx.create_sky_shader("dusk", "
    ///     fn sky(dir: vec3<f32>) -> vec3<f32> {
    ///         return mix(vec3<f32>(1.0, 0.4, 0.2), vec3<f32>(0.1, 0.1, 0.4), clamp(dir.y * 2.0, 0.0, 1.0));
    ///     }");
    /// ctx.scene3d.sky.shader = Some(dusk);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If it doesn't compile (the message says where).
    pub fn create_sky_shader(&mut self, name: &str, source: &str) -> SkyShader {
        self.try_create_sky_shader(name, source)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_sky_shader`](Self::create_sky_shader) with errors as values:
    /// [`ErrorKind::Shader`](crate::error::ErrorKind::Shader) if it doesn't
    /// compile or has no `sky` function, with the reason in the message.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_create_sky_shader("no hook", "fn sky_color() -> f32 { return 1.0; }").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Shader);
    /// });
    /// ```
    pub fn try_create_sky_shader(&mut self, name: &str, source: &str) -> Result<SkyShader, crate::error::Error> {
        let compiled = self.compile_sky_shader(name, source, None);
        self.record_shader(name, crate::render::ShaderKind::Sky, false, compiled)?;
        let shader = SkyShader {
            id: self.assets3d.sky_shaders,
        };
        self.assets3d.sky_shaders += 1;
        Ok(shader)
    }

    /// Checks sky shader `source` and builds its pipelines, as `replace` or
    /// as the next one. Without a GPU (headless) only the contract is checked.
    fn compile_sky_shader(
        &mut self,
        name: &str,
        source: &str,
        replace: Option<u32>,
    ) -> Result<(), Box<crate::render::ShaderDiagnostic>> {
        #[cfg(feature = "window")]
        crate::render::diagnose(
            name,
            crate::render::ShaderKind::Sky,
            crate::render::SKY_VARIANT,
            &crate::render::compose_sky(source),
        )?;
        #[cfg(not(feature = "window"))]
        let _ = (name, source);
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            match replace {
                Some(id) => renderer.replace_sky_shader(id, name, source)?,
                None => {
                    let id = renderer.create_sky_shader(name, source)?;
                    debug_assert_eq!(id, self.assets3d.sky_shaders);
                }
            }
        }
        let _ = replace;
        Ok(())
    }

    /// Recompiles a sky shader whose file changed (hot reload). On an error
    /// the last good version stays in use.
    pub(crate) fn reload_sky_shader(&mut self, id: u32, name: &str, source: &str) -> Result<(), String> {
        if id >= self.assets3d.sky_shaders {
            return Ok(());
        }
        let compiled = self.compile_sky_shader(name, source, Some(id));
        Ok(self.record_shader(name, crate::render::ShaderKind::Sky, true, compiled)?)
    }
}
