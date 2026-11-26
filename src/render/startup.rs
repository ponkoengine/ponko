//! Conversion of fallible GPU startup operations; no panic-based recovery.
//!
//! Each step that can fail names what failed, the GPU's own words, and what
//! to try: the first thing a player or a developer on a new machine reads.

use crate::error::{Error, ErrorKind};

/// The step of opening the GPU that failed.
#[derive(Clone, Copy, Debug)]
pub(super) enum Step {
    /// The window refused a drawing surface.
    Surface,
    /// No adapter can draw to the window's surface.
    Adapter,
    /// The adapter refused to open a device.
    Device,
}

impl Step {
    /// What failed, and what to try, for the platform this is built for.
    fn message(self, detail: &str) -> String {
        let web = cfg!(target_arch = "wasm32");
        match self {
            Step::Surface if web => format!(
                "could not create a GPU surface ({detail}): the page's canvas has no WebGPU or WebGL2 context; \
                 use a current browser with hardware acceleration turned on"
            ),
            Step::Surface => format!(
                "could not create a GPU surface ({detail}): the window was refused a drawing surface; \
                 update the graphics driver, and on Linux check that a Wayland or X11 session is running"
            ),
            Step::Adapter if web => format!(
                "no compatible GPU found ({detail}): this browser offers neither WebGPU nor WebGL2 here; \
                 use a current browser with hardware acceleration turned on"
            ),
            Step::Adapter => format!(
                "no compatible GPU found ({detail}): install or update the graphics driver (Vulkan on Linux, \
                 Direct3D 12 or Vulkan on Windows, Metal on macOS), turn on GPU acceleration in a virtual machine, \
                 or on a machine without a GPU use `Game::run_headless`; `WGPU_BACKEND` (`vulkan`, `dx12`, \
                 `metal` or `gl`) picks the backend to try"
            ),
            Step::Device => format!(
                "could not open the GPU device ({detail}): the GPU lacks a limit or feature ponko needs, or its \
                 driver failed; update the graphics driver, and try another adapter or backend (`WGPU_BACKEND`)"
            ),
        }
    }
}

pub(super) fn result<T>(result: Result<T, impl std::fmt::Display>, step: Step) -> Result<T, Error> {
    result.map_err(|error| Error::new(ErrorKind::Unsupported, step.message(&error.to_string())))
}

pub(super) fn config<T>(config: Option<T>) -> Result<T, Error> {
    config.ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            "the GPU can't draw to this window: its surface offers no format this adapter can show; \
             update the graphics driver, or try another backend (`WGPU_BACKEND`)",
        )
    })
}
