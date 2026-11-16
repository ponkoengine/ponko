//! Conversion of fallible GPU startup operations; no panic-based recovery.

use crate::error::{Error, ErrorKind};

pub(super) fn result<T>(result: Result<T, impl std::fmt::Display>, context: &str) -> Result<T, Error> {
    result.map_err(|error| Error::new(ErrorKind::Unsupported, format!("{context}: {error}")))
}

pub(super) fn config<T>(config: Option<T>) -> Result<T, Error> {
    config.ok_or_else(|| Error::new(ErrorKind::Unsupported, "the GPU can't draw to this window"))
}
