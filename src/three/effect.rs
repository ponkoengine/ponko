//! Screen effects: a game's WGSL run over the whole lit 3D frame, before
//! bloom and tone mapping (sun shafts, underwater, heat haze, outlines).

use super::Material;
use super::target::RenderTarget;
use crate::gfx::Graphics;
use crate::handle::INDEX_MASK;

/// A screen effect shader, made with [`Graphics::load_effect`](crate::gfx::Graphics::load_effect) or
/// [`Graphics::create_effect`](crate::gfx::Graphics::create_effect); a cheap handle. Run it by adding a
/// [`ScreenEffect`] to [`RenderPasses::effects`](crate::render::RenderPasses::effects).
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let underwater = ctx.gfx.load_effect("effects/underwater.wgsl");
/// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(underwater));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoEffect`)
pub struct Effect {
    pub(crate) id: u32,
}

/// One effect in [`RenderPasses::effects`](crate::render::RenderPasses::effects): its shader, and a
/// material whose custom values and textures the shader reads
/// (`material.custom[0]` to `[3]`, `custom_texture0`, ...).
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx, tint: ponko::render::Effect) {
/// let strength = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_custom([vec4(0.5, 0.0, 0.0, 0.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO]));
/// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(tint).with_material(strength));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenEffect {
    /// The shader.
    pub effect: Effect,
    /// Where its values and textures come from. `None`: the default
    /// material (all custom values 0).
    pub material: Option<Material>,
}

impl ScreenEffect {
    /// Runs `effect` with no values of its own.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, e: ponko::render::Effect) {
    /// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(e));
    /// # }
    /// ```
    #[must_use]
    pub fn new(effect: Effect) -> Self {
        Self { effect, material: None }
    }

    /// The same, reading its values and textures from `material`; change
    /// them with [`Graphics::update_material`](crate::gfx::Graphics::update_material) to animate the effect.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(e: ponko::render::Effect, m: ponko::render::Material) {
    /// let fx = ponko::render::ScreenEffect::new(e).with_material(m);
    /// # }
    /// ```
    #[must_use]
    pub fn with_material(self, material: Material) -> Self {
        Self {
            material: Some(material),
            ..self
        }
    }
}

/// One entry of [`RenderPasses::target_effects`](crate::render::RenderPasses::target_effects): an
/// effect shader run over the lit 3D frame that writes its result into a
/// render target of its own, leaving the frame unchanged.
///
/// It reads the frame as a [`ScreenEffect`] does (`p.color`, `p.world`,
/// `p.distance`, `p.sky`, `scene_color`, `scene_position`) and writes `effect(p)`
/// into each texel of `target`. A target of another size reads the frame at
/// the matching point (the nearest pixel, no filtering): a small target is a
/// cheap, coarse copy.
///
/// **When it runs.** Once per frame, after the opaque and transparent
/// surfaces and temporal antialiasing, and before [`RenderPasses::effects`](crate::render::RenderPasses::effects),
/// bloom and tone mapping. Target effects run in list order. So in one frame:
/// - screen effects (and later target effects) that sample `target` see
///   this frame's result;
/// - materials, target cameras and compute passes of both stages
///   ([`RenderPasses::compute`](crate::render::RenderPasses::compute),
///   [`RenderPasses::scene_compute`](crate::render::RenderPasses::scene_compute)), which
///   run earlier, see the one from the frame before (all 0 at first).
///
/// **Resources.** `target` is a [`RenderTarget`](crate::render::RenderTarget), made by
/// [`Graphics::create_render_target`](crate::gfx::Graphics::create_render_target). Its whole contents are replaced each frame. The
/// game owns it and keeps it until it stops using it; the engine only writes
/// it. These entries can't run, and are skipped with a warning naming the
/// entry and the render target: one whose material samples its own `target`
/// (one pass can't read and write the same texture), and one whose `target`
/// something else wrote earlier in the frame (a compute pass, a target
/// camera or an earlier target effect), whose result it would replace.
/// Without a 3D scene nothing runs.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// // A quarter-size map of how far each pixel is, for a later effect to blur.
/// let far = ctx.gfx.create_effect("far", "fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(p.distance); }");
/// let depth = ctx.gfx.create_render_target(320, 180);
/// ctx.scene3d.passes.target_effects.push(ponko::render::TargetEffect::new(far, depth));
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetEffect {
    /// The shader, made like a screen effect's.
    pub effect: Effect,
    /// What it writes: made by [`Graphics::create_render_target`](crate::gfx::Graphics::create_render_target).
    pub target: RenderTarget,
    /// Where its values and textures come from. `None`: the default
    /// material (all custom values 0).
    pub material: Option<Material>,
}

impl TargetEffect {
    /// Runs `effect` into `target` with no values of its own.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, e: ponko::render::Effect, t: ponko::render::RenderTarget) {
    /// ctx.scene3d.passes.target_effects.push(ponko::render::TargetEffect::new(e, t));
    /// # }
    /// ```
    #[must_use]
    pub fn new(effect: Effect, target: RenderTarget) -> Self {
        Self {
            effect,
            target,
            material: None,
        }
    }

    /// The same, reading its values and textures from `material`.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(e: ponko::render::Effect, t: ponko::render::RenderTarget, m: ponko::render::Material) {
    /// let pass = ponko::render::TargetEffect::new(e, t).with_material(m);
    /// # }
    /// ```
    #[must_use]
    pub fn with_material(self, material: Material) -> Self {
        Self {
            material: Some(material),
            ..self
        }
    }
}

impl Graphics {
    /// Loads a screen effect shader (WGSL) from an [added asset
    /// source](crate::assets::Assets::add_source) or `assets/`. It defines
    /// `fn effect(p: Pixel) -> vec3<f32>`: the new light at each pixel of
    /// the 3D frame, from what's there (`p.color`, `p.world`, `p.distance`,
    /// `p.sky`, `p.screen`, `p.uv`). It can call `scene_color(screen)` and
    /// `scene_position(screen)` for other pixels, and read `frame` and
    /// `material` as material shaders do. With hot reload on, saving the
    /// file in `assets/` updates the game (an error keeps the old one). Loading the same
    /// file again returns the same effect.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let haze = ctx.gfx.load_effect("effects/haze.wgsl");
    /// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(haze));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the file can't be read or doesn't compile (the message says where).
    /// With hot reload on, a file that doesn't compile doesn't panic: it
    /// gets a stand-in and stays watched until a saved fix compiles
    /// ([`ShaderOutcome::Failed`](crate::render::ShaderOutcome::Failed), [`Graphics::shader_status`](crate::gfx::Graphics::shader_status)).
    pub fn load_effect(&mut self, path: &str) -> Effect {
        self.try_load_effect(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_effect`](Self::load_effect) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// is missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't UTF-8 text and
    /// [`Shader`](crate::error::ErrorKind::Shader) if it doesn't compile (the
    /// message has the compiler's log). With hot reload on, a file that doesn't
    /// compile still returns a stand-in, as `load_effect` does.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_effect("missing.wgsl").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_effect(&mut self, path: &str) -> Result<Effect, crate::error::Error> {
        let found = match self
            .asset_sources
            .lookup(path, |origin| self.assets3d.loaded_effects.get(origin).copied())?
        {
            crate::assets::Lookup::Cached(_, effect) => return Ok(effect),
            crate::assets::Lookup::Found(found) => found,
        };
        let origin = found.origin.clone();
        let source = found.into_text(path)?;
        let effect = match self.try_create_effect(path, &source) {
            Ok(effect) => effect,
            // With hot reload on, it shows the frame as it is until a saved
            // fix compiles; its status says what's wrong.
            Err(e) if self.keeps_failed_load(&origin) => {
                let e = e.with_path(path);
                Self::report_stand_in(path, &e);
                let id = self.assets3d.effect_ids.next("effect");
                self.compile_effect(path, super::EFFECT_STAND_IN, id, false)
                    .map_err(|_| e)?;
                let id = self.assets3d.effect_ids.make("effect");
                self.keep_shader_name(super::ShaderKind::Effect, id, path);
                self.assets3d.effects_live.insert(id);
                self.asset_ledger.made(crate::inventory::AssetClass::Effect, id);
                Effect { id }
            }
            Err(e) => return Err(e.with_path(path)),
        };
        self.assets3d.loaded_effects.insert(origin.clone(), effect);
        self.asset_ledger
            .own(crate::inventory::AssetClass::Effect, effect.id, path);
        if let crate::assets::Origin::File(file) = origin {
            self.watcher
                .watch(path, file, crate::hot_reload::Asset::Effect(effect.id));
        }
        Ok(effect)
    }

    /// Makes a screen effect shader from WGSL source; `name` is for error
    /// messages. See [`Graphics::load_effect`](crate::gfx::Graphics::load_effect) for what it defines.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let grey = ctx.gfx.create_effect("grey", "
    ///     fn effect(p: Pixel) -> vec3<f32> {
    ///         return vec3<f32>(dot(p.color, vec3<f32>(0.2126, 0.7152, 0.0722)));
    ///     }");
    /// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(grey));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If it doesn't compile (the message says where).
    pub fn create_effect(&mut self, name: &str, source: &str) -> Effect {
        self.try_create_effect(name, source).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_effect`](Self::create_effect) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Shader`](crate::error::ErrorKind::Shader) if it doesn't
    /// compile, with the compiler's log in the message.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_create_effect("broken", "fn effect(p: Pixel) -> vec3<f32> { return 1; }").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Shader);
    /// });
    /// ```
    pub fn try_create_effect(&mut self, name: &str, source: &str) -> Result<Effect, crate::error::Error> {
        let id = self.assets3d.effect_ids.next("effect");
        let compiled = self.compile_effect(name, source, id, false);
        self.record_shader(name, super::ShaderKind::Effect, false, compiled)?;
        let id = self.assets3d.effect_ids.make("effect");
        self.keep_shader_name(super::ShaderKind::Effect, id, name);
        self.assets3d.effects_live.insert(id);
        self.asset_ledger.made(crate::inventory::AssetClass::Effect, id);
        Ok(Effect { id })
    }

    /// Checks effect `source` and builds its pipeline, as `replace` or as the next one.
    fn compile_effect(
        &mut self,
        name: &str,
        source: &str,
        id: u32,
        replace: bool,
    ) -> Result<(), Box<super::ShaderDiagnostic>> {
        #[cfg(feature = "window")]
        crate::render::diagnose(
            name,
            super::ShaderKind::Effect,
            crate::render::EFFECT_VARIANT,
            &crate::render::compose_effect(source),
        )?;
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            let slot = id & INDEX_MASK;
            if replace {
                renderer.replace_effect(slot, name, source)?;
            } else {
                renderer.create_effect(slot, name, source)?;
            }
        }
        let _ = (name, source, id, replace);
        Ok(())
    }

    /// Recompiles an effect whose file changed (hot reload).
    pub(crate) fn reload_effect(&mut self, id: u32, name: &str, source: &str) -> Result<(), String> {
        if !self.assets3d.effects_live.contains(id) {
            return Ok(());
        }
        let compiled = self.compile_effect(name, source, id, true);
        Ok(self.record_shader(name, super::ShaderKind::Effect, true, compiled)?)
    }

    /// Frees screen effect `id` (see [`Ctx::free`](crate::ctx::Ctx::free)): entries of
    /// [`RenderPasses::effects`](crate::render::RenderPasses::effects) showing it are skipped from now on.
    pub(crate) fn free_effect_id(&mut self, id: u32) {
        if !self.assets3d.effects_live.remove(id) {
            return;
        }
        self.assets3d.effect_ids.release(id);
        self.forget_shader_name(super::ShaderKind::Effect, id);
        self.asset_ledger.freed(crate::inventory::AssetClass::Effect, id);
        self.assets3d.loaded_effects.retain(|_, e| e.id != id);
        self.watcher
            .forget(|a| matches!(a, crate::hot_reload::Asset::Effect(e) if *e == id));
        if let Some(renderer) = self.renderer.as_mut()
            && !self.headless
        {
            renderer.free_effect(id & INDEX_MASK);
        }
    }
}
