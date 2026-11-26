//! [`Color`].
//!
//! **Maturity: proven**, from the guide chapter `drawing.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows. Tests: the 2D render tests
//! (`draw_order_render_test.rs`, `views2d_render_test.rs`, `atlas_render_test.rs`, ...). Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics: the
//! packaged games run in a window on Linux (software Vulkan under Xvfb), macOS and Windows (DirectX
//! 12, WARP when there is no GPU); Coin Dash also runs in headless Chromium and WebKit on WebGL2;
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found.

/// An RGBA color. Components are 0.0 to 1.0, in sRGB (the values you'd pick
/// in a paint program or see in CSS).
///
/// ```
/// # use ponko::prelude::*;
/// let sky = Color::rgb(0.4, 0.7, 1.0);
/// let ghost = Color::WHITE.with_alpha(0.5);
/// let orange = Color::hex(0xFF8800);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)] // shared with C/C++ (`PonkoColor`)
pub struct Color {
    /// Red, 0 to 1.
    pub r: f32,
    /// Green, 0 to 1.
    pub g: f32,
    /// Blue, 0 to 1.
    pub b: f32,
    /// Opacity: 0 transparent, 1 opaque.
    pub a: f32,
}

impl Color {
    /// White.
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    /// Black.
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    /// Middle gray.
    pub const GRAY: Color = Color::rgb(0.5, 0.5, 0.5);
    /// Red.
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    /// Green.
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    /// Blue.
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    /// Yellow.
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    /// Fully transparent (draws nothing).
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    /// A soft dark blue-gray, the default clear color.
    pub const PONKO_DARK: Color = Color::rgb(0.11, 0.12, 0.15);

    /// An opaque color from red, green and blue (each 0.0 to 1.0).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let teal = Color::rgb(0.0, 0.5, 0.5);
    /// ```
    #[must_use]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    /// A color with transparency (`a`: 0.0 invisible, 1.0 opaque).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let shadow = Color::rgba(0.0, 0.0, 0.0, 0.3);
    /// ```
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// An opaque color from a `0xRRGGBB` hex number.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let crab_orange = Color::hex(0xE8743B);
    /// ```
    #[must_use]
    pub const fn hex(rgb: u32) -> Color {
        Color::rgb(
            ((rgb >> 16) & 0xFF) as f32 / 255.0,
            ((rgb >> 8) & 0xFF) as f32 / 255.0,
            (rgb & 0xFF) as f32 / 255.0,
        )
    }

    /// A color from a `0xRRGGBBAA` hex number: like [`hex`](Self::hex), with
    /// the alpha in the last byte (`0x00` transparent, `0xFF` opaque).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let half_orange = Color::hex_rgba(0xFF880080);
    /// assert_eq!(half_orange, Color::hex(0xFF8800).with_alpha(128.0 / 255.0));
    /// ```
    #[must_use]
    pub const fn hex_rgba(rgba: u32) -> Color {
        Color::rgba(
            ((rgba >> 24) & 0xFF) as f32 / 255.0,
            ((rgba >> 16) & 0xFF) as f32 / 255.0,
            ((rgba >> 8) & 0xFF) as f32 / 255.0,
            (rgba & 0xFF) as f32 / 255.0,
        )
    }

    /// The same color with a different alpha.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let faded = Color::RED.with_alpha(0.25);
    /// ```
    #[must_use]
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// Mixes `self` toward `other`: `t` of 0 gives `self`, 1 gives `other`,
    /// 0.5 the middle, in all four channels (alpha too).
    ///
    /// The mix is done on the sRGB values the type holds (what a paint
    /// program shows), not in linear light, so a mix of black and white at
    /// 0.5 is the middle gray `Color::GRAY`. `t` is clamped to 0 to 1: a
    /// value outside it gives the nearer end, never a color beyond the ends.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// // A health bar going from green to red as health falls.
    /// let health = 0.25;
    /// let bar = Color::RED.lerp(Color::GREEN, health);
    /// assert_eq!(bar, Color::rgb(0.75, 0.25, 0.0));
    /// assert_eq!(Color::BLACK.lerp(Color::WHITE, 0.5), Color::GRAY);
    /// ```
    #[must_use]
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a * (1.0 - t) + b * t;
        Color::rgba(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
            mix(self.a, other.a),
        )
    }

    /// Packs into 4 bytes (sRGB), the format sprites are sent to the GPU in.
    pub(crate) fn to_rgba8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// Linear-light values, which is what an sRGB render target expects.
    #[cfg(feature = "window")]
    pub(crate) fn to_linear_wgpu(self) -> wgpu::Color {
        fn lin(c: f32) -> f64 {
            let c = f64::from(c.clamp(0.0, 1.0));
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        wgpu::Color {
            r: lin(self.r),
            g: lin(self.g),
            b: lin(self.b),
            a: f64::from(self.a.clamp(0.0, 1.0)),
        }
    }
}

/// Reads a color from hex text: `#rgb`, `#rrggbb` or `#rrggbbaa` (the `#`
/// is optional, upper or lower case). The short form doubles each digit, so
/// `#f80` is `#ff8800`. No color names and no `rgb(...)`.
///
/// A bad string is an [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid)
/// [`Error`](crate::error::Error) whose message names the problem; parsing never
/// panics.
///
/// ```
/// # use ponko::prelude::*;
/// let orange: Color = "#ff8800".parse().unwrap();
/// assert_eq!(orange, Color::hex(0xFF8800));
/// assert_eq!("f80".parse::<Color>().unwrap(), orange);
/// assert_eq!("#ff8800CC".parse::<Color>().unwrap(), Color::hex_rgba(0xFF8800CC));
///
/// let err = "#gg0000".parse::<Color>().unwrap_err();
/// assert!(err.message().contains("'g'"));
/// ```
impl std::str::FromStr for Color {
    type Err = crate::error::Error;

    fn from_str(text: &str) -> Result<Color, crate::error::Error> {
        const HELP: &str = "expected #rgb, #rrggbb or #rrggbbaa";
        let invalid = |message: String| crate::error::Error::new(crate::error::ErrorKind::Invalid, message);
        let digits = text.strip_prefix('#').unwrap_or(text);
        let mut values = Vec::with_capacity(8);
        for (index, ch) in digits.chars().enumerate() {
            match ch.to_digit(16) {
                Some(v) => values.push(v),
                None => {
                    return Err(invalid(format!(
                        "{text:?} is not a color: {ch:?} (character {} after the #) is not a hex digit; {HELP}",
                        index + 1
                    )));
                }
            }
        }
        let byte = |hi: u32, lo: u32| ((hi << 4) | lo) as f32 / 255.0;
        match values[..] {
            [r, g, b] => Ok(Color::rgb(byte(r, r), byte(g, g), byte(b, b))),
            [r1, r0, g1, g0, b1, b0] => Ok(Color::rgb(byte(r1, r0), byte(g1, g0), byte(b1, b0))),
            [r1, r0, g1, g0, b1, b0, a1, a0] => Ok(Color::rgba(byte(r1, r0), byte(g1, g0), byte(b1, b0), byte(a1, a0))),
            _ => Err(invalid(format!(
                "{text:?} is not a color: it has {} hex digits; {HELP}",
                values.len()
            ))),
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_channels() {
        assert_eq!(Color::hex(0xFF8000).to_rgba8(), [255, 128, 0, 255]);
    }

    #[test]
    fn rgba8_clamps() {
        assert_eq!(Color::rgba(2.0, -1.0, 0.5, 1.0).to_rgba8(), [255, 0, 128, 255]);
    }

    #[test]
    #[cfg(feature = "window")]
    fn linear_conversion_endpoints_and_midpoint() {
        let c = Color::rgb(0.0, 1.0, 0.5).to_linear_wgpu();
        assert_eq!(c.r, 0.0);
        assert!((c.g - 1.0).abs() < 1e-9);
        assert!((c.b - 0.214).abs() < 0.001);
    }

    #[test]
    fn lerp_at_zero_and_one_gives_the_ends_exactly() {
        let a = Color::rgba(0.1, 0.35, 0.9, 0.2);
        let b = Color::rgba(0.7, 0.05, 0.3, 1.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn lerp_of_black_and_white_at_half_is_mid_gray() {
        assert_eq!(Color::BLACK.lerp(Color::WHITE, 0.5), Color::GRAY);
    }

    #[test]
    fn lerp_mixes_alpha_too() {
        let mixed = Color::WHITE.lerp(Color::TRANSPARENT, 0.25);
        assert_eq!(mixed.a, 0.75);
        assert_eq!((mixed.r, mixed.g, mixed.b), (0.75, 0.75, 0.75));
    }

    #[test]
    fn lerp_outside_zero_to_one_gives_the_nearer_end() {
        let (a, b) = (Color::RED, Color::BLUE);
        assert_eq!(a.lerp(b, -3.0), a);
        assert_eq!(a.lerp(b, 2.5), b);
        assert_eq!(a.lerp(b, f32::INFINITY), b);
    }

    #[test]
    fn hex_rgba_carries_alpha() {
        assert_eq!(Color::hex_rgba(0xFF880080).to_rgba8(), [255, 136, 0, 128]);
        assert_eq!(Color::hex_rgba(0x11223344).to_rgba8(), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(Color::hex_rgba(0xFF8800FF), Color::hex(0xFF8800));
    }

    #[test]
    fn text_parses_to_the_same_color_as_the_number() {
        assert_eq!("#ff8800".parse::<Color>().unwrap(), Color::hex(0xFF8800));
        assert_eq!("#FF8800".parse::<Color>().unwrap(), Color::hex(0xFF8800));
        assert_eq!("ff8800".parse::<Color>().unwrap(), Color::hex(0xFF8800));
        assert_eq!("#ff880080".parse::<Color>().unwrap(), Color::hex_rgba(0xFF880080));
        assert_eq!("#ff880080".parse::<Color>().unwrap().a, 128.0 / 255.0);
    }

    #[test]
    fn short_text_doubles_each_digit() {
        assert_eq!("#f80".parse::<Color>().unwrap(), "#ff8800".parse::<Color>().unwrap());
        assert_eq!("#F80".parse::<Color>().unwrap(), Color::hex(0xFF8800));
        assert_eq!("000".parse::<Color>().unwrap(), Color::BLACK);
        assert_eq!("#fff".parse::<Color>().unwrap(), Color::WHITE);
    }

    #[test]
    fn bad_text_is_an_error_that_names_the_problem() {
        for text in ["", "#", "#12", "#12345", "#1234567", "#123456789"] {
            let err = text.parse::<Color>().unwrap_err();
            assert_eq!(err.kind(), crate::error::ErrorKind::Invalid, "{text:?}");
            assert!(err.message().contains("hex digits"), "{text:?}: {err}");
            assert!(err.message().contains("#rrggbbaa"), "{text:?}: {err}");
        }
        let err = "#gg0000".parse::<Color>().unwrap_err();
        assert!(
            err.message().contains("'g'") && err.message().contains("not a hex digit"),
            "{err}"
        );
        assert!(err.message().contains("#gg0000"), "{err}");
    }

    #[test]
    fn text_with_multibyte_characters_is_an_error_not_a_panic() {
        for text in ["#é", "#ééé", "#ff88é0", "🦀", "#ff8800🦀", "日本語"] {
            let err = text.parse::<Color>().unwrap_err();
            assert_eq!(err.kind(), crate::error::ErrorKind::Invalid, "{text:?}");
        }
        let err = "#ff88é0".parse::<Color>().unwrap_err();
        assert!(
            err.message().contains("'é'") && err.message().contains("character 5"),
            "{err}"
        );
    }
}
