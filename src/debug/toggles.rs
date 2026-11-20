//! Named debug toggles a game or plugin adds with
//! [`Game::debug_toggle`](crate::game::Game::debug_toggle): switches for
//! its own debug drawing, which tools and keys turn on and off like the
//! built-in views.

use std::sync::Arc;

/// At most this many toggles: their states are the bits of a `u64`.
pub(crate) const MAX_TOGGLES: usize = 64;

/// The toggles' names (fixed once the game starts, shared by the view
/// systems' buffers without copying) and which are on.
#[derive(Debug, Clone, Default)]
pub(super) struct Toggles {
    names: Arc<[Box<str>]>,
    on: u64,
}

impl Toggles {
    /// Toggles `names`, each on or off.
    pub(super) fn new(toggles: &[(String, bool)]) -> Self {
        let mut on = 0;
        for (i, (_, state)) in toggles.iter().enumerate().take(MAX_TOGGLES) {
            if *state {
                on |= 1 << i;
            }
        }
        Self {
            names: toggles
                .iter()
                .take(MAX_TOGGLES)
                .map(|(n, _)| n.as_str().into())
                .collect(),
            on,
        }
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| &**n == name)
    }

    /// Whether toggle `name` is on (`false` for an unknown name).
    pub(super) fn get(&self, name: &str) -> bool {
        self.index(name).is_some_and(|i| self.on & (1 << i) != 0)
    }

    /// Turns toggle `name` on or off; `false` if there is none of that name.
    pub(super) fn set(&mut self, name: &str, state: bool) -> bool {
        let Some(i) = self.index(name) else {
            return false;
        };
        if state {
            self.on |= 1 << i;
        } else {
            self.on &= !(1 << i);
        }
        true
    }

    /// Every toggle and whether it is on, in the order added.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, bool)> + '_ {
        self.names
            .iter()
            .enumerate()
            .map(|(i, n)| (&**n, self.on & (1 << i) != 0))
    }

    /// The names, comma-separated, for error messages.
    pub(super) fn names(&self) -> String {
        let names: Vec<&str> = self.names.iter().map(|n| &**n).collect();
        if names.is_empty() {
            "none were added".into()
        } else {
            names.join(", ")
        }
    }

    /// Takes the states of `main`, sharing its names.
    pub(super) fn follow(&mut self, main: &Toggles) {
        if !Arc::ptr_eq(&self.names, &main.names) {
            self.names = Arc::clone(&main.names);
        }
        self.on = main.on;
    }
}
