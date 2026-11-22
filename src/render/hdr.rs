//! HDR display output: the settings a game picks ([`HdrOutput`]), what the
//! window does with them ([`HdrStatus`]) and the brightness math they share.
//!
//! The GPU side (choosing the surface's format and color space, the pass
//! that encodes the frame for the display) is in `hdr_output.rs`, behind the
//! `window` feature. Everything here also exists headless, so a settings
//! menu reads and writes the same values with or without a window.

use crate::gfx::Graphics;

/// Paper white when the game sets none: the brightness, in nits, of SDR
/// white (UI, sprites, a white diffuse surface) on an HDR display.
pub(crate) const DEFAULT_PAPER_WHITE_NITS: f32 = 200.0;
/// The peak assumed when neither the game nor the display names one.
pub(crate) const DEFAULT_PEAK_NITS: f32 = 1000.0;
/// The brightest a PQ signal can say, and so the most any peak can be.
pub(crate) const MAX_NITS: f32 = 10_000.0;
/// The brightness of `1.0` in scRGB (IEC 61966-2-2): 80 nits.
pub(crate) const SCRGB_NITS: f32 = 80.0;

/// HDR display output: brighter highlights on an HDR display, with SDR
/// white (the UI, sprites) at a comfortable paper white.
///
/// **Off by default**, and then nothing changes: the frame is drawn exactly
/// as before. Turn it on with
/// [`Game::hdr_output`](crate::game::Game::hdr_output) before the window
/// opens, or at any time with `ctx.gfx.set_hdr_output` (a player's settings
/// menu): the window then picks an HDR format and color space where the
/// display system offers one (see [`HdrStatus`]). While the game runs,
/// `set_hdr_output` also changes the paper white, the peak and the HDR10
/// metadata, and switches back to SDR output, without a restart.
///
/// With HDR on, everything is drawn into a floating-point image where 1.0 is
/// SDR white, and one extra full-screen pass encodes it for the display.
/// The 3D frame's tone mapping keeps its look up to near white and opens its
/// highlights up to the peak; sprites and UI stay at SDR white.
/// ```
/// use ponko::render::HdrOutput;
/// let off = HdrOutput::default();
/// assert!(!off.enabled);
/// assert_eq!((off.paper_white_nits, off.peak_nits), (200.0, 0.0));
/// let mut hdr = HdrOutput::on();
/// hdr.paper_white_nits = 250.0; // a bright room
/// assert!(hdr.enabled);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HdrOutput {
    /// Output in HDR where the display system offers it. Default false.
    pub enabled: bool,
    /// How bright SDR white is, in nits: the UI, sprites, and the 3D
    /// frame's white before its highlights. 80 to 400 is usual; default
    /// 200. Where the display system sets SDR white itself (macOS, browsers)
    /// it is the system's and this only scales [`peak_nits`](Self::peak_nits).
    /// A value that isn't finite and positive counts as the default.
    pub paper_white_nits: f32,
    /// The brightest highlight, in nits. 0 (the default) asks the display:
    /// its reported peak, or 1000 nits where it reports none. Clamped to
    /// paper white at least and 10 000 at most.
    pub peak_nits: f32,
    /// HDR10 metadata: the brightest pixel the game shows (MaxCLL), in
    /// nits. 0 (the default) is the peak in use. Sent to the display where
    /// the platform takes metadata ([`HdrStatus::metadata`]).
    pub max_content_light_nits: f32,
    /// HDR10 metadata: the brightest a whole frame is on average (MaxFALL),
    /// in nits. 0 (the default) is paper white.
    pub max_frame_average_nits: f32,
}

impl Default for HdrOutput {
    fn default() -> Self {
        Self {
            enabled: false,
            paper_white_nits: DEFAULT_PAPER_WHITE_NITS,
            peak_nits: 0.0,
            max_content_light_nits: 0.0,
            max_frame_average_nits: 0.0,
        }
    }
}

impl HdrOutput {
    /// HDR on, with the default paper white and the display's peak.
    /// ```
    /// assert!(ponko::render::HdrOutput::on().enabled);
    /// ```
    #[must_use]
    pub fn on() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// Paper white as used: [`paper_white_nits`](Self::paper_white_nits),
    /// or the default where it isn't finite and positive, at most 10 000.
    /// ```
    /// let mut hdr = ponko::render::HdrOutput::on();
    /// hdr.paper_white_nits = f32::NAN;
    /// assert_eq!(hdr.paper_white(), 200.0);
    /// ```
    #[must_use]
    pub fn paper_white(&self) -> f32 {
        if self.paper_white_nits.is_finite() && self.paper_white_nits > 0.0 {
            self.paper_white_nits.min(MAX_NITS)
        } else {
            DEFAULT_PAPER_WHITE_NITS
        }
    }
}

/// How the window's frames reach the display ([`HdrStatus::encoding`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
#[repr(u32)]
pub enum HdrEncoding {
    /// Standard dynamic range: sRGB, white is the brightest.
    #[default]
    Sdr = 0,
    /// scRGB: linear BT.709 in a 16-bit float image, 1.0 is SDR white
    /// (80 nits on Windows; the system's SDR white on macOS, where it is
    /// Apple's extended dynamic range).
    ScRgb = 1,
    /// Extended-range sRGB: the sRGB curve continued above 1.0, in a 16-bit
    /// float image (browsers' HDR canvas).
    ExtendedSrgb = 2,
    /// HDR10's PQ signal (SMPTE ST 2084) with BT.2020 primaries, in a 10-bit
    /// image: absolute brightness up to 10 000 nits.
    Pq = 3,
}

/// HDR10 static metadata (SMPTE ST 2086 and CTA-861.3) as the window sends
/// it to the display ([`HdrStatus::metadata`]). A display tone maps PQ
/// output with it: it learns the brightest the game ever gets, so it
/// needn't guess.
///
/// Made from [`HdrOutput`]: the mastering display's peak is the peak in
/// use, its black 0, MaxCLL [`HdrOutput::max_content_light_nits`] (or the
/// peak), MaxFALL [`HdrOutput::max_frame_average_nits`] (or paper white),
/// each at most the mastering peak. The primaries are the encoding's:
/// BT.2020 for PQ, BT.709 for scRGB; white is D65.
/// ```
/// use ponko::render::{HdrMetadata, HdrOutput};
/// let mut hdr = HdrOutput::on();
/// hdr.peak_nits = 1000.0;
/// let m = HdrMetadata::for_output(&hdr, 1000.0);
/// assert_eq!((m.max_mastering_nits, m.max_content_light_nits, m.max_frame_average_nits), (1000.0, 1000.0, 200.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HdrMetadata {
    /// The mastering display's black, in nits.
    pub min_mastering_nits: f32,
    /// The mastering display's peak, in nits: the peak in use.
    pub max_mastering_nits: f32,
    /// MaxCLL: the brightest pixel, in nits.
    pub max_content_light_nits: f32,
    /// MaxFALL: the brightest frame average, in nits.
    pub max_frame_average_nits: f32,
}

impl HdrMetadata {
    /// The metadata for `settings` with the peak at `peak_nits` (the
    /// peak in use, [`HdrStatus::peak_nits`]).
    #[must_use]
    pub fn for_output(settings: &HdrOutput, peak_nits: f32) -> Self {
        let peak = if peak_nits.is_finite() && peak_nits > 0.0 {
            peak_nits.min(MAX_NITS)
        } else {
            DEFAULT_PEAK_NITS
        };
        let or = |asked: f32, default: f32| {
            if asked.is_finite() && asked > 0.0 {
                asked.min(peak)
            } else {
                default.min(peak)
            }
        };
        Self {
            min_mastering_nits: 0.0,
            max_mastering_nits: peak,
            max_content_light_nits: or(settings.max_content_light_nits, peak),
            max_frame_average_nits: or(settings.max_frame_average_nits, settings.paper_white()),
        }
    }
}

/// What the window does with [`HdrOutput`] (`ctx.gfx.hdr_status()`).
/// ```
/// use ponko::prelude::*;
/// Game::new("hdr", 64, 64).run_headless(1, |ctx| {
///     let status = ctx.gfx.hdr_status();
///     // Headless there is no display: SDR, with the reason.
///     assert!(!status.active && !status.available);
///     assert_eq!(status.encoding, ponko::render::HdrEncoding::Sdr);
///     assert!(status.reason.is_some());
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HdrStatus {
    /// The window's display system offers an HDR color space this renderer
    /// can encode (whether or not HDR is on). It says nothing of the
    /// display itself: Windows and macOS offer one even on an SDR display,
    /// and tone map down to it.
    pub available: bool,
    /// Frames go out in HDR now.
    pub active: bool,
    /// How frames go out.
    pub encoding: HdrEncoding,
    /// Paper white as used, in nits.
    pub paper_white_nits: f32,
    /// The brightest highlight as used, in nits.
    pub peak_nits: f32,
    /// The brightest output over SDR white (`peak / paper white`, 1 in SDR):
    /// how far the 3D frame's highlights reach.
    pub headroom: f32,
    /// The display's peak as the system reports it, where it does (Windows).
    pub display_peak_nits: Option<f32>,
    /// The display's headroom over SDR white right now, where the system
    /// reports it (macOS, or Windows from its nits).
    pub display_headroom: Option<f32>,
    /// Why HDR isn't active, when it isn't.
    pub reason: Option<HdrOffReason>,
    /// The HDR10 metadata the display was given, where the platform takes
    /// it: Windows (DXGI, scRGB and PQ) and Apple's PQ layers. `None` in
    /// SDR and where there is no API for it (Vulkan through wgpu, browsers,
    /// macOS's scRGB, which is relative to the system's SDR white).
    pub metadata: Option<HdrMetadata>,
}

impl HdrStatus {
    /// SDR output, for `reason`.
    pub(crate) fn sdr(available: bool, reason: HdrOffReason) -> Self {
        Self {
            available,
            active: false,
            encoding: HdrEncoding::Sdr,
            paper_white_nits: DEFAULT_PAPER_WHITE_NITS,
            peak_nits: DEFAULT_PAPER_WHITE_NITS,
            headroom: 1.0,
            display_peak_nits: None,
            display_headroom: None,
            reason: Some(reason),
            metadata: None,
        }
    }
}

/// Why HDR isn't active ([`HdrStatus::reason`]).
/// ```
/// use ponko::render::HdrOffReason;
/// assert!(HdrOffReason::Off.describe().contains("HdrOutput::enabled"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[repr(u32)]
pub enum HdrOffReason {
    /// No window: headless runs draw nothing.
    Headless = 1,
    /// HDR output isn't turned on and never was since the window opened.
    /// It isn't reported any more: [`HdrOutput::enabled`] false is
    /// [`Off`](Self::Off), and turning it on while the game runs takes
    /// effect from the next frame. Kept so code matching on it builds.
    NotRequested = 2,
    /// [`HdrOutput::enabled`] is false.
    Off = 3,
    /// The window's display system offers no HDR color space the renderer
    /// can encode: WebGL2 and OpenGL never do, Vulkan only where the driver
    /// exposes one, a browser only with an HDR canvas.
    Unsupported = 4,
}

impl HdrOffReason {
    /// The reason in words.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Headless => "no window: headless runs draw nothing",
            Self::NotRequested => "HDR output was not turned on (Game::hdr_output or Graphics::set_hdr_output)",
            Self::Off => "HDR output is turned off (HdrOutput::enabled)",
            Self::Unsupported => {
                "the window's display system offers no HDR color space this renderer can encode \
                 (WebGL2 and OpenGL never do; Vulkan only where the driver exposes one)"
            }
        }
    }
}

impl std::fmt::Display for HdrOffReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.describe())
    }
}

/// What the display reports, as `hdr_output.rs` reads it from wgpu.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct DisplayReport {
    /// The panel's peak in nits (Windows).
    pub peak_nits: Option<f32>,
    /// Headroom over SDR white now (macOS directly, Windows from nits).
    pub headroom: Option<f32>,
}

/// Paper white, peak and headroom (`peak / paper white`) for `settings` on
/// a display reporting `display`. `relative`: 1.0 is the system's SDR white
/// (Apple, browsers), so a reported headroom is over paper white; otherwise
/// a reported peak in nits counts first.
pub(crate) fn brightness(settings: &HdrOutput, relative: bool, display: DisplayReport) -> (f32, f32, f32) {
    let paper = settings.paper_white();
    let asked = settings.peak_nits;
    let peak = if asked.is_finite() && asked > 0.0 {
        asked
    } else if let (false, Some(nits)) = (relative, display.peak_nits.filter(|n| n.is_finite() && *n > 0.0)) {
        nits
    } else if let Some(h) = display.headroom.filter(|h| h.is_finite() && *h > 0.0) {
        paper * h
    } else {
        DEFAULT_PEAK_NITS
    };
    let peak = peak.clamp(paper, MAX_NITS.max(paper));
    (paper, peak, peak / paper)
}

/// Linear BT.709 (1.0 = paper white) to PQ, as the output pass encodes it:
/// to BT.2020 primaries, scaled to nits, through SMPTE ST 2084's curve.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn pq_encode(rgb: [f32; 3], paper_white: f32) -> [f32; 3] {
    let m = BT709_TO_BT2020;
    let c2020 = [0, 1, 2].map(|r| m[r][0] * rgb[0] + m[r][1] * rgb[1] + m[r][2] * rgb[2]);
    c2020.map(|c| pq_curve(c.max(0.0) * paper_white / MAX_NITS))
}

/// SMPTE ST 2084's inverse EOTF: luminance over 10 000 nits to signal.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn pq_curve(l: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 4096.0 * 128.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 4096.0 * 32.0;
    const C3: f32 = 2392.0 / 4096.0 * 32.0;
    let y = l.max(0.0).powf(M1);
    ((C1 + C2 * y) / (1.0 + C3 * y)).powf(M2)
}

/// BT.709 to BT.2020 primaries (ITU-R BT.2087), rows by output channel.
pub(crate) const BT709_TO_BT2020: [[f32; 3]; 3] = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];

/// Where the tone mapped 3D frame stops following the SDR curve and its
/// highlights open up toward the peak (`post.wgsl`'s `expand_highlights`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const HIGHLIGHT_KNEE: f32 = 0.8;

/// The HDR form of a tone mapped color (each channel in 0..=1): below the
/// knee unchanged, above it the brightest channel opened up to `headroom`
/// with the channels' ratios kept, meeting the SDR curve with the same
/// slope at the knee. A CPU twin of `post.wgsl`'s `expand_highlights`, for
/// tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn expand_highlights(c: [f32; 3], headroom: f32) -> [f32; 3] {
    let m = c[0].max(c[1]).max(c[2]);
    let k = HIGHLIGHT_KNEE;
    if m <= k || headroom <= 1.0 {
        return c;
    }
    let t = (m.min(1.0) - k) / (1.0 - k);
    let r = (1.0 - k) / (headroom - k);
    let g = r * t + (1.0 - r) * t * t;
    let expanded = k + (headroom - k) * g;
    c.map(|x| x * expanded / m)
}

impl Graphics {
    /// The HDR output settings ([`HdrOutput`]): what
    /// [`Game::hdr_output`](crate::game::Game::hdr_output) or
    /// [`set_hdr_output`](Self::set_hdr_output) set last.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("hdr", 64, 64).run_headless(1, |ctx| {
    ///     assert!(!ctx.gfx.hdr_output().enabled);
    /// });
    /// ```
    #[must_use]
    pub fn hdr_output(&self) -> crate::render::HdrOutput {
        self.hdr_output
    }

    /// Changes the HDR output settings from the next frame: paper white,
    /// peak and HDR10 metadata, and `enabled` switches the window between
    /// HDR and SDR output while the game runs, with no restart and nothing
    /// of the game made again. The first switch to HDR of a window opened
    /// in SDR moves the frame into the HDR frame image (the renderer's
    /// pipelines for the window's format are made again, once); from then
    /// on switching costs a surface configuration.
    /// [`hdr_status`](Self::hdr_status) says what the window does.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("hdr", 64, 64).run_headless(1, |ctx| {
    ///     let mut hdr = ctx.gfx.hdr_output();
    ///     hdr.paper_white_nits = 300.0;
    ///     ctx.gfx.set_hdr_output(hdr);
    ///     assert_eq!(ctx.gfx.hdr_output().paper_white_nits, 300.0);
    /// });
    /// ```
    pub fn set_hdr_output(&mut self, settings: crate::render::HdrOutput) {
        self.hdr_output = settings;
        #[cfg(feature = "window")]
        if let Some(renderer) = &mut self.renderer {
            renderer.set_hdr_output(settings);
        }
    }

    /// What the window does with the HDR settings: whether HDR is active,
    /// the encoding, the paper white and peak in use, what the display
    /// reports, and why HDR is off when it is. See [`HdrStatus`].
    #[must_use]
    pub fn hdr_status(&self) -> crate::render::HdrStatus {
        #[cfg(feature = "window")]
        if let Some(renderer) = &self.renderer {
            return renderer.hdr_status();
        }
        HdrStatus::sdr(false, HdrOffReason::Headless)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_output_is_off_by_default() {
        let hdr = HdrOutput::default();
        assert!(!hdr.enabled);
        assert_eq!(hdr.paper_white(), 200.0);
    }

    #[test]
    fn pq_matches_st2084_reference_values() {
        // SMPTE ST 2084 / ITU-R BT.2100 reference points.
        for (nits, signal) in [
            (0.0, 0.0),
            (100.0, 0.508_078),
            (203.0, 0.580_690),
            (1000.0, 0.751_827),
            (10_000.0, 1.0),
        ] {
            let got = pq_curve(nits / MAX_NITS);
            assert!((got - signal).abs() < 1e-4, "{nits} nits: {got} (want {signal})");
        }
    }

    #[test]
    fn pq_encoding_puts_paper_white_at_its_nits_for_grey() {
        // Equal BT.709 channels stay equal in BT.2020 (both D65 white).
        let [r, g, b] = pq_encode([1.0; 3], 203.0);
        for c in [r, g, b] {
            assert!((c - 0.580_690).abs() < 2e-4, "{c}");
        }
        // A saturated BT.709 red lands inside BT.2020: all channels positive.
        let red = pq_encode([1.0, 0.0, 0.0], 100.0);
        assert!(red[0] > red[1] && red[1] > 0.0 && red[2] > 0.0, "{red:?}");
    }

    #[test]
    fn bt709_to_bt2020_keeps_white() {
        for row in BT709_TO_BT2020 {
            assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{row:?}");
        }
    }

    #[test]
    fn brightness_prefers_the_games_peak_then_the_displays() {
        let mut hdr = HdrOutput::on();
        let none = DisplayReport::default();
        assert_eq!(
            brightness(&hdr, false, none),
            (200.0, 1000.0, 5.0),
            "no report: 1000 nits"
        );
        let windows = DisplayReport {
            peak_nits: Some(600.0),
            headroom: Some(2.0),
        };
        assert_eq!(
            brightness(&hdr, false, windows),
            (200.0, 600.0, 3.0),
            "absolute: the panel's nits"
        );
        let mac = DisplayReport {
            peak_nits: None,
            headroom: Some(4.0),
        };
        assert_eq!(
            brightness(&hdr, true, mac),
            (200.0, 800.0, 4.0),
            "relative: the headroom"
        );
        hdr.peak_nits = 1500.0;
        assert_eq!(
            brightness(&hdr, false, windows),
            (200.0, 1500.0, 7.5),
            "the game's peak wins"
        );
        hdr.peak_nits = 50.0;
        assert_eq!(brightness(&hdr, false, windows).2, 1.0, "never below paper white");
        hdr.peak_nits = 1e9;
        assert_eq!(brightness(&hdr, false, windows).1, MAX_NITS, "never above PQ's 10 000");
    }

    #[test]
    fn highlight_expansion_keeps_sdr_below_the_knee_and_reaches_the_peak() {
        for headroom in [1.0, 2.0, 5.0, 12.5] {
            let mid = [0.5, 0.3, 0.1];
            assert_eq!(expand_highlights(mid, headroom), mid, "below the knee: unchanged");
            let top = expand_highlights([1.0, 0.5, 0.25], headroom);
            assert!((top[0] - headroom).abs() < 1e-5, "white reaches the peak: {top:?}");
            assert!((top[1] / top[0] - 0.5).abs() < 1e-6, "ratios kept: {top:?}");
            // Monotonic, and continuous in value and slope at the knee.
            let mut last = 0.0;
            for i in 0..=1000 {
                let v = expand_highlights([i as f32 / 1000.0; 3], headroom)[0];
                assert!(v >= last, "monotonic at {i}");
                last = v;
            }
            let k = HIGHLIGHT_KNEE;
            // A one-sided difference: the curve's second-order term adds
            // about (headroom - knee) * step / 0.04 to it.
            let e = 1e-4;
            let slope = (expand_highlights([k + e; 3], headroom)[0] - k) / e;
            assert!((slope - 1.0).abs() < 0.05, "slope {slope} at the knee");
        }
    }
}
