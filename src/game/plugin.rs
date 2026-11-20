//! What plugins add to a game beyond its systems, kept on the builder until
//! the game starts: the plugins' names (to find duplicates and missing
//! dependencies), debug toggles and control endpoint methods.

use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};

/// The builder's plugin bookkeeping.
#[derive(Default)]
pub(super) struct Extensions {
    /// Startup errors queued at the front of the setups (a duplicate
    /// plugin, a missing dependency, a bad toggle or method name), so the
    /// game fails before any setup runs, with the first one.
    pub(super) errors: usize,
    /// From `debug_toggle`, in the order added.
    pub(super) toggles: Vec<(String, bool)>,
    /// From `control_method`, and the plugins' names.
    pub(super) added: crate::control::GameExtensions,
}

impl Extensions {
    /// Whether plugin `name` was added.
    pub(super) fn has_plugin(&self, name: &str) -> bool {
        self.added.plugins.iter().any(|p| p == name)
    }

    /// Why plugin `name` can't be added now, if it can't: it was added
    /// already, or a dependency wasn't added before it.
    pub(super) fn check_plugin(&self, name: &str, dependencies: &[&str]) -> Result<(), Error> {
        if self.has_plugin(name) {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "plugin `{name}` was added twice; add each plugin once (a plugin type added several times gives each instance its own `Plugin::name`)"
                ),
            ));
        }
        if let Some(missing) = dependencies.iter().find(|d| !self.has_plugin(d)) {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "plugin `{name}` needs plugin `{missing}`, which wasn't added before it: add `{missing}` first"
                ),
            ));
        }
        Ok(())
    }

    /// Why debug toggle `name` can't be added, if it can't.
    pub(super) fn check_toggle(&self, name: &str) -> Result<(), Error> {
        let invalid = |why: String| Err(Error::new(ErrorKind::Invalid, why));
        if name.is_empty() {
            return invalid("a debug toggle needs a name".into());
        }
        if self.toggles.iter().any(|(n, _)| n == name) {
            return invalid(format!("debug toggle `{name}` was added twice; add each toggle once"));
        }
        if self.toggles.len() >= crate::debug::MAX_TOGGLES {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "debug toggle `{name}`: a game has at most {} debug toggles",
                    crate::debug::MAX_TOGGLES
                ),
            ));
        }
        Ok(())
    }

    /// Moves what was added into the context of a starting game.
    pub(super) fn install(&mut self, ctx: &mut Ctx) {
        ctx.debug.set_toggles(&self.toggles);
        ctx.extensions = std::mem::take(&mut self.added);
    }
}
