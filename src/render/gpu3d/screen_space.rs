//! The screen-space passes that read the scene's depth and motion before
//! or after it is lit: temporal antialiasing (`../taa.rs`), ambient
//! occlusion with contact shadows (`../screen.rs`) and reflections
//! (`../ssr.rs`).
//!
//! [`ScreenSpace`] owns their pipelines (made the first frame each is on)
//! and which of them run this frame. `Renderer3D::prepare` decides the
//! flags from the game's `Post` settings and the device (step 1), the
//! scene's images are made for the ones that are on (`targets.rs`), and
//! recording reads them. Their histories are their own, dropped with the
//! frame's (`History::decide`).

use crate::render::screen::Ao;
use crate::render::ssr::Ssr;
use crate::render::taa::Taa;

pub(super) struct ScreenSpace {
    /// Temporal antialiasing: the resolve pass (made when first used).
    pub(super) taa: Option<Taa>,
    /// Whether this device can run it (WebGL2 can't: FXAA stands in).
    pub(super) taa_supported: bool,
    /// Whether this frame is resolved by it.
    pub(super) taa_on: bool,
    /// Whether the temporal upscaler (`Upscaler::Temporal`) stands in for
    /// the resolve this frame: `taa_on` holds too (the frame is jittered and
    /// has motion vectors), but nothing is resolved at the render size;
    /// the tone mapped frame is accumulated into a window-sized history.
    pub(super) upscale_on: bool,
    /// Whether moved models are drawn again for their motion vectors
    /// (temporal antialiasing or temporal ambient occlusion).
    pub(super) velocity_on: bool,
    /// Whether the motion pass also draws the models a shader's `vertex`
    /// moves while they hold still (see `vertex_motion.rs`).
    pub(super) still_motion: bool,
    /// Whether this frame skips the motion pass: nothing moved, and the
    /// motion image (kept from frame to frame) holds only zeros.
    pub(super) motion_kept: bool,
    /// Whether the motion image holds only zeros: set when a motion pass
    /// is recorded (by whether it drew anything), read by the next
    /// `prepare`. A new image is zeros too, but counts only once a pass
    /// cleared it.
    pub(super) motion_clear: std::sync::atomic::AtomicBool,
    /// Ambient occlusion and contact shadows (made when first used), and
    /// whether this frame has them.
    pub(super) ao: Option<Ao>,
    pub(super) ao_enabled: bool,
    /// Screen-space reflections: the passes (made when first used), and
    /// whether this frame has them.
    pub(super) ssr: Option<Ssr>,
    pub(super) ssr_on: bool,
    /// Whether this frame's reflections trace thin objects
    /// (`Post::ssr_thin_objects`, see `ssr_thin.wgsl`).
    pub(super) ssr_thin: bool,
    /// Tests: no mip bias while upscaling (the negative control).
    #[cfg(test)]
    pub(super) mip_bias_off: bool,
    /// Tests: the motion pass runs every frame, as before it was skipped
    /// (the control of the skipped pass's tests).
    #[cfg(test)]
    pub(super) motion_always: bool,
}

impl ScreenSpace {
    /// Whether the motion pass runs even when it could be skipped (tests only).
    pub(super) fn motion_pass_always(&self) -> bool {
        #[cfg(test)]
        return self.motion_always;
        #[cfg(not(test))]
        false
    }

    /// Whether the temporal upscaler's mip bias is off (tests only).
    pub(super) fn no_mip_bias(&self) -> bool {
        #[cfg(test)]
        return self.mip_bias_off;
        #[cfg(not(test))]
        false
    }

    /// `taa_supported`: whether the device has compute-class limits.
    pub(super) fn new(taa_supported: bool) -> Self {
        Self {
            taa: None,
            taa_supported,
            taa_on: false,
            upscale_on: false,
            velocity_on: false,
            still_motion: false,
            motion_kept: false,
            motion_clear: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            mip_bias_off: false,
            #[cfg(test)]
            motion_always: false,
            ao: None,
            ao_enabled: false,
            ssr: None,
            ssr_on: false,
            ssr_thin: false,
        }
    }
}
