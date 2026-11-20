//! Recording what a world hands out, to check declared system access.
//!
//! [`World::record_access`] starts a log; [`World::take_access_log`] stops it
//! and returns what was read and written in between. Recording exists only
//! in builds with debug assertions: release builds compile every check out
//! and `take_access_log` returns `None`.

use std::any::TypeId;

use crate::storage::ComponentId;
#[cfg(doc)]
use crate::world::World;

/// What a world handed out while recording: components, resources and
/// structural changes. From [`World::take_access_log`].
///
/// Each entry appears once; `true` means it was written (a write also covers reads).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AccessLog {
    /// Components fetched by queries, `get`, `get_mut` or the raw C/C++ path.
    pub components: Vec<(ComponentId, bool)>,
    /// Typed resources: type id, type name, written.
    pub resources: Vec<(TypeId, &'static str, bool)>,
    /// Resources without a Rust type, by name. Their raw pointers allow
    /// writing, so every use counts as a write.
    pub dynamic_resources: Vec<String>,
    /// Entities were spawned or despawned, components added or removed, or
    /// rows reordered while recording.
    pub structural: bool,
}

impl AccessLog {
    /// `true` if nothing was accessed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.resources.is_empty() && self.dynamic_resources.is_empty() && !self.structural
    }

    pub(crate) fn component(&mut self, id: ComponentId, write: bool) {
        match self.components.iter_mut().find(|(c, _)| *c == id) {
            Some((_, w)) => *w |= write,
            None => self.components.push((id, write)),
        }
    }

    pub(crate) fn resource(&mut self, id: TypeId, name: &'static str, write: bool) {
        match self.resources.iter_mut().find(|(r, ..)| *r == id) {
            Some((_, _, w)) => *w |= write,
            None => self.resources.push((id, name, write)),
        }
    }

    pub(crate) fn dynamic_resource(&mut self, name: &str) {
        if !self.dynamic_resources.iter().any(|n| n == name) {
            self.dynamic_resources.push(name.to_string());
        }
    }
}

/// The world's recorder: a flag checked first, so a world that isn't
/// recording pays one relaxed load per access (and nothing in release).
#[derive(Default)]
pub(crate) struct Recorder {
    #[cfg(debug_assertions)]
    on: std::sync::atomic::AtomicBool,
    #[cfg(debug_assertions)]
    log: std::sync::Mutex<AccessLog>,
}

impl Recorder {
    #[cfg(debug_assertions)]
    #[inline]
    pub(crate) fn note(&self, f: impl FnOnce(&mut AccessLog)) {
        if self.on.load(std::sync::atomic::Ordering::Relaxed) {
            f(&mut self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        }
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    pub(crate) fn note(&self, _: impl FnOnce(&mut AccessLog)) {}
}

impl Recorder {
    pub(crate) fn start(&mut self) {
        #[cfg(debug_assertions)]
        {
            *self.log.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = AccessLog::default();
            *self.on.get_mut() = true;
        }
    }

    /// [`start`](Self::start) with `log`'s buffers, emptied: no allocation
    /// until the log outgrows them.
    pub(crate) fn start_into(&mut self, #[allow(unused_mut)] mut log: AccessLog) {
        #[cfg(debug_assertions)]
        {
            log.components.clear();
            log.resources.clear();
            log.dynamic_resources.clear();
            log.structural = false;
            *self.log.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = log;
            *self.on.get_mut() = true;
        }
        #[cfg(not(debug_assertions))]
        drop(log);
    }

    /// Stops noting accesses for a while (component hooks, whose work isn't
    /// the system's that caused it); returns whether it was on, for
    /// [`resume`](Self::resume).
    #[inline]
    pub(crate) fn pause(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            self.on.swap(false, std::sync::atomic::Ordering::Relaxed)
        }
        #[cfg(not(debug_assertions))]
        false
    }

    /// Ends a [`pause`](Self::pause): `was` is what it returned.
    #[inline]
    pub(crate) fn resume(&self, #[allow(unused_variables)] was: bool) {
        #[cfg(debug_assertions)]
        if was {
            self.on.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(crate) fn stop(&mut self) -> Option<AccessLog> {
        #[cfg(debug_assertions)]
        {
            if !std::mem::replace(self.on.get_mut(), false) {
                return None;
            }
            Some(std::mem::take(
                self.log.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner),
            ))
        }
        #[cfg(not(debug_assertions))]
        None
    }
}
